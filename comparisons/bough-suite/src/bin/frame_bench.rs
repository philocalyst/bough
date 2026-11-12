//! zgx-gpu `examples/frame_bench.rs`, native: per-frame CPU cost of animating a few nodes inside a larger
//! static scene (zgx's Motion page, `bough_suite::Motion`), 1180 x 780 points at scale 2 on the GPU.
//! Three modes as zgx: paint-only (15 dot opacities + 23 one-character text colours), layout inside fixed
//! slots (5 cells resized inside 16 x 16 slots), layout up to the root (the FOLD column's height: the 60
//! static cards below it move). 30 warm-up frames, then `FRAMES` (600) measured; per frame, as zgx:
//!
//! * update: the signal writes (`App::write`; zgx: `Signal::set`, its effects run there);
//! * flush: settling the wires and layout (`App::flush`; zgx: `scene.flush()`, layout + damage);
//! * render: damage -> tile list -> GPU encode + commit (`App::frame`; zgx: `renderer.render`).
//!
//! The GPU's execution is not in any span: before each frame the previous one is waited for (`gpu wait`,
//! reported apart; zgx's render begins with that wait, `finish_direct`, so its render span holds it).
//! Instructions are the process's
//! (`proc_pid_rusage`) over the three spans. `RENDER=0` stops each frame at the tile list (no presenter);
//! `ONLY=<substring>` runs the modes whose name contains it; `ROWS` static rows (60); `BOUGH_RING=1` draws
//! into one offscreen surface (zgx's one target) instead of the default round-robin ring of 4; `DIAG=1`
//! prints the layout path's instruction breakdown per frame (its profiler's snaps inflate the times).
//! Before the modes, one headless probe mount reads the geometry the page's FOLD clip and shimmer spaces
//! need (`motion_geom`).
use bough::raw::*;
use bough_suite::*;
use std::time::{Duration, Instant};

struct Res {
    records: usize,
    tiles: f64,
    tiles_all: f64,
    gpu_frames: u64,
    update: Duration,
    flush: Duration,
    render: Duration,
    wait: Duration,
    instrs: u64,
}

fn run(mode: MotionMode, frames: usize, rows: usize, geom: MotionGeom, gpu: bool) -> Res {
    let mut cfg = motion_config();
    // offscreen surfaces: 0 = the default round-robin ring of 4 (every repair at age 4); 1 = one target, as zgx
    cfg.ring = env_usize("BOUGH_RING", 0);
    let mut a = App::new(Motion::new(rows, geom), cfg, gpu);
    a.frame();
    // warm caches (layout, glyph atlas) before timing, as zgx
    for f in 0..30 {
        a.write(|v| motion_step(v, f, mode));
        a.flush();
        a.frame();
    }
    let ovh = bough::sys::perf::snap_overhead();
    let mut r = Res { records: a.rt.bough.len(), tiles: 0.0, tiles_all: (a.rt.tiles_x * a.rt.tiles_y) as f64, gpu_frames: 0, update: Duration::ZERO, flush: Duration::ZERO, render: Duration::ZERO, wait: Duration::ZERO, instrs: 0 };
    let g0 = a.surf_stats.gpu;
    let diag = std::env::var_os("DIAG").is_some();
    let st0 = a.rt.lay.stats;
    if diag {
        a.rt.lay.prof.enable();
    }
    for f in 0..frames {
        let w0 = Instant::now();
        if gpu {
            // the previous frame's GPU work (zgx's spans do not include the GPU's execution either)
            a.bins_overflowed();
        }
        r.wait += w0.elapsed();
        let i0 = instructions();
        let t0 = Instant::now();
        a.write(|v| motion_step(v, 30 + f, mode));
        let t1 = Instant::now();
        a.flush();
        let t2 = Instant::now();
        let drew = a.frame();
        let t3 = Instant::now();
        let i1 = instructions();
        r.update += t1 - t0;
        r.flush += t2 - t1;
        r.render += t3 - t2;
        r.instrs += (i1 - i0).saturating_sub(ovh);
        if drew {
            r.tiles += a.repaired_tiles as f64;
        }
    }
    r.gpu_frames = a.surf_stats.gpu - g0;
    if diag {
        // where the flush goes (instructions per frame; the profiler's own snaps are subtracted)
        let n = frames as f64;
        let (st, p) = (a.rt.lay.stats, &a.rt.lay.prof.acc);
        let parts: Vec<String> = bough::layout::LP_NAMES.iter().zip(p).filter(|(_, v)| **v > 0).map(|(k, v)| format!("{k} {:.1}k", *v as f64 / n / 1e3)).collect();
        eprintln!(
            "  layout per frame: {} | computes {:.2}, nodes visited {:.1}, written {:.1}, texts placed {:.1}",
            parts.join(", "),
            (st.computes - st0.computes) as f64 / n,
            (st.nodes_visited - st0.nodes_visited) as f64 / n,
            (st.nodes_written - st0.nodes_written) as f64 / n,
            (st.text_placed - st0.text_placed) as f64 / n
        );
    }
    r.tiles /= frames as f64;
    r
}

fn main() {
    let frames = env_usize("FRAMES", 600);
    let rows = env_usize("ROWS", 60);
    let gpu = std::env::var("RENDER").map_or(true, |v| v != "0");
    let only = std::env::var("ONLY").ok();
    let geom = motion_geom(rows);
    let per = |d: Duration| d.as_secs_f64() * 1e6 / frames as f64;
    println!("bough frame_bench: {frames} frames, {rows} static rows, {}", if gpu { "RENDER=1 (GPU)" } else { "RENDER=0 (tile list)" });
    for (name, mode) in MOTION_MODES {
        if only.as_deref().is_some_and(|o| !name.contains(o)) {
            continue;
        }
        let r = run(mode, frames, rows, geom, gpu);
        println!(
            "{name:<36} {:>5} records  damage {:>5.1}% ({:>6.1} tiles)  {:>4} frames drawn  update {:>5.1}  flush {:>6.1}  render {:>6.1}  total {:>6.1} µs/frame  {:>7.1} k instr/frame  (gpu wait {:.1} µs)",
            r.records,
            r.tiles / r.tiles_all * 100.0,
            r.tiles,
            r.gpu_frames,
            per(r.update),
            per(r.flush),
            per(r.render),
            per(r.update + r.flush + r.render),
            r.instrs as f64 / frames as f64 / 1e3,
            per(r.wait),
        );
    }
}
