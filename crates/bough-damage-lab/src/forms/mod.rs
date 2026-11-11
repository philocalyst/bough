//! Damage formulations. Each consumes write-time events (`record`) and, per
//! presented frame, produces a repair region and a paint-ordered draw list
//! (`repair`). `region` re-emits the last repair region as rects for the
//! verifier (untimed).

use crate::geom::{N, R};
use crate::scene::Scene;
use crate::trace::{Frame, GLYPH, PState, ROUND, SOLID, TINT, Trace};

pub mod bands;
pub mod bytemap;
pub mod hw;
pub mod basic;
pub mod plane;
pub mod registry;
pub mod sig;
pub mod stamps;
pub mod tiles;

pub use plane::Plane;

/// A repair: the draw list in paint order, split into batches each drawn
/// under its own scissor (one batch for most formulations; zgx draws one per
/// region). `copies` are copy-forward blits from the surface presented last
/// (`src = dst - (dx, dy)`), applied before the region is cleared and redrawn.
#[derive(Default)]
pub struct Out {
    pub list: Vec<u32>,
    pub batches: Vec<(R, u32)>,
    pub copies: Vec<(R, i32, i32)>,
    /// Format C instances: (slot, coverage clipped to one run of damaged
    /// hardware tiles in one tile row).
    pub runs: Vec<(u32, R)>,
}

impl Out {
    pub fn clear(&mut self) {
        self.list.clear();
        self.batches.clear();
        self.copies.clear();
        self.runs.clear();
    }
    #[inline]
    pub fn close(&mut self, scissor: R) {
        self.batches.push((scissor, self.list.len() as u32));
    }
}

pub trait Form {
    fn name(&self) -> String;
    fn reset(&mut self, tr: &Trace, sc: &Scene, ring: usize);
    /// Write-time: one frame's events (the scene already holds the new state).
    fn record(&mut self, f: &Frame, sc: &Scene);
    /// Repair surface `surf`, last drawn `age` frames ago.
    fn repair(&mut self, surf: usize, age: u32, sc: &Scene, out: &mut Out);
    /// Surface `surf` was fully redrawn this frame (never drawn before).
    fn full(&mut self, _surf: usize, _sc: &Scene) {}
    /// The last repair's region (verification only).
    fn region(&self, emit: &mut dyn FnMut(R));
    fn mem_bytes(&self) -> usize;
}

/// Exact symmetric-difference support of one slot's change, as rects
/// (see `render` for the pixel model it is exact for).
#[inline(always)]
pub fn exact_slot(o: &PState, n: &PState, emit: &mut impl FnMut(R)) {
    if o == n {
        return;
    }
    let (oc, nc) = (o.cov, n.cov);
    if oc.is_empty() {
        if !nc.is_empty() {
            emit(nc);
        }
        return;
    }
    if nc.is_empty() {
        emit(oc);
        return;
    }
    if o.key != n.key || o.rad != n.rad || o.kind != n.kind {
        emit(oc);
        if nc != oc {
            emit(nc);
        }
        return;
    }
    match o.kind {
        SOLID | TINT => R::xor(oc, nc, emit),
        GLYPH => {
            if o.ink == n.ink {
                R::xor(oc, nc, emit)
            } else {
                emit(oc);
                emit(nc);
            }
        }
        ROUND => {
            R::xor(oc, nc, emit);
            if o.ink != n.ink {
                corners(o, emit);
                corners(n, emit);
            }
        }
        _ => unreachable!(),
    }
}

#[inline(always)]
pub fn corners(s: &PState, emit: &mut impl FnMut(R)) {
    let r = s.rad as i32;
    let i = s.ink;
    if r == 0 {
        return;
    }
    let rx = r.min(i.x1 - i.x0);
    let ry = r.min(i.y1 - i.y0);
    for q in [
        R::new(i.x0, i.y0, i.x0 + rx, i.y0 + ry),
        R::new(i.x1 - rx, i.y0, i.x1, i.y0 + ry),
        R::new(i.x0, i.y1 - ry, i.x0 + rx, i.y1),
        R::new(i.x1 - rx, i.y1 - ry, i.x1, i.y1),
    ] {
        let c = q.inter(s.cov);
        if !c.is_empty() {
            emit(c);
        }
    }
}

/// Bounds support of a frame: per node old ∪ new bounds, per group the
/// moved content's old and new bounds (zgx / Chromium-style).
#[inline(always)]
pub fn bounds_events(f: &Frame, emit: &mut impl FnMut(R)) {
    for g in &f.groups {
        if !g.old_b.is_empty() {
            emit(g.old_b);
        }
        if !g.new_b.is_empty() {
            emit(g.new_b);
        }
    }
    for e in &f.nodes {
        if !e.old_b.is_empty() {
            emit(e.old_b);
        }
        if !e.new_b.is_empty() {
            emit(e.new_b);
        }
    }
}

/// Exact support of a frame (per slot symmetric difference).
#[inline(always)]
pub fn exact_events(f: &Frame, emit: &mut impl FnMut(R)) {
    for g in &f.groups {
        for j in g.at as usize..(g.at + g.n) as usize {
            exact_slot(&f.gold[j], &f.gnew[j], emit);
        }
    }
    for e in &f.nodes {
        for k in e.at as usize..(e.at + e.n) as usize {
            exact_slot(&f.olds[k], &f.news[k], emit);
        }
    }
}

/// Exact support for node events; a scroll group's moved content is treated
/// as one non-uniform primitive (its old and new bounds): moved text and
/// rounded shapes change nearly every pixel they cover, so per-slot
/// exactness buys little there and costs O(moved slots).
#[inline(always)]
pub fn exact_nodes_group_bounds(f: &Frame, emit: &mut impl FnMut(R)) {
    for g in &f.groups {
        if !g.old_b.is_empty() {
            emit(g.old_b);
        }
        if !g.new_b.is_empty() {
            emit(g.new_b);
        }
    }
    for e in &f.nodes {
        for k in e.at as usize..(e.at + e.n) as usize {
            exact_slot(&f.olds[k], &f.news[k], emit);
        }
    }
}

/// Cull: every slot whose coverage meets the rect, in paint order.
#[inline]
pub fn cull_rect(sc: &Scene, r: R, out: &mut Vec<u32>) {
    if r.is_empty() {
        return;
    }
    let q = N::from_r(r);
    for (p, pg) in sc.page.iter().enumerate() {
        if !pg.meets(q) {
            continue;
        }
        let base = p * 64;
        for (k, c) in sc.cov[base..base + 64].iter().enumerate() {
            if c.meets(q) {
                out.push((base + k) as u32);
            }
        }
    }
}

/// Cull against a tile plane: page summary vs the plane's pixel bbox, then
/// slot coverage vs bbox, then the slot's tile span against the bits.
#[inline]
pub fn cull_plane(sc: &Scene, pl: &Plane, out: &mut Vec<u32>) {
    let bb = pl.px_bbox();
    if bb.is_empty() {
        return;
    }
    let q = N::from_r(bb);
    for (p, pg) in sc.page.iter().enumerate() {
        if !pg.meets(q) {
            continue;
        }
        let base = p * 64;
        for (k, c) in sc.cov[base..base + 64].iter().enumerate() {
            if c.meets(q) && pl.test(c.to_r()) {
                out.push((base + k) as u32);
            }
        }
    }
}

/// Format C cull over a plane of <= 128 columns: per slot meeting the plane,
/// one instance per run of set tiles in each tile row it spans, its coverage
/// clipped to the run. Paint order holds within a tile; instances never
/// overlap across tiles of different runs, so the GPU needs no other order.
#[inline]
pub fn cull_plane_runs(sc: &Scene, pl: &Plane, out: &mut Out) {
    debug_assert!(pl.stride <= 2);
    let bb = pl.px_bbox();
    if bb.is_empty() {
        return;
    }
    let sh = pl.sh;
    let q = N::from_r(bb);
    let st = pl.stride;
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
            let (tx0, ty0, tx1, ty1) = (r.x0 >> sh, r.y0 >> sh, ((r.x1 - 1) >> sh) + 1, ((r.y1 - 1) >> sh) + 1);
            let len = (tx1 - tx0) as u32;
            let mask: u128 = if len >= 128 { !0 } else { (1u128 << len) - 1 };
            let slot = (base + k) as u32;
            let mut any = false;
            for ty in ty0..ty1 {
                let w0 = pl.bits[ty as usize * st] as u128;
                let w1 = if st > 1 { pl.bits[ty as usize * st + 1] as u128 } else { 0 };
                let mut m = ((w0 | w1 << 64) >> tx0) & mask;
                while m != 0 {
                    let s = m.trailing_zeros() as i32;
                    let e = (!(m >> s)).trailing_zeros() as i32;
                    let run = R::new((tx0 + s) << sh, ty << sh, (tx0 + s + e) << sh, (ty + 1) << sh);
                    out.runs.push((slot, r.inter(run)));
                    m &= !(((1u128 << e) - 1) << s);
                    any = true;
                }
            }
            if any {
                out.list.push(slot);
            }
        }
    }
}

/// Summed-area table of a plane's set tiles over the plane's bbox: the cull's
/// question "any damaged tile under this rect?" (and "all damaged?") becomes
/// four loads and three adds, independent of the rect's size: the per-row
/// bit loop vanishes (domain change: bits -> prefix counts).
#[derive(Default)]
pub struct Sat {
    pub bb: R,
    pub w: usize,
    pub t: Vec<u16>,
}

impl Sat {
    #[inline]
    pub fn build(&mut self, pl: &Plane) {
        self.bb = pl.bb;
        if pl.bb.is_empty() {
            return;
        }
        let (bw, bh) = ((pl.bb.x1 - pl.bb.x0) as usize, (pl.bb.y1 - pl.bb.y0) as usize);
        self.w = bw + 1;
        let need = (bw + 1) * (bh + 1);
        if self.t.len() < need {
            self.t.resize(need, 0);
        }
        let w = self.w;
        self.t[..w].fill(0);
        let st = pl.stride;
        for y in 0..bh {
            let ty = pl.bb.y0 as usize + y;
            let row = &pl.bits[ty * st..ty * st + st];
            let mut run = 0u16;
            let (prev, cur) = self.t.split_at_mut((y + 1) * w);
            let prev = &prev[y * w..];
            cur[0] = 0;
            for x in 0..bw {
                let tx = pl.bb.x0 as usize + x;
                run += (row[tx >> 6] >> (tx & 63) & 1) as u16;
                cur[x + 1] = prev[x + 1] + run;
            }
        }
    }
    /// Number of set tiles in the tile rect [tx0, tx1) x [ty0, ty1), clamped.
    #[inline(always)]
    pub fn count(&self, tx0: i32, ty0: i32, tx1: i32, ty1: i32) -> i32 {
        let b = self.bb;
        let (x0, y0) = ((tx0.max(b.x0) - b.x0) as usize, (ty0.max(b.y0) - b.y0) as usize);
        let (x1, y1) = ((tx1.min(b.x1) - b.x0).max(0) as usize, (ty1.min(b.y1) - b.y0).max(0) as usize);
        if x0 >= x1 || y0 >= y1 {
            return 0;
        }
        let w = self.w;
        // SAFETY: x0 < x1 <= bw and y0 < y1 <= bh index inside the (bw+1)x(bh+1) table.
        unsafe {
            let t = self.t.as_ptr();
            *t.add(y1 * w + x1) as i32 - *t.add(y0 * w + x1) as i32 - *t.add(y1 * w + x0) as i32
                + *t.add(y0 * w + x0) as i32
        }
    }
}

/// Cull through the SAT (format S: slots only).
#[inline]
pub fn cull_sat(sc: &Scene, pl: &Plane, sat: &mut Sat, out: &mut Vec<u32>) {
    let bb = pl.px_bbox();
    if bb.is_empty() {
        return;
    }
    sat.build(pl);
    let sh = pl.sh;
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
            if sat.count(tx0, ty0, tx1, ty1) > 0 {
                out.push((base + k) as u32);
            }
        }
    }
}

/// Cull through the SAT, format C: a slot whose tile span is entirely damaged
/// is one unclipped instance (touches only damaged tiles); only partially
/// damaged slots are split into per-row runs of damaged tiles.
#[inline]
pub fn cull_sat_runs(sc: &Scene, pl: &Plane, sat: &mut Sat, out: &mut Out) {
    let bb = pl.px_bbox();
    if bb.is_empty() {
        return;
    }
    sat.build(pl);
    let sh = pl.sh;
    let st = pl.stride;
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
            let cnt = sat.count(tx0, ty0, tx1, ty1);
            if cnt == 0 {
                continue;
            }
            let slot = (base + k) as u32;
            out.list.push(slot);
            let r = c.to_r();
            if cnt == (tx1 - tx0) * (ty1 - ty0) {
                out.runs.push((slot, r));
                continue;
            }
            let len = (tx1 - tx0) as u32;
            let mask: u128 = if len >= 128 { !0 } else { (1u128 << len) - 1 };
            for ty in ty0..ty1 {
                let w0 = pl.bits[ty as usize * st] as u128;
                let w1 = if st > 1 { pl.bits[ty as usize * st + 1] as u128 } else { 0 };
                let mut m = ((w0 | w1 << 64) >> tx0) & mask;
                while m != 0 {
                    let s = m.trailing_zeros() as i32;
                    let e = (!(m >> s)).trailing_zeros() as i32;
                    let run = R::new((tx0 + s) << sh, ty << sh, (tx0 + s + e) << sh, (ty + 1) << sh);
                    out.runs.push((slot, r.inter(run)));
                    m &= !(((1u128 << e) - 1) << s);
                }
            }
        }
    }
}
