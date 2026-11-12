//! Damage planes, adopted from `crates/bough-damage-lab/RESULTS.md` (`hw32`): one bit per 32x32
//! device-pixel tile -- the Apple GPU's own load/store unit, so tracking anything finer buys nothing
//! once geometry is clipped -- one `u128` per tile row, a ring of 4 planes by epoch,
//! `repair(age) = OR of the last age planes`. No bounding box, no dirty-row mask, no per-record stamps.
//!
//! Domain bent deliberately: windows up to 4096 x 4096 px (128 tile columns, 128 tile rows), so a tile
//! row is one `u128` and every row mask is two shifts. Damage outside the window is dropped.
//!
//! One addition to the lab's plane: `[y0, y1)`, a row range outside which every row is zero,
//! maintained by `mark` with one `min` and one `max`. Clear, OR and repair touch only those rows,
//! so a frame with one small write costs what its 1-2 damaged rows cost instead of a pass over every
//! row of every plane in the age window (RESULTS.md: the lab's whole-plane passes cost 731
//! instructions per repair at 25 rows; an exact row-occupancy word cost 10 instructions per mark).

use crate::nrect::{NRect, Thresh};

pub const TILE_SHIFT: u32 = 5;
pub const TILE: i32 = 1 << TILE_SHIFT;
pub const MAX_TILES_X: usize = 128;
pub const MAX_TILES_Y: usize = 128;
/// Ring of planes: buffer age 1..=RING can be repaired; older (or unknown) surfaces repaint fully.
pub const RING: usize = 4;

/// A tile bitmap. Only rows `[0, n)` (the window's tile rows) are ever non-zero.
#[derive(Clone)]
#[repr(C, align(16))]
pub struct Plane {
    pub rows: [u128; MAX_TILES_Y],
    /// Rows outside `[y0, y1)` are zero (`y0 >= y1` when clear).
    pub y0: u32,
    pub y1: u32,
    /// Certificate set by `repair_into`: the set tiles are exactly their bounding rectangle, so a
    /// bbox test is already the exact tile test. Cleared by `mark`/`or_assign`; false is always safe.
    pub solid: bool,
    /// Tile rows of the window this plane belongs to.
    pub n: u32,
}

impl Default for Plane {
    fn default() -> Self {
        Plane::EMPTY
    }
}

/// Tile span `[tx0, tx1) x [ty0, ty1)` of a non-empty rect that lies inside the window.
#[inline(always)]
pub fn tile_span(r: NRect) -> (u32, u32, u32, u32) {
    let [x0, y0, nx1, ny1] = r.0;
    // !(-x1) = x1 - 1 (two's complement), so the exclusive end tile is ((x1 - 1) >> 5) + 1.
    let tx0 = (x0 as i32 >> TILE_SHIFT) as u32;
    let ty0 = (y0 as i32 >> TILE_SHIFT) as u32;
    let tx1 = ((!(nx1 as i32) >> TILE_SHIFT) + 1) as u32;
    let ty1 = ((!(ny1 as i32) >> TILE_SHIFT) + 1) as u32;
    (tx0, ty0, tx1, ty1)
}

#[inline(always)]
fn colmask(tx0: u32, tx1: u32) -> u128 {
    // 0 <= tx0 < tx1 <= 128
    (u128::MAX >> (128 - (tx1 - tx0))) << tx0
}

impl Plane {
    pub const EMPTY: Plane = Plane { rows: [0; MAX_TILES_Y], y0: MAX_TILES_Y as u32, y1: 0, solid: false, n: 0 };

    pub fn new(n_rows: u32) -> Plane {
        assert!(n_rows as usize <= MAX_TILES_Y);
        Plane { rows: [0; MAX_TILES_Y], y0: MAX_TILES_Y as u32, y1: 0, solid: false, n: n_rows }
    }

    /// Row range `[lo, hi)` outside which every row is zero (empty range when clear).
    #[inline(always)]
    pub fn occ_range(&self) -> (usize, usize) {
        (self.y0 as usize, (self.y1 as usize).max(self.y0 as usize))
    }

    /// Set the tiles of span `[tx0, tx1) x [ty0, ty1)` (non-empty, inside the window).
    /// The common case (span inside one 64-bit word) is one load-OR-store per tile row.
    #[inline(always)]
    pub fn mark_span(&mut self, tx0: u32, ty0: u32, tx1: u32, ty1: u32) {
        debug_assert!(tx0 < tx1 && ty0 < ty1 && tx1 as usize <= MAX_TILES_X && ty1 as usize <= MAX_TILES_Y);
        self.solid = false;
        self.y0 = self.y0.min(ty0);
        self.y1 = self.y1.max(ty1);
        let w = tx0 >> 6;
        if w == (tx1 - 1) >> 6 {
            let m = (u64::MAX >> (64 - (tx1 - tx0))) << (tx0 & 63);
            // SAFETY: ty1 <= 128 rows; a u128 row is two little-endian u64 words.
            unsafe {
                let base = self.rows.as_mut_ptr() as *mut u64;
                for ty in ty0..ty1 {
                    *base.add(ty as usize * 2 + w as usize) |= m;
                }
            }
            return;
        }
        let m = colmask(tx0, tx1);
        for row in &mut self.rows[ty0 as usize..ty1 as usize] {
            *row |= m;
        }
    }

    /// `rows[r] |= m` for `r` in `r0..=r1` (tile rows, clamped to the window's rows; `r0 > r1` or
    /// `m == 0` marks nothing). The tile-mark sink of bough-text: its labels compute their damage as
    /// row masks at update time.
    #[inline(always)]
    pub fn or_rows(&mut self, r0: i32, r1: i32, m: u128) {
        let r0 = r0.max(0);
        let r1 = r1.min(self.n as i32 - 1);
        if r1 < r0 || m == 0 {
            return;
        }
        self.solid = false;
        self.y0 = self.y0.min(r0 as u32);
        self.y1 = self.y1.max(r1 as u32 + 1);
        for r in r0..=r1 {
            self.rows[r as usize] |= m;
        }
    }

    /// Set every tile a (canonical, window-clipped, non-empty) rect touches.
    #[inline(always)]
    pub fn mark(&mut self, r: NRect) {
        let (tx0, ty0, tx1, ty1) = tile_span(r);
        self.mark_span(tx0, ty0, tx1, ty1);
    }

    /// Add a rect clipped to `window` (no-op if the clipped rect is empty).
    #[inline(always)]
    pub fn add(&mut self, r: NRect, window: NRect) {
        let c = r.intersect(window);
        if !c.is_empty() {
            self.mark(c);
        }
    }

    /// Does any set tile lie in span `[tx0, tx1) x [ty0, ty1)` (non-empty)?
    #[inline(always)]
    pub fn test_span(&self, tx0: u32, ty0: u32, tx1: u32, ty1: u32) -> bool {
        let w = tx0 >> 6;
        if w == (tx1 - 1) >> 6 {
            let m = (u64::MAX >> (64 - (tx1 - tx0))) << (tx0 & 63);
            let mut hit = 0u64;
            // SAFETY: as in mark_span.
            unsafe {
                let base = self.rows.as_ptr() as *const u64;
                for ty in ty0..ty1 {
                    hit |= *base.add(ty as usize * 2 + w as usize) & m;
                }
            }
            return hit != 0;
        }
        let m = colmask(tx0, tx1);
        let mut hit = 0u128;
        for row in &self.rows[ty0 as usize..ty1 as usize] {
            hit |= *row & m;
        }
        hit != 0
    }

    /// (any tile set, every tile set) over span `[tx0, tx1) x [ty0, ty1)` (non-empty), one pass.
    #[inline(always)]
    pub fn classify_span(&self, tx0: u32, ty0: u32, tx1: u32, ty1: u32) -> (bool, bool) {
        let w = tx0 >> 6;
        if w == (tx1 - 1) >> 6 {
            let m = (u64::MAX >> (64 - (tx1 - tx0))) << (tx0 & 63);
            let (mut any, mut miss) = (0u64, 0u64);
            // SAFETY: as in mark_span.
            unsafe {
                let base = self.rows.as_ptr() as *const u64;
                for ty in ty0..ty1 {
                    let v = *base.add(ty as usize * 2 + w as usize);
                    any |= v & m;
                    miss |= !v & m;
                }
            }
            return (any != 0, miss == 0);
        }
        let m = colmask(tx0, tx1);
        let (mut any, mut miss) = (0u128, 0u128);
        for row in &self.rows[ty0 as usize..ty1 as usize] {
            any |= *row & m;
            miss |= !*row & m;
        }
        (any != 0, miss == 0)
    }

    /// Does any set tile overlap the (canonical) rect clipped to `window`?
    #[inline]
    pub fn hits(&self, r: NRect, window: NRect) -> bool {
        let c = r.intersect(window);
        if c.is_empty() {
            return false;
        }
        let (tx0, ty0, tx1, ty1) = tile_span(c);
        self.test_span(tx0, ty0, tx1, ty1)
    }

    /// Clear the occupied rows only.
    #[inline]
    pub fn clear(&mut self) {
        let (lo, hi) = self.occ_range();
        self.rows[lo..hi].fill(0);
        self.y0 = MAX_TILES_Y as u32;
        self.y1 = 0;
        self.solid = false;
    }

    /// `self |= o`.
    #[inline]
    pub fn or_assign(&mut self, o: &Plane) {
        let (lo, hi) = o.occ_range();
        for y in lo..hi {
            self.rows[y] |= o.rows[y];
        }
        self.y0 = self.y0.min(o.y0);
        self.y1 = self.y1.max(o.y1);
        self.solid = false;
        self.n = self.n.max(o.n);
    }

    #[inline]
    pub fn tile(&self, tx: usize, ty: usize) -> bool {
        (self.rows[ty] >> tx) & 1 != 0
    }

    pub fn count(&self) -> u32 {
        self.rows.iter().map(|r| r.count_ones()).sum()
    }

    pub fn is_clear(&self) -> bool {
        let (lo, hi) = self.occ_range();
        self.rows[lo..hi].iter().all(|&r| r == 0)
    }

    /// Scroll by copy (bough-metal RESULTS §15): the marks already made this epoch inside clip `c` were made
    /// where the content was before a scroll by `(dx, dy)` px; after the scroll that content (and its change)
    /// is `(dx, dy)` further. Each set tile meeting `c`'s tile span is ALSO set at its translated place (the
    /// 1-2 tiles a 32-px tile covers after a non-multiple-of-32 move), clipped to the span; the original marks
    /// stay (a mark from a record that did not scroll is still where it was). O(rows of the span).
    pub fn shift_within(&mut self, c: NRect, dx: i32, dy: i32) {
        let c = c.canon();
        if c.is_empty() || (dx == 0 && dy == 0) {
            return;
        }
        let (tx0, ty0, tx1, ty1) = tile_span(c);
        let (lo, hi) = self.occ_range();
        let (lo, hi) = (lo.max(ty0 as usize), hi.min(ty1 as usize));
        if lo >= hi {
            return;
        }
        let span = colmask(tx0, tx1);
        let (qx, rx) = (dx.div_euclid(TILE), dx.rem_euclid(TILE));
        let (qy, ry) = (dy.div_euclid(TILE), dy.rem_euclid(TILE));
        let shl = |m: u128, q: i32| if q >= 128 || q <= -128 { 0 } else if q >= 0 { m << q } else { m >> -q };
        // rows move by qy (and qy + 1 when ry != 0): visit them so that every source row is read before
        // anything is written into it (bottom-up when moving down)
        let mut y = if qy >= 0 { hi as i32 - 1 } else { lo as i32 };
        let step = if qy >= 0 { -1 } else { 1 };
        let (mut n0, mut n1) = (self.y0, self.y1);
        while y >= lo as i32 && y < hi as i32 {
            let m = self.rows[y as usize] & span;
            if m != 0 {
                let mh = (shl(m, qx) | if rx != 0 { shl(m, qx + 1) } else { 0 }) & span;
                if mh != 0 {
                    for k in 0..=(ry != 0) as i32 {
                        let t = y + qy + k;
                        if t >= ty0 as i32 && t < ty1 as i32 {
                            self.rows[t as usize] |= mh;
                            n0 = n0.min(t as u32);
                            n1 = n1.max(t as u32 + 1);
                        }
                    }
                }
            }
            y += step;
        }
        (self.y0, self.y1) = (n0, n1);
        self.solid = false;
    }

    /// Pixel bounding box of the set tiles clipped to `window` (EMPTY when clear).
    pub fn px_bbox(&self, window: NRect) -> NRect {
        let (lo, hi) = self.occ_range();
        let (mut cols, mut y0, mut y1) = (0u128, usize::MAX, 0usize);
        for y in lo..hi {
            let r = self.rows[y];
            cols |= r;
            if r != 0 {
                y0 = y0.min(y);
                y1 = y + 1;
            }
        }
        bbox_of(cols, y0, y1, window)
    }
}

#[inline(always)]
fn bbox_of(cols: u128, y0: usize, y1: usize, window: NRect) -> NRect {
    if cols == 0 {
        return NRect::EMPTY;
    }
    let x0 = cols.trailing_zeros() as i32;
    let x1 = 128 - cols.leading_zeros() as i32;
    NRect::new(x0 << TILE_SHIFT, (y0 as i32) << TILE_SHIFT, x1 << TILE_SHIFT, (y1 as i32) << TILE_SHIFT).intersect(window).canon()
}

/// Scroll by copy (bough-metal RESULTS §15): an epoch's scroll of clip rect `clip` by `(dx, dy)` device px whose
/// damage was only the exposed strip(s) and the static content's bands. A presenter repairs a surface of that
/// epoch by copying `clip ∩ (clip + d)` from the surface showing the previous epoch.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ScrollRec {
    pub clip: NRect,
    pub dx: i32,
    pub dy: i32,
}

pub struct DamageRing {
    planes: Box<[Plane; RING]>,
    /// One scroll record per epoch (`epoch % RING`), next to its plane; cleared with it.
    scrolls: [Option<ScrollRec>; RING],
    /// Bit `e % RING`: that epoch has a scroll record (so "no scroll in the ring" is one test).
    scroll_bits: u8,
    /// `&mut planes[epoch % RING]`, kept so a mark does no index arithmetic.
    cur: *mut Plane,
    pub epoch: u32,
    pub window: NRect,
    /// `Thresh::of_rect(window)` = (W, H, 0, 0).
    wthresh: Thresh,
    /// Tile columns / rows of the window.
    pub tw: u32,
    pub rows: u32,
}

impl DamageRing {
    /// Heap bytes of the ring's planes (memory ledger).
    pub fn heap_bytes(&self) -> usize {
        size_of::<[Plane; RING]>()
    }
    pub fn new(w: i32, h: i32) -> Self {
        assert!(w > 0 && h > 0 && w <= (MAX_TILES_X as i32) * TILE && h <= (MAX_TILES_Y as i32) * TILE, "window up to 4096x4096");
        let rows = ((h + TILE - 1) >> TILE_SHIFT) as u32;
        let mut planes = Box::new([const { Plane::EMPTY }; RING]);
        for p in planes.iter_mut() {
            p.n = rows;
        }
        let window = NRect::new(0, 0, w, h);
        let cur = &mut planes[1 % RING] as *mut Plane;
        DamageRing { planes, scrolls: [None; RING], scroll_bits: 0, cur, epoch: 1, window, wthresh: Thresh::of_rect(window), tw: ((w + TILE - 1) >> TILE_SHIFT) as u32, rows }
    }
    /// Mark a rect (any form; empty and off-window rects mark nothing), clipped to the window, into
    /// the current epoch. Clipping is done in tile units: the window starts at 0 and its last tile
    /// row/column holds its edge, so clamping the span after the shift equals clipping the rect
    /// before it; the pixel tests reject empty rects and rects beyond the window's edges.
    #[inline(always)]
    pub fn add(&mut self, r: NRect) {
        let [x0, y0, nx1, ny1] = r.0;
        let (x0, y0, nx1, ny1) = (x0 as i32, y0 as i32, nx1 as i32, ny1 as i32);
        let wt = self.wthresh.0;
        // non-empty (x0 < x1, y0 < y1) and meets the window (x0 < W, y0 < H, x1 > 0, y1 > 0)
        let ok = (x0 + nx1 < 0) & (y0 + ny1 < 0) & (x0 < wt[0] as i32) & (y0 < wt[1] as i32) & (nx1 < 0) & (ny1 < 0);
        if !ok {
            return;
        }
        let tx0 = (x0.max(0) >> TILE_SHIFT) as u32;
        let ty0 = (y0.max(0) >> TILE_SHIFT) as u32;
        let tx1 = (((!nx1) >> TILE_SHIFT) + 1).min(self.tw as i32) as u32;
        let ty1 = (((!ny1) >> TILE_SHIFT) + 1).min(self.rows as i32) as u32;
        // SAFETY: `cur` points into `planes` (boxed, never moved) at index epoch % RING.
        unsafe { (*self.cur).mark_span(tx0, ty0, tx1, ty1) };
    }
    /// Mark a canonical rect that lies inside the window (coverage already clipped by the caller,
    /// as every binding computes it): no clip, no clamp. Empty rects mark nothing.
    #[inline(always)]
    pub fn add_inside(&mut self, r: NRect) {
        debug_assert!(r.is_empty() || r.intersect(self.window) == r, "add_inside: {r:?} leaves the window");
        let [x0, y0, nx1, ny1] = r.0;
        if (x0 as i32 + nx1 as i32 >= 0) | (y0 as i32 + ny1 as i32 >= 0) {
            return;
        }
        let (tx0, ty0, tx1, ty1) = tile_span(r);
        // SAFETY: as in `add`; an inside rect's span is within the window's tiles.
        unsafe { (*self.cur).mark_span(tx0, ty0, tx1, ty1) };
    }
    /// A new window size (up to 4096 x 4096): window, tile columns and rows. The planes keep their marks
    /// (rows past the new height are never read); the caller damages what the resize repaints.
    pub fn set_window(&mut self, w: i32, h: i32) {
        assert!(w > 0 && h > 0 && w <= (MAX_TILES_X as i32) * TILE && h <= (MAX_TILES_Y as i32) * TILE, "window up to 4096x4096");
        let rows = ((h + TILE - 1) >> TILE_SHIFT) as u32;
        for p in self.planes.iter_mut() {
            p.n = rows;
        }
        self.window = NRect::new(0, 0, w, h);
        self.wthresh = Thresh::of_rect(self.window);
        self.tw = ((w + TILE - 1) >> TILE_SHIFT) as u32;
        self.rows = rows;
    }
    /// Start a new epoch: the slot it reuses (epoch - RING) is cleared.
    pub fn advance(&mut self) {
        self.epoch += 1;
        let e = self.epoch as usize % RING;
        self.planes[e].clear();
        self.scrolls[e] = None;
        self.scroll_bits &= !(1 << e);
        self.cur = &mut self.planes[e] as *mut Plane;
    }
    /// Does any epoch still in the ring have a scroll record?
    #[inline(always)]
    pub fn any_scroll(&self) -> bool {
        self.scroll_bits != 0
    }
    /// OR the planes of the last `age` epochs (1..=RING) into `rows` (tile rows, at least the window's):
    /// only each plane's occupied rows are read. Returns the row range that was written (empty when nothing
    /// is damaged) and whether any tile is set. The work is the damaged rows of those planes.
    #[inline]
    pub fn or_last_into(&self, age: u32, rows: &mut [u128]) -> (usize, usize, bool) {
        assert!((1..=RING as u32).contains(&age) && rows.len() >= self.rows as usize);
        let n = self.rows as usize;
        let (mut lo, mut hi, mut any) = (n, 0usize, 0u128);
        for k in 0..age as usize {
            let p = &self.planes[(self.epoch as usize + RING - k) % RING];
            let (a, b) = (p.y0 as usize, (p.y1 as usize).min(n));
            let mut y = a;
            while y < b {
                // SAFETY: y < b <= n <= rows.len() (asserted) and n <= MAX_TILES_Y = p.rows.len().
                unsafe {
                    let m = *p.rows.get_unchecked(y);
                    *rows.get_unchecked_mut(y) |= m;
                    any |= m;
                }
                y += 1;
            }
            if a < b {
                lo = lo.min(a);
                hi = hi.max(b);
            }
        }
        if lo < hi { (lo, hi, any != 0) } else { (0, 0, false) }
    }
    /// The scroll record of the epoch `back` epochs ago (0 = current).
    #[inline]
    pub fn scroll(&self, back: u32) -> Option<ScrollRec> {
        self.scrolls[self.epoch.wrapping_sub(back) as usize % RING]
    }
    /// Set (or clear) the current epoch's scroll record.
    #[inline]
    pub fn set_scroll(&mut self, r: Option<ScrollRec>) {
        let e = self.epoch as usize % RING;
        self.scroll_bits = (self.scroll_bits & !(1 << e)) | (r.is_some() as u8) << e;
        self.scrolls[e] = r;
    }
    /// [`Plane::shift_within`] on the current epoch's plane.
    #[inline]
    pub fn shift_within(&mut self, c: NRect, dx: i32, dy: i32) {
        self.cur_mut().shift_within(c, dx, dy);
    }
    /// Become a copy of `o` (same window), touching only occupied rows.
    pub fn copy_from(&mut self, o: &DamageRing) {
        for (d, s) in self.planes.iter_mut().zip(o.planes.iter()) {
            d.clear();
            let (lo, hi) = s.occ_range();
            d.rows[lo..hi].copy_from_slice(&s.rows[lo..hi]);
            (d.y0, d.y1, d.solid, d.n) = (s.y0, s.y1, s.solid, s.n);
        }
        self.scrolls = o.scrolls;
        self.scroll_bits = o.scroll_bits;
        self.epoch = o.epoch;
        self.cur = &mut self.planes[self.epoch as usize % RING] as *mut Plane;
    }
    #[inline(always)]
    pub fn cur_mut(&mut self) -> &mut Plane {
        // SAFETY: `cur` points into `planes` (boxed, never moved); &mut self is exclusive.
        unsafe { &mut *self.cur }
    }
    /// The plane of the epoch `back` epochs ago (0 = current).
    pub fn plane(&self, back: u32) -> &Plane {
        &self.planes[self.epoch.wrapping_sub(back) as usize % RING]
    }
    /// OR of the last `age` planes (1..=RING) into `out`; returns the pixel bbox of the result
    /// (EMPTY when nothing is damaged). Touches only the damaged row band: one fused pass of OR,
    /// column union and the solid-rectangle certificate.
    #[inline]
    pub fn repair_into(&self, age: u32, out: &mut Plane) -> NRect {
        assert!((1..=RING as u32).contains(&age));
        out.clear();
        out.n = self.rows;
        let e = self.epoch as usize;
        let (mut lo, mut hi) = (MAX_TILES_Y, 0usize);
        let mut ps: [&Plane; RING] = [&self.planes[0]; RING];
        for k in 0..age as usize {
            ps[k] = &self.planes[(e + RING - k) % RING];
            let (a, b) = ps[k].occ_range();
            if a < b {
                lo = lo.min(a);
                hi = hi.max(b);
            }
        }
        // Planes beyond `age` contribute nothing: point them at the first one (OR is idempotent).
        for k in age as usize..RING {
            ps[k] = ps[0];
        }
        if lo >= hi {
            return NRect::EMPTY;
        }
        out.y0 = lo as u32;
        out.y1 = hi as u32;
        let (mut cols, mut both) = (0u128, u128::MAX);
        let (mut y0, mut y1, mut nz) = (usize::MAX, 0usize, 0usize);
        for y in lo..hi {
            let r = ps[0].rows[y] | ps[1].rows[y] | ps[2].rows[y] | ps[3].rows[y];
            out.rows[y] = r;
            cols |= r;
            if r != 0 {
                y0 = y0.min(y);
                y1 = y + 1;
                both &= r;
                nz += 1;
            }
        }
        // Solid iff the non-empty rows are contiguous, each equals the column union, and the union
        // is one run of tiles.
        out.solid = nz == y1.wrapping_sub(y0) && both == cols && (cols >> cols.trailing_zeros()).wrapping_add(1).is_power_of_two();
        bbox_of(cols, y0, y1, self.window)
    }
    pub fn repair(&self, age: u32) -> Plane {
        let mut p = Plane::new(self.rows);
        self.repair_into(age, &mut p);
        p
    }
}

