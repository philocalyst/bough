//! Differential correctness + pixel metrics, in lockstep per (trace, ring).
//!
//! Every frame: render the reference full frame (dumb oracle) and, for every
//! formulation, literally repair its ring surface of age `ring`: apply its
//! copy-forward blits, clear its region to background, redraw its draw list
//! batch by batch clipped to batch scissor ∩ region, then compare the whole
//! surface with the reference (memcmp). Any mismatch is a bug. The same pass
//! computes the true change set Δ = {p : ref_n(p) ≠ ref_{n-ring}(p)}: the
//! minimum any formulation could repaint.

use crate::forms::{Form, Out, Plane};
use crate::geom::R;
use crate::render::{BG, Fb, Mask, draw, render_full};
use crate::scene::Scene;
use crate::trace::{PState, Trace};

#[derive(Clone, Copy, Default, Debug)]
pub struct FrameMetrics {
    /// Pixels in the repair region (cleared + shaded at least once).
    pub region_px: u64,
    /// Fragments shaded: Σ over drawn prims of |cov ∩ scissor ∩ region|.
    pub frags: u64,
    /// Primitives (instances) drawn, counting each batch separately.
    pub prims: u64,
    pub batches: u64,
    /// Pixels blitted from the previous surface (copy-forward).
    pub copy_px: u64,
    /// 32x32 hardware tiles touched, format S: each drawn prim's coverage ∩
    /// its batch scissor (stencil/discard masking does not untouch a tile).
    pub t32_s: u64,
    /// 32x32 hardware tiles containing region pixels (format C: geometry
    /// clipped to runs of damaged hardware tiles).
    pub t32_r: u64,
    /// Instances under format C: per drawn prim, the damaged hardware-tile
    /// runs (per tile row) its clipped coverage overlaps.
    pub inst_c: u64,
}

#[derive(Default, Debug, Clone)]
pub struct FormResult {
    pub name: String,
    pub frames: Vec<FrameMetrics>,
    pub fails: u32,
    pub first_fail: Option<String>,
    pub mem: usize,
}

#[derive(Clone, Copy, Default, Debug)]
pub struct OracleFrame {
    pub px: u64,
    pub tiles16: u64,
    pub tiles8: u64,
    pub tiles4: u64,
    pub tiles32: u64,
}

pub fn apply_state(st: &mut [PState], f: &crate::trace::Frame) {
    for (j, &s) in f.gslots.iter().enumerate() {
        st[s as usize] = f.gnew[j];
    }
    for e in &f.nodes {
        for k in 0..e.n as usize {
            st[e.slot0 as usize + k] = f.news[e.at as usize + k];
        }
    }
}

fn oracle(a: &Fb, b: &Fb) -> OracleFrame {
    let (w, h) = (a.w, a.h);
    let mut o = OracleFrame::default();
    let c16 = w.div_ceil(16);
    let c8 = w.div_ceil(8);
    let c4 = w.div_ceil(4);
    let c32 = w.div_ceil(32);
    let mut t32 = vec![false; c32 * h.div_ceil(32)];
    let mut t16 = vec![false; c16 * h.div_ceil(16)];
    let mut t8 = vec![false; c8 * h.div_ceil(8)];
    let mut t4 = vec![false; c4 * h.div_ceil(4)];
    for y in 0..h {
        let (ra, rb) = (&a.px[y * w..y * w + w], &b.px[y * w..y * w + w]);
        if ra == rb {
            continue;
        }
        for x in 0..w {
            if ra[x] != rb[x] {
                o.px += 1;
                t16[(y / 16) * c16 + x / 16] = true;
                t8[(y / 8) * c8 + x / 8] = true;
                t4[(y / 4) * c4 + x / 4] = true;
                t32[(y / 32) * c32 + x / 32] = true;
            }
        }
    }
    o.tiles16 = t16.iter().filter(|&&b| b).count() as u64;
    o.tiles8 = t8.iter().filter(|&&b| b).count() as u64;
    o.tiles4 = t4.iter().filter(|&&b| b).count() as u64;
    o.tiles32 = t32.iter().filter(|&&b| b).count() as u64;
    o
}

struct Lane {
    form: Box<dyn Form>,
    surfs: Vec<Fb>,
    mask: Mask,
    rects: Vec<R>,
    out: Out,
    res: FormResult,
    tmp: Fb,
    hw_r: Plane,
    hw_s: Plane,
}

/// Runs of set bits of `pl` row `ty` within tile columns [tx0, tx1).
fn runs_in(pl: &Plane, ty: i32, tx0: i32, tx1: i32) -> u64 {
    let mut n = 0u64;
    let mut prev = false;
    for tx in tx0..tx1 {
        let b = pl.get(tx, ty);
        n += (b && !prev) as u64;
        prev = b;
    }
    n
}

pub fn verify(
    tr: &Trace,
    ring: usize,
    forms: Vec<Box<dyn Form>>,
    max_frames: usize,
    with_oracle: bool,
) -> (Vec<FormResult>, Vec<OracleFrame>) {
    let mut st = tr.init.clone();
    let mut sc = Scene::new(tr);
    let mut refs: Vec<Fb> = (0..=ring).map(|_| Fb::new(tr.w, tr.h)).collect();
    render_full(&mut refs[0], &st);
    let mut lanes: Vec<Lane> = forms
        .into_iter()
        .map(|mut form| {
            form.reset(tr, &sc, ring);
            form.full(0, &sc);
            let mut surfs: Vec<Fb> = (0..ring).map(|_| Fb::new(tr.w, tr.h)).collect();
            surfs[0].px.copy_from_slice(&refs[0].px);
            Lane {
                res: FormResult { name: form.name(), ..Default::default() },
                form,
                surfs,
                mask: Mask::new(tr.w, tr.h),
                rects: vec![],
                out: Out::default(),
                tmp: Fb::new(1, 1),
                hw_r: Plane::new(tr.w, tr.h, 5),
                hw_s: Plane::new(tr.w, tr.h, 5),
            }
        })
        .collect();
    let mut oracles = vec![];
    let nframes = tr.frames.len().min(max_frames);
    for n in 1..=nframes {
        let f = &tr.frames[n - 1];
        apply_state(&mut st, f);
        sc.apply(f);
        let ri = n % (ring + 1);
        render_full(&mut refs[ri], &st);
        if with_oracle && n >= ring {
            oracles.push(oracle(&refs[ri], &refs[(n - ring) % (ring + 1)]));
        }
        let rf = &refs[ri];
        for lane in lanes.iter_mut() {
            lane.form.record(f, &sc);
            let surf = n % ring;
            if n < ring {
                lane.surfs[surf].px.copy_from_slice(&rf.px);
                lane.form.full(surf, &sc);
                continue;
            }
            lane.out.clear();
            lane.form.repair(surf, ring as u32, &sc, &mut lane.out);
            // Region mask.
            for r in lane.rects.drain(..) {
                lane.mask.clear_rect(r);
            }
            let rects = &mut lane.rects;
            lane.form.region(&mut |r: R| rects.push(r));
            let mut m = FrameMetrics::default();
            lane.hw_r.clear();
            lane.hw_s.clear();
            for &r in lane.rects.iter() {
                m.region_px += lane.mask.set(r);
                lane.hw_r.mark(r);
            }
            m.t32_r = lane.hw_r.count();
            // Copy-forward blits from the surface presented last.
            if !lane.out.copies.is_empty() {
                let src_i = (n - 1) % ring;
                if src_i == surf {
                    lane.tmp.clone_from(&lane.surfs[surf]);
                } else {
                    lane.tmp.clone_from(&lane.surfs[src_i]);
                }
                let fb = &mut lane.surfs[surf];
                for &(d, dx, dy) in &lane.out.copies {
                    m.copy_px += d.area() as u64;
                    for y in d.y0..d.y1 {
                        let (dr, sr) = (y as usize * fb.w, (y - dy) as usize * fb.w);
                        let (x0, x1) = (d.x0 as usize, d.x1 as usize);
                        let sx0 = (d.x0 - dx) as usize;
                        fb.px[dr + x0..dr + x1].copy_from_slice(&lane.tmp.px[sr + sx0..sr + sx0 + (x1 - x0)]);
                    }
                }
            }
            let fb = &mut lane.surfs[surf];
            for &r in lane.rects.iter() {
                fb.fill(r, BG);
            }
            let mut start = 0usize;
            for &(scissor, end) in &lane.out.batches {
                m.batches += 1;
                for &s in &lane.out.list[start..end as usize] {
                    m.prims += 1;
                    let c = st[s as usize].cov.inter(scissor);
                    if !c.is_empty() {
                        lane.hw_s.mark(c);
                        let (tx0, ty0, tx1, ty1) = (c.x0 >> 5, c.y0 >> 5, ((c.x1 - 1) >> 5) + 1, ((c.y1 - 1) >> 5) + 1);
                        for ty in ty0..ty1 {
                            m.inst_c += runs_in(&lane.hw_r, ty, tx0, tx1);
                        }
                    }
                    m.frags += draw(fb, &st[s as usize], scissor, Some(&lane.mask));
                }
                start = end as usize;
            }
            m.t32_s = lane.hw_s.count();
            if fb.px != rf.px {
                lane.res.fails += 1;
                if lane.res.first_fail.is_none() {
                    let i = fb.px.iter().zip(&rf.px).position(|(a, b)| a != b).unwrap();
                    let bad = fb.px.iter().zip(&rf.px).filter(|(a, b)| a != b).count();
                    lane.res.first_fail = Some(format!(
                        "{} ring {} frame {}: {} px differ, first at ({}, {}), in region: {}",
                        tr.name,
                        ring,
                        n,
                        bad,
                        i % fb.w,
                        i / fb.w,
                        lane.mask.m[i]
                    ));
                }
                // Resynchronise so later frames are checked independently.
                fb.px.copy_from_slice(&rf.px);
            }
            lane.res.frames.push(m);
        }
    }
    let res = lanes
        .into_iter()
        .map(|l| FormResult { mem: l.form.mem_bytes(), ..l.res })
        .collect();
    (res, oracles)
}
