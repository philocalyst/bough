//! The windowed lab (zgx `component_workload`): `ZGX_MODE` (idle|stream|scroll|both), `ZGX_SECONDS`
//! (30), `ZGX_INITIAL_TICKS`; one 960x720 pt window; the workload advances once per `PERIOD` by the
//! wall clock; ends with `{"framework":"bough","scene":"lab","ticks":N}`.
use bough::backend::run_window;
use bough_scenes::*;
use std::cell::Cell;
use std::rc::Rc;
use zgx_workload::{Mode, Workload, PERIOD};

fn main() {
    let work = Workload::from_env();
    let (mode, seconds) = (work.mode, work.seconds);
    let lab = Lab::new(&work);
    let mut work = work;
    let ticks = Rc::new(Cell::new(0u64));
    let report_ticks = ticks.clone();
    // BOUGH_BURST=<s>: stop ticking after that many seconds (the window stays open): a burst of activity
    // followed by calm, to watch the footprint come back down (calm trim + the driver letting go)
    let burst: Option<f64> = std::env::var("BOUGH_BURST").ok().and_then(|v| v.parse().ok());
    let t0 = std::time::Instant::now();
    run_window(
        "bough lab",
        lab_config,
        lab,
        move |app| {
            if burst.is_some_and(|b| t0.elapsed().as_secs_f64() > b) {
                return;
            }
            work.tick();
            ticks.set(work.frames);
            app.turn(|l| lab_publish(l, &work, mode));
        },
        (mode != Mode::Idle).then_some(PERIOD),
        seconds,
        move || println!("{{\"framework\":\"bough\",\"scene\":\"lab\",\"ticks\":{}}}", report_ticks.get()),
    )
}
