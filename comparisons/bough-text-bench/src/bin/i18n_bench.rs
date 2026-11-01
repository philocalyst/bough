//! zgx's `i18n_bench` (crates/zgx/examples/i18n_bench.rs) on bough: `ROWS` (1000) plural messages whose counts
//! change each frame (a tenth of the rows a frame, `FRAMES` 600), then a locale switch there and back, against
//! the same list as `format!` text (`template` argument). Prints zgx's line (µs per frame, µs a locale switch)
//! plus instructions. `RENDER` 0 (default; zgx's bench flushes its scene with no renderer) or 1.
use bough::prelude::*;
use bough_text_bench::i18n::*;
use bough_text_bench::{env, font_bytes, instrs};
use std::time::Instant;

fn main() {
    let template = std::env::args().nth(1).as_deref() == Some("template");
    let (rows, frames): (usize, u32) = (env("ROWS", 1000usize).min(MAX_ROWS), env("FRAMES", 600));
    let render = env("RENDER", 0) != 0;
    let view = Inbox { counts: std::array::from_fn(|i| Signal::new(i as u32)), rows, template };
    let cfg = Config::new(1200.0, 900.0, 2.0, font_bytes());
    let mut app = App::new(view, cfg, render);
    app.frame();
    let (t0, i0) = (Instant::now(), instrs());
    for frame in 1..=frames {
        app.turn(|v| {
            for c in v.counts[..rows].iter_mut().skip(frame as usize % 10).step_by(10) {
                let n = **c;
                c.set(n + 1);
            }
        });
        std::hint::black_box(app.frame());
    }
    let (updates, ui) = (t0.elapsed(), instrs() - i0);
    let (t1, i1) = (Instant::now(), instrs());
    for locale in [1u8, 0] {
        app.turn(|v| {
            if !template {
                LOCALE.with(|l| l.set(locale));
                for c in v.counts[..rows].iter_mut() {
                    let _ = &mut **c; // the messages read the locale: re-render them
                }
            }
        });
        std::hint::black_box(app.frame());
    }
    let (switch, si) = (t1.elapsed(), instrs() - i1);
    println!(
        "{}: {:.1} us per frame, {:.1} us a locale switch ({:.0} instructions a frame, {:.0} a switch, render={render})",
        if template { "template" } else { "i18n" },
        updates.as_secs_f64() * 1e6 / f64::from(frames),
        switch.as_secs_f64() * 1e6 / 2.,
        ui as f64 / f64::from(frames),
        si as f64 / 2.,
    );
}
