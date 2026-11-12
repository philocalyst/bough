//! What a frame's repair costs on each backend, by damaged tiles (lane memory, round 6, card
//! `2026-11-11-repair-backends`): the measurements behind the cost model in `App::draw`.
//!
//!   cargo run --release -p bough --example repair_cost
//!
//! A 1024 x 768 px window (scale 1) of 768 cells, one per 32 px tile: a rounded box, a border and a label. A turn
//! changes the colour of the first `k` cells' labels and boxes (the other cells' colours do not change, so only `k`
//! tiles are damaged). Per k and backend, 40 frames, medians of:
//! * instructions of the frame (the process's, so the GPU path's driver threads count) from the turn's write to the
//!   surface being written (CPU) or the GPU frame being done (`wait_presented`);
//! * wall time over the same span.
//!
//! Backends: `cpu` (CPU tiles), `gpu` (a live queue, frames back to back), `gpu-idle2s` (the queue kept, 2 s of
//! nothing before each frame: the GPU powered down; 5 frames).
use bough::app::Repair;
use bough::raw::*;
use std::time::Instant;

const COLS: usize = 32;
const ROWS: usize = 24;

struct Grid {
    c: Signal<u32>,
    k: usize,
}

fn colour(i: usize, c: u32, k: usize) -> Color {
    if i < k { Color(0xff00_0000 | c.wrapping_mul(2_654_435_761).wrapping_add(i as u32 * 40503) & 0x00ff_ffff) } else { Color(0xff40_6080) }
}

impl View for Grid {
    fn view(&self) -> impl Element<Self> + '_ {
        let k = self.k;
        (
            quad(0.0, 0.0, 1024.0, 768.0).bg(rgb(0xf4f4f0)),
            each((0..COLS * ROWS).map(move |i| {
                let (x, y) = ((i % COLS) as f32 * 32.0, (i / COLS) as f32 * 32.0);
                quad(x + 2.0, y + 2.0, 28.0, 28.0).rounded(6.0).border(1.0, rgb(0x203040)).bg(sig!(self.c).map(move |c| colour(i, *c, k)))
            })),
            each((0..COLS * ROWS).map(move |i| {
                let (x, y) = ((i % COLS) as f32 * 32.0, (i / COLS) as f32 * 32.0);
                text(format!("{}", i % 100)).at(x + 6.0, y + 10.0).size(11.0).line_height(14.0).color(rgb(0x101010))
            })),
        )
    }
}

fn instrs() -> u64 {
    bough_sys::perf::snap().instrs
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

fn main() {
    let font = std::fs::read(std::env::var("ZGX_FONT_FILE").unwrap_or_else(|_| "/System/Library/Fonts/Supplemental/Arial.ttf".into())).unwrap();
    println!("{:>5} {:>10} {:>14} {:>12} {:>14} {:>12}", "tiles", "backend", "instr/frame", "us/frame", "instr/tile", "us/tile");
    for k in [1usize, 2, 4, 8, 16, 32, 64, 128, 256, 512, 768] {
        for be in ["cpu", "gpu", "gpu-idle2s"] {
            let mut app = App::new(Grid { c: Signal::new(1), k }, Config::new(1024.0, 768.0, 1.0, font.clone()).once(), true);
            app.repair = if be == "cpu" { Repair::Cpu } else { Repair::Gpu };
            app.frame();
            app.wait_presented();
            let n = if be == "gpu-idle2s" { 5 } else { 40 };
            let (mut is, mut ts) = (Vec::new(), Vec::new());
            for f in 0..n {
                if be == "gpu-idle2s" {
                    std::thread::sleep(std::time::Duration::from_secs(2));
                }
                let (i0, t0) = (instrs(), Instant::now());
                app.turn(|g| *g.c = f as u32 + 2);
                app.frame();
                app.wait_presented();
                is.push((instrs() - i0) as f64);
                ts.push(t0.elapsed().as_secs_f64() * 1e6);
            }
            let tiles = app.repaired_tiles.max(1) as f64;
            let (mi, mt) = (median(is), median(ts));
            println!("{:>5} {:>10} {:>14.0} {:>12.1} {:>14.0} {:>12.2}   (repaired {} tiles; cpu frames {})", k, be, mi, mt, mi / tiles, mt / tiles, app.repaired_tiles, app.surf_stats.cpu);
        }
    }
}
