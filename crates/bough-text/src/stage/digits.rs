//! The font side of `bough_face::num`: whether a face's digits are a cell, and the reference formatting the
//! arithmetic is held to.
//!
//! `text(sig!(self.total).num(FMT)).tabular()` needs no shaping, no glyph lanes and no layout when its box
//! does not depend on it: the k-th glyph and its pen are integer functions of the value
//! ([`bough_face::num::Num`], the same source for every executor). What that requires of the face is checked
//! here once: the ten digits share an advance. Whether the face kerns pairs among digits, point and minus is
//! reported, not required: tabular is a definition (the digits sit on the cell), and where a face kerns them
//! (Arial: "11") a tabular number differs from the shaper's proportional setting of the same string.
use crate::script::{SimpleScript, TRIG};
pub use bough_face::num::{Num, ndigits};
pub use bough_face::Cell;

/// Why a face's digits cannot be placed by arithmetic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoCell {
    /// Digits `.0` and `.1` have different advances: the face needs its `tnum` feature, or a synthesised
    /// cell of the widest digit.
    Proportional(u8, u8),
    /// The pair `.0`, `.1` is outside the table shaper (a contextual rule): the face needs the full shaper.
    Contextual(u8, u8),
}

/// The digit cell of a face, in font units.
pub fn cell_of(sc: &SimpleScript) -> Result<Cell, NoCell> {
    let adv = sc.adv[b'0' as usize];
    for d in b'1'..=b'9' {
        if sc.adv[d as usize] != adv {
            return Err(NoCell::Proportional(b'0', d));
        }
    }
    let set = b"0123456789.-";
    let mut kern = 0i32;
    for &a in set {
        for &b in set {
            let k = sc.matrix[sc.class1[a as usize] as usize * sc.n2 + sc.class2[b as usize] as usize];
            if k == TRIG {
                return Err(NoCell::Contextual(a, b));
            }
            kern = kern.max((k as i32).abs());
        }
    }
    Ok(Cell { adv: adv as i32, dot: sc.adv[b'.' as usize] as i32, minus: sc.adv[b'-' as usize] as i32, kern })
}

/// `v` (an integer scaled by `10^d`) with `d` decimals and at least `pad` digits, as text: the reference.
pub fn format(v: i32, d: u32, pad: u32, out: &mut String) {
    out.clear();
    let m = v.unsigned_abs();
    if v < 0 {
        out.push('-');
    }
    use core::fmt::Write;
    let p = 10u32.pow(d);
    let int_w = (pad.max(d + 1) - d) as usize;
    let _ = write!(out, "{:0w$}", m / p, w = int_w);
    if d > 0 {
        let _ = write!(out, ".{:0w$}", m % p, w = d as usize);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn glyphs_are_the_formatted_string() {
        let mut s = String::new();
        let mut r = 12345u32;
        for i in 0..200_000 {
            r = r.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let v = match i % 4 {
                0 => (r >> 8) as i32 % 100_000,
                1 => -((r >> 4) as i32 & 0xfff_ffff),
                2 => r as i32,
                _ => (r % 2_000) as i32 - 1_000,
            };
            for (d, pad) in [(0, 0), (1, 0), (2, 0), (3, 0), (4, 0), (0, 5), (2, 6), (0, 9)] {
                format(v, d, pad, &mut s);
                let num = Num::new(v, d, pad);
                assert_eq!(num.count() as usize, s.len(), "{v} {d} {pad} {s}");
                for k in 0..num.count() {
                    assert_eq!(num.glyph(k), s.as_bytes()[(num.count() - 1 - k) as usize], "{v} {d} {pad} {s} k={k}");
                }
            }
        }
    }
}
