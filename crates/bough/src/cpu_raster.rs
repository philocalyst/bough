//! CPU tile repair (lane memory, round 6, card `2026-11-11-repair-backends`): the fused tile kernel's arithmetic
//! run on the CPU, bit for bit, for the kinds it models, writing the damaged tiles of the target IOSurface
//! directly. A frame whose damage is small never touches the GPU, so a window that only ever has small frames never
//! makes the Metal driver allocate its ~69 MiB per-queue block (bough-metal `examples/drvprobe.rs`).
//!
//! # Exactness
//! The model is shader.metal's `fs_bin` + `fs_tile` for untransformed records, operation by operation, in IEEE
//! binary16 / binary32 (`comparisons/bough-awagy/examples/cpu_tiles.rs` established it tile by tile against the
//! GPU's full redraw, adversarial scenes included):
//! * colours: `float(u8) * f32(1 / 255)` rounded to half; premultiplied with a half multiply;
//! * a glyph's coverage: `half(texel / 255)`; its source is `pm * cov` (half multiplies);
//! * a rounded box: `half(clamp(0.5 - f_sd))` with `f_sd` in f32; a border: `pm * h1 + pm2 * h2`;
//! * **the blend `sh + acc * (1 - sh.a)` is ONE half FMA**: the MSL compiler contracts it (fast math). Computed
//!   with two roundings, ~96 % of adversarial tiles differ by one LSB (the negative control);
//! * the store: `v * 255` in f32, rounded to nearest even.
//!
//! Every pixel of a damaged tile is recomputed from zero, as the tile pass does. Records of other kinds (images,
//! gradients, shadows), transformed pages or spaces, and anything out of range refuse the CPU path for the whole
//! frame before a byte is written.
//!
//! # Every kind (`all_kinds`)
//! Where the CPU executor is the only executor (no GPU presenter: Linux today, `App::target`), there is no seam to
//! avoid and nothing to be exact against, so it models every kind the tile pass draws: images (rotated, bilinear),
//! gradients (linear and radial ramps), box shadows (the closed-form blur), and records in transformed spaces
//! (`f_shade_x` / `f_shade_t`, the inverse affine per pixel), operation by operation as shader.metal writes them,
//! in f32 / f16 with the platform's `sin`, `cos`, `exp` and `sqrt`. Kinds 0-3 untransformed keep the exact paths
//! above. The kernel and this model are to become one source (compute lane, round 7: wgsl-rs kernels compiled
//! natively); until then this is the CPU twin of `f_shade_x`.
use crate::rt::Rt;
use bough_core::Plane;

/// Payload bits (bough-metal `fused::PAY_RUN` / `PAY_IRUN`).
const PAY_RUN: u32 = 0x8000_0000;
const PAY_IRUN: u32 = 0x4000_0000;

/// One staged entry (`FRec`) plus its pixel bounding box.
#[derive(Clone, Copy, Default)]
struct Ent {
    q: [f32; 4],
    b: [f32; 4],
    core: [f32; 4],
    pm: [f16; 4],
    pm2: [f16; 4],
    cv: [f16; 4],
    uv: [u16; 2],
    kind: u8,
    radius: f32,
    bw: f32,
    /// Gradient: radial (`uv[2]` of the instance non-zero) and its centre as fractions of the box.
    pad: u16,
    gc: [f32; 2],
    /// Transformed space: `u = xk + M^-1 (c - xt)`; `xs` the transform's scale, 0 = untransformed.
    xi: [f32; 4],
    xk: [f32; 2],
    xt: [f32; 2],
    xs: f32,
    /// Integer pixel box `[x0, y0, x1, y1)` holding every pixel centre inside `q`.
    px: [i32; 4],
}

/// Counters.
#[derive(Clone, Copy, Debug, Default)]
pub struct CpuStats {
    pub frames: u64,
    pub tiles: u64,
    pub entries: u64,
    /// Frames the model refused (an unmodelled kind, a transform): drawn by the GPU.
    pub refused: u64,
}

pub struct CpuRaster {
    pos: Vec<u32>,
    ents: Vec<Ent>,
    tile_ents: Vec<u32>,
    plane: Box<Plane>,
    /// One tile's accumulators: 32 x 32 pixels x RGBA halves.
    acc: Box<[f16; 32 * 32 * 4]>,
    /// `half(t / 255)` for every texel value.
    cov: [f16; 256],
    pub stats: CpuStats,
    /// The blend with two roundings instead of one FMA: the gates' sabotage (feature `sabotage_blend`).
    two_roundings: bool,
    /// Model every kind and transformed spaces (see the module docs): set where no GPU executor shares the
    /// surfaces. Off, those refuse the frame (the GPU draws it).
    pub all_kinds: bool,
}

impl Default for CpuRaster {
    fn default() -> Self {
        Self::new()
    }
}

#[inline(always)]
fn unpack(c: u32) -> [f16; 4] {
    let k = 1.0f32 / 255.0;
    [(c & 0xff) as f32 * k, (c >> 8 & 0xff) as f32 * k, (c >> 16 & 0xff) as f32 * k, (c >> 24) as f32 * k].map(|v| v as f16)
}

#[inline(always)]
fn premul(c: u32) -> [f16; 4] {
    let c1 = unpack(c);
    [c1[0] * c1[3], c1[1] * c1[3], c1[2] * c1[3], c1[3]]
}

/// `f_sd`: signed distance from `c` to the rounded box `b` of radius `r`, as shader.metal writes it: `precise::sqrt`
/// (IEEE) of the dot as `fma(y, y, x * x)`. (The kernel's former `length` was x * a hardware rsqrt that is not
/// correctly rounded: the one operation the CPU could not reproduce; examples/mathprobe.rs.)
#[inline(always)]
fn sd(c: [f32; 2], b: [f32; 4], r: f32) -> f32 {
    let q = [(c[0] - b[2]).max(b[0] - c[0]) + r, (c[1] - b[3]).max(b[1] - c[1]) + r];
    let (mx, my) = (q[0].max(0.0), q[1].max(0.0));
    my.mul_add(my, mx * mx).sqrt() + q[0].max(q[1]).min(0.0) - r
}

/// The integer pixel span `[x0, x1)` whose centres `x + 0.5` lie in `[lo, hi)`.
#[inline(always)]
fn span(lo: f32, hi: f32) -> (i32, i32) {
    ((lo - 0.5).ceil() as i32, (hi - 0.5).ceil() as i32)
}

impl CpuRaster {
    pub fn new() -> CpuRaster {
        let mut cov = [0.0 as f16; 256];
        for (t, c) in cov.iter_mut().enumerate() {
            *c = (t as f32 / 255.0) as f16;
        }
        CpuRaster {
            pos: Vec::new(),
            ents: Vec::new(),
            tile_ents: Vec::new(),
            plane: Box::new(Plane::EMPTY),
            acc: Box::new([0.0; 32 * 32 * 4]),
            cov,
            stats: CpuStats::default(),
            two_roundings: cfg!(feature = "sabotage_blend"),
            all_kinds: false,
        }
    }

    fn push(&mut self, e: Ent) {
        let (x0, x1) = span(e.q[0], e.q[2]);
        let (y0, y1) = span(e.q[1], e.q[3]);
        if x0 < x1 && y0 < y1 {
            self.ents.push(Ent { px: [x0, y0, x1, y1], ..e });
        }
    }

    /// Stage every entry of every record touching a damaged tile (`fs_bin` + the tile pass's staging), in paint
    /// order. `false`: something the model does not cover; nothing was written.
    fn stage(&mut self, rt: &Rt, rows: &[u128], full: bool, w: i32, h: i32) -> bool {
        let pl = &mut *self.plane;
        let ty = (h as usize).div_ceil(32);
        let all = if w >= 128 * 32 { u128::MAX } else { (1u128 << (w as u32).div_ceil(32)) - 1 };
        pl.rows = [0; bough_core::damage::MAX_TILES_Y];
        pl.n = ty as u32;
        let (mut y0, mut y1) = (u32::MAX, 0u32);
        for y in 0..ty {
            let r = if full { all } else { rows.get(y).copied().unwrap_or(0) & all };
            pl.rows[y] = r;
            if r != 0 {
                y0 = y0.min(y as u32);
                y1 = y as u32 + 1;
            }
        }
        pl.y0 = y0;
        pl.y1 = y1;
        pl.solid = false;
        self.ents.clear();
        if y0 >= y1 {
            return true;
        }
        let bbox = pl.px_bbox(rt.bough.window());
        rt.bough.cull_tiles_pos(pl, bbox, &mut self.pos);
        let pos = std::mem::take(&mut self.pos);
        let ok = pos.iter().all(|&p| self.stage_record(rt, p as usize));
        self.pos = pos;
        ok
    }

    /// A space's offset and, if it is transformed, its affine `(M, K, t)` (`f_xf`: `M = dm + I`, `K` = offset + pivot).
    /// `None`: out of range, or transformed while the model is limited to the exact kinds.
    fn space(&self, rt: &Rt, s: u16) -> Option<([f32; 2], Option<([f32; 4], [f32; 2], [f32; 2])>)> {
        let xf = rt.xforms.get(s as usize)?;
        let o = rt.glyphs.spaces.get(s as usize)?;
        let off = [o[0] as f32, o[1] as f32];
        if xf.iter().all(|&v| v == 0.0) {
            return Some((off, None));
        }
        if !self.all_kinds {
            return None;
        }
        let m = [xf[0] + 1.0, xf[1], xf[2], xf[3] + 1.0];
        let k = [off[0] + xf[4], off[1] + xf[5]];
        // `f_xf`: an identity matrix with a zero translation is untransformed (only the pivot was written)
        let tr = [xf[6], xf[7]];
        if xf[..4].iter().all(|&v| v == 0.0) && tr == [0.0, 0.0] {
            return Some((off, None));
        }
        Some((off, Some((m, k, tr))))
    }

    /// `f_stage_xf`: the transformed box's bounding box (grown by a pixel) as the coverage window, and the inverse.
    fn stage_xf(e: &mut Ent, (m, k, t): ([f32; 4], [f32; 2], [f32; 2]), qa: &mut [f32; 2], qb: &mut [f32; 2]) {
        let mul = |v: [f32; 2]| [m[0] * v[0] + m[1] * v[1], m[2] * v[0] + m[3] * v[1]];
        let pt = |u: [f32; 2]| {
            let r = mul([u[0] - k[0], u[1] - k[1]]);
            [k[0] + t[0] + r[0], k[1] + t[1] + r[1]]
        };
        let ps = [pt(*qa), pt([qb[0], qa[1]]), pt([qa[0], qb[1]]), pt(*qb)];
        let lo = [ps.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min) - 1.0, ps.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min) - 1.0];
        let hi = [ps.iter().map(|p| p[0]).fold(f32::NEG_INFINITY, f32::max) + 1.0, ps.iter().map(|p| p[1]).fold(f32::NEG_INFINITY, f32::max) + 1.0];
        (*qa, *qb) = (lo, hi);
        let det = m[0] * m[3] - m[1] * m[2];
        let id = if det.abs() > 1e-12 { 1.0 / det } else { 0.0 };
        e.xi = [m[3] * id, -m[1] * id, -m[2] * id, m[0] * id];
        e.xk = k;
        e.xt = [k[0] + t[0], k[1] + t[1]];
        e.xs = det.abs().sqrt().max(1e-6);
    }

    fn stage_record(&mut self, rt: &Rt, pos: usize) -> bool {
        // a transformed page's records: the core's cull already met their transformed covers; their instances carry
        // the space's affine, which is what the kernel shades with
        if !self.all_kinds && rt.bough.xform_at(pos).is_some() {
            return false;
        }
        let r = rt.bough.record(pos);
        let [cx0, cy0, cx1, cy1] = rt.bough.clip_rect(r.clip).to_xyxy();
        let cl = [cx0 as f32, cy0 as f32, cx1 as f32, cy1 as f32];
        let p = r.payload;
        if p & PAY_RUN != 0 {
            let (r0, rn) = (p & 0xFFFFF, ((p >> 20) & 0x7FF) + 1);
            for run in r0..r0 + rn {
                let Some(rh) = rt.glyphs.runs.get(run as usize).copied() else { return false };
                let Some((off, xf)) = self.space(rt, rh.space) else { return false };
                let Some(toks) = rt.text.cache.tok_strike(rh.strike as usize) else { return false };
                let pm = premul(rh.color);
                for slot in rh.base..rh.base + rh.len {
                    let (Some(&x), Some(&e)) = (rt.glyphs.x.get(slot as usize), rt.glyphs.ent.get(slot as usize)) else { return false };
                    let en = &toks.entry[(e & 255) as usize];
                    if en.uv[2] == 0 {
                        continue;
                    }
                    let dx = x.wrapping_sub(rh.x0);
                    let px = (dx as f32).mul_add(rh.scale, rh.origin[0] + 0.5).floor() + en.geo[0];
                    let p0 = [px + off[0], rh.origin[1] + en.geo[1] + off[1]];
                    let size = [en.geo[2], en.geo[3]];
                    let mut e = Ent { b: [p0[0], p0[1], p0[0] + size[0], p0[1] + size[1]], pm, cv: pm, uv: [en.uv[0], en.uv[1]], kind: 3, ..Ent::default() };
                    let (mut qa, mut qb) = ([p0[0] - 1.0, p0[1] - 1.0], [p0[0] - 1.0 + (size[0] + 2.0), p0[1] - 1.0 + (size[1] + 2.0)]);
                    if let Some(x) = xf {
                        Self::stage_xf(&mut e, x, &mut qa, &mut qb);
                    }
                    let qa = [qa[0].clamp(cl[0], cl[2]), qa[1].clamp(cl[1], cl[3])];
                    let qb = [qb[0].clamp(cl[0], cl[2]), qb[1].clamp(cl[1], cl[3])];
                    if qa[0] < qb[0] && qa[1] < qb[1] {
                        self.push(Ent { q: [qa[0], qa[1], qb[0], qb[1]], ..e });
                    }
                }
            }
            return true;
        }
        let (i0, n) = if p & PAY_IRUN != 0 { (p & 0x3FFFF, ((p >> 18) & 0xFFF) + 1) } else { (p, 1) };
        for i in i0..i0 + n {
            let Some(q) = rt.inst.get(i as usize) else { return false };
            if q.kind > 3 && !(self.all_kinds && q.kind <= 6) {
                return false;
            }
            let Some((off, xf)) = self.space(rt, q.space) else { return false };
            let p0 = [q.pos[0] + off[0], q.pos[1] + off[1]];
            let p1 = [p0[0] + q.size[0], p0[1] + q.size[1]];
            let m = if q.kind == 0 {
                0.0
            } else if q.flags != 0 {
                q.flags as f32
            } else {
                1.0
            };
            let mut e = Ent::default();
            let (mut qa, mut qb) = ([p0[0] - m, p0[1] - m], [p1[0] + m, p1[1] + m]);
            if let Some(x) = xf {
                Self::stage_xf(&mut e, x, &mut qa, &mut qb);
            }
            let qa = [qa[0].clamp(cl[0], cl[2]), qa[1].clamp(cl[1], cl[3])];
            let qb = [qb[0].clamp(cl[0], cl[2]), qb[1].clamp(cl[1], cl[3])];
            if !(qa[0] < qb[0] && qa[1] < qb[1]) {
                continue;
            }
            let (pm, pm2) = (premul(q.color), premul(q.color2));
            // CSS clamps a corner radius to the box; an image's word is its angle
            let rad = if q.kind == 4 { q.radius } else { q.radius.min(0.5 * q.size[0].min(q.size[1])) };
            let k = if q.kind == 1 { rad.max(0.5) + 1.0 } else { rad.max(0.5 + q.bw) + 1.0 };
            let core = if xf.is_some() {
                [0.0; 4]
            } else {
                match q.kind {
                    0 => [qa[0], qa[1], qb[0], qb[1]],
                    1 | 2 => [(p0[0] + k).max(qa[0]), (p0[1] + k).max(qa[1]), (p1[0] - k).min(qb[0]), (p1[1] - k).min(qb[1])],
                    _ => [0.0; 4],
                }
            };
            let cv = if q.kind == 2 { pm2 } else { pm };
            self.push(Ent {
                q: [qa[0], qa[1], qb[0], qb[1]],
                b: [p0[0], p0[1], p1[0], p1[1]],
                core,
                pm,
                pm2,
                cv,
                uv: [q.uv[0], q.uv[1]],
                kind: q.kind,
                radius: rad,
                bw: q.bw,
                pad: q.uv[2],
                gc: [f32::from_bits(q.color), f32::from_bits(q.color2)],
                ..e
            });
        }
        true
    }

    /// The last frame's staged entries whose pixel box holds `(x, y)`, described (diagnostics for a gate failure).
    pub fn entries_at(&self, x: i32, y: i32) -> Vec<String> {
        self.ents
            .iter()
            .filter(|e| x >= e.px[0] && x < e.px[2] && y >= e.px[1] && y < e.px[3])
            .map(|e| format!("kind {} q {:?} b {:?} core {:?} pm {:?} pm2 {:?} r {} bw {}", e.kind, e.q, e.b, e.core, e.pm, e.pm2, e.radius, e.bw))
            .collect()
    }

    /// Repair the damaged tiles (`rows`, or every tile when `full`) of a `w` x `h` px surface at `dst` (BGRA8,
    /// `bpr` bytes per row) from the runtime's tables. `None` (nothing written): the frame holds something the model
    /// does not cover. Returns the tiles written.
    ///
    /// # Safety
    /// `dst` must be valid for writes of `h` rows of `bpr` bytes (`bpr >= 4 w`), and nothing else may access it.
    pub unsafe fn repair(&mut self, rt: &Rt, rows: &[u128], full: bool, dst: *mut u8, bpr: usize) -> Option<u32> {
        let (w, h) = (rt.win_w, rt.win_h);
        if !self.stage(rt, rows, full, w, h) {
            self.stats.refused += 1;
            return None;
        }
        let mut tiles = 0u32;
        for ty in self.plane.y0..self.plane.y1 {
            let mut bits = self.plane.rows[ty as usize];
            while bits != 0 {
                let tx = bits.trailing_zeros();
                bits &= bits - 1;
                // SAFETY: the caller's contract.
                unsafe { self.tile(rt, [tx as i32 * 32, ty as i32 * 32], w, h, dst, bpr) };
                tiles += 1;
            }
        }
        self.stats.frames += 1;
        self.stats.tiles += tiles as u64;
        Some(tiles)
    }

    /// One tile: the entries that touch it, in paint order, over zeroed accumulators; then the store.
    unsafe fn tile(&mut self, rt: &Rt, o: [i32; 2], w: i32, h: i32, dst: *mut u8, bpr: usize) {
        self.tile_ents.clear();
        for (k, e) in self.ents.iter().enumerate() {
            if e.px[0] < o[0] + 32 && e.px[2] > o[0] && e.px[1] < o[1] + 32 && e.px[3] > o[1] {
                self.tile_ents.push(k as u32);
            }
        }
        self.stats.entries += self.tile_ents.len() as u64;
        self.acc.fill(0.0);
        let atlas = &rt.text.cache.atlas;
        for &k in &self.tile_ents {
            let e = self.ents[k as usize];
            let (y0, y1) = ((e.px[1] - o[1]).max(0), (e.px[3] - o[1]).min(32));
            let (x0, x1) = ((e.px[0] - o[0]).max(0), (e.px[2] - o[0]).min(32));
            for y in y0..y1 {
                let row = &mut self.acc[(y * 32 * 4) as usize..((y + 1) * 32 * 4) as usize];
                let cy = (o[1] + y) as f32 + 0.5;
                if e.xs != 0.0 || e.kind > 3 {
                    // the general shading (`f_shade_x`): images, gradients, shadows, transformed spaces
                    let img = rt.images.pixels();
                    for x in x0..x1 {
                        let sh = shade_x(&e, [(o[0] + x) as f32 + 0.5, cy], atlas, img);
                        blend_px(&mut row[(x * 4) as usize..(x * 4 + 4) as usize], sh, self.two_roundings);
                    }
                    continue;
                }
                match e.kind {
                    0 => blend_span(row, x0, x1, e.cv, self.two_roundings),
                    3 => {
                        let ty = cy - e.b[1];
                        if !(ty >= 0.0 && ty < e.b[3] - e.b[1]) {
                            continue;
                        }
                        let (lx, ly) = (atlas.width - 1, atlas.height - 1);
                        let ay = (e.uv[1] as u32 + ty as u32).min(ly);
                        for x in x0..x1 {
                            let tx = (o[0] + x) as f32 + 0.5 - e.b[0];
                            if !(tx >= 0.0 && tx < e.b[2] - e.b[0]) {
                                continue;
                            }
                            let ax = (e.uv[0] as u32 + tx as u32).min(lx);
                            let cov = self.cov[atlas.pixels[(ay * atlas.width + ax) as usize] as usize];
                            let sh = e.pm.map(|v| v * cov);
                            blend_px(&mut row[(x * 4) as usize..(x * 4 + 4) as usize], sh, self.two_roundings);
                        }
                    }
                    _ => {
                        let by = (o[1] + (y & !1)) as f32 + 0.5;
                        let row_core = by >= e.core[1] && by + 1.0 < e.core[3];
                        for x in x0..x1 {
                            let bx = (o[0] + (x & !1)) as f32 + 0.5;
                            let sh = if row_core && bx >= e.core[0] && bx + 1.0 < e.core[2] {
                                e.cv
                            } else {
                                let d = sd([(o[0] + x) as f32 + 0.5, cy], e.b, e.radius);
                                if e.kind == 1 {
                                    let cov = (0.5 - d).clamp(0.0, 1.0) as f16;
                                    e.pm.map(|v| v * cov)
                                } else {
                                    let outer = (0.5 - d).clamp(0.0, 1.0);
                                    let inner = (0.5 - (d + e.bw)).clamp(0.0, 1.0);
                                    let (h1, h2) = ((outer - inner).max(0.0) as f16, inner as f16);
                                    [0, 1, 2, 3].map(|i| e.pm[i].mul_add(h1, e.pm2[i] * h2))
                                }
                            };
                            blend_px(&mut row[(x * 4) as usize..(x * 4 + 4) as usize], sh, self.two_roundings);
                        }
                    }
                }
            }
        }
        // the store: BGRA8 unorm, v * 255 in f32 rounded to nearest even
        for y in 0..32 {
            let gy = o[1] + y;
            if gy >= h {
                break;
            }
            let xn = (w - o[0]).min(32);
            // SAFETY: row gy < h, pixels [o.x, o.x + xn) < w: inside the caller's surface.
            let out = unsafe { dst.add(gy as usize * bpr + o[0] as usize * 4) };
            store_row(&self.acc[(y * 32 * 4) as usize..(y * 32 * 4 + xn * 4) as usize], out);
        }
    }
}

/// `acc = sh + acc * (1 - sh.a)` over pixels `[x0, x1)` of a row, `sh` constant (a solid run).
#[inline(always)]
fn blend_span(row: &mut [f16], x0: i32, x1: i32, sh: [f16; 4], two: bool) {
    #[cfg(target_arch = "aarch64")]
    let k: f16 = 1.0 - sh[3];
    #[cfg(target_arch = "aarch64")]
    if !two {
        use core::arch::aarch64::*;
        // SAFETY: NEON (always present on aarch64) with FEAT_FP16 (every Apple core); the span lies in `row`
        // (x1 <= 32 = row.len() / 4), one 4-lane half vector per pixel.
        unsafe {
            let s = vld1_f16(sh.as_ptr());
            let kv = vdup_n_f16(k);
            let p = row.as_mut_ptr();
            for x in x0..x1 {
                let a = p.add((x * 4) as usize);
                vst1_f16(a, vfma_f16(s, vld1_f16(a), kv));
            }
        }
        return;
    }
    for x in x0..x1 {
        blend_px(&mut row[(x * 4) as usize..(x * 4 + 4) as usize], sh, two);
    }
}

#[inline(always)]
fn blend_px(px: &mut [f16], sh: [f16; 4], two: bool) {
    let k: f16 = 1.0 - sh[3];
    for i in 0..4 {
        px[i] = if two { sh[i] + px[i] * k } else { px[i].mul_add(k, sh[i]) };
    }
}

/// One row of accumulators to BGRA8 (`v * 255` in f32, nearest even).
#[inline(always)]
fn store_row(acc: &[f16], out: *mut u8) {
    let u = |v: f16| -> u8 { ((v as f32).clamp(0.0, 1.0) * 255.0).round_ties_even() as u8 };
    for (i, px) in acc.chunks_exact(4).enumerate() {
        // SAFETY: the caller sized `out` for `acc.len() / 4` pixels.
        unsafe { (out.add(i * 4) as *mut [u8; 4]).write_unaligned([u(px[2]), u(px[1]), u(px[0]), u(px[3])]) };
    }
}

// ======================================================================== every kind (`all_kinds`): `f_shade_x`

/// `f_sd` without the exactness constraint's name: the rounded-box distance in edge form.
#[inline(always)]
fn sd_box(c: [f32; 2], b: [f32; 4], r: f32) -> f32 {
    sd(c, b, r)
}

/// An RGBA8 texel of the image atlas as floats (the texture unit's unorm conversion), clamped to the atlas.
#[inline(always)]
fn img_texel(img: &[u8], x: i32, y: i32) -> [f32; 4] {
    if img.is_empty() {
        return [0.0; 4];
    }
    let (x, y) = (x.clamp(0, bough_frame::IMG_W as i32 - 1) as usize, y.clamp(0, bough_frame::IMG_H as i32 - 1) as usize);
    let i = (y * bough_frame::IMG_W as usize + x) * 4;
    [img[i] as f32 / 255.0, img[i + 1] as f32 / 255.0, img[i + 2] as f32 / 255.0, img[i + 3] as f32 / 255.0]
}

fn erf2(x: [f32; 2]) -> [f32; 2] {
    x.map(|x| {
        let s = if x > 0.0 { 1.0 } else if x < 0.0 { -1.0 } else { 0.0 };
        let a = x.abs().min(2.828_427_1);
        let mut v = 1.0 + (0.278393 + (0.230389 + 0.078108 * (a * a)) * a) * a;
        v *= v;
        if a >= 2.828_427_1 { s } else { s - s / (v * v) }
    })
}

fn shadow_x(x: f32, y: f32, sigma: f32, corner: f32, hs: [f32; 2]) -> f32 {
    let delta = (hs[1] - corner - y.abs()).min(0.0);
    let curved = hs[0] - corner + (corner * corner - delta * delta).max(0.0).sqrt();
    let k = 0.707_106_78 / sigma;
    let i = erf2([(x - curved) * k, (x + curved) * k]);
    (0.5 + 0.5 * i[1]) - (0.5 + 0.5 * i[0])
}

/// A rounded box's Gaussian shadow (`f_shadow`: the closed form along x, 4 samples along y).
fn shadow(p: [f32; 2], hs: [f32; 2], sigma: f32, corner: f32) -> f32 {
    let (low, high) = (p[1] - hs[1], p[1] + hs[1]);
    let start = (-3.0 * sigma).clamp(low, high);
    let end = (3.0 * sigma).clamp(low, high);
    let step = (end - start) * 0.25;
    let (mut y, mut v) = (start + step * 0.5, 0.0);
    for _ in 0..4 {
        let g = (-(y * y) / (2.0 * sigma * sigma)).exp() / (2.506_628_3 * sigma);
        v += shadow_x(p[0], p[1] - y, sigma, corner, hs) * g * step;
        y += step;
    }
    v
}

/// `f_shade_t`: a record of kinds 0-3 in a transformed space, at `u` (the pixel centre mapped back).
fn shade_t(e: &Ent, u: [f32; 2], atlas: &bough_text::atlas::Atlas) -> [f16; 4] {
    let s = e.xs;
    let mulh = |c: [f16; 4], k: f32| c.map(|v| v * (k as f16));
    match e.kind {
        0 => mulh(e.pm, (0.5 - sd_box(u, e.b, 0.0) * s).clamp(0.0, 1.0)),
        1 => mulh(e.pm, (0.5 - sd_box(u, e.b, e.radius) * s).clamp(0.0, 1.0)),
        2 => {
            let d = sd_box(u, e.b, e.radius);
            let outer = (0.5 - d * s).clamp(0.0, 1.0);
            let inner = (0.5 - (d + e.bw) * s).clamp(0.0, 1.0);
            let (a, b) = (mulh(e.pm, (outer - inner).max(0.0)), mulh(e.pm2, inner));
            [a[0] + b[0], a[1] + b[1], a[2] + b[2], a[3] + b[3]]
        }
        _ => {
            // the glyph's mask, bilinear (texels outside the glyph's rect are empty)
            let sz = [e.b[2] - e.b[0], e.b[3] - e.b[1]];
            let t = [u[0] - e.b[0] - 0.5, u[1] - e.b[1] - 0.5];
            let f0 = [t[0].floor(), t[1].floor()];
            let fr = [t[0] - f0[0], t[1] - f0[1]];
            let (lx, ly) = (atlas.width as i32 - 1, atlas.height as i32 - 1);
            let mut cov = 0.0f32;
            for k in 0..4 {
                let d = [k & 1, k >> 1];
                let q = [f0[0] as i32 + d[0], f0[1] as i32 + d[1]];
                if q[0] < 0 || q[1] < 0 || q[0] as f32 >= sz[0] || q[1] as f32 >= sz[1] {
                    continue;
                }
                let w = (if d[0] == 1 { fr[0] } else { 1.0 - fr[0] }) * (if d[1] == 1 { fr[1] } else { 1.0 - fr[1] });
                let (ax, ay) = ((e.uv[0] as i32 + q[0]).clamp(0, lx), (e.uv[1] as i32 + q[1]).clamp(0, ly));
                cov += atlas.pixels[(ay * atlas.width as i32 + ax) as usize] as f32 / 255.0 * w;
            }
            mulh(e.pm, cov)
        }
    }
}

/// `f_shade_x` for the entries the exact paths do not take: transformed spaces and kinds 4-6.
fn shade_x(e: &Ent, c: [f32; 2], atlas: &bough_text::atlas::Atlas, img: &[u8]) -> [f16; 4] {
    if !(c[0] >= e.q[0] && c[0] < e.q[2] && c[1] >= e.q[1] && c[1] < e.q[3]) {
        return [0.0; 4];
    }
    let mut c = c;
    if e.xs != 0.0 {
        let (dx, dy) = (c[0] - e.xt[0], c[1] - e.xt[1]);
        c = [e.xk[0] + (e.xi[0] * dx + e.xi[1] * dy), e.xk[1] + (e.xi[2] * dx + e.xi[3] * dy)];
        if e.kind < 4 {
            return shade_t(e, c, atlas);
        }
    }
    let hs = [(e.b[2] - e.b[0]) * 0.5, (e.b[3] - e.b[1]) * 0.5];
    let p = [c[0] - (e.b[0] + hs[0]), c[1] - (e.b[1] + hs[1])];
    match e.kind {
        4 => {
            let (cs, sn) = (e.radius.cos(), e.radius.sin());
            let t = [p[0] * cs + p[1] * sn + hs[0] - 0.5, -p[0] * sn + p[1] * cs + hs[1] - 0.5];
            let sz = [hs[0] * 2.0, hs[1] * 2.0];
            if t[0] <= -1.0 || t[1] <= -1.0 || t[0] >= sz[0] || t[1] >= sz[1] {
                return [0.0; 4];
            }
            let f0 = [t[0].floor(), t[1].floor()];
            let fr = [t[0] - f0[0], t[1] - f0[1]];
            let mut acc = [0.0f32; 4];
            for k in 0..4 {
                let d = [k & 1, k >> 1];
                let q = [f0[0] as i32 + d[0], f0[1] as i32 + d[1]];
                if q[0] < 0 || q[1] < 0 || q[0] >= sz[0] as i32 || q[1] >= sz[1] as i32 {
                    continue;
                }
                let w = (if d[0] == 1 { fr[0] } else { 1.0 - fr[0] }) * (if d[1] == 1 { fr[1] } else { 1.0 - fr[1] });
                let tx = img_texel(img, e.uv[0] as i32 + q[0], e.uv[1] as i32 + q[1]);
                for j in 0..4 {
                    acc[j] += tx[j] * w;
                }
            }
            let a = e.pm[3];
            acc.map(|v| (v as f16) * a)
        }
        5 => {
            let t = if e.pad == 0 {
                let dir = [e.bw.sin(), -e.bw.cos()];
                let len = (2.0 * hs[0] * dir[0]).abs() + (2.0 * hs[1] * dir[1]).abs();
                (p[0] * dir[0] + p[1] * dir[1]) / len.max(1e-6) + 0.5
            } else {
                let g = [(e.gc[0] - 0.5) * 2.0 * hs[0], (e.gc[1] - 0.5) * 2.0 * hs[1]];
                let (dx, dy) = (p[0] - g[0], p[1] - g[1]);
                let (fx, fy) = (g[0].abs() + hs[0], g[1].abs() + hs[1]);
                (dx * dx + dy * dy).sqrt() / (fx * fx + fy * fy).sqrt().max(1e-6)
            };
            let x = t.clamp(0.0, 1.0) * 255.0;
            let i = x.floor().min(254.0);
            let f = x - i;
            let (c0, c1) = (img_texel(img, e.uv[0] as i32 + i as i32, e.uv[1] as i32), img_texel(img, e.uv[0] as i32 + i as i32 + 1, e.uv[1] as i32));
            let cov = (0.5 - sd_box(c, e.b, e.radius) * if e.xs != 0.0 { e.xs } else { 1.0 }).clamp(0.0, 1.0);
            [0, 1, 2, 3].map(|j| ((c0[j] + (c1[j] - c0[j]) * f) * cov) as f16)
        }
        _ => {
            let v = shadow(p, hs, e.bw.max(1e-3), e.radius).clamp(0.0, 1.0) as f16;
            e.pm.map(|c| c * v)
        }
    }
}
