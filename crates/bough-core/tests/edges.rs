//! `Bough::damage_edges` (the edge-band rule for box geometry changes) against a pixel oracle: for random
//! old/new boxes on the 1/256-px geometry grid, every pixel whose value differs between the old and the new
//! box must lie in a damaged tile. The oracle shades each pixel centre with the fused kernels' arithmetic
//! (solid: the pixel-centre interval test; rounded / border: the edge-form SDF and its coverage), so it
//! checks both the rule and the band widths the façade passes (`bough::rt::edge_band`, mirrored here).
use bough_core::{Bough, NRect};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn f(&mut self, lo: f32, hi: f32) -> f32 {
        let u = (self.next() >> 40) as f32 / (1u64 << 24) as f32;
        snap(lo + (hi - lo) * u)
    }
}
fn snap(v: f32) -> f32 {
    (v * 256.0).round() * (1.0 / 256.0)
}

#[derive(Clone, Copy)]
struct Box_ {
    b: [f32; 4],
    kind: u8,
    r: f32,
    bw: f32,
}

fn sd(c: [f32; 2], b: [f32; 4], r: f32) -> f32 {
    let qx = (c[0] - b[2]).max(b[0] - c[0]) + r;
    let qy = (c[1] - b[3]).max(b[1] - c[1]) + r;
    (qx.max(0.0).hypot(qy.max(0.0))) + qx.max(qy).min(0.0) - r
}

/// The pixel's value as the kernel computes it (two coverage terms for a border).
fn shade(x: Box_, px: i32, py: i32) -> (u32, u32) {
    let c = [px as f32 + 0.5, py as f32 + 0.5];
    let m = if x.kind == 0 { 0.0 } else { 1.0 };
    if !(c[0] >= x.b[0] - m && c[0] < x.b[2] + m && c[1] >= x.b[1] - m && c[1] < x.b[3] + m) {
        return (0, 0);
    }
    match x.kind {
        0 => (1, 0),
        1 => ((0.5 - sd(c, x.b, x.r)).clamp(0.0, 1.0).to_bits(), 0),
        _ => {
            let d = sd(c, x.b, x.r);
            ((0.5 - d).clamp(0.0, 1.0).to_bits(), (0.5 - (d + x.bw)).clamp(0.0, 1.0).to_bits())
        }
    }
}

fn band(x: Box_) -> i16 {
    match x.kind {
        0 => 2,
        1 => (x.r.max(0.5) + 1.0).ceil() as i16 + 2,
        _ => (x.r.max(0.5 + x.bw) + 1.0).ceil() as i16 + 2,
    }
}

fn ink(x: Box_) -> NRect {
    let m = if x.kind == 0 { 0.0 } else { 1.0 };
    NRect::new((x.b[0] - m).floor() as i32, (x.b[1] - m).floor() as i32, (x.b[2] + m).ceil() as i32, (x.b[3] + m).ceil() as i32)
}

#[test]
fn edge_bands_cover_every_changed_pixel() {
    let (w, h) = (256, 256);
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut marked_total = 0u64;
    let mut union_total = 0u64;
    for case in 0..4000 {
        let kind = (case % 3) as u8;
        let r = if kind == 0 { 0.0 } else { rng.f(0.0, 12.0) };
        let bw = if kind == 2 { rng.f(0.5, 4.0) } else { 0.0 };
        let x0 = rng.f(8.0, 120.0);
        let y0 = rng.f(8.0, 120.0);
        let old = Box_ { b: [x0, y0, x0 + rng.f(1.0, 120.0), y0 + rng.f(1.0, 120.0)], kind, r, bw };
        // move a random subset of edges by a random amount (sub-pixel to large), keeping the box valid
        let mut new = old;
        let which = (rng.next() % 15 + 1) as u8;
        let amp = [0.25f32, 1.0, 6.0, 40.0][(rng.next() % 4) as usize];
        for i in 0..4 {
            if which >> i & 1 != 0 {
                new.b[i] = snap(new.b[i] + rng.f(-amp, amp));
            }
        }
        if new.b[2] <= new.b[0] + 0.5 || new.b[3] <= new.b[1] + 0.5 {
            continue;
        }
        let moved = (0..4).fold(0u8, |a, i| a | ((old.b[i] != new.b[i]) as u8) << i);
        let mut bough = Bough::new(w, h);
        bough.damage_edges(ink(old), ink(new), moved, band(old), 0);
        let pl = bough.plane(0);
        for py in 0..h {
            for px in 0..w {
                if shade(old, px, py) != shade(new, px, py) {
                    let (tx, ty) = (px / 32, py / 32);
                    assert!(pl.rows[ty as usize] >> tx & 1 != 0, "case {case}: kind {kind} pixel ({px},{py}) changed outside the damage (moved {moved:04b}, old {:?}, new {:?})", old.b, new.b);
                }
            }
        }
        // how much tighter than old ∪ new
        let mut u = Bough::new(w, h);
        u.damage_rect(ink(old));
        u.damage_rect(ink(new));
        marked_total += pl.rows.iter().map(|r| r.count_ones() as u64).sum::<u64>();
        union_total += u.plane(0).rows.iter().map(|r| r.count_ones() as u64).sum::<u64>();
    }
    assert!(marked_total <= union_total);
    eprintln!("edge bands marked {marked_total} tiles where old ∪ new marks {union_total}");
}
