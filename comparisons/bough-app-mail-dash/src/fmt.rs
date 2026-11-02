//! Text the tables show, as values: a fixed 16-byte string (`Str16`, `Copy`), so a cell's signal holds the
//! *displayed* text and its equality cut-off is a compare of what the label shows (a price that moved by less
//! than a cent patches nothing). And zgx's `ui.number` (`zgx-script/src/data.rs` `number`), ported verbatim.
use std::fmt::Write;

/// Up to 15 bytes of UTF-8, inline.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub struct Str16 {
    len: u8,
    b: [u8; 15],
}

impl Str16 {
    pub const EMPTY: Str16 = Str16 { len: 0, b: [0; 15] };
    pub fn new(s: &str) -> Str16 {
        let mut o = Str16::EMPTY;
        let n = s.len().min(15);
        o.b[..n].copy_from_slice(&s.as_bytes()[..n]);
        o.len = n as u8;
        o
    }
    pub fn as_str(&self) -> &str {
        // SAFETY: built only from whole `&str`s cut at 15 bytes, which callers keep ASCII or whole chars
        // (every caller's text is ASCII but the arrows, written whole).
        unsafe { std::str::from_utf8_unchecked(&self.b[..self.len as usize]) }
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl std::fmt::Debug for Str16 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.as_str())
    }
}

impl Write for Str16 {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        let n = self.len as usize;
        if n + s.len() > 15 {
            return Err(std::fmt::Error);
        }
        self.b[n..n + s.len()].copy_from_slice(s.as_bytes());
        self.len += s.len() as u8;
        Ok(())
    }
}

/// A label operand: the string of a `Str16` signal.
pub fn s16(s: &Str16) -> String {
    s.as_str().to_owned()
}

/// `ui.number`'s options (`Intl.NumberFormat`'s).
#[derive(Clone, Copy, Default)]
pub struct NumberOptions {
    pub decimals: Option<usize>,
    pub percent: bool,
    pub compact: bool,
    /// `sign = "except_zero"`.
    pub except_zero: bool,
    pub currency: Option<&'static str>,
}

/// zgx's `number(n, o)` (grouping on, as its default), written into a `Str16`.
pub fn number(n: f64, o: &NumberOptions) -> Str16 {
    let mut out = Str16::EMPTY;
    if !n.is_finite() {
        let _ = out.write_str(if n.is_nan() { "NaN" } else if n > 0. { "\u{221e}" } else { "-\u{221e}" });
        return out;
    }
    let mut value = if o.percent { n * 100. } else { n };
    let mut suffix = "";
    if o.compact {
        for (scale, unit) in [(1e12, "T"), (1e9, "B"), (1e6, "M"), (1e3, "K")] {
            if value.abs() >= scale {
                value /= scale;
                suffix = unit;
                break;
            }
        }
    }
    let decimals = o.decimals.unwrap_or(if o.compact {
        usize::from(value.abs() < 100. && !suffix.is_empty())
    } else if value.fract() == 0. {
        0
    } else {
        2
    });
    let mut digits = Str16::EMPTY;
    if write!(digits, "{:.*}", decimals, value.abs()).is_err() {
        // past 15 bytes: as zgx does, through a String (never for the apps' values)
        let s = format!("{:.*}", decimals, value.abs());
        let _ = out.write_str(&s[..s.len().min(15)]);
        return out;
    }
    let mut d = digits.as_str();
    if o.compact && o.decimals.is_none() && d.ends_with(".0") {
        d = &d[..d.len() - 2];
    }
    let zero = d.bytes().all(|b| b == b'0' || b == b'.');
    let negative = value < 0. && !zero;
    let (whole, fraction) = match d.split_once('.') {
        Some((w, f)) => (w, Some(f)),
        None => (d, None),
    };
    let sign = if negative {
        "-"
    } else if o.except_zero && !zero {
        "+"
    } else {
        ""
    };
    let _ = out.write_str(sign);
    if let Some(c) = o.currency {
        let _ = out.write_str(c);
    }
    let first = whole.len() % 3;
    for (i, c) in whole.char_indices() {
        if i > 0 && (i + 3 - first) % 3 == 0 {
            let _ = out.write_char(',');
        }
        let _ = out.write_char(c);
    }
    if let Some(f) = fraction {
        let _ = out.write_char('.');
        let _ = out.write_str(f);
    }
    let _ = out.write_str(suffix);
    if o.percent {
        let _ = out.write_char('%');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    fn f(n: f64, set: impl FnOnce(&mut NumberOptions)) -> String {
        let mut o = NumberOptions::default();
        set(&mut o);
        number(n, &o).as_str().to_owned()
    }
    /// zgx's own test (`numbers_format_as_intl_would`), the cases its options reach.
    #[test]
    fn numbers_format_as_zgx_does() {
        assert_eq!(f(1234.5, |_| {}), "1,234.50");
        assert_eq!(f(0.0123, |o| o.percent = true), "1.23%");
        assert_eq!(
            f(0.0123, |o| {
                o.percent = true;
                o.except_zero = true;
                o.decimals = Some(2);
            }),
            "+1.23%"
        );
        assert_eq!(f(0., |o| o.except_zero = true), "0");
        assert_eq!(f(1500., |o| o.compact = true), "1.5K");
        assert_eq!(f(1000., |o| o.compact = true), "1K");
        assert_eq!(f(123_456., |o| o.compact = true), "123K");
        assert_eq!(f(-2_500_000_000., |o| o.compact = true), "-2.5B");
        assert_eq!(f(950., |o| o.compact = true), "950");
        assert_eq!(f(-1234.5, |o| o.currency = Some("$")), "-$1,234.50");
        assert_eq!(f(12.3456, |o| o.decimals = Some(2)), "12.35");
        assert_eq!(f(63.2e12, |o| { o.compact = true; o.currency = Some("$") }), "$63.2T");
    }
}
