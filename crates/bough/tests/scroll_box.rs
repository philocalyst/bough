//! The prelude's scroll box (`.scroll(sig!(..))`): two defects the generalization lane found through gui-bench
//! (OVERFIT 1 and 8), reduced to the smallest scenes that show them. Each compares the retained window with a
//! fresh mount of the same state (same scroll offset, same pointer), pixel for pixel.
//!
//! * Hover after a scroll: the pointer stays still while the content moves under it; the row now under the
//!   pointer must be the hovered one (before the fix the row that had been under it stayed highlighted).
//! * Plain state inside a scroll box: a handler changes a plain field the description reads inside the scroll
//!   box; the re-description must reach it (before the fix the scroll box was opaque to re-description, so the
//!   window kept its first state).
use bough::prelude::*;
use bough::raw::{App, Config};

const W: f32 = 320.0;
const H: f32 = 240.0;

fn cfg() -> Config {
    let font = std::fs::read(std::env::var("ZGX_FONT_FILE").unwrap_or_else(|_| "/System/Library/Fonts/Supplemental/Arial.ttf".into())).unwrap();
    Config::new(W, H, 1.0, font)
}

struct Hovered {
    y: Signal<f32>,
}

impl View for Hovered {
    fn view(&self) -> impl Element<Self> + '_ {
        div().w_pct(1.0).h_pct(1.0).bg(rgb(0xffffff)).scroll(sig!(self.y)).child(each((0..40).map(|i| {
            block(0.0, 32.0).w_pct(1.0).shrink(0.0).bg(rgb(if i % 2 == 0 { 0xffffff } else { 0xf6f8fa })).hover_bg(rgb(0xcfe3ff))
        })))
    }
}

fn hovered_at(y: f32, pointer: (f32, f32)) -> Vec<u8> {
    let mut a = App::new(Hovered { y: Signal::new(y) }, cfg(), true);
    a.frame();
    a.pointer_move(pointer.0, pointer.1);
    a.frame();
    a.presented_pixels()
}

#[test]
fn hover_follows_the_content_under_a_still_pointer() {
    let p = (100.0, 100.0);
    let mut a = App::new(Hovered { y: Signal::new(0.0) }, cfg(), true);
    a.frame();
    a.pointer_move(p.0, p.1);
    a.frame();
    // three rows (96 pt) under the pointer, in steps a wheel would take
    for _ in 0..6 {
        a.wheel_points(p.0, p.1, 16.0);
        a.frame();
    }
    let fresh = hovered_at(96.0, p);
    let retained = a.presented_pixels();
    let bad = retained.iter().zip(&fresh).filter(|(x, y)| x != y).count();
    assert!(bad == 0, "after the scroll {bad} bytes differ from a fresh mount with the pointer at the same point (a stale hover)");
}
