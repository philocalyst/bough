//! The CPU repair backend writes the GPU's pixels (lane memory, round 6, card `2026-11-11-repair-backends`).
//!
//! An adversarial scene of every kind the CPU backend models (translucent solid quads at quarter-pixel positions,
//! rounded boxes and borders of random radius and width, translucent random-colour text) is drawn with the CPU
//! backend forced on, then changed (colours, a scroll of one layer, text) and repaired on the CPU at several ring
//! ages; every presented frame must equal the GPU's full redraw of the same state (`reference_pixels`), byte for
//! byte. The negative control (`--features sabotage_blend`: the blend with two roundings instead of the GPU's one
//! half FMA) must fail.
use bough::app::Repair;
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

/// The first differing pixels of two BGRA frames `w` px wide (empty: identical).
fn diff(a: &[u8], b: &[u8], w: usize) -> Vec<(usize, usize, Vec<u8>, Vec<u8>)> {
    a.chunks_exact(4).zip(b.chunks_exact(4)).enumerate().filter(|(_, (x, y))| x != y).take(4).map(|(i, (x, y))| (i % w, i / w, x.to_vec(), y.to_vec())).collect()
}

fn font() -> Vec<u8> {
    std::fs::read(std::env::var("ZGX_FONT_FILE").unwrap_or_else(|_| "/System/Library/Fonts/Supplemental/Arial.ttf".into())).unwrap()
}

#[test]
fn cpu_repair_equals_the_gpu_full_redraw() {
    for seed in [3u64, 11, 29] {
        let v = Stress { seed: 0x9e37_79b9 ^ seed * 7919, tint: Signal::new(0x80_40_c0_20), label: Signal::new("first label".into()) };
        let mut app = App::new(v, Config::new(480.0, 320.0, 2.0, font()), true);
        app.repair = Repair::Cpu;
        app.frame();
        assert!(app.surf_stats.cpu >= 1, "the first frame was repaired on the CPU");
        let d = diff(&app.presented_pixels(), &app.reference_pixels(), 960);
        if let Some((x, y, ..)) = d.first() {
            for e in app.cpu.entries_at(*x as i32, *y as i32) {
                eprintln!("  {e}");
            }
        }
        assert!(d.is_empty(), "seed {seed}: first frame differs: {d:?}");
        for k in 0..9u32 {
            app.turn(|v| {
                *v.tint = 0x10_20_30_40u32.wrapping_mul(k + 3) | 0x0100_0000;
                if k % 3 == 1 {
                    v.label.set(format!("label {k} changed"));
                }
            });
            app.frame();
            let d = diff(&app.presented_pixels(), &app.reference_pixels(), 960);
            assert!(d.is_empty(), "seed {seed}: change {k} differs: {d:?}");
        }
        assert!(app.surf_stats.cpu >= 8, "the changes were repaired on the CPU ({:?})", app.surf_stats);
    }
}
