//! Where the gallery's cold first frame goes: font read, simple-script tables, rustybuzz face, glyph
//! cache, the mount (template + shaping), the first frame. Instructions and wall time per phase.
use bough::raw::*;
use bough_zgui::gallery::Gallery;
use bough_zgui::*;

fn snap() -> (u64, std::time::Instant) {
    (bough::sys::perf::snap().instrs, std::time::Instant::now())
}
fn show(name: &str, a: (u64, std::time::Instant)) -> (u64, std::time::Instant) {
    let b = snap();
    println!("{name:>28}: {:>12} instr {:>9.3} ms", b.0 - a.0, (b.1 - a.1).as_secs_f64() * 1e3);
    b
}

fn main() {
    let t = snap();
    let font = font_bytes();
    let t = show("font read", t);
    let s = bough_text::SimpleScript::build(&font, 0).unwrap();
    let t = show("SimpleScript::build", t);
    let _ = std::hint::black_box(s);
    let c = bough_text::atlas::GlyphCache::new(&font);
    let t = show("GlyphCache::new", t);
    let _ = std::hint::black_box(c);
    let rb = rustybuzz::Face::from_slice(&font, 0).unwrap();
    let t = show("rustybuzz face", t);
    let _ = std::hint::black_box(rb);
    let t0 = t;
    let mut app = App::new(Gallery::new(), config(WIDTH, HEIGHT), false);
    show("App::new (all of it)", t);
    eprintln!("glyphs rasterized {} in {:.3} ms", app.rt.text.cache.rasterized, app.rt.text.cache.raster_ns as f64 / 1e6);
    let t = snap();
    app.frame();
    show("first frame", t);
    show("App::new + frame", t0);
}
