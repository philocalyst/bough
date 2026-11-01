//! Where a process's first `App::new` goes (instructions): font tables, glyph cache, faces, region, mount.
use bough_suite::*;
fn main() {
    let font = font();
    let i0 = instructions();
    let script = bough_text::SimpleScript::build(&font, 0).unwrap();
    let i1 = instructions();
    let sh = bough_text::Shaper::new(script);
    let i2 = instructions();
    let gc = bough_text::atlas::GlyphCache::new(&font);
    let i3 = instructions();
    let f = rustybuzz::Face::from_slice(&font, 0).unwrap();
    let i4 = instructions();
    let t = ttf_parser::Face::parse(&font, 0).unwrap();
    let i5 = instructions();
    std::hint::black_box((&sh, &gc, &f, &t));
    println!("SimpleScript::build {:.1}M Shaper::new {:.1}M GlyphCache::new {:.1}M rustybuzz face {:.1}M ttf face {:.1}M", (i1 - i0) as f64 / 1e6, (i2 - i1) as f64 / 1e6, (i3 - i2) as f64 / 1e6, (i4 - i3) as f64 / 1e6, (i5 - i4) as f64 / 1e6);
    let i6 = instructions();
    struct E;
    impl bough::prelude::View for E { const FIXED: bool = true; fn view(&self) -> impl bough::prelude::Element<Self> + '_ {} }
    let a = app(E, 1200.0, 900.0, false);
    let i7 = instructions();
    let b = app(E, 1200.0, 900.0, false);
    let i8 = instructions();
    std::hint::black_box((&a, &b));
    println!("first App::new {:.1}M second {:.1}M", (i7 - i6) as f64 / 1e6, (i8 - i7) as f64 / 1e6);
}
