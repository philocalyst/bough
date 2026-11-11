//! Damage as a byte per hardware tile (domain change: bits -> bytes). A
//! slot's span test in one tile row is one unaligned u64 load and a mask
//! (spans <= 8 tiles, i.e. everything but panels); OR-ing history is a
//! byte-wise OR the compiler vectorises 16 tiles per instruction.
//! Invariant: `b` has 8 bytes of zero padding past the last row, so the
//! unaligned load at any in-range tile index stays inside the allocation.

use super::*;

const K: usize = 4;

#[derive(Clone)]
pub struct Bmap {
    pub sh: u32,
    pub w: i32,
    pub h: i32,
    pub cols: i32,
    pub rows: i32,
    pub b: Vec<u8>,
    pub bb: R,
}

impl Bmap {
    pub fn new(w: i32, h: i32, sh: u32) -> Bmap {
        let t = 1 << sh;
        let cols = (w + t - 1) >> sh;
        let rows = (h + t - 1) >> sh;
        Bmap { sh, w, h, cols, rows, b: vec![0; (cols * rows) as usize + 8], bb: R::E }
    }
    #[inline(always)]
    pub fn mark(&mut self, r: R) {
        let s = self.sh;
        let (tx0, ty0, tx1, ty1) = (r.x0 >> s, r.y0 >> s, ((r.x1 - 1) >> s) + 1, ((r.y1 - 1) >> s) + 1);
        self.bb = self.bb.union(R::new(tx0, ty0, tx1, ty1));
        for ty in ty0..ty1 {
            let row = (ty * self.cols) as usize;
            self.b[row + tx0 as usize..row + tx1 as usize].fill(1);
        }
    }
    #[inline]
    pub fn clear(&mut self) {
        let bb = self.bb;
        if bb.is_empty() {
            return;
        }
        for ty in bb.y0..bb.y1 {
            let row = (ty * self.cols) as usize;
            self.b[row + bb.x0 as usize..row + bb.x1 as usize].fill(0);
        }
        self.bb = R::E;
    }
    #[inline]
    pub fn or_from(&mut self, o: &Bmap) {
        let bb = o.bb;
        if bb.is_empty() {
            return;
        }
        for ty in bb.y0..bb.y1 {
            let row = (ty * self.cols) as usize;
            let (a, z) = (row + bb.x0 as usize, row + bb.x1 as usize);
            for (d, s) in self.b[a..z].iter_mut().zip(&o.b[a..z]) {
                *d |= *s;
            }
        }
        self.bb = self.bb.union(bb);
    }
    /// Any damaged tile in [tx0, tx1) x [ty0, ty1)? (in range, non-empty)
    #[inline(always)]
    pub fn test(&self, tx0: i32, ty0: i32, tx1: i32, ty1: i32) -> bool {
        let len = (tx1 - tx0) as u32;
        let p = self.b.as_ptr();
        let mut row = (ty0 * self.cols + tx0) as usize;
        if len <= 8 {
            let m = u64::MAX >> (64 - 8 * len);
            for _ in ty0..ty1 {
                // SAFETY: row + 8 <= cols*rows + 8 (padding invariant).
                let v = unsafe { (p.add(row) as *const u64).read_unaligned() };
                if v & m != 0 {
                    return true;
                }
                row += self.cols as usize;
            }
            false
        } else {
            for _ in ty0..ty1 {
                if self.b[row..row + len as usize].iter().any(|&x| x != 0) {
                    return true;
                }
                row += self.cols as usize;
            }
            false
        }
    }
    pub fn px_bbox(&self) -> R {
        if self.bb.is_empty() {
            return R::E;
        }
        let s = self.sh;
        R::new(self.bb.x0 << s, self.bb.y0 << s, (self.bb.x1 << s).min(self.w), (self.bb.y1 << s).min(self.h))
    }
    pub fn emit(&self, emit: &mut dyn FnMut(R)) {
        let s = self.sh;
        for ty in self.bb.y0..self.bb.y1 {
            let mut tx = self.bb.x0;
            while tx < self.bb.x1 {
                if self.b[(ty * self.cols + tx) as usize] != 0 {
                    let a = tx;
                    while tx < self.bb.x1 && self.b[(ty * self.cols + tx) as usize] != 0 {
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
}

/// Cull against a byte map: page summary vs bbox, then each slot's tile span.
#[inline]
pub fn cull_bytes(sc: &Scene, m: &Bmap, out: &mut Vec<u32>) {
    let bb = m.px_bbox();
    if bb.is_empty() {
        return;
    }
    let sh = m.sh;
    let q = N::from_r(bb);
    for (p, pg) in sc.page.iter().enumerate() {
        if !pg.meets(q) {
            continue;
        }
        let base = p * 64;
        for (k, c) in sc.cov[base..base + 64].iter().enumerate() {
            if !c.meets(q) {
                continue;
            }
            let n = c.0;
            let (tx0, ty0) = (n[0] as i32 >> sh, n[1] as i32 >> sh);
            let (tx1, ty1) = (((-(n[2] as i32) - 1) >> sh) + 1, ((-(n[3] as i32) - 1) >> sh) + 1);
            if m.test(tx0, ty0, tx1, ty1) {
                out.push((base + k) as u32);
            }
        }
    }
}

/// Exact node support + group bounds into a ring of byte maps.
pub struct XBytes {
    sh: u32,
    ring: Vec<Bmap>,
    acc: Bmap,
    ep: usize,
}

impl XBytes {
    pub fn new(sh: u32) -> Self {
        XBytes { sh, ring: vec![], acc: Bmap::new(1, 1, sh), ep: 0 }
    }
}

impl Form for XBytes {
    fn name(&self) -> String {
        format!("xb{}", 1 << self.sh)
    }
    fn reset(&mut self, _tr: &Trace, sc: &Scene, _ring: usize) {
        self.ring = vec![Bmap::new(sc.w, sc.h, self.sh); K];
        self.acc = Bmap::new(sc.w, sc.h, self.sh);
        self.ep = 0;
    }
    fn record(&mut self, f: &Frame, _sc: &Scene) {
        self.ep += 1;
        let m = &mut self.ring[self.ep % K];
        m.clear();
        exact_nodes_group_bounds(f, &mut |r| m.mark(r));
    }
    fn repair(&mut self, _surf: usize, age: u32, sc: &Scene, out: &mut Out) {
        self.acc.clear();
        for k in 0..age as usize {
            self.acc.or_from(&self.ring[(self.ep - k) % K]);
        }
        cull_bytes(sc, &self.acc, &mut out.list);
        out.close(self.acc.px_bbox());
    }
    fn region(&self, emit: &mut dyn FnMut(R)) {
        self.acc.emit(emit)
    }
    fn mem_bytes(&self) -> usize {
        (K + 1) * self.acc.b.len()
    }
}
