//! style_bench's boxes, pixel-exact: the per-box one-lane wires, the colour-only quads and the single `lanes`
//! element, at every ring age and the windowed ring; wires and lanes draw the same pixels.
#![cfg(target_os = "macos")]
mod common;
use bough::prelude::*;
use bough_suite::*;
use common::*;

fn cfg() -> Config {
    config_sized(1200.0, 900.0, STYLE_N, 2, 4)
}
/// t = frame / 120 for a second, then jumps.
fn times() -> Vec<f32> {
    (1..=120).map(|f| f as f32 / 120.0).chain([2.5, 2.51, 0.0, 9.9, 1.0 / 3.0]).collect()
}

fn gate<V: View>(name: &str, mk: impl Fn(f32) -> V, set: fn(&mut V, f32)) -> Vec<Vec<u8>> {
    let times = times();
    let last = *times.last().unwrap();
    RINGS.iter().map(|&ring| check(name, app_ring(mk(0.0), cfg(), ring), times.len(), 4, |a, t| a.turn(|v| set(v, times[t - 1])), || app_ring(mk(last), cfg(), ring))).collect()
}

#[test]
fn style_wires_are_pixel_exact() {
    gate("style wires", |t| StyleWires { t: Signal::new(t) }, |v, s| _ = v.t.set(s));
}

#[test]
fn style_quads_are_pixel_exact() {
    gate("style quads", |t| StyleQuads { t: Signal::new(t) }, |v, s| _ = v.t.set(s));
}

#[test]
fn style_lanes_are_pixel_exact() {
    gate("style lanes", |t| StyleLanes { t: Signal::new(t) }, |v, s| _ = v.t.set(s));
}

#[test]
fn style_wires_and_lanes_draw_the_same_pixels() {
    let wires = gate("style wires", |t| StyleWires { t: Signal::new(t) }, |v, s| _ = v.t.set(s));
    let lanes = gate("style lanes", |t| StyleLanes { t: Signal::new(t) }, |v, s| _ = v.t.set(s));
    for (ring, (w, l)) in wires.iter().zip(&lanes).enumerate() {
        assert!(w == l, "ring {ring}: wires != lanes, first diff {:?}", first_diff(w, l, 1200));
    }
}

#[test]
fn style_labels_are_pixel_exact() {
    // 40 of the 60 labels fit the 900-pt window; the opacity writes of the others are not visible
    const N: usize = 60;
    let times = times();
    let last = *times.last().unwrap();
    for ring in RINGS {
        let mk = |t: f32| {
            let mut l = StyleLabels::new(N);
            style_labels_set(&mut l, t);
            l
        };
        check("style labels", app_ring(StyleLabels::new(N), config_sized(1200.0, 900.0, N, 2, 12), ring), times.len(), 4, |a, t| a.turn(|v| style_labels_set(v, times[t - 1])), || app_ring(mk(last), config_sized(1200.0, 900.0, N, 2, 12), ring));
    }
}
