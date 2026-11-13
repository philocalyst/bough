//! A prepared glyph image (round 8, e2e lane; ROUND8 Amendment 1, reformulation 2: text is staged).
//!
//! What a frame needs from a font is, per glyph, a coverage bitmap and where to put it. For one face at one
//! size both are constants, so they are made when the application is built (`examples/bake.rs`: the real shaper
//! and rasteriser, `swash`, the ones `bough-text` uses) and read in place from rodata. Placing a string is then
//! arithmetic over its bytes: an advance per character, a kerning adjustment per pair, a ligature where the
//! shaper would form one. No font file is opened, nothing is parsed, nothing is allocated, and the first frame
//! pays for exactly the glyphs it shows (their pages of rodata).
//!
//! The image covers a character set (Latin-1 here). [`Image::covers`] says whether a string is inside it; what
//! is outside (other scripts, anything the bake found context-dependent beyond pairs and ligatures) belongs to
//! the real shaper, loaded when first needed and never on the path to the first frame. That lazy path is the
//! text lane's; this crate is the fast path's data and its gate (`tests/exact.rs`: every placement and every
//! coverage byte equal to the shaper's and the rasteriser's on random strings).
#![no_std]

/// One glyph of the image.
#[derive(Clone, Copy, Debug)]
pub struct Glyph {
    /// Advance in font units.
    pub adv: i32,
    /// The bitmap's offset from the pen position on the baseline: left edge, and top edge above the baseline.
    pub left: i32,
    pub top: i32,
    pub w: u32,
    pub h: u32,
    /// `w * h` coverage bytes, rows top to bottom.
    pub cov: &'static [u8],
}

/// A prepared image, in place.
#[derive(Clone, Copy)]
pub struct Image {
    b: &'static [u8],
    /// Pixels per em.
    pub size: f32,
    pub upem: u32,
    /// Font units above and below the baseline, and the line gap.
    pub ascent: i32,
    pub descent: i32,
    pub gap: i32,
    n: usize,
    nlig: usize,
    glyphs: usize,
    kern: usize,
    ligs: usize,
    cov: usize,
}

const MAGIC: u32 = 0x3149_4742; // "BGI1"
pub const HEADER: usize = 40;
const GLYPH: usize = 20;
const LIG: usize = 8;
/// No glyph for this character.
pub const NONE: u8 = 0xff;

const fn rd32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}
const fn rd16(b: &[u8], at: usize) -> i32 {
    i16::from_le_bytes([b[at], b[at + 1]]) as i32
}

impl Image {
    /// Read the header of a baked image. `const`: an application's image is a `static`.
    pub const fn new(b: &'static [u8]) -> Image {
        assert!(rd32(b, 0) == MAGIC, "not a glyph image");
        let n = rd32(b, 28) as usize;
        let nlig = rd32(b, 32) as usize;
        let glyphs = HEADER + 256;
        let kern = glyphs + n * GLYPH;
        let ligs = kern + n * n * 2;
        let cov = ligs + nlig * LIG;
        assert!(cov + rd32(b, 36) as usize == b.len(), "truncated glyph image");
        Image { b, size: f32::from_bits(rd32(b, 4)), upem: rd32(b, 8), ascent: rd32(b, 12) as i32, descent: rd32(b, 16) as i32, gap: rd32(b, 20) as i32, n, nlig, glyphs, kern, ligs, cov }
    }
    /// Glyphs in the image (characters, then ligatures).
    pub const fn len(&self) -> usize {
        self.n
    }
    pub const fn is_empty(&self) -> bool {
        self.n == 0
    }
    /// The glyph of a Latin-1 code point, or `NONE`.
    #[inline(always)]
    pub fn index(&self, c: u32) -> u8 {
        if c < 256 { self.b[HEADER + c as usize] } else { NONE }
    }
    #[inline(always)]
    pub fn glyph(&self, i: u8) -> Glyph {
        let at = self.glyphs + i as usize * GLYPH;
        let b = self.b;
        let (w, h) = (rd16(b, at + 8) as u32, rd16(b, at + 10) as u32);
        let off = self.cov + rd32(b, at + 12) as usize;
        Glyph { adv: rd32(b, at) as i32, left: rd16(b, at + 4), top: rd16(b, at + 6), w, h, cov: &b[off..off + (w * h) as usize] }
    }
    /// The face's own id of glyph `i` (for the gate against the shaper).
    pub fn gid(&self, i: u8) -> u16 {
        rd16(self.b, self.glyphs + i as usize * GLYPH + 16) as u16
    }
    /// The adjustment to the advance of glyph `a` when glyph `b` follows it, in font units.
    #[inline(always)]
    pub fn kern(&self, a: u8, b: u8) -> i32 {
        rd16(self.b, self.kern + (a as usize * self.n + b as usize) * 2)
    }
    /// The ligature that starts at `s[0]`, if the shaper forms one: (glyph, bytes consumed).
    #[inline]
    fn lig(&self, s: &[u8]) -> Option<(u8, usize)> {
        for k in 0..self.nlig {
            let at = self.ligs + k * LIG;
            let n = self.b[at] as usize;
            if s.len() >= n && s[..n] == self.b[at + 1..at + 1 + n] {
                return Some((self.b[at + 5], n));
            }
        }
        None
    }
    /// Font units to pixels.
    #[inline(always)]
    pub fn px(&self, units: i32) -> f32 {
        units as f32 * self.size / self.upem as f32
    }

    /// Whether every character of `s` (UTF-8) is in the image: if not, the string is the real shaper's.
    pub fn covers(&self, s: &[u8]) -> bool {
        let mut it = Chars(s);
        while let Some(c) = it.next() {
            if self.index(c) == NONE {
                return false;
            }
        }
        true
    }

    /// Place `s` (UTF-8): `f(glyph, x)` for each glyph in order, `x` its pen position in font units from the
    /// start of the run. Returns the run's advance in font units. A character outside the image is skipped
    /// (callers that care ask [`Image::covers`] first).
    #[inline]
    pub fn place(&self, s: &[u8], mut f: impl FnMut(u8, i32)) -> i32 {
        // Latin-1 as UTF-8 is at most two bytes a character; ligatures are matched on the decoded bytes, which
        // for the ASCII sequences that ligate are the bytes themselves.
        let (mut pen, mut prev, mut i) = (0i32, NONE, 0usize);
        while i < s.len() {
            let (g, n) = match if self.nlig != 0 { self.lig(&s[i..]) } else { None } {
                Some(l) => l,
                None => {
                    let (c, n) = decode(&s[i..]);
                    (self.index(c), n)
                }
            };
            i += n;
            if g == NONE {
                continue;
            }
            if prev != NONE {
                pen += self.kern(prev, g);
            }
            f(g, pen);
            pen += rd32(self.b, self.glyphs + g as usize * GLYPH) as i32;
            prev = g;
        }
        pen
    }
    /// The advance of `s` in font units.
    #[inline]
    pub fn advance(&self, s: &[u8]) -> i32 {
        self.place(s, |_, _| {})
    }
    /// The pixel column of a pen position: where the shaper's fractional position lands on the grid.
    #[inline(always)]
    pub fn snap(&self, units: i32) -> i32 {
        // round half up, in integers: floor(units * size / upem + 0.5) with size in 1/64 px
        let s64 = (self.size * 64.0) as i64;
        let d = self.upem as i64 * 64;
        ((units as i64 * s64 * 2 + d).div_euclid(2 * d)) as i32
    }
}

/// One code point from the front of `s` and its length in bytes (malformed input: one byte, U+FFFD).
#[inline(always)]
pub fn decode(s: &[u8]) -> (u32, usize) {
    let b = s[0] as u32;
    if b < 0x80 {
        (b, 1)
    } else if b & 0xe0 == 0xc0 && s.len() >= 2 {
        ((b & 0x1f) << 6 | (s[1] as u32 & 0x3f), 2)
    } else if b & 0xf0 == 0xe0 && s.len() >= 3 {
        ((b & 0x0f) << 12 | (s[1] as u32 & 0x3f) << 6 | (s[2] as u32 & 0x3f), 3)
    } else if b & 0xf8 == 0xf0 && s.len() >= 4 {
        ((b & 0x07) << 18 | (s[1] as u32 & 0x3f) << 12 | (s[2] as u32 & 0x3f) << 6 | (s[3] as u32 & 0x3f), 4)
    } else {
        (0xfffd, 1)
    }
}
struct Chars<'a>(&'a [u8]);
impl Chars<'_> {
    fn next(&mut self) -> Option<u32> {
        if self.0.is_empty() {
            return None;
        }
        let (c, n) = decode(self.0);
        self.0 = &self.0[n..];
        Some(c)
    }
}

/// DejaVu Sans 2.37 at 14 px, Latin-1 (the face and size of gui-bench's spec). Provenance: `data/README.md`.
pub static DEJAVU_SANS_14: Image = Image::new(include_bytes!("../data/dejavu-sans-14.bgi"));
/// The same face at 28 px: the 14 px text of a window at scale 2.
pub static DEJAVU_SANS_28: Image = Image::new(include_bytes!("../data/dejavu-sans-28.bgi"));
