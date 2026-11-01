//! The Bough: paged SoA records in paint preorder + a flat directory of page summaries.
//!
//! # Safety model
//! Pages live in a [`PagePool`] (boxed, never freed while the `Bough` lives). The directory
//! stores raw `*mut Page` for each page so scans avoid a pool indirection. Invariants:
//! every pointer in `d_ptr[0..n]` refers to a distinct live pool page; mutation happens only
//! through `&mut self`; no `&Page`/`&mut Page` obtained from these pointers outlives the
//! method that created it (all helper references are local).

use crate::damage::{DamageRing, Plane};
use crate::nrect::{NRect, Thresh, lt_mask, translate_union, union_all};
use crate::page::{CAP, PAD_DEPTH, Page, PagePool, Record, find_first_le, find_last_lt, page_find_first_le, page_find_last_lt, stamp_gt_mask};
pub use crate::page::RecordInit;
use crate::prefix::Prefix;

/// Pages below this fill are merged into a neighbour when they fit (bbtree `minimum`).
pub const MIN_FILL: usize = 8;
/// After an end-biased split the side that keeps receiving inserts holds `min(2*MIN_FILL, CAP/2)`.
pub const RECEIVER_KEEPS: usize = if 2 * MIN_FILL < CAP / 2 { 2 * MIN_FILL } else { CAP / 2 };

const NO_PAGE: u32 = u32::MAX;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Handle {
    pub idx: u32,
    pub generation: u32,
}

#[derive(Clone, Copy)]
struct HEntry {
    page: u32,
    slot: u32,
    generation: u32,
    live: bool,
}

pub struct Bough {
    pool: PagePool,
    // Directory, SoA (index = directory position).
    d_ptr: Vec<*mut Page>,
    d_id: Vec<u32>,
    d_ink: Vec<NRect>,
    d_max_stamp: Vec<u32>,
    d_min_depth: Vec<u16>,
    d_len: Vec<u32>,
    prefix: Prefix,
    /// page id -> directory index
    page_dir: Vec<u32>,
    htab: Vec<HEntry>,
    hfree: Vec<u32>,
    ring: DamageRing,
    total: usize,
    scratch: Vec<RecordInit>,
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

impl Bough {
    pub fn new(window_w: i32, window_h: i32) -> Bough {
        Bough {
            pool: PagePool::new(),
            d_ptr: Vec::new(),
            d_id: Vec::new(),
            d_ink: Vec::new(),
            d_max_stamp: Vec::new(),
            d_min_depth: Vec::new(),
            d_len: Vec::new(),
            prefix: Prefix::new(),
            page_dir: Vec::new(),
            htab: vec![HEntry { page: NO_PAGE, slot: 0, generation: 0, live: false }],
            hfree: Vec::new(),
            ring: DamageRing::new(window_w, window_h),
            total: 0,
            scratch: Vec::new(),
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
        unsafe { &mut **self.d_ptr.get_unchecked(d) }
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
        self.page(d).ink[s]
    }
    pub fn payload_at(&self, pos: usize) -> u32 {
        let (d, s) = self.locate(pos);
        self.page(d).payload[s]
    }
    pub fn stamp_at(&self, pos: usize) -> u32 {
        let (d, s) = self.locate(pos);
        self.page(d).stamp[s]
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
    /// Start offsets of every directory page (the chunking damage is recorded at).
    pub fn page_starts(&self) -> Vec<usize> {
        (0..self.d_ptr.len()).map(|d| self.start_of(d)).collect()
    }
    pub fn page_versions(&self) -> Vec<(u32, u32)> {
        (0..self.d_ptr.len()).map(|d| (self.d_id[d], self.page(d).version)).collect()
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
        self.ring.add(r.canon());
    }
    pub fn plane(&self, back: u32) -> &Plane {
        self.ring.plane(back)
    }
    /// `repair(age)` into a caller-owned plane (no 1.5 KiB temporary): clears only its dirty rows.
    pub fn repair_into(&self, age: u32, out: &mut Plane) {
        out.clear();
        for back in 0..age {
            out.or_assign(self.ring.plane(back));
        }
    }
    /// OR of the last `age` planes (1..=8), current epoch included.
    pub fn repair(&self, age: u32) -> Plane {
        self.ring.repair(age)
    }

    // ------------------------------------------------------------------ directory plumbing

    fn refresh(&mut self, d: usize) {
        let p = self.page(d);
        let (ink, md, ms) = (p.union_ink(), p.min_depth(), p.max_stamp());
        self.d_ink[d] = ink;
        self.d_min_depth[d] = md;
        self.d_max_stamp[d] = ms;
    }

    fn reindex_pages(&mut self, from: usize) {
        for i in from..self.d_id.len() {
            self.page_dir[self.d_id[i] as usize] = i as u32;
        }
    }

    /// Insert `count` blank, empty pages at directory index `at`.
    fn dir_insert_blank(&mut self, at: usize, count: usize) {
        let mut ptrs = Vec::with_capacity(count);
        let mut ids = Vec::with_capacity(count);
        for _ in 0..count {
            let p = self.pool.alloc();
            // SAFETY: p was just handed out by the pool.
            let id = unsafe { (*p).id };
            ptrs.push(p);
            ids.push(id);
            if self.page_dir.len() <= id as usize {
                self.page_dir.resize(id as usize + 1, NO_PAGE);
            }
        }
        self.d_ptr.splice(at..at, ptrs);
        self.d_id.splice(at..at, ids);
        self.d_ink.splice(at..at, core::iter::repeat_n(NRect::EMPTY, count));
        self.d_max_stamp.splice(at..at, core::iter::repeat_n(0, count));
        self.d_min_depth.splice(at..at, core::iter::repeat_n(PAD_DEPTH, count));
        self.d_len.splice(at..at, core::iter::repeat_n(0, count));
        for _ in 0..count {
            self.prefix.grow();
        }
        self.reindex_pages(at);
    }

    fn dir_remove(&mut self, d: usize) {
        let id = self.d_id[d];
        self.d_ptr.remove(d);
        self.d_id.remove(d);
        self.d_ink.remove(d);
        self.d_max_stamp.remove(d);
        self.d_min_depth.remove(d);
        self.d_len.remove(d);
        self.prefix.shrink();
        self.page_dir[id as usize] = NO_PAGE;
        self.pool.free(id);
        self.reindex_pages(d);
    }

    /// Re-point handle-table entries for every live handle in slots `[lo, hi)` of page `d`.
    #[inline]
    fn fixup_handles(&mut self, d: usize, lo: usize, hi: usize) {
        let p = self.page_ptr(d);
        // SAFETY: p is a live page; the handle table is a separate allocation.
        let (id, col) = unsafe { ((*p).id, &(*p).handle) };
        let any = col[lo..hi].iter().fold(0u32, |a, &b| a | b) != 0;
        if any {
            for s in lo..hi {
                let h = col[s];
                if h != 0 {
                    let e = &mut self.htab[h as usize];
                    e.page = id;
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
            e.page = NO_PAGE;
            Handle { idx, generation: e.generation }
        } else {
            let idx = self.htab.len() as u32;
            self.htab.push(HEntry { page: NO_PAGE, slot: 0, generation: 0, live: true });
            Handle { idx, generation: 0 }
        }
    }
    fn release_handle(&mut self, idx: u32) {
        let e = &mut self.htab[idx as usize];
        debug_assert!(e.live);
        e.live = false;
        e.page = NO_PAGE;
        e.generation = e.generation.wrapping_add(1);
        self.hfree.push(idx);
    }
    #[inline(always)]
    fn handle_loc(&self, h: Handle) -> Option<(usize, usize)> {
        let e = self.htab.get(h.idx as usize)?;
        if !e.live || e.generation != h.generation || e.page == NO_PAGE {
            return None;
        }
        Some((self.page_dir[e.page as usize] as usize, e.slot as usize))
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
                    u = u.union(r.ink.canon());
                }
                self.ring.add(u);
            }
        }
        if self.d_ptr.is_empty() {
            self.dir_insert_blank(0, 1);
            self.prefix.invalidate_from(0, &self.d_len);
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
        let epoch = self.ring.epoch;
        let p = self.page_mut(d);
        p.version += 1;
        if s < l {
            p.shift(s, s + k, l - s);
        }
        let mut u = NRect::EMPTY;
        let mut md = PAD_DEPTH;
        for (i, r) in recs.iter().enumerate() {
            p.write(s + i, r, epoch);
            u = u.union(p.ink[s + i]);
            md = md.min(r.depth);
        }
        self.fixup_handles(d, s, l + k);
        self.d_len[d] = (l + k) as u32;
        self.d_ink[d] = self.d_ink[d].union(u);
        self.d_min_depth[d] = self.d_min_depth[d].min(md);
        self.d_max_stamp[d] = epoch;
        self.prefix.delta(d, k as i32, &self.d_len);
    }

    fn insert_split(&mut self, d: usize, s: usize, recs: &[RecordInit]) {
        let l = self.d_len[d] as usize;
        let k = recs.len();
        let t = l + k;
        let m = t.div_ceil(CAP);
        let epoch = self.ring.epoch;
        let sizes = Sizes::new(t, m, s, l);
        self.dir_insert_blank(d + 1, m - 1);
        let sz0 = sizes.size(0);
        let mut off = sz0;
        for i in 1..m {
            let szi = sizes.size(i);
            let di = d + i;
            // SAFETY: di != d, both live distinct pool pages.
            let (newp, oldp) = unsafe { (&mut *self.page_ptr(di), &*self.page_ptr(d)) };
            copy_virtual(newp, off, off + szi, oldp, s, k, recs, epoch);
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
                p.write(s + i, r, epoch);
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
                let u = if s == 0 && n == l { self.d_ink[d] } else { union_all(&self.page(d).ink[s..s + n]) };
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
                self.d_len[d] = (l - n) as u32;
                self.refresh(d);
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
            // Pure trims: add each page's negative delta to the suffix (NEON) instead of rebuilding.
            for &(td, n) in &trimmed[..ntrim] {
                self.prefix.delta(td, -(n as i32), &self.d_len);
            }
        } else {
            self.prefix.invalidate_from(d0.min(self.d_len.len()), &self.d_len);
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
    }

    fn merge_next(&mut self, d: usize) {
        let (la, lb) = (self.d_len[d] as usize, self.d_len[d + 1] as usize);
        {
            // SAFETY: d and d+1 are distinct live pages.
            let (a, b) = unsafe { (&mut *self.page_ptr(d), &*self.page_ptr(d + 1)) };
            a.version += 1;
            a.copy_from(la, b, 0, lb);
        }
        self.fixup_handles(d, la, la + lb);
        self.d_len[d] = (la + lb) as u32;
        self.d_ink[d] = self.d_ink[d].union(self.d_ink[d + 1]);
        self.d_min_depth[d] = self.d_min_depth[d].min(self.d_min_depth[d + 1]);
        self.d_max_stamp[d] = self.d_max_stamp[d].max(self.d_max_stamp[d + 1]);
        self.dir_remove(d + 1);
        self.prefix.invalidate_from(d, &self.d_len);
    }

    // ------------------------------------------------------------------ move

    /// Move the run `[start, start+len)` so that it starts at `to` in the resulting sequence
    /// (`to <= len() - run_len`). Stamps the run and damages its ink. Handles survive.
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
        for d in 0..self.d_ptr.len() {
            // collect lazily below; fallthrough to range read
            let _ = d;
            break;
        }
        let (mut d, mut s) = self.locate(start);
        let mut rem = len;
        while rem > 0 {
            let l = self.d_len[d] as usize;
            let n = rem.min(l - s);
            let p = self.page(d);
            for i in s..s + n {
                stage.push(RecordInit {
                    depth: (p.depth[i] as i32 + ddepth) as u16,
                    ink: p.ink[i],
                    kind: p.kind[i],
                    payload: p.payload[i],
                    handle: p.handle[i],
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
    /// columns, fused with both union reductions (old and new support go to damage per page chunk).
    pub fn translate_run(&mut self, start: usize, len: usize, dx: i32, dy: i32) {
        if len == 0 || (dx == 0 && dy == 0) {
            return;
        }
        assert!(start + len <= self.total);
        let epoch = self.ring.epoch;
        let (mut d, mut s) = self.locate(start);
        let mut rem = len;
        while rem > 0 {
            let l = self.d_len[d] as usize;
            let n = rem.min(l - s);
            let p = self.page_mut(d);
            // SAFETY: slots s..s+n are live in-bounds ink entries; domain contract in nrect docs.
            let (old, new) = unsafe { translate_union(p.ink.as_mut_ptr().add(s), n, dx, dy) };
            p.stamp[s..s + n].fill(epoch);
            self.d_max_stamp[d] = epoch;
            self.d_ink[d] = if s == 0 && n == l { new } else { union_all(&self.page(d).ink) };
            self.ring.add(old);
            self.ring.add(new);
            rem -= n;
            d += 1;
            s = 0;
        }
    }

    pub fn set_ink(&mut self, pos: usize, ink: NRect) {
        let (d, s) = self.locate(pos);
        self.set_ink_at(d, s, ink);
    }
    pub fn set_ink_handle(&mut self, h: Handle, ink: NRect) -> bool {
        match self.handle_loc(h) {
            Some((d, s)) => {
                self.set_ink_at(d, s, ink);
                true
            }
            None => false,
        }
    }
    #[inline]
    fn set_ink_at(&mut self, d: usize, s: usize, ink: NRect) {
        let ink = ink.canon();
        let epoch = self.ring.epoch;
        let p = self.page_mut(d);
        let old = p.ink[s];
        p.ink[s] = ink;
        p.stamp[s] = epoch;
        self.d_max_stamp[d] = epoch;
        // Old support first, then new: write-through.
        self.ring.add(old);
        self.ring.add(ink);
        let sum = self.d_ink[d];
        // The old ink may have been what defined the summary: recompute exactly if any lane ties.
        let ties = old.0.iter().zip(sum.0.iter()).any(|(a, b)| a == b);
        self.d_ink[d] = if ties { self.page(d).union_ink() } else { sum.union(ink) };
    }

    /// Stamp the record and capture its ink in the epoch's damage (a paint-only write).
    #[inline]
    pub fn touch(&mut self, pos: usize) {
        let (d, s) = self.locate(pos);
        self.touch_at(d, s);
    }
    #[inline]
    pub fn touch_handle(&mut self, h: Handle) -> bool {
        match self.handle_loc(h) {
            Some((d, s)) => {
                self.touch_at(d, s);
                true
            }
            None => false,
        }
    }
    #[inline(always)]
    fn touch_at(&mut self, d: usize, s: usize) {
        let epoch = self.ring.epoch;
        let p = self.page_mut(d);
        p.stamp[s] = epoch;
        let ink = p.ink[s];
        self.d_max_stamp[d] = epoch;
        self.ring.add(ink);
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

    pub fn children(&self, pos: usize) -> Children<'_> {
        let end = self.subtree_end(pos);
        Children { b: self, next: pos + 1, end }
    }

    // ------------------------------------------------------------------ hit test

    /// Topmost (last in preorder) record whose ink contains pixel (x, y).
    pub fn hit(&self, x: i32, y: i32) -> Option<usize> {
        let t = Thresh::of_point(x, y)?;
        let n = self.d_ptr.len();
        let mut hi = n;
        while hi > 0 {
            let b = (hi - 1) / 64 * 64;
            // SAFETY: b + (hi - b) = hi <= d_ink.len() rects are readable; hi - b <= 64.
            let mut pm = unsafe { lt_mask(self.d_ink.as_ptr().add(b), hi - b, t) };
            while pm != 0 {
                let bit = 63 - pm.leading_zeros() as usize;
                pm &= !(1u64 << bit);
                let d = b + bit;
                // SAFETY: a page's ink column is exactly CAP = 64 rects.
                let rm = unsafe { lt_mask(self.page(d).ink.as_ptr(), CAP, t) };
                if rm != 0 {
                    let slot = 63 - rm.leading_zeros() as usize;
                    return Some(self.start_of(d) + slot);
                }
            }
            hi = b;
        }
        None
    }

    // ------------------------------------------------------------------ cull / changed_since

    /// Payload of every record (paint order) whose ink intersects `bbox` and, if given, touches a
    /// set tile of `tiles`. `out` is cleared first.
    pub fn cull(&self, bbox: NRect, tiles: Option<&Plane>, out: &mut Vec<u32>) {
        out.clear();
        if bbox.is_empty() {
            return;
        }
        let t = Thresh::of_rect(bbox);
        let n = self.d_ptr.len();
        let window = self.ring.window;
        let mut b = 0;
        while b < n {
            let cnt = (n - b).min(64);
            // SAFETY: b + cnt <= n = d_ink.len(); cnt <= 64.
            let mut pm = unsafe { lt_mask(self.d_ink.as_ptr().add(b), cnt, t) };
            while pm != 0 {
                let i = pm.trailing_zeros() as usize;
                pm &= pm - 1;
                let p = self.page(b + i);
                // SAFETY: a page's ink column is exactly CAP = 64 rects.
                let mut rm = unsafe { lt_mask(p.ink.as_ptr(), CAP, t) };
                match tiles {
                    None => {
                        while rm != 0 {
                            let s = rm.trailing_zeros() as usize;
                            rm &= rm - 1;
                            out.push(p.payload[s]);
                        }
                    }
                    Some(pl) => {
                        while rm != 0 {
                            let s = rm.trailing_zeros() as usize;
                            rm &= rm - 1;
                            if pl.hits(p.ink[s], window) {
                                out.push(p.payload[s]);
                            }
                        }
                    }
                }
            }
            b += 64;
        }
    }

    /// Positions (ascending) of records stamped after `epoch`.
    pub fn changed_since(&self, epoch: u32, out: &mut Vec<u32>) {
        out.clear();
        for d in 0..self.d_ptr.len() {
            if self.d_max_stamp[d] > epoch {
                let mut m = stamp_gt_mask(&self.page(d).stamp, epoch);
                let base = self.start_of(d) as u32;
                while m != 0 {
                    out.push(base + m.trailing_zeros());
                    m &= m - 1;
                }
            }
        }
    }

    // ------------------------------------------------------------------ memory & validation

    /// Heap bytes owned by the structure (pages, directory, prefix, handle table, planes).
    pub fn heap_bytes(&self) -> usize {
        let n = self.d_ptr.capacity();
        self.pool.heap_bytes()
            + n * (8 + 4 + 8 + 4 + 2 + 4)
            + self.prefix.heap_bytes()
            + self.page_dir.capacity() * 4
            + self.htab.capacity() * core::mem::size_of::<HEntry>()
            + core::mem::size_of::<Plane>() * 8
    }

    /// Exhaustive structural validation (tests / debug).
    pub fn check_invariants(&self) {
        let n = self.d_ptr.len();
        assert_eq!(self.d_id.len(), n);
        assert_eq!(self.d_ink.len(), n);
        assert_eq!(self.d_max_stamp.len(), n);
        assert_eq!(self.d_min_depth.len(), n);
        assert_eq!(self.d_len.len(), n);
        let mut sum = 0usize;
        let mut live_handles = 0usize;
        let mut prev_depth: Option<u16> = None;
        let starts = self.prefix.snapshot(&self.d_len);
        assert_eq!(starts.len(), n + 1);
        for d in 0..n {
            let p = self.page(d);
            assert_eq!(p.id, self.d_id[d]);
            assert_eq!(self.page_dir[p.id as usize], d as u32);
            let l = self.d_len[d] as usize;
            assert!((1..=CAP).contains(&l), "page {d} has len {l}");
            assert_eq!(starts[d] as usize, sum);
            for s in l..CAP {
                assert_eq!(p.depth[s], PAD_DEPTH, "pad depth p{d} s{s}");
                assert_eq!(p.ink[s], NRect::EMPTY);
                assert_eq!(p.stamp[s], 0);
                assert_eq!(p.kind[s], 0);
                assert_eq!(p.payload[s], 0);
                assert_eq!(p.handle[s], 0);
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
                    assert!(e.live && e.page == p.id && e.slot == s as u32, "handle table mismatch");
                }
            }
            assert_eq!(self.d_ink[d], union_all(&p.ink[..l]), "page {d} ink summary");
            assert_eq!(self.d_min_depth[d], p.depth[..l].iter().copied().min().unwrap());
            assert_eq!(self.d_max_stamp[d], p.stamp[..l].iter().copied().max().unwrap());
            sum += l;
        }
        assert_eq!(starts[n] as usize, sum);
        assert_eq!(sum, self.total);
        let bound = self.htab.iter().filter(|e| e.live && e.page != NO_PAGE).count();
        assert_eq!(bound, live_handles, "bound handle count");
    }
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
fn copy_virtual(dst: &mut Page, lo: usize, hi: usize, old: &Page, s: usize, k: usize, recs: &[RecordInit], epoch: u32) {
    let b = hi.min(s);
    if lo < b {
        dst.copy_from(0, old, lo, b - lo);
    }
    let (a, b) = (lo.max(s), hi.min(s + k));
    for v in a..b {
        dst.write(v - lo, &recs[v - s], epoch);
    }
    let a = lo.max(s + k);
    if a < hi {
        dst.copy_from(a - lo, old, a - k, hi - a);
    }
}
