//! layout_bench's label-heavy screen, pixel-exact, on the benchmark's own path (one app, the screen remounted
//! per salt, then the window resized back and forth): after each remount and after each resize the presented
//! surface equals a full redraw (cropped to the window: the surfaces keep the size they were made at), at
//! every ring age 1..=4 and the windowed ring; after the sequence the page equals a fresh mount of the
//! screen at the final width.
#![cfg(target_os = "macos")]
mod common;
use bough::raw::*;
use bough_suite::*;
use common::*;

/// Resize steps (zgx's sawtooth from 1400 down to 925 points and back; the last width is 1075).
const RESIZES: usize = 33;

fn crop(px: &[u8], full_w: usize, w: usize, h: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        out.extend_from_slice(&px[y * full_w * 4..y * full_w * 4 + w * 4]);
    }
    out
}

fn assert_exact<V: View>(a: &mut App<V>, what: &str) {
    assert!(!a.bins_overflowed(), "{what}: row bin overflow");
    let full_w = a.surface_width();
    let (w, h) = (a.rt.win_w as usize, a.rt.win_h as usize);
    let px = crop(&a.presented_pixels(), full_w, w, h);
    let r = crop(&a.reference_pixels(), full_w, w, h);
    assert!(px == r, "{what}: presented != full redraw at {:?}", first_diff(&px, &r, w));
}

#[test]
fn layout_screen_remounts_and_resizes_are_pixel_exact() {
    for ring in RINGS {
        let name = format!("layout screen (ring {ring})");
        let mut a = app_ring(LayoutHost::new(), lb_config(LB_W), ring);
        a.frame();
        for salt in 1..=3 {
            lb_unmount(&mut a);
            a.turn(|v| {
                v.salt = salt;
                v.show = true;
            });
            assert!(a.frame());
            assert_exact(&mut a, &format!("{name} mount {salt}"));
        }
        for step in 1..=RESIZES {
            a.resize(lb_width(step), LB_H);
            assert!(a.frame());
            assert_exact(&mut a, &format!("{name} resize {step} to {}", lb_width(step)));
        }
        let (w, h) = (a.rt.win_w as usize, a.rt.win_h as usize);
        assert_eq!(w as f32, lb_width(RESIZES) * 2.0);
        let px = crop(&a.presented_pixels(), a.surface_width(), w, h);
        let mut f = app_ring(LayoutScreen { salt: 3 }, lb_config(lb_width(RESIZES)), ring);
        f.frame();
        let fp = crop(&f.presented_pixels(), f.surface_width(), w, h);
        assert!(px == fp, "{name}: retained after {RESIZES} resizes != fresh mount at {:?}", first_diff(&px, &fp, w));
    }
}

/// The remounted screen is the screen: the host's first mount draws what a fixed mount of the same salt
/// draws.
#[test]
fn layout_host_mount_equals_fixed_mount() {
    let mut a = App::new(LayoutHost::new(), lb_config(LB_W), true);
    a.frame();
    a.turn(|v| {
        v.salt = 7;
        v.show = true;
    });
    a.frame();
    let mut f = App::new(LayoutScreen { salt: 7 }, lb_config(LB_W), true);
    f.frame();
    let (px, fp) = (a.presented_pixels(), f.presented_pixels());
    if let Some(dir) = std::env::var_os("BOUGH_DUMP") {
        // BGRA, 2800 x 1800: for looking at the screen (`DUMP=dir`)
        std::fs::write(std::path::Path::new(&dir).join("layout_screen.bgra"), &px).unwrap();
    }
    assert!(px == fp, "host != fixed mount at {:?}", first_diff(&px, &fp, a.rt.win_w as usize));
}
