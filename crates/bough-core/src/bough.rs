//! The Bough: paged SoA records in paint preorder + a directory of page summaries.
//!
//! Filtering hierarchy (every level is a SIMD test of an `NRect` column against one threshold):
//! chunk (64 pages) -> page -> group (8 records) -> record. The cull emits payloads through a
//! table-driven compaction (`emit::compact8`), so emission costs no per-record branch.
//!
//! # Safety model
//! Pages live in a [`PagePool`] (boxed, never freed while the `Bough` lives). The directory
//! stores raw `*mut Page` for each page so scans avoid a pool indirection, and the handle table
//! stores the page pointer of each bound handle. Invariants: every pointer in `d_ptr[0..n]` refers
//! to a distinct live pool page whose `dir` field is its directory index; every bound handle entry
//! points at the live page holding its record; mutation happens only through `&mut self`; no
//! `&Page`/`&mut Page` obtained from these pointers outlives the method that created it.

use crate::damage::{DamageRing, Plane, RING, ScrollRec, tile_span};
use crate::emit::compact8;
use crate::nrect::{COORD_MAX, COORD_MIN, NRect, Thresh, WRect, lt_mask, lt8, translate_union, union_all};
use crate::page::{F_FAR, F_WIDE, F_XF, CAP, GROUP, NGROUPS, PAD_DEPTH, Page, PagePool, Record, find_first_le, find_last_lt, page_find_first_le, page_find_last_lt};
pub use crate::page::RecordInit;
use crate::state::{STATE_PLANES, StatePool, low};
use bough_mem::Col;

/// Pointer and focus queries (hit test, hit cell, flagged chains, tab order): `bough/hit.rs`.
pub mod hit;

/// The GPU-visible storage of a Bough, carved from one region (see `bough_mem`): pages, the
/// directory's page-id and union-ink columns, and the clip table. The GPU reads these in place.
pub struct Columns {
    pub pages: Col<Page>,
    pub dir_id: Col<u32>,
    pub dir_ink: Col<NRect>,
    pub clips: Col<NRect>,
}

/// Pages below this fill are merged into a neighbour when they fit (bbtree `minimum`).
pub const MIN_FILL: usize = 8;
/// After an end-biased split the side that keeps receiving inserts holds `min(2*MIN_FILL, CAP/2)`.
pub const RECEIVER_KEEPS: usize = if 2 * MIN_FILL < CAP / 2 { 2 * MIN_FILL } else { CAP / 2 };
/// Directory entries per block. The directory's columns are flat arrays cut into blocks of `CHUNK`
/// entries in paint order; a block holds 1..=CHUNK consecutive pages at its front and sentinel holes after
/// them (EMPTY ink, depth 0xFFFF, a null pointer). A page's directory index is `block * CHUNK + slot`: a
/// physical position, which changes only when a page is inserted or removed before it *in its own
/// block*, so a structural edit moves at most one block's entries. Every per-block scan runs at capacity.
pub const CHUNK: usize = 64;
const RUN_SHIFT: usize = 6;
const _: () = assert!(CHUNK == 1 << RUN_SHIFT);
/// A bulk build, and appends at the end, fill blocks to this many pages: the headroom is the number of
/// page splits a block absorbs before it is cut in two (the one event that still moves the later blocks).
pub const RUN_FILL: usize = 56;
const LOCAL_PAD: u16 = 0xFFFF;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Handle {
    pub idx: u32,
    pub generation: u32,
}

#[derive(Clone, Copy)]
struct HEntry {
    /// Page holding the record, or null when unbound.
    page: *mut Page,
    slot: u32,
    generation: u32,
    live: bool,
}

pub struct Bough {
    pool: PagePool,
    // Directory, SoA (index = directory position).
    /// The page of each directory entry; null in holes, and one more null after the last block (so the
    /// entry after any live one can be read: `next_dir`).
    d_ptr: Vec<*mut Page>,
    /// Page id per directory entry and the page's absolute union ink: the two directory columns the
    /// GPU's cull reads (carved from the shared region by [`Bough::with_columns`]).
    d_id: Col<u32>,
    d_ink: Col<NRect>,
    d_min_depth: Vec<u16>,
    d_len: Vec<u32>,
    /// Records before each page inside its block (`0xFFFF` in holes): rank inside a block is a count of
    /// the lanes `<= rel` over all 64, no tail mask.
    d_local: Vec<u16>,
    /// Per block (index = block number, paint order): live entries, records, records before the block
    /// (`r_start` has one more entry: the total), and the least depth in the block.
    r_len: Vec<u32>,
    r_recs: Vec<u32>,
    r_start: Vec<u32>,
    r_min_depth: Vec<u16>,
    /// Blocks whose derived columns are stale after a structural edit (inclusive; empty when lo > hi).
    pend: (usize, usize),
    /// Live directory entries (pages).
    np: usize,
    /// Counts every change to what a replica holds (pages, directory, clips): `freeze_into` a replica
    /// that already has this count copies only the damage ring. `uid` tells Boughs apart.
    mgen: u64,
    uid: u64,
    /// For a replica: the (uid, mgen) of the source it last copied.
    frozen: (u64, u64),
    /// Union of `d_ink` per chunk of 64 directory entries.
    c_ink: Vec<NRect>,
    htab: Vec<HEntry>,
    hfree: Vec<u32>,
    ring: DamageRing,
    total: usize,
    scratch: Vec<RecordInit>,
    /// Records restated in a page's frame (an insert into a page whose offset is not zero).
    rel_scratch: Vec<RecordInit>,
    /// The exact rects of the records in wide pages (`F_WIDE`: one record no 16-bit frame can hold), by
    /// page id. A handful at most (a document's background, a dormant run's extent).
    wide: Vec<(u32, WRect)>,
    /// Record state planes (`state.rs`): the state pages by page id, and per plane the ids of the pages
    /// whose bits differ from what the plane's consumer last acknowledged.
    state: StatePool,
    flip: [Vec<u32>; STATE_PLANES],
    dirty_scratch: Vec<u32>,
    /// Clip rects by id (absolute); id 0 = the whole domain (unclipped). GPU-visible (the shader clamps
    /// every record to `clips[page.clip[slot]]`).
    clips: Col<NRect>,
    /// Kinds whose content is uniform (translation-invariant): their ink changes mark only the
    /// symmetric difference of old and new (damage lab section 5.2).
    uniform: [u64; 4],
}

/// How the `m` pages of an overflowing insert are sized.
struct Sizes {
    m: usize,
    t: usize,
    kind: u8, // 0 append, 1 prepend, 2 balanced
}
impl Sizes {
    fn new(t: usize, m: usize, s: usize, l: usize) -> Sizes {
        let kind = if s == l { 0 } else if s == 0 { 1 } else { 2 };
        Sizes { m, t, kind }
    }
    #[inline]
    fn size(&self, i: usize) -> usize {
        let (m, t) = (self.m, self.t);
        let r = t - CAP * (m - 1);
        match self.kind {
            0 => {
                if r < RECEIVER_KEEPS {
                    if i == m - 1 {
                        RECEIVER_KEEPS
                    } else if i == m - 2 {
                        CAP - (RECEIVER_KEEPS - r)
                    } else {
                        CAP
                    }
                } else if i == m - 1 {
                    r
                } else {
                    CAP
                }
            }
            1 => {
                if r < RECEIVER_KEEPS {
                    if i == 0 {
                        RECEIVER_KEEPS
                    } else if i == 1 {
                        CAP - (RECEIVER_KEEPS - r)
                    } else {
                        CAP
                    }
                } else if i == 0 {
                    r
                } else {
                    CAP
                }
            }
            _ => t / m + usize::from(i < t % m),
        }
    }
}

/// Emit `a \ b` (pixel set difference of two canonical rects) as at most 4 disjoint rects.
#[inline]
pub fn rect_sub(a: NRect, b: NRect, emit: &mut impl FnMut(NRect)) {
    if a.is_empty() {
        return;
    }
    let i = a.intersect(b);
    if i.is_empty() {
        emit(a);
        return;
    }
    let [ax0, ay0, ax1, ay1] = a.to_xyxy();
    let [ix0, iy0, ix1, iy1] = i.to_xyxy();
    if ay0 < iy0 {
        emit(NRect::new(ax0, ay0, ax1, iy0));
    }
    if iy1 < ay1 {
        emit(NRect::new(ax0, iy1, ax1, ay1));
    }
    if ax0 < ix0 {
        emit(NRect::new(ax0, iy0, ix0, iy1));
    }
    if ix1 < ax1 {
        emit(NRect::new(ix1, iy0, ax1, iy1));
    }
}

impl Bough {
    /// A Bough on its own heap storage (tests, benches, replicas).
    pub fn new(window_w: i32, window_h: i32) -> Bough {
        Self::build(window_w, window_h, PagePool::new(), Col::owned("dir_id"), Col::owned("dir_ink"), Col::owned("clips"))
    }

    /// A Bough whose pages, directory columns and clip table are `cols` (carved, empty columns of one
    /// region): the GPU reads them where the CPU writes them.
    pub fn with_columns(window_w: i32, window_h: i32, cols: Columns) -> Bough {
        Self::build(window_w, window_h, PagePool::in_arena(cols.pages), cols.dir_id, cols.dir_ink, cols.clips)
    }

    /// Region offsets of (page 0, directory page ids, directory inks, clips), when carved.
    pub fn gpu_offsets(&self) -> Option<[usize; 4]> {
        Some([self.pool.arena_offset()?, self.d_id.region_offset()?, self.d_ink.region_offset()?, self.clips.region_offset()?])
    }

    fn build(window_w: i32, window_h: i32, pool: PagePool, d_id: Col<u32>, d_ink: Col<NRect>, mut clips: Col<NRect>) -> Bough {
        clips.clear();
        clips.push(NRect::new(crate::nrect::COORD_MIN, crate::nrect::COORD_MIN, crate::nrect::COORD_MAX, crate::nrect::COORD_MAX));
        Bough {
            pool,
            d_ptr: vec![core::ptr::null_mut()],
            d_id,
            d_ink,
            d_min_depth: Vec::new(),
            d_len: Vec::new(),
            d_local: Vec::new(),
            r_len: Vec::new(),
            r_recs: Vec::new(),
            r_start: vec![0],
            r_min_depth: Vec::new(),
            pend: (usize::MAX, 0),
            np: 0,
            mgen: 0,
            uid: {
                static NEXT: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(1);
                NEXT.fetch_add(1, core::sync::atomic::Ordering::Relaxed)
            },
            frozen: (0, 0),
            c_ink: Vec::new(),
            htab: vec![HEntry { page: core::ptr::null_mut(), slot: 0, generation: 0, live: false }],
            hfree: Vec::new(),
            ring: DamageRing::new(window_w, window_h),
            total: 0,
            scratch: Vec::new(),
            rel_scratch: Vec::new(),
            wide: Vec::new(),
            state: StatePool::default(),
            flip: Default::default(),
            dirty_scratch: Vec::new(),
            clips,
            uniform: [0; 4],
        }
    }

    // ------------------------------------------------------------------ basic accessors

    #[inline(always)]
    pub fn len(&self) -> usize {
        self.total
    }
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.total == 0
    }
    /// Live pages.
    #[inline(always)]
    pub fn num_pages(&self) -> usize {
        self.np
    }
    /// Entries of the directory columns (`d_id`, `d_ink`): blocks of 64, each with its live pages first and
    /// holes after (EMPTY ink, which fails every test). What a reader that scans the columns must cover.
    #[inline(always)]
    pub fn dir_len(&self) -> usize {
        self.r_len.len() << RUN_SHIFT
    }
    pub fn pool(&self) -> &PagePool {
        &self.pool
    }
    /// High-water marks of the GPU-read columns: (pages ever created, directory entries, clip ids ever
    /// made). A presenter whose GPU view of a column is its used prefix must cover these (bough-metal
    /// `Fused::ensure_columns`). The directory runs ahead of the page count (blocks of 64 with holes).
    #[inline]
    pub fn gpu_extent(&self) -> (usize, usize, usize) {
        (self.pool.created(), self.d_ptr.len(), self.clips.len())
    }
    /// Every column and table this Bough holds, as memory-ledger lines (diagnostics; never on a frame path).
    pub fn ledger(&self, l: &mut bough_mem::Ledger) {
        use bough_mem::Scales::{Content, Window};
        const G: &str = "core";
        self.pool.ledger(l);
        l.col(G, &self.d_id, Content);
        l.col(G, &self.d_ink, Content);
        l.col(G, &self.clips, Content);
        l.vec(G, "dir page pointers", &self.d_ptr, self.d_ptr.capacity(), Content);
        l.vec(G, "dir min depth", &self.d_min_depth, self.d_min_depth.capacity(), Content);
        l.vec(G, "dir lengths", &self.d_len, self.d_len.capacity(), Content);
        l.vec(G, "dir chunk inks", &self.c_ink, self.c_ink.capacity(), Content);
        l.vec(G, "dir local ranks", &self.d_local, self.d_local.capacity(), Content);
        l.vec(G, "block lengths", &self.r_len, self.r_len.capacity(), Content);
        l.vec(G, "block records", &self.r_recs, self.r_recs.capacity(), Content);
        l.vec(G, "block starts", &self.r_start, self.r_start.capacity(), Content);
        l.vec(G, "block min depth", &self.r_min_depth, self.r_min_depth.capacity(), Content);
        l.many(G, "state planes", Content, self.state.heap_bytes(), self.state.heap_bytes());
        l.vec(G, "wide records", &self.wide, self.wide.capacity(), Content);
        l.vec(G, "handle table", &self.htab, self.htab.capacity(), Content);
        l.vec(G, "handle free list", &self.hfree, self.hfree.capacity(), Content);
        l.vec(G, "insert scratch", &self.scratch, self.scratch.capacity(), Content);
        l.vec(G, "dirty scratch", &self.dirty_scratch, self.dirty_scratch.capacity(), Content);
        l.many(G, "damage ring planes", Window, self.ring.heap_bytes(), self.ring.heap_bytes());
    }
    pub fn window(&self) -> NRect {
        self.ring.window
    }
    /// Mark `kind` as uniform content (solid / tinted rects): its ink changes damage only
    /// `old XOR new` instead of `old UNION new`.
    pub fn set_kind_uniform(&mut self, kind: u8, uniform: bool) {
        self.mgen = self.mgen.wrapping_add(1);
        let (w, b) = ((kind >> 6) as usize, kind & 63);
        if uniform {
            self.uniform[w] |= 1 << b;
        } else {
            self.uniform[w] &= !(1 << b);
        }
    }
    #[inline(always)]
    pub fn kind_uniform(&self, kind: u8) -> bool {
        (self.uniform[(kind >> 6) as usize] >> (kind & 63)) & 1 != 0
    }

    // ------------------------------------------------------------------ clips

    /// A clip rect (absolute) records can reference by id: visible ink = ink ∩ clip.
    pub fn new_clip(&mut self, r: NRect) -> u16 {
        assert!(self.clips.len() < u16::MAX as usize);
        self.mgen = self.mgen.wrapping_add(1);
        self.clips.push(r.canon());
        (self.clips.len() - 1) as u16
    }
    pub fn clip_rect(&self, id: u16) -> NRect {
        self.clips[id as usize]
    }
    /// Move/resize a clip (a scroll container's viewport): damage is `old XOR new`.
    pub fn set_clip_rect(&mut self, id: u16, r: NRect) {
        assert!(id != 0);
        let old = self.clips[id as usize];
        self.mgen = self.mgen.wrapping_add(1);
        self.clips[id as usize] = r.canon();
        self.damage_xor(old, r);
    }
    /// Give the records `[start, start+len)` clip `id`; damages each record's old and new visible ink.
    pub fn set_clip_run(&mut self, start: usize, len: usize, id: u16) {
        if len == 0 {
            return;
        }
        let (mut d, mut s) = self.locate(start);
        let mut rem = len;
        while rem > 0 {
            let l = self.d_len[d] as usize;
            let n = rem.min(l - s);
            for i in s..s + n {
                let old = self.visible(d, i);
                self.page_mut(d).clip[i] = id;
                let new = self.visible(d, i);
                self.ring.add(old);
                self.ring.add(new);
            }
            self.page_mut(d).regroup_range(s, s + n);
            rem -= n;
            d = self.next_dir(d);
            s = 0;
        }
    }
    /// Absolute visible ink of slot `s` of page `d` (canonical).
    #[inline(always)]
    fn visible(&self, d: usize, s: usize) -> NRect {
        self.visible_in(self.page(d), s)
    }
    #[inline(always)]
    fn visible_in(&self, p: &Page, s: usize) -> NRect {
        self.clipped(p.abs(p.ink[s]), p.clip[s])
    }
    /// `r ∩ clips[id]`, canonical; id 0 (unclipped) costs one compare.
    #[inline(always)]
    fn clipped(&self, r: NRect, id: u16) -> NRect {
        if id == 0 {
            return r;
        }
        // SAFETY: clip ids are always < clips.len() (assigned from new_clip).
        r.intersect(unsafe { *self.clips.get_unchecked(id as usize) }).canon()
    }
    /// Page-relative visible inks of group `g` (8 rects, canonical). Out of line: clip-free groups
    /// (the common case) never reach it, and inlining it into the cull loop costs registers.
    #[inline(never)]
    fn group_visible(&self, p: &Page, g: usize) -> [NRect; GROUP] {
        let ids = &p.clip[g * GROUP..g * GROUP + GROUP];
        #[cfg(target_arch = "aarch64")]
        if ids.iter().all(|&c| c == ids[0]) {
            // One clip for the whole group (a scroll container's rows): broadcast it, 4 `smax`,
            // then canonicalise empties lane-wise (x0 + -x1 >= 0 or y0 + -y1 >= 0 -> EMPTY).
            use core::arch::aarch64::*;
            let c = p.rel_sat(unsafe { *self.clips.get_unchecked(ids[0] as usize) });
            let mut v = [NRect::EMPTY; GROUP];
            unsafe {
                let cv = vreinterpretq_s16_u64(vdupq_n_u64(core::mem::transmute::<[i16; 4], u64>(c.0)));
                let maxv = vdupq_n_s16(i16::MAX);
                let src = p.ink.as_ptr().add(g * GROUP) as *const i16;
                for k in 0..4 {
                    let r = vmaxq_s16(vld1q_s16(src.add(k * 8)), cv);
                    let sw = vreinterpretq_s16_s32(vrev64q_s32(vreinterpretq_s32_s16(r)));
                    let nonneg = vcgezq_s16(vqaddq_s16(r, sw)); // lanes 0,1 of each rect: emptiness
                    // any of lanes {0,1} per 64-bit half -> whole half
                    let w = vreinterpretq_u32_u16(nonneg);
                    let e = vreinterpretq_u16_u64(vtstq_u64(vreinterpretq_u64_u32(w), vdupq_n_u64(0xFFFF_FFFF)));
                    vst1q_s16((v.as_mut_ptr() as *mut i16).add(k * 8), vbslq_s16(e, maxv, r));
                }
            }
            return v;
        }
        let mut v = [NRect::EMPTY; GROUP];
        for (l, r) in v.iter_mut().enumerate() {
            let s = g * GROUP + l;
            let c = unsafe { *self.clips.get_unchecked(p.clip[s] as usize) };
            *r = p.ink[s].intersect(p.rel_sat(c)).canon();
        }
        v
    }

    #[inline(always)]
    fn page(&self, d: usize) -> &Page {
        debug_assert!(d < self.d_ptr.len());
        // SAFETY: see module docs; d is a valid directory index.
        unsafe { &**self.d_ptr.get_unchecked(d) }
    }
    #[inline(always)]
    fn page_ptr(&self, d: usize) -> *mut Page {
        debug_assert!(d < self.d_ptr.len());
        unsafe { *self.d_ptr.get_unchecked(d) }
    }
    #[inline(always)]
    fn page_mut(&mut self, d: usize) -> &mut Page {
        debug_assert!(d < self.d_ptr.len());
        // SAFETY: &mut self gives exclusive access to all pool pages.
        let p = unsafe { &mut **self.d_ptr.get_unchecked(d) };
        p.wgen = p.wgen.wrapping_add(1);
        self.mgen = self.mgen.wrapping_add(1);
        p
    }

    /// Directory page + slot of global position `pos` (`pos < len`).
    #[inline(always)]
    fn locate(&self, pos: usize) -> (usize, usize) {
        let (d, s, _) = self.locate_base(pos);
        (d, s)
    }
    /// (directory page, slot, page start).
    #[inline(always)]
    fn locate_base(&self, pos: usize) -> (usize, usize, usize) {
        debug_assert!(pos < self.total, "position {pos} out of range {}", self.total);
        // the block: the last one that starts at or before pos (branchless binary search over the
        // block starts; no iteration while there is one block)
        let (mut lo, mut size) = (0usize, self.r_len.len());
        while size > 1 {
            let half = size / 2;
            let mid = lo + half;
            // SAFETY: mid < nr < r_start.len().
            let m = unsafe { *self.r_start.get_unchecked(mid) } as usize;
            lo = if m <= pos { mid } else { lo };
            size -= half;
        }
        // SAFETY: lo < nr; the block's 64 local starts are in bounds; holes are 0xFFFF.
        unsafe {
            let rb = *self.r_start.get_unchecked(lo) as usize;
            let rel = (pos - rb) as u16;
            let lp = self.d_local.as_ptr().add(lo << RUN_SHIFT);
            // the page inside the block: the number of pages that start at or before rel, minus one
            // (a count over all 64 lanes; a hole's 0xFFFF is never <= rel, which is < 4096)
            #[cfg(target_arch = "aarch64")]
            let cnt = {
                use core::arch::aarch64::*;
                let rv = vdupq_n_u16(rel);
                let mut acc = vdupq_n_u16(0);
                for i in 0..8 {
                    acc = vsubq_u16(acc, vcleq_u16(vld1q_u16(lp.add(i * 8)), rv));
                }
                vaddvq_u16(acc) as usize
            };
            #[cfg(not(target_arch = "aarch64"))]
            let cnt = (0..CHUNK).filter(|&i| *lp.add(i) <= rel).count();
            let d = (lo << RUN_SHIFT) + cnt - 1;
            let base = rb + *self.d_local.get_unchecked(d) as usize;
            (d, pos - base, base)
        }
    }
    #[inline(always)]
    fn start_of(&self, d: usize) -> usize {
        // SAFETY: d is a live directory index.
        unsafe { (*self.r_start.get_unchecked(d >> RUN_SHIFT) + *self.d_local.get_unchecked(d) as u32) as usize }
    }

    pub fn record(&self, pos: usize) -> Record {
        let (d, s) = self.locate(pos);
        self.page(d).read(s)
    }
    pub fn depth_at(&self, pos: usize) -> u16 {
        let (d, s) = self.locate(pos);
        self.page(d).depth[s]
    }
    pub fn ink_at(&self, pos: usize) -> NRect {
        let (d, s) = self.locate(pos);
        let p = self.page(d);
        p.abs(p.ink[s])
    }
    pub fn payload_at(&self, pos: usize) -> u32 {
        let (d, s) = self.locate(pos);
        self.page(d).payload[s]
    }
    pub fn kind_at(&self, pos: usize) -> u8 {
        let (d, s) = self.locate(pos);
        self.page(d).kind[s]
    }
    pub fn records(&self) -> Vec<Record> {
        let mut v = Vec::with_capacity(self.total);
        for d in self.dirs() {
            let p = self.page(d);
            for s in 0..self.d_len[d] as usize {
                v.push(p.read(s));
            }
        }
        v
    }
    /// Start offsets of every directory page (the chunking removal damage is recorded at).
    pub fn page_starts(&self) -> Vec<usize> {
        self.dirs().map(|d| self.start_of(d)).collect()
    }
    pub fn page_versions(&self) -> Vec<(u32, u32)> {
        self.dirs().map(|d| (self.page(d).id, self.page(d).version)).collect()
    }

    // ------------------------------------------------------------------ epochs & damage

    #[inline(always)]
    pub fn epoch(&self) -> u32 {
        self.ring.epoch
    }
    /// Close the current epoch and open the next one.
    pub fn next_epoch(&mut self) {
        self.ring.advance();
    }
    /// Add an arbitrary device-pixel rect to the current epoch's damage.
    #[inline]
    pub fn damage_rect(&mut self, r: NRect) {
        self.ring.add(r);
    }
    /// `damage_rect` for a canonical rect already inside the window (clipped coverage): no clip.
    #[inline(always)]
    pub fn damage_inside(&mut self, r: NRect) {
        self.ring.add_inside(r);
    }
    /// The general rule for a geometry change of a box primitive (DESIGN §2; bough-metal RESULTS Part IV): a
    /// pixel's value depends on edge `e` only within `band` of it, so the pixels that can change are the bands
    /// around the edges that **moved**, never `old ∪ new`. For each lane `i` set in `moved` (bit 0 = x0,
    /// 1 = y0, 2 = x1, 3 = y1) this marks the strip `[min(old_i, new_i) - band, max(old_i, new_i) + band)`
    /// across the union of both inks' perpendicular extent, clipped to clip `clip` (0: none). `old` / `new`
    /// are the primitive's inks (outward-rounded, margin included); `band` is the kind's edge influence +
    /// margin + 1 (rounding slack). Only the writer knows which edges moved at sub-pixel resolution, so it
    /// passes them. Up to 4 strips: a bar that grows marks the strip between its old and new top, not the bar.
    #[inline]
    pub fn damage_edges(&mut self, old: NRect, new: NRect, moved: u8, band: i16, clip: u16) {
        if moved == 0 {
            return;
        }
        let (old, new) = (old.canon(), new.canon());
        let ([ox0, oy0, ox1, oy1], [nx0, ny0, nx1, ny1]) = (old.to_xyxy(), new.to_xyxy());
        // the perpendicular extent: both inks (an empty side contributes nothing; its lanes are meaningless)
        let (ux0, uy0, ux1, uy1) = match (old.is_empty(), new.is_empty()) {
            (true, true) => return,
            (true, false) => (nx0, ny0, nx1, ny1),
            (false, true) => (ox0, oy0, ox1, oy1),
            _ => (ox0.min(nx0), oy0.min(ny0), ox1.max(nx1), oy1.max(ny1)),
        };
        if old.is_empty() || new.is_empty() {
            // appearing or vanishing: everything it covers changes
            let r = self.clipped(NRect::new(ux0, uy0, ux1, uy1), clip);
            self.ring.add(r);
            return;
        }
        let b = band as i32;
        let strip = |lo: i32, hi: i32| (lo.min(hi) - b, lo.max(hi) + b);
        let mut strips = [NRect::EMPTY; 4];
        if moved & 1 != 0 {
            let (a, z) = strip(ox0, nx0);
            strips[0] = NRect::new(a, uy0, z, uy1);
        }
        if moved & 2 != 0 {
            let (a, z) = strip(oy0, ny0);
            strips[1] = NRect::new(ux0, a, ux1, z);
        }
        if moved & 4 != 0 {
            let (a, z) = strip(ox1, nx1);
            strips[2] = NRect::new(a, uy0, z, uy1);
        }
        if moved & 8 != 0 {
            let (a, z) = strip(oy1, ny1);
            strips[3] = NRect::new(ux0, a, ux1, z);
        }
        for r in strips {
            if !r.is_empty() {
                let r = self.clipped(r, clip);
                self.ring.add(r);
            }
        }
    }
    /// `set_ink_handle` for a box primitive whose geometry changed: the record's ink becomes `ink` and the
    /// damage is [`Bough::damage_edges`] of its old and new ink (`moved`, `band` as there), not old ∪ new.
    pub fn set_ink_handle_edges(&mut self, h: Handle, ink: NRect, moved: u8, band: i16) -> bool {
        let Some((d, s)) = self.handle_loc(h) else { return false };
        let p = self.page(d);
        if p.is_xf() {
            // edge bands are a property of the untransformed frame: a transformed record takes its covers
            self.set_ink_at(d, s, ink, true);
            return true;
        }
        let (old, clip) = (p.abs(p.ink[s]), p.clip[s]);
        self.set_ink_at(d, s, ink, false);
        self.damage_edges(old, ink, moved, band, clip);
        true
    }
    /// Mark `r` clipped to clip `clip` (0: none): a writer's own union of [`Bough::damage_edges`] strips.
    #[inline]
    pub fn damage_clipped(&mut self, r: NRect, clip: u16) {
        let r = self.clipped(r.canon(), clip);
        self.ring.add(r);
    }
    /// The clip id of a handle's record (0: unclipped).
    pub fn clip_of_handle(&self, h: Handle) -> Option<u16> {
        let (d, s) = self.handle_loc(h)?;
        Some(self.page(d).clip[s])
    }
    /// Mark the symmetric difference `old XOR new` (as at most 8 strips).
    #[inline]
    pub fn damage_xor(&mut self, old: NRect, new: NRect) {
        let (old, new) = (old.canon(), new.canon());
        if old == new {
            return;
        }
        let ring = &mut self.ring;
        rect_sub(old, new, &mut |r| ring.add(r));
        rect_sub(new, old, &mut |r| ring.add(r));
    }
    /// The current epoch's plane, for writers that produce tile marks themselves (bough-text's
    /// labels OR their tile rows in at update time; [`Plane::or_rows`] keeps its row band exact).
    #[inline(always)]
    pub fn plane_mut(&mut self) -> &mut Plane {
        self.ring.cur_mut()
    }
    pub fn plane(&self, back: u32) -> &Plane {
        self.ring.plane(back)
    }
    /// `repair(age)` into a caller-owned plane; returns its pixel bounding box (EMPTY if clean).
    #[inline]
    pub fn repair_into(&self, age: u32, out: &mut Plane) -> NRect {
        self.ring.repair_into(age, out)
    }
    /// OR of the last `age` planes (1..=4), current epoch included.
    pub fn repair(&self, age: u32) -> Plane {
        self.ring.repair(age)
    }

    // ------------------------------------------------------------------ record state planes

    /// The words of `pp`'s state page were written: summaries, flip lists, and the page's state byte.
    #[inline]
    fn st_settle(&mut self, pp: *mut Page) {
        // SAFETY: pp is a live page of this Bough; &mut self is exclusive.
        let id = unsafe { (*pp).id };
        let sp = self.state.get_mut(id);
        sp.summarise();
        let mut new = sp.changed & !sp.listed;
        sp.listed |= new;
        let blank = sp.is_blank();
        while new != 0 {
            self.flip[new.trailing_zeros() as usize].push(id);
            new &= new - 1;
        }
        unsafe { (*pp).state = !blank as u8 };
    }
    /// Plane `f` of page `pp` becomes `new`: the word, its two summary bits, the flip list and the page's
    /// state byte, without touching the other fifteen planes.
    #[inline(always)]
    fn st_write(&mut self, pp: *mut Page, f: u8, new: u64) {
        // SAFETY: pp is a live page of this Bough; &mut self is exclusive.
        let (id, has) = unsafe { ((*pp).id, (*pp).state != 0) };
        let sp = if has { self.state.get_mut(id) } else { self.state.ensure(id) };
        let (fi, bit) = (f as usize, 1u16 << f);
        if sp.bits[fi] == new {
            return;
        }
        sp.bits[fi] = new;
        let ch = new != sp.seen[fi];
        sp.any = (sp.any & !bit) | ((new != 0) as u16) << f;
        sp.changed = (sp.changed & !bit) | (ch as u16) << f;
        let blank = sp.any | sp.changed == 0;
        if ch && sp.listed & bit == 0 {
            sp.listed |= bit;
            self.flip[fi].push(id);
        }
        unsafe { (*pp).state = !blank as u8 };
    }
    /// The state half of `copy_virtual`: virtual slots `[lo, hi)` of `old[0..s) ++ k new ++ old[s..)` to
    /// `dst[0..]` (new records have no state).
    fn st_copy_virtual(&mut self, dst: *mut Page, lo: usize, hi: usize, old: *mut Page, s: usize, k: usize) {
        // SAFETY: both are live pages; the caller checked `(*old).state != 0`.
        let (di, oi) = unsafe { ((*dst).id, (*old).id) };
        let src = *self.state.get(oi);
        let sp = self.state.ensure(di);
        let b = hi.min(s);
        if lo < b {
            sp.copy(0, &src, lo, b - lo);
        }
        let a = lo.max(s + k);
        if a < hi {
            sp.copy(a - lo, &src, a - k, hi - a);
        }
        self.st_settle(dst);
    }
    /// A freed page keeps no state (its flip-list entries, if any, find nothing changed).
    #[inline]
    fn st_free(&mut self, pp: *mut Page) {
        // SAFETY: a live page about to be freed.
        if unsafe { (*pp).state } != 0 {
            let sp = self.state.get_mut(unsafe { (*pp).id });
            (sp.bits, sp.seen, sp.any, sp.changed) = ([0; STATE_PLANES], [0; STATE_PLANES], 0, 0);
        }
    }
    /// The (bits, seen) of every record of `[start, start + len)`, or `None` if no page of the run has state.
    fn st_read_run(&self, start: usize, len: usize) -> Option<Vec<(u16, u16)>> {
        let (mut d, mut s) = self.locate(start);
        let mut rem = len;
        let mut any = false;
        let mut out = Vec::new();
        while rem > 0 {
            let n = rem.min(self.d_len[d] as usize - s);
            let p = self.page(d);
            if p.state != 0 {
                any = true;
                let sp = self.state.get(p.id);
                out.extend((s..s + n).map(|i| sp.get(i)));
            } else {
                out.extend(core::iter::repeat_n((0, 0), n));
            }
            rem -= n;
            d = self.next_dir(d);
            s = 0;
        }
        any.then_some(out)
    }
    fn st_put(&mut self, pos: usize, v: (u16, u16)) {
        let (d, s) = self.locate(pos);
        let pp = self.page_ptr(d);
        // SAFETY: a live page.
        self.state.ensure(unsafe { (*pp).id }).put(s, v);
        self.st_settle(pp);
    }

    /// Set or clear state plane `f` for the records `[start, start + len)`: one mask write per page.
    pub fn set_state_run(&mut self, start: usize, len: usize, f: u8, on: bool) {
        assert!((f as usize) < STATE_PLANES && start + len <= self.total);
        if len == 0 {
            return;
        }
        let (mut d, mut s) = self.locate(start);
        let mut rem = len;
        while rem > 0 {
            let n = rem.min(self.d_len[d] as usize - s);
            let pp = self.page_ptr(d);
            // SAFETY: a live page; &mut self is exclusive.
            let (id, has) = unsafe { ((*pp).id, (*pp).state != 0) };
            if on || has {
                let m = low(n) << s;
                let w = if has { self.state.get(id).bits[f as usize] } else { 0 };
                self.st_write(pp, f, if on { w | m } else { w & !m });
            }
            rem -= n;
            d = self.next_dir(d);
            s = 0;
        }
    }
    pub fn set_state(&mut self, pos: usize, f: u8, on: bool) {
        self.set_state_run(pos, 1, f, on);
    }
    pub fn set_state_handle(&mut self, h: Handle, f: u8, on: bool) -> bool {
        let Some((pp, s)) = self.handle_ptr(h) else { return false };
        assert!((f as usize) < STATE_PLANES);
        // SAFETY: bound handles point at live pages.
        let (id, has) = unsafe { ((*pp).id, (*pp).state != 0) };
        if on || has {
            let w = if has { self.state.get(id).bits[f as usize] } else { 0 };
            self.st_write(pp, f, if on { w | 1 << s } else { w & !(1 << s) });
        }
        true
    }
    /// Is state `f` set for the record at `pos`?
    pub fn state(&self, pos: usize, f: u8) -> bool {
        let (d, s) = self.locate(pos);
        let p = self.page(d);
        p.state != 0 && self.state.get(p.id).bits[f as usize] >> s & 1 != 0
    }
    pub fn state_handle(&self, h: Handle, f: u8) -> Option<bool> {
        let (pp, s) = self.handle_ptr(h)?;
        // SAFETY: bound handles point at live pages.
        let p = unsafe { &*pp };
        Some(p.state != 0 && self.state.get(p.id).bits[f as usize] >> s & 1 != 0)
    }
    /// The records of `[start, start + len)` with state `f`, per page: `g(page start, mask of slots)`.
    /// Pages without state, or without plane `f` (the page summary), are skipped on one test.
    #[inline]
    fn st_scan(&self, start: usize, len: usize, f: u8, mut g: impl FnMut(usize, u64) -> bool) {
        if len == 0 {
            return;
        }
        let (mut d, mut s, mut base) = self.locate_base(start);
        let mut rem = len;
        while rem > 0 {
            let l = self.d_len[d] as usize;
            let n = rem.min(l - s);
            let p = self.page(d);
            if p.state != 0 {
                let sp = self.state.get(p.id);
                if sp.any >> f & 1 != 0 {
                    let m = sp.bits[f as usize] & (low(n) << s);
                    if m != 0 && g(base, m) {
                        return;
                    }
                }
            }
            rem -= n;
            base += l;
            d = self.next_dir(d);
            s = 0;
        }
    }
    /// Does any record of `[start, start + len)` have state `f`? (focus-within, "anything selected here")
    pub fn any_state(&self, start: usize, len: usize, f: u8) -> bool {
        let mut any = false;
        self.st_scan(start, len, f, |_, _| {
            any = true;
            true
        });
        any
    }
    pub fn count_state(&self, start: usize, len: usize, f: u8) -> usize {
        let mut c = 0;
        self.st_scan(start, len, f, |_, m| {
            c += m.count_ones() as usize;
            false
        });
        c
    }
    /// The first record at or after `pos` with state `f`.
    pub fn next_state(&self, pos: usize, f: u8) -> Option<usize> {
        let mut r = None;
        if pos < self.total {
            self.st_scan(pos, self.total - pos, f, |base, m| {
                r = Some(base + m.trailing_zeros() as usize);
                true
            });
        }
        r
    }
    /// Every record whose state `f` differs from what the plane's consumer last acknowledged
    /// (`commit_state`): `g(position, now)`. Visits only the pages that hold such a record.
    pub fn flips(&self, f: u8, mut g: impl FnMut(usize, bool)) {
        for &id in &self.flip[f as usize] {
            let sp = self.state.get(id);
            if sp.changed >> f & 1 == 0 {
                continue;
            }
            // SAFETY: a page with a changed plane is live (a freed page's state is cleared).
            let base = self.start_of(unsafe { (*self.pool.ptr(id)).dir } as usize);
            let now = sp.bits[f as usize];
            let mut m = now ^ sp.seen[f as usize];
            while m != 0 {
                let s = m.trailing_zeros() as usize;
                m &= m - 1;
                g(base + s, now >> s & 1 != 0);
            }
        }
    }
    /// The consumer of plane `f` has applied its flips.
    pub fn commit_state(&mut self, f: u8) {
        let list = core::mem::take(&mut self.flip[f as usize]);
        for &id in &list {
            let sp = self.state.get_mut(id);
            sp.seen[f as usize] = sp.bits[f as usize];
            sp.listed &= !(1 << f);
            sp.changed &= !(1 << f);
            let blank = sp.is_blank();
            let pp = self.pool.ptr(id);
            // SAFETY: a pool page (valid for the pool's life); a free page's state byte is 0 and stays 0.
            unsafe {
                if (*pp).state != 0 {
                    (*pp).state = !blank as u8;
                }
            }
        }
        let mut list = list;
        list.clear();
        self.flip[f as usize] = list;
    }

    // ------------------------------------------------------------------ directory plumbing

    /// The next live directory entry after `d` in paint order (`dir_len()` after the last).
    #[inline(always)]
    fn next_dir(&self, d: usize) -> usize {
        // the entry after a live one is live, a hole (the block's live entries are over: the next page is
        // the next block's first), or the null after the last block
        let e = d + 1;
        // SAFETY: d is a live directory index and d_ptr has one entry more than the directory.
        if unsafe { *self.d_ptr.get_unchecked(e) }.is_null() { (e + CHUNK - 1) & !(CHUNK - 1) } else { e }
    }
    /// The live entry before `d`.
    #[inline]
    fn prev_dir(&self, d: usize) -> Option<usize> {
        if d & (CHUNK - 1) != 0 {
            return Some(d - 1);
        }
        let r = d >> RUN_SHIFT;
        (r > 0).then(|| ((r - 1) << RUN_SHIFT) + self.r_len[r - 1] as usize - 1)
    }
    /// The last live entry.
    #[inline]
    fn last_dir(&self) -> Option<usize> {
        let nr = self.r_len.len();
        (nr > 0).then(|| ((nr - 1) << RUN_SHIFT) + self.r_len[nr - 1] as usize - 1)
    }
    /// Every live directory index, in paint order.
    fn dirs(&self) -> impl Iterator<Item = usize> + '_ {
        self.r_len.iter().enumerate().flat_map(|(r, &l)| (r << RUN_SHIFT)..(r << RUN_SHIFT) + l as usize)
    }

    /// Recompute the block summaries of the blocks holding directory entries `[lo, hi)`.
    #[inline]
    fn fix_chunks(&mut self, lo: usize, hi: usize) {
        let nr = self.r_len.len();
        if nr == 0 || lo >= hi {
            return;
        }
        for c in (lo >> RUN_SHIFT).min(nr - 1)..=((hi - 1) >> RUN_SHIFT).min(nr - 1) {
            let b = c << RUN_SHIFT;
            self.c_ink[c] = union_all(&self.d_ink[b..b + self.r_len[c] as usize]);
        }
    }
    /// Page `d`'s summary changed from `old` to `self.d_ink[d]`: keep its block's exact.
    #[inline(always)]
    fn chunk_update(&mut self, d: usize, old: NRect) {
        let new = self.d_ink[d];
        if new == old {
            return;
        }
        let c = d >> RUN_SHIFT;
        // SAFETY: c is a block because d is a live index.
        let cs = unsafe { self.c_ink.get_unchecked_mut(c) };
        if new.union(old) == new {
            // grew (or stayed): monotone
            *cs = cs.union(new);
        } else {
            self.fix_chunks(d, d + 1);
        }
    }
    /// Page `d`'s least depth changed: its block's is the least of its 64 entries (holes are 0xFFFF).
    #[inline]
    fn depth_update(&mut self, d: usize) {
        let r = d >> RUN_SHIFT;
        self.r_min_depth[r] = block_min(&self.d_min_depth[r << RUN_SHIFT..(r + 1) << RUN_SHIFT]);
    }

    fn refresh(&mut self, d: usize) {
        let p = self.page_mut(d);
        for g in 0..NGROUPS {
            p.regroup(g);
        }
        let (ink, md) = (p.union_abs(), p.min_depth());
        self.d_ink[d] = ink;
        self.d_min_depth[d] = md;
    }

    /// Page `d` gained (lost) `delta` records and nothing else changed: the pages after it in its block
    /// start `delta` later (one masked lane add over the block: holes keep their sentinel), and so does
    /// every later block (one vector add over the block starts).
    #[inline]
    fn len_delta(&mut self, d: usize, delta: i32) {
        self.mgen = self.mgen.wrapping_add(1);
        let (r, s) = (d >> RUN_SHIFT, d & (CHUNK - 1));
        let b = r << RUN_SHIFT;
        // SAFETY: the block's 64 local starts are in bounds; lanes are touched only past slot s.
        #[cfg(target_arch = "aarch64")]
        unsafe {
            use core::arch::aarch64::*;
            let lp = self.d_local.as_mut_ptr().add(b);
            // the lanes up to the next vector boundary one by one, then whole vectors with the holes
            // masked out (a hole is 0xFFFF and stays it)
            let l = *self.r_len.get_unchecked(r) as usize;
            let mut i = s + 1;
            while i & 7 != 0 && i < l {
                *lp.add(i) = (*lp.add(i)).wrapping_add(delta as u16);
                i += 1;
            }
            let (dv, pad) = (vdupq_n_u16(delta as u16), vdupq_n_u16(LOCAL_PAD));
            while i < l {
                let v = vld1q_u16(lp.add(i));
                vst1q_u16(lp.add(i), vaddq_u16(v, vbicq_u16(dv, vceqq_u16(v, pad))));
                i += 8;
            }
        }
        #[cfg(not(target_arch = "aarch64"))]
        {
            let l = self.r_len[r] as usize;
            for v in &mut self.d_local[b + s + 1..b + l] {
                *v = v.wrapping_add(delta as u16);
            }
        }
        self.r_recs[r] = self.r_recs[r].wrapping_add(delta as u32);
        crate::prefix::add_suffix(&mut self.r_start[r + 1..], delta);
    }
    /// Mark block `r` stale (its derived columns are rebuilt by `dir_settle`).
    #[inline]
    fn pend_run(&mut self, r: usize) {
        self.pend = (self.pend.0.min(r), self.pend.1.max(r));
    }
    /// After a structural edit and the page writes that follow it: rebuild the derived columns of the stale
    /// blocks (local starts, record count, union ink, least depth) from their entries, then the block
    /// starts from the first stale block on (a running sum over the block counts).
    fn dir_settle(&mut self) {
        self.mgen = self.mgen.wrapping_add(1);
        let nr = self.r_len.len();
        let (lo, hi) = (self.pend.0.min(nr), self.pend.1.min(nr.wrapping_sub(1)));
        self.pend = (usize::MAX, 0);
        if lo > hi || nr == 0 {
            if nr == 0 {
                self.r_start[0] = 0;
            }
            return;
        }
        // what the block after the stale ones started at before (blocks that were inserted or removed
        // moved the later starts with them, unchanged)
        let old_next = self.r_start[hi + 1];
        // the first stale block starts where the block before it ends (its own entry may be a fresh
        // block's zero, or what stood there before blocks were removed)
        let mut start = if lo == 0 { 0 } else { self.r_start[lo - 1] + self.r_recs[lo - 1] };
        for r in lo..=hi {
            let b = r << RUN_SHIFT;
            let l = self.r_len[r] as usize;
            // (the holes hold their sentinels already: `dir_holes`)
            let mut acc = 0u32;
            for s in 0..l {
                // SAFETY: b + s is a live entry of block r.
                unsafe {
                    *self.d_local.get_unchecked_mut(b + s) = acc as u16;
                    acc += *self.d_len.get_unchecked(b + s);
                }
            }
            self.r_recs[r] = acc;
            self.r_start[r] = start;
            start += acc;
            self.c_ink[r] = union_all(&self.d_ink[b..b + l]);
            self.r_min_depth[r] = block_min(&self.d_min_depth[b..b + CHUNK]);
        }
        // every later block starts later (earlier) by what the stale blocks gained (lost): one vector add
        crate::prefix::add_suffix(&mut self.r_start[hi + 1..], start.wrapping_sub(old_next) as i32);
    }

    /// Write the sentinel into entries `[lo, hi)` of the directory columns (holes).
    fn dir_holes(&mut self, lo: usize, hi: usize) {
        self.d_ptr[lo..hi].fill(core::ptr::null_mut());
        self.d_id[lo..hi].fill(0);
        self.d_ink[lo..hi].fill(NRect::EMPTY);
        self.d_min_depth[lo..hi].fill(PAD_DEPTH);
        self.d_len[lo..hi].fill(0);
        self.d_local[lo..hi].fill(LOCAL_PAD);
    }
    /// Move `n` entries of the directory columns from `src` to `dst` (memmove) and tell the pages.
    fn dir_move(&mut self, src: usize, dst: usize, n: usize) {
        if n == 0 {
            return;
        }
        self.d_ptr.copy_within(src..src + n, dst);
        self.d_id.copy_within(src..src + n, dst);
        self.d_ink.copy_within(src..src + n, dst);
        self.d_min_depth.copy_within(src..src + n, dst);
        self.d_len.copy_within(src..src + n, dst);
        for i in dst..dst + n {
            // SAFETY: a live page.
            unsafe { (*self.d_ptr[i]).dir = i as u32 };
        }
    }
    /// A fresh blank page at directory entry `d`.
    fn dir_fresh(&mut self, d: usize) {
        let p = self.pool.alloc();
        self.d_ptr[d] = p;
        // SAFETY: a live pool page.
        unsafe {
            self.d_id[d] = (*p).id;
            (*p).dir = d as u32;
        }
        self.d_ink[d] = NRect::EMPTY;
        self.d_min_depth[d] = PAD_DEPTH;
        self.d_len[d] = 0;
    }
    /// Insert `count` blocks of holes before block `r`. The later blocks move up (one memmove per column)
    /// and their pages learn their new index: the one directory operation that is O(pages after), paid when
    /// a block overflows, not on every page split.
    fn blocks_insert(&mut self, r: usize, count: usize) {
        let (at, n) = (r << RUN_SHIFT, count << RUN_SHIFT);
        attr!(2, drop(self.d_ptr.splice(at..at, core::iter::repeat_n(core::ptr::null_mut(), n))));
        attr!(1, self.d_id.insert_n(at, n, 0));
        attr!(3, self.d_ink.insert_n(at, n, NRect::EMPTY));
        attr!(4, drop(self.d_min_depth.splice(at..at, core::iter::repeat_n(PAD_DEPTH, n))));
        attr!(5, {
            drop(self.d_len.splice(at..at, core::iter::repeat_n(0, n)));
            drop(self.d_local.splice(at..at, core::iter::repeat_n(LOCAL_PAD, n)));
        });
        attr!(6, {
            drop(self.r_len.splice(r..r, core::iter::repeat_n(0, count)));
            drop(self.r_recs.splice(r..r, core::iter::repeat_n(0, count)));
            drop(self.r_start.splice(r..r, core::iter::repeat_n(0, count)));
            drop(self.r_min_depth.splice(r..r, core::iter::repeat_n(PAD_DEPTH, count)));
            drop(self.c_ink.splice(r..r, core::iter::repeat_n(NRect::EMPTY, count)));
        });
        attr!(7, self.reindex_from(r + count));
    }
    /// Remove blocks `[r, r + count)` (all holes by now).
    fn blocks_remove(&mut self, r: usize, count: usize) {
        let (at, n) = (r << RUN_SHIFT, count << RUN_SHIFT);
        let len = self.d_ink.len();
        attr!(2, drop(self.d_ptr.drain(at..at + n)));
        attr!(1, {
            self.d_id.copy_within(at + n..len, at);
            self.d_id.truncate(len - n);
        });
        attr!(3, {
            self.d_ink.copy_within(at + n..len, at);
            self.d_ink.truncate(len - n);
        });
        attr!(4, drop(self.d_min_depth.drain(at..at + n)));
        attr!(5, {
            drop(self.d_len.drain(at..at + n));
            drop(self.d_local.drain(at..at + n));
        });
        attr!(6, {
            drop(self.r_len.drain(r..r + count));
            drop(self.r_recs.drain(r..r + count));
            drop(self.r_start.drain(r..r + count));
            drop(self.r_min_depth.drain(r..r + count));
            drop(self.c_ink.drain(r..r + count));
        });
        attr!(7, self.reindex_from(r));
    }
    /// The pages of blocks `r..` learn their directory index again.
    fn reindex_from(&mut self, r: usize) {
        for rr in r..self.r_len.len() {
            let b = rr << RUN_SHIFT;
            for i in b..b + self.r_len[rr] as usize {
                // SAFETY: a live page.
                unsafe { (*self.d_ptr[i]).dir = i as u32 };
            }
        }
    }

    /// Insert `count` blank, empty pages after directory entry `prev` in paint order (`None`: at the very
    /// front). Returns the index of the first; the others follow it (`next_dir`). The entries after `prev`
    /// in its block move up inside the block. A block that would exceed `CHUNK` is cut: its pages and the
    /// new ones are laid out again over as many blocks as they need (at the end of the directory filled to
    /// `RUN_FILL`, anywhere else evenly), which inserts blocks after it. The blocks touched are left stale
    /// (`pend`): the caller writes the pages, then `dir_settle`.
    fn dir_insert_after(&mut self, prev: Option<usize>, count: usize) -> usize {
        self.mgen = self.mgen.wrapping_add(1);
        if self.r_len.is_empty() {
            self.blocks_insert(0, 1);
        }
        let (r, s) = match prev {
            None => (0, 0),
            Some(p) => (p >> RUN_SHIFT, (p & (CHUNK - 1)) + 1),
        };
        let len = self.r_len[r] as usize;
        let b = r << RUN_SHIFT;
        self.np += count;
        if len + count <= CHUNK {
            attr!(1, self.dir_move(b + s, b + s + count, len - s));
            attr!(0, for k in 0..count {
                self.dir_fresh(b + s + k);
            });
            self.r_len[r] = (len + count) as u32;
            self.pend_run(r);
            return b + s;
        }
        let nr = self.r_len.len();
        if r + 1 == nr && s == len && count <= CHUNK {
            // an append at the very end: fresh blocks after the last one (nothing moves)
            let q = count.div_ceil(RUN_FILL);
            self.blocks_insert(nr, q);
            for j in 0..q {
                let (rb, sz) = ((nr + j) << RUN_SHIFT, RUN_FILL.min(count - j * RUN_FILL));
                for t in 0..sz {
                    self.dir_fresh(rb + t);
                }
                self.r_len[nr + j] = sz as u32;
                self.pend_run(nr + j);
            }
            return nr << RUN_SHIFT;
        }
        // the block overflows: its pages and the new ones are laid out evenly over as many blocks as they
        // need at `RUN_FILL`, inserted right after it. The later blocks move up once and their pages learn
        // their index: the directory's one O(pages after) event, once per block's worth of headroom.
        // (Measured against the alternative of rebalancing a doubling window of neighbours, a packed-memory
        // array: with blocks this full the windows rebalance too often; 256 page splits at the front of
        // 1e6 records cost 52.7 k each amortised and 1.2 M at worst that way, against the cut's numbers in
        // TABLE-R6.)
        let q = (len + count).div_ceil(RUN_FILL);
        self.blocks_insert(r + 1, q - 1);
        self.relayout(r, r + q, b + s, count)
    }

    /// Lay the pages of blocks `[lo, hi)` out again, evenly, with `count` fresh blank pages inserted
    /// before the entry now at directory index `at` (or at the end of its block's live entries). Every
    /// page in the window learns its index; the blocks are left stale. Returns the index of the first new
    /// page.
    fn relayout(&mut self, lo: usize, hi: usize, at: usize, count: usize) -> usize {
        let mut old: Vec<(*mut Page, u32, NRect, u16, u32)> = Vec::new();
        let mut ins = 0usize;
        for r in lo..hi {
            let b = r << RUN_SHIFT;
            let l = self.r_len[r] as usize;
            if at >= b && at <= b + l && at >> RUN_SHIFT == r {
                ins = old.len() + (at - b);
            }
            old.extend((b..b + l).map(|i| (self.d_ptr[i], self.d_id[i], self.d_ink[i], self.d_min_depth[i], self.d_len[i])));
        }
        let total = old.len() + count;
        let q = hi - lo;
        debug_assert!(total <= q * CHUNK && total >= q);
        let mut v = 0usize;
        let mut first = 0usize;
        for j in 0..q {
            let rb = (lo + j) << RUN_SHIFT;
            let sz = total / q + usize::from(j < total % q);
            for t in 0..sz {
                let d = rb + t;
                if v >= ins && v < ins + count {
                    if v == ins {
                        first = d;
                    }
                    self.dir_fresh(d);
                } else {
                    let e = old[if v < ins { v } else { v - count }];
                    (self.d_ptr[d], self.d_id[d], self.d_ink[d], self.d_min_depth[d], self.d_len[d]) = e;
                    // SAFETY: a live page.
                    unsafe { (*e.0).dir = d as u32 };
                }
                v += 1;
            }
            self.dir_holes(rb + sz, rb + CHUNK);
            self.r_len[lo + j] = sz as u32;
            self.pend_run(lo + j);
        }
        first
    }

    /// Remove the `k` directory entries from `d` on in paint order (whole pages emptied by one
    /// `remove_run`, possibly across blocks) and free their pages. Inside a block the entries after them
    /// move down; a block left empty goes (the later blocks move down); a block left small is merged into
    /// its neighbour when both fit in half a block. The blocks touched are left stale: `dir_settle`.
    fn dir_remove_seq(&mut self, d: usize, k: usize) {
        if k == 0 {
            return;
        }
        self.mgen = self.mgen.wrapping_add(1);
        let r0 = d >> RUN_SHIFT;
        let (mut r, mut s, mut rem) = (r0, d & (CHUNK - 1), k);
        while rem > 0 {
            let len = self.r_len[r] as usize;
            let n = rem.min(len - s);
            let b = r << RUN_SHIFT;
            for i in b + s..b + s + n {
                // SAFETY: live page.
                let (id, fl) = unsafe { ((*self.d_ptr[i]).id, (*self.d_ptr[i]).flags) };
                if fl & F_WIDE != 0 {
                    self.wide_drop(id);
                }
                self.st_free(self.d_ptr[i]);
                attr!(0, self.pool.free(id));
            }
            attr!(1, self.dir_move(b + s + n, b + s, len - s - n));
            self.dir_holes(b + len - n, b + len);
            self.r_len[r] = (len - n) as u32;
            rem -= n;
            r += 1;
            s = 0;
        }
        self.np -= k;
        // blocks r0..r were touched; the emptied ones are contiguous among them
        let lo = if self.r_len[r0] == 0 { r0 } else { r0 + 1 };
        let hi = if self.r_len[r - 1] == 0 { r } else { r - 1 };
        if lo < hi {
            self.blocks_remove(lo, hi - lo);
        }
        // the survivors next to the cut: blocks lo - 1 and lo (when they exist)
        let nr = self.r_len.len();
        if lo > 0 && lo < nr && (self.r_len[lo - 1] + self.r_len[lo]) as usize <= CHUNK / 2 {
            let (a, c) = (self.r_len[lo - 1] as usize, self.r_len[lo] as usize);
            let (src, dst) = (lo << RUN_SHIFT, ((lo - 1) << RUN_SHIFT) + a);
            self.dir_move(src, dst, c);
            self.dir_holes(src, src + c);
            self.r_len[lo - 1] = (a + c) as u32;
            self.r_len[lo] = 0;
            self.blocks_remove(lo, 1);
        }
        let nr = self.r_len.len();
        if nr > 0 {
            self.pend_run(lo.saturating_sub(1).min(nr - 1));
            self.pend_run(lo.min(nr - 1));
        } else {
            self.pend = (0, 0);
        }
    }

    /// Re-point handle-table entries for every live handle in slots `[lo, hi)` of page `d`.
    #[inline]
    fn fixup_handles(&mut self, d: usize, lo: usize, hi: usize) {
        let p = self.page_ptr(d);
        // SAFETY: p is a live page; the handle table is a separate allocation.
        let col = unsafe { &(*p).handle };
        let any = col[lo..hi].iter().fold(0u32, |a, &b| a | b) != 0;
        if any {
            for s in lo..hi {
                let h = col[s];
                if h != 0 {
                    let e = &mut self.htab[h as usize];
                    e.page = p;
                    e.slot = s as u32;
                }
            }
        }
    }

    // ------------------------------------------------------------------ handles

    pub fn new_handle(&mut self) -> Handle {
        if let Some(idx) = self.hfree.pop() {
            let e = &mut self.htab[idx as usize];
            e.live = true;
            e.page = core::ptr::null_mut();
            Handle { idx, generation: e.generation }
        } else {
            let idx = self.htab.len() as u32;
            self.htab.push(HEntry { page: core::ptr::null_mut(), slot: 0, generation: 0, live: true });
            Handle { idx, generation: 0 }
        }
    }
    fn release_handle(&mut self, idx: u32) {
        let e = &mut self.htab[idx as usize];
        debug_assert!(e.live);
        e.live = false;
        e.page = core::ptr::null_mut();
        e.generation = e.generation.wrapping_add(1);
        self.hfree.push(idx);
    }
    /// (page pointer, slot) of a bound, live handle.
    #[inline(always)]
    fn handle_ptr(&self, h: Handle) -> Option<(*mut Page, usize)> {
        let e = self.htab.get(h.idx as usize)?;
        if !e.live || e.generation != h.generation || e.page.is_null() {
            return None;
        }
        Some((e.page, e.slot as usize))
    }
    #[inline(always)]
    fn handle_loc(&self, h: Handle) -> Option<(usize, usize)> {
        let (p, s) = self.handle_ptr(h)?;
        // SAFETY: bound handles point at live pages.
        Some((unsafe { (*p).dir } as usize, s))
    }
    pub fn pos_of(&self, h: Handle) -> Option<usize> {
        let (d, s) = self.handle_loc(h)?;
        Some(self.start_of(d) + s)
    }
    pub fn handle_live(&self, h: Handle) -> bool {
        self.htab.get(h.idx as usize).is_some_and(|e| e.live && e.generation == h.generation)
    }
    /// Allocate a handle for the existing record at `pos` (a binding destination).
    pub fn attach_handle(&mut self, pos: usize) -> Handle {
        let (d, s) = self.locate(pos);
        assert_eq!(self.page(d).handle[s], 0, "record already has a handle");
        let h = self.new_handle();
        self.page_mut(d).handle[s] = h.idx;
        self.fixup_handles(d, s, s + 1);
        h
    }
    /// Release the handle of the record at `pos` (the record stays, addressable by position only).
    pub fn unbind_handle_at(&mut self, pos: usize) {
        let (d, s) = self.locate(pos);
        let p = self.page_mut(d);
        let h = core::mem::take(&mut p.handle[s]);
        if h != 0 {
            self.release_handle(h);
        }
    }

    // ------------------------------------------------------------------ insert

    /// Insert a run (in valid preorder) so its first record lands at global index `pos`.
    pub fn insert_run(&mut self, pos: usize, recs: &[RecordInit]) {
        self.insert_impl(pos, recs, true);
    }

    /// `insert_run` with exact rects: record `i`'s ink is `wide[i]` (any i32 device-pixel rect;
    /// `recs[i].ink` is ignored). For content outside the 16-bit domain: the rows of a long scrolled
    /// column, a document taller than 32,766 px. Such records live in pages with their own offset, and
    /// translating them later is exact.
    pub fn insert_run_wide(&mut self, pos: usize, recs: &[RecordInit], wide: &[WRect]) {
        assert_eq!(recs.len(), wide.len());
        self.insert_core(pos, recs, Some(wide), true);
    }

    fn insert_impl(&mut self, pos: usize, recs: &[RecordInit], damage: bool) {
        self.insert_core(pos, recs, None, damage);
    }

    #[inline]
    fn insert_core(&mut self, pos: usize, recs: &[RecordInit], wide: Option<&[WRect]>, damage: bool) {
        let k = recs.len();
        if k == 0 {
            return;
        }
        assert!(pos <= self.total, "insert position out of range");
        if damage {
            match wide {
                None => {
                    for chunk in recs.chunks(CAP) {
                        let mut u = NRect::EMPTY;
                        for r in chunk {
                            u = u.union(self.clipped(r.ink.canon(), r.clip));
                        }
                        self.ring.add(u);
                    }
                }
                Some(w) => {
                    for (chunk, wc) in recs.chunks(CAP).zip(w.chunks(CAP)) {
                        let mut u = NRect::EMPTY;
                        for (r, w) in chunk.iter().zip(wc) {
                            u = u.union(self.clipped(w.to_nrect(), r.clip));
                        }
                        self.ring.add(u);
                    }
                }
            }
        }
        if self.np == 0 {
            self.dir_insert_after(None, 1);
            self.dir_settle();
        }
        // Insert after the record at pos-1 (same page); at 0, at the front of page 0.
        let (d, s) = if pos == 0 {
            (0, 0)
        } else {
            let (d, s) = self.locate(pos - 1);
            (d, s + 1)
        };
        {
            let p = self.page(d);
            if wide.is_some() || p.off != [0, 0] || p.flags != 0 {
                // not the plain frame (stored = absolute): restate the records in the page's frame, or
                // give them pages of their own
                return self.insert_framed(pos, d, s, recs, wide);
            }
        }
        let l = self.d_len[d] as usize;
        if l + k <= CAP {
            self.insert_in_page(d, s, recs);
        } else {
            self.insert_split(d, s, recs);
        }
        self.total += k;
    }

    /// An insert at slot `s` of page `d` when the page's frame is not the identity or the records carry
    /// exact rects. A transformed page takes the records as before (its offset folded away). A plain
    /// page takes them restated relative to its offset when every one is inside the domain and fits the
    /// frame. Anything else (records outside the domain, a far or wide page) goes to pages of its own.
    #[inline(never)]
    fn insert_framed(&mut self, pos: usize, d: usize, s: usize, recs: &[RecordInit], wide: Option<&[WRect]>) {
        let k = recs.len();
        let l = self.d_len[d] as usize;
        let flags = self.page(d).flags;
        if flags & F_XF != 0 && wide.is_none() {
            self.page_mut(d).normalize();
            if l + k <= CAP {
                self.insert_in_page(d, s, recs);
            } else {
                self.insert_split(d, s, recs);
            }
            self.total += k;
            return;
        }
        if flags & (F_XF | F_WIDE) == 0 {
            // a plain page takes records inside the domain that fit its frame; a far page takes any that
            // fit its frame and keep its frame rule (blank records, the next rows of its column)
            let mut rel = core::mem::take(&mut self.rel_scratch);
            rel.clear();
            let p = self.page(d);
            let mut u = if flags == 0 { WRect::EMPTY } else { p.abs_wide(p.union_ink()) };
            for (i, r) in recs.iter().enumerate() {
                let w = wide.map_or(WRect::from(r.ink.canon()), |w| w[i]);
                match p.rel_of(w) {
                    Some(ink) if flags != 0 || w.in_domain() => {
                        rel.push(RecordInit { ink, ..*r });
                        u = u.union(w);
                    }
                    _ => break,
                }
            }
            let off = p.off;
            let fits = rel.len() == k && (flags == 0 || (0..2).all(|a| off[a].abs() <= 32767 || u.is_empty() || u.0[a] > COORD_MAX || u.0[a + 2] < COORD_MIN));
            if fits {
                if l + k <= CAP {
                    self.insert_in_page(d, s, &rel);
                } else {
                    self.insert_split(d, s, &rel);
                }
                self.total += k;
            }
            rel.clear();
            self.rel_scratch = rel;
            if fits {
                return;
            }
        }
        let rects: Vec<WRect> = recs.iter().enumerate().map(|(i, r)| wide.map_or(WRect::from(r.ink.canon()), |w| w[i])).collect();
        self.insert_cold(pos, recs, &rects);
    }

    /// The frame (offset, flags) of a page whose records' exact union is `u`: plain (`keep` if it still
    /// fits, else 0) when `u` is inside the domain; otherwise a far frame, the offset nearest zero that
    /// puts every coordinate in 16 bits, valid only if on each axis `|off| <= 32767` or `u` lies wholly
    /// beyond the domain there (so a page any window can meet always has exact 16-bit offset lanes).
    /// `None`: no such frame (the span is over 65,532 px, or straddles too far): cut the page, or, for a
    /// single record, keep its exact rect aside (`F_WIDE`).
    fn frame_for(u: WRect, keep: [i32; 2]) -> Option<([i32; 2], u8)> {
        let small = |o: [i32; 2]| o[0].abs() <= 32767 && o[1].abs() <= 32767;
        if u.is_empty() {
            return Some((if small(keep) { keep } else { [0, 0] }, 0));
        }
        if u.in_domain() {
            let fits = |o: [i32; 2]| (0..4).all(|i| (COORD_MIN..=COORD_MAX).contains(&(u.0[i] - o[i & 1])));
            return Some((if small(keep) && fits(keep) { keep } else { [0, 0] }, 0));
        }
        let mut off = [0i32; 2];
        for a in 0..2 {
            let (lo, hi) = (u.0[a], u.0[a + 2]);
            // offsets that put both edges in 16 bits; on an axis where the union is not wholly beyond the
            // domain the offset itself must be in 16 bits too
            let (mut min, mut max) = (hi - COORD_MAX, lo - COORD_MIN);
            if !(lo > COORD_MAX || hi < COORD_MIN) {
                (min, max) = (min.max(-32767), max.min(32767));
            }
            if min > max {
                return None;
            }
            // the middle of the union, so later records on either side (the next rows of a column) still fit
            off[a] = (lo + (hi - lo) / 2).clamp(min, max);
        }
        Some((off, F_FAR))
    }

    /// Cut `rects` (records in order) into runs of at most `CAP` that each have a frame; a record that
    /// has none on its own is a run of one (wide).
    fn frame_runs(rects: &[WRect]) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        let mut a = 0;
        while a < rects.len() {
            let mut u = rects[a];
            let mut b = a + 1;
            if Self::frame_for(u, [0, 0]).is_some() {
                while b < rects.len() && b - a < CAP {
                    let v = u.union(rects[b]);
                    if Self::frame_for(v, [0, 0]).is_none() {
                        break;
                    }
                    u = v;
                    b += 1;
                }
            }
            out.push((a, b));
            a = b;
        }
        out
    }

    /// Exact rect of slot `s` of page `d` (on a transformed page: the transformed bounding box).
    fn slot_wide(&self, d: usize, s: usize) -> WRect {
        let p = self.page(d);
        if p.flags & F_WIDE != 0 {
            return self.wide.iter().find(|e| e.0 == p.id).expect("wide page has its rect").1;
        }
        if p.flags & F_XF != 0 {
            return WRect::from(p.abs(p.ink[s]));
        }
        p.abs_wide(p.ink[s])
    }
    fn page_rects(&self, d: usize) -> Vec<WRect> {
        (0..self.d_len[d] as usize).map(|s| self.slot_wide(d, s)).collect()
    }
    /// The exact ink of the record at `pos`.
    pub fn ink_wide_at(&self, pos: usize) -> WRect {
        let (d, s) = self.locate(pos);
        self.slot_wide(d, s)
    }
    fn wide_drop(&mut self, id: u32) {
        self.wide.retain(|e| e.0 != id);
    }

    /// Write the inks of page `d`'s live slots from their exact rects, choosing its frame; a page whose
    /// records have no common frame is cut into pages that do. Returns the number of directory pages
    /// the page became. (The exact path: far content only.)
    fn reencode(&mut self, d: usize, rects: &[WRect]) -> usize {
        let l = self.d_len[d] as usize;
        debug_assert_eq!(l, rects.len());
        let u = rects.iter().fold(WRect::EMPTY, |u, r| u.union(*r));
        let (keep, id, was_wide) = { let p = self.page(d); (p.off, p.id, p.flags & F_WIDE != 0) };
        let frame = Self::frame_for(u, keep);
        if frame.is_none() && l > 1 {
            let runs = Self::frame_runs(rects);
            let j = runs[0].1;
            // (by position: a cut may lay the block's entries out again)
            let pos = self.start_of(d);
            self.split_at(pos + j);
            let a = self.reencode(self.locate(pos).0, &rects[..j]);
            return a + self.reencode(self.locate(pos + j).0, &rects[j..]);
        }
        if was_wide {
            self.wide_drop(id);
        }
        let p = self.page_mut(d);
        p.version += 1;
        match frame {
            Some((off, fl)) => {
                p.set_off(off[0], off[1]);
                p.flags = fl;
                for (s, r) in rects.iter().enumerate() {
                    p.ink[s] = p.rel_of(*r).expect("the frame holds every record");
                }
            }
            None => {
                p.set_off(0, 0);
                p.flags = F_WIDE;
                p.ink[0] = rects[0].to_nrect();
                self.wide.push((id, rects[0]));
            }
        }
        let p = self.page_mut(d);
        for g in 0..NGROUPS {
            p.regroup(g);
        }
        self.d_ink[d] = self.page(d).union_abs();
        self.fix_chunks(d, d + 1);
        1
    }

    /// Records that fit no existing page's frame: `pos` is made a page boundary and the records get
    /// pages of their own, cut where their frames change.
    fn insert_cold(&mut self, pos: usize, recs: &[RecordInit], rects: &[WRect]) {
        let k = recs.len();
        if self.total == 0 && self.np != 0 {
            // the blank page a first insert opened: these records bring their own
            self.dir_remove_seq(0, 1);
            self.dir_settle();
        }
        self.split_at(pos);
        let prev = if pos == self.total { self.last_dir() } else { self.prev_dir(self.locate(pos).0) };
        let runs = Self::frame_runs(rects);
        let mut d = self.dir_insert_after(prev, runs.len());
        let mut pages = Vec::with_capacity(runs.len());
        for &(a, b) in &runs {
            let p = self.page_mut(d);
            p.version += 1;
            let mut md = PAD_DEPTH;
            for (i, r) in recs[a..b].iter().enumerate() {
                p.write(i, &RecordInit { ink: NRect::EMPTY, ..*r });
                md = md.min(r.depth);
            }
            self.d_len[d] = (b - a) as u32;
            self.d_min_depth[d] = md;
            self.fixup_handles(d, 0, b - a);
            pages.push(self.page_ptr(d));
            d = self.next_dir(d);
        }
        self.dir_settle();
        self.total += k;
        for (&pp, &(a, b)) in pages.iter().zip(&runs) {
            // SAFETY: a live page (reencode of an earlier one moves no entry: each has a frame).
            let n = self.reencode(unsafe { (*pp).dir } as usize, &rects[a..b]);
            debug_assert_eq!(n, 1);
        }
    }

    fn insert_in_page(&mut self, d: usize, s: usize, recs: &[RecordInit]) {
        let l = self.d_len[d] as usize;
        let k = recs.len();
        let p = self.page_mut(d);
        p.version += 1;
        if s < l {
            p.shift(s, s + k, l - s);
        }
        let mut md = PAD_DEPTH;
        for (i, r) in recs.iter().enumerate() {
            p.write(s + i, r);
            md = md.min(r.depth);
        }
        p.regroup_range(s, l + k);
        let u = p.union_abs();
        if p.state != 0 {
            // the records above the gap take their state with them; the new ones have none
            let (pp, id) = (p as *mut Page, p.id);
            let sp = self.state.get_mut(id);
            if s < l {
                sp.shift(s, s + k, l - s);
            }
            sp.clear(s, s + k);
            self.st_settle(pp);
        }
        self.fixup_handles(d, s, l + k);
        self.d_len[d] = (l + k) as u32;
        let old = self.d_ink[d];
        self.d_ink[d] = u;
        if md < self.d_min_depth[d] {
            self.d_min_depth[d] = md;
            let r = d >> RUN_SHIFT;
            self.r_min_depth[r] = self.r_min_depth[r].min(md);
        }
        self.chunk_update(d, old);
        self.len_delta(d, k as i32);
    }

    fn insert_split(&mut self, d: usize, s: usize, recs: &[RecordInit]) {
        let l = self.d_len[d] as usize;
        let k = recs.len();
        let t = l + k;
        let m = t.div_ceil(CAP);
        let sizes = Sizes::new(t, m, s, l);
        // (the block may be laid out again: page d is found by its pointer afterwards)
        let dp = self.page_ptr(d);
        let first = self.dir_insert_after(Some(d), m - 1);
        // SAFETY: a live page.
        let d = unsafe { (*dp).dir } as usize;
        debug_assert_eq!(self.next_dir(d), first);
        let sz0 = sizes.size(0);
        let mut off = sz0;
        let mut di = d;
        for i in 1..m {
            let szi = sizes.size(i);
            di = self.next_dir(di);
            // SAFETY: di != d, both live distinct pool pages.
            let (newp, oldp) = unsafe { (&mut *self.page_ptr(di), &*self.page_ptr(d)) };
            newp.wgen = newp.wgen.wrapping_add(1);
            // the new pages share the old one's frame: stored inks are copied as they are
            (newp.xm, newp.xv) = (oldp.xm, oldp.xv);
            newp.set_off(oldp.off[0], oldp.off[1]);
            newp.flags = oldp.flags;
            copy_virtual(newp, off, off + szi, oldp, s, k, recs);
            newp.version += 1;
            if oldp.state != 0 {
                self.st_copy_virtual(self.page_ptr(di), off, off + szi, self.page_ptr(d), s, k);
            }
            self.d_len[di] = szi as u32;
            self.fixup_handles(di, 0, szi);
            off += szi;
        }
        debug_assert_eq!(off, t);
        // Page d keeps virtual [0, sz0).
        let p = self.page_mut(d);
        p.version += 1;
        if sz0 <= s {
            p.set_sentinel(sz0, CAP);
        } else {
            let tail_n = sz0.saturating_sub(s + k);
            if tail_n > 0 {
                p.shift(s, s + k, tail_n);
            }
            p.set_sentinel(sz0, CAP);
            let nrec = k.min(sz0 - s);
            for (i, r) in recs.iter().take(nrec).enumerate() {
                p.write(s + i, r);
            }
        }
        if p.state != 0 {
            let (pp, id) = (p as *mut Page, p.id);
            let sp = self.state.get_mut(id);
            if sz0 > s {
                let tail_n = sz0.saturating_sub(s + k);
                if tail_n > 0 {
                    sp.shift(s, s + k, tail_n);
                }
                sp.clear(s, (s + k).min(sz0));
            }
            sp.clear(sz0, CAP);
            self.st_settle(pp);
        }
        self.d_len[d] = sz0 as u32;
        if sz0 > s {
            self.fixup_handles(d, s, sz0);
        }
        let mut di = d;
        for _ in 0..m {
            self.refresh(di);
            self.pend_run(di >> RUN_SHIFT);
            di = self.next_dir(di);
        }
        attr!(8, self.dir_settle());
    }

    // ------------------------------------------------------------------ remove

    /// Remove the run `[start, start+len)`. Handles inside are released.
    pub fn remove_run(&mut self, start: usize, len: usize) {
        self.remove_impl(start, len, true, true);
    }

    fn remove_impl(&mut self, start: usize, len: usize, release: bool, damage: bool) {
        if len == 0 {
            return;
        }
        assert!(start + len <= self.total, "remove range out of bounds");
        let (d0, mut s) = self.locate(start);
        let mut d = d0;
        let mut remaining = len;
        let d0_full = s == 0 && len >= self.d_len[d0] as usize;
        let mut freed = false;
        let mut trimmed: [(usize, usize); 4] = [(0, 0); 4];
        let mut ntrim = 0usize;
        // whole pages emptied: one contiguous directory range, removed after the walk
        let (mut full_at, mut full_n) = (usize::MAX, 0usize);
        while remaining > 0 {
            let l = self.d_len[d] as usize;
            let n = remaining.min(l - s);
            if damage {
                let p = self.page(d);
                let u = if p.cgroups != 0 {
                    (s..s + n).fold(NRect::EMPTY, |u, i| u.union(self.visible_in(p, i)))
                } else if s == 0 && n == l {
                    self.d_ink[d]
                } else {
                    p.abs(union_all(&p.ink[s..s + n]))
                };
                self.ring.add(u);
            }
            if release {
                let ptr = self.page_ptr(d);
                // SAFETY: live page; release_handle touches only the handle table.
                let col = unsafe { &(*ptr).handle };
                if col[s..s + n].iter().fold(0u32, |a, &b| a | b) != 0 {
                    for i in s..s + n {
                        if col[i] != 0 {
                            self.release_handle(col[i]);
                        }
                    }
                }
            }
            if n == l {
                freed = true;
                if full_n == 0 {
                    full_at = d;
                }
                full_n += 1;
                d = self.next_dir(d);
            } else {
                if ntrim < 4 {
                    trimmed[ntrim] = (d, n);
                }
                ntrim += 1;
                let p = self.page_mut(d);
                p.version += 1;
                if s + n < l {
                    p.shift(s + n, s, l - s - n);
                }
                p.set_sentinel(l - n, CAP);
                p.regroup_range(s, l);
                if p.state != 0 {
                    let (pp, id) = (p as *mut Page, p.id);
                    let sp = self.state.get_mut(id);
                    if s + n < l {
                        sp.shift(s + n, s, l - s - n);
                    }
                    sp.clear(l - n, CAP);
                    self.st_settle(pp);
                }
                self.d_len[d] = (l - n) as u32;
                let old = self.d_ink[d];
                self.d_ink[d] = self.page(d).union_abs();
                let md = self.page(d).min_depth();
                let md_changed = md != self.d_min_depth[d];
                self.d_min_depth[d] = md;
                if !freed {
                    self.chunk_update(d, old);
                    if md_changed {
                        self.depth_update(d);
                    }
                }
                self.pend_run(d >> RUN_SHIFT);
                if s + n < l {
                    self.fixup_handles(d, s, l - n);
                }
                d = self.next_dir(d);
            }
            s = 0;
            remaining -= n;
        }
        // the page the merges are tried around: the first one touched, if it was only trimmed; and the page
        // after the last one touched, if there is one
        let anchor0 = if d0_full { core::ptr::null_mut() } else { self.page_ptr(d0) };
        let after = if d < self.dir_len() { self.page_ptr(d) } else { core::ptr::null_mut() };
        self.total -= len;
        if !freed && ntrim <= 4 {
            // Pure trims: add each page's negative delta instead of rebuilding (the blocks are not stale).
            self.pend = (usize::MAX, 0);
            for &(td, n) in &trimmed[..ntrim] {
                self.len_delta(td, -(n as i32));
            }
        } else {
            self.dir_remove_seq(full_at, full_n);
            attr!(8, self.dir_settle());
        }
        // Underflow: merge small neighbours around the cut: the first page touched if it survives (else
        // the page after the cut, else the last page), its successor and its predecessor, each with the
        // page after it (pages are named by pointer: a merge moves directory entries).
        if self.np == 0 {
            return;
        }
        // SAFETY: both are live pages when they are used (anchor0 survived; after was not touched).
        let x = if !anchor0.is_null() {
            unsafe { (*anchor0).dir as usize }
        } else if !after.is_null() {
            unsafe { (*after).dir as usize }
        } else {
            self.last_dir().unwrap()
        };
        let mut cand = [core::ptr::null_mut::<Page>(); 3];
        let nx = self.next_dir(x);
        if nx < self.dir_len() {
            cand[0] = self.page_ptr(nx);
        }
        cand[1] = self.page_ptr(x);
        if let Some(pv) = self.prev_dir(x) {
            cand[2] = self.page_ptr(pv);
        }
        for p in cand {
            if p.is_null() {
                continue;
            }
            // SAFETY: a candidate is freed only as the second of a pair, after its own turn.
            let i = unsafe { (*p).dir } as usize;
            let e = self.next_dir(i);
            if e < self.dir_len() {
                let (a, b) = (self.d_len[i] as usize, self.d_len[e] as usize);
                // pages of different frames or transforms never merge
                if a + b <= CAP && (a < MIN_FILL || b < MIN_FILL) && self.mergeable(i, e) {
                    self.merge_next(i);
                }
            }
        }
    }

    /// May pages `i` and `i + 1` become one? Plain pages when the second's ink fits the first's frame
    /// (always, when neither has an offset); transformed pages of the same transform; never a far or
    /// wide page (it keeps its own frame).
    #[inline]
    fn mergeable(&self, i: usize, e: usize) -> bool {
        let (p, q) = (self.page(i), self.page(e));
        match (p.flags, q.flags) {
            (0, 0) => p.off == q.off || self.d_ink[e].translate_checked(-p.off[0], -p.off[1]).is_some(),
            (F_XF, F_XF) => p.xm == q.xm && p.xv == q.xv,
            _ => false,
        }
    }

    fn merge_next(&mut self, d: usize) {
        let e = self.next_dir(d);
        let (la, lb) = (self.d_len[d] as usize, self.d_len[e] as usize);
        {
            // SAFETY: d and d+1 are distinct live pages.
            let (a, b) = unsafe { (&mut *self.page_ptr(d), &mut *self.page_ptr(e)) };
            a.wgen = a.wgen.wrapping_add(1);
            b.wgen = b.wgen.wrapping_add(1);
            a.normalize();
            b.normalize();
            a.version += 1;
            a.copy_from(la, b, 0, lb);
            if a.off != b.off {
                // into a's frame (`mergeable`: it fits)
                // SAFETY: slots la..la+lb are live in-bounds inks.
                unsafe { translate_union(a.ink.as_mut_ptr().add(la), lb, b.off[0] - a.off[0], b.off[1] - a.off[1]) };
            }
            a.regroup_range(la, la + lb);
        }
        if self.page(e).state != 0 {
            let (pa, src) = (self.page_ptr(d), *self.state.get(self.page(e).id));
            // SAFETY: a live page.
            self.state.ensure(unsafe { (*pa).id }).copy(la, &src, 0, lb);
            self.st_settle(pa);
        }
        self.fixup_handles(d, la, la + lb);
        self.d_len[d] = (la + lb) as u32;
        self.d_ink[d] = self.d_ink[d].union(self.d_ink[e]);
        self.d_min_depth[d] = self.d_min_depth[d].min(self.d_min_depth[e]);
        let dp = self.page_ptr(d);
        self.dir_remove_seq(e, 1);
        // SAFETY: a live page (its entry may have moved with its block).
        self.pend_run(unsafe { (*dp).dir } as usize >> RUN_SHIFT);
        attr!(8, self.dir_settle());
    }

    // ------------------------------------------------------------------ move

    /// Move the run `[start, start+len)` so that it starts at `to` in the resulting sequence
    /// (`to <= len() - run_len`). Damages the run's ink (its paint order changed). Handles survive.
    pub fn move_run(&mut self, start: usize, len: usize, to: usize) {
        self.move_impl(start, len, to, 0);
    }

    /// `move_run` that also shifts the run's depths by `new_root_depth - depth(start)`.
    pub fn reparent_run(&mut self, start: usize, len: usize, to: usize, new_root_depth: u16) {
        let dd = new_root_depth as i32 - self.depth_at(start) as i32;
        self.move_impl(start, len, to, dd);
    }

    fn move_impl(&mut self, start: usize, len: usize, to: usize, ddepth: i32) {
        if len == 0 {
            return;
        }
        assert!(start + len <= self.total && to <= self.total - len);
        let mut stage = core::mem::take(&mut self.scratch);
        stage.clear();
        let (far, has_state) = self.read_run_impl(start, len, ddepth, &mut stage);
        // the records pass through a staging buffer: their state bits ride beside it
        let st = if has_state { self.st_read_run(start, len) } else { None };
        if far {
            // some of the run is outside the domain: its 16-bit inks are views; carry the exact rects
            let wide: Vec<WRect> = (start..start + len).map(|pos| self.ink_wide_at(pos)).collect();
            self.remove_impl(start, len, false, true);
            self.insert_core(to, &stage, Some(&wide), false);
        } else {
            self.remove_impl(start, len, false, true);
            self.insert_impl(to, &stage, false);
        }
        if let Some(st) = st {
            for (i, v) in st.into_iter().enumerate() {
                if v != (0, 0) {
                    self.st_put(to + i, v);
                }
            }
        }
        stage.clear();
        self.scratch = stage;
    }

    /// Returns whether any page of the run is far or wide (then the inks pushed are clamped views), and
    /// whether any has state bits.
    fn read_run_impl(&self, start: usize, len: usize, ddepth: i32, out: &mut Vec<RecordInit>) -> (bool, bool) {
        if len == 0 {
            return (false, false);
        }
        let mut st = 0u8;
        let (mut d, mut s) = self.locate(start);
        let mut rem = len;
        let mut far = 0u8;
        while rem > 0 {
            let l = self.d_len[d] as usize;
            let n = rem.min(l - s);
            let p = self.page(d);
            far |= p.flags;
            st |= p.state;
            for i in s..s + n {
                out.push(RecordInit {
                    depth: (p.depth[i] as i32 + ddepth) as u16,
                    ink: p.abs(p.ink[i]),
                    kind: p.kind[i],
                    payload: p.payload[i],
                    handle: p.handle[i],
                    clip: p.clip[i],
                });
            }
            rem -= n;
            d = self.next_dir(d);
            s = 0;
        }
        (far & (F_FAR | F_WIDE) != 0, st != 0)
    }

    /// Append the records `[start, start+len)` to `out` (absolute ink, handles kept): the input of a
    /// `rewrite_run` that permutes sub-runs (a list's rows reordered). Inks outside the 16-bit domain
    /// come out clamped: a run that may hold such content is read with `read_run_wide`.
    pub fn read_run(&self, start: usize, len: usize, out: &mut Vec<RecordInit>) {
        assert!(start + len <= self.total);
        self.read_run_impl(start, len, 0, out);
    }
    /// `read_run`, with each record's exact rect appended to `wide`.
    pub fn read_run_wide(&self, start: usize, len: usize, out: &mut Vec<RecordInit>, wide: &mut Vec<WRect>) {
        assert!(start + len <= self.total);
        self.read_run_impl(start, len, 0, out);
        wide.extend((start..start + len).map(|pos| self.ink_wide_at(pos)));
    }
    /// `rewrite_run` with exact rects (`wide[i]` is record `i`'s ink).
    pub fn rewrite_run_wide(&mut self, start: usize, recs: &[RecordInit], wide: &[WRect]) {
        if recs.is_empty() {
            return;
        }
        let st = self.st_read_by_handle(start, recs.len());
        self.remove_impl(start, recs.len(), false, true);
        self.insert_core(start, recs, Some(wide), true);
        self.st_put_by_handle(start, recs, st);
    }

    /// Replace the run `[start, start+recs.len())` by `recs`, a rearrangement of the same records (their
    /// handles stay bound and follow them). Damages the old and the new ink of the run.
    pub fn rewrite_run(&mut self, start: usize, recs: &[RecordInit]) {
        if recs.is_empty() {
            return;
        }
        let st = self.st_read_by_handle(start, recs.len());
        self.remove_impl(start, recs.len(), false, true);
        self.insert_impl(start, recs, true);
        self.st_put_by_handle(start, recs, st);
    }
    /// The state of the run's records that have a handle, by handle index (a rewrite restates the run in
    /// another order; a record is recognised by its handle).
    fn st_read_by_handle(&self, start: usize, len: usize) -> Vec<(u32, (u16, u16))> {
        let Some(st) = self.st_read_run(start, len) else { return Vec::new() };
        let mut out = Vec::new();
        for (i, v) in st.into_iter().enumerate() {
            if v != (0, 0) {
                let (d, s) = self.locate(start + i);
                let h = self.page(d).handle[s];
                if h != 0 {
                    out.push((h, v));
                }
            }
        }
        out
    }
    fn st_put_by_handle(&mut self, start: usize, recs: &[RecordInit], st: Vec<(u32, (u16, u16))>) {
        if st.is_empty() {
            return;
        }
        for (i, r) in recs.iter().enumerate() {
            if let Some(&(_, v)) = st.iter().find(|e| e.0 == r.handle && r.handle != 0) {
                self.st_put(start + i, v);
            }
        }
    }

    // ------------------------------------------------------------------ translate / ink / touch

    /// Translate the ink of `[start, start+len)` by (dx, dy). SIMD add over the contiguous ink
    /// columns fused with both union reductions; damage is the run's old and new content bounds
    /// (two marks, never one per moved record).
    pub fn translate_run(&mut self, start: usize, len: usize, dx: i32, dy: i32) {
        if len == 0 || (dx == 0 && dy == 0) {
            return;
        }
        let (uo, un) = self.translate_impl(start, len, dx, dy);
        self.ring.add(uo);
        self.ring.add(un);
    }

    /// Translate the run's ink; returns (old union, new union) of its unclipped ink.
    fn translate_impl(&mut self, start: usize, len: usize, dx: i32, dy: i32) -> (NRect, NRect) {
        assert!(start + len <= self.total);
        let (mut d0, mut s) = self.locate(start);
        let mut d = d0;
        let mut rem = len;
        // (the position of the next record to move: the exact path may lay directory entries out again)
        let mut pos = start;
        let (mut uo, mut un) = (NRect::EMPTY, NRect::EMPTY);
        while rem > 0 {
            let l = self.d_len[d] as usize;
            #[cfg(target_arch = "aarch64")]
            if s == 0 && rem >= l {
                // a run of whole pages: the vector loop takes every leading plain one
                let (o, w, nd, nrem) = self.translate_pages(d, rem, dx, dy);
                if nd != d {
                    pos += rem - nrem;
                    (uo, un, d, rem) = (uo.union(o), un.union(w), nd, nrem);
                    continue;
                }
            }
            let n = rem.min(l - s);
            let old_d = self.d_ink[d];
            let mut pages = 0;
            if s == 0 && n == l {
                // Whole page: the offset moves, the records do not (O(1) per page). The page stays plain
                // while its offset stays in 16 bits and its ink inside the domain (one saturating lane add
                // and one compare on its summary); otherwise the exact path decides its frame.
                let p = self.page_mut(d);
                let (nx, ny) = (p.off[0] + dx, p.off[1] + dy);
                let fast = p.flags & !F_XF == 0 && nx.abs() <= 32767 && ny.abs() <= 32767;
                match if fast { old_d.translate_in_domain(dx, dy) } else { None } {
                    Some(new_d) => {
                        p.set_off(nx, ny);
                        self.d_ink[d] = new_d;
                        uo = uo.union(old_d);
                        un = un.union(new_d);
                    }
                    None if p.flags & F_XF != 0 => {
                        // a transformed page: fold the offset into the affine, then move the stored inks
                        p.normalize();
                        // SAFETY: l live in-bounds slots.
                        unsafe { translate_union(p.ink.as_mut_ptr(), l, dx, dy) };
                        p.regroup_range(0, l);
                        let new_d = p.union_abs();
                        self.d_ink[d] = new_d;
                        uo = uo.union(old_d);
                        un = un.union(new_d);
                    }
                    None => match self.translate_far(d, dx, dy) {
                        // a page with its own frame whose frame stays valid: its offset moves, like a
                        // plain page's
                        Some((o, w)) => (uo, un) = (uo.union(o), un.union(w)),
                        None => {
                            self.fix_chunks(d0, d + 1);
                            let (o, w, np) = self.translate_exact(d, 0, l, dx, dy);
                            (uo, un, pages) = (uo.union(o), un.union(w), np);
                        }
                    },
                }
            } else {
                let p = self.page_mut(d);
                if p.flags == 0 {
                    // Partial page: its records move relative to the others, in the page's frame. The add
                    // wraps, so it can be taken back exactly if the run would leave the frame or the domain.
                    // SAFETY: slots s..s+n are live in-bounds ink entries.
                    let (old, new) = unsafe { translate_union(p.ink.as_mut_ptr().add(s), n, dx, dy) };
                    let plain0 = p.off == [0, 0];
                    let ok = old.translate_in_domain(dx, dy).is_some() && (plain0 || p.abs(old).translate_in_domain(dx, dy).is_some());
                    if ok {
                        p.regroup_range(s, s + n);
                        // (no offset: stored is absolute)
                        let (o, w) = if plain0 { (old, new) } else { (p.abs(old), p.abs(new)) };
                        self.d_ink[d] = p.union_abs();
                        uo = uo.union(o);
                        un = un.union(w);
                    } else {
                        // SAFETY: as above; the wrapping add is its own inverse.
                        unsafe { translate_union(p.ink.as_mut_ptr().add(s), n, -dx, -dy) };
                        self.fix_chunks(d0, d + 1);
                        let (o, w, np) = self.translate_exact(d, s, n, dx, dy);
                        (uo, un, pages) = (uo.union(o), un.union(w), np);
                    }
                } else if p.flags & F_XF != 0 {
                    p.normalize();
                    // SAFETY: slots s..s+n are live in-bounds ink entries.
                    let (old, new) = unsafe { translate_union(p.ink.as_mut_ptr().add(s), n, dx, dy) };
                    p.regroup_range(s, s + n);
                    let u = p.union_abs();
                    self.d_ink[d] = u;
                    uo = uo.union(old);
                    un = un.union(new);
                } else {
                    self.fix_chunks(d0, d + 1);
                        let (o, w, np) = self.translate_exact(d, s, n, dx, dy);
                    (uo, un, pages) = (uo.union(o), un.union(w), np);
                }
            }
            rem -= n;
            pos += n;
            s = 0;
            if pages == 0 {
                d = self.next_dir(d);
            } else {
                // the exact path ran (and settled its own blocks): find the next page by position
                d = if rem > 0 { self.locate(pos).0 } else { 0 };
                d0 = d;
            }
        }
        self.fix_chunks(d0, d);
        (uo, un)
    }

    /// Whole plain pages from directory index `d`, while `rem` records cover the next page whole, in
    /// vector form (overfitting finding 13: a scroll is this loop over the content's pages; it was ~109
    /// scalar instructions a page). The page summary moves by one saturating lane add, the offset by one
    /// 32-bit pair add, the offset lanes are that pair and its negation narrowed, both damage unions are a
    /// lane min, and one compare pair proves the result is still a plain page (summary inside the domain,
    /// offset in 16 bits). Stops at the first page that is not (the caller's general path takes it).
    /// Returns (old union, new union, next directory index, records left).
    #[cfg(target_arch = "aarch64")]
    #[inline(never)]
    fn translate_pages(&mut self, mut d: usize, mut rem: usize, dx: i32, dy: i32) -> (NRect, NRect, usize, usize) {
        use core::arch::aarch64::*;
        if (dx.unsigned_abs() | dy.unsigned_abs()) > 32766 {
            return (NRect::EMPTY, NRect::EMPTY, d, rem);
        }
        self.mgen = self.mgen.wrapping_add(1);
        let nr = self.r_len.len();
        // SAFETY: d is a live directory index inside the loops (below its block's live count: d_len, d_ink
        // and d_ptr have an entry for it and the page is live); `off` is two i32 and `offl` four i16 of the
        // page header; &mut self is exclusive.
        unsafe {
            let dl = [dx as i16, dy as i16, (-dx) as i16, (-dy) as i16];
            let (dlv, d32) = (vld1_s16(dl.as_ptr()), vld1_s32([dx, dy].as_ptr()));
            let (mut uov, mut unv) = (vdup_n_s16(i16::MAX), vdup_n_s16(i16::MAX));
            let (lens, inks, ptrs) = (self.d_len.as_ptr(), self.d_ink.as_mut_ptr() as *mut i16, self.d_ptr.as_ptr());
            let rl = self.r_len.as_ptr();
            'blocks: while d >> RUN_SHIFT < nr {
                let r = d >> RUN_SHIFT;
                let end = (r << RUN_SHIFT) + *rl.add(r) as usize;
                while d < end {
                let l = *lens.add(d) as usize;
                if rem < l {
                    break 'blocks;
                }
                let pp = *ptrs.add(d);
                let dip = inks.add(d * 4);
                let v = vld1_s16(dip);
                let r = vqadd_s16(v, dlv);
                let no = vadd_s32(vld1_s32((*pp).off.as_ptr()), d32);
                // (an empty summary has saturated lanes: it is `out`, and the general path keeps it empty)
                let out = vget_lane_u64::<0>(vreinterpret_u64_u16(vcgt_s16(vqabs_s16(r), vdup_n_s16(COORD_MAX as i16))))
                    | vget_lane_u64::<0>(vreinterpret_u64_u32(vcgt_s32(vabs_s32(no), vdup_n_s32(32767))));
                if out != 0 || (*pp).flags & !F_XF != 0 {
                    break 'blocks;
                }
                (*pp).wgen = (*pp).wgen.wrapping_add(1);
                vst1_s32((*pp).off.as_mut_ptr(), no);
                vst1_s16((*pp).offl.as_mut_ptr(), vmovn_s32(vcombine_s32(no, vneg_s32(no))));
                vst1_s16(dip, r);
                uov = vmin_s16(uov, v);
                unv = vmin_s16(unv, r);
                rem -= l;
                d += 1;
                }
                // the block's live entries are done: the next page is the next block's first
                d = (r + 1) << RUN_SHIFT;
            }
            // an int16x4 is four i16 lanes, the layout of NRect
            (core::mem::transmute::<int16x4_t, NRect>(uov), core::mem::transmute::<int16x4_t, NRect>(unv), d, rem)
        }
    }

    /// A whole page that is plain or far (not transformed, not wide) moves by `(dx, dy)`, in i32: its
    /// offset moves if the frame rule still holds afterwards (the offset in 16 bits on every axis where
    /// the page is not wholly beyond the domain). `None`: the page must choose its frame again. The
    /// block summary is left to the caller's `fix_chunks` over the run.
    #[inline]
    fn translate_far(&mut self, d: usize, dx: i32, dy: i32) -> Option<(NRect, NRect)> {
        let p = self.page_mut(d);
        if p.flags & (F_XF | F_WIDE) != 0 {
            return None;
        }
        let uo = p.abs_wide(p.union_ink());
        if uo.is_empty() {
            // nothing drawn here: nothing moves (the offset stays where a plain page may have it)
            return Some((NRect::EMPTY, NRect::EMPTY));
        }
        let un = uo.translate(dx, dy);
        let no = [p.off[0] + dx, p.off[1] + dy];
        let valid = (0..2).all(|a| no[a].abs() <= 32767 || un.0[a] > COORD_MAX || un.0[a + 2] < COORD_MIN);
        if !valid {
            return None;
        }
        p.set_off(no[0], no[1]);
        p.flags = if un.in_domain() { 0 } else { F_FAR };
        let v = un.to_nrect();
        self.d_ink[d] = v;
        Some((uo.to_nrect(), v))
    }

    /// The exact path of a translation: slots `[s, s + n)` of page `d` (not transformed) move by
    /// `(dx, dy)` in i32. A whole far page whose frame stays valid only moves its offset; anything else
    /// is rewritten from the exact rects (`reencode`: the frame is chosen again, the page cut if it must
    /// be). Returns the clamped views of the old and the new union of the moved records, and the number
    /// of directory pages the page became.
    #[inline(never)]
    fn translate_exact(&mut self, d: usize, s: usize, n: usize, dx: i32, dy: i32) -> (NRect, NRect, usize) {
        let mut rects = self.page_rects(d);
        let (mut uo, mut un) = (WRect::EMPTY, WRect::EMPTY);
        for r in &mut rects[s..s + n] {
            uo = uo.union(*r);
            *r = r.translate(dx, dy);
            un = un.union(*r);
        }
        let pages = self.reencode(d, &rects);
        (uo.to_nrect(), un.to_nrect(), pages)
    }

    /// A scroll step: `translate_run` of a scroll container's content, with damage
    /// `clip ∩ old content bounds` and `clip ∩ new content bounds` (the container's viewport `clip`).
    pub fn scroll_run(&mut self, start: usize, len: usize, dx: i32, dy: i32, clip: NRect) {
        if len == 0 || (dx == 0 && dy == 0) {
            return;
        }
        let (uo, un) = self.translate_impl(start, len, dx, dy);
        self.ring.add(uo.intersect(clip));
        self.ring.add(un.intersect(clip));
    }

    /// Scroll by copy (bough-metal RESULTS §15): `translate_run` of a scroll container's content by an
    /// integral `(dx, dy)` whose damage is only what a copy of the previous frame's pixels cannot supply:
    /// * the **exposed strip(s)** `clip \ (clip + d)`;
    /// * the marks this epoch already made inside `clip` (before the move), also at their moved place
    ///   (`Plane::shift_within`; a paint ring kept by the caller needs the same call);
    /// * the **static content's bands**: a record outside the run whose visible ink meets `clip` does not
    ///   move, but the copy moves its pixels. For a box (`band(kind, payload) >= 0`) its pixels depend on its
    ///   edges only within `band` of them, so only the edges perpendicular to `d` see a difference: exactly
    ///   [`Bough::damage_edges`] of its visible ink and that ink moved by `d`. Anything else (`band < 0`: text,
    ///   images, gradients; and every record of a transformed page, whose ink is a cover, not its edges)
    ///   marks its visible ink and its moved copy.
    /// * the **transformed content of the run**: a copy is exact for a moved record only if the executor's
    ///   pixels of it are a function of the pixel's offset from the record, which an untransformed record's
    ///   are (integral move, geometry on the 1/256 grid) and a transformed one's are not (the kernels map a
    ///   pixel through the space's affine in absolute coordinates, so the rounding depends on where the
    ///   record is: 1-level differences in a turned label). A page of the run carrying a transform (`F_XF`)
    ///   marks its cover at the old place and the new. Untransformed pages cost nothing here.
    ///
    /// The interaction flags of a record's kind (`KF_MASK`) play no part: they route input, they do not
    /// change pixels (a hover look is a paint write, marked where it happens).
    ///
    /// The epoch keeps a [`ScrollRec`] (`scroll_rec`). A second scroll in the same epoch that is not the same
    /// clip and delta cannot share the one copy: both clips are marked whole and the record is dropped (a
    /// bounds condition, never a policy). `band` is the caller's per-record edge band (monomorphised).
    pub fn scroll_run_copy(&mut self, start: usize, len: usize, dx: i32, dy: i32, clip: NRect, mut band: impl FnMut(u8, u32) -> i16) {
        if len == 0 || (dx == 0 && dy == 0) {
            return;
        }
        let c = clip.intersect(self.ring.window).canon();
        let rec = ScrollRec { clip: c, dx, dy };
        match self.ring.scroll(0) {
            Some(r) if r == rec => {
                // another run of the same move (a split run): the first call's marks cover it
                self.translate_impl(start, len, dx, dy);
                return;
            }
            Some(r) => {
                self.translate_impl(start, len, dx, dy);
                self.ring.add(r.clip);
                self.ring.add(c);
                // poisoned: every later scroll of this epoch is marked whole too (`scroll_rec` reports none)
                self.ring.set_scroll(Some(ScrollRec { clip: NRect::EMPTY, dx: 0, dy: 0 }));
                return;
            }
            None => {}
        }
        if c.is_empty() {
            self.translate_impl(start, len, dx, dy);
            return;
        }
        self.ring.shift_within(c, dx, dy);
        // the static content: every record outside the run whose visible ink meets the clip, before the move
        let moved: u8 = if dx != 0 { 1 | 4 } else { 0 } | if dy != 0 { 2 | 8 } else { 0 };
        let th = Thresh::of_rect(c);
        let mut pos0 = 0usize;
        let (mut d, end) = (0usize, if self.np == 0 { 0 } else { self.dir_len() });
        while d < end {
            let cur = d;
            d = self.next_dir(d);
            let d = cur;
            let l = self.d_len[d] as usize;
            let (p0, p1) = (pos0, pos0 + l);
            pos0 = p1;
            if !th.matches(self.d_ink[d]) {
                continue;
            }
            // SAFETY: directory pointers are live pages of the pool, which nothing below writes (the marks go
            // to the damage ring, a separate allocation).
            let p: &Page = unsafe { &*self.d_ptr[d] };
            let xf = p.flags & F_XF != 0;
            if xf && p0 < start + len && p1 > start {
                // transformed records moving with the run: repainted where they were and where they land
                let u = self.d_ink[d];
                self.ring.add(u.intersect(c));
                self.ring.add(u.translate(dx, dy).intersect(c));
            }
            if p0 >= start && p1 <= start + len {
                continue;
            }
            for s in 0..l {
                let pos = p0 + s;
                if pos >= start && pos < start + len {
                    continue;
                }
                let ink = p.abs(p.ink[s]);
                if !th.matches(ink) {
                    continue;
                }
                let v = self.clipped(ink, p.clip[s]).intersect(c).canon();
                if v.is_empty() {
                    continue;
                }
                let b = if xf { -1 } else { band(p.kind[s], p.payload[s]) };
                let [x0, y0, x1, y1] = v.to_xyxy();
                if b < 0 {
                    self.ring.add(v);
                    self.ring.add(v.translate(dx, dy).intersect(c));
                    continue;
                }
                let b = b as i32;
                // damage_edges(v + d, v, moved, band) clipped to c: the hull of each perpendicular edge's band
                // and its moved copy, across both inks' extent
                let (ux0, uy0, ux1, uy1) = (x0.min(x0 + dx), y0.min(y0 + dy), x1.max(x1 + dx), y1.max(y1 + dy));
                if moved & 2 != 0 {
                    for e in [y0, y1] {
                        let (a, z) = (e.min(e + dy) - b, e.max(e + dy) + b);
                        self.ring.add(NRect::new(ux0, a, ux1, z).intersect(c));
                    }
                }
                if moved & 1 != 0 {
                    for e in [x0, x1] {
                        let (a, z) = (e.min(e + dx) - b, e.max(e + dx) + b);
                        self.ring.add(NRect::new(a, uy0, z, uy1).intersect(c));
                    }
                }
            }
        }
        self.translate_impl(start, len, dx, dy);
        // the exposed strips: clip \ (clip + d)
        let [cx0, cy0, cx1, cy1] = c.to_xyxy();
        if dy > 0 {
            self.ring.add(NRect::new(cx0, cy0, cx1, (cy0 + dy).min(cy1)));
        } else if dy < 0 {
            self.ring.add(NRect::new(cx0, (cy1 + dy).max(cy0), cx1, cy1));
        }
        if dx > 0 {
            self.ring.add(NRect::new(cx0, cy0, (cx0 + dx).min(cx1), cy1));
        } else if dx < 0 {
            self.ring.add(NRect::new((cx1 + dx).max(cx0), cy0, cx1, cy1));
        }
        self.ring.set_scroll(Some(rec));
    }

    /// The scroll-by-copy record of the epoch `back` epochs ago (0 = current), if it had one.
    #[inline]
    pub fn scroll_rec(&self, back: u32) -> Option<ScrollRec> {
        self.ring.scroll(back).filter(|r| !r.clip.is_empty())
    }

    pub fn set_ink(&mut self, pos: usize, ink: NRect) {
        let (d, s) = self.locate(pos);
        self.set_ink_at(d, s, ink, true);
    }
    pub fn set_ink_handle(&mut self, h: Handle, ink: NRect) -> bool {
        match self.handle_loc(h) {
            Some((d, s)) => {
                self.set_ink_at(d, s, ink, true);
                true
            }
            None => false,
        }
    }
    /// Change ink without marking damage: the caller marks the exact support itself (e.g. per
    /// changed glyph) with `damage_rect`.
    pub fn set_ink_nodamage(&mut self, pos: usize, ink: NRect) {
        let (d, s) = self.locate(pos);
        self.set_ink_at(d, s, ink, false);
    }
    #[inline]
    fn set_ink_at(&mut self, d: usize, s: usize, ink: NRect, damage: bool) {
        let ink = ink.canon();
        let p = self.page_mut(d);
        let old = p.abs(p.ink[s]);
        if old == ink && p.flags & !F_XF == 0 {
            return;
        }
        let (kind, id) = (p.kind[s], p.clip[s]);
        let xf = p.is_xf();
        let old_stored = p.ink[s];
        if !p.store_abs(s, ink) {
            return self.set_ink_exact(d, s, WRect::from(ink), damage);
        }
        p.regroup(s / GROUP);
        let u = p.union_abs();
        let od = self.d_ink[d];
        self.d_ink[d] = u;
        self.chunk_update(d, od);
        if damage && xf {
            // a transformed record (`ink` is its untransformed ink): the tile covers of the old and the new
            // transformed rects, exact at tile granularity
            let new_stored = self.page(d).ink[s];
            self.damage_cover(d, old_stored, id);
            self.damage_cover(d, new_stored, id);
        } else if damage {
            let (old, ink) = (self.clipped(old, id), self.clipped(ink, id));
            if self.kind_uniform(kind) {
                self.damage_xor(old, ink);
            } else {
                self.ring.add(old);
                self.ring.add(ink);
            }
        }
    }

    /// The exact path of an ink write: the record's page is far or wide, or the new ink leaves its
    /// frame or the domain. The page is rewritten from exact rects. Damage: the clamped views of the old
    /// and the new ink (both whole: no symmetric-difference refinement out here).
    #[inline(never)]
    fn set_ink_exact(&mut self, d: usize, s: usize, ink: WRect, damage: bool) {
        // A page with a frame of its own (or a plain page taking its first ink outside the domain): when
        // the new ink fits the frame and the frame rule still holds, this is the same store as on a plain
        // page, with the absolute forms made in i32.
        let p = self.page_mut(d);
        if p.flags & (F_XF | F_WIDE) == 0 {
            if let Some(rel) = p.rel_of(ink) {
                let (old_rel, id) = (p.ink[s], p.clip[s]);
                if old_rel == rel {
                    return;
                }
                let old = p.abs_wide(old_rel);
                p.ink[s] = rel;
                p.regroup(s / GROUP);
                let u = p.abs_wide(p.union_ink());
                let off = p.off;
                let valid = (0..2).all(|a| off[a].abs() <= 32767 || u.is_empty() || u.0[a] > COORD_MAX || u.0[a + 2] < COORD_MIN);
                if valid {
                    p.flags = if u.in_domain() && off[0].abs() <= 32767 && off[1].abs() <= 32767 { 0 } else { F_FAR };
                    let od = self.d_ink[d];
                    self.d_ink[d] = u.to_nrect();
                    self.chunk_update(d, od);
                    if damage {
                        let (o, w) = (self.clipped(old.to_nrect(), id), self.clipped(ink.to_nrect(), id));
                        self.ring.add(o);
                        self.ring.add(w);
                    }
                    return;
                }
                // the frame rule broke: put the ink back and let the page choose its frame again
                p.ink[s] = old_rel;
                p.regroup(s / GROUP);
            }
        }
        let mut rects = self.page_rects(d);
        if rects[s] == ink {
            return;
        }
        let id = self.page(d).clip[s];
        let old = rects[s];
        rects[s] = ink;
        self.reencode(d, &rects);
        if damage {
            let (o, w) = (self.clipped(old.to_nrect(), id), self.clipped(ink.to_nrect(), id));
            self.ring.add(o);
            self.ring.add(w);
        }
    }
    /// `set_ink` with an exact rect (any i32 device-pixel rect).
    pub fn set_ink_wide(&mut self, pos: usize, ink: WRect) {
        let (d, s) = self.locate(pos);
        if ink.in_domain() {
            self.set_ink_at(d, s, ink.to_nrect(), true);
        } else {
            self.set_ink_exact(d, s, ink, true);
        }
    }
    /// `set_ink_handle` with an exact rect.
    pub fn set_ink_handle_wide(&mut self, h: Handle, ink: WRect) -> bool {
        let Some((d, s)) = self.handle_loc(h) else { return false };
        if ink.in_domain() {
            self.set_ink_at(d, s, ink.to_nrect(), true);
        } else {
            self.set_ink_exact(d, s, ink, true);
        }
        true
    }

    /// `set_ink_handle_edges` with an exact rect: the edge-band damage when the ink is inside the domain
    /// (where it can be seen), the exact path otherwise.
    pub fn set_ink_handle_edges_wide(&mut self, h: Handle, ink: WRect, moved: u8, band: i16) -> bool {
        if ink.in_domain() {
            return self.set_ink_handle_edges(h, ink.to_nrect(), moved, band);
        }
        let Some((d, s)) = self.handle_loc(h) else { return false };
        self.set_ink_exact(d, s, ink, true);
        true
    }
    /// `set_ink_handle_nodamage` with an exact rect.
    pub fn set_ink_handle_nodamage_wide(&mut self, h: Handle, ink: WRect) -> bool {
        let Some((d, s)) = self.handle_loc(h) else { return false };
        if ink.in_domain() {
            self.set_ink_at(d, s, ink.to_nrect(), false);
        } else {
            self.set_ink_exact(d, s, ink, false);
        }
        true
    }

    /// Batched ink writes (a layout pass, a turn's bindings): every ink is stored, each touched
    /// group summary and page summary is recomputed once, the chunk summaries once. With
    /// `damage`, each write marks what `set_ink` marks. Positions in ascending order skip the
    /// binary search inside a page (a locate per page instead of per write).
    pub fn set_inks(&mut self, writes: &[(usize, NRect)], damage: bool) {
        // Walk: a page's writes are contiguous when positions ascend, so its summary is settled the
        // moment the walk leaves it (unsorted input still works: a page may then settle twice).
        let (mut d, mut base, mut len, mut gm) = (0usize, 0usize, 0usize, 0u32);
        for &(pos, ink) in writes {
            if pos.wrapping_sub(base) >= len {
                if gm != 0 {
                    self.settle_page(d, gm);
                }
                let (dd, _, b) = self.locate_base(pos);
                (d, base, len, gm) = (dd, b, self.d_len[dd] as usize, 0);
            }
            let s = pos - base;
            let ink = ink.canon();
            let p = self.page_mut(d);
            let old = p.abs(p.ink[s]);
            if old == ink && p.flags & !F_XF == 0 {
                continue;
            }
            let (kind, id) = (p.kind[s], p.clip[s]);
            if !p.store_abs(s, ink) {
                // the exact path may recut pages: settle this one, write alone, locate afresh
                if gm != 0 {
                    self.settle_page(d, gm);
                }
                self.set_ink_exact(d, s, WRect::from(ink), damage);
                (base, len, gm) = (0, 0, 0);
                continue;
            }
            gm |= 1 << (s / GROUP);
            if damage {
                let (old, ink) = (self.clipped(old, id), self.clipped(ink, id));
                if self.kind_uniform(kind) {
                    self.damage_xor(old, ink);
                } else {
                    self.ring.add(old);
                    self.ring.add(ink);
                }
            }
        }
        if gm != 0 {
            self.settle_page(d, gm);
        }
    }

    /// Recompute the groups `gm` of page `d`, its summary and (if it changed) its chunk.
    #[inline(always)]
    fn settle_page(&mut self, d: usize, mut gm: u32) {
        let p = self.page_mut(d);
        while gm != 0 {
            p.regroup(gm.trailing_zeros() as usize);
            gm &= gm - 1;
        }
        let u = p.union_abs();
        let old = self.d_ink[d];
        self.d_ink[d] = u;
        self.chunk_update(d, old);
    }

    /// `set_inks` addressed by handles (stale handles are skipped); any order.
    pub fn set_inks_handles(&mut self, writes: &[(Handle, NRect)], damage: bool) {
        let mut dirty = core::mem::take(&mut self.dirty_scratch);
        for &(h, ink) in writes {
            if let Some((p, s)) = self.handle_ptr(h) {
                self.write_ink_deferred(p, s, ink, damage, &mut dirty);
            }
        }
        self.settle_pages(&mut dirty);
        self.dirty_scratch = dirty;
    }

    #[inline(always)]
    fn write_ink_deferred(&mut self, p: *mut Page, s: usize, ink: NRect, damage: bool, dirty: &mut Vec<u32>) {
        let ink = ink.canon();
        // SAFETY: p is a live page of this Bough (from the directory or a bound handle); s < CAP.
        let pg = unsafe { &mut *p };
        pg.wgen = pg.wgen.wrapping_add(1);
        self.mgen = self.mgen.wrapping_add(1);
        let old = pg.abs(pg.ink[s]);
        if old == ink && pg.flags & !F_XF == 0 {
            return;
        }
        let (kind, id) = (pg.kind[s], pg.clip[s]);
        if !pg.store_abs(s, ink) {
            // the exact path may recut pages: settle what is pending first (directory indices move)
            let d = pg.dir as usize;
            self.settle_pages(dirty);
            return self.set_ink_exact(d, s, WRect::from(ink), damage);
        }
        if pg.dirty == 0 {
            dirty.push(pg.dir);
        }
        pg.dirty |= 1 << (s / GROUP);
        if damage {
            let (old, ink) = (self.clipped(old, id), self.clipped(ink, id));
            if self.kind_uniform(kind) {
                self.damage_xor(old, ink);
            } else {
                self.ring.add(old);
                self.ring.add(ink);
            }
        }
    }

    /// Recompute the stale groups of every page in `dirty`, their page summaries, and the chunks.
    fn settle_pages(&mut self, dirty: &mut Vec<u32>) {
        if dirty.is_empty() {
            return;
        }
        let (mut lo, mut hi) = (usize::MAX, 0usize);
        for &d in dirty.iter() {
            let d = d as usize;
            let p = self.page_mut(d);
            let mut m = core::mem::take(&mut p.dirty) as u32;
            while m != 0 {
                p.regroup(m.trailing_zeros() as usize);
                m &= m - 1;
            }
            self.d_ink[d] = p.union_abs();
            lo = lo.min(d);
            hi = hi.max(d + 1);
        }
        if hi - lo <= 2 * CHUNK {
            self.fix_chunks(lo, hi);
        } else {
            // scattered over a big tree: only the chunks that hold a dirty page
            let mut seen = [0u64; 64];
            for &d in dirty.iter() {
                let c = d as usize / CHUNK;
                if c < 4096 && seen[c / 64] & (1 << (c % 64)) == 0 {
                    seen[c / 64] |= 1 << (c % 64);
                    self.fix_chunks(c * CHUNK, c * CHUNK + 1);
                } else if c >= 4096 {
                    self.fix_chunks(c * CHUNK, c * CHUNK + 1);
                }
            }
        }
        dirty.clear();
    }

    /// Change the payload of a handle's record (what the GPU draws for it: an instance, a run of instances,
    /// a range of text runs). No damage: the caller marks what the change repaints.
    pub fn set_payload_handle(&mut self, h: Handle, payload: u32) -> bool {
        match self.handle_ptr(h) {
            Some((p, s)) => {
                // SAFETY: bound handles point at live pages; s < CAP; &mut self is exclusive.
                let pg = unsafe { &mut *p };
                pg.wgen = pg.wgen.wrapping_add(1);
                self.mgen = self.mgen.wrapping_add(1);
                pg.payload[s] = payload;
                true
            }
            None => false,
        }
    }

    /// `set_ink_handle` without damage (the caller produced exact tile marks itself, e.g. bough-text).
    pub fn set_ink_handle_nodamage(&mut self, h: Handle, ink: NRect) -> bool {
        match self.handle_loc(h) {
            Some((d, s)) => {
                self.set_ink_at(d, s, ink, false);
                true
            }
            None => false,
        }
    }

    /// Absolute visible ink (ink ∩ clip) of a handle's record.
    #[inline]
    pub fn visible_ink_handle(&self, h: Handle) -> Option<NRect> {
        let (p, s) = self.handle_ptr(h)?;
        // SAFETY: bound handles point at live pages; s < CAP.
        Some(self.visible_in(unsafe { &*p }, s))
    }

    /// Mark the record's ink damaged (a paint-only write: colour, glyph content, image).
    #[inline]
    pub fn touch(&mut self, pos: usize) {
        let (d, s) = self.locate(pos);
        if self.page(d).is_xf() {
            let (r, c) = (self.page(d).ink[s], self.page(d).clip[s]);
            self.damage_cover(d, r, c);
            return;
        }
        let ink = self.visible(d, s);
        self.ring.add(ink);
    }
    #[inline]
    pub fn touch_handle(&mut self, h: Handle) -> bool {
        match self.handle_ptr(h) {
            Some((p, s)) => {
                // SAFETY: bound handles point at live pages; s < CAP.
                let pg = unsafe { &*p };
                if pg.is_xf() {
                    let (d, _) = self.handle_loc(h).unwrap();
                    let (r, c) = (pg.ink[s], pg.clip[s]);
                    self.damage_cover(d, r, c);
                    return true;
                }
                let ink = self.visible_in(pg, s);
                self.ring.add(ink);
                true
            }
            None => false,
        }
    }

    // ------------------------------------------------------------------ transforms

    /// Mark the tiles the transformed stored rect `r` of page `d` covers (with the kernels' 1 px of
    /// anti-aliasing), clipped to clip `clip`: per 32-px tile row, the x extent of the convex quad within the
    /// row's band. Exact at tile granularity (not the bounding box).
    fn damage_cover(&mut self, d: usize, r: NRect, clip: u16) {
        if r.is_empty() {
            return;
        }
        let q = self.page(d).xf_quad(r);
        let c = if clip == 0 { NRect::new(i32::MIN / 4, i32::MIN / 4, i32::MAX / 4, i32::MAX / 4) } else { self.clips[clip as usize] };
        let ring = &mut self.ring;
        quad_cover(&q, |t| {
            let t = t.intersect(c).canon();
            if !t.is_empty() {
                ring.add(t);
            }
        });
    }

    /// Split the page holding `pos` so that `pos` starts a page (no-op at a page start or the end).
    fn split_at(&mut self, pos: usize) {
        if pos >= self.total {
            return;
        }
        let (d, s) = self.locate(pos);
        if s == 0 {
            return;
        }
        let l = self.d_len[d] as usize;
        let dp = self.page_ptr(d);
        let e = self.dir_insert_after(Some(d), 1);
        // SAFETY: a live page (its entry may have moved with its block).
        let d = unsafe { (*dp).dir } as usize;
        {
            // SAFETY: d and d+1 are distinct live pages.
            let (newp, oldp) = unsafe { (&mut *self.page_ptr(e), &*self.page_ptr(d)) };
            newp.wgen = newp.wgen.wrapping_add(1);
            // the same frame: stored inks are copied as they are
            (newp.xm, newp.xv) = (oldp.xm, oldp.xv);
            newp.set_off(oldp.off[0], oldp.off[1]);
            newp.flags = oldp.flags;
            copy_virtual(newp, s, l, oldp, s, 0, &[]);
            newp.version += 1;
        }
        if self.page(d).state != 0 {
            let (po, pn) = (self.page_ptr(d), self.page_ptr(e));
            self.st_copy_virtual(pn, s, l, po, s, 0);
            // SAFETY: a live page.
            self.state.get_mut(unsafe { (*po).id }).clear(s, CAP);
            self.st_settle(po);
        }
        self.d_len[e] = (l - s) as u32;
        self.fixup_handles(e, 0, l - s);
        {
            let p = self.page_mut(d);
            p.version += 1;
            p.set_sentinel(s, CAP);
            p.regroup_range(0, CAP);
        }
        self.d_len[d] = s as u32;
        self.refresh(d);
        self.refresh(e);
        self.pend_run(d >> RUN_SHIFT);
        self.pend_run(e >> RUN_SHIFT);
        self.dir_settle();
    }

    /// Transform the run `[start, start+len)` (a subtree): its records' inks are taken as untransformed and
    /// drawn at `K + t + m (p - K)` (`m` row-major [a, b, c, d], pivot `k` absolute px, translation `t`).
    /// The run's pages are isolated (split at both ends) and carry the affine, as a translated run's pages
    /// carry their offset; later translations (`translate_run`: scrolls) compose after it. Damage: the
    /// tile covers of every record's old and new transformed ink. The identity clears the transform.
    pub fn set_run_xform(&mut self, start: usize, len: usize, m: [f32; 4], k: [f32; 2], t: [f32; 2]) {
        if len == 0 {
            return;
        }
        assert!(start + len <= self.total);
        self.split_at(start + len);
        self.split_at(start);
        let (d0, s0) = self.locate(start);
        debug_assert_eq!(s0, 0);
        let (mut d, mut rem) = (d0, len);
        while rem > 0 {
            let l = self.d_len[d] as usize;
            self.damage_page_covers(d, l);
            let od = self.d_ink[d];
            {
                let p = self.page_mut(d);
                p.wgen = p.wgen.wrapping_add(1);
                p.version += 1;
                if m == [1.0, 0.0, 0.0, 1.0] && t == [0.0, 0.0] {
                    (p.xm, p.xv) = ([0.0; 4], [0.0; 2]);
                    p.sync_xf();
                } else {
                    p.set_xf(m, k, t);
                }
            }
            self.damage_page_covers(d, l);
            self.d_ink[d] = self.page(d).union_abs();
            self.chunk_update(d, od);
            rem -= l.min(rem);
            d = self.next_dir(d);
        }
        self.fix_chunks(d0, d);
    }

    fn damage_page_covers(&mut self, d: usize, l: usize) {
        for s in 0..l {
            let (r, c, xf) = { let p = self.page(d); (p.ink[s], p.clip[s], p.is_xf()) };
            if xf {
                self.damage_cover(d, r, c);
            } else {
                let v = self.visible(d, s);
                self.ring.add(v);
            }
        }
    }

    /// The transform of the page holding `pos` (`None`: untransformed): (M - I, translation).
    pub fn xform_at(&self, pos: usize) -> Option<([f32; 4], [f32; 2])> {
        let (d, _) = self.locate(pos);
        let p = self.page(d);
        p.is_xf().then_some((p.xm, p.xv))
    }

    // ------------------------------------------------------------------ structure queries

    /// The first page after `d` (paint order) holding a record of depth `<= dpt`: the rest of `d`'s block
    /// (its live entries), then the first later block whose least depth qualifies, then inside that block.
    /// Two short column scans and one over the block summaries, instead of one over every later page.
    #[inline]
    fn depth_next_le(&self, d: usize, dpt: u16) -> Option<usize> {
        let r = d >> RUN_SHIFT;
        // SAFETY: d is a live directory index: r is a block, and a block's live entries are in bounds.
        unsafe {
            let (b, l) = (r << RUN_SHIFT, *self.r_len.get_unchecked(r) as usize);
            let col = self.d_min_depth.as_ptr();
            if let Some(j) = find_first_le(core::slice::from_raw_parts(col.add(b), l), d - b + 1, dpt) {
                return Some(b + j);
            }
            let r2 = find_first_le(&self.r_min_depth, r + 1, dpt)?;
            let b2 = r2 << RUN_SHIFT;
            Some(b2 + find_first_le(core::slice::from_raw_parts(col.add(b2), *self.r_len.get_unchecked(r2) as usize), 0, dpt).unwrap_unchecked())
        }
    }
    /// The last page before `d` holding a record of depth `< dpt`.
    #[inline]
    fn depth_prev_lt(&self, d: usize, dpt: u16) -> Option<usize> {
        let r = d >> RUN_SHIFT;
        let b = r << RUN_SHIFT;
        // SAFETY: as in `depth_next_le`.
        unsafe {
            let col = self.d_min_depth.as_ptr();
            if let Some(j) = find_last_lt(core::slice::from_raw_parts(col.add(b), d - b), d - b, dpt) {
                return Some(b + j);
            }
            let r2 = find_last_lt(&self.r_min_depth, r, dpt)?;
            let (b2, l2) = (r2 << RUN_SHIFT, *self.r_len.get_unchecked(r2) as usize);
            Some(b2 + find_last_lt(core::slice::from_raw_parts(col.add(b2), l2), l2, dpt).unwrap_unchecked())
        }
    }

    /// First index `j > pos` with `depth[j] <= depth[pos]`, or `len()`.
    pub fn subtree_end(&self, pos: usize) -> usize {
        let (d, s, base) = self.locate_base(pos);
        self.subtree_end_at(d, s, base)
    }

    #[inline]
    fn subtree_end_at(&self, d: usize, s: usize, base: usize) -> usize {
        let p = self.page(d);
        let dpt = p.depth[s];
        if let Some(j) = page_find_first_le(&p.depth, s + 1, dpt) {
            return base + j;
        }
        match self.depth_next_le(d, dpt) {
            Some(d2) => {
                let j = page_find_first_le(&self.page(d2).depth, 0, dpt).unwrap();
                self.start_of(d2) + j
            }
            None => self.total,
        }
    }

    /// Last `j < pos` with `depth[j] == depth[pos] - 1` (well-formed preorder: last with smaller depth).
    pub fn parent(&self, pos: usize) -> Option<usize> {
        let (d, s, base) = self.locate_base(pos);
        let p = self.page(d);
        let dpt = p.depth[s];
        if dpt == 0 {
            return None;
        }
        if let Some(j) = page_find_last_lt(&p.depth, s, dpt) {
            return Some(base + j);
        }
        let d2 = self.depth_prev_lt(d, dpt)?;
        let j = page_find_last_lt(&self.page(d2).depth, CAP, dpt).unwrap();
        Some(self.start_of(d2) + j)
    }

    /// Every ancestor of `pos`, nearest first (event bubbling): one backward pass, each step
    /// continuing the depth scan from the previous ancestor; earlier pages are skipped by their
    /// `min_depth` (no locate per level).
    pub fn ancestors(&self, pos: usize, out: &mut Vec<usize>) {
        out.clear();
        let (mut d, mut s, mut base) = self.locate_base(pos);
        let mut dpt = self.page(d).depth[s];
        while dpt > 0 {
            let p = self.page(d);
            if let Some(j) = page_find_last_lt(&p.depth, s, dpt) {
                out.push(base + j);
                (s, dpt) = (j, p.depth[j]);
                continue;
            }
            let Some(d2) = self.depth_prev_lt(d, dpt) else { break };
            (d, s, base) = (d2, CAP, self.start_of(d2));
        }
    }

    /// Children of `pos`, in order: one locate, then a forward scan of the depth column for
    /// `depth + 1` that stops at the first depth `<= depth` (the subtree end); pages whose
    /// `min_depth` exceeds `depth + 1` lie inside a child's subtree and are skipped.
    pub fn children_into(&self, pos: usize, out: &mut Vec<usize>) {
        out.clear();
        let (mut d, s0, mut base) = self.locate_base(pos);
        let dpt = self.page(d).depth[s0];
        let tgt = dpt + 1;
        let mut from = s0 + 1;
        loop {
            let p = self.page(d);
            let l = self.d_len[d] as usize;
            for i in from..l {
                let dep = p.depth[i];
                if dep <= dpt {
                    return;
                }
                if dep == tgt {
                    out.push(base + i);
                }
            }
            match self.depth_next_le(d, tgt) {
                Some(d2) => (d, base, from) = (d2, self.start_of(d2), 0),
                None => return,
            }
        }
    }

    pub fn children(&self, pos: usize) -> Children<'_> {
        let end = self.subtree_end(pos);
        Children { b: self, next: pos + 1, end }
    }

    // ------------------------------------------------------------------ hit test

    /// Topmost slot of transformed page `p` whose stored ink contains the inverse of pixel (x, y) and whose
    /// clip contains (x, y); records with a `skip` kind bit excluded.
    #[inline(never)]
    pub(crate) fn xf_hit_slot(&self, p: &Page, l: usize, x: i32, y: i32, skip: u8) -> Option<usize> {
        let (sx, sy) = p.xf_inv_point(x, y);
        let q = NRect::new(sx, sy, sx + 1, sy + 1);
        let px = NRect::new(x, y, x + 1, y + 1);
        (0..l).rev().find(|&s| p.kind[s] & skip == 0 && p.ink[s].intersects(q) && (p.clip[s] == 0 || self.clips[p.clip[s] as usize].intersects(px)))
    }

    /// Topmost (last in preorder) record whose ink contains pixel (x, y).
    /// Descent: chunk -> page -> group -> record, each a SIMD point test, scanned from the top. A transformed
    /// page tests the inverse of the point against its records' untransformed inks.
    pub fn hit(&self, x: i32, y: i32) -> Option<usize> {
        let t = Thresh::of_point(x, y)?;
        let nc = self.c_ink.len();
        let mut chi = nc;
        while chi > 0 {
            let cb = (chi - 1) / 64 * 64;
            // SAFETY: cb + (chi - cb) <= c_ink.len(), <= 64 rects.
            let mut cm = unsafe { lt_mask(self.c_ink.as_ptr().add(cb), chi - cb, t) };
            while cm != 0 {
                let c = cb + 63 - cm.leading_zeros() as usize;
                cm &= !(1u64 << (c - cb));
                let b = c * CHUNK;
                // the block's live pages (its holes are EMPTY and would fail anyway: a short block is a
                // short scan)
                // SAFETY: c is a block; its live entries are in bounds; cnt <= 64.
                let cnt = unsafe { *self.r_len.get_unchecked(c) } as usize;
                let mut pm = unsafe { lt_mask(self.d_ink.as_ptr().add(b), cnt, t) };
                while pm != 0 {
                    let bit = 63 - pm.leading_zeros() as usize;
                    pm &= !(1u64 << bit);
                    let d = b + bit;
                    let p = self.page(d);
                    if p.is_xf() {
                        if let Some(slot) = self.xf_hit_slot(p, self.d_len[d] as usize, x, y, 0) {
                            return Some(self.start_of(d) + slot);
                        }
                        continue;
                    }
                    let tp = t.shift_lanes(p.offl);
                    // SAFETY: gink is 8 rects; each group is 8 rects of the ink column.
                    let mut gm = unsafe { lt8(p.gink.as_ptr(), tp) };
                    while gm != 0 {
                        let g = 31 - gm.leading_zeros() as usize;
                        gm &= !(1u32 << g);
                        let rm = if p.cgroups & (1 << g) != 0 {
                            let v = self.group_visible(p, g);
                            unsafe { lt8(v.as_ptr(), tp) }
                        } else {
                            unsafe { lt8(p.ink.as_ptr().add(g * GROUP), tp) }
                        };
                        if rm != 0 {
                            let slot = g * GROUP + 31 - rm.leading_zeros() as usize;
                            return Some(self.start_of(d) + slot);
                        }
                    }
                }
            }
            chi = cb;
        }
        None
    }

    // ------------------------------------------------------------------ cull / changed_since

    /// Payload of every record (paint order) whose ink intersects `bbox` and, if given, touches a
    /// set tile of `tiles`; with tiles, `bbox` is clipped to the window first (invisible ink never
    /// draws). `out` is cleared first.
    pub fn cull(&self, bbox: NRect, tiles: Option<&Plane>, out: &mut Vec<u32>) {
        out.clear();
        match tiles {
            None => self.cull_impl::<false, false, false>(bbox, bbox, &Plane::EMPTY, out),
            Some(pl) => {
                // Tiles outside the plane's bbox are clear: spans are clipped to it.
                let clip = pl.px_bbox(self.ring.window);
                if !clip.is_empty() {
                    self.cull_impl::<true, false, true>(bbox.intersect(self.ring.window), clip, pl, out)
                }
            }
        }
    }

    /// The frame cull: payload of every record (paint order) whose ink touches a set tile of
    /// `plane`; `bbox` is the plane's pixel bounding box as returned by `repair_into`.
    #[inline]
    pub fn cull_tiles(&self, plane: &Plane, bbox: NRect, out: &mut Vec<u32>) {
        out.clear();
        if plane.solid {
            // Certificate: the damaged tiles are exactly the bbox rectangle, so the bbox test is exact.
            self.cull_impl::<false, false, false>(bbox, bbox, plane, out);
        } else {
            self.cull_impl::<true, false, false>(bbox, bbox, plane, out);
        }
    }

    /// `cull_tiles`, returning record positions (ascending paint order) instead of payloads (the CPU repair
    /// backend reads each record's clip and payload).
    pub fn cull_tiles_pos(&self, plane: &Plane, bbox: NRect, out: &mut Vec<u32>) {
        out.clear();
        if !plane.is_clear() {
            self.cull_impl::<true, true, false>(bbox, bbox, plane, out);
        }
    }

    /// `repair_into(age, scratch)` then `cull_tiles`: the draw list for a surface of that age.
    pub fn cull_repair(&self, age: u32, scratch: &mut Plane, out: &mut Vec<u32>) {
        let bb = self.repair_into(age, scratch);
        self.cull_tiles(scratch, bb, out);
    }

    /// Core of every cull: records whose ink meets `bbox`, and (`TILES`) touches a set tile of
    /// `plane`. Every span test is clipped to `clip`, a rect outside which the plane is clear (the
    /// plane's pixel bbox), so a container spanning the window costs what its damaged rows cost.
    /// `POS`: emit global positions instead of payloads. `GENERAL`: `clip != bbox`, so a record or
    /// group that meets `bbox` may miss `clip` (one extra emptiness test).
    #[inline(always)]
    fn cull_impl<const TILES: bool, const POS: bool, const GENERAL: bool>(&self, bbox: NRect, clip: NRect, plane: &Plane, out: &mut Vec<u32>) {
        if bbox.is_empty() {
            return;
        }
        let t = Thresh::of_rect(bbox);
        let tclip = if GENERAL { Thresh::of_rect(clip) } else { t };
        let nc = self.c_ink.len();
        let mut cb = 0;
        let mut iota = [0u32; CAP];
        while cb < nc {
            let ccnt = (nc - cb).min(64);
            // SAFETY: cb + ccnt <= c_ink.len(); ccnt <= 64.
            let mut cm = unsafe { lt_mask(self.c_ink.as_ptr().add(cb), ccnt, t) };
            while cm != 0 {
                let c = cb + cm.trailing_zeros() as usize;
                cm &= cm - 1;
                let b = c * CHUNK;
                // the block's live pages (its holes are EMPTY and would fail anyway: a short block is a
                // short scan)
                // SAFETY: c is a block; its live entries are in bounds; cnt <= 64.
                let cnt = unsafe { *self.r_len.get_unchecked(c) } as usize;
                let mut pm = unsafe { lt_mask(self.d_ink.as_ptr().add(b), cnt, t) };
                // One reservation per chunk: every surviving page emits <= 64 + 8 (overstore).
                out.reserve(pm.count_ones() as usize * CAP + GROUP);
                while pm != 0 {
                    let d = b + pm.trailing_zeros() as usize;
                    pm &= pm - 1;
                    let p = self.page(d);
                    stat!(0, 1);
                    if p.is_xf() {
                        // A transformed page stores local inks: the lane tests below would compare them
                        // against an absolute threshold. Test each record's transformed bounds instead
                        // (out of line: a transformed page holds one transformed subtree).
                        self.cull_xf_page::<TILES, POS>(d, bbox, clip, plane, out);
                        continue;
                    }
                    let src: *const u32 = if POS {
                        let s0 = self.start_of(d) as u32;
                        for (i, v) in iota.iter_mut().enumerate() {
                            *v = s0 + i as u32;
                        }
                        iota.as_ptr()
                    } else {
                        p.payload.as_ptr()
                    };
                    // SAFETY: capacity - len >= (pages left in this chunk) * 64 + 8 (compact8 contract).
                    unsafe {
                        let start = out.as_mut_ptr();
                        let mut w = start.add(out.len());
                        let tp = t.shift_lanes(p.offl);
                        // per page: the clip in page coordinates and the stored->absolute lanes
                        let (clip_rel, dl) = if TILES { (p.rel_sat(clip), p.off_lanes()) } else { (clip, [0; 4]) };
                        let mut gm = lt8(p.gink.as_ptr(), tp);
                        while gm != 0 {
                            let g = gm.trailing_zeros() as usize;
                            gm &= gm - 1;
                            // Clipped groups test their visible inks (ink ∩ clip), materialised once.
                            let vis;
                            let ink: *const NRect = if p.cgroups & (1 << g) != 0 {
                                vis = self.group_visible(p, g);
                                vis.as_ptr()
                            } else {
                                p.ink.as_ptr().add(g * GROUP)
                            };
                            let mut rm = lt8(ink, tp);
                            stat!(1, 1);
                            if TILES {
                                if GENERAL {
                                    if p.gink.get_unchecked(g).intersect(clip_rel).is_empty() {
                                        continue;
                                    }
                                    rm &= lt8(ink, tclip.shift_lanes(p.offl));
                                }
                                rm = group_tiles(p, g, ink, rm, plane, clip_rel, dl);
                            }
                            w = compact8(src.add(g * GROUP), rm, w);
                        }
                        out.set_len(w.offset_from(start) as usize);
                    }
                }
            }
            cb += 64;
        }
    }

    /// The cull of one transformed page: record by record on the absolute bounds of the transformed
    /// (and clipped) ink, the same bounds `hit` and the directory summary use.
    #[inline(never)]
    fn cull_xf_page<const TILES: bool, const POS: bool>(&self, d: usize, bbox: NRect, clip: NRect, plane: &Plane, out: &mut Vec<u32>) {
        let p = self.page(d);
        let base = if POS { self.start_of(d) as u32 } else { 0 };
        for s in 0..self.d_len[d] as usize {
            let v = self.visible_in(p, s);
            if v.intersect(bbox).is_empty() {
                continue;
            }
            if TILES {
                let c = v.intersect(clip);
                if c.is_empty() || !plane.hits(c, self.ring.window) {
                    continue;
                }
            }
            out.push(if POS { base + s as u32 } else { p.payload[s] });
        }
    }

    /// Positions (ascending) of records whose ink touches a tile damaged after `epoch`. Tile
    /// granular (the planes are the only history); older than the ring: every record.
    pub fn changed_since(&self, epoch: u32, out: &mut Vec<u32>) {
        out.clear();
        let age = self.ring.epoch.saturating_sub(epoch);
        if age == 0 {
            return;
        }
        if age as usize > RING {
            out.extend(0..self.total as u32);
            return;
        }
        let pl = self.repair(age);
        let bb = pl.px_bbox(self.ring.window);
        self.cull_impl::<true, true, false>(bb, bb, &pl, out);
    }

    // ------------------------------------------------------------------ epoch snapshots

    /// Make `dst` an exact, read-only replica of `self` for a reader thread (render/encode,
    /// accessibility, inspector): pages are copied only when their write generation differs from
    /// the replica's copy (a turn that touched k pages copies k * 1.5 KiB); the directory columns,
    /// clip table and damage planes are copied whole. `dst` must have been created with the same
    /// window and only ever be written by `freeze_into`. Handles are not replicated. Returns the
    /// number of pages copied.
    pub fn freeze_into(&self, dst: &mut Bough) -> usize {
        if dst.frozen == (self.uid, self.mgen) {
            // nothing a replica holds has changed since it last copied this Bough: only damage and the
            // epoch can have moved
            dst.ring.copy_from(&self.ring);
            return 0;
        }
        dst.frozen = (self.uid, self.mgen);
        let mut copied = 0;
        dst.d_ptr.clear();
        dst.d_ptr.resize(self.d_ptr.len(), core::ptr::null_mut());
        for d in self.dirs() {
            let sp = self.page(d);
            let dp = dst.pool.ensure(sp.id);
            // SAFETY: dp is a live page of dst's pool, distinct from every page of self.
            unsafe {
                if (*dp).wgen != sp.wgen || (*dp).version != sp.version {
                    core::ptr::copy_nonoverlapping(sp as *const Page, dp, 1);
                    copied += 1;
                }
            }
            dst.d_ptr[d] = dp;
        }
        dst.d_id.clone_from_slice_of(&self.d_id);
        dst.d_ink.clone_from_slice_of(&self.d_ink);
        dst.d_min_depth.clone_from(&self.d_min_depth);
        dst.d_len.clone_from(&self.d_len);
        dst.c_ink.clone_from(&self.c_ink);
        dst.wide.clone_from(&self.wide);
        dst.d_local.clone_from(&self.d_local);
        dst.r_len.clone_from(&self.r_len);
        dst.r_recs.clone_from(&self.r_recs);
        dst.r_start.clone_from(&self.r_start);
        dst.r_min_depth.clone_from(&self.r_min_depth);
        dst.np = self.np;
        dst.clips.clone_from_slice_of(&self.clips);
        dst.ring.copy_from(&self.ring);
        dst.total = self.total;
        dst.uniform = self.uniform;
        copied
    }

    // ------------------------------------------------------------------ memory & validation

    /// Heap bytes owned by the structure (pages, directory, prefix, handle table, planes).
    pub fn heap_bytes(&self) -> usize {
        let n = self.d_ptr.capacity();
        self.pool.heap_bytes()
            + n * (8 + 8 + 2 + 4)
            + self.c_ink.capacity() * 8
            + self.state.heap_bytes()
            + self.d_local.capacity() * 2
            + (self.r_len.capacity() + self.r_recs.capacity() + self.r_start.capacity()) * 4
            + self.r_min_depth.capacity() * 2
            + self.htab.capacity() * core::mem::size_of::<HEntry>()
            + core::mem::size_of::<Plane>() * RING
    }

    /// Exhaustive structural validation (tests / debug).
    pub fn check_invariants(&self) {
        // the directory: blocks of CHUNK entries, live pages first, sentinel holes after; the per-block
        // columns are what their entries say
        let nr = self.r_len.len();
        let n = nr << RUN_SHIFT;
        assert_eq!(self.pend, (usize::MAX, 0), "stale blocks left");
        assert!(self.d_ptr[n].is_null());
        assert_eq!((self.d_ptr.len() - 1, self.d_ink.len(), self.d_id.len(), self.d_min_depth.len(), self.d_len.len(), self.d_local.len()), (n, n, n, n, n, n));
        assert_eq!((self.c_ink.len(), self.r_recs.len(), self.r_min_depth.len(), self.r_start.len()), (nr, nr, nr, nr + 1));
        assert_eq!(self.r_start[0], 0);
        let mut live = 0usize;
        for r in 0..nr {
            let (b, l) = (r << RUN_SHIFT, self.r_len[r] as usize);
            assert!((1..=CHUNK).contains(&l), "block {r} holds {l} pages");
            live += l;
            let mut acc = 0u32;
            for i in 0..CHUNK {
                if i < l {
                    assert_eq!(self.d_local[b + i] as u32, acc, "block {r} local start {i}");
                    acc += self.d_len[b + i];
                } else {
                    assert!(self.d_ptr[b + i].is_null() && self.d_ink[b + i] == NRect::EMPTY && self.d_min_depth[b + i] == PAD_DEPTH && self.d_len[b + i] == 0 && self.d_local[b + i] == LOCAL_PAD, "block {r} hole {i}");
                }
            }
            assert_eq!(self.r_recs[r], acc, "block {r} record count");
            assert_eq!(self.r_start[r + 1], self.r_start[r] + acc, "block {r} start");
            assert_eq!(self.r_min_depth[r], self.d_min_depth[b..b + l].iter().copied().min().unwrap(), "block {r} least depth");
        }
        assert_eq!(live, self.np, "live page count");
        let mut sum = 0usize;
        let mut live_handles = 0usize;
        let mut prev_depth: Option<u16> = None;
        let mut starts: Vec<u32> = self.dirs().map(|d| self.start_of(d) as u32).collect();
        starts.push(self.total as u32);
        let mut seq = 0usize;
        for d in self.dirs() {
            seq += 1;
            let p = self.page(d);
            assert_eq!(p.dir, d as u32, "page dir index");
            assert_eq!(self.d_id[d], p.id, "directory page id column");
            assert_eq!(p.dirty, 0, "page left with stale groups");
            for g in 0..NGROUPS {
                let any = p.clip[g * GROUP..g * GROUP + GROUP].iter().any(|&c| c != 0);
                assert_eq!(p.cgroups >> g & 1 == 1, any, "page {d} cgroups bit {g}");
            }
            let l = self.d_len[d] as usize;
            assert!((1..=CAP).contains(&l), "page {d} has len {l}");
            // state planes: a page's state byte says whether its state page holds anything; bits only in
            // live slots; the summaries are the words'; a changed plane is on its flip list
            if self.state.has(p.id) {
                let sp = self.state.get(p.id);
                let mut q = *sp;
                q.summarise();
                assert_eq!((q.any, q.changed), (sp.any, sp.changed), "page {d} state summaries");
                assert_eq!(p.state != 0, !sp.is_blank(), "page {d} state byte");
                assert_eq!(sp.changed & !sp.listed, 0, "page {d}: a changed plane is not listed");
                for f in 0..STATE_PLANES {
                    assert_eq!((sp.bits[f] | sp.seen[f]) & !low(l), 0, "page {d} plane {f}: state in padding");
                    if sp.listed >> f & 1 != 0 {
                        assert!(self.flip[f].contains(&p.id), "page {d} plane {f}: listed but not on the list");
                    }
                }
            } else {
                assert_eq!(p.state, 0, "page {d}: state byte without a state page");
            }
            // frames (round 6): a plain page's ink is inside the domain and its offset in 16 bits; a far
            // page's offset is in 16 bits on every axis where it is not wholly beyond the domain; a wide
            // page is one record whose exact rect is in the table
            assert_eq!(p.flags & F_XF != 0, p.xm != [0.0; 4] || p.xv != [0.0; 2], "page {d} F_XF");
            assert_eq!(p.offl, [p.off[0] as i16, p.off[1] as i16, (-p.off[0]) as i16, (-p.off[1]) as i16]);
            if p.flags & F_XF == 0 {
                let u = (0..l).fold(WRect::EMPTY, |u, s| u.union(p.abs_wide(p.ink[s])));
                match p.flags {
                    0 => assert!(u.in_domain() && p.off[0].abs() <= 32767 && p.off[1].abs() <= 32767, "plain page {d}: union {u:?} off {:?}", p.off),
                    F_FAR => {
                        for a in 0..2 {
                            assert!(p.off[a].abs() <= 32767 || u.is_empty() || u.0[a] > COORD_MAX || u.0[a + 2] < COORD_MIN, "far page {d} axis {a}: union {u:?} off {:?}", p.off);
                        }
                    }
                    F_WIDE => {
                        let w = self.wide.iter().filter(|e| e.0 == p.id).collect::<Vec<_>>();
                        assert!(l == 1 && p.off == [0, 0] && w.len() == 1 && p.ink[0] == w[0].1.to_nrect(), "wide page {d}");
                    }
                    f => panic!("page {d} flags {f}"),
                }
            }
            assert_eq!(starts[seq - 1] as usize, sum, "page {d} start");
            if sum < self.total {
                assert_eq!(self.locate_base(sum), (d, 0, sum), "locate of page {d}'s first record");
                assert_eq!(self.locate(sum + l - 1), (d, l - 1), "locate of page {d}'s last record");
            }
            for s in l..CAP {
                assert_eq!(p.depth[s], PAD_DEPTH, "pad depth p{d} s{s}");
                assert_eq!(p.ink[s], NRect::EMPTY);
                assert_eq!(p.kind[s], 0);
                assert_eq!(p.payload[s], 0);
                assert_eq!(p.handle[s], 0);
            }
            for g in 0..NGROUPS {
                assert_eq!(p.gink[g], union_all(&p.ink[g * GROUP..g * GROUP + GROUP]), "page {d} group {g} summary");
            }
            for s in 0..l {
                assert_ne!(p.depth[s], PAD_DEPTH);
                if p.ink[s].is_empty() {
                    assert_eq!(p.ink[s], NRect::EMPTY, "non-canonical empty ink");
                }
                match prev_depth {
                    None => assert_eq!(p.depth[s], 0, "first record must be depth 0"),
                    Some(pd) => assert!(p.depth[s] <= pd + 1, "preorder depth jump"),
                }
                prev_depth = Some(p.depth[s]);
                let h = p.handle[s];
                if h != 0 {
                    live_handles += 1;
                    let e = self.htab[h as usize];
                    assert!(e.live && core::ptr::eq(e.page, p) && e.slot == s as u32, "handle table mismatch");
                }
            }
            assert_eq!(self.d_ink[d], p.abs(union_all(&p.ink[..l])), "page {d} ink summary");
            assert_eq!(self.d_min_depth[d], p.depth[..l].iter().copied().min().unwrap());
            sum += l;
        }
        for c in 0..self.c_ink.len() {
            assert_eq!(self.c_ink[c], union_all(&self.d_ink[c << RUN_SHIFT..(c + 1) << RUN_SHIFT]), "block {c} summary");
        }
        assert_eq!(sum, self.total);
        let wide_pages = self.dirs().filter(|&d| self.page(d).flags & F_WIDE != 0).count();
        assert_eq!(wide_pages, self.wide.len(), "wide table");
        let bound = self.htab.iter().filter(|e| e.live && !e.page.is_null()).count();
        assert_eq!(bound, live_handles, "bound handle count");
    }
}

/// Exact tile test for the records `rm` of group `g`: keep the lanes whose ink (clipped to `clip`,
/// outside which the plane is clear) touches a set tile. Every lane of `rm` meets `clip`.
///
/// Representation: the group's clipped tile span `G` (at most 16 x 16 tiles for all but container
/// groups) is cut out of the plane as one `u16` window per row; none/all fall out of that extraction
/// (no per-record work at all), and a partial group tests its 8 records at once in NEON lanes:
/// per lane a column mask `(1 << lx1) - (1 << lx0)` and a row mask, then per row one `tst` of the row
/// window against all 8 column masks.
#[inline(always)]
unsafe fn group_tiles(p: &Page, g: usize, ink: *const NRect, rm: u32, plane: &Plane, clip: NRect, dl: [i16; 4]) -> u32 {
    unsafe {
        // non-empty: the group passed the bbox test, and clip contains the bbox
        let gi = p.gink.get_unchecked(g).intersect(clip).add_lanes(dl);
        let (gx0, gy0, gx1, gy1) = tile_span(gi);
        let (w, h) = (gx1 - gx0, gy1 - gy0);
        stat!(6, h);
        if w > 16 || h > 16 {
            // Container-sized group: classify, then per record.
            let (any, all) = plane.classify_span(gx0, gy0, gx1, gy1);
            stat!(2, !any);
            stat!(3, any && all);
            stat!(4, any && !all);
            if !any {
                return 0;
            }
            if all {
                return rm;
            }
            stat!(5, rm.count_ones());
            return refine(ink, rm, plane, clip, dl);
        }
        let full = (1u32 << w) - 1;
        let mut win = [0u16; 16];
        let (mut any, mut all) = (0u32, full);
        for y in 0..h as usize {
            let v = (*plane.rows.get_unchecked(gy0 as usize + y) >> gx0) as u32 & full;
            *win.get_unchecked_mut(y) = v as u16;
            any |= v;
            all &= v;
        }
        stat!(2, any == 0);
        stat!(3, any != 0 && all == full);
        stat!(4, any != 0 && all != full);
        if any == 0 {
            return 0;
        }
        if all == full {
            return rm;
        }
        stat!(5, rm.count_ones());
        lanes_test(ink, rm, clip, dl, gx0, gy0, &win, h)
    }
}

#[cfg(target_arch = "aarch64")]
#[inline(always)]
unsafe fn lanes_test(ink: *const NRect, rm: u32, clip: NRect, dl: [i16; 4], gx0: u32, gy0: u32, win: &[u16; 16], h: u32) -> u32 {
    use core::arch::aarch64::*;
    unsafe {
        let v = vld4q_s16(ink as *const i16);
        let c = clip.0;
        // clip (page-relative), then to absolute: + (dx, dy, -dx, -dy)
        let x0 = vaddq_s16(vmaxq_s16(v.0, vdupq_n_s16(c[0])), vdupq_n_s16(dl[0]));
        let y0 = vaddq_s16(vmaxq_s16(v.1, vdupq_n_s16(c[1])), vdupq_n_s16(dl[1]));
        let nx1 = vaddq_s16(vmaxq_s16(v.2, vdupq_n_s16(c[2])), vdupq_n_s16(dl[2]));
        let ny1 = vaddq_s16(vmaxq_s16(v.3, vdupq_n_s16(c[3])), vdupq_n_s16(dl[3]));
        // Local tile coordinates: [lx0, lx1) x [ly0, ly1) relative to the group span origin.
        let lx0 = vsubq_s16(vshrq_n_s16::<5>(x0), vdupq_n_s16(gx0 as i16));
        let lx1 = vsubq_s16(vshrq_n_s16::<5>(vmvnq_s16(nx1)), vdupq_n_s16(gx0 as i16 - 1));
        let ly0 = vsubq_s16(vshrq_n_s16::<5>(y0), vdupq_n_s16(gy0 as i16));
        let ly1 = vsubq_s16(vshrq_n_s16::<5>(vmvnq_s16(ny1)), vdupq_n_s16(gy0 as i16 - 1));
        let one = vdupq_n_u16(1);
        // (1 << hi) - (1 << lo): a shift by 16 yields 0, so hi = 16 wraps to the right mask.
        let cm = vsubq_u16(vshlq_u16(one, lx1), vshlq_u16(one, lx0));
        let rmask = vsubq_u16(vshlq_u16(one, ly1), vshlq_u16(one, ly0));
        let mut acc = vdupq_n_u16(0);
        let mut bit = vdupq_n_u16(1);
        for y in 0..h as usize {
            let hit = vtstq_u16(vdupq_n_u16(*win.get_unchecked(y)), cm);
            let inr = vtstq_u16(rmask, bit);
            acc = vorrq_u16(acc, vandq_u16(hit, inr));
            bit = vshlq_n_u16::<1>(bit);
        }
        const W: [u8; 8] = [1, 2, 4, 8, 16, 32, 64, 128];
        (vaddv_u8(vand_u8(vmovn_u16(acc), vld1_u8(W.as_ptr()))) as u32) & rm
    }
}

#[cfg(not(target_arch = "aarch64"))]
#[inline(always)]
unsafe fn lanes_test(ink: *const NRect, rm: u32, clip: NRect, dl: [i16; 4], gx0: u32, gy0: u32, win: &[u16; 16], _h: u32) -> u32 {
    let mut keep = 0;
    let mut m = rm;
    while m != 0 {
        let l = m.trailing_zeros() as usize;
        m &= m - 1;
        let r = unsafe { *ink.add(l) }.intersect(clip).add_lanes(dl);
        let (tx0, ty0, tx1, ty1) = tile_span(r);
        let cm = ((1u32 << (tx1 - gx0)) - (1u32 << (tx0 - gx0))) as u16;
        let hit = (ty0..ty1).any(|y| win[(y - gy0) as usize] & cm != 0);
        keep |= (hit as u32) << l;
    }
    keep
}

/// Records of a container-sized group: one span test each (clipped to `clip`).
#[inline(always)]
fn refine(ink: *const NRect, mut rm: u32, plane: &Plane, clip: NRect, dl: [i16; 4]) -> u32 {
    let mut keep = 0u32;
    while rm != 0 {
        let l = rm.trailing_zeros() as usize;
        rm &= rm - 1;
        // SAFETY: g * 8 + l < CAP.
        let r = unsafe { *ink.add(l) }.intersect(clip).add_lanes(dl);
        let (tx0, ty0, tx1, ty1) = tile_span(r);
        stat!(7, ty1 - ty0);
        keep |= (plane.test_span(tx0, ty0, tx1, ty1) as u32) << l;
    }
    keep
}

/// The least of a block's 64 depths (holes are 0xFFFF): eight loads, seven lane mins, one reduction.
#[inline(always)]
fn block_min(b: &[u16]) -> u16 {
    debug_assert_eq!(b.len(), CHUNK);
    #[cfg(target_arch = "aarch64")]
    // SAFETY: 64 u16 in bounds.
    unsafe {
        use core::arch::aarch64::*;
        let p = b.as_ptr();
        let mut a = vld1q_u16(p);
        for i in 1..8 {
            a = vminq_u16(a, vld1q_u16(p.add(i * 8)));
        }
        vminvq_u16(a)
    }
    #[cfg(not(target_arch = "aarch64"))]
    b.iter().copied().min().unwrap()
}

pub struct Children<'a> {
    b: &'a Bough,
    next: usize,
    end: usize,
}
impl Iterator for Children<'_> {
    type Item = usize;
    #[inline]
    fn next(&mut self) -> Option<usize> {
        if self.next >= self.end {
            return None;
        }
        let c = self.next;
        self.next = self.b.subtree_end(c);
        Some(c)
    }
}

/// Copy virtual sequence `[lo, hi)` of `head(old[0..s)) ++ recs ++ tail(old[s..])` into `dst[0..]`.
#[inline]
fn copy_virtual(dst: &mut Page, lo: usize, hi: usize, old: &Page, s: usize, k: usize, recs: &[RecordInit]) {
    let b = hi.min(s);
    if lo < b {
        dst.copy_from(0, old, lo, b - lo);
    }
    let (a, b) = (lo.max(s), hi.min(s + k));
    for v in a..b {
        dst.write(v - lo, &recs[v - s]);
    }
    let a = lo.max(s + k);
    if a < hi {
        dst.copy_from(a - lo, old, a - k, hi - a);
    }
}

/// The tile rows a convex quad (absolute px) covers, grown by a pixel: one rect per 32-px row band, spanning
/// the quad's x extent inside the band.
fn quad_cover(q: &[[f32; 2]; 4], mut f: impl FnMut(NRect)) {
    let ymin = q.iter().map(|p| p[1]).fold(f32::MAX, f32::min) - 1.0;
    let ymax = q.iter().map(|p| p[1]).fold(f32::MIN, f32::max) + 1.0;
    let mut ty = (ymin / 32.0).floor() as i32;
    let last = (ymax / 32.0).floor() as i32;
    while ty <= last {
        let (b0, b1) = ((ty * 32) as f32 - 1.0, (ty * 32 + 32) as f32 + 1.0);
        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
        for i in 0..4 {
            let (a, b) = (q[i], q[(i + 1) % 4]);
            let (t0, t1) = if (b[1] - a[1]).abs() < 1e-9 {
                if a[1] < b0 || a[1] > b1 {
                    continue;
                }
                (0.0, 1.0)
            } else {
                let (ta, tb) = ((b0 - a[1]) / (b[1] - a[1]), (b1 - a[1]) / (b[1] - a[1]));
                let (t0, t1) = (ta.min(tb).max(0.0), ta.max(tb).min(1.0));
                if t0 > t1 {
                    continue;
                }
                (t0, t1)
            };
            for t in [t0, t1] {
                let x = a[0] + (b[0] - a[0]) * t;
                lo = lo.min(x);
                hi = hi.max(x);
            }
        }
        if lo <= hi {
            f(NRect::new((lo - 1.0).floor() as i32, ty * 32, (hi + 1.0).ceil() as i32, ty * 32 + 32));
        }
        ty += 1;
    }
}
