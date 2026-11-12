//! The quick bench's application-side probes (`bench/quick/run.sh`), each in its own cold process:
//!
//!   quick_life counter        startup, memory and shutdown of the design's counter (`examples/counter.rs`),
//!                             headless, the way `Application::run` builds it (bench/quick/life.rs)
//!   quick_life mount <what>   instructions of a cold mount without a GPU: `App::new(view, cfg, false)` and the
//!                             first frame's CPU work, for `empty` (a bare div: the fixed cost), `counter` and
//!                             `list1000` (a plain column of 1,000 rows, a tinted box with two labels each,
//!                             mounted through `each`: not virtualised)
//!
//!   cargo run --release -p bough --example quick_life -- counter
#[path = "counter.rs"]
mod counter;
#[path = "../../../bench/quick/life.rs"]
mod life;

use bough::prelude::*;
use bough::raw::{App, Caps, Config};
use bough::signal::Signal as RawSignal;
use bough_metal::perf::{inject, snap, tinstr};
use counter::Counter;

/// `Application::run`'s region capacities (ui.rs `APP_CAPS`): the counter is measured as an application.
const APP_CAPS: Caps = Caps {
    instances: 1 << 16,
    pages: 1 << 13,
    glyphs: 1 << 19,
    runs: 1 << 16,
    spaces: 1 << 12,
    clips: 1 << 12,
    strikes: 16,
};

fn font() -> Vec<u8> {
    let candidates = [
        std::env::var("BOUGH_FONT").ok(),
        std::env::var("ZGX_FONT_FILE").ok(),
        Some("/System/Library/Fonts/Supplemental/Arial.ttf".into()),
    ];
    candidates
        .into_iter()
        .flatten()
        .find_map(|p| std::fs::read(p).ok())
        .expect("a TrueType font (set BOUGH_FONT)")
}

/// The counter's window as `Window::document` sizes it (480 x 320 points), at a Retina scale.
fn counter_cfg() -> Config {
    let mut cfg = Config::new(480.0, 320.0, 2.0, font());
    cfg.caps = APP_CAPS;
    cfg
}

struct Empty;
impl View for Empty {
    fn view(&self) -> impl Element<Self> + '_ {
        div().bg(rgb(0xffffff))
    }
}

struct Rows {
    rows: Vec<(u32, String)>,
}
impl View for Rows {
    fn view(&self) -> impl Element<Self> + '_ {
        let line = move |i: usize| {
            let (id, label) = &self.rows[i];
            let tint = if i % 2 == 0 {
                rgb(0xf5f5f5)
            } else {
                rgb(0xe6e6e6)
            };
            row((text(format!("{id}")).w(64.0), text(label.clone())))
                .w(960.0)
                .h(32.0)
                .bg(tint)
        };
        div()
            .bg(rgb(0xffffff))
            .child(each((0..self.rows.len()).map(line)))
    }
}

fn list_cfg() -> Config {
    let mut cfg = Config::new(1280.0, 800.0, 1.0, font());
    cfg.caps = Caps {
        instances: 1 << 17,
        pages: 1 << 13,
        glyphs: 1 << 20,
        runs: 1 << 17,
        spaces: 1 << 12,
        clips: 1 << 12,
        strikes: 16,
    };
    cfg
}

/// One cold mount: process-wide and main-thread instructions of `App::new` (no GPU) and of the first frame.
fn mount<V: View>(make: impl FnOnce() -> (V, Config)) {
    let (view, cfg) = make();
    let (p0, t0) = (snap().instrs, tinstr());
    let mut app = App::new(view, cfg, false);
    inject();
    let (p1, t1) = (snap().instrs, tinstr());
    app.frame();
    let (p2, t2) = (snap().instrs, tinstr());
    println!(
        "quick_mount instr_new={} instr_new_thread={} instr_frame={} instr_frame_thread={} records={}",
        p1 - p0,
        t1 - t0,
        p2 - p1,
        t2 - t1,
        app.record_count()
    );
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["counter"] => life::run(
            || {
                App::new(
                    Counter {
                        count: RawSignal::new(0),
                    },
                    counter_cfg(),
                    true,
                )
            },
            |a, _| a.turn(Counter::inc),
        ),
        ["mount", "empty"] => mount(|| (Empty, counter_cfg())),
        ["mount", "counter"] => mount(|| {
            (
                Counter {
                    count: RawSignal::new(0),
                },
                counter_cfg(),
            )
        }),
        ["mount", "list1000"] => mount(|| {
            (
                Rows {
                    rows: (0..1000u32)
                        .map(|i| (i, format!("Row label {i}")))
                        .collect(),
                },
                list_cfg(),
            )
        }),
        _ => {
            eprintln!("usage: quick_life counter | quick_life mount <empty|counter|list1000>");
            std::process::exit(2);
        }
    }
}
