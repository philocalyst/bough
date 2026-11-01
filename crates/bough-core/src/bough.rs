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

use crate::damage::{DamageRing, Plane, RING, tile_span};
use crate::emit::compact8;
use crate::nrect::{NRect, Thresh, lt_mask, lt8, translate_union, union_all};
use crate::page::{CAP, GROUP, NGROUPS, PAD_DEPTH, Page, PagePool, Record, find_first_le, find_last_lt, page_find_first_le, page_find_last_lt};
pub use crate::page::RecordInit;
use crate::prefix::Prefix;

/// Pages below this fill are merged into a neighbour when they fit (bbtree `minimum`).
pub const MIN_FILL: usize = 8;
/// After an end-biased split the side that keeps receiving inserts holds `min(2*MIN_FILL, CAP/2)`.
pub const RECEIVER_KEEPS: usize = if 2 * MIN_FILL < CAP / 2 { 2 * MIN_FILL } else { CAP / 2 };
/// Directory pages per chunk summary.
pub const CHUNK: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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
    d_ptr: Vec<*mut Page>,
    d_ink: Vec<NRect>,
    d_min_depth: Vec<u16>,
    d_len: Vec<u32>,
    /// Union of `d_ink` per chunk of 64 directory entries.
    c_ink: Vec<NRect>,
    prefix: Prefix,
    htab: Vec<HEntry>,
    hfree: Vec<u32>,
    ring: DamageRing,
    total: usize,
    scratch: Vec<RecordInit>,
    dirty_scratch: Vec<u32>,
    /// Clip rects by id (absolute); id 0 = the whole domain (unclipped).
    clips: Vec<NRect>,
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
    pub fn new(window_w: i32, window_h: i32) -> Bough {
        Bough {
            pool: PagePool::new(),
            d_ptr: Vec::new(),
            d_ink: Vec::new(),
            d_min_depth: Vec::new(),
            d_len: Vec::new(),
            c_ink: Vec::new(),
            prefix: Prefix::new(),
            htab: vec![HEntry { page: core::ptr::null_mut(), slot: 0, generation: 0, live: false }],
            hfree: Vec::new(),
            ring: DamageRing::new(window_w, window_h),
            total: 0,
            scratch: Vec::new(),
            dirty_scratch: Vec::new(),
            clips: vec![NRect::new(crate::nrect::COORD_MIN, crate::nrect::COORD_MIN, crate::nrect::COORD_MAX, crate::nrect::COORD_MAX)],
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
    #[inline(always)]
    pub fn num_pages(&self) -> usize {
        self.d_ptr.len()
    }
    pub fn pool(&self) -> &PagePool {
        &self.pool
    }
    pub fn window(&self) -> NRect {
        self.ring.window
    }
    /// Mark `kind` as uniform content (solid / tinted rects): its ink changes damage only
    /// `old XOR new` instead of `old UNION new`.
    pub fn set_kind_uniform(&mut self, kind: u8, uniform: bool) {
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
            d += 1;
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
        let (d, base) = self.prefix.locate(pos, &self.d_len);
        (d, pos - base, base)
    }
    #[inline(always)]
    fn start_of(&self, d: usize) -> usize {
        self.prefix.get(d, &self.d_len)
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
        for d in 0..self.d_ptr.len() {
            let p = self.page(d);
            for s in 0..self.d_len[d] as usize {
                v.push(p.read(s));
            }
        }
        v
    }
    /// Start offsets of every directory page (the chunking removal damage is recorded at).
    pub fn page_starts(&self) -> Vec<usize> {
        (0..self.d_ptr.len()).map(|d| self.start_of(d)).collect()
    }
    pub fn page_versions(&self) -> Vec<(u32, u32)> {
        (0..self.d_ptr.len()).map(|d| (self.page(d).id, self.page(d).version)).collect()
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

    // ------------------------------------------------------------------ directory plumbing

    /// Recompute the chunk summaries covering directory entries `[lo, hi)` (and resize the column).
    #[inline]
    fn fix_chunks(&mut self, lo: usize, hi: usize) {
        let n = self.d_ink.len();
        let nc = n.div_ceil(CHUNK);
        self.c_ink.resize(nc, NRect::EMPTY);
        if n == 0 {
            return;
        }
        // Pages may have been removed at the tail: the chunk that held them still needs a refresh.
        let lo = lo.min(n - 1);
        let hi = hi.min(n).max(lo + 1);
        for c in lo / CHUNK..hi.div_ceil(CHUNK) {
            let e = ((c + 1) * CHUNK).min(n);
            self.c_ink[c] = union_all(&self.d_ink[c * CHUNK..e]);
        }
    }
    /// Page `d`'s summary changed from `old` to `self.d_ink[d]`: keep its chunk exact.
    #[inline(always)]
    fn chunk_update(&mut self, d: usize, old: NRect) {
        let new = self.d_ink[d];
        if new == old {
            return;
        }
        let c = d / CHUNK;
        // SAFETY: c < c_ink.len() because d < n.
        let cs = unsafe { self.c_ink.get_unchecked_mut(c) };
        if new.union(old) == new {
            // grew (or stayed): monotone
            *cs = cs.union(new);
        } else {
            self.fix_chunks(d, d + 1);
        }
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

    fn reindex_pages(&mut self, from: usize) {
        for i in from..self.d_ptr.len() {
            // SAFETY: live page.
            unsafe { (*self.d_ptr[i]).dir = i as u32 };
        }
    }

    /// Insert `count` blank, empty pages at directory index `at`.
    fn dir_insert_blank(&mut self, at: usize, count: usize) {
        let mut ptrs = Vec::with_capacity(count);
        for _ in 0..count {
            ptrs.push(self.pool.alloc());
        }
        self.d_ptr.splice(at..at, ptrs);
        self.d_ink.splice(at..at, core::iter::repeat_n(NRect::EMPTY, count));
        self.d_min_depth.splice(at..at, core::iter::repeat_n(PAD_DEPTH, count));
        self.d_len.splice(at..at, core::iter::repeat_n(0, count));
        for _ in 0..count {
            self.prefix.grow();
        }
        self.reindex_pages(at);
    }

    fn dir_remove(&mut self, d: usize) {
        // SAFETY: live page.
        let id = unsafe { (*self.d_ptr[d]).id };
        self.d_ptr.remove(d);
        self.d_ink.remove(d);
        self.d_min_depth.remove(d);
        self.d_len.remove(d);
        self.prefix.shrink();
        self.pool.free(id);
        self.reindex_pages(d);
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

    fn insert_impl(&mut self, pos: usize, recs: &[RecordInit], damage: bool) {
        let k = recs.len();
        if k == 0 {
            return;
        }
        assert!(pos <= self.total, "insert position out of range");
        if damage {
            for chunk in recs.chunks(CAP) {
                let mut u = NRect::EMPTY;
                for r in chunk {
                    u = u.union(self.clipped(r.ink.canon(), r.clip));
                }
                self.ring.add(u);
            }
        }
        if self.d_ptr.is_empty() {
            self.dir_insert_blank(0, 1);
            self.prefix.invalidate_from(0, &self.d_len);
            self.fix_chunks(0, 1);
        }
        // Insert after the record at pos-1 (same page); at 0, at the front of page 0.
        let (d, s) = if pos == 0 {
            (0, 0)
        } else {
            let (d, s) = self.locate(pos - 1);
            (d, s + 1)
        };
        let l = self.d_len[d] as usize;
        if l + k <= CAP {
            self.insert_in_page(d, s, recs);
        } else {
            self.insert_split(d, s, recs);
        }
        self.total += k;
    }

    fn insert_in_page(&mut self, d: usize, s: usize, recs: &[RecordInit]) {
        let l = self.d_len[d] as usize;
        let k = recs.len();
        let p = self.page_mut(d);
        p.normalize();
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
        self.fixup_handles(d, s, l + k);
        self.d_len[d] = (l + k) as u32;
        let old = self.d_ink[d];
        self.d_ink[d] = u;
        self.d_min_depth[d] = self.d_min_depth[d].min(md);
        self.chunk_update(d, old);
        self.prefix.delta(d, k as i32, &self.d_len);
    }

    fn insert_split(&mut self, d: usize, s: usize, recs: &[RecordInit]) {
        let l = self.d_len[d] as usize;
        let k = recs.len();
        let t = l + k;
        let m = t.div_ceil(CAP);
        let sizes = Sizes::new(t, m, s, l);
        self.page_mut(d).normalize();
        self.dir_insert_blank(d + 1, m - 1);
        let sz0 = sizes.size(0);
        let mut off = sz0;
        for i in 1..m {
            let szi = sizes.size(i);
            let di = d + i;
            // SAFETY: di != d, both live distinct pool pages.
            let (newp, oldp) = unsafe { (&mut *self.page_ptr(di), &*self.page_ptr(d)) };
            newp.wgen = newp.wgen.wrapping_add(1);
            copy_virtual(newp, off, off + szi, oldp, s, k, recs);
            newp.version += 1;
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
        self.d_len[d] = sz0 as u32;
        if sz0 > s {
            self.fixup_handles(d, s, sz0);
        }
        for i in 0..m {
            self.refresh(d + i);
        }
        self.prefix.invalidate_from(d, &self.d_len);
        self.fix_chunks(d, self.d_ptr.len());
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
        let mut freed = false;
        let mut trimmed: [(usize, usize); 4] = [(0, 0); 4];
        let mut ntrim = 0usize;
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
                self.dir_remove(d);
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
                self.d_len[d] = (l - n) as u32;
                let old = self.d_ink[d];
                self.d_ink[d] = self.page(d).union_abs();
                self.d_min_depth[d] = self.page(d).min_depth();
                if !freed {
                    self.chunk_update(d, old);
                }
                if s + n < l {
                    self.fixup_handles(d, s, l - n);
                }
                d += 1;
            }
            s = 0;
            remaining -= n;
        }
        self.total -= len;
        if !freed && ntrim <= 4 {
            // Pure trims: add each page's negative delta instead of rebuilding.
            for &(td, n) in &trimmed[..ntrim] {
                self.prefix.delta(td, -(n as i32), &self.d_len);
            }
        } else {
            self.prefix.invalidate_from(d0.min(self.d_len.len()), &self.d_len);
            self.fix_chunks(d0, self.d_ptr.len());
        }
        // Underflow: merge small neighbours around the cut.
        let mut j = (d0 + 1).min(self.d_ptr.len().saturating_sub(1)) as isize;
        let lo = d0.saturating_sub(1) as isize;
        while j >= lo {
            let i = j as usize;
            if i + 1 < self.d_ptr.len() {
                let (a, b) = (self.d_len[i] as usize, self.d_len[i + 1] as usize);
                if a + b <= CAP && (a < MIN_FILL || b < MIN_FILL) {
                    self.merge_next(i);
                }
            }
            j -= 1;
        }
        if self.d_ptr.is_empty() {
            self.c_ink.clear();
        }
    }

    fn merge_next(&mut self, d: usize) {
        let (la, lb) = (self.d_len[d] as usize, self.d_len[d + 1] as usize);
        {
            // SAFETY: d and d+1 are distinct live pages.
            let (a, b) = unsafe { (&mut *self.page_ptr(d), &mut *self.page_ptr(d + 1)) };
            a.wgen = a.wgen.wrapping_add(1);
            b.wgen = b.wgen.wrapping_add(1);
            a.normalize();
            b.normalize();
            a.version += 1;
            a.copy_from(la, b, 0, lb);
            a.regroup_range(la, la + lb);
        }
        self.fixup_handles(d, la, la + lb);
        self.d_len[d] = (la + lb) as u32;
        self.d_ink[d] = self.d_ink[d].union(self.d_ink[d + 1]);
        self.d_min_depth[d] = self.d_min_depth[d].min(self.d_min_depth[d + 1]);
        self.dir_remove(d + 1);
        self.prefix.invalidate_from(d, &self.d_len);
        self.fix_chunks(d, self.d_ptr.len());
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
        let (mut d, mut s) = self.locate(start);
        let mut rem = len;
        while rem > 0 {
            let l = self.d_len[d] as usize;
            let n = rem.min(l - s);
            let p = self.page(d);
            for i in s..s + n {
                stage.push(RecordInit {
                    depth: (p.depth[i] as i32 + ddepth) as u16,
                    ink: p.abs(p.ink[i]),
                    kind: p.kind[i],
                    payload: p.payload[i],
                    handle: p.handle[i],
                    clip: p.clip[i],
                });
            }
            rem -= n;
            d += 1;
            s = 0;
        }
        self.remove_impl(start, len, false, true);
        self.insert_impl(to, &stage, false);
        stage.clear();
        self.scratch = stage;
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
        let (d0, mut s) = self.locate(start);
        let mut d = d0;
        let mut rem = len;
        let (mut uo, mut un) = (NRect::EMPTY, NRect::EMPTY);
        while rem > 0 {
            let l = self.d_len[d] as usize;
            let n = rem.min(l - s);
            let old_d = self.d_ink[d];
            if s == 0 && n == l {
                // Whole page: the offset moves, the records do not (O(1) per page).
                let p = self.page_mut(d);
                let (nx, ny) = (p.off[0] + dx, p.off[1] + dy);
                if nx.abs() > 32767 || ny.abs() > 32767 {
                    // keep |off| in i16: fold the offset into the stored inks, then move them
                    p.normalize();
                    // SAFETY: l live in-bounds slots; absolute results in the domain by contract.
                    unsafe { translate_union(p.ink.as_mut_ptr(), l, dx, dy) };
                    p.regroup_range(0, l);
                } else {
                    p.set_off(nx, ny);
                }
                let new_d = old_d.translate(dx, dy);
                self.d_ink[d] = new_d;
                uo = uo.union(old_d);
                un = un.union(new_d);
            } else {
                let p = self.page_mut(d);
                // Partial page: its records move relative to the others, so the offset is applied to
                // the stored inks first (relative coordinates could otherwise leave the domain).
                p.normalize();
                // SAFETY: slots s..s+n are live in-bounds ink entries; domain contract in nrect docs.
                let (old, new) = unsafe { translate_union(p.ink.as_mut_ptr().add(s), n, dx, dy) };
                p.regroup_range(s, s + n);
                let u = p.union_abs();
                // normalised: stored = absolute
                self.d_ink[d] = u;
                uo = uo.union(old);
                un = un.union(new);
            }
            rem -= n;
            d += 1;
            s = 0;
        }
        self.fix_chunks(d0, d);
        (uo, un)
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
        if old == ink {
            return;
        }
        let (kind, id) = (p.kind[s], p.clip[s]);
        p.store_abs(s, ink);
        p.regroup(s / GROUP);
        let u = p.union_abs();
        let od = self.d_ink[d];
        self.d_ink[d] = u;
        self.chunk_update(d, od);
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
            if old == ink {
                continue;
            }
            let (kind, id) = (p.kind[s], p.clip[s]);
            p.store_abs(s, ink);
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
        let old = pg.abs(pg.ink[s]);
        if old == ink {
            return;
        }
        let (kind, id) = (pg.kind[s], pg.clip[s]);
        pg.store_abs(s, ink);
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

    /// Mark the record's ink damaged (a paint-only write: colour, glyph content, image).
    #[inline]
    pub fn touch(&mut self, pos: usize) {
        let (d, s) = self.locate(pos);
        let ink = self.visible(d, s);
        self.ring.add(ink);
    }
    #[inline]
    pub fn touch_handle(&mut self, h: Handle) -> bool {
        match self.handle_ptr(h) {
            Some((p, s)) => {
                // SAFETY: bound handles point at live pages; s < CAP.
                let ink = self.visible_in(unsafe { &*p }, s);
                self.ring.add(ink);
                true
            }
            None => false,
        }
    }

    // ------------------------------------------------------------------ structure queries

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
        match find_first_le(&self.d_min_depth, d + 1, dpt) {
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
        let d2 = find_last_lt(&self.d_min_depth, d, dpt)?;
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
            let Some(d2) = find_last_lt(&self.d_min_depth, d, dpt) else { break };
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
            match find_first_le(&self.d_min_depth, d + 1, tgt) {
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

    /// Topmost (last in preorder) record whose ink contains pixel (x, y).
    /// Descent: chunk -> page -> group -> record, each a SIMD point test, scanned from the top.
    pub fn hit(&self, x: i32, y: i32) -> Option<usize> {
        let t = Thresh::of_point(x, y)?;
        let n = self.d_ptr.len();
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
                let cnt = (n - b).min(CHUNK);
                // SAFETY: b + cnt <= n = d_ink.len(); cnt <= 64.
                let mut pm = unsafe { lt_mask(self.d_ink.as_ptr().add(b), cnt, t) };
                while pm != 0 {
                    let bit = 63 - pm.leading_zeros() as usize;
                    pm &= !(1u64 << bit);
                    let d = b + bit;
                    let p = self.page(d);
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
        let n = self.d_ptr.len();
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
                let cnt = (n - b).min(CHUNK);
                // SAFETY: b + cnt <= n = d_ink.len(); cnt <= 64.
                let mut pm = unsafe { lt_mask(self.d_ink.as_ptr().add(b), cnt, t) };
                // One reservation per chunk: every surviving page emits <= 64 + 8 (overstore).
                out.reserve(pm.count_ones() as usize * CAP + GROUP);
                while pm != 0 {
                    let d = b + pm.trailing_zeros() as usize;
                    pm &= pm - 1;
                    let p = self.page(d);
                    stat!(0, 1);
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
        let n = self.d_ptr.len();
        let mut copied = 0;
        dst.d_ptr.clear();
        for d in 0..n {
            let sp = self.page(d);
            let dp = dst.pool.ensure(sp.id);
            // SAFETY: dp is a live page of dst's pool, distinct from every page of self.
            unsafe {
                if (*dp).wgen != sp.wgen || (*dp).version != sp.version {
                    core::ptr::copy_nonoverlapping(sp as *const Page, dp, 1);
                    copied += 1;
                }
            }
            dst.d_ptr.push(dp);
        }
        dst.d_ink.clone_from(&self.d_ink);
        dst.d_min_depth.clone_from(&self.d_min_depth);
        dst.d_len.clone_from(&self.d_len);
        dst.c_ink.clone_from(&self.c_ink);
        dst.prefix.copy_from(&self.prefix);
        dst.clips.clone_from(&self.clips);
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
            + self.prefix.heap_bytes()
            + self.htab.capacity() * core::mem::size_of::<HEntry>()
            + core::mem::size_of::<Plane>() * RING
    }

    /// Exhaustive structural validation (tests / debug).
    pub fn check_invariants(&self) {
        let n = self.d_ptr.len();
        assert_eq!(self.d_ink.len(), n);
        assert_eq!(self.d_min_depth.len(), n);
        assert_eq!(self.d_len.len(), n);
        assert_eq!(self.c_ink.len(), n.div_ceil(CHUNK));
        let mut sum = 0usize;
        let mut live_handles = 0usize;
        let mut prev_depth: Option<u16> = None;
        let starts = self.prefix.snapshot(&self.d_len);
        assert_eq!(starts.len(), n + 1);
        for d in 0..n {
            let p = self.page(d);
            assert_eq!(p.dir, d as u32, "page dir index");
            assert_eq!(p.dirty, 0, "page left with stale groups");
            for g in 0..NGROUPS {
                let any = p.clip[g * GROUP..g * GROUP + GROUP].iter().any(|&c| c != 0);
                assert_eq!(p.cgroups >> g & 1 == 1, any, "page {d} cgroups bit {g}");
            }
            let l = self.d_len[d] as usize;
            assert!((1..=CAP).contains(&l), "page {d} has len {l}");
            assert_eq!(starts[d] as usize, sum);
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
            let e = ((c + 1) * CHUNK).min(n);
            assert_eq!(self.c_ink[c], union_all(&self.d_ink[c * CHUNK..e]), "chunk {c} summary");
        }
        assert_eq!(starts[n] as usize, sum);
        assert_eq!(sum, self.total);
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
