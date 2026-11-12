//! The windowed heavy adapter (zgx comparisons contract): `ZGX_MODE` (idle|stream|scroll|both),
//! `ZGX_SECONDS` (30), `ZGX_FONT_FILE`, `ZGX_PARITY`; one 1280x800 pt window titled "bough heavy";
//! the workload advances once per `PERIOD` by the wall clock; the run ends with
//! `{"framework":"bough","scene":"heavy","ticks":N}` on stdout.
use bough::backend::run_window;
use bough_heavy::*;
use std::cell::Cell;
use std::rc::Rc;
use zgx_workload::heavy::*;
use zgx_workload::Mode;

fn main() {
    let work = Heavy::from_env();
    let (mode, seconds) = (work.mode, work.seconds);
    let ticks = Rc::new(Cell::new(0u64));
    let dash = Dash::new(&work);
    let mut work = work;
    let period = (mode != Mode::Idle).then(Heavy::period);
    let report_ticks = ticks.clone();
    run_window(
        "bough heavy",
        config,
        dash,
        move |app| {
            if !Heavy::turn() {
                return;
            }
            work.tick();
            ticks.set(work.frames);
            app.turn(|d| publish(d, &work, mode));
        },
        period,
        seconds,
        move || println!("{{\"framework\":\"bough\",\"scene\":\"heavy\",\"ticks\":{}}}", report_ticks.get()),
    )
}
