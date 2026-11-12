//! macOS only: the Metal presenter's publisher thread and layer.
#![cfg(target_os = "macos")]
//! The presenter's publication path and window resize (bough-metal RESULTS Part IV):
//! * the publisher thread puts every drawn or held surface on the layer (detached here: no window) once
//!   the GPU is done, the main thread never waits, and the surface it shows is exact;
//! * a resize gives the presenter surfaces, bins and a layer frame of the new size; the first frame after it
//!   is a full redraw at that size, and every frame at every ring age (and the windowed, content-addressed
//!   ring) equals a full redraw, growing past the size the window was created at and shrinking again.
use bough::raw::*;
use bough_scenes::*;
use zgx_workload::{Mode, Workload};

fn exact(app: &mut App<Lab>, what: &str) {
    let px = app.presented_pixels();
    let r = app.reference_pixels();
    let g = app.gpu.as_ref().unwrap();
    assert_eq!((g.w, g.h), (app.rt.win_w as usize, app.rt.win_h as usize), "{what}: surfaces follow the window");
    assert_eq!(px.len(), g.w * g.h * 4);
    if let Some(i) = px.iter().zip(&r).position(|(a, b)| a != b) {
        panic!("{what}: presented != full redraw at pixel ({}, {})", (i / 4) % g.w, (i / 4) / g.w);
    }
}

fn lab_app(ring: usize, publish: bool) -> (App<Lab>, Workload) {
    let mut work = Workload::from_env();
    work.mode = Mode::Both;
    let mut cfg = lab_config(1.0);
    cfg.ring = ring;
    let mut app = App::new(Lab::new(&work), cfg, true);
    app.windowed = ring == 0;
    app.gpu.as_mut().unwrap().publish = publish;
    app.frame();
    (app, work)
}

#[test]
fn publisher_shows_every_frame_exactly() {
    let (mut app, mut work) = lab_app(0, true);
    for t in 0..90 {
        work.tick();
        app.turn(|l| lab_publish(l, &work, Mode::Both));
        assert!(app.frame());
        if t % 10 == 0 {
            app.gpu.as_ref().unwrap().publisher_idle();
            exact(&mut app, &format!("publish tick {t}"));
        }
    }
    let g = app.gpu.as_ref().unwrap();
    g.publisher_idle();
    let (commits, held_back) = g.publish_stats();
    assert_eq!(held_back, 0);
    // one job per frame; a job superseded while queued is never committed (the newer one shows everything)
    assert!(commits >= 1 && commits <= g.stats.gpu_frames + g.stats.ca_only, "commits {commits}, {:?}", g.stats);
}

#[test]
fn resize_grows_and_shrinks_the_surfaces_exactly() {
    // (width, height) in points; the lab is created at 960 x 720
    let sizes = [(1200.0, 800.0), (1200.0, 810.0), (700.0, 500.0), (1400.0, 1000.0), (960.0, 720.0)];
    for (ring, publish) in [(1, false), (2, false), (3, false), (4, false), (0, true)] {
        let (mut app, mut work) = lab_app(ring, publish);
        for (k, &(w, h)) in sizes.iter().enumerate() {
            app.resize(w, h);
            assert!(app.frame(), "the first frame after a resize draws");
            exact(&mut app, &format!("ring {ring} resize {k} to {w}x{h}"));
            for t in 0..6 {
                work.tick();
                app.turn(|l| lab_publish(l, &work, Mode::Both));
                app.frame();
                exact(&mut app, &format!("ring {ring} size {k} tick {t}"));
            }
            if publish {
                let g = app.gpu.as_ref().unwrap();
                g.publisher_idle();
                let f = g.layer.frame();
                assert_eq!((f.size.width, f.size.height), (w as f64, h as f64), "the layer frame follows with the first new frame");
            }
        }
    }
}

#[test]
fn paced_and_input_frames_publish_exactly_in_order() {
    // clock-driven turns publish at the next turn on the calling thread, input turns through the publisher;
    // mixed, every presented surface is exact and the last commit shows the last frame
    let (mut app, mut work) = lab_app(0, true);
    for t in 0..60 {
        work.tick();
        app.turn(|l| lab_publish(l, &work, Mode::Both));
        let drew = if t % 3 == 0 { app.frame() } else { app.frame_paced() };
        assert!(drew);
        if t % 7 == 0 {
            exact(&mut app, &format!("mixed tick {t}"));
        }
    }
    let g = app.gpu.as_mut().unwrap();
    g.sync();
    g.publisher_idle();
    exact(&mut app, "mixed end");
}
