//! The retained substrate every formulation culls against: a paint-ordered
//! slot array of clipped coverage in NRect form plus 64-slot page summaries
//! (union coverage). Maintaining it is the tree's job, not damage's: its cost
//! is measured separately (bench stage A) and subtracted.

use crate::geom::{N, R};
use crate::trace::{Frame, Trace};

pub struct Scene {
    pub w: i32,
    pub h: i32,
    pub viewport: R,
    pub cov: Vec<N>,
    pub kind: Vec<u8>,
    pub page: Vec<N>,
    dirty: Vec<u64>,
}

impl Scene {
    pub fn new(tr: &Trace) -> Scene {
        let mut s = Scene {
            w: tr.w,
            h: tr.h,
            viewport: R::new(0, 0, tr.w, tr.h),
            cov: vec![N::EMPTY; tr.slots],
            kind: tr.kinds.clone(),
            page: vec![N::EMPTY; tr.slots / 64],
            dirty: vec![0; (tr.slots / 64).div_ceil(64)],
        };
        s.reset(tr);
        s
    }
    pub fn reset(&mut self, tr: &Trace) {
        for (c, st) in self.cov.iter_mut().zip(&tr.init) {
            *c = N::from_r(st.cov);
        }
        for p in 0..self.page.len() {
            self.summarize(p);
        }
    }
    #[inline]
    fn summarize(&mut self, p: usize) {
        let mut u = N::EMPTY;
        for c in &self.cov[p * 64..p * 64 + 64] {
            u = u.min(*c);
        }
        self.page[p] = u;
    }
    #[inline]
    pub fn apply(&mut self, f: &Frame) {
        for g in &f.groups {
            for j in g.at as usize..(g.at + g.n) as usize {
                let s = f.gslots[j] as usize;
                self.cov[s] = N::from_r(f.gnew[j].cov);
                self.dirty[s >> 12] |= 1 << ((s >> 6) & 63);
            }
        }
        for e in &f.nodes {
            for k in 0..e.n as usize {
                let s = e.slot0 as usize + k;
                self.cov[s] = N::from_r(f.news[e.at as usize + k].cov);
                self.dirty[s >> 12] |= 1 << ((s >> 6) & 63);
            }
        }
        for w in 0..self.dirty.len() {
            let mut m = std::mem::take(&mut self.dirty[w]);
            while m != 0 {
                let p = w * 64 + m.trailing_zeros() as usize;
                m &= m - 1;
                self.summarize(p);
            }
        }
    }
}
