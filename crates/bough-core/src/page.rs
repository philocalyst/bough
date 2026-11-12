//! 64-slot SoA pages and the type-stable page pool.
//!
//! # Unsafe
//! The only `unsafe` here is NEON loads/stores through raw pointers derived from the page's own
//! fixed-size arrays (or a caller slice). Each loop reads only indices `< len` of that array:
//! blocks of 8 u16 lanes are issued only while `i + lanes <= len` (or, for the page-specialised
//! scans, the 64-slot arrays are exact multiples of the block size). Unaligned `vld1q` is permitted.

use crate::nrect::{COORD_MAX, COORD_MIN, NRect, WRect, union8};
#[cfg(target_arch = "aarch64")]
use core::arch::aarch64::*;

pub const CAP: usize = 64;
/// Records per group: the unit the cull and hit test filter at below the page.
pub const GROUP: usize = 8;
pub const NGROUPS: usize = CAP / GROUP;
pub const PAD_DEPTH: u16 = 0xFFFF;

/// The page carries an affine (`xm`, `xv`): absolute = M stored + xv + off.
pub const F_XF: u8 = 1;
/// The page holds ink outside the 16-bit domain: absolute = stored + `off` is formed in i32 and the
/// 16-bit absolute form is a clamped view. `|off| <= 32767` unless every record is beyond the domain on
/// that axis (then nothing in a window can meet the page and `offl` is not read).
pub const F_FAR: u8 = 2;
/// The page holds one record whose extent no 16-bit frame can hold (over 65,532 px): the stored ink is
/// its clamped view (`off` = 0) and the exact rect lives in the Bough's wide table under the page id.
pub const F_WIDE: u8 = 4;

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
#[derive(Clone, Copy)]
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
    /// 0 for a plain page; otherwise the page is not in the plain frame (see `F_XF`, `F_FAR`,
    /// `F_WIDE`) and every stored-to-absolute conversion takes the exact path. (A former padding byte.)
    pub flags: u8,
    /// Non-zero iff the page has a state page (`state.rs`: some record here has, or had until its consumer
    /// acknowledges, a state bit). A stateless page's operations test this byte and nothing else.
    pub state: u8,
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
    /// The page's affine, generalising `off` (round 3c, input agent: transformed subtrees). Stored inks are
    /// then the records' untransformed (local) inks, and absolute = `M s + xv + off` with `M = I + xm`
    /// (row-major `[a, b, c, d]`); all zero is the identity, so every untransformed page is unchanged. A
    /// transformed page holds one transformed subtree's records only (`Bough::set_run_xform` isolates them).
    pub xm: [f32; 4],
    pub xv: [f32; 2],
}

const _: () = assert!(core::mem::size_of::<Page>() == 1472);
const _: () = assert!(core::mem::align_of::<Page>() == 64);

/// Byte offsets of the columns the GPU reads (bough-metal `INTERFACE.md`): the shader's constants are
/// these numbers, and the asserts below pin them.
pub mod gpu_layout {
    use super::Page;
    pub const PAGE_BYTES: usize = core::mem::size_of::<Page>();
    pub const INK: usize = core::mem::offset_of!(Page, ink);
    pub const GINK: usize = core::mem::offset_of!(Page, gink);
    pub const PAYLOAD: usize = core::mem::offset_of!(Page, payload);
    pub const CLIP: usize = core::mem::offset_of!(Page, clip);
    pub const OFFL: usize = core::mem::offset_of!(Page, offl);
    pub const XF: usize = core::mem::offset_of!(Page, xm);
    const _: () = assert!(PAGE_BYTES == 1472 && INK == 128 && GINK == 640 && PAYLOAD == 768 && CLIP == 1280 && OFFL == 1436 && XF == 1448);
}

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
            flags: 0,
            state: 0,
            version: 0,
            id,
            dir: u32::MAX,
            dirty: 0,
            off: [0, 0],
            offl: [0; 4],
            wgen: 0,
            xm: [0.0; 4],
            xv: [0.0; 2],
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

    /// Stored (page-relative) rect -> absolute, as the 16-bit form: exact for a plain page (its ink is
    /// inside the domain); the bounding box of the transformed rect (grown by a pixel, as the kernels'
    /// coverage window) on a transformed page; the clamped view on a far or wide page.
    #[inline(always)]
    pub fn abs(&self, r: NRect) -> NRect {
        if self.flags != 0 {
            return self.abs_slow(r);
        }
        r.translate(self.off[0], self.off[1])
    }
    #[inline(never)]
    fn abs_slow(&self, r: NRect) -> NRect {
        if self.flags & F_XF != 0 {
            return self.xf_abs(r);
        }
        self.abs_wide(r).to_nrect()
    }
    /// Stored rect -> exact absolute rect (not for transformed or wide pages: see `Bough::slot_wide`).
    #[inline]
    pub fn abs_wide(&self, r: NRect) -> WRect {
        WRect::from(r).translate(self.off[0], self.off[1])
    }
    /// Exact absolute rect -> stored, if it fits this page's frame.
    #[inline]
    pub fn rel_of(&self, w: WRect) -> Option<NRect> {
        if w.is_empty() {
            return Some(NRect::EMPTY);
        }
        let r = [w.0[0] - self.off[0], w.0[1] - self.off[1], w.0[2] - self.off[0], w.0[3] - self.off[1]];
        r.iter().all(|v| (COORD_MIN..=COORD_MAX).contains(v)).then(|| NRect::new(r[0], r[1], r[2], r[3]))
    }
    /// Does this page carry a transform?
    #[inline(always)]
    pub fn is_xf(&self) -> bool {
        self.flags & F_XF != 0
    }
    /// Re-derive `F_XF` after `xm` / `xv` were written.
    #[inline]
    pub fn sync_xf(&mut self) {
        let xf = self.xm != [0.0; 4] || self.xv != [0.0; 2];
        self.flags = (self.flags & !F_XF) | if xf { F_XF } else { 0 };
    }
    /// The four corners of stored rect `r` transformed (absolute px, `off` included).
    #[inline]
    pub fn xf_quad(&self, r: NRect) -> [[f32; 2]; 4] {
        let [x0, y0, x1, y1] = r.canon().to_xyxy();
        let m = [1.0 + self.xm[0], self.xm[1], self.xm[2], 1.0 + self.xm[3]];
        let (ox, oy) = (self.xv[0] + self.off[0] as f32, self.xv[1] + self.off[1] as f32);
        [(x0, y0), (x1, y0), (x1, y1), (x0, y1)].map(|(x, y)| {
            let (x, y) = (x as f32, y as f32);
            [m[0] * x + m[1] * y + ox, m[2] * x + m[3] * y + oy]
        })
    }
    #[inline(never)]
    fn xf_abs(&self, r: NRect) -> NRect {
        if r.is_empty() {
            return r;
        }
        let q = self.xf_quad(r);
        let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
        for p in q {
            for i in 0..2 {
                lo[i] = lo[i].min(p[i]);
                hi[i] = hi[i].max(p[i]);
            }
        }
        NRect::new(lo[0].floor() as i32 - 1, lo[1].floor() as i32 - 1, hi[0].ceil() as i32 + 1, hi[1].ceil() as i32 + 1)
    }
    /// The stored-frame point that absolute pixel `(x, y)`'s centre maps to (the inverse transform).
    #[inline]
    pub fn xf_inv_point(&self, x: i32, y: i32) -> (i32, i32) {
        let m = [1.0 + self.xm[0], self.xm[1], self.xm[2], 1.0 + self.xm[3]];
        let det = m[0] * m[3] - m[1] * m[2];
        if det.abs() < 1e-12 {
            return (i32::MIN / 2, i32::MIN / 2);
        }
        let (dx, dy) = (x as f32 + 0.5 - self.xv[0] - self.off[0] as f32, y as f32 + 0.5 - self.xv[1] - self.off[1] as f32);
        let (sx, sy) = ((m[3] * dx - m[1] * dy) / det, (-m[2] * dx + m[0] * dy) / det);
        (sx.floor() as i32, sy.floor() as i32)
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
    /// Set the offset. `offl` is exact for `|dx|, |dy| <= 32767` (every page a window can meet); beyond
    /// that the page is far (`F_FAR`, wholly outside the domain) and its lanes are never read.
    #[inline(always)]
    pub fn set_off(&mut self, dx: i32, dy: i32) {
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
    /// Absolute union of all ink (on a transformed page: the transformed union's bounding box).
    #[inline(always)]
    pub fn union_abs(&self) -> NRect {
        self.abs(self.union_ink())
    }

    /// Set the page's affine to `x -> m (x + off) + K + t - m K` in absolute px... expressed in the stored
    /// frame: absolute = `m s + xv + off` keeps `off` free to move (translations after the transform).
    pub fn set_xf(&mut self, m: [f32; 4], k: [f32; 2], t: [f32; 2]) {
        // absolute(s) = K + t + m (s + off - K)  =>  xv = K + t - m K + (m - I) off
        let (ox, oy) = (self.off[0] as f32, self.off[1] as f32);
        let mk = [m[0] * k[0] + m[1] * k[1], m[2] * k[0] + m[3] * k[1]];
        let mo = [m[0] * ox + m[1] * oy - ox, m[2] * ox + m[3] * oy - oy];
        self.xm = [m[0] - 1.0, m[1], m[2], m[3] - 1.0];
        self.xv = [k[0] + t[0] - mk[0] + mo[0], k[1] + t[1] - mk[1] + mo[1]];
        self.sync_xf();
    }

    /// Store an absolute (canonical, in-domain) ink at `slot` of a plain page, relative to `off`; false
    /// (nothing stored) if the page is not plain or the relative form leaves the 16-bit frame: the caller
    /// takes the exact path.
    #[inline(always)]
    pub fn store_abs(&mut self, slot: usize, ink: NRect) -> bool {
        if self.flags & !F_XF != 0 {
            return false;
        }
        match ink.translate_checked(-self.off[0], -self.off[1]) {
            Some(r) => {
                self.ink[slot] = r;
                true
            }
            None if self.flags & F_XF != 0 => {
                self.normalize();
                self.ink[slot] = ink;
                true
            }
            None => false,
        }
    }

    /// Transformed pages only: fold `off` into the affine's translation and reset it to 0 (before records
    /// are copied to another page). Untransformed pages keep their frame; pages that exchange records
    /// share it.
    #[inline]
    pub fn normalize(&mut self) {
        if self.off != [0, 0] && self.is_xf() {
            {
                // fold the offset into the affine's translation instead: absolute = m s + xv + off and
                // absolute = m (s + off) + xv' must agree, so xv' = xv + off - m off
                let (ox, oy) = (self.off[0] as f32, self.off[1] as f32);
                let m = [1.0 + self.xm[0], self.xm[1], self.xm[2], 1.0 + self.xm[3]];
                self.xv = [self.xv[0] + ox - (m[0] * ox + m[1] * oy), self.xv[1] + oy - (m[2] * ox + m[3] * oy)];
            }
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

/// Type-stable pool: pages are created once and never returned to the allocator while the pool
/// lives. Freed pages are reset to padding and go on a free list; a page's `version` only grows.
///
/// Two backings, one mechanism: **arena** pages are the elements of a fixed-capacity column carved
/// from the GPU-shared region ([`PagePool::in_arena`]): page `id` is element `id`, so the GPU finds it
/// at `arena offset + id * size_of::<Page>()` and reads the very bytes the CPU writes (no mirror).
/// Without an arena each page is its own heap box (standalone use: tests, benches, replicas).
pub struct PagePool {
    pages: Vec<*mut Page>,
    free: Vec<u32>,
    arena: Option<bough_mem::Col<Page>>,
}

impl Default for PagePool {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for PagePool {
    fn drop(&mut self) {
        if self.arena.is_none() {
            for &p in &self.pages {
                // SAFETY: heap pages were created by Box::into_raw in `alloc`/`ensure` and are freed once.
                drop(unsafe { Box::from_raw(p) });
            }
        }
    }
}

impl PagePool {
    /// The pool's pages (the region arena, or boxed heap pages) and its tables, as ledger lines.
    pub fn ledger(&self, l: &mut bough_mem::Ledger) {
        use bough_mem::Scales::Content;
        match &self.arena {
            Some(a) => l.col("core", a, Content),
            None => {
                let b = self.pages.len() * size_of::<Page>();
                l.many("core", "pages (boxed)", Content, b, b);
            }
        }
        l.vec("core", "page pointers", &self.pages, self.pages.capacity(), Content);
        l.vec("core", "page free list", &self.free, self.free.capacity(), Content);
    }
    pub fn new() -> Self {
        PagePool { pages: Vec::new(), free: Vec::new(), arena: None }
    }

    /// A pool whose pages are the elements of `arena` (an empty carved column).
    pub fn in_arena(arena: bough_mem::Col<Page>) -> Self {
        assert!(arena.is_empty());
        PagePool { pages: Vec::with_capacity(arena.capacity()), free: Vec::new(), arena: Some(arena) }
    }

    /// Byte offset of page 0 in the arena's region (`None` for heap pages).
    pub fn arena_offset(&self) -> Option<usize> {
        self.arena.as_ref().and_then(|a| a.region_offset())
    }

    fn create(&mut self) -> *mut Page {
        let id = self.pages.len() as u32;
        let p = match self.arena.as_mut() {
            Some(a) => {
                // SAFETY: a page written whole (`blank`), element `id` of the arena; the column never
                // moves (carved), so the pointer stays valid for the pool's life.
                a.push(*Page::blank(id));
                unsafe { a.as_mut_ptr().add(id as usize) }
            }
            None => Box::into_raw(Page::blank(id)),
        };
        self.pages.push(p);
        p
    }

    /// A blank page (all sentinel). The returned pointer stays valid for the pool's lifetime.
    pub fn alloc(&mut self) -> *mut Page {
        if let Some(id) = self.free.pop() {
            // SAFETY: a pool page (valid for the pool's life); &mut self is exclusive.
            let p = unsafe { &mut *self.pages[id as usize] };
            p.version += 1;
            p.wgen = p.wgen.wrapping_add(1);
            p as *mut Page
        } else {
            self.create()
        }
    }

    /// Return a page to the free list (reset to sentinels, version bumped).
    pub fn free(&mut self, id: u32) {
        // SAFETY: as in `alloc`.
        let p = unsafe { &mut *self.pages[id as usize] };
        p.set_sentinel(0, CAP);
        p.gink = [NRect::EMPTY; NGROUPS];
        p.cgroups = 0;
        p.flags = 0;
        p.state = 0;
        p.set_off(0, 0);
        // A freed page carries no transform: the next run to reuse it must not inherit one (round 6).
        (p.xm, p.xv) = ([0.0; 4], [0.0; 2]);
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
    /// The stable pointer of page `id`.
    #[inline(always)]
    pub fn ptr(&self, id: u32) -> *mut Page {
        self.pages[id as usize]
    }
    pub fn page(&self, id: u32) -> &Page {
        // SAFETY: as in `alloc`; shared access through &self.
        unsafe { &*self.pages[id as usize] }
    }
    /// Make page `id` exist (blank) in this pool; returns its stable pointer (replica pools).
    pub fn ensure(&mut self, id: u32) -> *mut Page {
        while self.pages.len() <= id as usize {
            self.create();
        }
        self.pages[id as usize]
    }
    pub fn heap_bytes(&self) -> usize {
        self.pages.capacity() * 8 + self.pages.len() * core::mem::size_of::<Page>() + self.free.capacity() * 4
    }
    /// Pages created so far (free ones included): the arena's high-water mark.
    #[inline]
    pub fn created(&self) -> usize {
        self.pages.len()
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
