//! Text round 3's new scenes, pixel-exact at every ring age 1..4: zgui's label churn and zgx's i18n rows
//! (messages with Latin-1, a locale switch re-rendering every row): every frame the presented surface equals a
//! full redraw by the same kernels, and after the run the retained scene equals a fresh mount of the same values.
use bough::prelude::*;
use bough_text_bench::churn::{self, Plot};
use bough_text_bench::font_bytes;
use bough_text_bench::i18n::*;

fn first_diff(a: &[u8], b: &[u8]) -> Option<usize> {
    a.iter().zip(b).position(|(x, y)| x != y)
}

#[test]
fn label_churn_is_pixel_exact_at_every_ring_age() {
    for ring in 1..=4 {
        let mut c = churn::config(font_bytes());
        c.ring = ring;
        let mut app = App::new(Plot { zoom: Signal::new(0) }, c, true);
        app.frame();
        for step in 1..=40 {
            app.turn(|p| {
                p.zoom.set(step);
            });
            if app.frame() {
                let (px, rf) = (app.presented_pixels(), app.reference_pixels());
                assert!(px == rf, "ring {ring} step {step}: presented != full redraw at {:?}", first_diff(&px, &rf));
            }
        }
        let px = app.presented_pixels();
        let mut c = churn::config(font_bytes());
        c.ring = ring;
        let mut fresh = App::new(Plot { zoom: Signal::new(40) }, c, true);
        fresh.frame();
        assert!(px == fresh.presented_pixels(), "ring {ring}: retained != fresh mount");
    }
}

#[test]
fn churn_labels_are_zgui_strings() {
    // the label text is zgui's own function; spot-check the domain's edges
    assert_eq!(churn::label(0, 0), "-100.00");
    assert_eq!(churn::label(1, 0), "-92.08");
    assert!((0..200_000).step_by(997).all(|z| churn::label(z, 3).parse::<f64>().is_ok()));
}

fn inbox(counts: &[u32], tpl: bool) -> Inbox {
    Inbox { counts: std::array::from_fn(|i| Signal::new(counts.get(i).copied().unwrap_or(0))), rows: counts.len(), template: tpl }
}

#[test]
fn i18n_rows_and_locale_switch_are_pixel_exact_at_every_ring_age() {
    let rows = 120;
    for ring in 1..=4 {
        LOCALE.with(|l| l.set(0));
        let mut counts: Vec<u32> = (0..rows as u32).collect();
        let mut cfg = Config::new(1200.0, 900.0, 1.0, font_bytes());
        cfg.ring = ring;
        let mut app = App::new(inbox(&counts, false), cfg, true);
        app.frame();
        for frame in 1..=30usize {
            app.turn(|v| {
                for (i, c) in v.counts[..rows].iter_mut().enumerate().skip(frame % 10).step_by(10) {
                    let n = **c;
                    c.set(n + 1);
                    counts[i] = n + 1;
                }
                if frame == 20 || frame == 25 {
                    // a locale switch (fr: "reçus", Latin-1 through the token lanes), then back
                    LOCALE.with(|l| l.set((frame == 20) as u8));
                    for c in v.counts[..rows].iter_mut() {
                        let _ = &mut **c;
                    }
                }
            });
            if app.frame() {
                let (px, rf) = (app.presented_pixels(), app.reference_pixels());
                assert!(px == rf, "ring {ring} frame {frame}: presented != full redraw at {:?}", first_diff(&px, &rf));
            }
        }
        let px = app.presented_pixels();
        let mut cfg = Config::new(1200.0, 900.0, 1.0, font_bytes());
        cfg.ring = ring;
        let mut fresh = App::new(inbox(&counts, false), cfg, true);
        fresh.frame();
        assert!(px == fresh.presented_pixels(), "ring {ring}: retained != fresh mount");
    }
}
