//! 64-slot SoA pages and the type-stable page pool.
//!
//! # Unsafe
//! The only `unsafe` here is NEON loads/stores through raw pointers derived from the page's own
//! fixed-size arrays (or a caller slice). Each loop reads only indices `< len` of that array:
//! blocks of 8 u16 lanes are issued only while `i + lanes <= len` (or, for the page-specialised
//! scans, the 64-slot arrays are exact multiples of the block size). Unaligned `vld1q` is permitted.

use crate::nrect::{NRect, union8};
#[cfg(target_arch = "aarch64")]
use core::arch::aarch64::*;

pub const CAP: usize = 64;
/// Records per group: the unit the cull and hit test filter at below the page.
pub const GROUP: usize = 8;
pub const NGROUPS: usize = CAP / GROUP;
pub const PAD_DEPTH: u16 = 0xFFFF;

/// A fully materialised record (what the oracle stores and what `Bough::record` returns).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Record {
    pub depth: u16,
    pub ink: NRect,
    pub kind: u8,
    pub payload: u32,
    pub handle: u32,
    /// Clip id (`Bough::new_clip`); 0 = unclipped. Visible ink = ink ∩ clip.
    pub clip: u16,
}

/// What a caller supplies to insert.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecordInit {
    pub depth: u16,
    pub ink: NRect,
    pub kind: u8,
    pub payload: u32,
    /// Handle index (from `Bough::new_handle`), or 0 for none.
    pub handle: u32,
    /// Clip id (from `Bough::new_clip`), or 0 for none.
    pub clip: u16,
}

impl RecordInit {
    pub const fn new(depth: u16, ink: NRect, kind: u8, payload: u32) -> Self {
        RecordInit { depth, ink, kind, payload, handle: 0, clip: 0 }
    }
    pub const fn with_clip(mut self, clip: u16) -> Self {
        self.clip = clip;
        self
    }
}

/// One page: 64 slots, columns are contiguous arrays. Slots `[0, len)` are live (the directory
/// owns `len`); slots `[len, 64)` are padding sentinels: `depth = 0xFFFF`, `ink = EMPTY`,
/// `kind = 0`, `payload = 0`, `handle = 0`. Every SIMD scan therefore runs at full capacity with no
/// tail mask. `gink[g]` is the exact union of `ink[8g..8g+8]` (EMPTY for all-padding groups).
///
/// Layout (1472 B, 64-aligned): depth 0..128, ink 128..640, gink 640..704, kind 704..768,
/// payload 768..1024, handle 1024..1280, clip 1280..1408, then the header (cgroups, version, id,
/// dir, dirty, off).
#[repr(C, align(64))]
pub struct Page {
    pub depth: [u16; CAP],
    pub ink: [NRect; CAP],
    pub gink: [NRect; NGROUPS],
    pub kind: [u8; CAP],
    pub payload: [u32; CAP],
    pub handle: [u32; CAP],
    /// Clip id per record (0 = none).
    pub clip: [u16; CAP],
    /// Bit `g` set iff group `g` holds a record with a clip id (maintained by `regroup`): clip-free
    /// groups never look at the clip column.
    pub cgroups: u8,
    /// Only grows: bumped on alloc, free and every structural edit (shift/insert/remove).
    pub version: u32,
    pub id: u32,
    /// Directory index of this page (maintained by the directory; `u32::MAX` when free).
    pub dir: u32,
    /// Groups whose summary is stale inside a batched write (`Bough::set_inks`); 0 otherwise.
    pub dirty: u8,
    /// Translation of every record in the page: absolute ink = `ink[s] + off` (and `gink` likewise).
    /// A translated run's interior pages change only this (attempt 5); queries fold it into their
    /// threshold instead of touching records.
    pub off: [i32; 2],
    /// `off` as i16 lanes `(dx, dy, -dx, -dy)` (invariant: |dx|, |dy| <= 32767): stored -> absolute
    /// is one wrapping lane add, and a query threshold or clip moves into page coordinates with one
    /// saturating lane subtract.
    pub offl: [i16; 4],
    /// Write generation: bumped by every mutable access through the directory or a handle (and on
    /// alloc/free); `Bough::freeze_into` copies a page only when it differs from the replica's.
    pub wgen: u32,
}

const _: () = assert!(core::mem::size_of::<Page>() == 1472);
const _: () = assert!(core::mem::align_of::<Page>() == 64);

impl Page {
    fn blank(id: u32) -> Box<Page> {
        Box::new(Page {
            depth: [PAD_DEPTH; CAP],
            ink: [NRect::EMPTY; CAP],
            gink: [NRect::EMPTY; NGROUPS],
            kind: [0; CAP],
            payload: [0; CAP],
            handle: [0; CAP],
            clip: [0; CAP],
            cgroups: 0,
            version: 0,
            id,
            dir: u32::MAX,
            dirty: 0,
            off: [0, 0],
            offl: [0; 4],
            wgen: 0,
        })
    }

    /// Reset slots `[lo, hi)` to the padding sentinel (group summaries are the caller's job).
    #[inline]
    pub fn set_sentinel(&mut self, lo: usize, hi: usize) {
        self.depth[lo..hi].fill(PAD_DEPTH);
        self.ink[lo..hi].fill(NRect::EMPTY);
        self.kind[lo..hi].fill(0);
        self.payload[lo..hi].fill(0);
        self.handle[lo..hi].fill(0);
        self.clip[lo..hi].fill(0);
    }

    /// Move `n` slots within the page from `src` to `dst` (memmove semantics) in every column.
    #[inline]
    pub fn shift(&mut self, src: usize, dst: usize, n: usize) {
        self.depth.copy_within(src..src + n, dst);
        self.ink.copy_within(src..src + n, dst);
        self.kind.copy_within(src..src + n, dst);
        self.payload.copy_within(src..src + n, dst);
        self.handle.copy_within(src..src + n, dst);
        self.clip.copy_within(src..src + n, dst);
    }

    /// Copy `n` slots from another (distinct) page.
    #[inline]
    pub fn copy_from(&mut self, dst: usize, src: &Page, s: usize, n: usize) {
        self.depth[dst..dst + n].copy_from_slice(&src.depth[s..s + n]);
        self.ink[dst..dst + n].copy_from_slice(&src.ink[s..s + n]);
        self.kind[dst..dst + n].copy_from_slice(&src.kind[s..s + n]);
        self.payload[dst..dst + n].copy_from_slice(&src.payload[s..s + n]);
        self.handle[dst..dst + n].copy_from_slice(&src.handle[s..s + n]);
        self.clip[dst..dst + n].copy_from_slice(&src.clip[s..s + n]);
    }

    #[inline(always)]
    pub fn write(&mut self, slot: usize, r: &RecordInit) {
        self.depth[slot] = r.depth;
        self.ink[slot] = r.ink.canon();
        self.kind[slot] = r.kind;
        self.payload[slot] = r.payload;
        self.handle[slot] = r.handle;
        self.clip[slot] = r.clip;
    }

    /// The record with its absolute ink.
    #[inline]
    pub fn read(&self, slot: usize) -> Record {
        Record { depth: self.depth[slot], ink: self.abs(self.ink[slot]), kind: self.kind[slot], payload: self.payload[slot], handle: self.handle[slot], clip: self.clip[slot] }
    }

    /// Stored (page-relative) rect -> absolute.
    #[inline(always)]
    pub fn abs(&self, r: NRect) -> NRect {
        r.translate(self.off[0], self.off[1])
    }
    /// Absolute rect -> stored (page-relative).
    #[inline(always)]
    pub fn rel(&self, r: NRect) -> NRect {
        r.translate(-self.off[0], -self.off[1])
    }
    /// `(dx, dy, -dx, -dy)` as i16 lanes, for `NRect::add_lanes` (stored -> absolute, non-empty).
    #[inline(always)]
    pub fn off_lanes(&self) -> [i16; 4] {
        self.offl
    }
    /// Set the offset (|dx|, |dy| <= 32767).
    #[inline(always)]
    pub fn set_off(&mut self, dx: i32, dy: i32) {
        debug_assert!(dx.abs() <= 32767 && dy.abs() <= 32767);
        self.off = [dx, dy];
        self.offl = [dx as i16, dy as i16, (-dx) as i16, (-dy) as i16];
    }
    /// Absolute rect -> page-relative with saturating lanes: a lane that leaves i16 becomes
    /// -inf/+inf, which keeps `intersect` exact against in-domain stored inks (for query rects).
    #[inline(always)]
    pub fn rel_sat(&self, r: NRect) -> NRect {
        let (a, o) = (r.0, self.offl);
        NRect([a[0].saturating_sub(o[0]), a[1].saturating_sub(o[1]), a[2].saturating_sub(o[2]), a[3].saturating_sub(o[3])])
    }
    /// Absolute union of all ink.
    #[inline(always)]
    pub fn union_abs(&self) -> NRect {
        self.abs(self.union_ink())
    }

    /// Store an absolute (canonical) ink at `slot`: relative to `off`, or -- if that would leave
    /// the i16 domain -- after normalising the page (then relative = absolute).
    #[inline(always)]
    pub fn store_abs(&mut self, slot: usize, ink: NRect) {
        match ink.translate_checked(-self.off[0], -self.off[1]) {
            Some(r) => self.ink[slot] = r,
            None => {
                self.normalize();
                self.ink[slot] = ink;
            }
        }
    }

    /// Apply `off` to the stored inks and reset it to 0 (before records are copied to another page).
    #[inline]
    pub fn normalize(&mut self) {
        if self.off != [0, 0] {
            // SAFETY: 64 in-bounds rects; padding is EMPTY and stays EMPTY.
            unsafe { crate::nrect::translate_union(self.ink.as_mut_ptr(), CAP, self.off[0], self.off[1]) };
            self.set_off(0, 0);
            for g in 0..NGROUPS {
                self.regroup(g);
            }
        }
    }

    /// Recompute the summary of group `g` (8 slots): 4 loads, 3 `smin`, a fold.
    #[inline(always)]
    pub fn regroup(&mut self, g: usize) {
        // SAFETY: g < NGROUPS, so 8g + 8 <= CAP.
        unsafe { *self.gink.get_unchecked_mut(g) = union8(self.ink.as_ptr().add(g * GROUP)) };
        let c = &self.clip[g * GROUP..g * GROUP + GROUP];
        let any = c.iter().fold(0u16, |a, &b| a | b) != 0;
        self.cgroups = (self.cgroups & !(1 << g)) | ((any as u8) << g);
    }

    /// Recompute the summaries of every group that holds a slot in `[lo, hi)` (hi > lo).
    #[inline]
    pub fn regroup_range(&mut self, lo: usize, hi: usize) {
        for g in lo / GROUP..hi.div_ceil(GROUP) {
            self.regroup(g);
        }
    }

    /// Union of all ink = union of the 8 group summaries.
    #[inline(always)]
    pub fn union_ink(&self) -> NRect {
        // SAFETY: gink has exactly 8 rects.
        unsafe { union8(self.gink.as_ptr()) }
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
            p.wgen = p.wgen.wrapping_add(1);
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
        p.gink = [NRect::EMPTY; NGROUPS];
        p.cgroups = 0;
        p.set_off(0, 0);
        p.version += 1;
        p.wgen = p.wgen.wrapping_add(1);
        p.dir = u32::MAX;
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
    /// Make page `id` exist (blank) in this pool; returns its stable pointer (replica pools).
    pub fn ensure(&mut self, id: u32) -> *mut Page {
        while self.pages.len() <= id as usize {
            let n = self.pages.len() as u32;
            self.pages.push(Page::blank(n));
        }
        &mut *self.pages[id as usize] as *mut Page
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
