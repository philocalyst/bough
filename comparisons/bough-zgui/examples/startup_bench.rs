//! zgx-gpu's `startup_bench` on bough: what a renderer costs before its first frame, headless: the GPU
//! device and pipelines (`Fused::new` inside `App::new`, 1920 x 1440), the font (read, simple-script
//! tables, glyph cache), and shaping "zgx performance lab" at 14 px in four styles (bough ships one face,
//! so the four styles are four shapes of the same run: no font matching). Count instructions with
//! `/usr/bin/time -l` (process-wide, as zgx's suite does).
use bough::raw::*;
use std::time::Instant;

struct Empty;
impl View for Empty {
    fn view(&self) -> impl Element<Self> + '_ {}
}

fn main() {
    let start = Instant::now();
    let font = bough_zgui::font_bytes();
    let mut app = App::new(Empty, Config::new(960.0, 720.0, 2.0, font), true);
    let created = start.elapsed();
    let mut run = bough_text::GlyphRun::default();
    for size in [14.0f32, 14.0, 14.0, 14.0] {
        app.rt.text.shape_run("zgx performance lab", size * 2.0, &mut run);
        std::hint::black_box(&run);
    }
    eprintln!("renderer {created:?}, shaped {:?}", start.elapsed() - created);
}
