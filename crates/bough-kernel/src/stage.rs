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
    Cmd { win, core, b: Vec4 { x: b[0], y: b[1], z: b[2], w: b[3] }, par: Vec4 { x: kind, y: sh, z: aux[0], w: aux[1] }, rad: Vec4 { x: rad[0], y: rad[1], z: rad[2], w: rad[3] }, pm, pm2 }
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
    Some(cmd(k::K_GLYPH, 0, win, EMPTY, [p0[0], p0[1], p1[0], p1[1]], [0, 0, gw as i32, gh as i32], [texel0, 0], k::premul(color), k::splat(0)))
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

/// A command list as words: what the GPU's buffer holds and what the CPU executors decode.
#[derive(Default, Clone)]
pub struct Scene {
    pub words: Vec<u32>,
}

fn pack(c: &Cmd, flags: u32, time: u32) -> [u32; WORDS] {
    let p2 = |a: i32, b: i32| (a as u32 & 0xffff) | (b as u32) << 16;
    let u2 = |a: u32, b: u32| a | b << 16;
    [
        c.par.x | c.par.y << 8 | flags,
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
    ]
}

impl Scene {
    pub fn len(&self) -> usize {
        self.words.len() / WORDS
    }
    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }
    pub fn clear(&mut self) {
        self.words.clear();
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
        let mut a = a;
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
    /// Slot `i` as stored.
    #[inline]
    pub fn load(&self, i: u32) -> Cmd {
        let w = i as usize * WORDS;
        k::decode(self.w4(w), self.w4(w + 4), self.w4(w + 8), self.w4(w + 12), self.w4(w + 16))
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
