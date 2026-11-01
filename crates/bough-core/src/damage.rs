//! Damage planes: a tile bitmap (16x16 device-pixel tiles) plus a bounding NRect, and a ring of
//! the last 8 epochs for buffer-age repair.

use crate::nrect::NRect;

pub const TILE_SHIFT: u32 = 4;
pub const TILE: i32 = 1 << TILE_SHIFT;
/// Window up to 2048 x 1536 device px: 128 x 96 tiles; a row of tiles is exactly one u128.
pub const MAX_TILES_X: usize = 128;
pub const MAX_TILES_Y: usize = 96;
pub const RING: usize = 8;

#[derive(Clone)]
#[repr(C, align(16))]
pub struct Plane {
    /// Bounding box of everything added (clamped to the window); `EMPTY` when clean.
    pub bbox: NRect,
    pub rows: [u128; MAX_TILES_Y],
}

impl Default for Plane {
    fn default() -> Self {
        Plane::EMPTY
    }
}

/// Tile row range `[ty0, ty1)` and column range `[tx0, tx1)` of a non-empty, window-clamped rect.
#[inline(always)]
fn tile_span(r: NRect) -> (usize, usize, u32, u32) {
    let [x0, y0, nx1, ny1] = r.0;
    let tx0 = (x0 as i32) >> TILE_SHIFT;
    let ty0 = (y0 as i32) >> TILE_SHIFT;
    let tx1 = (-(nx1 as i32) + TILE - 1) >> TILE_SHIFT;
    let ty1 = (-(ny1 as i32) + TILE - 1) >> TILE_SHIFT;
    (ty0 as usize, ty1 as usize, tx0 as u32, tx1 as u32)
}

#[inline(always)]
fn row_mask(tx0: u32, tx1: u32) -> u128 {
    // tx0 in 0..=127, tx1 in 1..=128, tx0 < tx1: two shifts, no branch.
    (u128::MAX >> (128 - tx1)) & (u128::MAX << tx0)
}

impl Plane {
    pub const EMPTY: Plane = Plane { bbox: NRect::EMPTY, rows: [0; MAX_TILES_Y] };

    /// Add a rect (already canonical) clamped to `window`. Branch-free row mask; the loop only
    /// runs over the rect's tile rows.
    #[inline]
    pub fn add(&mut self, r: NRect, window: NRect) {
        let c = r.intersect(window);
        if c.is_empty() {
            return;
        }
        self.bbox = self.bbox.union(c);
        let (ty0, ty1, tx0, tx1) = tile_span(c);
        let m = row_mask(tx0, tx1);
        for row in &mut self.rows[ty0..ty1] {
            *row |= m;
        }
    }

    /// Clear only the rows the bbox says may be dirty.
    #[inline]
    pub fn clear(&mut self) {
        if !self.bbox.is_empty() {
            let (ty0, ty1, _, _) = tile_span(self.bbox);
            self.rows[ty0..ty1].fill(0);
        }
        self.bbox = NRect::EMPTY;
    }

    /// `self |= o`.
    #[inline]
    pub fn or_assign(&mut self, o: &Plane) {
        if o.bbox.is_empty() {
            return;
        }
        let (ty0, ty1, _, _) = tile_span(o.bbox);
        for y in ty0..ty1 {
            self.rows[y] |= o.rows[y];
        }
        self.bbox = self.bbox.union(o.bbox);
    }

    #[inline]
    pub fn tile(&self, tx: usize, ty: usize) -> bool {
        (self.rows[ty] >> tx) & 1 != 0
    }

    pub fn count(&self) -> u32 {
        self.rows.iter().map(|r| r.count_ones()).sum()
    }

    /// Does any set tile overlap the (canonical) rect clamped to `window`?
    #[inline]
    pub fn hits(&self, r: NRect, window: NRect) -> bool {
        let c = r.intersect(window);
        if c.is_empty() {
            return false;
        }
        let (ty0, ty1, tx0, tx1) = tile_span(c);
        let m = row_mask(tx0, tx1);
        let mut acc = 0u128;
        for row in &self.rows[ty0..ty1] {
            acc |= *row & m;
        }
        acc != 0
    }
}

pub struct DamageRing {
    planes: Box<[Plane; RING]>,
    pub epoch: u32,
    pub window: NRect,
}

impl DamageRing {
    pub fn new(w: i32, h: i32) -> Self {
        assert!(w > 0 && h > 0 && w <= (MAX_TILES_X as i32) * TILE && h <= (MAX_TILES_Y as i32) * TILE, "window up to 2048x1536");
        DamageRing {
            planes: Box::new([const { Plane::EMPTY }; RING]),
            epoch: 1,
            window: NRect::new(0, 0, w, h),
        }
    }
    #[inline(always)]
    pub fn add(&mut self, r: NRect) {
        let w = self.window;
        let e = self.epoch as usize % RING;
        self.planes[e].add(r, w);
    }
    /// Start a new epoch: the slot it reuses (epoch - 8) is cleared.
    pub fn advance(&mut self) {
        self.epoch += 1;
        let e = self.epoch as usize % RING;
        self.planes[e].clear();
    }
    /// The plane of the epoch `back` epochs ago (0 = current).
    pub fn plane(&self, back: u32) -> &Plane {
        &self.planes[self.epoch.wrapping_sub(back) as usize % RING]
    }
    /// Damage a buffer of `age` frames needs repaired: OR of the last `age` planes, current included.
    pub fn repair(&self, age: u32) -> Plane {
        assert!((1..=RING as u32).contains(&age));
        let mut out = Plane::EMPTY;
        for back in 0..age {
            out.or_assign(self.plane(back));
        }
        out
    }
}
