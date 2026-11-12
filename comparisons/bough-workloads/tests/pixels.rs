//! The pixel gate on the standing workloads: through every phase of every scene, the frame the GPU presents equals
//! a from-scratch full redraw of the same retained state by the same kernels (`App::reference_pixels`), at ring
//! ages 1 to 4 (round-robin rings of 1..=4 surfaces) and on the windowed (content-addressed) ring. Scenes run at a
//! small scale (each phase's steps are the harness's, so the gate covers what the benchmarks measure).
//!
//!     cargo test --profile dist -p bough-workloads --test pixels            (macOS: needs Metal)
//!     PIXEL_SCALE=0.2 PIXEL_RINGS=0,2 cargo test ...                          (other scale / rings)
#![cfg(target_os = "macos")]
use bough::raw::{App, Config};
use bough_workloads::harness::sweep;
use bough_workloads::Scene;

fn first_diff(a: &[u8], b: &[u8], w: usize) -> Option<(usize, usize, u8, u8)> {
    a.iter().zip(b).position(|(x, y)| x != y).map(|i| ((i / 4) % w, (i / 4) / w, a[i], b[i]))
}

fn rings() -> Vec<usize> {
    std::env::var("PIXEL_RINGS").ok().map(|v| v.split(',').filter_map(|r| r.trim().parse().ok()).collect()).unwrap_or_else(|| vec![1, 2, 3, 4, 0])
}

fn scale() -> f32 {
    std::env::var("PIXEL_SCALE").ok().and_then(|v| v.parse().ok()).unwrap_or(0.05)
}

/// Mount `S` at `ring` surfaces (0: the windowed ring of 4) and compare after every drawn frame of every phase.
fn gate<S: Scene>(ring: usize, once: bool) {
    let model = S::build(scale());
    let (w, h) = S::SIZE;
    let mut c = Config::new(w, h, 1.0, bough_workloads::font());
    c.caps = model.caps();
    c.ring = if ring == 0 { 4 } else { ring };
    if once {
        c = c.once();
    }
    let mut app = App::new(model, c, true);
    app.windowed = ring == 0;
    let name = format!("{} ring {} {}", S::NAME, if ring == 0 { "windowed".to_string() } else { ring.to_string() }, if once { "once" } else { "redescribe" });
    let mut compared = 0;
    let mut check = |app: &mut App<S>, what: &str, k: usize| {
        let drew = app.frame();
        assert!(!app.bins_overflowed(), "{name}: row bin overflow in {what} {k}");
        if drew {
            let px = app.presented_pixels();
            let reference = app.reference_pixels();
            let w = app.rt.win_w as usize;
            assert!(px == reference, "{name}: {what} step {k}: presented != full redraw, first diff {:?}", first_diff(&px, &reference, w));
            compared += 1;
        }
    };
    check(&mut app, "first frame", 0);
    for k in 0..8 {
        S::steady(&mut app, k);
        check(&mut app, "steady", k);
    }
    for k in 0..4 {
        S::hover(&mut app, k);
        check(&mut app, "hover", k);
    }
    for k in 0..2 {
        S::theme(&mut app, k);
        check(&mut app, "theme", k);
    }
    for k in 0..12 {
        if k % 4 == 0 {
            S::animate(&mut app, k);
        }
        app.tick(1.0 / 60.0);
        check(&mut app, "anim", k);
    }
    for (k, (w, h)) in sweep(S::SIZE, 8).into_iter().enumerate() {
        if k % 3 == 0 {
            S::animate(&mut app, 100 + k);
        }
        app.tick(1.0 / 60.0);
        app.resize(w, h);
        check(&mut app, "resize_anim", k);
    }
    assert!(compared > 0, "{name}: nothing compared");
}

fn all<S: Scene>() {
    for r in rings() {
        gate::<S>(r, true);
    }
    gate::<S>(2, false);
}

#[cfg(feature = "sheet")]
#[test]
fn sheet_retained_equals_full_redraw() {
    all::<bough_workloads::sheet::Sheet>();
}

#[cfg(feature = "ide")]
#[test]
fn ide_retained_equals_full_redraw() {
    all::<bough_workloads::ide::Ide>();
}

#[cfg(feature = "dash")]
#[test]
fn dash_retained_equals_full_redraw() {
    all::<bough_workloads::dash::Dash>();
}

#[cfg(feature = "canvas")]
#[test]
fn canvas_retained_equals_full_redraw() {
    all::<bough_workloads::canvas::Canvas>();
}

#[cfg(feature = "chat")]
#[test]
fn chat_retained_equals_full_redraw() {
    all::<bough_workloads::chat::Chat>();
}
