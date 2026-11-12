//! Shaped lines as ranges of two shared lanes, memoised by (face, text).
//!
//! The lanes are the 6-byte form: per glyph a slot (`u16`: the face's token) and a pen in font units (`i32`).
//! They are position-independent and size-independent, so a line is placed nowhere until it is painted: a
//! record that draws one holds a [`Run`] (two words) and its rect.
//!
//! The memo is what makes a run a stable operand: describing the same text again finds the same range, so a
//! re-description compares equal and damages nothing. Its table is open addressing over 64-byte buckets of
//! eight (tag, offset) slots; an entry is a four-word header with the key bytes behind it, placed so that a
//! short entry does not straddle a line: a hit is two dependent lines.
use crate::face::Face;
use alloc::vec;
use alloc::vec::Vec;

const MUL: u128 = 0x9E37_79B9_7F4A_7C15_F39C_C060_5CED_C835;
#[inline(always)]
const fn mix(x: u64) -> u64 {
    let m = (x as u128).wrapping_mul(MUL);
    (m as u64) ^ (m >> 64) as u64
}
/// The key of (face, text): eight bytes per step. `const`, so the key of a literal is a constant.
#[inline(always)]
pub const fn key(face: u32, s: &[u8]) -> u64 {
    let mut h = face as u64 ^ ((s.len() as u64) << 32) ^ 0x243F_6A88_85A3_08D3;
    let mut rest = s;
    while let Some((w, r)) = rest.split_first_chunk::<8>() {
        h = mix(h ^ u64::from_le_bytes(*w));
        rest = r;
    }
    let mut w = 0u64;
    let mut k = 0;
    while k < rest.len() {
        w |= (rest[k] as u64) << (8 * k);
        k += 1;
    }
    h = mix(h ^ w);
    if h == 0 { 1 } else { h }
}

/// A shaped line: `n` glyphs from `g0` in the lanes, shaped in face `face`. Two words, zero is the empty run.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Run {
    pub g0: u32,
    /// Glyphs in the low 16 bits, the face in the high 16.
    pub n_face: u32,
}
impl Run {
    #[inline(always)]
    pub const fn len(&self) -> usize {
        (self.n_face & 0xffff) as usize
    }
    #[inline(always)]
    pub const fn is_empty(&self) -> bool {
        self.n_face & 0xffff == 0
    }
    #[inline(always)]
    pub const fn face(&self) -> usize {
        (self.n_face >> 16) as usize
    }
}

#[repr(C, align(64))]
#[derive(Clone, Copy)]
struct Bucket {
    tag: [u32; 8],
    off: [u32; 8],
}
const EMPTY: Bucket = Bucket { tag: [0; 8], off: [0; 8] };
/// Header words: hash; face | len << 32; g0 | n << 32; width | words << 32.
const HDR: usize = 4;

pub struct Glyphs {
    table: Vec<Bucket>,
    used: usize,
    ents: Vec<u64>,
    slot: Vec<u16>,
    x: Vec<i32>,
    /// Texts shaped (misses) and found (hits).
    pub shaped: u64,
    pub found: u64,
}

impl Default for Glyphs {
    fn default() -> Self {
        Glyphs::new()
    }
}

impl Glyphs {
    pub fn new() -> Glyphs {
        Glyphs { table: vec![EMPTY; 64], used: 0, ents: vec![0; 8], slot: Vec::new(), x: Vec::new(), shaped: 0, found: 0 }
    }
    /// The lanes: every run indexes these.
    #[inline(always)]
    pub fn lanes(&self) -> (&[u16], &[i32]) {
        (&self.slot, &self.x)
    }
    /// A run's glyph slots and pens.
    #[inline(always)]
    pub fn of(&self, r: Run) -> (&[u16], &[i32]) {
        let (a, b) = (r.g0 as usize, r.g0 as usize + r.len());
        (&self.slot[a..b], &self.x[a..b])
    }
    /// Resident bytes.
    pub fn bytes(&self) -> usize {
        self.table.len() * 64 + self.ents.len() * 8 + self.slot.len() * 2 + self.x.len() * 4
    }
    /// Forget every line, keeping the memory. Runs handed out before are dead: the caller re-describes.
    pub fn clear(&mut self) {
        self.table.fill(EMPTY);
        self.used = 0;
        self.ents.truncate(8);
        self.slot.clear();
        self.x.clear();
    }

    #[inline(always)]
    fn find(&self, h: u64, head: u64, text: &[u8]) -> usize {
        let tag = (h >> 32) as u32 | 1;
        let mask = self.table.len() - 1;
        let mut i = h as usize & mask;
        loop {
            let b = &self.table[i];
            let mut m = 0u32;
            for k in 0..8 {
                m |= ((b.tag[k] == tag) as u32) << k;
            }
            while m != 0 {
                let off = b.off[m.trailing_zeros() as usize] as usize;
                m &= m - 1;
                if self.ents[off] == h && self.ents[off + 1] == head {
                    let w = &self.ents[off + HDR..off + HDR + text.len().div_ceil(8)];
                    // SAFETY: `w` holds at least `text.len()` initialised bytes; `u8` has alignment 1.
                    if unsafe { core::slice::from_raw_parts(w.as_ptr() as *const u8, text.len()) } == text {
                        return off;
                    }
                }
            }
            if b.off[7] == 0 {
                return 0;
            }
            i = (i + 1) & mask;
        }
    }

    /// The line of `text` in `face` (number `id` in the application's table of faces), and its advance in font
    /// units; shaped the first time it is asked for. `None`: the face does not cover the text.
    #[inline]
    pub fn line(&mut self, id: u16, face: &Face, text: &str) -> Option<(Run, i32)> {
        let h = key(id as u32, text.as_bytes());
        self.line_keyed(h, id, face, text)
    }
    /// [`Glyphs::line`] with the key known ([`key`] of a literal is a constant).
    #[inline]
    pub fn line_keyed(&mut self, h: u64, id: u16, face: &Face, text: &str) -> Option<(Run, i32)> {
        let head = id as u64 | (text.len() as u64) << 32;
        let off = self.find(h, head, text.as_bytes());
        if off != 0 {
            self.found += 1;
            let (g, w) = (self.ents[off + 2], self.ents[off + 3]);
            return Some((Run { g0: g as u32, n_face: (g >> 32) as u32 | (id as u32) << 16 }, w as u32 as i32));
        }
        self.miss(h, head, id, face, text.as_bytes())
    }
    #[inline(never)]
    fn miss(&mut self, h: u64, head: u64, id: u16, face: &Face, text: &[u8]) -> Option<(Run, i32)> {
        let g0 = self.slot.len();
        let (slot, x) = (&mut self.slot, &mut self.x);
        let Some(width) = face.shape(text, |t, pen, _| {
            slot.push(t as u16);
            x.push(pen);
        }) else {
            self.slot.truncate(g0);
            self.x.truncate(g0);
            return None;
        };
        let n = self.slot.len() - g0;
        assert!(n < 1 << 16 && g0 < u32::MAX as usize, "a line of more than 65,535 glyphs is not a line");
        let words = HDR + text.len().div_ceil(8);
        // a short entry does not straddle a line
        let at = self.ents.len();
        if words <= 8 && (at & 7) + words > 8 {
            self.ents.resize((at + 7) & !7, 0);
        }
        let off = self.ents.len();
        self.ents.extend_from_slice(&[h, head, g0 as u64 | (n as u64) << 32, width as u32 as u64 | (words as u64) << 32]);
        self.ents.resize(off + words, 0);
        // SAFETY: the `words - HDR` words just appended hold at least `text.len()` bytes.
        unsafe { core::ptr::copy_nonoverlapping(text.as_ptr(), self.ents.as_mut_ptr().add(off + HDR) as *mut u8, text.len()) };
        if (self.used + 1) * 4 > self.table.len() * 8 * 3 {
            self.grow();
        }
        Self::put(&mut self.table, h, off as u32);
        self.used += 1;
        self.shaped += 1;
        Some((Run { g0: g0 as u32, n_face: n as u32 | (id as u32) << 16 }, width))
    }
    fn put(table: &mut [Bucket], h: u64, off: u32) {
        let mask = table.len() - 1;
        let mut i = h as usize & mask;
        loop {
            let b = &mut table[i];
            if let Some(k) = b.off.iter().position(|&o| o == 0) {
                b.tag[k] = (h >> 32) as u32 | 1;
                b.off[k] = off;
                return;
            }
            i = (i + 1) & mask;
        }
    }
    #[cold]
    fn grow(&mut self) {
        let mut t = vec![EMPTY; self.table.len() * 2];
        let mut p = 8;
        while p < self.ents.len() {
            if self.ents[p] == 0 {
                p += 1; // alignment gap
                continue;
            }
            Self::put(&mut t, self.ents[p], p as u32);
            p += (self.ents[p + 3] >> 32) as usize;
        }
        self.table = t;
    }
}
