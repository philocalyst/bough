//! Invention B of `RESULTS.md`: the value never becomes a `String`.
//!
//! A numeric label (`"{:.1} ms"`, `"{:.2}%"`, `"{}"`) changes because a number changed. The
//! usual path formats it through `core::fmt` into a heap `String` and only then reaches the
//! text layer, which diffs the bytes. Here the format is fixed once ([`NumFmt`]) and the value
//! is rendered straight into the zero-padded 16-byte vector that the pool's patch compares,
//! with integer arithmetic only:
//!
//! * `{:.D}` of an `f32`: `v as f64 * 10^D` is exact (24 + 7 bits < 53), and Rust rounds the
//!   exact decimal value half to even, so `round_ties_even` of that product is the digit
//!   string. Verified against `format!` exhaustively over 30 binades of `f32` (tests/numeric.rs).
//! * `{}` of a `u32`: plain digits.

/// `prefix` + number + `suffix`, at most 16 bytes rendered.
#[derive(Clone, Copy, Debug)]
pub struct NumFmt {
    prefix: [u8; 8],
    plen: u8,
    suffix: [u8; 8],
    slen: u8,
    /// Decimals of an `f32` (`{:.D}`), or `None` for a `u32` (`{}`).
    pub decimals: Option<u8>,
}

const POW10: [f64; 7] = [1.0, 10.0, 100.0, 1e3, 1e4, 1e5, 1e6];

impl NumFmt {
    pub fn new(prefix: &str, decimals: Option<u8>, suffix: &str) -> Self {
        assert!(prefix.len() <= 8 && suffix.len() <= 8 && prefix.is_ascii() && suffix.is_ascii());
        assert!(decimals.is_none_or(|d| d <= 6));
        let mut p = [0u8; 8];
        p[..prefix.len()].copy_from_slice(prefix.as_bytes());
        let mut s = [0u8; 8];
        s[..suffix.len()].copy_from_slice(suffix.as_bytes());
        NumFmt { prefix: p, plen: prefix.len() as u8, suffix: s, slen: suffix.len() as u8, decimals }
    }

    /// Render `v` (an `f32` for `{:.D}`; for `{}` its value is taken as `u32`) into `out`
    /// (zero padded). Returns the length, or `None` if it would not fit 16 bytes or is not
    /// finite (the caller formats the slow way).
    #[inline]
    pub fn render_f32(&self, v: f32, out: &mut [u8; 16]) -> Option<usize> {
        let d = self.decimals? as usize;
        if !v.is_finite() {
            return None;
        }
        let neg = v.is_sign_negative();
        let q = (v.abs() as f64 * POW10[d]).round_ties_even();
        if q >= 1e15 {
            return None;
        }
        self.render_digits(neg, q as u64, d, out)
    }

    #[inline]
    pub fn render_u32(&self, v: u32, out: &mut [u8; 16]) -> Option<usize> {
        self.render_digits(false, v as u64, 0, out)
    }

    /// Build the text right to left in one `u128` (byte 0 = first character): prepending a
    /// byte is a shift by 8 and an OR; the register *is* the zero-padded 16-byte result.
    #[inline(always)]
    fn render_digits(&self, neg: bool, mut q: u64, d: usize, out: &mut [u8; 16]) -> Option<usize> {
        let (pl, sl) = (self.plen as usize, self.slen as usize);
        let mut t: u128 = u64::from_le_bytes(self.suffix) as u128;
        let mut len = sl;
        for _ in 0..d {
            t = (t << 8) | (b'0' + (q % 10) as u8) as u128;
            q /= 10;
        }
        if d > 0 {
            t = (t << 8) | b'.' as u128;
            len += d + 1;
        }
        loop {
            t = (t << 8) | (b'0' + (q % 10) as u8) as u128;
            q /= 10;
            len += 1;
            if q == 0 {
                break;
            }
        }
        if neg {
            t = (t << 8) | b'-' as u128;
            len += 1;
        }
        len += pl;
        if len > 16 {
            return None;
        }
        if pl > 0 {
            t = (t << (8 * pl)) | u64::from_le_bytes(self.prefix) as u128;
        }
        *out = t.to_le_bytes();
        Some(len)
    }
}

/// The rendered bytes as a `&str` (they are ASCII by construction).
#[inline]
pub fn as_str(buf: &[u8; 16], len: usize) -> &str {
    // SAFETY: `NumFmt` writes only ASCII (prefix/suffix are checked ASCII, digits, '-', '.').
    unsafe { core::str::from_utf8_unchecked(&buf[..len]) }
}
