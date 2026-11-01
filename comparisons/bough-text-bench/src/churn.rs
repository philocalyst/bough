//! zgui's `label-churn` probe (`crates/zgui-bench/src/bin/label-churn.rs`) as a bough view: thirty absolutely
//! positioned 11 px labels on a 1000 x 700 sheet, the first half along the bottom edge, the rest up the left
//! edge, every one relabelled each frame (`format!("{value:.2}")` of a value that differs at every step and
//! from every other label).
use bough::prelude::*;

pub const WIDTH: f32 = 1000.0;
pub const HEIGHT: f32 = 700.0;
pub const LABELS: usize = 30;

/// zgui's label text at one zoom step.
pub fn label(zoom: usize, index: usize) -> String {
    let value = ((zoom * 7_919 + index * 104_729) % 200_000) as f64 / 1_000.0 - 100.0;
    format!("{value:.2}")
}

/// zgui's placement of label `index` of `labels` (CSS px from the plot's top left).
pub fn place(index: usize, labels: usize) -> (f32, f32) {
    if index < labels / 2 { ((40 + index * 56 % 840) as f32, 580.0) } else { (4.0, ((index - labels / 2) * 38 % 570) as f32) }
}

pub struct Plot {
    pub zoom: Signal<usize>,
}

impl View for Plot {
    fn view(&self) -> impl Element<Self> + '_ {
        (
            quad(0.0, 0.0, WIDTH, HEIGHT).bg(rgb(0x14161a)),
            each((0..LABELS).map(|i| {
                let (x, y) = place(i, LABELS);
                text(sig!(self.zoom).map(move |z| label(*z, i))).at(x, y).size(11.0).line_height(13.0).color(rgb(0xe7ecf5))
            })),
        )
    }
}

pub fn config(font: Vec<u8>) -> Config {
    let mut c = Config::new(WIDTH, HEIGHT, 1.0, font);
    c.ring = std::env::var("BOUGH_RING").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    c
}
