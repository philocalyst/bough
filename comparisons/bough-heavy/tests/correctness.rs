//! Pixel-exactness of the fused pipeline on zgx's heavy scene, per mode:
//!  1. every `EVERY` ticks the presented surface (read back from the IOSurface the ring drew into) equals a
//!     from-scratch full redraw of the same retained state through the same kernels: damage marks (wires,
//!     text patches, scroll_run, structure) -> repair(age) -> tile list -> GPU cull of the core pages ->
//!     repair is byte exact, for EVERY ring age 1..=4 (`ring` surfaces drawn round-robin: age = ring);
//!  2. at the end it equals a *fresh mount* of the view at the same signal values;
//!  3. the numeric operands render exactly zgx's `format!` strings;
//!  4. idle is zero work.
use bough::prelude::*;
use bough_heavy::*;
use zgx_workload::heavy::*;
use zgx_workload::Mode;

fn first_diff(a: &[u8], b: &[u8]) -> Option<(usize, usize, u8, u8)> {
    a.iter().zip(b).position(|(x, y)| x != y).map(|i| ((i / 4) % 2560, (i / 4) / 2560, a[i], b[i]))
}

fn cfg(ring: usize) -> Config {
    let mut c = config(2.0);
    c.ring = ring;
    c
}

fn check(mode: Mode, ticks: usize, every: usize, ring: usize) {
    let mut work = Heavy::from_env();
    work.mode = mode;
    let mut app = App::new(Dash::new(&work), cfg(ring), true);
    app.frame();
    let mut checked = 0;
    for t in 1..=ticks {
        work.tick();
        app.turn(|d| publish(d, &work, mode));
        let drew = app.frame();
        assert!(!app.bins_overflowed(), "row bin overflow at tick {t}");
        if drew && t % every == 0 {
            let px = app.presented_pixels();
            let reference = app.reference_pixels();
            assert!(px == reference, "{mode:?} ring {ring} tick {t}: presented != full redraw at {:?}", first_diff(&px, &reference));
            checked += 1;
        }
    }
    assert!(checked > 0 || mode == Mode::Idle);
    if mode != Mode::Idle {
        let px = app.presented_pixels();
        let mut fresh = App::new(Dash::new(&work), cfg(ring), true);
        fresh.frame();
        let fp = fresh.presented_pixels();
        assert!(px == fp, "{mode:?} ring {ring}: retained after {ticks} ticks != fresh mount, first diff {:?}", first_diff(&px, &fp));
    }
}

#[test]
fn stream_is_pixel_exact_at_every_ring_age() {
    for ring in 1..=4 {
        check(Mode::Stream, 120, 7, ring);
    }
}
#[test]
fn scroll_is_pixel_exact_at_every_ring_age() {
    for ring in 1..=4 {
        check(Mode::Scroll, 120, 7, ring);
    }
}
#[test]
fn both_is_pixel_exact_at_every_ring_age() {
    for ring in 1..=4 {
        check(Mode::Both, 200, 9, ring);
    }
}
#[test]
fn scroll_across_many_rows_and_a_jump_is_pixel_exact() {
    // large scroll steps: whole windows of rows unmount/mount at once, then a jump back to the top
    for ring in [1, 3] {
        let work = Heavy::from_env();
        let mut app = App::new(Dash::new(&work), cfg(ring), true);
        app.frame();
        for (i, s) in [0.0f32, 14.0, 200.0, 1000.0, 1001.0, 5000.0, 123456.0, 14.0, 0.0, 777.5].iter().enumerate() {
            app.turn(|d| {
                d.scroll.set(*s);
            });
            if app.frame() {
                let (px, r) = (app.presented_pixels(), app.reference_pixels());
                assert!(px == r, "ring {ring} scroll step {i} to {s}: {:?}", first_diff(&px, &r));
            }
        }
    }
}
#[test]
fn numeric_operands_render_zgx_strings() {
    let mut buf = [0u8; 16];
    for live in 0..3000u64 {
        for t in 0..TILES {
            let n = tile_fmt(t).render_f32(tile_v(t, live), &mut buf).or_else(|| tile_fmt(t).render_u32(tile_v(t, live) as u32, &mut buf)).unwrap();
            assert_eq!(bough::NumFmt::as_str(&buf, n), tile_value(t, live), "tile {t} live {live}");
        }
        for c in 0..CARDS {
            let n = MS.render_f32(card_v(c, live), &mut buf).unwrap();
            assert_eq!(bough::NumFmt::as_str(&buf, n), card_value(c, live));
        }
        for r in [0usize, 1, 17, 499] {
            let n = MS.render_f32(latency_v(r, live), &mut buf).unwrap();
            assert_eq!(bough::NumFmt::as_str(&buf, n), row_latency(r, live));
            let n = MBS.render_f32(throughput_v(r, live), &mut buf).unwrap();
            assert_eq!(bough::NumFmt::as_str(&buf, n), row_throughput(r, live));
        }
    }
}
#[test]
fn idle_is_zero_work() {
    let mut work = Heavy::from_env();
    work.mode = Mode::Idle;
    let mut app = App::new(Dash::new(&work), config(2.0), true);
    assert!(app.frame());
    let (w0, f0, t0) = (app.rt.ctr.wires_run, app.frames, app.rt.ctr.text_set);
    for _ in 0..100 {
        work.tick();
        app.turn(|d| publish(d, &work, Mode::Idle));
        assert!(!app.frame(), "idle frame drew");
    }
    assert_eq!((app.rt.ctr.wires_run, app.frames, app.rt.ctr.text_set), (w0, f0, t0));
    // a write that changes nothing is cut off by the signal's equality test
    app.turn(|d| {
        d.live.set(0);
    });
    assert!(!app.frame());
}

