//! The windowed lab (zgx `component_workload`): `ZGX_MODE` (idle|stream|scroll|both), `ZGX_SECONDS`
//! (30), `ZGX_INITIAL_TICKS`; one 960x720 pt window; the workload advances once per `PERIOD` by the
//! wall clock; ends with `{"framework":"bough","scene":"lab","ticks":N}`.
use bough::backend::host::run_window;
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
    run_window(
        "bough lab",
        lab_config,
        lab,
        move |app| {
            work.tick();
            ticks.set(work.frames);
            app.turn(|l| lab_publish(l, &work, mode));
        },
        (mode != Mode::Idle).then_some(PERIOD),
        seconds,
        move || println!("{{\"framework\":\"bough\",\"scene\":\"lab\",\"ticks\":{}}}", report_ticks.get()),
    )
}
