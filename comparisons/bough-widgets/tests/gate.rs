//! Pixel gates for the widget benches: after every drawn frame of a hover sweep (and of a click-and-type
//! sequence) the presented surface equals a full redraw of the same retained state, at ring ages 1..=4 and
//! the windowed ring; at the end the retained app equals a fresh mount with the same pointer position
//! (hover state) / the same final events.
#![cfg(target_os = "macos")]
mod common;
use bough::prelude::*;
use bough_widgets::*;
use common::*;

const N: usize = 40;

fn cfg() -> Config {
    config(1200.0, 800.0, N, 12, 40)
}

fn mk(xs: &[f32]) -> Widgets {
    Widgets::new(N, xs)
}

/// zgx's sweep (x = 20, y over the first 20 rows), then the pointer across the buttons and the input of a
/// row, onto the empty right side, out of the window, and back onto a button.
fn path(t: usize, xs: &[f32]) -> Option<(f32, f32)> {
    let row = |k: usize| k as f32 * ROW_H + 19.0;
    Some(match t {
        1..=60 => (20.0, hover_y(t - 1)),
        61 => (140.0, row(3)),
        62 => (xs[0] + 40.0, row(3)),
        63 => (900.0, row(3)),
        64 => return None,
        65 => (30.0, row(30)),
        66 => (xs[0] + 40.0, row(30)),
        67 => (20.0, row(5) - 15.0),
        _ => (20.0, hover_y(t)),
    })
}

#[test]
fn widget_hover_sweep_is_pixel_exact() {
    let xs = field_xs(N);
    let steps = 80;
    let last = path(steps, &xs);
    for ring in RINGS {
        let fresh = || {
            let mut a = app_ring(mk(&xs), cfg(), ring);
            a.frame();
            if let Some((x, y)) = last {
                a.pointer_move(x, y);
            }
            a
        };
        let px = check(
            "widgets hover",
            app_ring(mk(&xs), cfg(), ring),
            steps,
            1,
            |a, t| match path(t, &xs) {
                Some((x, y)) => a.pointer_move(x, y),
                None => a.pointer_leave(),
            },
            fresh,
        );
        // the pointer's button is drawn in the hover colour, its neighbour row's at rest (not just consistently)
        let (x, y) = last.unwrap();
        let rgb = |x: usize, y: usize| {
            let at = (y * 1200 + x) * 4;
            let mut c = [px[at], px[at + 1], px[at + 2]];
            c.sort();
            c
        };
        assert_eq!(rgb(x as usize, y as usize), [0x2f, 0x40, 0x58], "ring {ring}: hovered button colour");
        assert_eq!(rgb(x as usize, y as usize + ROW_H as usize), [0x23, 0x2e, 0x40], "ring {ring}: resting button colour");
    }
}

#[test]
fn widget_click_and_type_is_pixel_exact() {
    let xs = field_xs(N);
    let (fx, fy) = (xs[0] + 60.0, 5.0 * ROW_H + 18.0);
    let events = |a: &mut App<Widgets>| {
        a.pointer_move(fx, fy);
        a.frame();
        a.pointer_down(fx, fy, Button::Primary);
        a.frame();
        a.pointer_up(fx, fy, Button::Primary);
        a.frame();
        for c in "Ada".chars() {
            a.key_down(Key::Char(c), Mods::default());
            a.frame();
        }
    };
    for ring in RINGS {
        let mut a = app_ring(mk(&xs), cfg(), ring);
        let w = 1200;
        a.frame();
        // hover a button, click it (no route: nothing happens), move to the field, click, type, backspace
        for (i, &(x, y)) in [(20.0, 19.0), (20.0, 19.0 + ROW_H), (fx, fy)].iter().enumerate() {
            a.pointer_move(x, y);
            a.frame();
            let (px, rf) = (a.presented_pixels(), a.reference_pixels());
            assert!(px == rf, "ring {ring} move {i}: presented != full redraw at {:?}", first_diff(&px, &rf, w));
        }
        a.pointer_down(fx, fy, Button::Primary);
        a.frame();
        a.pointer_up(fx, fy, Button::Primary);
        a.frame();
        for (i, c) in "Ada L".chars().enumerate() {
            a.key_down(Key::Char(c), Mods::default());
            a.frame();
            let (px, rf) = (a.presented_pixels(), a.reference_pixels());
            assert!(px == rf, "ring {ring} key {i}: presented != full redraw at {:?}", first_diff(&px, &rf, w));
        }
        a.key_down(Key::Backspace, Mods::default());
        a.frame();
        a.key_down(Key::Backspace, Mods::default());
        a.frame();
        let (px, rf) = (a.presented_pixels(), a.reference_pixels());
        assert!(px == rf, "ring {ring} backspace: presented != full redraw at {:?}", first_diff(&px, &rf, w));
        assert_eq!(*a.owner.values.v5, "Ada");
        // the typed text is on screen: pixels inside the field's box (clear of its border) other than its fill
        let px = a.presented_pixels();
        let fill = |x: usize, y: usize| {
            let at = (y * w + x) * 4;
            [px[at], px[at + 1], px[at + 2]]
        };
        let (x0, y0) = (xs[0] as usize, 5 * ROW_H as usize);
        let bg = fill(x0 + 150, y0 + 18);
        let inked = (y0 + 4..y0 + 32).flat_map(|y| (x0 + 8..x0 + 60).map(move |x| (x, y))).filter(|&(x, y)| fill(x, y) != bg).count();
        assert!(inked > 20, "ring {ring}: the typed text is drawn in row 5's field ({inked} pixels differ from the fill)");
        // a fresh mount given only the final events
        let mut f = app_ring(mk(&xs), cfg(), ring);
        f.frame();
        events(&mut f);
        f.frame();
        let fp = f.presented_pixels();
        let px = a.presented_pixels();
        assert!(px == fp, "ring {ring}: retained != fresh mount, first diff {:?}", first_diff(&px, &fp, w));
    }
}

/// callback_mount_bench's rows (inputs overlapping by 8 pt): each step clicks into the next input and types
/// a letter; the retained app is compared with a fresh mount that got the same events without frames between.
#[test]
fn callback_rows_are_pixel_exact() {
    let c = || config(1200.0, 800.0, 40, 8, 40);
    let step = |a: &mut App<CallbackRows>, t: usize| {
        let (x, y) = (20.0 + t as f32, 28.0 * t as f32 + 10.0);
        a.pointer_move(x, y);
        a.pointer_down(x, y, Button::Primary);
        a.pointer_up(x, y, Button::Primary);
        a.key_down(Key::Char((b'a' + t as u8) as char), Mods::default());
    };
    for ring in RINGS {
        check("callback rows", app_ring(CallbackRows::new(40), c(), ring), 12, 1, step, || {
            let mut a = app_ring(CallbackRows::new(40), c(), ring);
            a.frame();
            for t in 1..=12 {
                step(&mut a, t);
            }
            a
        });
    }
}
