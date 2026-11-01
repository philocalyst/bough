//! `hw32`: damage at the GPU's unit of cost. Apple's TBDR loads/stores only
//! the 32x32 tiles some geometry touches (calib/metal_calib*.swift), so the
//! damage plane is one bit per hardware tile. Domain bent deliberately: at
//! most 128 tile columns and 128 tile rows (4096 x 4096 px), so a tile row is
//! one `u128`, a mark is `row |= colmask` plus a dirty-row bit, a span test is
//! one shift+mask per row, and history OR / clear touch only dirty rows.
//!
//! Support: exact symmetric difference per changed slot for node events; a
//! scroll group's moved content as one non-uniform primitive (old ∪ new
//! bounds). History: ring of 4 planes, OR over the age window.

use super::*;

const K: usize = 4;
const SH: u32 = 5;

#[inline(always)]
fn colmask(tx0: i32, tx1: i32) -> u128 {
    // tiles [tx0, tx1), 0 <= tx0 < tx1 <= 128
    (u128::MAX >> (128 - (tx1 - tx0) as u32)) << tx0 as u32
}

#[derive(Clone)]
pub struct Hw {
    pub rows: Vec<u128>,
    pub dirty: u128,
}

impl Hw {
    pub fn new(h: i32) -> Hw {
        let n = ((h + 31) >> SH) as usize;
        assert!(n <= 128, "hw plane: at most 128 tile rows (4096 px)");
        Hw { rows: vec![0; n], dirty: 0 }
    }
    #[inline(always)]
    pub fn mark(&mut self, r: R) {
        let (tx0, ty0, tx1, ty1) = (r.x0 >> SH, r.y0 >> SH, ((r.x1 - 1) >> SH) + 1, ((r.y1 - 1) >> SH) + 1);
        let w = (tx0 >> 6) as usize;
        if w == ((tx1 - 1) >> 6) as usize {
            let len = (tx1 - tx0) as u32;
            let m = (u64::MAX >> (64 - len)) << (tx0 & 63) as u32;
            // SAFETY: r lies inside the viewport, so ty1 <= rows.len().
            unsafe {
                let base = self.rows.as_mut_ptr() as *mut u64;
                for ty in ty0..ty1 {
                    *base.add(ty as usize * 2 + w) |= m;
                }
            }
            return;
        }
        let m = colmask(tx0, tx1);
        // SAFETY: r lies inside the viewport, so ty1 <= rows.len().
        unsafe {
            for ty in ty0..ty1 {
                *self.rows.get_unchecked_mut(ty as usize) |= m;
            }
        }
    }
    /// Whole-plane clear: <= 128 rows x 16 B, cheaper than tracking dirty
    /// rows on every mark (Joffe move 3: width deletes the fixup).
    #[inline(always)]
    pub fn clear(&mut self) {
        self.rows.fill(0);
        self.dirty = 0;
    }
    #[inline(always)]
    pub fn or_from(&mut self, o: &Hw) {
        for (d, s) in self.rows.iter_mut().zip(&o.rows) {
            *d |= *s;
        }
    }
    /// Tile bbox (exclusive ends) of set tiles: one pass over the rows.
    #[inline]
    pub fn bbox(&self) -> R {
        let (mut cols, mut y0, mut y1) = (0u128, i32::MAX, 0);
        for (y, &r) in self.rows.iter().enumerate() {
            if r != 0 {
                cols |= r;
                y0 = y0.min(y as i32);
                y1 = y as i32 + 1;
            }
        }
        if cols == 0 {
            return R::E;
        }
        R::new(cols.trailing_zeros() as i32, y0, 128 - cols.leading_zeros() as i32, y1)
    }
    #[inline(always)]
    pub fn test(&self, tx0: i32, ty0: i32, tx1: i32, ty1: i32) -> bool {
        let w = (tx0 >> 6) as usize;
        if w == ((tx1 - 1) >> 6) as usize {
            // One 64-bit word: the common case (spans that do not straddle tile 64).
            let len = (tx1 - tx0) as u32;
            let m = (u64::MAX >> (64 - len)) << (tx0 & 63) as u32;
            let mut hit = 0u64;
            // SAFETY: as below; a u128 row is two little-endian u64 words.
            unsafe {
                let base = self.rows.as_ptr() as *const u64;
                for ty in ty0..ty1 {
                    hit |= *base.add(ty as usize * 2 + w) & m;
                }
            }
            return hit != 0;
        }
        let m = colmask(tx0, tx1);
        let mut hit = 0u128;
        // SAFETY: the span comes from a coverage rect inside the viewport.
        unsafe {
            for ty in ty0..ty1 {
                hit |= *self.rows.get_unchecked(ty as usize) & m;
            }
        }
        hit != 0
    }
}

/// Cull: page summary vs bbox, slot coverage vs bbox, then span test.
#[inline]
pub fn cull_hw(sc: &Scene, p: &Hw, bb: R, out: &mut Vec<u32>) {
    if bb.is_empty() {
        return;
    }
    let q = N::from_r(R::new(bb.x0 << SH, bb.y0 << SH, (bb.x1 << SH).min(sc.w), (bb.y1 << SH).min(sc.h)));
    for (pi, pg) in sc.page.iter().enumerate() {
        if !pg.meets(q) {
            continue;
        }
        let base = pi * 64;
        let page = &sc.cov[base..base + 64];
        for k in 0..64 {
            let c = page[k];
            if !c.meets(q) {
                continue;
            }
            let n = c.0;
            let (tx0, ty0) = (n[0] as i32 >> SH, n[1] as i32 >> SH);
            let (tx1, ty1) = ((!(n[2] as i32) >> SH) + 1, (!(n[3] as i32) >> SH) + 1);
            if p.test(tx0, ty0, tx1, ty1) {
                out.push((base + k) as u32);
            }
        }
    }
}

/// NEON cull: 8 slots per step. `vld4q_s16` deinterleaves 8 NRects into
/// x0 / y0 / -x1 / -y1 lanes; the bbox test is two lane-wise `max` and two
/// saturating adds (`x0 + -x1 < 0`, saturation keeps the EMPTY sentinel
/// non-negative); tile spans are lane shifts (`!(-x1) = x1 - 1`). Only lanes
/// that pass go to the scalar span test against the plane.
#[cfg(target_arch = "aarch64")]
#[inline]
pub fn cull_hw_neon(sc: &Scene, p: &Hw, bb: R, out: &mut Vec<u32>) {
    use core::arch::aarch64::*;
    if bb.is_empty() {
        return;
    }
    let qr = R::new(bb.x0 << SH, bb.y0 << SH, (bb.x1 << SH).min(sc.w), (bb.y1 << SH).min(sc.h));
    let q = N::from_r(qr);
    // SAFETY: cov.len() is a multiple of 64 (pages), each N is 4 x i16 laid
    // out contiguously (repr(C)), so every vld4q_s16 reads 8 whole NRects in bounds.
    unsafe {
        let (qx0, qy0, qx1, qy1) = (vdupq_n_s16(q.0[0]), vdupq_n_s16(q.0[1]), vdupq_n_s16(q.0[2]), vdupq_n_s16(q.0[3]));
        let zero = vdupq_n_s16(0);
        let mut tx0 = [0i16; 8];
        let mut ty0 = [0i16; 8];
        let mut tx1 = [0i16; 8];
        let mut ty1 = [0i16; 8];
        let lanebits: [u16; 8] = [1, 2, 4, 8, 16, 32, 64, 128];
        let lb = vld1q_u16(lanebits.as_ptr());
        let base_ptr = sc.cov.as_ptr() as *const i16;
        for (pi, pg) in sc.page.iter().enumerate() {
            if !pg.meets(q) {
                continue;
            }
            for g in 0..8 {
                let slot0 = pi * 64 + g * 8;
                let v = vld4q_s16(base_ptr.add(slot0 * 4));
                let sx = vqaddq_s16(vmaxq_s16(v.0, qx0), vmaxq_s16(v.2, qx1));
                let sy = vqaddq_s16(vmaxq_s16(v.1, qy0), vmaxq_s16(v.3, qy1));
                let hit = vandq_u16(vcltq_s16(sx, zero), vcltq_s16(sy, zero));
                let mut m = vaddvq_u16(vandq_u16(hit, lb)) as u32;
                if m == 0 {
                    continue;
                }
                vst1q_s16(tx0.as_mut_ptr(), vshrq_n_s16::<5>(v.0));
                vst1q_s16(ty0.as_mut_ptr(), vshrq_n_s16::<5>(v.1));
                vst1q_s16(tx1.as_mut_ptr(), vshrq_n_s16::<5>(vmvnq_s16(v.2)));
                vst1q_s16(ty1.as_mut_ptr(), vshrq_n_s16::<5>(vmvnq_s16(v.3)));
                while m != 0 {
                    let l = m.trailing_zeros() as usize;
                    m &= m - 1;
                    if p.test(tx0[l] as i32, ty0[l] as i32, tx1[l] as i32 + 1, ty1[l] as i32 + 1) {
                        out.push((slot0 + l) as u32);
                    }
                }
            }
        }
    }
}

impl Hw {
    /// Format C emission for one slot whose span meets damage: one unclipped
    /// instance if every tile of its span is damaged (the common case: it
    /// touches only damaged tiles), else one clipped instance per run of
    /// damaged tiles in each tile row. Single-u64-word fast path.
    #[inline(always)]
    pub fn emit_runs(&self, slot: u32, r: R, tx0: i32, ty0: i32, tx1: i32, ty1: i32, runs: &mut Vec<(u32, R)>) {
        let w = (tx0 >> 6) as usize;
        if w == ((tx1 - 1) >> 6) as usize {
            let sh0 = (tx0 & 63) as u32;
            let m = (u64::MAX >> (64 - (tx1 - tx0) as u32)) << sh0;
            // SAFETY: span rows lie inside the plane (coverage inside viewport).
            let base = self.rows.as_ptr() as *const u64;
            let mut full = true;
            for ty in ty0..ty1 {
                full &= unsafe { *base.add(ty as usize * 2 + w) } & m == m;
            }
            if full {
                runs.push((slot, r));
                return;
            }
            for ty in ty0..ty1 {
                let mut b = (unsafe { *base.add(ty as usize * 2 + w) } & m) >> sh0;
                while b != 0 {
                    let s = b.trailing_zeros() as i32;
                    let e = (!(b >> s)).trailing_zeros() as i32;
                    let run = R::new((tx0 + s) << SH, ty << SH, (tx0 + s + e) << SH, (ty + 1) << SH);
                    runs.push((slot, r.inter(run)));
                    b &= !((u64::MAX >> (64 - e as u32)) << s as u32);
                }
            }
            return;
        }
        let m = colmask(tx0, tx1);
        let mut full = true;
        for ty in ty0..ty1 {
            full &= self.rows[ty as usize] & m == m;
        }
        if full {
            runs.push((slot, r));
            return;
        }
        for ty in ty0..ty1 {
            let mut b = (self.rows[ty as usize] & m) >> tx0 as u32;
            while b != 0 {
                let s = b.trailing_zeros() as i32;
                let e = (!(b >> s as u32)).trailing_zeros() as i32;
                let run = R::new((tx0 + s) << SH, ty << SH, (tx0 + s + e) << SH, (ty + 1) << SH);
                runs.push((slot, r.inter(run)));
                b &= !colmask(s, s + e);
            }
        }
    }
}

/// NEON cull emitting format C instances.
#[cfg(target_arch = "aarch64")]
#[inline]
pub fn cull_hw_neon_runs(sc: &Scene, p: &Hw, bb: R, out: &mut Out) {
    use core::arch::aarch64::*;
    if bb.is_empty() {
        return;
    }
    let qr = R::new(bb.x0 << SH, bb.y0 << SH, (bb.x1 << SH).min(sc.w), (bb.y1 << SH).min(sc.h));
    let q = N::from_r(qr);
    // SAFETY: as in `cull_hw_neon`.
    unsafe {
        let (qx0, qy0, qx1, qy1) = (vdupq_n_s16(q.0[0]), vdupq_n_s16(q.0[1]), vdupq_n_s16(q.0[2]), vdupq_n_s16(q.0[3]));
        let zero = vdupq_n_s16(0);
        let lanebits: [u16; 8] = [1, 2, 4, 8, 16, 32, 64, 128];
        let lb = vld1q_u16(lanebits.as_ptr());
        let base_ptr = sc.cov.as_ptr() as *const i16;
        for (pi, pg) in sc.page.iter().enumerate() {
            if !pg.meets(q) {
                continue;
            }
            for g in 0..8 {
                let slot0 = pi * 64 + g * 8;
                let v = vld4q_s16(base_ptr.add(slot0 * 4));
                let sx = vqaddq_s16(vmaxq_s16(v.0, qx0), vmaxq_s16(v.2, qx1));
                let sy = vqaddq_s16(vmaxq_s16(v.1, qy0), vmaxq_s16(v.3, qy1));
                let hit = vandq_u16(vcltq_s16(sx, zero), vcltq_s16(sy, zero));
                let mut m = vaddvq_u16(vandq_u16(hit, lb)) as u32;
                while m != 0 {
                    let l = m.trailing_zeros() as usize;
                    m &= m - 1;
                    let c = *sc.cov.get_unchecked(slot0 + l);
                    let n = c.0;
                    let (tx0, ty0) = (n[0] as i32 >> SH, n[1] as i32 >> SH);
                    let (tx1, ty1) = ((!(n[2] as i32) >> SH) + 1, (!(n[3] as i32) >> SH) + 1);
                    let slot = (slot0 + l) as u32;
                    let w = (tx0 >> 6) as usize;
                    if w == ((tx1 - 1) >> 6) as usize {
                        // One pass over the rows: any damaged tile, and all damaged?
                        let wm = (u64::MAX >> (64 - (tx1 - tx0) as u32)) << (tx0 & 63) as u32;
                        let rows = p.rows.as_ptr() as *const u64;
                        let (mut any, mut all) = (0u64, wm);
                        for ty in ty0..ty1 {
                            let b = *rows.add(ty as usize * 2 + w) & wm;
                            any |= b;
                            all &= b;
                        }
                        if any == 0 {
                            continue;
                        }
                        out.list.push(slot);
                        if all == wm {
                            out.runs.push((slot, c.to_r()));
                        } else {
                            p.emit_runs(slot, c.to_r(), tx0, ty0, tx1, ty1, &mut out.runs);
                        }
                    } else if p.test(tx0, ty0, tx1, ty1) {
                        out.list.push(slot);
                        p.emit_runs(slot, c.to_r(), tx0, ty0, tx1, ty1, &mut out.runs);
                    }
                }
            }
        }
    }
}

pub struct HwForm {
    pub runs: bool,
    ring: Vec<Hw>,
    acc: Hw,
    bb: R,
    w: i32,
    h: i32,
    ep: usize,
}

impl HwForm {
    pub fn new() -> Self {
        HwForm { runs: false, ring: vec![], acc: Hw::new(32), bb: R::E, w: 0, h: 0, ep: 0 }
    }
}

impl Default for HwForm {
    fn default() -> Self {
        Self::new()
    }
}

impl Form for HwForm {
    fn name(&self) -> String {
        if self.runs { "hw32c".into() } else { "hw32".into() }
    }
    fn reset(&mut self, _tr: &Trace, sc: &Scene, _ring: usize) {
        assert!(sc.w <= 4096 && sc.h <= 4096);
        self.ring = vec![Hw::new(sc.h); K];
        self.acc = Hw::new(sc.h);
        self.w = sc.w;
        self.h = sc.h;
        self.ep = 0;
    }
    fn record(&mut self, f: &Frame, _sc: &Scene) {
        self.ep += 1;
        let p = &mut self.ring[self.ep % K];
        p.clear();
        exact_nodes_group_bounds(f, &mut |r| p.mark(r));
    }
    fn repair(&mut self, _surf: usize, age: u32, sc: &Scene, out: &mut Out) {
        self.acc.clear();
        for k in 0..age as usize {
            self.acc.or_from(&self.ring[(self.ep - k) % K]);
        }
        self.bb = self.acc.bbox();
        #[cfg(target_arch = "aarch64")]
        if self.runs {
            cull_hw_neon_runs(sc, &self.acc, self.bb, out);
        } else {
            cull_hw_neon(sc, &self.acc, self.bb, &mut out.list);
        }
        #[cfg(not(target_arch = "aarch64"))]
        cull_hw(sc, &self.acc, self.bb, &mut out.list);
        out.close(R::new(self.bb.x0 << SH, self.bb.y0 << SH, (self.bb.x1 << SH).min(self.w), (self.bb.y1 << SH).min(self.h)));
    }
    fn region(&self, emit: &mut dyn FnMut(R)) {
        let vp = R::new(0, 0, self.w, self.h);
        for (y, &row) in self.acc.rows.iter().enumerate() {
            let mut m = row;
            while m != 0 {
                let s = m.trailing_zeros() as i32;
                let e = (!(m >> s)).trailing_zeros() as i32;
                emit(R::new(s << SH, (y as i32) << SH, (s + e) << SH, (y as i32 + 1) << SH).clip(vp));
                m &= !(colmask(s, s + e));
            }
        }
    }
    fn mem_bytes(&self) -> usize {
        (K + 1) * self.acc.rows.len() * 16
    }
}
