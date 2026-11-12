//! GPU kernel A/B on the lab's real damage (bough-metal RESULTS Part IV): the lab is ticked like the windowed
//! adapter (scale 2, a ring of 2: age-2 repairs), then the last frame's repair rows and a full redraw are
//! rendered `REPS` times back to back in one command buffer (`Fused::bench_frames`: the GPU saturated, so
//! its clock is steady). `DEBUG=<n>` selects a kernel variant through the header's debug word.
//! Usage: `ZGX_MODE=stream|scroll kernel_bench`.
use bough::raw::*;
use bough_scenes::*;
use zgx_workload::{Mode, Workload};

fn main() {
    let mode = match std::env::var("ZGX_MODE").as_deref() {
        Ok("scroll") => Mode::Scroll,
        Ok("both") => Mode::Both,
        _ => Mode::Stream,
    };
    let reps: usize = std::env::var("REPS").ok().and_then(|v| v.parse().ok()).unwrap_or(200);
    let debugs: Vec<u32> = std::env::var("DEBUG").ok().map(|v| v.split(',').filter_map(|x| x.parse().ok()).collect()).unwrap_or(vec![0]);
    let mut work = Workload::from_env();
    work.mode = mode;
    let mut cfg = lab_config(2.0);
    cfg.ring = 2;
    let mut app = App::new(Lab::new(&work), cfg, true);
    app.frame();
    for _ in 0..120 {
        work.tick();
        app.turn(|l| lab_publish(l, &work, mode));
        app.frame();
    }
    let rows = app.debug_rows().to_vec();
    let tiles: u32 = rows.iter().map(|r| r.count_ones()).sum();
    let n_dir = app.rt.bough.dir_len() as u32;
    let target = app.last_surface;
    let g = app.gpu.as_mut().unwrap();
    for &d in &debugs {
        let mut part: Vec<f64> = (0..5).map(|_| g.bench_frames(target, &rows, false, n_dir, reps, d, &bough_metal::fused::SlotCopy::NONE)).collect();
        let mut full: Vec<f64> = (0..5).map(|_| g.bench_frames(target, &rows, true, n_dir, reps, d, &bough_metal::fused::SlotCopy::NONE)).collect();
        part.sort_by(|a, b| a.partial_cmp(b).unwrap());
        full.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!("{mode:?} debug {d}: repair ({tiles} tiles) {:.4} ms [{:.4}..{:.4}]   full redraw {:.4} ms [{:.4}..{:.4}]", part[2], part[0], part[4], full[2], full[0], full[4]);
    }
    // the scroll copy alone (RESULTS §15): the whole window from the other surface moved 28 px up, no repair
    let (w, h) = (g.w as i32, g.h as i32);
    let copy = bough_metal::fused::SlotCopy { src: (1 - target) as u32, rect: [0, 0, w, h - 28], dx: 0, dy: -28 };
    let mut c: Vec<f64> = (0..5).map(|_| g.bench_frames(target, &[], false, n_dir, reps, 2, &copy)).collect();
    c.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!("copy {w}x{} px (debug 2: no tile pass): {:.4} ms [{:.4}..{:.4}]", h - 28, c[2], c[0], c[4]);
}
