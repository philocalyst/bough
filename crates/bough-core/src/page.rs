//! 64-slot SoA pages and the type-stable page pool.

//! # Unsafe
//! The only `unsafe` here is NEON loads/stores through raw pointers derived from the page's own
//! fixed-size arrays (or a caller slice). Each loop reads only indices `< len` of that array:
//! blocks of 8 u16 / 4 u32 lanes are issued only while `i + lanes <= len` (or, for the page-specialised
//! scans, the 64-slot arrays are exact multiples of the block size). Unaligned `vld1q` is permitted.

use crate::nrect::{NRect, union_all};
#[cfg(target_arch = "aarch64")]
use core::arch::aarch64::*;

pub const CAP: usize = 64;
pub const PAD_DEPTH: u16 = 0xFFFF;

/// A fully materialised record (what the oracle stores and what `Bough::record` returns).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Record {
    pub depth: u16,
    pub ink: NRect,
    pub stamp: u32,
    pub kind: u8,
    pub payload: u32,
    pub handle: u32,
}

/// What a caller supplies to insert: stamp is assigned from the current epoch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecordInit {
    pub depth: u16,
    pub ink: NRect,
    pub kind: u8,
    pub payload: u32,
    /// Handle index (from `Bough::new_handle`), or 0 for none.
    pub handle: u32,
}

impl RecordInit {
    pub const fn new(depth: u16, ink: NRect, kind: u8, payload: u32) -> Self {
        RecordInit { depth, ink, kind, payload, handle: 0 }
    }
}

/// One page: 64 slots, columns are contiguous arrays. Slots `[0, len)` are live (the directory
/// owns `len`); slots `[len, 64)` are padding sentinels: `depth = 0xFFFF`, `ink = EMPTY`,
/// `stamp = 0`, `kind = 0`, `payload = 0`, `handle = 0`. Every SIMD scan therefore runs at full
/// capacity with no tail mask.
///
/// Layout (1536 B, 64-aligned): depth 0..128, ink 128..640, stamp 640..896, kind 896..960,
/// payload 960..1216, handle 1216..1472, version/id 1472..1480, pad to 1536.
#[repr(C, align(64))]
pub struct Page {
    pub depth: [u16; CAP],
    pub ink: [NRect; CAP],
    pub stamp: [u32; CAP],
    pub kind: [u8; CAP],
    pub payload: [u32; CAP],
    pub handle: [u32; CAP],
    /// Only grows: bumped on alloc, free and every structural edit (shift/insert/remove).
    pub version: u32,
    pub id: u32,
}

const _: () = assert!(core::mem::size_of::<Page>() == 1536);
const _: () = assert!(core::mem::align_of::<Page>() == 64);

impl Page {
    fn blank(id: u32) -> Box<Page> {
        let mut p = Box::new(Page {
            depth: [PAD_DEPTH; CAP],
            ink: [NRect::EMPTY; CAP],
            stamp: [0; CAP],
            kind: [0; CAP],
            payload: [0; CAP],
            handle: [0; CAP],
            version: 0,
            id,
        });
        p.version = 0;
        p
    }

    /// Reset slots `[lo, hi)` to the padding sentinel.
    #[inline]
    pub fn set_sentinel(&mut self, lo: usize, hi: usize) {
        self.depth[lo..hi].fill(PAD_DEPTH);
        self.ink[lo..hi].fill(NRect::EMPTY);
        self.stamp[lo..hi].fill(0);
        self.kind[lo..hi].fill(0);
        self.payload[lo..hi].fill(0);
        self.handle[lo..hi].fill(0);
    }

    /// Move `n` slots within the page from `src` to `dst` (memmove semantics) in every column.
    #[inline]
    pub fn shift(&mut self, src: usize, dst: usize, n: usize) {
        self.depth.copy_within(src..src + n, dst);
        self.ink.copy_within(src..src + n, dst);
        self.stamp.copy_within(src..src + n, dst);
        self.kind.copy_within(src..src + n, dst);
        self.payload.copy_within(src..src + n, dst);
        self.handle.copy_within(src..src + n, dst);
    }

    /// Copy `n` slots from another (distinct) page.
    #[inline]
    pub fn copy_from(&mut self, dst: usize, src: &Page, s: usize, n: usize) {
        self.depth[dst..dst + n].copy_from_slice(&src.depth[s..s + n]);
        self.ink[dst..dst + n].copy_from_slice(&src.ink[s..s + n]);
        self.stamp[dst..dst + n].copy_from_slice(&src.stamp[s..s + n]);
        self.kind[dst..dst + n].copy_from_slice(&src.kind[s..s + n]);
        self.payload[dst..dst + n].copy_from_slice(&src.payload[s..s + n]);
        self.handle[dst..dst + n].copy_from_slice(&src.handle[s..s + n]);
    }

    #[inline(always)]
    pub fn write(&mut self, slot: usize, r: &RecordInit, stamp: u32) {
        self.depth[slot] = r.depth;
        self.ink[slot] = r.ink.canon();
        self.stamp[slot] = stamp;
        self.kind[slot] = r.kind;
        self.payload[slot] = r.payload;
        self.handle[slot] = r.handle;
    }

    #[inline]
    pub fn read(&self, slot: usize) -> Record {
        Record {
            depth: self.depth[slot],
            ink: self.ink[slot],
            stamp: self.stamp[slot],
            kind: self.kind[slot],
            payload: self.payload[slot],
            handle: self.handle[slot],
        }
    }

    /// Union of all ink (padding is EMPTY, the identity).
    #[inline]
    pub fn union_ink(&self) -> NRect {
        union_all(&self.ink)
    }

    /// Minimum depth over the page (padding is 0xFFFF).
    #[inline]
    pub fn min_depth(&self) -> u16 {
        #[cfg(target_arch = "aarch64")]
        unsafe {
            let p = self.depth.as_ptr();
            let mut a = vld1q_u16(p);
            for i in 1..8 {
                a = vminq_u16(a, vld1q_u16(p.add(i * 8)));
            }
            vminvq_u16(a)
        }
        #[cfg(not(target_arch = "aarch64"))]
        {
            *self.depth.iter().min().unwrap()
        }
    }

    /// Maximum stamp over the page (padding is 0).
    #[inline]
    pub fn max_stamp(&self) -> u32 {
        #[cfg(target_arch = "aarch64")]
        unsafe {
            let p = self.stamp.as_ptr();
            let mut a = vld1q_u32(p);
            for i in 1..16 {
                a = vmaxq_u32(a, vld1q_u32(p.add(i * 4)));
            }
            vmaxvq_u32(a)
        }
        #[cfg(not(target_arch = "aarch64"))]
        {
            *self.stamp.iter().max().unwrap()
        }
    }
}

/// Type-stable pool: pages are boxed once and never returned to the allocator while the pool
/// lives. Freed pages are reset to padding and go on a free list; a page's `version` only grows.
pub struct PagePool {
    pages: Vec<Box<Page>>,
    free: Vec<u32>,
}

impl Default for PagePool {
    fn default() -> Self {
        Self::new()
    }
}

impl PagePool {
    pub fn new() -> Self {
        PagePool { pages: Vec::new(), free: Vec::new() }
    }

    /// A blank page (all sentinel). The returned pointer stays valid for the pool's lifetime.
    pub fn alloc(&mut self) -> *mut Page {
        if let Some(id) = self.free.pop() {
            let p = &mut *self.pages[id as usize];
            p.version += 1;
            p as *mut Page
        } else {
            let id = self.pages.len() as u32;
            self.pages.push(Page::blank(id));
            &mut *self.pages[id as usize] as *mut Page
        }
    }

    /// Return a page to the free list (reset to sentinels, version bumped).
    pub fn free(&mut self, id: u32) {
        let p = &mut *self.pages[id as usize];
        p.set_sentinel(0, CAP);
        p.version += 1;
        self.free.push(id);
    }

    pub fn total_pages(&self) -> usize {
        self.pages.len()
    }
    pub fn free_pages(&self) -> usize {
        self.free.len()
    }
    pub fn page(&self, id: u32) -> &Page {
        &self.pages[id as usize]
    }
    pub fn heap_bytes(&self) -> usize {
        self.pages.capacity() * 8 + self.pages.len() * core::mem::size_of::<Page>() + self.free.capacity() * 4
    }
}

// ---------------------------------------------------------------------------------------------
// u16 scans over depth columns (pages and the directory's min_depth column).
// ---------------------------------------------------------------------------------------------

/// First index `i >= from` with `a[i] <= d`.
#[inline(always)]
pub fn find_first_le(a: &[u16], from: usize, d: u16) -> Option<usize> {
    let n = a.len();
    let mut i = from;
    #[cfg(target_arch = "aarch64")]
    unsafe {
        let dv = vdupq_n_u16(d);
        while i + 8 <= n {
            let m = vcleq_u16(vld1q_u16(a.as_ptr().add(i)), dv);
            let nib = vget_lane_u64::<0>(vreinterpret_u64_u8(vmovn_u16(m)));
            if nib != 0 {
                return Some(i + (nib.trailing_zeros() / 8) as usize);
            }
            i += 8;
        }
    }
    while i < n {
        if a[i] <= d {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Last index `i < upto` with `a[i] < d`.
#[inline(always)]
pub fn find_last_lt(a: &[u16], upto: usize, d: u16) -> Option<usize> {
    let mut i = upto;
    #[cfg(target_arch = "aarch64")]
    unsafe {
        let dv = vdupq_n_u16(d);
        while i >= 8 {
            let m = vcltq_u16(vld1q_u16(a.as_ptr().add(i - 8)), dv);
            let nib = vget_lane_u64::<0>(vreinterpret_u64_u8(vmovn_u16(m)));
            if nib != 0 {
                return Some(i - 8 + ((63 - nib.leading_zeros()) / 8) as usize);
            }
            i -= 8;
        }
    }
    while i > 0 {
        i -= 1;
        if a[i] < d {
            return Some(i);
        }
    }
    None
}

/// `find_first_le` specialised to a full page column (64 slots, no tail): aligned 8-lane blocks, the
/// first one masked to lanes `>= from`. Padding (0xFFFF) never matches.
#[inline(always)]
pub fn page_find_first_le(a: &[u16; CAP], from: usize, d: u16) -> Option<usize> {
    #[cfg(target_arch = "aarch64")]
    unsafe {
        if from >= CAP {
            return None;
        }
        let dv = vdupq_n_u16(d);
        let mut i = from & !7;
        let mut keep = !0u64 << ((from & 7) * 8);
        loop {
            let m = vcleq_u16(vld1q_u16(a.as_ptr().add(i)), dv);
            let bytes = vget_lane_u64::<0>(vreinterpret_u64_u8(vmovn_u16(m))) & keep;
            if bytes != 0 {
                return Some(i + (bytes.trailing_zeros() / 8) as usize);
            }
            i += 8;
            keep = !0;
            if i == CAP {
                return None;
            }
        }
    }
    #[cfg(not(target_arch = "aarch64"))]
    find_first_le(a, from, d)
}

/// `find_last_lt` specialised to a full page column: last `i < upto` with `a[i] < d`.
#[inline(always)]
pub fn page_find_last_lt(a: &[u16; CAP], upto: usize, d: u16) -> Option<usize> {
    #[cfg(target_arch = "aarch64")]
    unsafe {
        if upto == 0 {
            return None;
        }
        let dv = vdupq_n_u16(d);
        let last = upto - 1;
        let mut i = last & !7;
        let mut keep = !0u64 >> ((7 - (last & 7)) * 8);
        loop {
            let m = vcltq_u16(vld1q_u16(a.as_ptr().add(i)), dv);
            let bytes = vget_lane_u64::<0>(vreinterpret_u64_u8(vmovn_u16(m))) & keep;
            if bytes != 0 {
                return Some(i + ((63 - bytes.leading_zeros()) / 8) as usize);
            }
            if i == 0 {
                return None;
            }
            i -= 8;
            keep = !0;
        }
    }
    #[cfg(not(target_arch = "aarch64"))]
    find_last_lt(a, upto, d)
}

/// Bit `i` set iff `stamp[i] > e`, all 64 slots.
#[inline(always)]
pub fn stamp_gt_mask(stamp: &[u32; CAP], e: u32) -> u64 {
    #[cfg(target_arch = "aarch64")]
    unsafe {
        let ev = vdupq_n_u32(e);
        const W: [u32; 4] = [1, 2, 4, 8];
        let w = vld1q_u32(W.as_ptr());
        let mut out = 0u64;
        for i in 0..16 {
            let m = vcgtq_u32(vld1q_u32(stamp.as_ptr().add(i * 4)), ev);
            out |= (vaddvq_u32(vandq_u32(m, w)) as u64) << (i * 4);
        }
        out
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        let mut out = 0u64;
        for i in 0..CAP {
            out |= ((stamp[i] > e) as u64) << i;
        }
        out
    }
}
