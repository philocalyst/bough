//! The measured variable-height virtual list.
//!
//! * rows are laid out by cephal as independent groups (`layout.rs`); a row's height is *measured as
//!   it is laid out* and recorded in a **prefix index** (a Fenwick tree over device-pixel heights:
//!   `prefix(k)`, `row_at(y)` and a point update are O(log n)); unseen rows count as an estimate
//!   (zgx's measured_virtual_list: a three-line row);
//! * only rows entering / leaving the range are mounted / unmounted; a scroll writes one space entry per
//!   mounted row and translates their ink as one contiguous run (instances are never rewritten);
//! * **anchoring**: the first visible row and the offset inside it are fixed while measurement runs, so
//!   a row measured above the viewport (taller or shorter than its estimate) moves the scroll offset by
//!   the difference instead of moving the content; the same loop is what makes a **jump** converge: the
//!   target row is placed from estimates, the rows around it are mounted and measured *inside the same
//!   frame*, the offset is re-derived from the anchor, and rows that now fall in range are mounted until
//!   the range is stable (it takes 1-3 passes).
use crate::elem::{Element, Mounter};
use crate::rt::*;
use crate::signal::Signal;
use bough_core::RecordInit;
use std::collections::VecDeque;
use std::marker::PhantomData;
use std::ops::Range;

// =============================================================================== prefix index

/// Row heights (device px) with prefix sums: a Fenwick tree (`fen`) beside the plain heights (`h`).
pub struct HeightIndex {
    n: usize,
    est: i32,
    h: Vec<i32>,
    fen: Vec<i32>,
    total: i64,
    top: usize,
}

impl HeightIndex {
    pub fn new(n: usize, est: i32) -> HeightIndex {
        let mut fen = vec![0i32; n + 1];
        for (i, f) in fen.iter_mut().enumerate().skip(1) {
            *f = est * (i & i.wrapping_neg()) as i32;
        }
        let top = if n == 0 { 0 } else { 1usize << (usize::BITS - 1 - n.leading_zeros()) };
        HeightIndex { n, est, h: vec![est; n], fen, total: est as i64 * n as i64, top }
    }
    pub fn len(&self) -> usize {
        self.n
    }
    pub fn estimate(&self) -> i32 {
        self.est
    }
    pub fn height(&self, k: usize) -> i32 {
        self.h[k]
    }
    pub fn total(&self) -> i64 {
        self.total
    }
    /// Sum of the heights of rows `0..k`.
    pub fn prefix(&self, k: usize) -> i64 {
        let mut i = k.min(self.n);
        let mut s = 0i64;
        while i > 0 {
            s += self.fen[i] as i64;
            i &= i - 1;
        }
        s
    }
    /// Set row `k`'s height; returns the change.
    pub fn set(&mut self, k: usize, v: i32) -> i32 {
        let d = v - self.h[k];
        if d != 0 {
            self.h[k] = v;
            self.total += d as i64;
            let mut i = k + 1;
            while i <= self.n {
                self.fen[i] += d;
                i += i & i.wrapping_neg();
            }
        }
        d
    }
    /// The row containing offset `y` (the last row for `y` at or past the end).
    pub fn row_at(&self, y: i64) -> usize {
        if self.n == 0 {
            return 0;
        }
        let (mut pos, mut rem) = (0usize, y.max(0));
        let mut pw = self.top;
        while pw > 0 {
            let next = pos + pw;
            if next <= self.n && (self.fen[next] as i64) <= rem {
                pos = next;
                rem -= self.fen[next] as i64;
            }
            pw >>= 1;
        }
        pos.min(self.n - 1)
    }
}

// =============================================================================== region state

pub struct MRow {
    pub nrec: u32,
    pub space: u16,
    pub scope: Scope,
    pub root: u32,
    /// Mounted this pass, not yet placed / written back.
    pub fresh: bool,
    /// Its height has been taken from the layout.
    pub measured: bool,
}

pub type MRowMountFn = unsafe fn(&mut Rt, *const u8, usize, usize, &mut Vec<RecordInit>) -> MRow;

#[derive(Default, Clone, Copy, Debug)]
pub struct MStats {
    pub passes: u64,
    pub rows_mounted: u64,
    pub rows_unmounted: u64,
    pub height_changes: u64,
}

pub struct MRegion {
    pub idx: HeightIndex,
    /// Viewport, device px: x, y, w, h.
    pub vp: [f32; 4],
    pub clip: u16,
    pub depth: u16,
    pub base_pos: usize,
    pub overscan: usize,
    pub range: Range<usize>,
    pub rows: VecDeque<MRow>,
    pub scroll_px: i32,
    pub init_scroll_pt: f32,
    pub mount_row: MRowMountFn,
    pub row_fn: usize,
    pub stats: MStats,
}

impl Rt {
    fn m_visible(&self, reg: usize, s: i32) -> Range<usize> {
        let r = &self.mregions[reg];
        if r.idx.len() == 0 {
            return 0..0;
        }
        let first = r.idx.row_at(s as i64);
        let last = r.idx.row_at(s as i64 + r.vp[3] as i64 - 1);
        first.saturating_sub(r.overscan)..(last + 1 + r.overscan).min(r.idx.len())
    }

    fn m_remove(&mut self, reg: usize, front: bool, count: usize) {
        self.lay.prof.reset();
        let mut nrec = 0usize;
        for _ in 0..count {
            let row = if front { self.mregions[reg].rows.pop_front() } else { self.mregions[reg].rows.pop_back() }.unwrap();
            nrec += row.nrec as usize;
            self.free_space(row.space);
            self.release_scope(row.scope);
            self.mregions[reg].stats.rows_unmounted += 1;
        }
        let r = &self.mregions[reg];
        let start = if front { r.base_pos } else { r.base_pos + r.rows.iter().map(|x| x.nrec as usize).sum::<usize>() };
        self.bough.remove_run(start, nrec);
        self.lay.prof.mark(crate::layout::LP_UNMOUNT);
    }

    unsafe fn m_mount(&mut self, reg: usize, owner: *const u8, range: Range<usize>, front: bool) {
        if range.is_empty() {
            return;
        }
        let mut recs: Vec<RecordInit> = Vec::with_capacity(range.len() * 64);
        let f = self.mregions[reg].mount_row;
        let mut built: Vec<MRow> = Vec::with_capacity(range.len());
        let vp_w = self.mregions[reg].vp[2];
        self.lay.mount_avail = (vp_w, f32::NAN);
        self.lay.prof.reset();
        for row in range {
            let mut r = unsafe { f(self, owner, reg, row, &mut recs) };
            r.fresh = true;
            built.push(r);
            self.mregions[reg].stats.rows_mounted += 1;
        }
        self.lay.prof.mark(crate::layout::LP_MOUNT);
        let r = &mut self.mregions[reg];
        let pos = if front { r.base_pos } else { r.base_pos + r.rows.iter().map(|x| x.nrec as usize).sum::<usize>() };
        if front {
            for b in built.into_iter().rev() {
                r.rows.push_front(b);
            }
        } else {
            r.rows.extend(built);
        }
        self.bough.insert_run(pos, &recs);
        self.lay.prof.mark(crate::layout::LP_INSERT);
    }

    unsafe fn m_set_range(&mut self, reg: usize, owner: *const u8, new_range: Range<usize>) {
        let old = self.mregions[reg].range.clone();
        if new_range == old {
            return;
        }
        let disjoint = self.mregions[reg].rows.is_empty() || new_range.start >= old.end || new_range.end <= old.start;
        if disjoint {
            let n = self.mregions[reg].rows.len();
            self.m_remove(reg, true, n);
            self.mregions[reg].range = new_range.clone();
            unsafe { self.m_mount(reg, owner, new_range, false) };
            return;
        }
        let drop_front = new_range.start.saturating_sub(old.start);
        let drop_back = old.end.saturating_sub(new_range.end);
        if drop_front > 0 {
            self.m_remove(reg, true, drop_front);
        }
        if drop_back > 0 {
            self.m_remove(reg, false, drop_back);
        }
        let surv_start = old.start.max(new_range.start);
        let surv_end = old.end.min(new_range.end);
        self.mregions[reg].range = new_range.clone();
        unsafe {
            self.m_mount(reg, owner, new_range.start..surv_start, true);
            self.m_mount(reg, owner, surv_end..new_range.end, false);
        }
    }

    /// Bring the list to scroll offset `req_px` (device px): mount / unmount / measure / anchor until the
    /// range is stable, then place the rows (spaces + ink) and write back the fresh ones.
    pub unsafe fn mregion_update(&mut self, reg: usize, owner: *const u8, req_px: i32) {
        let mut s = req_px;
        let mut anchor: Option<(usize, i32)> = None;
        for _pass in 0..32 {
            self.mregions[reg].stats.passes += 1;
            let (total, vh) = {
                let r = &self.mregions[reg];
                (r.idx.total(), r.vp[3] as i64)
            };
            s = (s as i64).clamp(0, (total - vh).max(0)) as i32;
            let (f, within) = *anchor.get_or_insert_with(|| {
                let r = &self.mregions[reg];
                let f = r.idx.row_at(s as i64);
                (f, s - r.idx.prefix(f) as i32)
            });
            let range = self.m_visible(reg, s);
            unsafe { self.m_set_range(reg, owner, range) };
            // measure what was just mounted
            self.lay_compute_pending();
            let mut changed = false;
            {
                let start = self.mregions[reg].range.start;
                let n = self.mregions[reg].rows.len();
                for i in 0..n {
                    if self.mregions[reg].rows[i].measured {
                        continue;
                    }
                    let root = self.mregions[reg].rows[i].root;
                    let h = self.lay.root_height(root).round() as i32;
                    let r = &mut self.mregions[reg];
                    r.rows[i].measured = true;
                    if r.idx.set(start + i, h) != 0 {
                        r.stats.height_changes += 1;
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
            let r = &self.mregions[reg];
            s = (r.idx.prefix(f) + within as i64) as i32;
        }
        self.m_place(reg, s);
    }

    /// Spaces, ink and write-back for the final offset `s`.
    fn m_place(&mut self, reg: usize, s: i32) {
        let (start, vp_x, vp_y, base_pos) = {
            let r = &self.mregions[reg];
            (r.range.start, r.vp[0], r.vp[1], r.base_pos)
        };
        let n = self.mregions[reg].rows.len();
        let mut p = self.mregions[reg].idx.prefix(start);
        let mut pos = base_pos;
        // pending contiguous survivors with one shift: (first record, count, dy)
        let mut run: Option<(usize, usize, i32)> = None;
        for i in 0..n {
            let (space, nrec, fresh, h) = {
                let r = &self.mregions[reg];
                (r.rows[i].space, r.rows[i].nrec as usize, r.rows[i].fresh, r.idx.height(start + i))
            };
            let y = vp_y + (p - s as i64) as f32;
            if fresh {
                self.set_space(space, [vp_x, y]);
                self.mregions[reg].rows[i].fresh = false;
            } else {
                let old = self.space(space);
                let dy = (y - old[1]) as i32;
                if dy != 0 {
                    self.set_space(space, [old[0], y]);
                    match &mut run {
                        Some((_, cnt, d)) if *d == dy => *cnt += nrec,
                        _ => {
                            if let Some((a, c, d)) = run.take() {
                                self.bough.translate_run(a, c, 0, d);
                            }
                            run = Some((pos, nrec, dy));
                        }
                    }
                } else if let Some((a, c, d)) = run.take() {
                    self.bough.translate_run(a, c, 0, d);
                }
            }
            p += h as i64;
            pos += nrec;
        }
        if let Some((a, c, d)) = run.take() {
            self.bough.translate_run(a, c, 0, d);
        }
        self.mregions[reg].scroll_px = s;
        self.lay_writeback_ready();
    }

    /// Initial rows of every measured list (call once the first records are in the Bough).
    pub unsafe fn mregions_init(&mut self, owner: *const u8) {
        for reg in 0..self.mregions.len() {
            let s = (self.mregions[reg].init_scroll_pt * self.scale).round() as i32;
            unsafe { self.mregion_update(reg, owner, s) };
        }
    }

    // ------------------------------------------------------------------ queries (points)
    pub fn mregion_scroll_pt(&self, reg: usize) -> f32 {
        self.mregions[reg].scroll_px as f32 / self.scale
    }
    pub fn mregion_content_height_pt(&self, reg: usize) -> f32 {
        self.mregions[reg].idx.total() as f32 / self.scale
    }
    pub fn mregion_row_offset_pt(&self, reg: usize, row: usize) -> f32 {
        self.mregions[reg].idx.prefix(row) as f32 / self.scale
    }
    pub fn mregion_row_height_pt(&self, reg: usize, row: usize) -> f32 {
        self.mregions[reg].idx.height(row) as f32 / self.scale
    }
}

// =============================================================================== element

pub struct MeasuredList<'a, V> {
    scroll: &'a Signal<f32>,
    rows: usize,
    estimate_pt: f32,
    vp: [f32; 4],
    overscan: usize,
    row: fn(&V, usize, &mut Mounter<'_, V>),
    _v: PhantomData<fn(&V)>,
}

/// A virtual list of `rows` variable-height rows in the viewport `vp` (x, y, w, h points), unmeasured rows
/// counted as `estimate_pt`, `overscan` extra rows mounted either side. `row` mounts one row; its outermost
/// layout box (`column` / `row`) is the row's group, laid out at the viewport width.
pub fn measured_list<'a, V>(
    scroll: &'a Signal<f32>,
    rows: usize,
    estimate_pt: f32,
    vp: [f32; 4],
    overscan: usize,
    row: fn(&V, usize, &mut Mounter<'_, V>),
) -> MeasuredList<'a, V> {
    MeasuredList { scroll, rows, estimate_pt, vp, overscan, row, _v: PhantomData }
}

unsafe fn mount_mrow<V: 'static>(rt: &mut Rt, owner: *const u8, reg: usize, row: usize, recs: &mut Vec<RecordInit>) -> MRow {
    let space = rt.alloc_space();
    let (clip, depth, f, vp) = {
        let r = &rt.mregions[reg];
        (r.clip, r.depth, r.row_fn, r.vp)
    };
    rt.set_space(space, [vp[0], vp[1]]);
    let f: fn(&V, usize, &mut Mounter<'_, V>) = unsafe { std::mem::transmute::<usize, fn(&V, usize, &mut Mounter<'_, V>)>(f) };
    let mut scope = Scope::default();
    let before = recs.len();
    rt.lay.cur = crate::layout::NONE;
    {
        let mut m = Mounter { rt, owner: owner as *const V, recs, scope: &mut scope, depth, ox: 0.0, oy: 0.0, space, clip };
        f(unsafe { &*(owner as *const V) }, row, &mut m);
    }
    assert_eq!(scope.lroots.len(), 1, "a measured-list row must mount exactly one layout group (a column / row)");
    let root = scope.lroots[0];
    MRow { nrec: (recs.len() - before) as u32, space, scope, root, fresh: true, measured: false }
}

unsafe fn patch_mscroll(rt: &mut Rt, w: &Wire, sig: *const u8, owner: *const u8) {
    let s = unsafe { &*(sig as *const Signal<f32>) };
    let px = (**s * rt.scale).round() as i32;
    unsafe { rt.mregion_update(w.node as usize, owner, px) };
}

impl<'a, V: 'static> Element<V> for MeasuredList<'a, V> {
    fn mount(self, m: &mut Mounter<'_, V>) {
        let s = m.rt.scale;
        let o = m.rt.space(m.space);
        let [x, y, w, h] = self.vp;
        let (vx, vy) = (o[0] + (m.ox + x) * s, o[1] + (m.oy + y) * s);
        let clip = m.rt.clip_rect([vx, vy, vx + w * s, vy + h * s]);
        let reg = m.rt.mregions.len();
        let est = (self.estimate_pt * s).round() as i32;
        m.rt.mregions.push(MRegion {
            idx: HeightIndex::new(self.rows, est),
            vp: [vx, vy, w * s, h * s],
            clip,
            depth: m.depth,
            base_pos: m.recs.len(),
            overscan: self.overscan,
            range: 0..0,
            rows: VecDeque::new(),
            scroll_px: 0,
            init_scroll_pt: **self.scroll,
            mount_row: mount_mrow::<V>,
            row_fn: self.row as usize,
            stats: MStats::default(),
        });
        let sc = self.scroll;
        if let Some(off) = {
            let a = sc as *const Signal<f32> as usize;
            let ow = m.owner as usize;
            (a >= ow && a + std::mem::size_of::<Signal<f32>>() <= ow + std::mem::size_of::<V>()).then(|| (a - ow) as u32)
        } {
            let wi = m.rt.add_wire(sc.head(), off, patch_mscroll, reg as u32, [0; 16]);
            m.scope.wires.push(wi);
        }
    }
}
