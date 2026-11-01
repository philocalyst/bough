//! i18n_bench's rows, pixel-exact: a tenth of the rows change a frame, with locale switches (every row
//! re-rendered) in between, for the plural-message rows and the `format!` template rows; at every ring age
//! and the windowed ring; and the retained rows equal a fresh mount built from an independent model of the
//! counts. The rows below the 900-pt window (40 of the 60) are mounted but not visible.
#![cfg(target_os = "macos")]
mod common;
use bough::prelude::*;
use bough_suite::*;
use common::*;

const ROWS: usize = 60;
const STEPS: usize = 50;

fn cfg() -> Config {
    config_sized(1200.0, 900.0, ROWS, 16, 24)
}
/// Locale switches at these steps (fr, en, fr).
fn locale_at(t: usize) -> Option<bool> {
    match t {
        12 => Some(true),
        25 => Some(false),
        38 => Some(true),
        _ => None,
    }
}
/// Row `i`'s count after `steps` frames (it is bumped by every frame f with f % 10 == i % 10).
fn count(i: usize, steps: usize) -> u32 {
    (i + (1..=steps).filter(|f| f % 10 == i % 10).count()) as u32
}

#[test]
fn i18n_rows_are_pixel_exact() {
    let fr_end = (1..=STEPS).filter_map(locale_at).next_back().unwrap_or(false);
    for ring in RINGS {
        check(
            "i18n",
            app_ring(I18nRows::new(ROWS), cfg(), ring),
            STEPS,
            3,
            |a, t| {
                a.turn(|v| {
                    i18n_step(v, t);
                    if let Some(fr) = locale_at(t) {
                        i18n_locale(v, fr);
                    }
                })
            },
            || {
                let rows = I18nRows { rows: List::from_iter_in((0..ROWS).map(|i| I18nRow { m: Signal::new(Msg { n: count(i, STEPS), fr: fr_end }) })) };
                app_ring(rows, cfg(), ring)
            },
        );
    }
}

#[test]
fn template_rows_are_pixel_exact() {
    for ring in RINGS {
        check(
            "template",
            app_ring(TplRows::new(ROWS), cfg(), ring),
            STEPS,
            3,
            |a, t| a.turn(|v| tpl_step(v, t)),
            || app_ring(TplRows { rows: List::from_iter_in((0..ROWS).map(|i| TplRow { n: Signal::new(count(i, STEPS)) })) }, cfg(), ring),
        );
    }
}

#[test]
fn plural_rules() {
    let t = |n, fr| plural_text(&Msg { n, fr });
    assert_eq!((t(0, false), t(1, false), t(2, false)), ("0 messages".into(), "1 message".into(), "2 messages".into()));
    assert_eq!((t(0, true), t(1, true), t(2, true)), ("0 message".into(), "1 message".into(), "2 messages reçus".into()));
    assert_eq!((template_text(&1), template_text(&5)), ("1 message".to_string(), "5 messages".to_string()));
}
