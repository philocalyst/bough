//! The heavy scenes (bough's standing workloads ported to gui-bench, `apps/shared/scenes.rs`): the geometry the
//! probes watch, the layout check points, and the measurements. Every cell of a scene takes, after startup and
//! the layout check:
//!
//! * `idle`: the shared idle measurement (memory and VRAM after startup, idle CPU, commits while suspended);
//! * `hover`: the fan-out: the pointer enters the hover target from a neutral point, and the region of the
//!   elements the target restyles (thousands of them) tells the change (`hover-latency`, pixels);
//! * `theme`: `t` and `t` again, the whole scene recolours (`key-latency`);
//! * `edit`: the scene's steady edit and its undo (`e` in the sheet, `m` in the canvas; `key-latency`);
//! * `scroll`: the sheet's body glides under a finger (`scroll-glide`), when it is long enough;
//! * `resize`: the shared resize storm (`resize-storm`).

use fakeland_bench::presets::PresetParams;
use serde_json::{Value, json};

use crate::cell::CellRun;
use crate::scenarios::Kind;
use crate::steps;

/// Mirrors of `apps/shared/scenes.rs`.
mod sheet {
    pub const TOOLBAR_H: i32 = 40;
    pub const HEADER_H: i32 = 24;
    pub const GUTTER_W: i32 = 56;
    pub const CELL_W: i32 = 110;
    pub const CELL_H: i32 = 22;
    pub const BODY_TOP: i32 = TOOLBAR_H + HEADER_H;
    pub const FOOTER_TOP: i32 = 800 - 22;
    pub const EDIT: (i32, i32) = (2, 1);
    pub const CHROME: u32 = 0xeceef2;
    pub const FILL_EVEN: u32 = 0xffffff;
    pub const FILL_ODD: u32 = 0xf6f7f9;
    pub const fn col_x(c: i32) -> i32 {
        GUTTER_W + c * CELL_W
    }
}

mod canvas {
    pub const TOOLBAR_H: i32 = 44;
    pub const ROW_H: i32 = 24;
    pub const FRAME_X0: i32 = 258;
    pub const FRAME_Y0: i32 = 74;
    pub const FRAME_W: i32 = 140;
    pub const FRAME_H: i32 = 160;
    pub const FRAME_STEP_X: i32 = 152;
    pub const CHROME: u32 = 0xeceef2;
    pub const CANVAS_BG: u32 = 0xe5e5e5;
}

/// The column whose header the hover probe enters, and the rows of it the probe watches.
const HOT_COLUMN: i32 = 3;
const HOT_ROWS: (i32, i32) = (1, 10);
/// The component row the canvas hover probe enters (component 3).
const HOT_COMPONENT: i32 = 2;

/// Returns the points and colours that the layout check reads for a scene (window coordinates, light theme).
pub fn checks(kind: Kind) -> Vec<(i32, i32, u32)> {
    match kind {
        Kind::Sheet => {
            use sheet::*;
            // the right part of a cell is empty (values are left aligned and at most 7 characters wide)
            let x = col_x(4) + CELL_W - 10;
            vec![
                (1200, TOOLBAR_H / 2, CHROME),
                (x, TOOLBAR_H + HEADER_H / 2, CHROME),
                (x, BODY_TOP + CELL_H / 2, FILL_EVEN),
                (x, BODY_TOP + CELL_H + CELL_H / 2, FILL_ODD),
                (1220, 400, FILL_EVEN),
                (1200, FOOTER_TOP + 11, CHROME),
            ]
        }
        Kind::Canvas => {
            use canvas::*;
            vec![
                (1200, TOOLBAR_H / 2, CHROME),
                (1150, 700, CHROME),
                (630, 792, CANVAS_BG),
                (1013, 400, CANVAS_BG),
            ]
        }
        _ => Vec::new(),
    }
}

fn common(iterations: u32) -> PresetParams {
    PresetParams::new().set("iterations", iterations).set("warmup", 1)
}

fn roi(x: i32, y: i32, w: i32, h: i32) -> Value {
    json!({ "x": x, "y": y, "w": w, "h": h })
}

/// The hover fan-out probe: from `away` into `at`, watching `region`.
fn hover(run: &CellRun, at: (i32, i32), away: (i32, i32), region: Value) -> PresetParams {
    common(run.effort.iterations)
        .set("position", json!({ "x": at.0, "y": at.1 }))
        .set("away", json!({ "x": away.0, "y": away.1 }))
        .set("roi", region)
        .set("detector", "pixels")
        .set("repeats", run.effort.hover_repeats)
        .set("timeout", "5s")
}

/// Runs the measurements of a scene.
pub fn measure(kind: Kind, run: &mut CellRun) {
    run.step("idle", |r| steps::measure(r, "idle", "idle", &steps::idle()));
    match kind {
        Kind::Sheet => {
            use sheet::*;
            let hx = col_x(HOT_COLUMN) + CELL_W / 2;
            run.step("hover", |r| {
                let region = roi(col_x(HOT_COLUMN), BODY_TOP + HOT_ROWS.0 * CELL_H, CELL_W, (HOT_ROWS.1 - HOT_ROWS.0) * CELL_H);
                let p = hover(r, (hx, TOOLBAR_H + HEADER_H / 2), (hx, TOOLBAR_H / 2), region);
                steps::measure(r, "hover", "hover-latency", &p)
            });
            run.step("theme", |r| {
                let p = steps::key(r, "t", "t", roi(GUTTER_W, BODY_TOP + 2 * CELL_H, 3 * CELL_W, 5 * CELL_H));
                steps::measure(r, "theme", "key-latency", &p)
            });
            run.step("edit", |r| {
                let p = steps::key(r, "e", "e", roi(col_x(EDIT.1), BODY_TOP + EDIT.0 * CELL_H, CELL_W, CELL_H));
                steps::measure(r, "edit", "key-latency", &p)
            });
            // a glide needs about 1.5 s of travel below the body's first screen
            if run.cell.rows as i32 * CELL_H >= (FOOTER_TOP - BODY_TOP) + 1200 {
                run.step("scroll", |r| {
                    let p = steps::scroll(r)
                        .set("position", json!({ "x": 640, "y": 600 }))
                        .set("roi", roi(0, BODY_TOP, 1280, 440));
                    steps::measure(r, "scroll", "scroll-glide", &p)
                });
            } else {
                run.skip("scroll", "the sheet is shorter than a glide");
            }
        }
        Kind::Canvas => {
            use canvas::*;
            run.step("hover", |r| {
                let y = TOOLBAR_H + HOT_COMPONENT * ROW_H + ROW_H / 2;
                let region = roi(FRAME_X0, FRAME_Y0, FRAME_STEP_X + FRAME_W, FRAME_H);
                let p = hover(r, (120, y), (120, TOOLBAR_H / 2), region);
                steps::measure(r, "hover", "hover-latency", &p)
            });
            run.step("theme", |r| {
                let p = steps::key(r, "t", "t", roi(240, TOOLBAR_H, 780, 200));
                steps::measure(r, "theme", "key-latency", &p)
            });
            run.step("edit", |r| {
                let p = steps::key(r, "m", "m", roi(FRAME_X0, FRAME_Y0, FRAME_W, FRAME_H));
                steps::measure(r, "edit", "key-latency", &p)
            });
        }
        Kind::Ide | Kind::Dash | Kind::Chat => {
            // the reference scenes (`apps/shared/scenes.rs`, `mod ide/dash/chat`): hover target, away point, the
            // regions the probes watch
            let (at, away, hover_roi, theme_roi, edit_roi) = match kind {
                Kind::Ide => ((306, 99), (640, 789), roi(284, 36, 700, 360), roi(284, 36, 700, 360), roi(284, 36 + 2 * 18, 776, 18)),
                Kind::Dash => ((1108, 28), (700, 28), roi(16, 72, 1248, 170), roi(16, 72, 1248, 170), roi(16, 72, 1248, 170)),
                _ => ((766, 30), (416, 30), roi(300, 60, 980, 600), roi(300, 60, 980, 600), roi(300, 736, 500, 64)),
            };
            run.step("hover", |r| {
                let p = hover(r, at, away, hover_roi.clone());
                steps::measure(r, "hover", "hover-latency", &p)
            });
            run.step("theme", |r| {
                let p = steps::key(r, "t", "t", theme_roi.clone());
                steps::measure(r, "theme", "key-latency", &p)
            });
            run.step("edit", |r| {
                let p = steps::key(r, "e", "e", edit_roi.clone());
                steps::measure(r, "edit", "key-latency", &p)
            });
        }
        _ => unreachable!("not a scene"),
    }
    run.step("resize", |r| steps::measure(r, "resize", "resize-storm", &steps::resize_storm(r)));
}
