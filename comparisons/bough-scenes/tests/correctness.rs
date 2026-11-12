//! Pixel-exactness of every scene, as for heavy: (1) after every `every`-th drawn frame the presented
//! surface (read back from the IOSurface the ring drew into) equals a from-scratch full redraw of the same
//! retained state through the same kernels (damage -> repair(age) -> tile list -> GPU cull and repair is
//! byte exact), for EVERY ring age 1..=4 (a round-robin ring of `ring` surfaces repairs at age `ring`),
//! and with the windowed content-addressed ring; (2) at the end it equals a fresh mount of the view at
//! the same signal values; (3) a turn that changes nothing draws nothing.
use bough::raw::*;
use bough_scenes::*;
use zgx_workload::{Mode, Workload};

fn first_diff(a: &[u8], b: &[u8], w: usize) -> Option<(usize, usize, u8, u8)> {
    a.iter().zip(b).position(|(x, y)| x != y).map(|i| ((i / 4) % w, (i / 4) / w, a[i], b[i]))
}

/// Run `steps` (each returns after queueing its writes), checking as described. `fresh` mounts the
/// view at the state the steps reached.
fn check<V: View>(name: &str, mut app: App<V>, steps: usize, every: usize, mut step: impl FnMut(&mut App<V>, usize), fresh: impl Fn() -> App<V>) -> u64 {
    let w = app.rt.win_w as usize;
    app.frame();
    let name = format!("{name} (ring {:?})", ring_of(&app));
    let mut checked = 0;
    for t in 1..=steps {
        step(&mut app, t);
        let drew = app.frame();
        assert!(!app.bins_overflowed(), "{name}: row bin overflow at step {t}");
        if drew && t % every == 0 {
            let px = app.presented_pixels();
            let reference = app.reference_pixels();
            assert!(px == reference, "{name} step {t}: presented != full redraw at {:?}", first_diff(&px, &reference, w));
            checked += 1;
        }
    }
    assert!(checked > 0, "{name}: nothing was compared");
    let px = app.presented_pixels();
    let mut f = fresh();
    f.frame();
    let fp = f.presented_pixels();
    assert!(px == fp, "{name}: retained after {steps} steps != fresh mount, first diff {:?}", first_diff(&px, &fp, w));
    checked
}

/// The surface ring a test runs with: `1..=4` round-robin surfaces (repair age = ring), or 0 for the windowed
/// content-addressed ring (offscreen: nothing is held by a compositor).
fn ring_of<V: View>(a: &App<V>) -> String {
    if a.windowed { "windowed".into() } else { "rr".into() }
}
const RINGS: [usize; 5] = [1, 2, 3, 4, 0];
fn with_ring(mut c: Config, ring: usize) -> Config {
    c.ring = if ring == 0 { 4 } else { ring };
    c
}
fn app_ring<V: View>(v: V, c: Config, ring: usize) -> App<V> {
    let mut a = App::new(v, with_ring(c, ring), true);
    a.windowed = ring == 0;
    a
}

fn lab(mode: Mode, ticks: usize, every: usize) {
    for ring in RINGS {
        lab_ring(mode, ticks, every, ring);
    }
}

fn lab_ring(mode: Mode, ticks: usize, every: usize, ring: usize) {
    let mut work = Workload::from_env();
    work.mode = mode;
    let app = app_ring(Lab::new(&work), lab_config(2.0), ring);
    let mut w2 = Workload::from_env();
    w2.mode = mode;
    check(
        &format!("lab {mode:?}"),
        app,
        ticks,
        every,
        |a, _| {
            w2.tick();
            a.turn(|l| lab_publish(l, &w2, mode));
        },
        || {
            let mut w3 = Workload::from_env();
            w3.mode = mode;
            for _ in 0..ticks {
                w3.tick();
            }
            App::new(Lab::new(&w3), lab_config(2.0), true)
        },
    );
}

#[test]
fn lab_stream_is_pixel_exact() {
    lab(Mode::Stream, 120, 7);
}
#[test]
fn lab_scroll_is_pixel_exact() {
    lab(Mode::Scroll, 120, 7);
}
#[test]
fn lab_both_is_pixel_exact() {
    lab(Mode::Both, 200, 9);
}

#[test]
fn lab_scroll_jumps_are_pixel_exact() {
    let jumps = [14.0f32, 200.0, 1000.0, 1001.0, 5000.0, 123456.0, 14.0, 0.0, 777.5, 2_799_600.0, 2_799_586.0, 0.0];
    for ring in RINGS {
    let mk = |s: f32| {
        let work = Workload::from_env();
        let mut l = Lab::new(&work);
        l.scroll.set(s);
        app_ring(l, lab_config(2.0), ring)
    };
    check(
        "lab jumps",
        mk(0.0),
        jumps.len(),
        1,
        |a, t| {
            a.turn(|l| {
                l.scroll.set(jumps[t - 1]);
            })
        },
        || mk(*jumps.last().unwrap()),
    );
    }
}

#[test]
fn animate_is_pixel_exact() {
    // every 120 Hz refresh for two seconds, then irregular jumps (a dropped-frame clock)
    let times: Vec<f32> = (1..=240).map(|f| f as f32 / 120.0).chain([2.5, 2.51, 7.3, 7.31, 100.0, 0.0, 3.4]).collect();
    let n = times.len();
    for ring in RINGS {
    check(
        "animate",
        app_ring(Anim { clock: Signal::new(0.0) }, anim_config(2.0), ring),
        n,
        5,
        |a, t| {
            a.turn(|x| {
                x.clock.set(times[t - 1]);
            })
        },
        || app_ring(Anim { clock: Signal::new(*times.last().unwrap()) }, anim_config(2.0), ring),
    );
    }
}

#[test]
fn blink_is_pixel_exact_and_idle_between_flips_is_free() {
    let mut app = App::new(Blink { shown: Signal::new(true) }, blink_config(2.0), true);
    assert!(app.frame());
    let (f0, t0) = (app.frames, app.rt.ctr.text_set);
    for _ in 0..50 {
        app.turn(|_| {});
        assert!(!app.frame(), "an unchanged turn drew");
    }
    assert_eq!((app.frames, app.rt.ctr.text_set), (f0, t0));
    for ring in RINGS {
    let n = check(
        "blink",
        app_ring(Blink { shown: Signal::new(true) }, blink_config(2.0), ring),
        31,
        1,
        |a, _| {
            a.turn(|b| {
                let s = !*b.shown;
                b.shown.set(s);
            })
        },
        || app_ring(Blink { shown: Signal::new(false) }, blink_config(2.0), ring),
    );
    assert!(n >= 30);
    }
}

#[test]
fn blink_is_served_by_held_surfaces_after_two_frames() {
    // the windowed ring is content-addressed: after the mount frame and the first flip, every flip finds a
    // surface that already holds its state and publishes it with no GPU work; the pixels stay exact
    let mut app = app_ring(Blink { shown: Signal::new(true) }, blink_config(2.0), 0);
    app.frame();
    for t in 1..=40 {
        app.turn(|b| {
            let s = !*b.shown;
            b.shown.set(s);
        });
        assert!(app.frame());
        let (px, r) = (app.presented_pixels(), app.reference_pixels());
        assert!(px == r, "flip {t}: presented != full redraw");
    }
    assert_eq!(app.surf_stats.gpu, 2, "{:?}", app.surf_stats);
    assert_eq!(app.surf_stats.held, 39, "{:?}", app.surf_stats);
}

#[test]
fn micro_state_is_pixel_exact() {
    for ring in RINGS {
    check(
        "micro state",
        app_ring(MicroState { counter: Signal::new(0) }, micro_config("state", 2.0), ring),
        300,
        11,
        |a, t| {
            a.write(|s| {
                s.counter.set(t as u64 * 37);
            });
            a.flush();
        },
        || app_ring(MicroState { counter: Signal::new(300 * 37) }, micro_config("state", 2.0), ring),
    );
    }
}

#[test]
fn micro_vscroll_is_pixel_exact_through_the_wrap() {
    use zgx_workload::micro::*;
    let end = ROWS as f32 * FIXED_ROW - VIEWPORT;
    // 12 pt steps, then a stretch near the end, then the wrap back to the top, then a few more steps
    let mut offs: Vec<f32> = (1..=120).map(|t| t as f32 * STEP).collect();
    offs.extend([5000.0, 5012.0, end - 24.0, end - 12.0, end, 0.0, 12.0, 24.0]);
    let n = offs.len();
    for ring in RINGS {
    check(
        "micro vscroll",
        app_ring(MicroScroll { scroll: Signal::new(0.0) }, micro_config("vscroll", 2.0), ring),
        n,
        3,
        |a, t| {
            a.write(|s| {
                s.scroll.set(offs[t - 1]);
            });
            a.flush();
        },
        || app_ring(MicroScroll { scroll: Signal::new(*offs.last().unwrap()) }, micro_config("vscroll", 2.0), ring),
    );
    }
}

#[test]
fn trim_to_purgeable_and_resume_is_pixel_exact() {
    // offscreen the 4-slot ring is round-robin, so every slot holds pixels; a calm trim makes all but the
    // displayed one purgeable-volatile; drawing on (slots made resident again, contents intact) stays exact
    let mut work = Workload::from_env();
    work.mode = Mode::Both;
    let mut app = App::new(Lab::new(&work), lab_config(2.0), true);
    app.frame();
    let mut trimmed = 0;
    for t in 1..=160 {
        work.tick();
        app.turn(|l| lab_publish(l, &work, Mode::Both));
        let drew = app.frame();
        if t == 60 || t == 120 {
            assert!(app.needs_trim());
            assert!(app.trim(false), "nothing was trimmed");
            trimmed += 1;
            assert!(!app.needs_trim());
        }
        if drew && t % 5 == 0 {
            let (px, r) = (app.presented_pixels(), app.reference_pixels());
            assert!(px == r, "tick {t}: presented != full redraw after a trim at {:?}", first_diff(&px, &r, 1920));
        }
    }
    assert_eq!(trimmed, 2);
    assert_eq!(app.surf_stats.purged, 0, "no memory pressure here: volatile surfaces keep their pixels");
}

#[test]
fn animate_in_throughput_mode_is_pixel_exact() {
    // the pre-armed command stream: frames released by a shared event, published one turn later
    let times: Vec<f32> = (1..=120).map(|f| f as f32 / 120.0).collect();
    for ring in [2, 3, 4, 0] {
        let mut app = app_ring(Anim { clock: Signal::new(0.0) }, anim_config(2.0), ring);
        app.frame();
        if ring == 3 {
            // tiny bins: an overflow inside the pre-armed stream drains it, grows the bins, redraws, re-arms
            app.reset_bins(4);
        }
        app.stream(8);
        for (t, &c) in times.iter().enumerate() {
            app.turn(|x| {
                x.clock.set(c);
            });
            assert!(app.frame());
            if t % 9 == 0 {
                let (px, r) = (app.presented_pixels(), app.reference_pixels());
                assert!(px == r, "ring {ring} frame {t}: presented != full redraw");
            }
        }
        // the presenter's Metal kernels only: their row bins (the kernel backend's tile lists have no capacity)
        #[cfg(target_os = "macos")]
        if ring == 3 && app.repair == bough::app::Repair::Gpu {
            assert!(app.gpu.as_ref().unwrap().bin_growths > 0);
        }
        let px = app.presented_pixels();
        let mut f = app_ring(Anim { clock: Signal::new(*times.last().unwrap()) }, anim_config(2.0), ring);
        f.frame();
        assert!(px == f.presented_pixels(), "ring {ring}: retained != fresh mount");
    }
}
