//! Tile bitmaps with buffer-age OR-history: bounds support (formulation 3) or
//! exact symmetric-difference support (formulation 4), one or two levels.

use super::*;

const K: usize = 4;

/// 0 = bounds, 1 = exact per slot, 2 = exact nodes + group bounds.
/// `runs`: cull emits format-C (slot, run) instances. `sat`: cull through a
/// summed-area table.
pub struct Tiles {
    pub sh: u32,
    pub exact: u8,
    pub runs: bool,
    pub use_sat: bool,
    sat: Sat,
    planes: Vec<Plane>,
    acc: Plane,
    ep: usize,
}

impl Tiles {
    pub fn new(sh: u32, exact: u8) -> Self {
        Tiles { sh, exact, runs: false, use_sat: false, sat: Sat::default(), planes: vec![], acc: Plane::new(1, 1, sh), ep: 0 }
    }
}

impl Form for Tiles {
    fn name(&self) -> String {
        let base = if self.use_sat { "xs" } else { ["tile", "exact", "xg"][self.exact as usize] };
        format!("{}{}{}", base, 1 << self.sh, if self.runs { "c" } else { "" })
    }
    fn reset(&mut self, _tr: &Trace, sc: &Scene, _ring: usize) {
        self.planes = vec![Plane::new(sc.w, sc.h, self.sh); K];
        self.acc = Plane::new(sc.w, sc.h, self.sh);
        self.ep = 0;
    }
    fn record(&mut self, f: &Frame, _sc: &Scene) {
        self.ep += 1;
        let p = &mut self.planes[self.ep % K];
        p.clear();
        match self.exact {
            0 => bounds_events(f, &mut |r| p.mark(r)),
            1 => exact_events(f, &mut |r| p.mark(r)),
            _ => exact_nodes_group_bounds(f, &mut |r| p.mark(r)),
        }
    }
    fn repair(&mut self, _surf: usize, age: u32, sc: &Scene, out: &mut Out) {
        self.acc.clear();
        for k in 0..age as usize {
            self.acc.or_from(&self.planes[(self.ep - k) % K]);
        }
        match (self.use_sat, self.runs) {
            (false, false) => cull_plane(sc, &self.acc, &mut out.list),
            (false, true) => cull_plane_runs(sc, &self.acc, out),
            (true, false) => cull_sat(sc, &self.acc, &mut self.sat, &mut out.list),
            (true, true) => cull_sat_runs(sc, &self.acc, &mut self.sat, out),
        }
        out.close(self.acc.px_bbox());
    }
    fn region(&self, emit: &mut dyn FnMut(R)) {
        self.acc.emit(emit)
    }
    fn mem_bytes(&self) -> usize {
        self.planes.iter().map(|p| p.bytes()).sum::<usize>() + self.acc.bytes()
    }
}

/// Two-level hierarchical bitmap: 128 px super-tiles over 16 px tiles; the
/// cull rejects on the coarse level first.
pub struct Tiles2 {
    exact: bool,
    l0: Vec<Plane>,
    l1: Vec<Plane>,
    a0: Plane,
    a1: Plane,
    ep: usize,
}

impl Tiles2 {
    pub fn new(exact: bool) -> Self {
        Tiles2 { exact, l0: vec![], l1: vec![], a0: Plane::new(1, 1, 4), a1: Plane::new(1, 1, 7), ep: 0 }
    }
}

impl Form for Tiles2 {
    fn name(&self) -> String {
        if self.exact { "exact2L".into() } else { "tile2L".into() }
    }
    fn reset(&mut self, _tr: &Trace, sc: &Scene, _ring: usize) {
        self.l0 = vec![Plane::new(sc.w, sc.h, 4); K];
        self.l1 = vec![Plane::new(sc.w, sc.h, 7); K];
        self.a0 = Plane::new(sc.w, sc.h, 4);
        self.a1 = Plane::new(sc.w, sc.h, 7);
        self.ep = 0;
    }
    fn record(&mut self, f: &Frame, _sc: &Scene) {
        self.ep += 1;
        let (p0, p1) = (&mut self.l0[self.ep % K], &mut self.l1[self.ep % K]);
        p0.clear();
        p1.clear();
        let mut m = |r: R| {
            p0.mark(r);
            p1.mark(r);
        };
        if self.exact {
            exact_events(f, &mut m);
        } else {
            bounds_events(f, &mut m);
        }
    }
    fn repair(&mut self, _surf: usize, age: u32, sc: &Scene, out: &mut Out) {
        self.a0.clear();
        self.a1.clear();
        for k in 0..age as usize {
            self.a0.or_from(&self.l0[(self.ep - k) % K]);
            self.a1.or_from(&self.l1[(self.ep - k) % K]);
        }
        let bb = self.a0.px_bbox();
        if !bb.is_empty() {
            let q = N::from_r(bb);
            for (p, pg) in sc.page.iter().enumerate() {
                if !pg.meets(q) {
                    continue;
                }
                let base = p * 64;
                for (k, c) in sc.cov[base..base + 64].iter().enumerate() {
                    if c.meets(q) {
                        let r = c.to_r();
                        if self.a1.test(r) && self.a0.test(r) {
                            out.list.push((base + k) as u32);
                        }
                    }
                }
            }
        }
        out.close(bb);
    }
    fn region(&self, emit: &mut dyn FnMut(R)) {
        self.a0.emit(emit)
    }
    fn mem_bytes(&self) -> usize {
        self.l0.iter().chain(&self.l1).map(|p| p.bytes()).sum::<usize>()
    }
}

/// Versions at the tile: each tile stores the epoch it last changed (a store,
/// never cleared). A consumer at epoch e takes the tiles with stamp > e:
/// unbounded consumers, no ring of planes, one compare per tile per query.
pub struct TileStamps {
    sh: u32,
    cols: i32,
    rows: i32,
    stamp: Vec<u32>,
    acc: Plane,
    ep: u32,
}

impl TileStamps {
    pub fn new(sh: u32) -> Self {
        TileStamps { sh, cols: 0, rows: 0, stamp: vec![], acc: Plane::new(1, 1, sh), ep: 0 }
    }
}

impl Form for TileStamps {
    fn name(&self) -> String {
        format!("tstamp{}", 1 << self.sh)
    }
    fn reset(&mut self, _tr: &Trace, sc: &Scene, _ring: usize) {
        self.acc = Plane::new(sc.w, sc.h, self.sh);
        self.cols = self.acc.cols;
        self.rows = self.acc.rows;
        self.stamp = vec![0; (self.cols * self.rows) as usize];
        self.ep = 0;
    }
    fn record(&mut self, f: &Frame, _sc: &Scene) {
        self.ep += 1;
        let (sh, cols, ep) = (self.sh, self.cols, self.ep);
        let st = &mut self.stamp;
        exact_nodes_group_bounds(f, &mut |r: R| {
            for ty in r.y0 >> sh..((r.y1 - 1) >> sh) + 1 {
                let row = (ty * cols) as usize;
                st[row + (r.x0 >> sh) as usize..row + (((r.x1 - 1) >> sh) + 1) as usize].fill(ep);
            }
        });
    }
    fn repair(&mut self, _surf: usize, age: u32, sc: &Scene, out: &mut Out) {
        let e = self.ep - age;
        self.acc.clear();
        for ty in 0..self.rows {
            let row = &self.stamp[(ty * self.cols) as usize..((ty + 1) * self.cols) as usize];
            for (tx, &v) in row.iter().enumerate() {
                if v > e {
                    self.acc.set(tx as i32, ty);
                }
            }
        }
        cull_plane(sc, &self.acc, &mut out.list);
        out.close(self.acc.px_bbox());
    }
    fn region(&self, emit: &mut dyn FnMut(R)) {
        self.acc.emit(emit)
    }
    fn mem_bytes(&self) -> usize {
        self.stamp.len() * 4
    }
}
