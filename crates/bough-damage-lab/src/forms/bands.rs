//! Damage as interval sets per scanline band: each band (2^sh px tall) keeps
//! up to `KI` disjoint x-intervals, merged on insert (min/max algebra on
//! `(x0, x1)`); on overflow the nearest pair is fused. Exact in x, band-
//! quantised in y. Ring of epochs; a query unions `age` epochs per band.

use super::*;

const KI: usize = 8;
const K: usize = 4;

#[derive(Clone, Copy, Default)]
pub struct Band {
    n: u8,
    x0: [i16; KI],
    x1: [i16; KI],
}

impl Band {
    #[inline(always)]
    fn insert(&mut self, mut a: i16, mut c: i16) {
        loop {
            let mut i = 0;
            while i < self.n as usize {
                if self.x0[i] <= c && a <= self.x1[i] {
                    a = a.min(self.x0[i]);
                    c = c.max(self.x1[i]);
                    let last = self.n as usize - 1;
                    self.x0[i] = self.x0[last];
                    self.x1[i] = self.x1[last];
                    self.n -= 1;
                } else {
                    i += 1;
                }
            }
            if (self.n as usize) < KI {
                self.x0[self.n as usize] = a;
                self.x1[self.n as usize] = c;
                self.n += 1;
                return;
            }
            // Full: fuse with the nearest interval and retry.
            let mut best = 0;
            let mut gap = i32::MAX;
            for i in 0..KI {
                let g = (self.x0[i] as i32 - c as i32).max(a as i32 - self.x1[i] as i32);
                if g < gap {
                    gap = g;
                    best = i;
                }
            }
            a = a.min(self.x0[best]);
            c = c.max(self.x1[best]);
        }
    }
    #[inline(always)]
    fn meets(&self, a: i16, c: i16) -> bool {
        let mut hit = false;
        for i in 0..self.n as usize {
            hit |= self.x0[i] < c && a < self.x1[i];
        }
        hit
    }
}

pub struct Bands {
    sh: u32,
    w: i32,
    h: i32,
    ring: Vec<Vec<Band>>,
    span: Vec<(i32, i32)>,
    acc: Vec<Band>,
    aspan: (i32, i32),
    ep: usize,
}

impl Bands {
    pub fn new(sh: u32) -> Self {
        Bands { sh, w: 0, h: 0, ring: vec![], span: vec![], acc: vec![], aspan: (0, 0), ep: 0 }
    }
}

impl Form for Bands {
    fn name(&self) -> String {
        format!("bands{}", 1 << self.sh)
    }
    fn reset(&mut self, _tr: &Trace, sc: &Scene, _ring: usize) {
        let nb = ((sc.h + (1 << self.sh) - 1) >> self.sh) as usize;
        self.w = sc.w;
        self.h = sc.h;
        self.ring = vec![vec![Band::default(); nb]; K];
        self.span = vec![(0, 0); K];
        self.acc = vec![Band::default(); nb];
        self.aspan = (0, 0);
        self.ep = 0;
    }
    fn record(&mut self, f: &Frame, _sc: &Scene) {
        self.ep += 1;
        let e = self.ep % K;
        let (b0, b1) = self.span[e];
        for b in &mut self.ring[e][b0 as usize..b1 as usize] {
            b.n = 0;
        }
        let sh = self.sh;
        let bands = &mut self.ring[e];
        let mut span = (i32::MAX, i32::MIN);
        exact_nodes_group_bounds(f, &mut |r: R| {
            let (y0, y1) = (r.y0 >> sh, ((r.y1 - 1) >> sh) + 1);
            span = (span.0.min(y0), span.1.max(y1));
            for b in &mut bands[y0 as usize..y1 as usize] {
                b.insert(r.x0 as i16, r.x1 as i16);
            }
        });
        self.span[e] = if span.0 < span.1 { span } else { (0, 0) };
    }
    fn repair(&mut self, _surf: usize, age: u32, sc: &Scene, out: &mut Out) {
        let (a0, a1) = self.aspan;
        for b in &mut self.acc[a0 as usize..a1 as usize] {
            b.n = 0;
        }
        let mut span = (i32::MAX, i32::MIN);
        for k in 0..age as usize {
            let e = (self.ep - k) % K;
            let (b0, b1) = self.span[e];
            if b0 >= b1 {
                continue;
            }
            span = (span.0.min(b0), span.1.max(b1));
            for y in b0 as usize..b1 as usize {
                let src = self.ring[e][y];
                for i in 0..src.n as usize {
                    self.acc[y].insert(src.x0[i], src.x1[i]);
                }
            }
        }
        self.aspan = if span.0 < span.1 { span } else { (0, 0) };
        let (a0, a1) = self.aspan;
        if a0 < a1 {
            let sh = self.sh;
            let bb = R::new(0, a0 << sh, self.w, (a1 << sh).min(self.h));
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
                    let r = c.to_r();
                    let (y0, y1) = ((r.y0 >> sh).max(a0), (((r.y1 - 1) >> sh) + 1).min(a1));
                    let mut hit = false;
                    for y in y0..y1 {
                        hit |= self.acc[y as usize].meets(r.x0 as i16, r.x1 as i16);
                    }
                    if hit {
                        out.list.push((base + k) as u32);
                    }
                }
            }
            out.close(bb);
        } else {
            out.close(R::E);
        }
    }
    fn region(&self, emit: &mut dyn FnMut(R)) {
        let (a0, a1) = self.aspan;
        let vp = R::new(0, 0, self.w, self.h);
        for y in a0..a1 {
            let b = &self.acc[y as usize];
            for i in 0..b.n as usize {
                let r = R::new(b.x0[i] as i32, y << self.sh, b.x1[i] as i32, (y + 1) << self.sh).clip(vp);
                if !r.is_empty() {
                    emit(r);
                }
            }
        }
    }
    fn mem_bytes(&self) -> usize {
        (self.ring.len() + 1) * self.acc.len() * std::mem::size_of::<Band>()
    }
}
