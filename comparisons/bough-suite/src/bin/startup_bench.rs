//! zgx-gpu `startup_bench`, native: what a renderer costs before its first frame. The fused GPU presenter
//! (device, pipelines, the shared region) and the text system for 1920 x 1440 at scale 1 (`App::new` of an
//! empty view with `gpu = true`), then shaping "zgx performance lab" at 14 pt. bough carries one font
//! file (the shaping tables come from it): zgx's four styles (bold, italic, monospace) do not exist, so one
//! line is shaped. Count instructions with `/usr/bin/time -l`.
use bough::prelude::*;
use bough_suite::*;
use std::time::Instant;

struct Empty;
impl View for Empty {
    const FIXED: bool = true;
    fn view(&self) -> impl Element<Self> + '_ {}
}

fn main() {
    let start = Instant::now();
    let mut a = App::new(Empty, Config::new(1920.0, 1440.0, 1.0, font()), true);
    let created = start.elapsed();
    let mut run = bough_text::GlyphRun::default();
    a.rt.text.shape_run("zgx performance lab", 14.0, &mut run);
    std::hint::black_box(&run);
    eprintln!("renderer {created:?}, shaped {:?} (one style: bold, italic and monospace are not available)", start.elapsed() - created);
}
