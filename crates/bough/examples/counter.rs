//! The smallest bough app: a counter.
//!
//!   cargo run --release -p bough --example counter       # a window; click "+1"
//!   cargo test --release -p bough --test counter         # headless: three clicks, the count, the pixels
use bough::prelude::*;

pub struct Counter {
    pub count: Signal<i64>,
}

impl View for Counter {
    // Described once at mount; afterwards only the wires below run.
    const FIXED: bool = true;

    fn view(&self) -> impl Element<Self> + '_ {
        (
            quad(0.0, 0.0, 240.0, 120.0).bg(rgb(0x14161a)),
            // A wire: when `count` changes, only this label's changed digits are re-shaped and repainted.
            text(sig!(self.count).map(|n| format!("Count: {n}"))).at(24.0, 20.0).size(20.0).color(rgb(0xe7ecf5)),
            quad(24.0, 64.0, 72.0, 32.0)
                .bg(rgb(0x2f6bff))
                .rounded(8.0)
                .children(text("+1").at(26.0, 7.0).size(16.0).color(rgb(0xffffff)))
                .hover_bg(rgb(0x4a80ff))
                // A typed route: a plain `&mut Counter`. The write marks `count`, and its wire runs.
                .on_click(|c: &mut Counter| *c.count += 1),
        )
    }
}

pub fn config(scale: f32) -> Config {
    let font = std::env::var("ZGX_FONT_FILE").unwrap_or_else(|_| "/System/Library/Fonts/Supplemental/Arial.ttf".into());
    Config::new(240.0, 120.0, scale, std::fs::read(font).expect("a TrueType font (set ZGX_FONT_FILE)"))
}

#[allow(dead_code)]
fn main() {
    bough::backend::host::run_window("Counter", config, Counter { count: Signal::new(0) }, |_| {}, None, 0.0, || {});
}
