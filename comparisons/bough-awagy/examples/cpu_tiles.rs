//! Is the fused tile kernel's arithmetic reproducible on the CPU, bit for bit? (lane memory, round 6: a CPU tile
//! rasteriser for small damage would let a mostly idle app never submit GPU work, which is what holds the
//! driver's ~68 MiB, but it only counts if its pixels are the kernels' pixels.)
//!
//!   cpu_tiles [scroll|counter|dashboard]
//!
//! A model of `fs_bin` + `fs_tile` for untransformed solid quads (kind 0), glyph instances (kind 3) and glyph runs
//! (text records), written from shader.metal operation by operation in IEEE binary16 / binary32 (Rust's `f16`,
//! correctly rounded), is run over every 32 x 32 tile of the workload's window and compared byte for byte with the
//! GPU's full redraw of the same state (`App::reference_pixels`). Tiles touched by any other kind (rounded boxes,
//! borders, images, gradients, shadows, transforms) are skipped and counted. The model's two unspecified steps are
//! tried both ways and every combination is reported:
//! * the blend `sh + acc * (1 - sh.a)` in half: two roundings, or fused (one rounding, as an FMA);
//! * the store of a half into a BGRA8 unorm texel: round to nearest even of `v * 255`, or `floor(v * 255 + 0.5)`.
#![feature(f16)]

#[path = "../../../crates/bough/examples/counter.rs"]
#[allow(dead_code)]
mod counter;
#[path = "../src/dashboard.rs"]
#[allow(dead_code)]
mod dashboard;
#[path = "../src/scroll.rs"]
#[allow(dead_code)]
mod scroll;
#[path = "../src/scroll_config.rs"]
mod scroll_config;

use bough::app::{App, Config, View};

/// One staged record (`FRec` in shader.metal, the fields kinds 0 and 3 use).
#[derive(Clone, Copy)]
struct Rec {
    q: [f32; 4],
    b: [f32; 4],
    pm: [f16; 4],
    uv: [u16; 2],
    glyph: bool,
}

fn unpack(c: u32) -> [f16; 4] {
    // float4(as_type<uchar4>(c)) * (1.0 / 255.0), then half4(..)
    let k = 1.0f32 / 255.0;
    [(c & 0xff) as f32 * k, (c >> 8 & 0xff) as f32 * k, (c >> 16 & 0xff) as f32 * k, (c >> 24) as f32 * k].map(|v| v as f16)
}

fn premul(c: u32) -> [f16; 4] {
    let c1 = unpack(c);
    [c1[0] * c1[3], c1[1] * c1[3], c1[2] * c1[3], c1[3]]
}

/// The model of one window: what the kernels read, from the runtime's own tables.
struct Model<'a, V: View> {
    app: &'a App<V>,
    w: i32,
    h: i32,
}

/// Why a tile cannot be modelled.
const SKIP: Option<Vec<Rec>> = None;
/// Payload bits (bough-metal `fused::PAY_RUN` / `PAY_IRUN`).
const PAY_RUN: u32 = 0x8000_0000;
const PAY_IRUN: u32 = 0x4000_0000;

impl<V: View> Model<'_, V> {
    fn space(&self, s: u16) -> Option<[f32; 2]> {
        let xf = self.app.rt.xforms.get(s as usize).copied().unwrap_or([0.0; 8]);
        if xf.iter().any(|&v| v != 0.0) {
            return None;
        }
        let o = self.app.rt.glyphs.spaces.get(s as usize).copied().unwrap_or([0, 0]);
        Some([o[0] as f32, o[1] as f32])
    }

    fn clip(&self, id: u16) -> [f32; 4] {
        let [x0, y0, x1, y1] = self.app.rt.bough.clip_rect(id).to_xyxy();
        [x0 as f32, y0 as f32, x1 as f32, y1 as f32]
    }

    /// The records a tile's pixels depend on, staged in paint order (`None`: a kind the model does not cover).
    fn stage(&self, recs: &[([i32; 4], u16, u32)], tile: [i32; 4]) -> Option<Vec<Rec>> {
        let mut out = Vec::new();
        let [tx0, ty0, tx1, ty1] = tile;
        for &(ink, clip, payload) in recs {
            let [x0, y0, x1, y1] = ink;
            if x0 >= tx1 || y0 >= ty1 || x1 <= tx0 || y1 <= ty0 || x0 >= x1 || y0 >= y1 {
                continue;
            }
            let cl = self.clip(clip);
            let p = payload;
            if p & PAY_RUN != 0 {
                let (r0, rn) = (p & 0xFFFFF, ((p >> 20) & 0x7FF) + 1);
                for run in r0..r0 + rn {
                    let rh = self.app.rt.glyphs.runs[run as usize];
                    let off = self.space(rh.space)?;
                    let toks = self.app.rt.text.cache.tok_strike(rh.strike as usize)?;
                    for slot in rh.base..rh.base + rh.len {
                        let x = self.app.rt.glyphs.x[slot as usize];
                        let e = (self.app.rt.glyphs.ent[slot as usize] & 255) as usize;
                        let en = &toks.entry[e];
                        if en.uv[2] == 0 {
                            continue;
                        }
                        let dx = x.wrapping_sub(rh.x0);
                        let px = (dx as f32).mul_add(rh.scale, rh.origin[0] + 0.5).floor() + en.geo[0];
                        let p0 = [px + off[0], rh.origin[1] + en.geo[1] + off[1]];
                        let size = [en.geo[2], en.geo[3]];
                        let qa = [(p0[0] - 1.0).clamp(cl[0], cl[2]), (p0[1] - 1.0).clamp(cl[1], cl[3])];
                        let qb = [(p0[0] - 1.0 + (size[0] + 2.0)).clamp(cl[0], cl[2]), (p0[1] - 1.0 + (size[1] + 2.0)).clamp(cl[1], cl[3])];
                        if qa[0] < qb[0] && qa[1] < qb[1] {
                            out.push(Rec { q: [qa[0], qa[1], qb[0], qb[1]], b: [p0[0], p0[1], p0[0] + size[0], p0[1] + size[1]], pm: premul(rh.color), uv: [en.uv[0], en.uv[1]], glyph: true });
                        }
                    }
                }
                continue;
            }
            let (i0, n) = if p & PAY_IRUN != 0 { (p & 0x3FFFF, ((p >> 18) & 0xFFF) + 1) } else { (p, 1) };
            for i in i0..i0 + n {
                let q = &self.app.rt.inst[i as usize];
                if q.kind != 0 && q.kind != 3 {
                    return SKIP;
                }
                let off = self.space(q.space)?;
                let p0 = [q.pos[0] + off[0], q.pos[1] + off[1]];
                let p1 = [p0[0] + q.size[0], p0[1] + q.size[1]];
                let m = if q.kind == 0 {
                    0.0
                } else if q.flags != 0 {
                    q.flags as f32
                } else {
                    1.0
                };
                let qa = [(p0[0] - m).clamp(cl[0], cl[2]), (p0[1] - m).clamp(cl[1], cl[3])];
                let qb = [(p1[0] + m).clamp(cl[0], cl[2]), (p1[1] + m).clamp(cl[1], cl[3])];
                if qa[0] < qb[0] && qa[1] < qb[1] {
                    out.push(Rec { q: [qa[0], qa[1], qb[0], qb[1]], b: [p0[0], p0[1], p1[0], p1[1]], pm: premul(q.color), uv: [q.uv[0], q.uv[1]], glyph: q.kind == 3 });
                }
            }
        }
        Some(out)
    }

    /// `f_shade` for kinds 0 and 3 at pixel centre `c`.
    fn shade(&self, r: &Rec, c: [f32; 2]) -> [f16; 4] {
        let z = [0.0 as f16; 4];
        if !(c[0] >= r.q[0] && c[0] < r.q[2] && c[1] >= r.q[1] && c[1] < r.q[3]) {
            return z;
        }
        if !r.glyph {
            return r.pm;
        }
        let t = [c[0] - r.b[0], c[1] - r.b[1]];
        let mut cov: f16 = 0.0;
        if t[0] >= 0.0 && t[1] >= 0.0 && t[0] < r.b[2] - r.b[0] && t[1] < r.b[3] - r.b[1] {
            let a = &self.app.rt.text.cache.atlas;
            let (lx, ly) = (a.width - 1, a.height - 1);
            let x = (r.uv[0] as f32 as u32 + t[0] as u32).min(lx);
            let y = (r.uv[1] as f32 as u32 + t[1] as u32).min(ly);
            let texel = a.pixels[(y * a.width + x) as usize];
            cov = (texel as f32 / 255.0) as f16;
        }
        [r.pm[0] * cov, r.pm[1] * cov, r.pm[2] * cov, r.pm[3] * cov]
    }

    /// The tile's BGRA bytes under one choice of the two unspecified steps.
    fn tile(&self, recs: &[Rec], o: [i32; 2], fused: bool, rtne: bool) -> Vec<u8> {
        let mut px = vec![0u8; 32 * 32 * 4];
        for y in 0..32 {
            for x in 0..32 {
                let c = [(o[0] + x) as f32 + 0.5, (o[1] + y) as f32 + 0.5];
                let mut acc = [0.0 as f16; 4];
                for r in recs {
                    let sh = self.shade(r, c);
                    let k: f16 = 1.0 - sh[3];
                    for i in 0..4 {
                        acc[i] = if fused { (sh[i] as f64 + acc[i] as f64 * k as f64) as f16 } else { sh[i] + acc[i] * k };
                    }
                }
                let u = |v: f16| -> u8 {
                    let v = (v as f32).clamp(0.0, 1.0) * 255.0;
                    if rtne { v.round_ties_even() as u8 } else { (v + 0.5).floor() as u8 }
                };
                let i = ((y * 32 + x) * 4) as usize;
                px[i..i + 4].copy_from_slice(&[u(acc[2]), u(acc[1]), u(acc[0]), u(acc[3])]);
            }
        }
        px
    }
}

/// Compare every tile of the current state; returns (tiles modelled, skipped, mismatches per variant, first
/// mismatch per variant).
fn compare<V: View>(app: &mut App<V>, tag: &str) -> [u64; 4] {
    let gpu = app.reference_pixels();
    let (w, h) = (app.rt.win_w, app.rt.win_h);
    let app: &App<V> = app;
    let m = Model { app, w, h };
    let recs: Vec<([i32; 4], u16, u32)> = app.rt.bough.records().iter().map(|r| (r.ink.to_xyxy(), r.clip, r.payload)).collect();
    let (mut done, mut skipped) = (0u64, 0u64);
    let mut bad = [0u64; 4];
    let mut shown = [false; 4];
    for ty in 0..(m.h + 31) / 32 {
        for tx in 0..(m.w + 31) / 32 {
            let o = [tx * 32, ty * 32];
            let Some(st) = m.stage(&recs, [o[0], o[1], o[0] + 32, o[1] + 32]) else {
                skipped += 1;
                continue;
            };
            done += 1;
            for (v, &(fused, rtne)) in [(false, true), (false, false), (true, true), (true, false)].iter().enumerate() {
                let t = m.tile(&st, o, fused, rtne);
                let mut diff = None;
                'px: for y in 0..32 {
                    for x in 0..32 {
                        let (gx, gy) = (o[0] + x, o[1] + y);
                        if gx >= m.w || gy >= m.h {
                            continue;
                        }
                        let gi = ((gy * m.w + gx) * 4) as usize;
                        let ti = ((y * 32 + x) * 4) as usize;
                        if gpu[gi..gi + 4] != t[ti..ti + 4] {
                            diff = Some((gx, gy, gpu[gi..gi + 4].to_vec(), t[ti..ti + 4].to_vec()));
                            break 'px;
                        }
                    }
                }
                if let Some((x, y, g, c)) = diff {
                    bad[v] += 1;
                    if !shown[v] {
                        shown[v] = true;
                        eprintln!("  {tag} variant {v} (fused {fused}, rtne {rtne}): tile ({tx},{ty}) first differs at ({x},{y}): gpu {g:?} cpu {c:?}");
                    }
                }
            }
        }
    }
    println!("{tag}: tiles modelled {done}, skipped {skipped}; mismatching tiles: sep+rtne {} sep+halfup {} fused+rtne {} fused+halfup {}", bad[0], bad[1], bad[2], bad[3]);
    bad
}

/// An adversarial scene for the model: overlapping translucent quads of random colour and alpha, and random-colour
/// translucent text (glyph runs) over them, so every rounding of the blend and the store is exercised.
struct Stress {
    seed: u64,
}

fn rnd(s: &mut u64) -> u32 {
    *s ^= *s << 13;
    *s ^= *s >> 7;
    *s ^= *s << 17;
    (*s >> 16) as u32
}

impl View for Stress {
    fn view(&self) -> impl bough::raw::Element<Self> + '_ {
        use bough::raw::*;
        let mut s = self.seed;
        let quads: Vec<(f32, f32, f32, f32, u32)> = (0..3000)
            .map(|_| {
                let (x, y) = ((rnd(&mut s) % 960) as f32 + (rnd(&mut s) % 4) as f32 * 0.25, (rnd(&mut s) % 680) as f32);
                let (w, h) = ((rnd(&mut s) % 200 + 1) as f32, (rnd(&mut s) % 120 + 1) as f32);
                (x, y, w, h, rnd(&mut s) | 0x0100_0000)
            })
            .collect();
        let labels: Vec<(f32, f32, f32, u32, String)> = (0..400)
            .map(|i| {
                let (x, y) = ((rnd(&mut s) % 900) as f32, (rnd(&mut s) % 660 + 20) as f32);
                (x, y, (rnd(&mut s) % 30 + 8) as f32, rnd(&mut s) | 0x0100_0000, format!("Wg{i} quick brown fox {}", rnd(&mut s) % 1000))
            })
            .collect();
        (
            each(quads.into_iter().map(|(x, y, w, h, c)| quad(x, y, w, h).bg(Color(c)))),
            each(labels.into_iter().map(|(x, y, sz, c, t)| text(t).at(x, y).size(sz).line_height(sz * 1.25).color(Color(c)))),
        )
    }
}

fn font() -> Vec<u8> {
    let p = std::env::var("BOUGH_FONT").unwrap_or_else(|_| "/System/Library/Fonts/Supplemental/Arial.ttf".into());
    std::fs::read(p).expect("font")
}

fn main() {
    let what = std::env::args().nth(1).unwrap_or_else(|| "scroll".into());
    match what.as_str() {
        "stress" => {
            for seed in [1u64, 2, 3, 4] {
                let mut app = App::new(Stress { seed: 0x9e37_79b9 ^ seed * 7919 }, Config::new(1000.0, 700.0, 2.0, font()).once(), true);
                app.frame();
                compare(&mut app, &format!("stress seed {seed}"));
            }
        }
        "counter" => {
            let mut app = App::new(counter::Counter { count: bough::prelude::Signal::new(0) }, Config::new(480.0, 320.0, 2.0, font()), true);
            app.frame();
            compare(&mut app, "counter");
            for _ in 0..7 {
                app.turn(|c| c.inc());
                app.frame();
            }
            compare(&mut app, "counter +7");
        }
        "dashboard" => {
            let mut app = App::new(dashboard::app(), Config::new(1000.0, 700.0, 2.0, font()), true);
            app.frame();
            compare(&mut app, "dashboard");
            for k in 0..3 {
                app.wheel_points(500.0, 500.0, 173.0);
                app.frame();
                compare(&mut app, &format!("dashboard scrolled {k}"));
            }
        }
        _ => {
            let mut app = App::new(scroll::app(), Config::new(1000.0, 700.0, 2.0, font()), true);
            app.frame();
            compare(&mut app, "scroll");
            for (k, dy) in [37.0f32, 400.5, 1234.0, 9.25].iter().enumerate() {
                app.wheel_points(500.0, 400.0, *dy);
                app.frame();
                compare(&mut app, &format!("scroll step {k} (+{dy} pt)"));
            }
        }
    }
}
