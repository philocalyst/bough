//! The windowed animation scene: dots + shimmer redrawn on every display refresh at the frame's time.
//! `ZGX_SECONDS` (30); ends with `animation_frames=N fps=F` on stderr.
use bough::backend::run_window_linked;
use bough_scenes::*;
use std::cell::Cell;
use std::rc::Rc;

fn main() {
    let seconds = zgx_workload::Workload::from_env().seconds;
    let frames = Rc::new(Cell::new(0u64));
    let report_frames = frames.clone();
    run_window_linked(
        "bough animation",
        anim_config,
        Anim { clock: bough::signal::Signal::new(0.0) },
        move |app, t| {
            frames.set(frames.get() + 1);
            if frames.get() == 1 {
                // animation no input drives: the pre-armed command stream (BOUGH_STREAM=K frames, 0 = off)
                let k = std::env::var("BOUGH_STREAM").ok().and_then(|v| v.parse().ok()).unwrap_or(8);
                if k > 0 {
                    app.stream(k);
                }
            }
            app.turn(|a| {
                a.clock.set(t as f32);
            });
        },
        seconds,
        move || {
            let n = report_frames.get();
            eprintln!("animation_frames={n} fps={:.1}", n as f64 / seconds.max(1e-9));
        },
    )
}
