//! A number as a glyph source: the k-th glyph of a formatted number is arithmetic on its value.
//!
//! `text(sig!(self.total).num(FMT)).tabular()` in a box whose size does not depend on it needs no shaping, no
//! glyph column and no layout: the record holds (or reads from the block) an integer scaled by `10^D`, and
//! the glyph at each position, and its pen, are pure integer functions of that integer. These are the leaf
//! functions (`x_ndigits`, `x_digit`, `x_pen` in the raster lane's naming); the same source serves both
//! executors.
//!
//! What this requires of the font is checked once per face by [`Cell::of`]: the ten digits share one advance
//! and no pair among the digits, the decimal point and the minus sign kerns. Then the shaper's output for the
//! formatted string is exactly this arithmetic (held by `examples/stage_bench.rs`, T3).
use crate::script::{SimpleScript, TRIG};

const POW10: [u32; 10] = [1, 10, 100, 1_000, 10_000, 100_000, 1_000_000, 10_000_000, 100_000_000, 1_000_000_000];

/// A face's digit cell, in font units.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub adv: i32,
    pub dot: i32,
    pub minus: i32,
    /// The largest pair kern (absolute, font units) among digits, point and minus. Not zero: the face kerns
    /// some of these pairs ("11" in Arial), and a tabular number, which by definition sits on the cell,
    /// differs from the shaper's proportional setting of the same string by at most this per pair.
    pub kern: i32,
}

/// Why a face's digits cannot be placed by arithmetic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoCell {
    /// Digits `.0` and `.1` have different advances: the face needs its `tnum` feature, or a synthesised
    /// cell of the widest digit.
    Proportional(u8, u8),
    /// The pair `.0`, `.1` is outside the table shaper (a contextual rule): the face needs the full shaper.
    Contextual(u8, u8),
}

impl Cell {
    pub fn of(sc: &SimpleScript) -> Result<Cell, NoCell> {
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
}

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

/// Digits drawn for `v` with `d` decimals: at least one before the point.
#[inline(always)]
fn drawn(v: i32, d: u32) -> u32 {
    let n = ndigits(v.unsigned_abs());
    if n > d { n } else { d + 1 }
}

/// Glyphs of `v` (an integer scaled by `10^d`) with `d` decimals.
#[inline(always)]
pub fn count(v: i32, d: u32) -> u32 {
    drawn(v, d) + (d > 0) as u32 + (v < 0) as u32
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
    /// `v` is an integer scaled by `10^d`.
    #[inline(always)]
    pub fn new(v: i32, d: u32) -> Num {
        Num { m: v.unsigned_abs(), d, drawn: drawn(v, d), point: (d > 0) as u32, neg: (v < 0) as u32 }
    }
    #[inline(always)]
    pub fn count(&self) -> u32 {
        self.drawn + self.point + self.neg
    }
    /// The `k`-th glyph from the right, as its ASCII byte. `k < count()`.
    #[inline(always)]
    pub fn glyph(&self, k: u32) -> u8 {
        // digit positions skip the point
        let q = k - ((k > self.d) as u32 & self.point);
        if k == self.drawn + self.point {
            b'-'
        } else if (k == self.d) & (self.point != 0) {
            b'.'
        } else {
            b'0' + ((self.m / POW10[(q as usize).min(9)]) % 10) as u8
        }
    }
    #[inline(always)]
    pub fn width(&self, c: &Cell) -> i32 {
        self.drawn as i32 * c.adv + self.point as i32 * c.dot + self.neg as i32 * c.minus
    }
    /// The pen of the `k`-th glyph from the right, in font units from the left edge of the number.
    #[inline(always)]
    pub fn pen(&self, c: &Cell, k: u32) -> i32 {
        // advances of glyphs k, k-1, .. 0
        let digits = if (self.point == 0) | (k < self.d) { k + 1 } else { k };
        let digits = digits.min(self.drawn);
        let dot = ((k >= self.d) as u32 & self.point) as i32;
        let minus = (k == self.drawn + self.point) as i32;
        self.width(c) - digits as i32 * c.adv - dot * c.dot - minus * c.minus
    }
}

/// The `k`-th glyph from the right of `v` with `d` decimals.
#[inline(always)]
pub fn glyph(v: i32, d: u32, k: u32) -> u8 {
    Num::new(v, d).glyph(k)
}
/// The pen of the `k`-th glyph from the right.
#[inline(always)]
pub fn pen(c: &Cell, v: i32, d: u32, k: u32) -> i32 {
    Num::new(v, d).pen(c, k)
}
/// The width of the number in font units.
#[inline(always)]
pub fn width(c: &Cell, v: i32, d: u32) -> i32 {
    Num::new(v, d).width(c)
}

/// `v` with `d` decimals as text: the reference the arithmetic is held to.
pub fn format(v: i32, d: u32, out: &mut String) {
    out.clear();
    let m = v.unsigned_abs();
    if v < 0 {
        out.push('-');
    }
    use core::fmt::Write;
    let p = POW10[d as usize];
    let _ = write!(out, "{}", m / p);
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
            for d in 0..5 {
                format(v, d, &mut s);
                let n = count(v, d);
                assert_eq!(n as usize, s.len(), "{v} {d} {s}");
                for k in 0..n {
                    assert_eq!(glyph(v, d, k), s.as_bytes()[(n - 1 - k) as usize], "{v} {d} {s} k={k}");
                }
            }
        }
    }
}
