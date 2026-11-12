//! zgui's `label-churn` probe on bough: thirty labels relabelled every frame. Prints zgui's statistic (median
//! wall time of one relabelled frame: 12 warm-up turns, 48 samples per round, median of 5 round medians) and
//! the instructions per frame. `RENDER=0` (default, zgui's probe draws into a renderer that does nothing):
//! everything up to the GPU's input; `RENDER=1`: plus the offscreen GPU frame. `ARGS`: the label count is
//! fixed at 30 (zgui's default).
use bough::raw::*;
use bough_text_bench::churn::*;
use bough_text_bench::{env, font_bytes, instrs, median};
use std::time::Instant;

fn main() {
    let render = env("RENDER", 0) != 0;
    let mut app = App::new(Plot { zoom: Signal::new(0) }, config(font_bytes()), render);
    app.frame();
    let mut step = 0usize;
    let mut frame = |app: &mut App<Plot>| {
        step += 1;
        let t = Instant::now();
        app.turn(|p| { p.zoom.set(step); });
        app.frame();
        t.elapsed().as_secs_f64() * 1e9
    };
    let i0 = instrs();
    let mut rounds = Vec::new();
    let mut frames = 0u64;
    for _ in 0..5 {
        for _ in 0..12 {
            frame(&mut app);
        }
        let mut samples: Vec<f64> = (0..48).map(|_| frame(&mut app)).collect();
        frames += 60;
        rounds.push(median(&mut samples));
    }
    #[cfg(target_os = "macos")]
    if render {
        app.wait_presented();
    }
    let per = (instrs() - i0) as f64 / frames as f64;
    let mut sorted = rounds.clone();
    println!(
        "label-churn: {LABELS} labels relabelled, {:.1} us/frame median of round medians ({}), {:.0} instructions/frame, render={render}, text sets {}",
        median(&mut sorted) / 1e3,
        rounds.iter().map(|ns| format!("{:.1}", ns / 1e3)).collect::<Vec<_>>().join(" "),
        per,
        app.rt.ctr.text_set,
    );
}
