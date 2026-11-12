//! The windowed vlist adapter (zgx comparisons contract, `vlist_workload`): `ZGX_MODE`
//! (scroll|fling|jump|probe), `ZGX_SECONDS` (30), `ZGX_FONT_FILE`; one 720x800 pt window titled
//! "bough vlist"; the scene advances once per `PERIOD` by the wall clock; on close it reports
//! `workload_ticks=N` on stderr (as zgx's adapter does) and the JSON line on stdout.
use bough::backend::run_window;
use bough_vlist::*;
use std::cell::Cell;
use std::rc::Rc;
use zgx_workload::vlist::{self, Mode};

fn main() {
    let mode = Mode::from_env();
    let seconds: f64 = std::env::var("ZGX_SECONDS").ok().and_then(|v| v.parse().ok()).unwrap_or(30.);
    let ticks = Rc::new(Cell::new(0u64));
    let report_ticks = ticks.clone();
    let tick_count = ticks.clone();
    if mode == Mode::Probe {
        // measure each probe row, report, exit (a window is opened as in zgx's, but only briefly)
        let done = Rc::new(Cell::new(false));
        run_window(
            "bough vlist",
            config,
            VList::new(vlist::ROWS),
            move |app| {
                if !done.replace(true) {
                    eprintln!("{}", probe(app));
                    std::process::exit(0);
                }
            },
            Some(zgx_workload::PERIOD),
            seconds,
            || {},
        )
    }
    run_window(
        "bough vlist",
        config,
        VList::new(vlist::ROWS),
        move |app| {
            let t = tick_count.get() + 1;
            tick_count.set(t);
            step(app, mode, t);
        },
        Some(zgx_workload::PERIOD),
        seconds,
        move || {
            eprintln!("workload_ticks={}", report_ticks.get());
            println!("{{\"framework\":\"bough\",\"scene\":\"vlist\",\"ticks\":{}}}", report_ticks.get());
        },
    )
}
