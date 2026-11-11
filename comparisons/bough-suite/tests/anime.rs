//! anime_bench's dots, pixel-exact: both variants (per-dot wires, one `lanes` element) at every ring age and
//! the windowed ring equal a full redraw after every drawn frame checked and a fresh mount at the end; and
//! the two variants draw the same pixels.
#![cfg(target_os = "macos")]
mod common;
use bough::raw::*;
use bough_suite::*;
use common::*;

fn cfg() -> Config {
    Config::new(1200.0, 900.0, 1.0, font()).once()
}
/// 120 Hz frames for a second, then irregular jumps (a dropped-frame clock, the stagger's start and wrap).
fn times() -> Vec<f32> {
    (1..=120).map(|f| f as f32 / 120.0).chain([2.5, 2.51, 4.003, 0.0, 0.01, 7.3, 100.0, 3.4]).collect()
}

fn gate<V: View>(name: &str, mk: impl Fn(f32) -> V, set: fn(&mut V, f32)) -> Vec<Vec<u8>> {
    let times = times();
    let last = *times.last().unwrap();
    RINGS.iter().map(|&ring| check(name, app_ring(mk(0.0), cfg(), ring), times.len(), 4, |a, t| a.turn(|v| set(v, times[t - 1])), || app_ring(mk(last), cfg(), ring))).collect()
}

#[test]
fn anime_wires_are_pixel_exact() {
    gate("anime wires", |t| AnimeWires { t: Signal::new(t) }, |v, s| _ = v.t.set(s));
}

#[test]
fn anime_lanes_are_pixel_exact() {
    gate("anime lanes", |t| AnimeLanes { t: Signal::new(t) }, |v, s| _ = v.t.set(s));
}

#[test]
fn anime_wires_and_lanes_draw_the_same_pixels() {
    let wires = gate("anime wires", |t| AnimeWires { t: Signal::new(t) }, |v, s| _ = v.t.set(s));
    let lanes = gate("anime lanes", |t| AnimeLanes { t: Signal::new(t) }, |v, s| _ = v.t.set(s));
    for (ring, (w, l)) in wires.iter().zip(&lanes).enumerate() {
        assert!(w == l, "ring {ring}: wires != lanes, first diff {:?}", first_diff(w, l, 1200));
    }
}
