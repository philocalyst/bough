//! Pixel-exactness of the whole pipeline on zgx's heavy scene, per mode:
//!  1. after every `EVERY` ticks the presented surface (read back from the IOSurface the ring drew
//!     into) equals a from-scratch full redraw of the same retained state (all records, one pass):
//!     proves damage -> repair -> cull -> stencil is byte exact (ring ages 1..4 included);
//!  2. at the end it equals a *fresh mount* of the view at the same signal values: proves the wires
//!     (text, bars, progress, status, virtual-list mount/unmount/scroll) leave the same pixels as
//!     describing the final state from scratch;
//!  3. idle is zero work.
use bough::prelude::*;
use bough_heavy::*;
use zgx_workload::heavy::*;
use zgx_workload::Mode;

fn first_diff(a: &[u8], b: &[u8]) -> Option<(usize, usize, u8, u8)> {
    a.iter().zip(b).position(|(x, y)| x != y).map(|i| ((i / 4) % 2560, (i / 4) / 2560, a[i], b[i]))
}

fn check(mode: Mode, ticks: usize, every: usize) {
    let mut work = Heavy::from_env();
    work.mode = mode;
    let mut app = App::new(Dash::new(&work), config(2.0), true);
    app.frame();
    let mut checked = 0;
    for t in 1..=ticks {
        work.tick();
        app.turn(|d| publish(d, &work, mode));
        let drew = app.frame();
        if drew && t % every == 0 {
            let px = app.presented_pixels();
            let reference = app.reference_pixels();
            assert!(px == reference, "{mode:?} tick {t}: presented != full redraw at {:?}", first_diff(&px, &reference));
            checked += 1;
        }
    }
    assert!(checked > 0 || mode == Mode::Idle);
    if mode != Mode::Idle {
        let px = app.presented_pixels();
        let mut fresh = App::new(Dash::new(&work), config(2.0), true);
        fresh.frame();
        let fp = fresh.presented_pixels();
        assert!(px == fp, "{mode:?}: retained after {ticks} ticks != fresh mount, first diff {:?}", first_diff(&px, &fp));
    }
}

#[test]
fn stream_is_pixel_exact() {
    check(Mode::Stream, 240, 7);
}
#[test]
fn scroll_is_pixel_exact() {
    check(Mode::Scroll, 240, 7);
}
#[test]
fn both_is_pixel_exact() {
    check(Mode::Both, 400, 9);
}
#[test]
fn scroll_across_many_rows_and_a_jump_is_pixel_exact() {
    // large scroll steps: whole windows of rows unmount/mount at once, then a jump back to the top
    let work = Heavy::from_env();
    let mut app = App::new(Dash::new(&work), config(2.0), true);
    app.frame();
    for (i, s) in [0.0f32, 14.0, 200.0, 1000.0, 1001.0, 5000.0, 123456.0, 14.0, 0.0, 777.5].iter().enumerate() {
        app.turn(|d| {
            d.scroll.set(*s);
        });
        if app.frame() {
            let (px, r) = (app.presented_pixels(), app.reference_pixels());
            assert!(px == r, "scroll step {i} to {s}: {:?}", first_diff(&px, &r));
        }
    }
}
#[test]
fn idle_is_zero_work() {
    let mut work = Heavy::from_env();
    work.mode = Mode::Idle;
    let mut app = App::new(Dash::new(&work), config(2.0), true);
    assert!(app.frame());
    let (w0, f0, t0) = (app.rt.ctr.wires_run, app.frames, app.rt.ctr.text_shaped);
    for _ in 0..100 {
        work.tick();
        app.turn(|d| publish(d, &work, Mode::Idle));
        assert!(!app.frame(), "idle frame drew");
    }
    assert_eq!((app.rt.ctr.wires_run, app.frames, app.rt.ctr.text_shaped), (w0, f0, t0));
    // a write that changes nothing is cut off by the signal's equality test
    app.turn(|d| {
        d.live.set(0);
    });
    assert!(!app.frame());
}
