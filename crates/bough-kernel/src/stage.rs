//! Staging: a primitive in window coordinates (floats, as the runtime's instances hold them) to a command's
//! 20 words. Everything after this point is integer. The words are the one representation both executors
//! read (the GPU from a shared buffer, the CPU from the same bytes).
use crate::k::{self, Cmd};
use wgsl_rs::std::{Vec4, Vec4i, Vec4u};

pub const WORDS: usize = k::CMD_WORDS as usize;
/// Word 0 flag (staging only): this slot is the end state of the transition in the slot before it.
pub const F_TO: u32 = 0x20000;

/// A pixel centre this far (Q8) inside a box, beyond its radius, has coverage ONE: half a pixel for the ramp
/// plus 8/256 px for the rounding of an interpolated transition's edges.
const CORE_SLACK: i32 = 128 + 8;

const EMPTY: Vec4i = Vec4 { x: 0, y: 0, z: 0, w: 0 };

#[inline]
pub fn q8(v: f32) -> i32 {
    (v * 256.0).round() as i32
}
/// The first pixel whose centre is at or after `q` (Q8).
#[inline]
fn px(q: i32) -> i32 {
    (q + 127) >> 8
}
/// A clip rectangle (x0, y0, x1, y1) in px.
pub type Clip = [f32; 4];
pub const NO_CLIP: Clip = [-16000.0, -16000.0, 16000.0, 16000.0];

/// Pixels whose centres lie in [a, b) clamped to `clip`; `None` when there are none.
fn window(a: [i32; 2], b: [i32; 2], clip: &Clip) -> Option<Vec4i> {
    let c = clip.map(q8);
    let (x0, y0) = (px(a[0].clamp(c[0], c[2])), px(a[1].clamp(c[1], c[3])));
    let (x1, y1) = (px(b[0].clamp(c[0], c[2])), px(b[1].clamp(c[1], c[3])));
    (x0 < x1 && y0 < y1).then_some(Vec4 { x: x0.max(-32768), y: y0.max(-32768), z: x1.min(32767), w: y1.min(32767) })
}
/// Pixels whose centres are at least `kc` inside the box (coverage provably ONE), within `win`.
fn core(p0: [i32; 2], p1: [i32; 2], kc: i32, win: Vec4i) -> Vec4i {
    let x0 = px(p0[0] + kc).max(win.x);
    let y0 = px(p0[1] + kc).max(win.y);
    let x1 = (((p1[0] - kc - 128) >> 8) + 1).min(win.z);
    let y1 = (((p1[1] - kc - 128) >> 8) + 1).min(win.w);
    if x0 < x1 && y0 < y1 { Vec4 { x: x0, y: y0, z: x1, w: y1 } } else { EMPTY }
}
/// The root shift for a corner radius: its legs (radius + the AA margin) must fit 14 bits.
fn shift(r: i32) -> u32 {
    let mut s = 0;
    while (r + 512) >> s > 16383 {
        s += 1;
    }
    s
}
fn cmd(kind: u32, sh: u32, win: Vec4i, core: Vec4i, b: [i32; 4], rad: [i32; 4], aux: [u32; 2], pm: Vec4u, pm2: Vec4u) -> Cmd {
    Cmd { win, core, b: Vec4 { x: b[0], y: b[1], z: b[2], w: b[3] }, par: Vec4 { x: kind, y: sh, z: aux[0], w: aux[1] }, rad: Vec4 { x: rad[0], y: rad[1], z: rad[2], w: rad[3] }, pm, pm2, xi: EMPTY, xk: EMPTY, xs: 0 }
}
fn edges(x: f32, y: f32, w: f32, h: f32) -> ([i32; 2], [i32; 2]) {
    let p0 = [q8(x), q8(y)];
    (p0, [p0[0] + q8(w), p0[1] + q8(h)])
}

/// A solid box: every pixel whose centre it holds takes `color` (no antialiasing, as the runtime's solids).
pub fn solid(x: f32, y: f32, w: f32, h: f32, color: u32, clip: &Clip) -> Option<Cmd> {
    let (p0, p1) = edges(x, y, w, h);
    let win = window(p0, p1, clip)?;
    Some(cmd(k::K_SOLID, 0, win, win, [p0[0], p0[1], p1[0], p1[1]], [0; 4], [0; 2], k::premul(color), k::splat(0)))
}
/// A rounded box.
pub fn rrect(x: f32, y: f32, w: f32, h: f32, radius: f32, color: u32, clip: &Clip) -> Option<Cmd> {
    let (p0, p1) = edges(x, y, w, h);
    let win = window([p0[0] - 256, p0[1] - 256], [p1[0] + 256, p1[1] + 256], clip)?;
    let r = q8(radius).clamp(0, (p1[0] - p0[0]).min(p1[1] - p0[1]) / 2);
    let kc = r.max(128) + CORE_SLACK;
    Some(cmd(k::K_RRECT, shift(r), win, core(p0, p1, kc, win), [p0[0], p0[1], p1[0], p1[1]], [r, 0, 0, 0], [0; 2], k::premul(color), k::splat(0)))
}
/// A rounded box filled with `fill` carrying a border of `bw` px in `color` inside its edge.
pub fn border(x: f32, y: f32, w: f32, h: f32, radius: f32, bw: f32, color: u32, fill: u32, clip: &Clip) -> Option<Cmd> {
    let (p0, p1) = edges(x, y, w, h);
    let win = window([p0[0] - 256, p0[1] - 256], [p1[0] + 256, p1[1] + 256], clip)?;
    let r = q8(radius).clamp(0, (p1[0] - p0[0]).min(p1[1] - p0[1]) / 2);
    let b = q8(bw).max(0);
    let kc = r.max(128 + b) + CORE_SLACK;
    Some(cmd(k::K_BORDER, shift(r), win, core(p0, p1, kc, win), [p0[0], p0[1], p1[0], p1[1]], [r, b, 0, 0], [0; 2], k::premul(color), k::premul(fill)))
}
/// A glyph: `gw` x `gh` coverage texels starting at atlas index `texel0` (row stride = the atlas width).
pub fn glyph(x: f32, y: f32, gw: u32, gh: u32, texel0: u32, color: u32, clip: &Clip) -> Option<Cmd> {
    let p0 = [q8(x), q8(y)];
    let p1 = [p0[0] + gw as i32 * 256, p0[1] + gh as i32 * 256];
    let win = window(p0, p1, clip)?;
    Some(cmd(k::K_GLYPH, 0, win, EMPTY, [p0[0], p0[1], p1[0], p1[1]], [0; 4], [texel0, 0], k::premul(color), k::splat(0)))
}
/// The shadow of a rounded box blurred by `sigma` px (the caller applies offset and spread to the box).
pub fn shadow(x: f32, y: f32, w: f32, h: f32, radius: f32, sigma: f32, color: u32, clip: &Clip) -> Option<Cmd> {
    let (p0, p1) = edges(x, y, w, h);
    let bw = q8(2.0 * sigma).clamp(16, 1 << 15);
    let ext = bw + bw / 2 + 256;
    let win = window([p0[0] - ext, p0[1] - ext], [p1[0] + ext, p1[1] + ext], clip)?;
    let r = q8(radius).clamp(0, (p1[0] - p0[0]).min(p1[1] - p0[1]) / 2);
    // interior: both 1-D profiles saturate and no corner is within reach (2 units for the centre's rounding)
    let cr = core(p0, p1, bw + bw / 2 + r + 2, win);
    Some(cmd(k::K_SHADOW, shift(r), win, cr, [p0[0], p0[1], p1[0], p1[1]], [r, bw, ((1 << 24) + bw - 1) / bw, if r > 0 { ((bw as i64 * 32768) / (8 * r as i64)).min(32768) as i32 } else { 0 }], [0; 2], k::premul(color), k::splat(0)))
}
/// A two-stop linear gradient at `angle` (radians, CSS: 0 = to top, clockwise) in a rounded box.
pub fn grad(x: f32, y: f32, w: f32, h: f32, radius: f32, angle: f32, c0: u32, c1: u32, clip: &Clip) -> Option<Cmd> {
    let (p0, p1) = edges(x, y, w, h);
    let win = window([p0[0] - 256, p0[1] - 256], [p1[0] + 256, p1[1] + 256], clip)?;
    let r = q8(radius).clamp(0, (p1[0] - p0[0]).min(p1[1] - p0[1]) / 2);
    let (dx, dy) = (angle.sin(), -angle.cos());
    let len = ((w * dx).abs() + (h * dy).abs()).max(1.0);
    let s = (1u32 << 26) as f32 / len;
    Some(cmd(k::K_GRAD, shift(r), win, core(p0, p1, r.max(128) + CORE_SLACK, win), [p0[0], p0[1], p1[0], p1[1]], [r, 0, (dx * s).round() as i32, (dy * s).round() as i32], [0; 2], k::premul(c0), k::premul(c1)))
}
/// A user shader node `id` (`fx::user_shade`) in a rounded box, with two colours for it to use.
pub fn user(x: f32, y: f32, w: f32, h: f32, radius: f32, id: u32, c0: u32, c1: u32, clip: &Clip) -> Option<Cmd> {
    let (p0, p1) = edges(x, y, w, h);
    let win = window([p0[0] - 256, p0[1] - 256], [p1[0] + 256, p1[1] + 256], clip)?;
    let r = q8(radius).clamp(0, (p1[0] - p0[0]).min(p1[1] - p0[1]) / 2);
    Some(cmd(k::K_USER, shift(r), win, core(p0, p1, r.max(128) + CORE_SLACK, win), [p0[0], p0[1], p1[0], p1[1]], [r, 0, 0, 0], [id, 0], k::premul(c0), k::premul(c1)))
}

/// An image: `w` x `h` premultiplied RGBA texels of the image atlas starting at index `texel0` (row stride =
/// the atlas width), drawn in the box at (x, y), rotated by `angle` radians about its centre, times the
/// alpha of `color`.
pub fn image(x: f32, y: f32, w: f32, h: f32, angle: f32, texel0: u32, color: u32, clip: &Clip) -> Option<Cmd> {
    let (p0, p1) = edges(x, y, w, h);
    let win = window([p0[0] - 256, p0[1] - 256], [p1[0] + 256, p1[1] + 256], clip)?;
    let q16 = |v: f32| (v * 65536.0).round() as i32;
    Some(cmd(k::K_IMAGE, 0, win, EMPTY, [p0[0], p0[1], p1[0], p1[1]], [0, 0, q16(angle.cos()), q16(angle.sin())], [texel0, 0], k::premul(color), k::splat(0)))
}
/// Where a ramp gradient runs.
pub enum Ramp {
    /// Along `angle` (radians, CSS: 0 = to top, clockwise), spanning the box.
    Linear(f32),
    /// From a centre given as fractions of the box, to the box's farthest corner.
    Radial(f32, f32),
}
/// A ramp gradient in a rounded box: 256 premultiplied RGBA texels of the image atlas starting at `texel0`.
pub fn ramp(x: f32, y: f32, w: f32, h: f32, radius: f32, dir: Ramp, texel0: u32, clip: &Clip) -> Option<Cmd> {
    let (p0, p1) = edges(x, y, w, h);
    let win = window([p0[0] - 256, p0[1] - 256], [p1[0] + 256, p1[1] + 256], clip)?;
    let r = q8(radius).clamp(0, (p1[0] - p0[0]).min(p1[1] - p0[1]) / 2);
    let (rz, rw, aux) = match dir {
        Ramp::Linear(angle) => {
            let (dx, dy) = (angle.sin(), -angle.cos());
            let len = ((w * dx).abs() + (h * dy).abs()).max(1.0);
            let s = (1u32 << 26) as f32 / len;
            ((dx * s).round() as i32, (dy * s).round() as i32, 0)
        }
        Ramp::Radial(gx, gy) => {
            let (hx, hy) = (w * 0.5, h * 0.5);
            let g = [(gx - 0.5) * 2.0 * hx, (gy - 0.5) * 2.0 * hy];
            let far = ((g[0].abs() + hx).powi(2) + (g[1].abs() + hy).powi(2)).sqrt();
            (q8(g[0]), q8(g[1]), 0x8000_0000 | ((far * 16.0).round() as u32).clamp(1, 16383))
        }
    };
    Some(cmd(k::K_RAMP, shift(r), win, EMPTY, [p0[0], p0[1], p1[0], p1[1]], [r, 0, rz, rw], [texel0, aux], k::splat(0), k::splat(0)))
}

/// An affine transform of a space: p' = k + t + M (p - k), M = (m00, m01, m10, m11).
#[derive(Clone, Copy, Debug)]
pub struct Xf {
    pub m: [f32; 4],
    pub k: [f32; 2],
    pub t: [f32; 2],
}
/// Put a command staged in its own frame (with no clip, or with a clip that lives in that frame) into a
/// transformed space: its window becomes the transformed window's bounding box grown by a pixel and clipped
/// to `clip` (window coordinates); the kernel maps each pixel back through the inverse. A solid becomes a
/// square-cornered box (its edges are no longer on pixel boundaries); no core survives.
pub fn xf(c: Option<Cmd>, x: &Xf, clip: &Clip) -> Option<Cmd> {
    let mut c = c?;
    let pt = |u: [f32; 2]| {
        let d = [u[0] - x.k[0], u[1] - x.k[1]];
        [x.k[0] + x.t[0] + x.m[0] * d[0] + x.m[1] * d[1], x.k[1] + x.t[1] + x.m[2] * d[0] + x.m[3] * d[1]]
    };
    let (a, b) = ([c.win.x as f32, c.win.y as f32], [c.win.z as f32, c.win.w as f32]);
    let ps = [pt(a), pt([b[0], a[1]]), pt([a[0], b[1]]), pt(b)];
    let lo = [ps.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min) - 1.0, ps.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min) - 1.0];
    let hi = [ps.iter().map(|p| p[0]).fold(f32::NEG_INFINITY, f32::max) + 1.0, ps.iter().map(|p| p[1]).fold(f32::NEG_INFINITY, f32::max) + 1.0];
    let lim = |v: f32| v.clamp(-16000.0, 16000.0);
    c.win = window([q8(lim(lo[0])), q8(lim(lo[1]))], [q8(lim(hi[0])), q8(lim(hi[1]))], clip)?;
    c.core = EMPTY;
    let det = x.m[0] * x.m[3] - x.m[1] * x.m[2];
    let id = if det.abs() > 1e-12 { 1.0 / det } else { 0.0 };
    let q16 = |v: f32| ((v * 65536.0).round() as i64).clamp(-(1 << 23) + 1, (1 << 23) - 1) as i32;
    c.xi = Vec4 { x: q16(x.m[3] * id), y: q16(-x.m[1] * id), z: q16(-x.m[2] * id), w: q16(x.m[0] * id) };
    c.xk = Vec4 { x: q8(x.k[0]), y: q8(x.k[1]), z: q8(x.k[0] + x.t[0]), w: q8(x.k[1] + x.t[1]) };
    c.xs = ((det.abs().sqrt() * 256.0).round() as i32).clamp(1, 16384);
    if c.par.x == k::K_SOLID {
        c.par.x = k::K_RRECT;
    }
    Some(c)
}

/// A command list as words: what the GPU's buffer holds and what the CPU executors decode.
#[derive(Default, Clone)]
pub struct Scene {
    pub words: Vec<u32>,
    /// The frame's table of themed operands (two words per entry; `table_color`, `table_scalar`). Commands
    /// staged with a `Tint` read their colours from it when they are loaded; the binner appends it to the
    /// program, which is where the GPU reads it.
    pub table: Vec<u32>,
}

/// Which of a command's colours come from the table (`Scene::push_tinted`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tint {
    /// The first colour is this table entry.
    pub pm: Option<u16>,
    /// The second colour (a border's fill, a gradient's end) is this table entry.
    pub pm2: Option<u16>,
    /// Both colours are scaled by this scalar entry (an opacity step).
    pub opacity: Option<u16>,
}

/// A table entry holding the colour `rgba8` (premultiplied here, once).
pub fn table_color(rgba8: u32) -> [u32; 2] {
    let p = k::premul(rgba8);
    [p.x | p.y << 16, p.z | p.w << 16]
}
/// A table entry holding a Q15 scalar (ONE = 32768).
pub fn table_scalar(q15: u32) -> [u32; 2] {
    [q15.min(k::ONE), 0]
}
/// The table between `from` and `to` at progress `t` (Q15): what a presenter writes each frame of an
/// animated theme change. The kernel's own interpolation, entry by entry.
pub fn table_mix(from: &[u32], to: &[u32], t: u32) -> Vec<u32> {
    let un = |w: &[u32]| Vec4 { x: w[0] & 0xffff, y: w[0] >> 16, z: w[1] & 0xffff, w: w[1] >> 16 };
    from.chunks_exact(2)
        .zip(to.chunks_exact(2))
        .flat_map(|(a, b)| {
            let v = k::mix4u(un(a), un(b), t.min(k::ONE));
            [v.x | v.y << 16, v.z | v.w << 16]
        })
        .collect()
}
/// The damage bloom bit of a table index (a page ORs these for every index its records name; a write to
/// entry i damages the pages whose bloom has `bloom(i)`).
pub fn bloom(index: u16) -> u64 {
    1 << (index & 63)
}

fn pack(c: &Cmd, flags: u32, time: u32) -> [u32; WORDS] {
    let p2 = |a: i32, b: i32| (a as u32 & 0xffff) | (b as u32) << 16;
    let u2 = |a: u32, b: u32| a | b << 16;
    [
        c.par.x | c.par.y << 8 | flags | if c.xs != 0 { k::F_XF } else { 0 },
        c.par.z,
        c.par.w,
        time,
        p2(c.win.x, c.win.y),
        p2(c.win.z, c.win.w),
        p2(c.core.x, c.core.y),
        p2(c.core.z, c.core.w),
        c.b.x as u32,
        c.b.y as u32,
        c.b.z as u32,
        c.b.w as u32,
        c.rad.x as u32,
        c.rad.y as u32,
        c.rad.z as u32,
        c.rad.w as u32,
        u2(c.pm.x, c.pm.y),
        u2(c.pm.z, c.pm.w),
        u2(c.pm2.x, c.pm2.y),
        u2(c.pm2.z, c.pm2.w),
        c.xi.x as u32,
        c.xi.y as u32,
        c.xi.z as u32,
        c.xi.w as u32,
        c.xk.x as u32,
        c.xk.y as u32,
        c.xk.z as u32,
        c.xk.w as u32,
        c.xs as u32,
        0,
        0,
        0,
    ]
}

impl Scene {
    pub fn len(&self) -> usize {
        self.words.len() / WORDS
    }
    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }
    /// Forget the commands (the table stays: it is the theme, not the frame).
    pub fn clear(&mut self) {
        self.words.clear();
    }
    /// Set table entry `i` (growing the table with zero entries as needed).
    pub fn set_entry(&mut self, i: u16, e: [u32; 2]) {
        let at = i as usize * 2;
        if self.table.len() < at + 2 {
            self.table.resize(at + 2, 0);
        }
        self.table[at..at + 2].copy_from_slice(&e);
    }
    #[inline]
    fn entry(&self, i: u32) -> Vec4u {
        // the kernel's clamp: an index past the table reads its last entry; an empty table is one zero entry
        let n = self.table.len() / 2;
        if n == 0 {
            return k::splat(0);
        }
        let at = (i as usize).min(n - 1) * 2;
        k::unpack_u16x4(self.table[at], self.table[at + 1])
    }
    /// One bit per table entry: is its colour opaque (resolved alpha == ONE)? A retained tile program is
    /// valid while the bits of the entries it uses are unchanged (the occlusion cut depends on nothing else
    /// of the table).
    pub fn opacity_bits(&self) -> [u64; 4] {
        let mut b = [0u64; 4];
        for i in 0..(self.table.len() / 2).min(256) {
            if self.table[i * 2 + 1] >> 16 == k::ONE {
                b[i / 64] |= 1 << (i % 64);
            }
        }
        b
    }
    /// The same scene with every themed operand written into its command (no table): what staging would
    /// have produced had the theme been applied to each record. Equal pixels by definition; the gate checks.
    pub fn baked(&self) -> Scene {
        let mut out = Scene { words: self.words.clone(), table: Vec::new() };
        for i in 0..self.len() {
            let w = i * WORDS;
            if self.words[w] & k::F_T_ANY == 0 {
                continue;
            }
            let c = self.load(i as u32);
            out.words[w] &= !k::F_T_ANY;
            out.words[w + 16] = c.pm.x | c.pm.y << 16;
            out.words[w + 17] = c.pm.z | c.pm.w << 16;
            out.words[w + 18] = c.pm2.x | c.pm2.y << 16;
            out.words[w + 19] = c.pm2.z | c.pm2.w << 16;
            out.words[w + 29] = 0;
            out.words[w + 30] = 0;
        }
        out
    }
    /// Append a command whose colours (some of them) are table entries; returns its slot.
    pub fn push_tinted(&mut self, c: Cmd, t: Tint) -> u32 {
        let flags = if t.pm.is_some() { k::F_T_PM } else { 0 } | if t.pm2.is_some() { k::F_T_PM2 } else { 0 } | if t.opacity.is_some() { k::F_T_OP } else { 0 };
        let mut w = pack(&c, flags, 0);
        w[29] = t.pm.unwrap_or(0) as u32 | (t.pm2.unwrap_or(0) as u32) << 16;
        w[30] = t.opacity.unwrap_or(0) as u32;
        self.words.extend_from_slice(&w);
        (self.len() - 1) as u32
    }
    /// Append a command; returns its slot.
    pub fn push(&mut self, c: Cmd) -> u32 {
        self.words.extend_from_slice(&pack(&c, 0, 0));
        (self.len() - 1) as u32
    }
    pub fn add(&mut self, c: Option<Cmd>) {
        if let Some(c) = c {
            self.push(c);
        }
    }
    /// Append a transition from `a` to `b` (same kind) starting at tick `t0`, lasting `dur` ticks (1..65535),
    /// `ease` 0 linear / 1 smooth. Slot i holds `a` with the swept window and the common core, so the tile
    /// program is valid for the whole transition; slot i + 1 holds `b`.
    pub fn push_anim(&mut self, a: Cmd, b: Cmd, t0: u32, dur: u32, ease: u32) -> u32 {
        assert_eq!(a.par.x, b.par.x, "a transition keeps its kind");
        // `k::at` interpolates the box, the radius, the border or blur width and the colours, and keeps
        // `a`'s derived fields (the inverse blur width and corner curve of a shadow, a gradient's
        // direction, a transformed space). Where those differ between the two states the interpolation
        // would not arrive at `b`: such a transition is admitted as its target state, unanimated.
        let reaches = a.rad.z == b.rad.z && a.rad.w == b.rad.w && a.par.z == b.par.z && a.par.w == b.par.w && a.xi == b.xi && a.xk == b.xk && a.xs == b.xs;
        // (one tick long, so its progress is only ever 0 or ONE, where a mix of two equal states is exact)
        let (mut a, dur) = if reaches { (a, dur) } else { (b, 1) };
        a.win = Vec4 { x: a.win.x.min(b.win.x), y: a.win.y.min(b.win.y), z: a.win.z.max(b.win.z), w: a.win.w.max(b.win.w) };
        // A pixel in both cores is in the core of every interpolated box (edges move linearly and the radii
        // interpolate between two values the margin already covers).
        let c = Vec4 { x: a.core.x.max(b.core.x), y: a.core.y.max(b.core.y), z: a.core.z.min(b.core.z), w: a.core.w.min(b.core.w) };
        a.core = if c.x < c.z && c.y < c.w { c } else { EMPTY };
        a.par.y = a.par.y.max(b.par.y);
        self.words.extend_from_slice(&pack(&a, k::F_ANIM, t0));
        self.words.extend_from_slice(&pack(&b, F_TO, dur.clamp(1, 65535) << 2 | (ease & 3)));
        (self.len() - 2) as u32
    }
    #[inline]
    fn w4(&self, at: usize) -> Vec4u {
        let w = &self.words[at..at + 4];
        Vec4 { x: w[0], y: w[1], z: w[2], w: w[3] }
    }
    #[inline]
    pub fn head(&self, i: u32) -> u32 {
        self.words[i as usize * WORDS]
    }
    /// Slot `i` with its themed operands resolved against the table (the CPU's twin of the kernel's `load`:
    /// the same `decode` and `tint`, the same clamp).
    #[inline]
    pub fn load(&self, i: u32) -> Cmd {
        let w = i as usize * WORDS;
        let c = k::decode(self.w4(w), self.w4(w + 4), self.w4(w + 8), self.w4(w + 12), self.w4(w + 16), self.w4(w + 20), self.w4(w + 24), self.words[w + 28]);
        let flags = self.words[w];
        if flags & k::F_T_ANY == 0 {
            return c;
        }
        let ix = self.words[w + 29];
        k::tint(c, flags, self.entry(ix & 0xffff), self.entry(ix >> 16), self.entry(self.words[w + 30] & 0xffff).x)
    }
    /// Command `i` as it is at `now` (the CPU's twin of the kernel's `load_at`: the same `decode` and `timed`
    /// over the same words).
    #[inline]
    pub fn load_at(&self, i: u32, now: u32) -> Cmd {
        let a = self.load(i);
        let w = i as usize * WORDS;
        if self.words[w] & k::F_ANIM == 0 {
            return a;
        }
        k::timed(a, self.load(i + 1), self.w4(w), self.w4(w + WORDS), now)
    }
}
