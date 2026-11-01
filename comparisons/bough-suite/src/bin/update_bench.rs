//! zgx-script `update_bench`, native (the Rust side of each scenario; bough has no scripting layer):
//! instructions this thread retires per update (the step and its frame), after a warm-up of 60 updates
//! of `TICKS` (600). `ONLY=name` runs one scenario.
//!
//! * template: a counter's text (`Count: {n}`), a numeric operand rendered into the label.
//! * function-prop: 200 boxes coloured by `selected == i`, as one keyed `select` wire.
//! * rows / rows-in-place: 200 keyed rows (`List`), 20 labels change an update (zgx's published lists).
//! * handler: the click's handler and frame (bough has no input layer on this branch: the handler is
//!   invoked directly; zgx's number includes its pointer dispatch and hit test).
use bough::prelude::*;
use bough_suite::*;

fn measure<V: View>(mut a: App<V>, ticks: usize, mut step: impl FnMut(&mut App<V>, usize)) -> f64 {
    a.frame();
    let prof = std::env::var_os("PHASES").is_some();
    if prof {
        a.prof.enable();
    }
    let ovh = (0..16).map(|_| {
        let s = thread_instructions();
        thread_instructions() - s
    }).min().unwrap_or(0);
    let warmup = 60.min(ticks / 2);
    let mut spent = 0u64;
    for tick in 1..=ticks {
        let s = thread_instructions();
        step(&mut a, tick);
        a.frame();
        if tick > warmup {
            spent += (thread_instructions() - s).saturating_sub(ovh);
        }
    }
    if prof {
        let n = ticks as f64;
        let parts: Vec<String> = a.prof.acc.iter().enumerate().map(|(i, v)| format!("{} {:.1}k", bough::app::PHASE_NAMES[i], *v as f64 / n / 1e3)).collect();
        eprintln!("phases per update: {} | lay stats {:?}", parts.join(", "), a.rt.lay.stats);
    }
    spent as f64 / (ticks - warmup) as f64
}

fn main() {
    let ticks = env_usize("TICKS", 600);
    let only = std::env::var("ONLY").ok();
    let want = |n: &str| only.as_deref().is_none_or(|o| o == n);
    println!("per update: instructions (k)");
    if want("template") {
        let a = app(UTemplate { count: Signal::new(0) }, 1200.0, 900.0, false);
        let k = measure(a, ticks, |a, t| a.turn(|v| *v.count = t as u32));
        println!("  {:14} bough {:8.2}k", "template", k / 1e3);
    }
    if want("function-prop") {
        let a = app(USelect { selected: Signal::new(0) }, 1200.0, 900.0, false);
        let k = measure(a, ticks, |a, t| a.turn(|v| *v.selected = t % ROWS + 1));
        println!("  {:14} bough {:8.2}k", "function-prop", k / 1e3);
    }
    for name in ["rows", "rows-in-place"] {
        if want(name) {
            let published = upd_published(ticks);
            let a = app(URows::new(), 1200.0, 900.0, false);
            let k = measure(a, ticks, |a, t| a.turn(|v| urows_step(v, &published[t])));
            println!("  {:14} bough {:8.2}k", name, k / 1e3);
        }
    }
    if want("handler") {
        let a = app(UHandler { count: Signal::new(0) }, 1200.0, 900.0, false);
        let k = measure(a, ticks, |a, _| a.turn(|v| *v.count += 1));
        println!("  {:14} bough {:8.2}k", "handler", k / 1e3);
    }
}
