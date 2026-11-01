//! Which font files zgx's text system resolves for text_bench's three styles (sans-serif 400, sans-serif 700,
//! monospace) on this machine: shapes a word in each and prints the face's source.
use cosmic_text::{Attrs, Buffer, Family, Metrics, Shaping, Weight};
fn main() {
    let context = zgx_gpu::GpuContext::new().expect("GPU");
    let fonts = context.text_system();
    let mut fonts = fonts.borrow_mut();
    for (label, family, weight) in [("body", Family::SansSerif, 400), ("bold", Family::SansSerif, 700), ("code", Family::Monospace, 400)] {
        let mut b = Buffer::new(&mut fonts, Metrics::new(14., 17.));
        b.set_text("renderer office", &Attrs::new().family(family).weight(Weight(weight)), Shaping::Advanced, None);
        b.shape_until_scroll(&mut fonts, false);
        let run = b.layout_runs().next().unwrap();
        let g = &run.glyphs[0];
        let face = fonts.db().face(g.font_id).unwrap();
        println!("{label}: {:?} index {} family {:?} weight {:?} glyphs {}", face.source, face.index, face.families, face.weight, run.glyphs.len());
    }
}
