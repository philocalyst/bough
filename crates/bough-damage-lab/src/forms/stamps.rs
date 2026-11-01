//! Formulation 5: versions, not flags. Each slot carries the epoch of its
//! last change (`stamp`), each 64-slot page the max stamp. Nothing is
//! cleared: a consumer at epoch `e` asks "changed since e" by scanning page
//! max stamps, then slot stamps, and marks the *current* coverage of what
//! changed. Old support (where a slot used to paint) cannot be recovered from
//! a stamp, so departures go to a per-epoch plane (ring) at write time.

use super::*;

const K: usize = 4;

pub struct Stamps {
    sh: u32,
    stamp: Vec<u32>,
    pmax: Vec<u32>,
    dep: Vec<Plane>,
    acc: Plane,
    ep: u32,
}

impl Stamps {
    pub fn new(sh: u32) -> Self {
        Stamps { sh, stamp: vec![], pmax: vec![], dep: vec![], acc: Plane::new(1, 1, sh), ep: 0 }
    }
    #[inline(always)]
    fn touch(&mut self, s: usize, o: &PState, n: &PState) {
        if o == n {
            return;
        }
        self.stamp[s] = self.ep;
        self.pmax[s >> 6] = self.ep;
        if !o.cov.is_empty() && o.cov != n.cov {
            self.dep[self.ep as usize % K].mark(o.cov);
        }
    }
}

impl Form for Stamps {
    fn name(&self) -> String {
        format!("stamps{}", 1 << self.sh)
    }
    fn reset(&mut self, tr: &Trace, sc: &Scene, _ring: usize) {
        self.stamp = vec![0; tr.slots];
        self.pmax = vec![0; tr.slots / 64];
        self.dep = vec![Plane::new(sc.w, sc.h, self.sh); K];
        self.acc = Plane::new(sc.w, sc.h, self.sh);
        self.ep = 0;
    }
    fn record(&mut self, f: &Frame, _sc: &Scene) {
        self.ep += 1;
        self.dep[self.ep as usize % K].clear();
        for g in &f.groups {
            for j in g.at as usize..(g.at + g.n) as usize {
                self.touch(f.gslots[j] as usize, &f.gold[j], &f.gnew[j]);
            }
        }
        for e in &f.nodes {
            for k in 0..e.n as usize {
                let j = e.at as usize + k;
                self.touch(e.slot0 as usize + k, &f.olds[j], &f.news[j]);
            }
        }
    }
    fn repair(&mut self, _surf: usize, age: u32, sc: &Scene, out: &mut Out) {
        let e = self.ep - age;
        self.acc.clear();
        for k in 0..age {
            self.acc.or_from(&self.dep[(self.ep - k) as usize % K]);
        }
        for (p, &m) in self.pmax.iter().enumerate() {
            if m <= e {
                continue;
            }
            for s in p * 64..p * 64 + 64 {
                if self.stamp[s] > e {
                    let c = sc.cov[s];
                    if !c.is_empty() {
                        self.acc.mark(c.to_r());
                    }
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
        self.stamp.len() * 4 + self.pmax.len() * 4 + self.dep.iter().map(|p| p.bytes()).sum::<usize>()
    }
}
