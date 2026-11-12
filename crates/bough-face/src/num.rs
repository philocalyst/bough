//! A number as a glyph source: the k-th glyph of a formatted number, and its pen, are integer functions of
//! the value.
//!
//! A tabular number in a box whose size does not depend on it needs no shaping, no glyph lanes and no layout:
//! its record holds (or reads from the block) an integer scaled by `10^d`. *Tabular is a definition*: the
//! digits sit on the face's cell and no pair among digits, point and minus is kerned, whatever the face's
//! kerning table says ([`Cell::kern`] reports what a proportional setting would have applied).
use crate::face::{Cell, Face};

const POW10: [u32; 10] = [1, 10, 100, 1_000, 10_000, 100_000, 1_000_000, 10_000_000, 100_000_000, 1_000_000_000];

/// Decimal digits of `m` (at least 1).
#[inline(always)]
pub fn ndigits(m: u32) -> u32 {
    let mut n = 1;
    let mut i = 1;
    while i < 10 {
        n += (m >= POW10[i]) as u32;
        i += 1;
    }
    n
}

/// A number ready to be drawn: what is computed once per record, so that each glyph is a few operations.
#[derive(Clone, Copy)]
pub struct Num {
    m: u32,
    d: u32,
    drawn: u32,
    point: u32,
    neg: u32,
}

impl Num {
    /// `v` is an integer scaled by `10^d`, drawn with `d` decimals and at least `pad` digits in all (zeros
    /// on the left: `{:05}` is `pad = 5`).
    #[inline(always)]
    pub fn new(v: i32, d: u32, pad: u32) -> Num {
        let m = v.unsigned_abs();
        let n = ndigits(m);
        let least = if pad > d + 1 { pad } else { d + 1 };
        Num { m, d, drawn: if n > least { n } else { least }, point: (d > 0) as u32, neg: (v < 0) as u32 }
    }
    /// Glyphs: digits, the point, the minus.
    #[inline(always)]
    pub fn count(&self) -> u32 {
        self.drawn + self.point + self.neg
    }
    /// The `k`-th glyph from the right, as its ASCII byte (which is its token). `k < count()`.
    #[inline(always)]
    pub fn glyph(&self, k: u32) -> u8 {
        // digit positions skip the point
        let q = k - ((k > self.d) as u32 & self.point);
        if k == self.drawn + self.point {
            b'-'
        } else if (k == self.d) & (self.point != 0) {
            b'.'
        } else {
            b'0' + ((self.m / POW10[if q < 9 { q as usize } else { 9 }]) % 10) as u8
        }
    }
    /// The width in font units.
    #[inline(always)]
    pub fn width(&self, c: &Cell) -> i32 {
        self.drawn as i32 * c.adv + self.point as i32 * c.dot + self.neg as i32 * c.minus
    }
    /// The pen of the `k`-th glyph from the right, in font units from the left edge of the number.
    #[inline(always)]
    pub fn pen(&self, c: &Cell, k: u32) -> i32 {
        // advances of glyphs k, k-1, .. 0
        let digits = if (self.point == 0) | (k < self.d) { k + 1 } else { k };
        let digits = if digits < self.drawn { digits } else { self.drawn };
        let dot = ((k >= self.d) as u32 & self.point) as i32;
        let minus = (k == self.drawn + self.point) as i32;
        self.width(c) - digits as i32 * c.adv - dot * c.dot - minus * c.minus
    }
    /// The number's coverage, as [`Face::spans`] gives a line's: its left edge at pixel `org.0`, or with
    /// `end` its right edge there; the baseline at `org.1`.
    #[inline]
    pub fn spans(&self, face: &Face, c: &Cell, org: (i32, i32), end: bool, clip: [i32; 4], mut f: impl FnMut(i32, i32, &[u8])) {
        let left = if end { org.0 - face.px(self.width(c)) } else { org.0 };
        let mut k = 0;
        while k < self.count() {
            face.glyph_spans(self.glyph(k) as u16, left + face.px(self.pen(c, k)), org.1, clip, &mut f);
            k += 1;
        }
    }
}
