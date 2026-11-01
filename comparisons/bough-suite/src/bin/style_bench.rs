//! zgx `style_bench`, native: 1,000 boxes (20 x 4, #335577) in a column (no gap: box i at y = 4i) whose
//! x translation `sin((t + i) * 7) * 20` and opacity `0.5 + 0.5 sin(3 t + i)` a clock `t = frame / 120`
//! drives every frame, headless in 1200 x 900. Timed as zgx: the whole loop of clock write + flush
//! (here `App::turn` + `App::frame`), divided by the `FRAMES` (600) run. `ONLY=wires|quads|lanes`.
//!
//! bough has no translate or opacity operand on a layout box and a quad's x is constant, so the boxes are
//! absolutely positioned at their column positions (x origin 20, so the +-20 swing stays in the window):
//! * wires: each box its own one-lane `lanes` element (one wire per box: x and colour alpha);
//! * quads: a plain quad per box with a reactive colour (opacity only: no translation possible);
//! * lanes: one `lanes(1000)` element.
//!
//! argv `labels`: 1,000 text labels "Label {i}" (22 pt lines), opacity only: a text node has no x operand
//! (translation is not portable), and its opacity is `glyph_alpha`, a function pointer that cannot see
//! which label it is, so each label is a `List` row with its own `Signal<Phase>` (index and clock) that the
//! driver writes every frame (1000 signal writes, as zgx's batch of 1000 phase signals in anime_bench).
use bough::prelude::*;
use bough_suite::*;
use std::time::Instant;

fn run<V: View>(name: &str, v: V, set: impl Fn(&mut V, f32)) {
    let frames = env_usize("FRAMES", 600);
    let mut a = app(v, 1200.0, 900.0, false);
    a.frame();
    let (started, i0) = (Instant::now(), instructions());
    for frame in 1..=frames {
        a.turn(|v| set(v, frame as f32 / 120.0));
        std::hint::black_box(a.frame());
    }
    let (t, spent) = (started.elapsed(), instructions() - i0);
    println!("{}[{name}]: {:.1} us per frame, {:.1} k instructions per frame", if name.starts_with("labels") { "labels" } else { "boxes" }, t.as_secs_f64() * 1e6 / frames as f64, spent as f64 / frames as f64 / 1e3);
}

fn main() {
    if std::env::args().nth(1).as_deref() == Some("labels") {
        run("labels, opacity only", StyleLabels::new(STYLE_N), style_labels_set);
        return;
    }
    let only = std::env::var("ONLY").ok();
    let want = |n: &str| only.as_deref().is_none_or(|o| o == n);
    if want("wires") {
        run("wires", StyleWires { t: Signal::new(0.0) }, |v, s| _ = v.t.set(s));
    }
    if want("quads") {
        run("quads", StyleQuads { t: Signal::new(0.0) }, |v, s| _ = v.t.set(s));
    }
    if want("lanes") {
        run("lanes", StyleLanes { t: Signal::new(0.0) }, |v, s| _ = v.t.set(s));
    }
}
