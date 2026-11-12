//! Record state planes: sixteen bit columns over the record pages (`docs/lanes/core/COLUMN-PLAN.md`).
//!
//! Plane `f`, record at (page, slot) -> bit `slot` of `StatePage::bits[f]` of that page. A page with no bit
//! set in `bits` or `seen` has no state (`Page::state == 0`), and no operation on it comes here.
//! `seen` is the plane as its consumer last acknowledged it: `bits ^ seen` is exactly the records to
//! visit. Both words move with the records, so a record that moves has not flipped.

use crate::page::CAP;

pub const STATE_PLANES: usize = 16;
/// Pointer-transparent (the kind byte's `KF_NOHIT`).
pub const S_NOHIT: u8 = 0;
/// A dispatch target (`KF_INTERACT`).
pub const S_INTERACT: u8 = 1;
/// A tab stop (`KF_FOCUS`).
pub const S_FOCUS: u8 = 2;
/// A dormant run: one record standing for a run of unrealised items.
pub const S_DORMANT: u8 = 3;

#[repr(C, align(64))]
#[derive(Clone, Copy)]
pub struct StatePage {
    pub bits: [u64; STATE_PLANES],
    pub seen: [u64; STATE_PLANES],
    /// Bit `f`: `bits[f] != 0`.
    pub any: u16,
    /// Bit `f`: `bits[f] != seen[f]`.
    pub changed: u16,
    /// Bit `f`: this page's id is on plane `f`'s flip list (kept across a free: the entry is still there).
    pub listed: u16,
}

/// The low `n` bits (`n <= 64`).
#[inline(always)]
pub fn low(n: usize) -> u64 {
    if n >= 64 { !0 } else { (1u64 << n) - 1 }
}

impl StatePage {
    pub const BLANK: StatePage = StatePage { bits: [0; STATE_PLANES], seen: [0; STATE_PLANES], any: 0, changed: 0, listed: 0 };

    /// Re-derive the two summaries from the words (after any write to them).
    #[inline]
    pub fn summarise(&mut self) {
        let (mut any, mut ch) = (0u16, 0u16);
        for f in 0..STATE_PLANES {
            any |= ((self.bits[f] != 0) as u16) << f;
            ch |= ((self.bits[f] != self.seen[f]) as u16) << f;
        }
        (self.any, self.changed) = (any, ch);
    }
    /// No bit anywhere: the page needs no state page.
    #[inline]
    pub fn is_blank(&self) -> bool {
        self.any == 0 && self.changed == 0
    }
    /// `Page::shift`: slots `[src, src + n)` are copied to `[dst, dst + n)` in every word (the source
    /// slots keep their bits, as the record columns keep their bytes, until they are overwritten or cleared).
    #[inline]
    pub fn shift(&mut self, src: usize, dst: usize, n: usize) {
        debug_assert!(n > 0 && src + n <= CAP && dst + n <= CAP);
        let m = low(n);
        for w in self.bits.iter_mut().chain(self.seen.iter_mut()) {
            *w = (*w & !(m << dst)) | (((*w >> src) & m) << dst);
        }
    }
    /// `Page::copy_from`: slots `[from, from + n)` of `src` are copied to `[at, at + n)`.
    #[inline]
    pub fn copy(&mut self, at: usize, src: &StatePage, from: usize, n: usize) {
        debug_assert!(n > 0 && from + n <= CAP && at + n <= CAP);
        let m = low(n);
        for f in 0..STATE_PLANES {
            self.bits[f] = (self.bits[f] & !(m << at)) | (((src.bits[f] >> from) & m) << at);
            self.seen[f] = (self.seen[f] & !(m << at)) | (((src.seen[f] >> from) & m) << at);
        }
    }
    /// Slots `[lo, hi)` hold no state (new records, padding).
    #[inline]
    pub fn clear(&mut self, lo: usize, hi: usize) {
        if lo >= hi {
            return;
        }
        let keep = !(low(hi - lo) << lo);
        for w in self.bits.iter_mut().chain(self.seen.iter_mut()) {
            *w &= keep;
        }
    }
    /// A record's sixteen bits and sixteen seen bits (a move through a staging buffer carries these).
    #[inline]
    pub fn get(&self, slot: usize) -> (u16, u16) {
        let (mut b, mut s) = (0u16, 0u16);
        for f in 0..STATE_PLANES {
            b |= ((self.bits[f] >> slot & 1) as u16) << f;
            s |= ((self.seen[f] >> slot & 1) as u16) << f;
        }
        (b, s)
    }
    #[inline]
    pub fn put(&mut self, slot: usize, v: (u16, u16)) {
        for f in 0..STATE_PLANES {
            self.bits[f] = (self.bits[f] & !(1 << slot)) | (((v.0 >> f & 1) as u64) << slot);
            self.seen[f] = (self.seen[f] & !(1 << slot)) | (((v.1 >> f & 1) as u64) << slot);
        }
    }
}

/// State pages by page id: chunks of 64, created on first use and never moved or returned.
#[derive(Default)]
pub struct StatePool {
    /// Chunk `c` holds the state pages of page ids `64c .. 64c + 64`; null until one of them has state.
    chunks: Vec<*mut [StatePage; 64]>,
}

impl Drop for StatePool {
    fn drop(&mut self) {
        for &c in &self.chunks {
            if !c.is_null() {
                // SAFETY: created by Box::into_raw in `ensure`, freed once.
                drop(unsafe { Box::from_raw(c) });
            }
        }
    }
}

impl StatePool {
    /// The state page of page `id`. Precondition (the page's `state` byte is set, or `has(id)`): its
    /// chunk exists.
    #[inline(always)]
    pub fn get(&self, id: u32) -> &StatePage {
        debug_assert!(self.has(id));
        // SAFETY: the precondition: `ensure(id)` ran, so the chunk index is in range and the chunk is live.
        unsafe { &(**self.chunks.get_unchecked(id as usize >> 6))[id as usize & 63] }
    }
    #[inline(always)]
    pub fn get_mut(&mut self, id: u32) -> &mut StatePage {
        debug_assert!(self.has(id));
        // SAFETY: as in `get`; &mut self is exclusive.
        unsafe { &mut (**self.chunks.get_unchecked(id as usize >> 6))[id as usize & 63] }
    }
    #[inline]
    pub fn ensure(&mut self, id: u32) -> &mut StatePage {
        let c = id as usize >> 6;
        if self.chunks.len() <= c || self.chunks[c].is_null() {
            self.grow(c);
        }
        self.get_mut(id)
    }
    #[cold]
    #[inline(never)]
    fn grow(&mut self, c: usize) {
        if self.chunks.len() <= c {
            self.chunks.resize(c + 1, core::ptr::null_mut());
        }
        self.chunks[c] = Box::into_raw(Box::new([StatePage::BLANK; 64]));
    }
    pub fn has(&self, id: u32) -> bool {
        self.chunks.get(id as usize >> 6).is_some_and(|c| !c.is_null())
    }
    pub fn heap_bytes(&self) -> usize {
        self.chunks.iter().filter(|c| !c.is_null()).count() * core::mem::size_of::<[StatePage; 64]>() + self.chunks.capacity() * 8
    }
}
