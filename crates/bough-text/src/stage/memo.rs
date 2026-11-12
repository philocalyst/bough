//! Shaping memoised by (font, text), laid out for the memory hierarchy.
//!
//! * The table is open addressing over 64-byte buckets of eight (tag, offset) slots: one line decides a
//!   probe, and a second bucket is touched only when the first is full.
//! * An entry is a header of six words followed by the key's bytes, placed so that a short entry (a label of
//!   up to 16 bytes) does not straddle a line: the second dependent line holds the hash, the length, the key
//!   bytes to compare and the glyph and break ranges.
//! * Glyphs and breaks are ranges of shared lanes in font units (size-independent: one entry serves every
//!   size of an unhinted face). Nothing holds a pointer, so the whole memo is a position-independent image
//!   ([`Image`]): written by a build step, mapped or `include_bytes!`-ed, read in place.
use super::brk::{self, Brk, Extent};
use crate::shape::{GlyphRun, Shaper};

const MUL: u128 = 0x9E37_79B9_7F4A_7C15_F39C_C060_5CED_C835;

#[inline(always)]
const fn mix(x: u64) -> u64 {
    let m = (x as u128).wrapping_mul(MUL);
    (m as u64) ^ (m >> 64) as u64
}

/// The key of (font, text). A `const fn`: the key of a constant string is a constant. Eight bytes per step,
/// each step one load and one widening multiply.
#[inline(always)]
pub const fn key(font: u32, s: &[u8]) -> u64 {
    let mut h = font as u64 ^ ((s.len() as u64) << 32) ^ 0x243F_6A88_85A3_08D3;
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

/// `a == b` for a key: `a` is the entry's bytes (zero-padded to a word, so readable to the next word
/// boundary), `b` the text asked for, both `len` long. Whole words, the last one overlapping: no call.
#[inline(always)]
fn same_key(a: *const u8, b: &[u8]) -> bool {
    let len = b.len();
    // SAFETY: `a` points at `len` readable bytes (the caller's contract) and `b` is a slice of `len` bytes;
    // every load below is of 8 bytes inside `[0, len)` when `len >= 8`, and byte loads inside `[0, len)`
    // otherwise.
    unsafe {
        if len >= 8 {
            let ld = |p: *const u8, i: usize| (p.add(i) as *const u64).read_unaligned();
            let mut i = 0;
            let mut d = 0u64;
            while i + 8 <= len {
                d |= ld(a, i) ^ ld(b.as_ptr(), i);
                i += 8;
            }
            d |= ld(a, len - 8) ^ ld(b.as_ptr(), len - 8);
            d == 0
        } else {
            let mut d = 0u8;
            let mut i = 0;
            while i < len {
                d |= *a.add(i) ^ *b.as_ptr().add(i);
                i += 1;
            }
            d == 0
        }
    }
}

#[repr(C, align(64))]
#[derive(Clone, Copy)]
pub struct Bucket {
    tag: [u32; 8],
    off: [u32; 8],
}
const EMPTY: Bucket = Bucket { tag: [0; 8], off: [0; 8] };

/// Words of an entry's header: hash; font | len << 32; g0 | gn << 32; b0 | bn << 32; width | min << 32;
/// max | words << 32 (bit 63: the text has a hard break). The key's bytes follow, zero-padded to a word.
const HDR: usize = 6;

/// `bn` of an entry whose break table has not been asked for (a label never asks).
pub const NO_BREAKS: u32 = u32::MAX;

/// A memo entry, read out of its header.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ent {
    pub g0: u32,
    pub gn: u32,
    pub b0: u32,
    pub bn: u32,
    pub width: i32,
    pub ext: Extent,
    pub bytes: u32,
}

/// The read side of a memo: a table and an entry arena, wherever they live.
#[derive(Clone, Copy)]
pub struct View<'a> {
    table: &'a [Bucket],
    ents: &'a [u64],
}

impl<'a> View<'a> {
    /// The entry of (font, text) whose key is `h`: its offset, or 0.
    #[inline(always)]
    pub fn find(&self, h: u64, font: u32, text: &[u8]) -> u32 {
        let tag = (h >> 32) as u32 | 1;
        let mask = self.table.len() - 1;
        let mut i = h as usize & mask;
        let head = font as u64 | (text.len() as u64) << 32;
        loop {
            let b = &self.table[i];
            let mut m = 0u32;
            for k in 0..8 {
                m |= ((b.tag[k] == tag) as u32) << k;
            }
            while m != 0 {
                let off = b.off[m.trailing_zeros() as usize] as usize;
                m &= m - 1;
                // SAFETY: an offset stored in the table is the start of an entry inside `ents` (`Memo::insert`
                // and `Image::new` establish it), so the header words are in bounds.
                let (e0, e1) = unsafe { (*self.ents.get_unchecked(off), *self.ents.get_unchecked(off + 1)) };
                // SAFETY: an entry's key bytes follow its header and are `len` long (`e1 == head` checked the
                // length before the bytes are read; `&&` orders it).
                if (e0 == h) & (e1 == head) && same_key(unsafe { self.ents.as_ptr().add(off + HDR) } as *const u8, text) {
                    return off as u32;
                }
            }
            if b.off[7] == 0 {
                return 0;
            }
            i = (i + 1) & mask;
        }
    }
    #[inline(always)]
    fn key_bytes(&self, off: usize, len: usize) -> &'a [u8] {
        let w = &self.ents[off + HDR..off + HDR + len.div_ceil(8)];
        // SAFETY: `w` holds at least `len` initialised bytes; u8 has alignment 1.
        unsafe { core::slice::from_raw_parts(w.as_ptr() as *const u8, len) }
    }
    #[inline(always)]
    pub fn ent(&self, off: u32) -> Ent {
        let e = &self.ents[off as usize..off as usize + HDR];
        Ent {
            g0: e[2] as u32,
            gn: (e[2] >> 32) as u32,
            b0: e[3] as u32,
            bn: (e[3] >> 32) as u32,
            width: e[4] as u32 as i32,
            ext: Extent { min: (e[4] >> 32) as u32 as i32, max: e[5] as u32 as i32, hard: e[5] >> 63 != 0 },
            bytes: (e[1] >> 32) as u32,
        }
    }
    pub fn text(&self, off: u32) -> &'a [u8] {
        self.key_bytes(off as usize, (self.ents[off as usize + 1] >> 32) as usize)
    }
}

/// The lanes an entry's ranges index.
#[derive(Clone, Copy)]
pub struct Lanes<'a> {
    pub gid: &'a [u16],
    pub x: &'a [i32],
    pub brk: &'a [Brk],
    pub first: &'a [u32],
}

pub struct Memo {
    table: Vec<Bucket>,
    used: usize,
    ents: Vec<u64>,
    pub gid: Vec<u16>,
    pub x: Vec<i32>,
    pub brk: Vec<Brk>,
    pub first: Vec<u32>,
    scratch: GlyphRun,
    pub hits: u64,
    pub misses: u64,
}

impl Default for Memo {
    fn default() -> Self {
        Memo::new(64)
    }
}

impl Memo {
    pub fn new(buckets: usize) -> Memo {
        // offset 0 means "empty", so the arena starts one line in
        Memo { table: vec![EMPTY; buckets.next_power_of_two()], used: 0, ents: vec![0; 8], gid: Vec::new(), x: Vec::new(), brk: Vec::new(), first: Vec::new(), scratch: GlyphRun::default(), hits: 0, misses: 0 }
    }
    #[inline(always)]
    pub fn view(&self) -> View<'_> {
        View { table: &self.table, ents: &self.ents }
    }
    #[inline(always)]
    pub fn lanes(&self) -> Lanes<'_> {
        Lanes { gid: &self.gid, x: &self.x, brk: &self.brk, first: &self.first }
    }
    /// Room for `glyphs` glyphs and `segs` break opportunities without moving the lanes.
    pub fn reserve(&mut self, texts: usize, glyphs: usize, segs: usize) {
        self.gid.reserve(glyphs);
        self.x.reserve(glyphs);
        self.brk.reserve(segs);
        self.first.reserve(segs);
        self.ents.reserve(texts * HDR + glyphs / 8 + texts);
    }
    pub fn len(&self) -> usize {
        self.used
    }
    pub fn is_empty(&self) -> bool {
        self.used == 0
    }
    /// Resident bytes: (table, entries, glyph lanes, break lanes).
    pub fn bytes(&self) -> (usize, usize, usize, usize) {
        (self.table.len() * 64, self.ents.len() * 8, self.gid.len() * 2 + self.x.len() * 4, self.brk.len() * 8 + self.first.len() * 4)
    }

    /// The entry of (font, text), shaping it on a miss. `None`: the table shaper does not cover the text
    /// (the caller queues it for the full shaper).
    #[inline]
    pub fn get(&mut self, font: u32, sh: &Shaper, text: &str) -> Option<u32> {
        let h = key(font, text.as_bytes());
        self.get_keyed(h, font, sh, text)
    }
    /// [`Memo::get`] with the key already known (a constant string's key is a constant).
    #[inline]
    pub fn get_keyed(&mut self, h: u64, font: u32, sh: &Shaper, text: &str) -> Option<u32> {
        let off = self.view().find(h, font, text.as_bytes());
        if off != 0 {
            self.hits += 1;
            return Some(off);
        }
        self.miss(h, font, sh, text)
    }
    // Not `#[cold]`: at first paint and whenever a row enters exposure, the miss is the path.
    #[inline(never)]
    fn miss(&mut self, h: u64, font: u32, sh: &Shaper, text: &str) -> Option<u32> {
        let mut run = core::mem::take(&mut self.scratch);
        let ok = sh.shape_into(text, 1.0, &mut run);
        let off = ok.then(|| self.insert(h, font, text.as_bytes(), &run));
        self.scratch = run;
        self.misses += 1;
        off
    }

    /// Add the entry of a shaped text. Its break table is built when first asked for ([`Memo::breaks`]),
    /// unless the glyphs are not one per byte (then now, while the run's clusters are at hand).
    pub fn insert(&mut self, h: u64, font: u32, text: &[u8], run: &GlyphRun) -> u32 {
        let g0 = self.gid.len() as u32;
        self.gid.extend_from_slice(&run.gid);
        self.x.extend_from_slice(&run.x);
        let lazy = run.identity_clusters && text.len() == run.len();
        let (b0, bn, ext) = if lazy {
            (0, NO_BREAKS, Extent::default())
        } else {
            let b0 = self.brk.len() as u32;
            let ext = brk::build(text, run, 0, 0, &mut self.brk, &mut self.first);
            (b0, self.brk.len() as u32 - b0, ext)
        };
        let words = HDR + text.len().div_ceil(8);
        // a short entry does not straddle a line
        let at = self.ents.len();
        if words <= 8 && (at & 7) + words > 8 {
            self.ents.resize((at + 7) & !7, 0);
        }
        let off = self.ents.len();
        self.ents.extend_from_slice(&[
            h,
            font as u64 | (text.len() as u64) << 32,
            g0 as u64 | (run.gid.len() as u64) << 32,
            b0 as u64 | (bn as u64) << 32,
            run.width as u32 as u64 | (ext.min as u32 as u64) << 32,
            ext.max as u32 as u64 | (words as u64) << 32 | (ext.hard as u64) << 63,
        ]);
        self.ents.resize(off + words, 0);
        // SAFETY: the `words - HDR` words just appended hold at least `text.len()` bytes.
        unsafe { core::ptr::copy_nonoverlapping(text.as_ptr(), self.ents.as_mut_ptr().add(off + HDR) as *mut u8, text.len()) };
        if (self.used + 1) * 4 > self.table.len() * 8 * 3 {
            self.grow();
        }
        Self::place(&mut self.table, h, off as u32);
        self.used += 1;
        off as u32
    }

    /// The entry with its break table, built now if this is the first time it is asked for.
    #[inline]
    pub fn breaks(&mut self, off: u32) -> Ent {
        let e = self.view().ent(off);
        if e.bn != NO_BREAKS {
            return e;
        }
        self.build_breaks(off, e)
    }
    #[inline(never)]
    fn build_breaks(&mut self, off: u32, e: Ent) -> Ent {
        let o = off as usize;
        let b0 = self.brk.len() as u32;
        let len = e.bytes as usize;
        // SAFETY: the entry's key bytes follow its header and are `len` long; `ents` is not touched while
        // they are borrowed (the break lanes are other fields).
        let text = unsafe { core::slice::from_raw_parts(self.ents.as_ptr().add(o + HDR) as *const u8, len) };
        let x = &self.x[e.g0 as usize..(e.g0 + e.gn) as usize];
        let ext = brk::build_bytes(text, x, e.width, 0, 0, &mut self.brk, &mut self.first);
        let bn = self.brk.len() as u32 - b0;
        self.ents[o + 3] = b0 as u64 | (bn as u64) << 32;
        self.ents[o + 4] = e.width as u32 as u64 | (ext.min as u32 as u64) << 32;
        self.ents[o + 5] = (self.ents[o + 5] & 0x7fff_ffff_0000_0000) | ext.max as u32 as u64 | (ext.hard as u64) << 63;
        Ent { b0, bn, ext, ..e }
    }
    /// Build every break table not built yet (before the memo is written out as an image).
    pub fn build_all_breaks(&mut self) {
        let mut p = 8;
        while p < self.ents.len() {
            if self.ents[p] == 0 {
                p += 1;
                continue;
            }
            self.breaks(p as u32);
            p += (self.ents[p + 5] >> 32) as usize & 0x7fff_ffff;
        }
    }
    /// Forget everything, keeping the memory.
    pub fn clear(&mut self) {
        self.table.fill(EMPTY);
        self.used = 0;
        self.ents.truncate(8);
        self.gid.clear();
        self.x.clear();
        self.brk.clear();
        self.first.clear();
    }

    fn place(table: &mut [Bucket], h: u64, off: u32) {
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
            Self::place(&mut t, self.ents[p], p as u32);
            p += (self.ents[p + 5] >> 32) as usize & 0x7fff_ffff;
        }
        self.table = t;
    }

    /// The memo as one position-independent image (see [`Image`]).
    pub fn to_image(&self) -> Vec<u64> {
        let pad = |n: usize| n.div_ceil(8) * 8; // sections start on a line
        let t = self.table.len() * 8;
        let e = pad(self.ents.len());
        let xw = pad(self.x.len().div_ceil(2));
        let gw = pad(self.gid.len().div_ceil(4));
        let bw = pad(self.brk.len());
        let fw = pad(self.first.len().div_ceil(2));
        let mut out = vec![0u64; 8 + t + e + xw + gw + bw + fw];
        out[..8].copy_from_slice(&[IMAGE_MAGIC, self.table.len() as u64, self.ents.len() as u64, self.x.len() as u64, self.gid.len() as u64, self.brk.len() as u64, self.first.len() as u64, 0]);
        // SAFETY: each destination range lies inside `out` (sized above from the same lengths), sources are
        // plain data of the copied byte length, and the ranges do not overlap the sources.
        unsafe {
            let base = out.as_mut_ptr().add(8) as *mut u8;
            let mut at = 0usize;
            let mut put = |src: *const u8, bytes: usize, words: usize| {
                core::ptr::copy_nonoverlapping(src, base.add(at), bytes);
                at += words * 8;
            };
            put(self.table.as_ptr() as *const u8, t * 8, t);
            put(self.ents.as_ptr() as *const u8, self.ents.len() * 8, e);
            put(self.x.as_ptr() as *const u8, self.x.len() * 4, xw);
            put(self.gid.as_ptr() as *const u8, self.gid.len() * 2, gw);
            put(self.brk.as_ptr() as *const u8, self.brk.len() * 8, bw);
            put(self.first.as_ptr() as *const u8, self.first.len() * 4, fw);
        }
        out
    }
}

const IMAGE_MAGIC: u64 = 0x626f_7567_685f_7478; // "bough_tx"

/// A memo read in place from bytes that were there before the program started. Opening it is a header
/// check: nothing is parsed, copied or allocated.
#[derive(Clone, Copy)]
pub struct Image<'a> {
    view: View<'a>,
    lanes: Lanes<'a>,
}

impl<'a> Image<'a> {
    /// `None` if `words` is not an image, or is not on a 64-byte boundary.
    pub fn new(words: &'a [u64]) -> Option<Image<'a>> {
        if words.len() < 8 || words[0] != IMAGE_MAGIC || words.as_ptr() as usize % 64 != 0 {
            return None;
        }
        let [nt, ne, nx, ng, nb, nf] = [words[1] as usize, words[2] as usize, words[3] as usize, words[4] as usize, words[5] as usize, words[6] as usize];
        let pad = |n: usize| n.div_ceil(8) * 8;
        let (t, e, xw, gw, bw, fw) = (nt * 8, pad(ne), pad(nx.div_ceil(2)), pad(ng.div_ceil(4)), pad(nb), pad(nf.div_ceil(2)));
        if !nt.is_power_of_two() || words.len() != 8 + t + e + xw + gw + bw + fw {
            return None;
        }
        let body = &words[8..];
        let (tw, rest) = body.split_at(t);
        let (ew, rest) = rest.split_at(e);
        let (xs, rest) = rest.split_at(xw);
        let (gs, rest) = rest.split_at(gw);
        let (bs, fs) = rest.split_at(bw);
        // SAFETY: every section starts on a 64-byte boundary of a `u64` slice (alignment 8 at least, 64 for
        // the table, checked above), holds at least the stated number of elements of plain integer types
        // (lengths checked against the image's size), and lives as long as `words`.
        unsafe {
            Some(Image {
                view: View { table: core::slice::from_raw_parts(tw.as_ptr() as *const Bucket, nt), ents: &ew[..ne] },
                lanes: Lanes {
                    x: core::slice::from_raw_parts(xs.as_ptr() as *const i32, nx),
                    gid: core::slice::from_raw_parts(gs.as_ptr() as *const u16, ng),
                    brk: core::slice::from_raw_parts(bs.as_ptr() as *const Brk, nb),
                    first: core::slice::from_raw_parts(fs.as_ptr() as *const u32, nf),
                },
            })
        }
    }
    #[inline(always)]
    pub fn view(&self) -> View<'a> {
        self.view
    }
    #[inline(always)]
    pub fn lanes(&self) -> Lanes<'a> {
        self.lanes
    }
}
