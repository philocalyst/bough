//! Defects the standing workloads ran into, as minimal repros (bench lane, round 7). Each is `#[ignore]`d while
//! it fails, so the default run stays green; `cargo test ... --test defects -- --ignored` shows them, and a fix
//! removes the `ignore`.
//!
//!     cargo test --profile dist -p bough-workloads --no-default-features --test defects -- --ignored   (macOS)
#![cfg(target_os = "macos")]
use bough::prelude::*;
use bough::raw::{clip_by, offset, quad, App, Config};

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
#[ignore = "defect: nested clip_by reads its parent's stale rect after a write (round6 3b0b00c698)"]
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
