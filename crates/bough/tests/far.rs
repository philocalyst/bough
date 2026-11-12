//! Content far from its scroll origin (round 6, finding 3 of the overfitting map): rows more than 32,766 device
//! px down a scrolled column drew wrong once scrolled into view, because their inks were clamped to the 16-bit
//! window domain when written and a clamped rect cannot be translated back. The oracle is a fresh mount at the
//! same scroll offset.
use bough::prelude::*;
use bough::raw::{App, Caps, Config};

struct Rows {
    n: usize,
    y: Signal<f32>,
}

impl View for Rows {
    fn view(&self) -> impl Element<Self> + '_ {
        let line = |i: usize| {
            let bg = if i % 2 == 0 { rgb(0xf5f5f5) } else { rgb(0xdcdcdc) };
            row((text(i as u32).w(60.0).color(rgb(0x101010)), text("far row").color(rgb(0x202020)))).w(300.0).h(32.0).bg(bg)
        };
        div().w(320.0).h(240.0).bg(rgb(0xffffff)).scroll(sig!(self.y)).child(each((0..self.n).map(line)))
    }
}

fn app(n: usize, y: f32, scale: f32) -> App<Rows> {
    let font = std::fs::read(std::env::var("ZGX_FONT_FILE").unwrap_or_else(|_| "/System/Library/Fonts/Supplemental/Arial.ttf".into())).unwrap();
    let mut cfg = Config::new(320.0, 240.0, scale, font);
    cfg.caps = Caps { instances: 1 << 16, pages: 1 << 13, glyphs: 1 << 19, runs: 1 << 16, spaces: 1 << 12, clips: 1 << 12, strikes: 16 };
    let mut a = App::new(Rows { n, y: Signal::new(y) }, cfg, true);
    a.frame();
    a
}

fn scrolled_equals_fresh(n: usize, scale: f32) {
    let end = n as f32 * 32.0 - 240.0;
    let mut a = app(n, 0.0, scale);
    for y in [end, end - 100.0, end * 0.5, 0.0, end] {
        a.turn(|v| drop(v.y.set(y)));
        a.frame();
        let (retained, presented) = (a.reference_pixels(), a.presented_pixels());
        let fresh = app(n, y, scale).reference_pixels();
        assert!(retained == fresh, "{n} rows at scale {scale}, scrolled to {y}: retained != fresh mount");
        assert!(presented == retained, "{n} rows at scale {scale}, scrolled to {y}: presented != full redraw");
    }
}

#[test]
fn rows_past_the_16_bit_domain_scroll_into_view_exactly() {
    scrolled_equals_fresh(1_000, 1.0); // 32,000 px: inside the domain
    scrolled_equals_fresh(1_100, 1.0); // 35,200 px: past it
    scrolled_equals_fresh(2_500, 1.0); // 80,000 px
    scrolled_equals_fresh(700, 2.0); // 44,800 device px at scale 2
}
