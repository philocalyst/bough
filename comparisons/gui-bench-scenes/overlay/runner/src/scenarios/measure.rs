//! What each scenario measures, in order. Every cell first takes the idle measurement, which also
//! gives the memory and VRAM footprint after startup.

use crate::cell::CellRun;
use crate::layout::{Page, status_roi};
use crate::steps;

use super::Kind;

/// The table operations as key probes: name, key, undo key and the first row whose pixels change.
/// `None` for the row means the header status text.
const OPS: &[(&str, &str, &str, Option<i32>)] = &[
    ("update10", "F1", "F1", Some(0)),
    ("swap", "F2", "F2", Some(1)),
    ("remove", "F3", "F4", Some(2)),
    ("insert", "F4", "F3", Some(2)),
    ("replace", "F5", "F5", Some(0)),
    ("append", "F6", "F7", None),
    ("clear", "F8", "F8", Some(0)),
];
/// The row that the select probe clicks.
const SELECT_ROW: i32 = 4;
/// The key that the search probe types: no label holds a `z`, so it hides every row.
const SEARCH_KEY: &str = "z";

/// Runs the measurements of a scenario.
pub fn run(kind: Kind, run: &mut CellRun) {
    run.step("idle", |r| {
        steps::measure(r, "idle", "idle", &steps::idle())
    });
    match kind {
        Kind::Rows | Kind::Virtual => rows(run),
        Kind::Table => table(run),
        Kind::Ticker | Kind::Spinner => {
            run.step("pacing", |r| {
                steps::measure(r, "pacing", "pacing", &steps::pacing(r))
            });
        }
        Kind::Typing => {
            run.step("typing", |r| {
                steps::measure(r, "typing", "typing", &steps::typing(r))
            });
        }
        Kind::Search => {
            run.step("filter", |r| {
                let params = steps::key(r, SEARCH_KEY, "BackSpace", Page::Header.rows_roi(0, 4));
                steps::measure(r, "filter", "key-latency", &params)
            });
            run.step("typing", |r| {
                steps::measure(r, "typing", "typing", &steps::typing(r))
            });
        }
        Kind::Sheet | Kind::Canvas | Kind::Ide | Kind::Dash | Kind::Chat => unreachable!("scenes measure in crate::scenes"),
        Kind::Resize => {
            run.step("resize", |r| {
                steps::measure(r, "resize", "resize-storm", &steps::resize_storm(r))
            });
            run.step("scale", |r| {
                steps::measure(r, "scale", "scale-switch", &steps::scale_switch(r))
            });
        }
    }
}

/// Hover and scroll of a list from the top of the window.
fn rows(run: &mut CellRun) {
    let page = run.cell.kind.page();
    run.step("hover", |r| {
        steps::measure(
            r,
            "hover",
            "hover-latency",
            &steps::hover(r, page, "pixels"),
        )
    });
    if run.fw.marks {
        run.step("hover-marks", |r| {
            steps::measure(
                r,
                "hover-marks",
                "hover-latency",
                &steps::hover(r, page, "app_mark"),
            )
        });
    }
    if steps::scrolls(run.cell.rows) {
        run.step("scroll", |r| {
            steps::measure(r, "scroll", "scroll-glide", &steps::scroll(r))
        });
    } else {
        run.skip("scroll", "the list is shorter than a glide");
    }
}

/// The selection, every table operation, and the churn of replacements.
fn table(run: &mut CellRun) {
    let page = Page::Header;
    run.step("op-select", |r| {
        steps::measure(
            r,
            "op-select",
            "click-latency",
            &steps::click(r, page, SELECT_ROW),
        )
    });
    for &(name, key, undo, row) in OPS {
        let file = format!("op-{name}");
        let roi = row.map_or_else(status_roi, |row| page.rows_roi(row, 1));
        run.step(&file, |r| {
            steps::measure(r, &file, "key-latency", &steps::key(r, key, undo, roi))
        });
    }
    let cycles = run.effort.churn_cycles;
    run.step("churn", |r| steps::churn(r, "F5", cycles));
}
