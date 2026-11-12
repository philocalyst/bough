//! Tile redundancy of lab / animate / blink / vscroll frames (Opus collaborator probe; see opus/stats.rs).
use bough::raw::*;
use bough_scenes::*;
use zgx_workload::{Mode, Workload};
#[path = "opus/stats.rs"]
mod stats;

fn run<V: View>(mut app: App<V>, ticks: usize, label: &str, mut step: impl FnMut(&mut App<V>, usize)) {
    app.frame();
    let px = app.presented_pixels();
    let (w, h) = (app.rt.win_w as usize, app.rt.win_h as usize);
    assert_eq!(px.len(), w * h * 4);
    let mut st = stats::Stats::new(w, h);
    st.frame(&px, &[]);
    for t in 1..=ticks {
        step(&mut app, t);
        if app.frame() {
            let rows = app.debug_rows().to_vec();
            let px = app.presented_pixels();
            st.frame(&px, &rows);
        }
    }
    st.report(label);
}

fn rc(mut c: Config) -> Config {
    c.ring = std::env::var("RING").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    c
}

fn main() {
    let ticks: usize = std::env::var("TICKS").ok().and_then(|v| v.parse().ok()).unwrap_or(120);
    let scale = 2.0;
    for m in ["stream", "scroll", "both"] {
        let mut work = Workload::from_env();
        let mode = match m { "stream" => Mode::Stream, "scroll" => Mode::Scroll, _ => Mode::Both };
        work.mode = mode;
        let app = App::new(Lab::new(&work), rc(lab_config(scale)), true);
        run(app, ticks, &format!("lab {m}"), |app, _| {
            work.tick();
            app.turn(|l| lab_publish(l, &work, mode));
        });
    }
    let app = App::new(Anim { clock: Signal::new(0.0) }, rc(anim_config(scale)), true);
    run(app, ticks, "animate", |app, t| app.turn(|a| { a.clock.set(t as f32 / 60.0); }));
    let app = App::new(Blink { shown: Signal::new(true) }, rc(blink_config(scale)), true);
    run(app, ticks, "blink", |app, _| app.turn(|b| { let s = !*b.shown; b.shown.set(s); }));
    let app = App::new(MicroScroll { scroll: Signal::new(0.0) }, rc(micro_config("vscroll", scale)), true);
    run(app, ticks, "micro vscroll", |app, t| app.turn(|s| { s.scroll.set(t as f32 * 12.0); }));
}
