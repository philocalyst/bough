//! Formulation 6: content signatures. Each tile keeps an additive 64-bit
//! signature: the sum over slots covering it of a hash of what that slot
//! paints *in that tile* (slot index = paint order, key, kind, coverage
//! clipped to the tile, and the content anchor relative to the tile: glyph
//! origin, rounded-corner squares). Updated incrementally at write time
//! (subtract old contribution, add new). Each surface keeps the signatures it
//! was drawn with; its damage is the touched tiles whose signature differs.
//! Exact w.r.t. NET change over the age window (A→B→A costs nothing).

use super::*;

const K: usize = 4;

#[inline(always)]
fn fin(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}
#[inline(always)]
fn m(h: u64, v: u64) -> u64 {
    (h ^ v).wrapping_mul(0x9E37_79B9_7F4A_7C15).rotate_left(27)
}
#[inline(always)]
fn pk(a: i32, b: i32, c: i32, d: i32) -> u64 {
    (a as u16 as u64) | (b as u16 as u64) << 16 | (c as u16 as u64) << 32 | (d as u16 as u64) << 48
}

/// What slot `slot` in state `s` paints inside tile `t`.
#[inline(always)]
pub fn contrib(s: &PState, slot: u32, t: R) -> u64 {
    let c = s.cov.inter(t);
    let mut h = m(slot as u64 | (s.kind as u64) << 32 | (s.rad as u64) << 40, s.key as u64);
    h = m(h, pk(c.x0 - t.x0, c.y0 - t.y0, c.x1 - t.x0, c.y1 - t.y0));
    match s.kind {
        GLYPH => h = m(h, pk(s.ink.x0 - t.x0, s.ink.y0 - t.y0, 0, 0)),
        ROUND => {
            let r = s.rad as i32;
            let i = s.ink;
            let rx = r.min(i.x1 - i.x0);
            let ry = r.min(i.y1 - i.y0);
            for (k, q) in [
                R::new(i.x0, i.y0, i.x0 + rx, i.y0 + ry),
                R::new(i.x1 - rx, i.y0, i.x1, i.y0 + ry),
                R::new(i.x0, i.y1 - ry, i.x0 + rx, i.y1),
                R::new(i.x1 - rx, i.y1 - ry, i.x1, i.y1),
            ]
            .into_iter()
            .enumerate()
            {
                if q.meets(c) {
                    h = m(h, pk(q.x0 - t.x0, q.y0 - t.y0, q.x1 - t.x0, q.y1 - t.y0) ^ (k as u64) << 62);
                }
            }
        }
        _ => {}
    }
    fin(h)
}

pub struct Sig {
    sh: u32,
    cols: i32,
    sig: Vec<u64>,
    snap: Vec<Vec<u64>>,
    touched: Vec<Plane>,
    cand: Plane,
    reg: Plane,
    ep: usize,
}

impl Sig {
    pub fn new(sh: u32) -> Self {
        Sig {
            sh,
            cols: 0,
            sig: vec![],
            snap: vec![],
            touched: vec![],
            cand: Plane::new(1, 1, sh),
            reg: Plane::new(1, 1, sh),
            ep: 0,
        }
    }
    #[inline(always)]
    fn apply(&mut self, slot: u32, s: &PState, add: bool) {
        let c = s.cov;
        if c.is_empty() {
            return;
        }
        let sh = self.sh;
        let (tx0, ty0, tx1, ty1) = (c.x0 >> sh, c.y0 >> sh, ((c.x1 - 1) >> sh) + 1, ((c.y1 - 1) >> sh) + 1);
        for ty in ty0..ty1 {
            for tx in tx0..tx1 {
                let t = R::new(tx << sh, ty << sh, (tx + 1) << sh, (ty + 1) << sh);
                let v = contrib(s, slot, t);
                let e = &mut self.sig[(ty * self.cols + tx) as usize];
                *e = if add { e.wrapping_add(v) } else { e.wrapping_sub(v) };
            }
        }
        self.touched[self.ep % K].mark_tiles(tx0, ty0, tx1, ty1);
    }
    #[inline(always)]
    fn change(&mut self, slot: u32, o: &PState, n: &PState) {
        if o == n {
            return;
        }
        self.apply(slot, o, false);
        self.apply(slot, n, true);
    }
}

impl Form for Sig {
    fn name(&self) -> String {
        format!("sig{}", 1 << self.sh)
    }
    fn reset(&mut self, tr: &Trace, sc: &Scene, ring: usize) {
        let p = Plane::new(sc.w, sc.h, self.sh);
        self.cols = p.cols;
        self.sig = vec![0; (p.cols * p.rows) as usize];
        self.touched = vec![p.clone(); K];
        self.cand = p.clone();
        self.reg = p;
        self.ep = 0;
        for (s, st) in tr.init.iter().enumerate() {
            self.apply(s as u32, st, true);
        }
        self.touched[0].clear();
        self.snap = vec![self.sig.clone(); ring];
    }
    fn record(&mut self, f: &Frame, _sc: &Scene) {
        self.ep += 1;
        self.touched[self.ep % K].clear();
        for g in &f.groups {
            for j in g.at as usize..(g.at + g.n) as usize {
                self.change(f.gslots[j], &f.gold[j], &f.gnew[j]);
            }
        }
        for e in &f.nodes {
            for k in 0..e.n as usize {
                let j = e.at as usize + k;
                self.change(e.slot0 + k as u32, &f.olds[j], &f.news[j]);
            }
        }
    }
    fn full(&mut self, surf: usize, _sc: &Scene) {
        self.snap[surf].copy_from_slice(&self.sig);
    }
    fn repair(&mut self, surf: usize, age: u32, sc: &Scene, out: &mut Out) {
        self.cand.clear();
        for k in 0..age as usize {
            self.cand.or_from(&self.touched[(self.ep - k) % K]);
        }
        self.reg.clear();
        let bb = self.cand.bb;
        let snap = &mut self.snap[surf];
        if !bb.is_empty() {
            let st = self.cand.stride;
            for ty in bb.y0..bb.y1 {
                for w in 0..st {
                    let mut word = self.cand.bits[ty as usize * st + w];
                    while word != 0 {
                        let tx = (w * 64) as i32 + word.trailing_zeros() as i32;
                        word &= word - 1;
                        let i = (ty * self.cols + tx) as usize;
                        if self.sig[i] != snap[i] {
                            snap[i] = self.sig[i];
                            self.reg.set(tx, ty);
                        }
                    }
                }
            }
        }
        cull_plane(sc, &self.reg, &mut out.list);
        out.close(self.reg.px_bbox());
    }
    fn region(&self, emit: &mut dyn FnMut(R)) {
        self.reg.emit(emit)
    }
    fn mem_bytes(&self) -> usize {
        self.sig.len() * 8 * (1 + self.snap.len()) + self.touched.iter().map(|p| p.bytes()).sum::<usize>()
    }
}
