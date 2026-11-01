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

// ------------------------------------------------------------------ vlist rows edited reactively (round 3b)

mod edited_rows {
    use bough::layout::prelude::*;
    use bough::prelude::*;

    pub struct Edits {
        pub scroll: Signal<f32>,
        pub edit: Signal<u32>,
    }

    /// A row's text depends on the edit counter: its line count (so its height) changes with it.
    pub fn words(i: usize, e: u32) -> String {
        let n = 1 + (i * 7 + e as usize * 3) % 6;
        (0..n).map(|k| if (k + i) % 3 == 0 { "the renderer keeps every glyph retained" } else { "while streaming tokens arrive" }).collect::<Vec<_>>().join(" ")
    }

    fn row<'a>(d: &'a Edits, i: usize) -> impl Element<Edits> + 'a {
        column((
            label(format!("row {i}")).font(13.0, 18.0).color(rgb(0x8b98ab)),
            para(sig!(d: Edits => edit).map(move |e| words(i, *e))).font(14.0, 20.0).color(rgb(0xe5edf7)).w(300.0),
        ))
        .w(340.0)
        .px(10.0)
        .py(6.0)
        .bg(rgb(if i % 2 == 0 { 0x161c26 } else { 0x19202b }))
    }

    impl View for Edits {
        fn view(&self) -> impl Element<Self> + '_ {
            (
                quad(0.0, 0.0, 360.0, 600.0).bg(rgb(0x0e1218)),
                measured_list(sig!(self.scroll), 1000, 60.0, [0.0, 0.0, 360.0, 600.0], 2, |d: &Edits, i, m: &mut Mounter<'_, Edits>| m.mount(row(d, i))),
            )
        }
    }
}

/// LAYOUT.md's gap, closed: a reactive text edit in mounted rows changes their heights; the prefix index follows
/// (rows below move, the first visible row stays anchored). Every frame at ring ages 1..4: presented == full
/// redraw, and every mounted row's index height equals its laid-out height (the gap was that it did not). With
/// the list at the top (no row above the viewport measured at an older value), retained == a fresh mount.
#[test]
fn reactive_row_edits_update_the_prefix_index() {
    use edited_rows::*;
    fn index_follows(app: &App<Edits>, ctx: &str) {
        let r = &app.rt.mregions[0];
        for (k, row) in r.rows.iter().enumerate() {
            let laid = app.rt.lay.root_height(row.root).round() as i32;
            assert_eq!(r.idx.height(r.range.start + k), laid, "{ctx}: row {} index height != its layout", r.range.start + k);
        }
    }
    for ring in 1..=4 {
        let mut cfg = Config::new(360.0, 600.0, 2.0, font_bytes());
        cfg.ring = ring;
        let mut app = App::new(Edits { scroll: Signal::new(0.0), edit: Signal::new(0) }, cfg, true);
        app.frame();
        let mut scroll = 0.0f32;
        for step in 1..=30u32 {
            app.turn(|v| {
                if step > 12 && step % 3 == 0 {
                    scroll += 47.0;
                    v.scroll.set(scroll);
                } else {
                    v.edit.set(step);
                }
            });
            if app.frame() {
                let (px, rf) = (app.presented_pixels(), app.reference_pixels());
                assert!(px == rf, "ring {ring} step {step}: presented != full redraw at {:?}", first_diff(&px, &rf));
            }
            index_follows(&app, &format!("ring {ring} step {step}"));
            if step == 12 {
                // still at the top: the retained list equals a fresh mount at the same edit
                let px = app.presented_pixels();
                let mut cfg = Config::new(360.0, 600.0, 2.0, font_bytes());
                cfg.ring = ring;
                let mut fresh = App::new(Edits { scroll: Signal::new(0.0), edit: Signal::new(12) }, cfg, true);
                fresh.frame();
                assert!(px == fresh.presented_pixels(), "ring {ring}: retained rows after reactive edits != fresh mount");
            }
        }
    }
}
