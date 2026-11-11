//! zgx-script `anime_bench`, native: 1,000 dots (6 x 6, #58a6ff, x = i % 40 * 20) whose y (300 * p) and
//! opacity (1 - 0.8 p) follow zgx's eased, staggered phase every 120 Hz frame, headless in a 1200 x 900
//! window. Per frame, as zgx: the update (`App::turn`: the clock write and its wires) plus the scene's
//! flush (`App::frame`: damage, tile list). `FRAMES` frames (600); `ONLY=wires|lanes`.
//!
//! * wires: one quad per dot with `.span(sig!(t).map(..))` and `.bg(sig!(t).map(..))`: one clock signal,
//!   two per-dot projections (zgx's "rust signals": a reactive style per dot).
//! * lanes: one `lanes(1000, sig!(t), ..)` element (one wire, one pass over a column).
//!
//! bough has no scripting layer (zgx's luau column) and no scene-graph "direct" floor.
use bough::raw::*;
use bough_suite::*;
use std::time::{Duration, Instant};

struct Res {
    animate: Duration,
    scene: Duration,
    mounted: Duration,
    instrs: u64,
}

fn run<V: View>(v: V, set: impl Fn(&mut V, f32)) -> Res {
    let frames = env_usize("FRAMES", 600);
    let built = Instant::now();
    let mut a = app(v, 1200.0, 900.0, false);
    a.frame();
    let mounted = built.elapsed();
    let (mut animate, mut scene) = (Duration::ZERO, Duration::ZERO);
    let i0 = instructions();
    for index in 1..=frames {
        let began = Instant::now();
        a.turn(|v| set(v, index as f32 / 120.0));
        let animated = Instant::now();
        assert!(a.frame());
        animate += animated - began;
        scene += animated.elapsed();
    }
    let instrs = instructions() - i0;
    Res { animate: animate / frames as u32, scene: scene / frames as u32, mounted, instrs: instrs / frames as u64 }
}

fn main() {
    let only = std::env::var("ONLY").ok();
    let want = |n: &str| only.as_deref().is_none_or(|o| o == n);
    let wires = want("wires").then(|| run(AnimeWires { t: Signal::new(0.0) }, |v, s| _ = v.t.set(s)));
    let lanes = want("lanes").then(|| run(AnimeLanes { t: Signal::new(0.0) }, |v, s| _ = v.t.set(s)));
    println!("{ANIME_N} elements, y + opacity every frame, mean per frame:");
    let us = |d: Duration| d.as_secs_f64() * 1e6;
    for (name, r) in [("bough wires", wires), ("bough lanes", lanes)] {
        let r = r.unwrap_or(Res { animate: Duration::ZERO, scene: Duration::ZERO, mounted: Duration::ZERO, instrs: 0 });
        println!(
            "  {name:20}: {:>7.1} µs  (animate {:>6.1}, scene {:>6.1})  build+mount+layout {:>5.2} ms  {:>8.1} k instr/frame",
            us(r.animate + r.scene),
            us(r.animate),
            us(r.scene),
            r.mounted.as_secs_f64() * 1e3,
            r.instrs as f64 / 1e3
        );
    }
}
