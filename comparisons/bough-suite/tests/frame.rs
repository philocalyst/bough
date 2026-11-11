//! frame_bench's Motion page, pixel-exact: each of zgx's three modes (paint-only, layout inside fixed slots,
//! layout up to the root) for 60 frames after its 30 warm-up frames, at every ring age 1..=4 and the
//! windowed ring: the presented surface equals a full redraw of the same retained state every other frame,
//! and the retained page equals a fresh mount at the same signal values.
#![cfg(target_os = "macos")]
mod common;
use bough::raw::*;
use bough_suite::*;
use common::*;

const ROWS: usize = 60;
const FRAMES: usize = 60;
const WARM: usize = 30;

fn page(geom: MotionGeom, mode: MotionMode, frame: Option<usize>) -> Motion {
    let mut v = Motion::new(ROWS, geom);
    if let Some(f) = frame {
        motion_step(&mut v, f, mode);
    }
    v
}

fn gate(mode: MotionMode, name: &str) {
    let geom = motion_geom(ROWS);
    for ring in RINGS {
        let mut a = app_ring(page(geom, mode, None), motion_config(), ring);
        a.frame();
        for f in 0..WARM {
            a.turn(|v| motion_step(v, f, mode));
            a.frame();
        }
        check(name, a, FRAMES, 2, |a, t| a.turn(|v| motion_step(v, WARM + t - 1, mode)), || app_ring(page(geom, mode, Some(WARM + FRAMES - 1)), motion_config(), ring));
    }
}

#[test]
fn motion_paint_only_is_pixel_exact() {
    gate(MotionMode::Paint, "motion paint");
}

#[test]
fn motion_cells_are_pixel_exact() {
    gate(MotionMode::Cells, "motion cells");
}

#[test]
fn motion_fold_is_pixel_exact() {
    gate(MotionMode::Fold, "motion fold");
}

/// The probe's FOLD rect is where the column is laid out in the page itself (with the shimmer's space widths
/// set), and the fold's height decides how much of the column shows (its clip follows the signal).
#[test]
fn motion_fold_clip_follows_the_column() {
    let geom = motion_geom(ROWS);
    assert!(geom.space_w > 4.0 && geom.space_w < 7.0, "{geom:?}");
    let a = App::new(page(geom, MotionMode::Fold, None), motion_config(), false);
    assert_eq!(motion_parent_of(&a, b"Folded line 0"), Some(geom.fold_at));
    let mut shown = Vec::new();
    for h in [0.0, 30.0, 84.0] {
        let mut v = page(geom, MotionMode::Fold, None);
        v.fold.set(h);
        let mut a = App::new(v, motion_config(), true);
        a.frame();
        shown.push(a.presented_pixels());
    }
    if let Some(dir) = std::env::var_os("BOUGH_DUMP") {
        // BGRA, 2360 x 1560, the page with the fold at 0, 30 and 84 points
        for (k, px) in shown.iter().enumerate() {
            std::fs::write(std::path::Path::new(&dir).join(format!("motion_fold{k}.bgra")), px).unwrap();
        }
    }
    assert!(shown[0] != shown[1] && shown[1] != shown[2], "the fold's height shows more of the column");
}
