#![cfg_attr(not(target_os = "macos"), allow(unused_imports, dead_code))]
//! GPU kernel time of the fused tile pass on an SDF-dense scene, for A/B of a kernel change on one binary
//! (`BOUGH_METAL_SRC`): the CPU-repair test's adversarial scene (600 translucent quads, 300 rounded boxes and
//! borders, 61 labels) fully redrawn `REPS` times back to back in one command buffer (`Fused::bench_frames`: the
//! GPU saturated, its clock steady); median of 5. `cargo run --release -p bough --example kernel_ab`
use bough::raw::*;

fn rnd(s: &mut u64) -> u32 {
    *s ^= *s << 13;
    *s ^= *s >> 7;
    *s ^= *s << 17;
    (*s >> 16) as u32
}

struct Stress {
    seed: u64,
    tint: Signal<u32>,
    label: Signal<String>,
}

impl View for Stress {
    fn view(&self) -> impl Element<Self> + '_ {
        let mut s = self.seed;
        let quads: Vec<(f32, f32, f32, f32, u32)> = (0..600)
            .map(|_| {
                let (x, y) = ((rnd(&mut s) % 440) as f32 + (rnd(&mut s) % 4) as f32 * 0.25, (rnd(&mut s) % 300) as f32);
                ((x), y, (rnd(&mut s) % 90 + 1) as f32, (rnd(&mut s) % 60 + 1) as f32, rnd(&mut s) | 0x0100_0000)
            })
            .collect();
        let shapes: Vec<(f32, f32, f32, f32, u32, u32, f32, f32)> = (0..300)
            .map(|_| {
                let (x, y) = ((rnd(&mut s) % 440) as f32 + (rnd(&mut s) % 4) as f32 * 0.25, (rnd(&mut s) % 300) as f32 + (rnd(&mut s) % 4) as f32 * 0.25);
                let (w, h) = ((rnd(&mut s) % 90 + 2) as f32, (rnd(&mut s) % 60 + 2) as f32);
                (x, y, w, h, rnd(&mut s) | 0x0100_0000, rnd(&mut s) | 0x0100_0000, (rnd(&mut s) % 40) as f32 * 0.5, (rnd(&mut s) % 6) as f32 * 0.5)
            })
            .collect();
        let labels: Vec<(f32, f32, f32, u32, String)> = (0..60)
            .map(|i| ((rnd(&mut s) % 400) as f32, (rnd(&mut s) % 280 + 20) as f32, (rnd(&mut s) % 20 + 8) as f32, rnd(&mut s) | 0x0100_0000, format!("Wg{i} quick fox {}", rnd(&mut s) % 1000)))
            .collect();
        (
            each(
                shapes
                    .into_iter()
                    .map(|(x, y, w, h, c, c2, r, bw)| if bw > 0.0 { quad(x, y, w, h).bg(Color(c)).rounded(r).border(bw, Color(c2)) } else { quad(x, y, w, h).bg(Color(c)).rounded(r.max(0.5)) }),
            ),
            each(quads.into_iter().map(|(x, y, w, h, c)| quad(x, y, w, h).bg(Color(c)))),
            quad(100.25, 80.0, 200.0, 90.0).bg(sig!(self.tint).map(|c| Color(*c))),
            each(labels.into_iter().map(|(x, y, sz, c, t)| text(t).at(x, y).size(sz).line_height(sz * 1.25).color(Color(c)))),
            text(sig!(self.label)).at(30.0, 40.0).size(17.0).line_height(21.0).color(Color(0xc0_30_90_e0)),
        )
    }
}

#[cfg(target_os = "macos")]
fn main() {
    let font = std::fs::read(std::env::var("ZGX_FONT_FILE").unwrap_or_else(|_| "/System/Library/Fonts/Supplemental/Arial.ttf".into())).unwrap();
    let reps: usize = std::env::var("REPS").ok().and_then(|v| v.parse().ok()).unwrap_or(400);
    for seed in [3u64, 11] {
        let v = Stress { seed: 0x9e37_79b9 ^ seed * 7919, tint: Signal::new(0x80_40_c0_20), label: Signal::new("first label".into()) };
        let mut app = App::new(v, Config::new(480.0, 320.0, 2.0, font.clone()).once(), true);
        app.frame();
        let _ = app.reference_pixels(); // grows the bins to the scene
        let n_dir = app.rt.bough.dir_len() as u32;
        let target = app.last_surface;
        let g = app.gpu.as_mut().unwrap();
        let mut full: Vec<f64> = (0..5).map(|_| g.bench_frames(target, &[], true, n_dir, reps, 0, &bough_frame::SlotCopy::NONE)).collect();
        full.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!("seed {seed}: full redraw {:.4} ms [{:.4}..{:.4}]", full[2], full[0], full[4]);
    }
}

/// Metal only (the workspace builds on every platform).
#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("kernel_ab: macOS only");
}
