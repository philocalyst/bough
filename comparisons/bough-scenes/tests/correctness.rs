//! Pixel-exactness of every scene, as for heavy: (1) after every `every`-th drawn frame the presented
//! surface (read back from the IOSurface the ring drew into) equals a from-scratch full redraw of the same
//! retained state (damage -> repair -> cull -> stencil is byte exact, ring ages 1..4 included); (2) at the
//! end it equals a fresh mount of the view at the same signal values (the wires leave the pixels a
//! from-scratch description would); (3) a turn that changes nothing draws nothing.
use bough::prelude::*;
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
    let mut checked = 0;
    for t in 1..=steps {
        step(&mut app, t);
        let drew = app.frame();
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

fn lab(mode: Mode, ticks: usize, every: usize) {
    let mut work = Workload::from_env();
    work.mode = mode;
    let app = App::new(Lab::new(&work), lab_config(2.0), true);
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
    lab(Mode::Stream, 240, 7);
}
#[test]
fn lab_scroll_is_pixel_exact() {
    lab(Mode::Scroll, 240, 7);
}
#[test]
fn lab_both_is_pixel_exact() {
    lab(Mode::Both, 400, 9);
}

#[test]
fn lab_scroll_jumps_are_pixel_exact() {
    let jumps = [14.0f32, 200.0, 1000.0, 1001.0, 5000.0, 123456.0, 14.0, 0.0, 777.5, 2_799_600.0, 2_799_586.0, 0.0];
    let mk = |s: f32| {
        let work = Workload::from_env();
        let mut l = Lab::new(&work);
        l.scroll.set(s);
        App::new(l, lab_config(2.0), true)
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

#[test]
fn animate_is_pixel_exact() {
    // every 120 Hz refresh for two seconds, then irregular jumps (a dropped-frame clock)
    let times: Vec<f32> = (1..=240).map(|f| f as f32 / 120.0).chain([2.5, 2.51, 7.3, 7.31, 100.0, 0.0, 3.4]).collect();
    let n = times.len();
    check(
        "animate",
        App::new(Anim { clock: Signal::new(0.0) }, anim_config(2.0), true),
        n,
        5,
        |a, t| {
            a.turn(|x| {
                x.clock.set(times[t - 1]);
            })
        },
        || App::new(Anim { clock: Signal::new(*times.last().unwrap()) }, anim_config(2.0), true),
    );
}

#[test]
fn blink_is_pixel_exact_and_idle_between_flips_is_free() {
    let mut app = App::new(Blink { shown: Signal::new(true) }, blink_config(2.0), true);
    assert!(app.frame());
    let (f0, t0) = (app.frames, app.rt.ctr.text_shaped);
    for _ in 0..50 {
        app.turn(|_| {});
        assert!(!app.frame(), "an unchanged turn drew");
    }
    assert_eq!((app.frames, app.rt.ctr.text_shaped), (f0, t0));
    let n = check(
        "blink",
        App::new(Blink { shown: Signal::new(true) }, blink_config(2.0), true),
        31,
        1,
        |a, _| {
            a.turn(|b| {
                let s = !*b.shown;
                b.shown.set(s);
            })
        },
        || App::new(Blink { shown: Signal::new(false) }, blink_config(2.0), true),
    );
    assert!(n >= 30);
}

#[test]
fn micro_state_is_pixel_exact() {
    check(
        "micro state",
        App::new(MicroState { counter: Signal::new(0) }, micro_config("state", 2.0), true),
        300,
        11,
        |a, t| {
            a.write(|s| {
                s.counter.set(t as u64 * 37);
            });
            a.flush();
        },
        || App::new(MicroState { counter: Signal::new(300 * 37) }, micro_config("state", 2.0), true),
    );
}

#[test]
fn micro_vscroll_is_pixel_exact_through_the_wrap() {
    use zgx_workload::micro::*;
    let end = ROWS as f32 * FIXED_ROW - VIEWPORT;
    // 12 pt steps, then a stretch near the end, then the wrap back to the top, then a few more steps
    let mut offs: Vec<f32> = (1..=120).map(|t| t as f32 * STEP).collect();
    offs.extend([5000.0, 5012.0, end - 24.0, end - 12.0, end, 0.0, 12.0, 24.0]);
    let n = offs.len();
    check(
        "micro vscroll",
        App::new(MicroScroll { scroll: Signal::new(0.0) }, micro_config("vscroll", 2.0), true),
        n,
        3,
        |a, t| {
            a.write(|s| {
                s.scroll.set(offs[t - 1]);
            });
            a.flush();
        },
        || App::new(MicroScroll { scroll: Signal::new(*offs.last().unwrap()) }, micro_config("vscroll", 2.0), true),
    );
}
