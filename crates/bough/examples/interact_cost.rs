//! The small interactions an idle-looking app is made of, per repair backend (lane memory, round 6, card
//! `2026-11-11-repair-backends`): instructions per interaction (the process's: the GPU path's driver threads count)
//! and wall time from the input call to the frame's pixels written (CPU) or the GPU frame done (`wait_presented`).
//!
//!   cargo run --release -p bough --example interact_cost
//!
//! Interactions: a counter click (pointer down + up on "+", the label repaints), the counter's label changed by a
//! turn, a hover over the button and off it, a caret blink in a focused text field, and one signal write among
//! 1,000 rows. 200 repetitions each after 20 warm-up ones; medians. Headless (an offscreen ring of 4 surfaces).
#[path = "counter.rs"]
#[allow(dead_code)]
mod counter;

use bough::app::{App, Config, Repair, View};
use bough::input::Button;
use bough::prelude::*;
use std::time::Instant;

struct Form {
    s: Signal<String>,
}
impl View for Form {
    fn view(&self) -> impl Element<Self> + '_ {
        div().p(24.0).child(text_input(sig!(self.s)).size(240.0, 32.0))
    }
}

struct Rows {
    n: Signal<u32>,
}
impl View for Rows {
    fn view(&self) -> impl Element<Self> + '_ {
        let row = |i: usize| div().w(960.0).h(24.0).px(12.0).bg(if i % 2 == 0 { rgb(0xf5f5f5) } else { rgb(0xe6e6e6) }).child(text(format!("Item {}", i + 1)));
        // 999 static rows with one signal-bound row among them (the eighth)
        column((each((0..7usize).map(row)), div().w(960.0).h(24.0).px(12.0).bg(rgb(0xe6e6e6)).child(text(sig!(self.n).map(|n| format!("Signal row {n}")))), each((8..1000usize).map(row))))
            .w_pct(1.0)
            .bg(rgb(0xffffff))
    }
}

fn instrs() -> u64 {
    bough_sys::perf::snap().instrs
}

fn med(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

fn font() -> Vec<u8> {
    std::fs::read(std::env::var("ZGX_FONT_FILE").unwrap_or_else(|_| "/System/Library/Fonts/Supplemental/Arial.ttf".into())).unwrap()
}

/// `step(app, k)` performs interaction k (input and frame); measured to pixels written / GPU done.
fn bench<V: View>(name: &str, be: Repair, mut app: App<V>, mut step: impl FnMut(&mut App<V>, u32)) {
    app.repair = be;
    app.frame();
    app.wait_presented();
    for k in 0..20 {
        step(&mut app, k);
        app.wait_presented();
    }
    let (mut is, mut ts) = (Vec::new(), Vec::new());
    let (c0, g0) = (app.surf_stats.cpu, app.surf_stats.frames);
    for k in 20..220 {
        let (i0, t0) = (instrs(), Instant::now());
        step(&mut app, k);
        app.wait_presented();
        is.push((instrs() - i0) as f64);
        ts.push(t0.elapsed().as_secs_f64() * 1e6);
    }
    println!(
        "{name:34} {:>4}: {:>9.0} instructions  {:>8.1} us   (frames {}, of them CPU-repaired {}, tiles last {})",
        format!("{be:?}"),
        med(is),
        med(ts),
        app.surf_stats.frames - g0,
        app.surf_stats.cpu - c0,
        app.repaired_tiles
    );
}

fn main() {
    let f = font();
    for be in [Repair::Cpu, Repair::Gpu] {
        let cfg = || Config::new(480.0, 320.0, 2.0, f.clone());
        // the counter: its button's centre by its interactive record's box
        let app = App::new(counter::Counter { count: Signal::new(0) }, cfg(), true);
        let r = (0..app.record_count()).map(|p| app.record_view(p)).find(|r| r.kind & 0x80 != 0).expect("button");
        let [x, y, w, h] = r.layout.expect("layout box");
        let (bx, by) = ((x + w / 2.0) / 2.0, (y + h / 2.0) / 2.0);
        bench("counter click", be, app, |a, _| {
            a.pointer_down(bx, by, Button::Primary);
            a.pointer_up(bx, by, Button::Primary);
            a.frame();
        });
        bench("counter label change (turn)", be, App::new(counter::Counter { count: Signal::new(0) }, cfg(), true), |a, _| {
            a.turn(|c| c.inc());
            a.frame();
        });
        bench("hover on / off the button", be, App::new(counter::Counter { count: Signal::new(0) }, cfg(), true), |a, k| {
            if k % 2 == 0 {
                a.pointer_move(bx, by)
            } else {
                a.pointer_move(5.0, 300.0)
            }
            a.frame();
        });
        let mut form = App::new(Form { s: Signal::new("caret".into()) }, cfg(), true);
        form.pointer_down(60.0, 40.0, Button::Primary);
        form.pointer_up(60.0, 40.0, Button::Primary);
        form.frame();
        bench("caret blink (focused field)", be, form, |a, _| {
            let half = a.rt.input.blink_half;
            if a.tick(half) {
                a.frame_paced();
            }
        });
        bench("one signal among 1,000 rows", be, App::new(Rows { n: Signal::new(0) }, Config::new(1000.0, 700.0, 2.0, f.clone()), true), |a, _| {
            a.turn(|v| *v.n += 1);
            a.frame();
        });
    }
}
