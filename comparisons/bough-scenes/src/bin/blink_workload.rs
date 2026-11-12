//! The blink scene: a 2x20 pt caret shown/hidden every 530 ms in a still window. `ZGX_SECONDS` (30);
//! ends with `blink_frames=N workload_ticks=0` on stderr.
use bough::backend::run_window;
use bough_scenes::*;
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

fn main() {
    let seconds = zgx_workload::Workload::from_env().seconds;
    let blinks = Rc::new(Cell::new(0u64));
    let report = blinks.clone();
    run_window(
        "bough blink",
        blink_config,
        Blink { shown: bough::signal::Signal::new(true) },
        move |app| {
            blinks.set(blinks.get() + 1);
            app.turn(|b| {
                let s = !*b.shown;
                b.shown.set(s);
            });
        },
        Some(Duration::from_millis(zgx_workload::blink::PHASE_MS)),
        seconds,
        move || eprintln!("blink_frames={} workload_ticks=0", report.get()),
    )
}
