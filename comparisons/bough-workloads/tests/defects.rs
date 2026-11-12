//! Defects the standing workloads ran into, as minimal repros (bench lane, round 7). Each is `#[ignore]`d while
//! it fails, so the default run stays green; `cargo test ... --test defects -- --ignored` shows them, and a fix
//! removes the `ignore`.
//!
//!     cargo test --profile dist -p bough-workloads --no-default-features --test defects -- --ignored   (macOS)
#![cfg(target_os = "macos")]
use bough::prelude::*;
use bough::raw::{clip_by, offset, quad, App, Config, Curve};

fn first_diff(a: &[u8], b: &[u8], w: usize) -> Option<(usize, usize, u8, u8)> {
    a.iter().zip(b).position(|(x, y)| x != y).map(|i| ((i / 4) % w, (i / 4) / w, a[i], b[i]))
}

/// Two `clip_by`s nested, both following one pan, around content that follows it too (the canvas scene's
/// frame -> group structure).
struct Nested {
    pan: Signal<(f32, f32)>,
}

impl View for Nested {
    fn view(&self) -> impl Element<Self> + '_ {
        (
            quad(0.0, 0.0, 400.0, 300.0).bg(rgb(0xffffff)),
            clip_by(
                sig!(self.pan),
                |p: &(f32, f32)| [20.0 + p.0, 20.0 + p.1, 200.0, 200.0],
                clip_by(
                    sig!(self.pan),
                    |p: &(f32, f32)| [60.0 + p.0, 60.0 + p.1, 100.0, 100.0],
                    offset(sig!(self.pan), quad(0.0, 0.0, 400.0, 300.0).bg(rgb(0x2f6bff))),
                ),
            ),
        )
    }
}

fn mount(pan: (f32, f32)) -> App<Nested> {
    let mut a = App::new(Nested { pan: Signal::new(pan) }, Config::new(400.0, 300.0, 1.0, bough_workloads::font()).once(), true);
    a.frame();
    a
}

/// Reported by the canvas scene: after a pan, the inner clip is cut against the outer clip's old rect (wires run
/// newest first, and a `clip_by` patch reads its parent's current rect), so the retained frame differs from a fresh
/// mount at the same pan.
#[test]
#[ignore = "defect: nested clip_by reads its parent's stale rect after a write (round6 3b0b00c698; fixed on r7/core, derived clips in rank order, not yet on round6)"]
fn nested_clip_by_retained_equals_fresh() {
    let mut a = mount((0.0, 0.0));
    for (k, p) in [(30.0f32, 0.0f32), (30.0, 40.0), (-15.0, 10.0)].into_iter().enumerate() {
        a.turn(|v| {
            v.pan.set(p);
        });
        a.frame();
        let px = a.presented_pixels();
        let full = a.reference_pixels();
        let mut f = mount(p);
        let fresh = f.presented_pixels();
        let w = a.rt.win_w as usize;
        assert!(px == full, "step {k}: presented != full redraw at {:?}", first_diff(&px, &full, w));
        assert!(px == fresh, "step {k} pan {p:?}: retained != fresh mount at {:?}", first_diff(&px, &fresh, w));
    }
}

/// A list row with a running layout transition, in a scroll box whose rows unmount by exposure (the chat scene's
/// arrivals). Reported by the chat agent: a resize in the same step as the row's transition panicked with
/// `layout node is in the Bough` (lay_deep.rs) from `lay_compute_pending` in `App::resize`; its reading of the
/// code is that `Rt::cut_scope` kills a released scope's wires but not its transitions, which keep restyling a
/// dead layout node.
///
/// Minimal trigger (found by bisecting the chat scene): the row's transition must be started by a write AFTER the
/// row mounted (a write in the push turn only sets the mounted value: no transition runs), the view must be
/// scrolled to the end (`wheel_points`) between the push and that write (without the scroll the test passes), and
/// the window must resize before the next frame.
struct Row {
    h: Signal<f32>,
}
impl View for Row {
    fn view(&self) -> impl Element<Self> + '_ {
        row((block(200.0, 0.0).bg(rgb(0x2f6bff)).h_op(sig!(self.h).transition(Curve::out(0.3))), text("arrival")))
    }
}
struct Feed {
    scroll: Signal<f32>,
    rows: List<Row>,
}
impl View for Feed {
    fn view(&self) -> impl Element<Self> + '_ {
        column((
            column(each((0..200).map(|i| para(format!("history message {i}: a paragraph long enough to wrap onto two or three lines when the window is narrow, and onto one when it is wide")).font(12.0, 18.0).w_pct(1.0)))),
            list(sig!(self.rows)).w_pct(1.0),
        ))
        .w_pct(1.0)
        .h_pct(1.0)
        .scroll(sig!(self.scroll))
    }
}

#[test]
#[ignore = "defect (reported by the chat scene): a list row's transition outlives the row; resize panics (round6 3b0b00c698)"]
fn list_row_transition_survives_resize() {
    let mut a = App::new(Feed { scroll: Signal::new(0.0), rows: List::new() }, Config::new(400.0, 300.0, 1.0, bough_workloads::font()).once(), true);
    a.frame();
    a.resize(900.0, 400.0);
    a.frame();
    for k in 0..8 {
        // a row arrives mounted at h = 28 (turn 1), the view scrolls to the end, the transition is started by a
        // second turn (28 -> 0: only a write AFTER the row mounted retargets it), and in the same step the window
        // narrows (the history re-wraps taller, so the new row leaves the visible window) or widens again
        a.turn(|v| {
            v.rows.push(Row { h: Signal::new(28.0) });
        });
        a.wheel_points(100.0, 100.0, 1.0e7);
        a.turn(|v| {
            let n = v.rows.len();
            v.rows.edit(n - 1).h.set(0.0);
        });
        a.tick(1.0 / 60.0);
        a.resize(if k % 2 == 0 { 260.0 } else { 900.0 }, if k % 2 == 0 { 300.0 } else { 400.0 });
        a.tick(1.0 / 60.0);
        a.frame();
        let px = a.presented_pixels();
        let full = a.reference_pixels();
        assert!(px == full, "step {k}: presented != full redraw");
    }
}
