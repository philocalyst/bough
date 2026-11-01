//! Tile bitplane: one bit per T x T device-pixel tile, rows padded to whole
//! u64 words (phantom tiles past the right edge are never set), plus the
//! tile-space bbox of set bits so clear/OR/cull touch only that window.

use crate::geom::R;

#[derive(Clone)]
pub struct Plane {
    pub sh: u32,
    pub w: i32,
    pub h: i32,
    pub cols: i32,
    pub rows: i32,
    pub stride: usize,
    pub bits: Vec<u64>,
    /// Tile-space bbox of set bits (exclusive ends), `R::E` when clear.
    pub bb: R,
}

impl Plane {
    pub fn new(w: i32, h: i32, sh: u32) -> Plane {
        let t = 1 << sh;
        let cols = (w + t - 1) >> sh;
        let rows = (h + t - 1) >> sh;
        let stride = (cols as usize).div_ceil(64);
        Plane { sh, w, h, cols, rows, stride, bits: vec![0; stride * rows as usize], bb: R::E }
    }
    pub fn bytes(&self) -> usize {
        self.bits.len() * 8
    }
    #[inline(always)]
    fn span(&self, r: R) -> (i32, i32, i32, i32) {
        let s = self.sh;
        (r.x0 >> s, r.y0 >> s, ((r.x1 - 1) >> s) + 1, ((r.y1 - 1) >> s) + 1)
    }
    /// Mark the tiles under `r` (device px, inside the viewport, non-empty).
    #[inline(always)]
    pub fn mark(&mut self, r: R) {
        let (tx0, ty0, tx1, ty1) = self.span(r);
        self.mark_tiles(tx0, ty0, tx1, ty1);
    }
    #[inline(always)]
    pub fn mark_tiles(&mut self, tx0: i32, ty0: i32, tx1: i32, ty1: i32) {
        self.bb = self.bb.union(R::new(tx0, ty0, tx1, ty1));
        let (w0, w1) = ((tx0 >> 6) as usize, ((tx1 - 1) >> 6) as usize);
        let lo = !0u64 << (tx0 & 63);
        let hi = !0u64 >> (63 - ((tx1 - 1) & 63));
        let st = self.stride;
        if w0 == w1 {
            let m = lo & hi;
            for y in ty0 as usize..ty1 as usize {
                self.bits[y * st + w0] |= m;
            }
        } else {
            for y in ty0 as usize..ty1 as usize {
                let row = &mut self.bits[y * st..y * st + st];
                row[w0] |= lo;
                for w in w0 + 1..w1 {
                    row[w] = !0;
                }
                row[w1] |= hi;
            }
        }
    }
    /// Any tile under `r` set?
    #[inline(always)]
    pub fn test(&self, r: R) -> bool {
        let (tx0, ty0, tx1, ty1) = self.span(r);
        let (w0, w1) = ((tx0 >> 6) as usize, ((tx1 - 1) >> 6) as usize);
        let lo = !0u64 << (tx0 & 63);
        let hi = !0u64 >> (63 - ((tx1 - 1) & 63));
        let st = self.stride;
        if w0 == w1 {
            let m = lo & hi;
            for y in ty0 as usize..ty1 as usize {
                if self.bits[y * st + w0] & m != 0 {
                    return true;
                }
            }
        } else {
            for y in ty0 as usize..ty1 as usize {
                let row = &self.bits[y * st..y * st + st];
                if row[w0] & lo != 0 || row[w1] & hi != 0 {
                    return true;
                }
                for w in w0 + 1..w1 {
                    if row[w] != 0 {
                        return true;
                    }
                }
            }
        }
        false
    }
    #[inline]
    pub fn get(&self, tx: i32, ty: i32) -> bool {
        self.bits[ty as usize * self.stride + (tx >> 6) as usize] >> (tx & 63) & 1 != 0
    }
    #[inline]
    pub fn set(&mut self, tx: i32, ty: i32) {
        self.bb = self.bb.union(R::new(tx, ty, tx + 1, ty + 1));
        self.bits[ty as usize * self.stride + (tx >> 6) as usize] |= 1 << (tx & 63);
    }
    /// Clear the rows inside the bbox.
    #[inline]
    pub fn clear(&mut self) {
        if self.bb.is_empty() {
            return;
        }
        let st = self.stride;
        self.bits[self.bb.y0 as usize * st..self.bb.y1 as usize * st].fill(0);
        self.bb = R::E;
    }
    /// `self |= o` over `o`'s bbox rows.
    #[inline]
    pub fn or_from(&mut self, o: &Plane) {
        if o.bb.is_empty() {
            return;
        }
        let st = self.stride;
        let (a, b) = (o.bb.y0 as usize * st, o.bb.y1 as usize * st);
        for (d, s) in self.bits[a..b].iter_mut().zip(&o.bits[a..b]) {
            *d |= *s;
        }
        self.bb = self.bb.union(o.bb);
    }
    /// Pixel bbox of set tiles, clipped to the surface.
    #[inline]
    pub fn px_bbox(&self) -> R {
        if self.bb.is_empty() {
            return R::E;
        }
        let s = self.sh;
        R::new(self.bb.x0 << s, self.bb.y0 << s, (self.bb.x1 << s).min(self.w), (self.bb.y1 << s).min(self.h))
    }
    /// Emit horizontal runs of set tiles as pixel rects (clipped).
    pub fn emit(&self, emit: &mut dyn FnMut(R)) {
        if self.bb.is_empty() {
            return;
        }
        let s = self.sh;
        for ty in self.bb.y0..self.bb.y1 {
            let mut tx = self.bb.x0;
            while tx < self.bb.x1 {
                if self.get(tx, ty) {
                    let a = tx;
                    while tx < self.bb.x1 && self.get(tx, ty) {
                        tx += 1;
                    }
                    let r = R::new(a << s, ty << s, tx << s, (ty + 1) << s).clip(R::new(0, 0, self.w, self.h));
                    if !r.is_empty() {
                        emit(r);
                    }
                } else {
                    tx += 1;
                }
            }
        }
    }
    pub fn count(&self) -> u64 {
        self.bits.iter().map(|w| w.count_ones() as u64).sum()
    }
}
