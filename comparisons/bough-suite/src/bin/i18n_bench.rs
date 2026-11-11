//! zgx `i18n_bench`, native: `ROWS` (1000) counts in a column, a tenth of them (`skip(frame % 10)
//! .step_by(10)`) incremented a frame for `FRAMES` (600) frames, then a locale switch there and back.
//!
//! bough has no i18n layer: the plural message `{n, plural, one {# message} other {# messages}}` is a native
//! rule in the row's text projection (`plural_text`), and the locale is part of each row's state, so a
//! switch rewrites every row's signal (1000 text patches). argv `template`: `format!` text, no locale.
//! Rows are a `List` of row views, each with its own `Signal`; the frame is `App::turn` + `App::frame`.
use bough::raw::*;
use bough_suite::*;
use std::time::Instant;

fn drive<V: View>(mut a: App<V>, frames: usize, step: impl Fn(&mut V, usize), locale: Option<fn(&mut V, bool)>) -> (f64, f64, f64, f64) {
    a.frame();
    let (started, i0) = (Instant::now(), instructions());
    for frame in 1..=frames {
        a.turn(|v| step(v, frame));
        std::hint::black_box(a.frame());
    }
    let (updates, ui) = (started.elapsed(), instructions() - i0);
    let (started, i0) = (Instant::now(), instructions());
    if let Some(set) = locale {
        for fr in [true, false] {
            a.turn(|v| set(v, fr));
            std::hint::black_box(a.frame());
        }
    }
    let (switch, si) = (started.elapsed(), instructions() - i0);
    (updates.as_secs_f64() * 1e6 / frames as f64, switch.as_secs_f64() * 1e6 / 2.0, ui as f64 / frames as f64, si as f64 / 2.0)
}

fn main() {
    let template = std::env::args().nth(1).as_deref() == Some("template");
    let (rows, frames) = (env_usize("ROWS", 1000), env_usize("FRAMES", 600));
    let (frame_us, switch_us, frame_i, switch_i) = if template {
        let a = app_sized(TplRows::new(rows), 1200.0, 900.0, false, rows, 2, 16);
        drive(a, frames, tpl_step, None)
    } else {
        let a = app_sized(I18nRows::new(rows), 1200.0, 900.0, false, rows, 16, 24);
        drive(a, frames, i18n_step, Some(i18n_locale))
    };
    println!(
        "{}: {frame_us:.1} us per frame, {switch_us:.1} us a locale switch; {:.1} k instructions per frame, {:.1} k a locale switch",
        if template { "template" } else { "i18n" },
        frame_i / 1e3,
        switch_i / 1e3
    );
}
