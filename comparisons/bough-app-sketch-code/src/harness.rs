//! zgx's `apps.rs` `Bench::measure` on bough: an interaction (the `k`th), each followed by its frame, `reps`
//! times after two to warm up; its time and instructions retired per interaction, printed in zgx's format
//! (`name: 1.23 ms, 0.45 M instructions`). With a GPU (`RENDER=1`) the span ends as zgx's does, with the GPU
//! caught up and the frame read back (zgx's `Drawing::finish` is `GpuRenderer::readback`).
use bough::raw::*;
use std::time::Instant;

/// `ONLY=name` measures the interactions whose names hold it, `SKIP=name` the others (zgx's filters).
pub fn selected(name: &str) -> bool {
    !(std::env::var("ONLY").is_ok_and(|only| !name.contains(only.as_str())) || std::env::var("SKIP").is_ok_and(|skip| name.contains(skip.as_str())))
}

/// The GPU caught up and the presented frame read back (nothing headless).
pub fn finish<V: View>(app: &mut App<V>) {
    if app.has_surfaces() {
        let px = app.presented_pixels();
        std::hint::black_box(px);
    }
}

/// zgx's `measure`: two warm-ups, then `reps` measured; `step(state, k)` is the interaction and its frame.
pub fn measure<T>(state: &mut T, name: &str, reps: usize, mut step: impl FnMut(&mut T, usize), mut finish: impl FnMut(&mut T)) -> Option<(f64, f64)> {
    if !selected(name) {
        return None;
    }
    for k in 0..2 {
        step(state, k);
    }
    finish(state);
    let (start, before) = (Instant::now(), crate::instructions());
    for k in 0..reps {
        step(state, k + 2);
    }
    finish(state);
    let r = reps as f64;
    let ms = start.elapsed().as_secs_f64() * 1e3 / r;
    let millions = (crate::instructions() - before) as f64 / 1e6 / r;
    println!("{name}: {ms:.2} ms, {millions:.2} M instructions");
    Some((ms, millions))
}

/// The interactive records under (x, y) points, nearest first (bough-core's hit test: the topmost record's
/// chain of `KF_INTERACT` ancestors-or-self, as `App`'s pointer dispatch finds it).
pub fn hit_chain<V: View>(app: &App<V>, x: f32, y: f32) -> Vec<bough_core::Handle> {
    let s = app.rt.scale;
    let (px, py) = ((x * s).floor() as i32, (y * s).floor() as i32);
    let b = &app.rt.bough;
    let mut chain = Vec::new();
    if let Some(t) = b.hit_top(px, py) {
        b.flagged_chain(t, bough_core::KF_INTERACT, &mut chain);
    }
    chain.iter().filter_map(|&p| b.handle_at(p)).collect()
}

/// A route bough's input has no form for (a wheel over an element that is not a scroller, a pinch): the event
/// is hit-tested as the pointer's are, and if `target` is in the chain under it, its handler runs as a turn.
pub fn route_to<V: View>(app: &mut App<V>, x: f32, y: f32, target: bough_core::Handle, f: impl FnOnce(&mut V)) -> bool {
    if !hit_chain(app, x, y).contains(&target) {
        return false;
    }
    app.turn(f);
    true
}
