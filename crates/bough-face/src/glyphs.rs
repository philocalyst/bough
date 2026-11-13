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

/// `a == b` for a key: `a` is an entry's bytes (zero-padded to a word), `b` the text asked for. Whole words, the
/// last one overlapping: no call.
///
/// # Safety
/// `a` points at `b.len()` readable bytes.
#[inline(always)]
unsafe fn same_key(a: *const u8, b: &[u8]) -> bool {
    let len = b.len();
    // SAFETY: every load is of 8 bytes inside `[0, len)` when `len >= 8`, and of single bytes inside it
    // otherwise; `a` is readable for `len` bytes by the caller's contract and `b` is a slice.
    unsafe {
        if len >= 8 {
            let ld = |p: *const u8, i: usize| (p.add(i) as *const u64).read_unaligned();
            let (mut i, mut d) = (0, 0u64);
            while i + 8 <= len {
                d |= ld(a, i) ^ ld(b.as_ptr(), i);
                i += 8;
            }
            d |= ld(a, len - 8) ^ ld(b.as_ptr(), len - 8);
            d == 0
        } else {
            let (mut i, mut d) = (0, 0u8);
            while i < len {
                d |= *a.add(i) ^ *b.as_ptr().add(i);
                i += 1;
            }
            d == 0
        }
    }
}

/// A shaped line: `n` glyphs from `g0` in the lanes, shaped in face `face`, made in generation `gen` of the
/// memo. Two words; zero is the empty run. A run from before a [`Glyphs::clear`] names lanes that no longer
/// hold it: its generation says so, and [`Glyphs::of`] gives it no glyphs.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Run {
    pub g0: u32,
    /// Glyphs in the low 16 bits, the face in the next 8, the generation in the top 8.
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
        (self.n_face >> 16 & 0xff) as usize
    }
    #[inline(always)]
    pub const fn generation(&self) -> u8 {
        (self.n_face >> 24) as u8
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
    /// Times the memo was cleared.
    pub cleared: u64,
    /// Bumped by [`Glyphs::clear`]: runs carry it.
    generation: u8,
}

impl Default for Glyphs {
    fn default() -> Self {
        Glyphs::new()
    }
}

impl Glyphs {
    pub fn new() -> Glyphs {
        Glyphs { table: vec![EMPTY; 64], used: 0, ents: vec![0; 8], slot: Vec::new(), x: Vec::new(), shaped: 0, found: 0, cleared: 0, generation: 0 }
    }
    /// The lanes: every run indexes these.
    #[inline(always)]
    pub fn lanes(&self) -> (&[u16], &[i32]) {
        (&self.slot, &self.x)
    }
    /// A run's glyph slots and pens. A run of another generation (made before the last [`Glyphs::clear`])
    /// has none: it draws nothing rather than someone else's glyphs.
    #[inline(always)]
    pub fn of(&self, r: Run) -> (&[u16], &[i32]) {
        let (a, b) = (r.g0 as usize, r.g0 as usize + r.len());
        if r.generation() != self.generation || b > self.slot.len() {
            return (&[], &[]);
        }
        (&self.slot[a..b], &self.x[a..b])
    }
    /// Bytes held, capacity included: the table, the entries, the lanes.
    pub fn bytes(&self) -> usize {
        self.table.capacity() * 64 + self.ents.capacity() * 8 + self.slot.capacity() * 2 + self.x.capacity() * 4
    }
    /// The generation runs are being made in.
    pub fn generation(&self) -> u8 {
        self.generation
    }
    pub fn len(&self) -> usize {
        self.used
    }
    pub fn is_empty(&self) -> bool {
        self.used == 0
    }
    /// Forget every line and give the memory back. Runs handed out before are dead (their generation is no
    /// longer the memo's): every holder describes its text again, which is what the caller of
    /// [`Glyphs::over`] does next.
    pub fn clear(&mut self) {
        self.table.clear();
        self.table.resize(64, EMPTY);
        self.table.shrink_to(64);
        self.used = 0;
        self.ents.truncate(8);
        self.ents.shrink_to(1 << 10);
        self.slot.clear();
        self.slot.shrink_to(1 << 12);
        self.x.clear();
        self.x.shrink_to(1 << 12);
        self.generation = self.generation.wrapping_add(1);
        self.cleared += 1;
    }

    #[inline(always)]
    fn find(&self, h: u64, head: u64, text: &[u8]) -> usize {
        let tag = (h >> 32) as u32 | 1;
        let mask = self.table.len() - 1;
        let mut i = h as usize & mask;
        loop {
            // SAFETY: `i` is masked to the table's length, a power of two.
            let b = unsafe { self.table.get_unchecked(i) };
            let mut m = 0u32;
            for k in 0..8 {
                m |= ((b.tag[k] == tag) as u32) << k;
            }
            while m != 0 {
                let off = b.off[m.trailing_zeros() as usize] as usize;
                m &= m - 1;
                // SAFETY: an offset in the table is the start of an entry inside `ents` (`enter` and `grow` put
                // it there), so its header words are in bounds; its key bytes follow the header and are as
                // long as `head` says, which is compared before they are read.
                unsafe {
                    if *self.ents.get_unchecked(off) == h && *self.ents.get_unchecked(off + 1) == head && same_key(self.ents.as_ptr().add(off + HDR) as *const u8, text) {
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
        self.line_with(h, id, text.as_bytes(), |slot, x| {
            face.shape(text.as_bytes(), |t, pen, _| {
                slot.push(t as u16);
                x.push(pen);
            })
        })
    }
    /// The line of (`id`, `text`) if it has been shaped: a hit and nothing else.
    #[inline(always)]
    pub fn get(&self, h: u64, id: u16, text: &[u8]) -> Option<(Run, i32)> {
        let off = self.find(h, id as u64 | (text.len() as u64) << 32, text);
        if off == 0 {
            return None;
        }
        // SAFETY: `find` returns the start of an entry: its four header words are in bounds.
        let (g, w) = unsafe { (*self.ents.get_unchecked(off + 2), *self.ents.get_unchecked(off + 3)) };
        Some((Run { g0: g as u32, n_face: (g >> 32) as u32 | (id as u32 & 0xff) << 16 | (self.generation as u32) << 24 }, w as u32 as i32))
    }
    /// The line of (`id`, `text`), shaped on a miss by `shape`: any shaper. It appends the line's glyph slots
    /// and pens to the lanes it is given and returns the advance, or `None` to refuse (what it appended is
    /// discarded). This is the one memo: the prepared face, the table shaper and the full shaper differ only
    /// in the closure.
    #[inline]
    pub fn line_with(&mut self, h: u64, id: u16, text: &[u8], shape: impl FnOnce(&mut Vec<u16>, &mut Vec<i32>) -> Option<i32>) -> Option<(Run, i32)> {
        if let Some(hit) = self.get(h, id, text) {
            self.found += 1;
            return Some(hit);
        }
        let g0 = self.slot.len();
        let Some(width) = shape(&mut self.slot, &mut self.x) else {
            self.slot.truncate(g0);
            self.x.truncate(g0);
            return None;
        };
        assert!(self.slot.len() == self.x.len(), "a shaper appends one pen per glyph slot");
        Some(self.enter(h, id, text, g0, width))
    }
    /// Whether what the memo holds has outgrown `budget` bytes: time to [`Glyphs::clear`] and describe what
    /// is exposed again. Counts what [`Glyphs::bytes`] counts.
    #[inline(always)]
    pub fn over(&self, budget: usize) -> bool {
        self.bytes() > budget
    }
    #[inline(never)]
    fn enter(&mut self, h: u64, id: u16, text: &[u8], g0: usize, width: i32) -> (Run, i32) {
        let head = id as u64 | (text.len() as u64) << 32;
        let n = self.slot.len() - g0;
        assert!(n < 1 << 16 && g0 < u32::MAX as usize && id < 256, "a line of more than 65,535 glyphs, or a face beyond 255");
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
        (Run { g0: g0 as u32, n_face: n as u32 | (id as u32 & 0xff) << 16 | (self.generation as u32) << 24 }, width)
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
