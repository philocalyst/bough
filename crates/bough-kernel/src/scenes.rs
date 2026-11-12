//! Scenes for the gates and the measurements: an adversarial generator (every kind, sub-pixel positions,
//! clips, transitions) and GUI-shaped workloads (a layered design canvas, a spreadsheet grid).
use crate::stage::{self, Clip, NO_CLIP, Scene};

pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    pub fn u(&mut self, n: u32) -> u32 {
        (self.next() >> 33) as u32 % n.max(1)
    }
    pub fn f(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * ((self.next() >> 40) as f32 / (1u64 << 24) as f32)
    }
    /// A position on the 1/8 px grid (adversarial for coverage rounding), sometimes on whole pixels.
    pub fn pos(&mut self, lo: f32, hi: f32) -> f32 {
        let v = self.f(lo, hi);
        if self.u(3) == 0 { v.round() } else { (v * 8.0).round() / 8.0 }
    }
    pub fn color(&mut self, opaque: bool) -> u32 {
        let c = self.next() as u32 & 0x00ff_ffff;
        c | if opaque { 0xff00_0000 } else { (0x20 + self.u(0xd0)) << 24 }
    }
}

pub const CELL_W: u32 = 12;
pub const CELL_H: u32 = 18;
pub const ATLAS_W: u32 = 16 * CELL_W;
pub const ATLAS_CELLS: u32 = 96;

/// A synthetic glyph atlas: `ATLAS_CELLS` cells of `CELL_W` x `CELL_H` coverage texels, each a few soft
/// strokes (edges at every coverage level, empty texels around them, as rasterised glyphs have).
pub fn atlas() -> Vec<u8> {
    let rows = ATLAS_CELLS.div_ceil(16);
    let mut a = vec![0u8; (ATLAS_W * rows * CELL_H) as usize];
    let mut r = Rng(0x9e37_79b9_7f4a_7c15);
    for cell in 0..ATLAS_CELLS {
        let (cx, cy) = (cell % 16 * CELL_W, cell / 16 * CELL_H);
        for _ in 0..3 {
            let (x0, y0) = (r.f(1.5, 9.0), r.f(2.0, 14.0));
            let (dx, dy) = (r.f(-4.0, 4.0), r.f(-6.0, 6.0));
            let wd = r.f(0.6, 1.3);
            for y in 0..CELL_H {
                for x in 0..CELL_W {
                    // distance from the texel centre to the stroke's segment
                    let (px, py) = (x as f32 + 0.5 - x0, y as f32 + 0.5 - y0);
                    let t = ((px * dx + py * dy) / (dx * dx + dy * dy).max(1e-3)).clamp(0.0, 1.0);
                    let d = ((px - t * dx).powi(2) + (py - t * dy).powi(2)).sqrt();
                    let c = ((wd + 0.5 - d).clamp(0.0, 1.0) * 255.0) as u8;
                    let at = ((cy + y) * ATLAS_W + cx + x) as usize;
                    a[at] = a[at].max(c);
                }
            }
        }
    }
    a
}
/// The first texel of atlas cell `i`.
pub fn cell(i: u32) -> u32 {
    let i = i % ATLAS_CELLS;
    i / 16 * CELL_H * ATLAS_W + i % 16 * CELL_W
}

fn any(r: &mut Rng, fw: f32, fh: f32, clip: &Clip) -> Option<crate::Cmd> {
    let (x, y) = (r.pos(-20.0, fw), r.pos(-20.0, fh));
    let (w, h) = (r.pos(1.0, 140.0), r.pos(1.0, 110.0));
    let rad = match r.u(5) {
        0 => 0.0,
        1 => r.f(0.0, 3.0),
        2 => 9999.0,
        // past 62 px the root shift is non-zero
        3 => r.f(60.0, 140.0),
        _ => r.f(2.0, 24.0),
    };
    let (o1, o2) = (r.u(2) == 0, r.u(3) == 0);
    let (c1, c2, ct) = (r.color(o1), r.color(o2), r.color(false));
    let (p, q) = (r.f(0.25, 5.0), r.f(0.3, 14.0));
    let g = r.u(ATLAS_CELLS);
    match r.u(12) {
        0 | 1 => stage::solid(x.round(), y.round(), w.round(), h.round(), c1, clip),
        2 | 3 => stage::rrect(x, y, w, h, rad, c2, clip),
        4 | 5 => stage::border(x, y, w, h, rad, p, c1, c2, clip),
        6 | 7 => stage::glyph(x.round(), y.round(), CELL_W, CELL_H, cell(g), c1, clip),
        8 => stage::shadow(x, y, w, h, if o1 { 0.0 } else { rad.min(40.0) }, q, ct, clip),
        9 => stage::grad(x, y, w, h, rad, q, c1, c2, clip),
        10 => stage::user(x, y, w, h, rad, g & 1, c1, ct, clip),
        _ => stage::rrect(x, y, w * 3.0, h * 3.0, rad, c1 | 0xff00_0000, clip),
    }
}

/// `n` random commands of every kind over a `fw` x `fh` frame; about one in eight is a transition (running,
/// finished or not yet started at ticks 0..200).
pub fn adversarial(seed: u64, fw: u32, fh: u32, n: usize) -> Scene {
    let mut r = Rng(seed.wrapping_mul(0x2545_f491_4f6c_dd1d) | 1);
    let mut s = Scene::default();
    let (fwf, fhf) = (fw as f32, fh as f32);
    s.add(stage::solid(0.0, 0.0, fwf, fhf, 0xff00_0000 | r.next() as u32, &NO_CLIP));
    for _ in 0..n {
        let clip = if r.u(4) == 0 {
            let (x, y) = (r.pos(0.0, fwf * 0.6), r.pos(0.0, fhf * 0.6));
            let (w, h) = (r.pos(8.0, fwf * 0.6), r.pos(8.0, fhf * 0.6));
            [x, y, x + w, y + h]
        } else {
            NO_CLIP
        };
        let Some(a) = any(&mut r, fwf, fhf, &clip) else { continue };
        if r.u(8) != 0 {
            s.push(a);
            continue;
        }
        // a transition: the same kind, moved and recoloured
        let mut b = a;
        let (dx, dy) = (r.u(60) as i32 * 64 - 1920, r.u(60) as i32 * 64 - 1920);
        if a.par.x != crate::k::K_GLYPH && r.u(3) != 0 {
            b.b.x += dx;
            b.b.z += dx + r.u(40) as i32 * 32;
            b.b.y += dy;
            b.b.w += dy;
            let m = 256 + if a.par.x == crate::k::K_SHADOW { a.rad.y * 2 } else { 0 };
            b.win = wgsl_rs::std::Vec4 { x: (b.b.x - m) >> 8, y: (b.b.y - m) >> 8, z: ((b.b.z + m) >> 8) + 1, w: ((b.b.w + m) >> 8) + 1 };
            b.core = wgsl_rs::std::Vec4 { x: 0, y: 0, z: 0, w: 0 };
        }
        let (o1, o2) = (r.u(2) == 0, r.u(2) == 0);
        b.pm = crate::k::premul(r.color(o1));
        b.pm2 = crate::k::premul(r.color(o2));
        let (t0, dur, ease) = (r.u(150), 1 + r.u(120), r.u(2));
        s.push_anim(a, b, t0, dur, ease);
    }
    s
}

/// A design canvas: `nodes` cards scattered in layers over a background. A card is a shadow, an opaque
/// rounded body, a border, a gradient header, and a few lines of glyphs; every fifth card is translucent.
pub fn canvas(seed: u64, fw: u32, fh: u32, nodes: usize) -> Scene {
    let mut r = Rng(seed | 1);
    let mut s = Scene::default();
    let (fwf, fhf) = (fw as f32, fh as f32);
    s.add(stage::solid(0.0, 0.0, fwf, fhf, 0xfff4_f1ee, &NO_CLIP));
    for i in 0..nodes {
        let (w, h) = (r.f(90.0, 320.0).round(), r.f(70.0, 240.0).round());
        let (x, y) = (r.f(-40.0, fwf - 40.0).round(), r.f(-40.0, fhf - 40.0).round());
        let rad = [0.0, 6.0, 10.0, 16.0][r.u(4) as usize];
        let body = if i % 5 == 4 { r.color(false) } else { r.color(true) };
        s.add(stage::shadow(x, y + 4.0, w, h, rad, r.f(3.0, 12.0), 0x5000_0000, &NO_CLIP));
        s.add(stage::rrect(x, y, w, h, rad, body, &NO_CLIP));
        s.add(stage::border(x, y, w, h, rad, 1.0, 0x4000_0000, 0, &NO_CLIP));
        let clip = [x, y, x + w, y + h];
        s.add(stage::grad(x + 1.0, y + 1.0, w - 2.0, 26.0, (rad - 1.0).max(0.0), 1.57, r.color(true), r.color(true), &clip));
        let lines = 2 + r.u(5);
        for l in 0..lines {
            let n = 4 + r.u(18);
            for g in 0..n {
                s.add(stage::glyph(x + 10.0 + g as f32 * 9.0, y + 34.0 + l as f32 * 22.0, CELL_W, CELL_H, cell(r.u(ATLAS_CELLS)), 0xe010_1418, &clip));
            }
        }
    }
    s
}

/// A spreadsheet: `cw` x `ch` px cells with hairlines, an opaque header row and column, optionally a short
/// number in every cell. `hot`: a column drawn highlighted (the hover that has thousands of dependents).
pub fn grid(fw: u32, fh: u32, cw: u32, ch: u32, text: bool, hot: Option<u32>) -> Scene {
    let mut r = Rng(77);
    let mut s = Scene::default();
    let (fwf, fhf) = (fw as f32, fh as f32);
    s.add(stage::solid(0.0, 0.0, fwf, fhf, 0xffff_ffff, &NO_CLIP));
    let (nx, ny) = (fw.div_ceil(cw), fh.div_ceil(ch));
    if let Some(c) = hot {
        s.add(stage::solid((c * cw) as f32, 0.0, cw as f32, fhf, 0x30f0_9040, &NO_CLIP));
    }
    for y in 0..ny {
        s.add(stage::solid(0.0, (y * ch) as f32, fwf, 1.0, 0xffd8_d8d8, &NO_CLIP));
    }
    for x in 0..nx {
        s.add(stage::solid((x * cw) as f32, 0.0, 1.0, fhf, 0xffd8_d8d8, &NO_CLIP));
    }
    if text {
        for y in 0..ny {
            for x in 0..nx {
                let n = 2 + r.u(5);
                let clip = [(x * cw) as f32 + 1.0, (y * ch) as f32 + 1.0, ((x + 1) * cw) as f32, ((y + 1) * ch) as f32];
                for g in 0..n {
                    s.add(stage::glyph((x * cw) as f32 + 6.0 + g as f32 * 9.0, (y * ch) as f32 + 3.0, CELL_W, CELL_H, cell(r.u(10)), 0xff20_2020, &clip));
                }
            }
        }
    }
    s
}
