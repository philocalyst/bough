//! The image of one face at one size, and the two things done with it: shaping and covering.
//!
//! Layout (little-endian; every table at a fixed offset computed from the header):
//!
//! ```text
//!   0  magic "BFC1"            4  size in 1/64 px          8  scale: px per font unit, Q32 (u64)
//!  16  upem                   20  ascent (font units)     24  descent (font units, positive)
//!  28  line gap               32  step rows (u16), step columns (u16)
//!  36  ligatures              40  digit advance, point advance, minus advance, largest digit-pair kern (u16 x 4;
//!                                 digit advance 0xffff: the digits are not a cell)
//!  48  coverage bytes         52  reserved (12)
//!  64  row class of each token (256 bytes)          320  column class of each token (256 bytes)
//! 576  slots: 256 x { coverage offset u32, left i8, top i8, w u8, h u8 }
//! 2624 which ASCII bytes start a ligature (16 bytes, one bit each)
//! 2640 ligatures: n x { first, rest length, rest[4], token, 0 }
//!      steps: rows x columns x u16 (0xffff: this pair needs the full shaper)
//!      coverage
//! ```
//!
//! A token is an ASCII byte or a Latin-1 code point as itself, [`TOK_PAD`] for the end of a run, and a value
//! below 0x20 for a ligature. `steps[row[t]][col[u]]` is the advance of `t` followed by `u`, kerning included.

pub const MAGIC: u32 = 0x3143_4642; // "BFC1"
/// The token after the last glyph of a run.
pub const TOK_PAD: u8 = 0;
const HEADER: usize = 64;
const ROWS: usize = HEADER;
const COLS: usize = ROWS + 256;
const SLOTS: usize = COLS + 256;
const LIG_FIRST: usize = SLOTS + 256 * 8;
const LIGS: usize = LIG_FIRST + 16;
const REFUSE: u32 = 0xffff;

const fn rd16(b: &[u8], at: usize) -> u32 {
    u16::from_le_bytes([b[at], b[at + 1]]) as u32
}
const fn rd32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// A glyph of the face: where its coverage is and how it sits on the pen (`left` from the pen, `top` from the
/// baseline, both in pixels; `top` is negative above the baseline).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    pub off: u32,
    pub left: i32,
    pub top: i32,
    pub w: u32,
    pub h: u32,
}

/// The face's digit cell in font units (see [`crate::num`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub adv: i32,
    pub dot: i32,
    pub minus: i32,
    /// The largest pair kern among digits, point and minus that a proportional setting would apply and a
    /// tabular one does not.
    pub kern: i32,
}

/// A prepared face, in place.
#[derive(Clone, Copy)]
pub struct Face {
    b: &'static [u8],
    scale: u64,
    cols: usize,
    nlig: usize,
    steps: usize,
    cov: usize,
}

impl Face {
    /// Read the header of an image. `const`: an application's face is a `static`, and opening it costs
    /// nothing at run time.
    pub const fn new(b: &'static [u8]) -> Face {
        assert!(b.len() >= LIGS && rd32(b, 0) == MAGIC, "not a face image");
        let (rows, cols, nlig) = (rd16(b, 32) as usize, rd16(b, 34) as usize, rd32(b, 36) as usize);
        let steps = LIGS + nlig * 8;
        let cov = steps + rows * cols * 2;
        assert!(cov + rd32(b, 48) as usize == b.len(), "truncated face image");
        let scale = (rd32(b, 8) as u64) | (rd32(b, 12) as u64) << 32;
        Face { b, scale, cols, nlig, steps, cov }
    }
    /// The size in pixels per em.
    pub const fn size(&self) -> f32 {
        rd32(self.b, 4) as f32 / 64.0
    }
    pub const fn upem(&self) -> i32 {
        rd32(self.b, 16) as i32
    }
    /// Font units above and below the baseline.
    pub const fn ascent(&self) -> i32 {
        rd32(self.b, 20) as i32
    }
    pub const fn descent(&self) -> i32 {
        rd32(self.b, 24) as i32
    }
    /// The bytes of the image.
    pub const fn bytes(&self) -> &'static [u8] {
        self.b
    }
    /// The digit cell, if the face's ten digits share an advance.
    pub const fn cell(&self) -> Option<Cell> {
        let adv = rd16(self.b, 40);
        if adv == 0xffff {
            return None;
        }
        Some(Cell { adv: adv as i32, dot: rd16(self.b, 42) as i32, minus: rd16(self.b, 44) as i32, kern: rd16(self.b, 46) as i32 })
    }

    /// A pen in font units as whole pixels: round half up, in integers. The one rounding rule.
    #[inline(always)]
    pub const fn px(&self, units: i32) -> i32 {
        ((units as i64 * self.scale as i64 + (1 << 31)) >> 32) as i32
    }
    /// The baseline of a line set in a box `h` pixels high, from the box's top: the content area (ascent plus
    /// descent) centred, as CSS half-leading does.
    #[inline(always)]
    pub const fn baseline(&self, h: i32) -> i32 {
        let (a, d) = (self.px(self.ascent()), self.px(self.descent()));
        (h - (a + d)).div_euclid(2) + a
    }

    #[inline(always)]
    pub fn slot(&self, tok: u16) -> Slot {
        let at = SLOTS + (tok as usize & 255) * 8;
        let b = self.b;
        Slot { off: rd32(b, at), left: b[at + 4] as i8 as i32, top: b[at + 5] as i8 as i32, w: b[at + 6] as u32, h: b[at + 7] as u32 }
    }
    /// A glyph's coverage: `w * h` bytes, rows top to bottom, linear, 255 is covered.
    #[inline(always)]
    pub fn coverage(&self, s: Slot) -> &'static [u8] {
        let at = self.cov + s.off as usize;
        &self.b[at..at + (s.w * s.h) as usize]
    }
    #[inline(always)]
    fn step(&self, t: u8, u: u8) -> u32 {
        let (r, c) = (self.b[ROWS + t as usize] as usize, self.b[COLS + u as usize] as usize);
        rd16(self.b, self.steps + (r * self.cols + c) * 2)
    }

    /// The token at the front of `s` and the bytes it takes, or `None` for a code point outside the face
    /// (beyond Latin-1, or a control).
    #[inline(always)]
    fn token(&self, s: &[u8]) -> Option<(u8, usize)> {
        let b = s[0];
        if b < 0x80 {
            if b < 0x20 || b == 0x7f {
                return None;
            }
            if self.nlig != 0 && self.b[LIG_FIRST + (b as usize >> 3)] >> (b & 7) & 1 != 0 {
                // the face's ligatures in the order the shaper tries them
                for k in 0..self.nlig {
                    let at = LIGS + k * 8;
                    let n = self.b[at + 1] as usize;
                    if self.b[at] == b && s.len() > n && s[1..1 + n] == self.b[at + 2..at + 2 + n] {
                        return Some((self.b[at + 6], 1 + n));
                    }
                }
            }
            Some((b, 1))
        } else if b == 0xc3 && s.len() >= 2 {
            Some((s[1].wrapping_add(0x40), 2))
        } else if b == 0xc2 && s.len() >= 2 && s[1] >= 0xa0 {
            Some((s[1], 2))
        } else {
            None
        }
    }

    /// Shape `s` (UTF-8): `f(token, pen, byte)` for each glyph in order, `pen` in font units from the start of
    /// the run, `byte` the offset of the glyph's first byte. Returns the run's advance, or `None` if the face
    /// does not cover the string (some glyphs may have been handed to `f` by then: a caller that keeps them
    /// discards them).
    #[inline]
    pub fn shape(&self, s: &[u8], mut f: impl FnMut(u8, i32, u32)) -> Option<i32> {
        let mut pen = 0i32;
        let mut i = 0usize;
        if s.is_empty() {
            return Some(0);
        }
        let (mut t, mut n) = self.token(s)?;
        loop {
            let at = i;
            i += n;
            let (u, m) = if i < s.len() { self.token(&s[i..])? } else { (TOK_PAD, 0) };
            let st = self.step(t, u);
            if st == REFUSE {
                return None;
            }
            f(t, pen, at as u32);
            pen += st as i32;
            if m == 0 {
                return Some(pen);
            }
            (t, n) = (u, m);
        }
    }
    /// The advance of `s` in font units, or `None` if the face does not cover it.
    #[inline]
    pub fn advance(&self, s: &[u8]) -> Option<i32> {
        self.shape(s, |_, _, _| {})
    }

    /// The coverage of a shaped line, row by row, cut to `clip` (`[x0, y0, x1, y1]`, half open): for glyph
    /// slots `slot` at pens `x` (font units), the line starting at pen `x0` and drawn from the pixel origin
    /// `org` (the pen's start, the baseline). `f(x, y, coverage)` is given each run of bytes that survives the
    /// clip, the first of them at pixel `(x, y)`.
    #[inline]
    pub fn spans(&self, slot: &[u16], x: &[i32], x0: i32, org: (i32, i32), clip: [i32; 4], mut f: impl FnMut(i32, i32, &[u8])) {
        for (&g, &pen) in slot.iter().zip(x) {
            self.glyph_spans(g, org.0 + self.px(pen - x0), org.1, clip, &mut f);
        }
    }
    /// One glyph with its pen at pixel `(px, baseline)`.
    #[inline(always)]
    pub fn glyph_spans(&self, tok: u16, px: i32, baseline: i32, clip: [i32; 4], f: &mut impl FnMut(i32, i32, &[u8])) {
        let s = self.slot(tok);
        if s.w == 0 {
            return;
        }
        let (gx, gy, w, h) = (px + s.left, baseline + s.top, s.w as i32, s.h as i32);
        let (cx0, cx1) = (if gx > clip[0] { gx } else { clip[0] }, if gx + w < clip[2] { gx + w } else { clip[2] });
        let (cy0, cy1) = (if gy > clip[1] { gy } else { clip[1] }, if gy + h < clip[3] { gy + h } else { clip[3] });
        if cx0 >= cx1 || cy0 >= cy1 {
            return;
        }
        let cov = self.coverage(s);
        let mut y = cy0;
        while y < cy1 {
            let row = ((y - gy) * w) as usize;
            f(cx0, y, &cov[row + (cx0 - gx) as usize..row + (cx1 - gx) as usize]);
            y += 1;
        }
    }
}
