//! Upstream parley's cost per operation (instructions), the baseline the grafts start from.
#[path = "common/meter.rs"]
mod meter;
#[path = "common/corpus.rs"]
mod corpus;

use parley::fontique::{Blob, Collection, CollectionOptions, SourceCache};
use parley::{FontContext, FontFamily, Layout, LayoutContext, StyleProperty, Alignment, AlignmentOptions};
use std::sync::Arc;

fn lay(fcx: &mut FontContext, lcx: &mut LayoutContext<()>, text: &str, size: f32, w: Option<f32>) -> Layout<()> {
    let mut b = lcx.ranged_builder(fcx, text, 1.0, true);
    b.push_default(StyleProperty::FontFamily(FontFamily::named("DejaVu Sans")));
    b.push_default(StyleProperty::FontSize(size));
    let mut l: Layout<()> = b.build(text);
    l.break_all_lines(w);
    l.align(Alignment::Start, AlignmentOptions::default());
    l
}

fn main() {
    let mut fcx = FontContext {
        collection: Collection::new(CollectionOptions { shared: false, system_fonts: false }),
        source_cache: SourceCache::default(),
    };
    fcx.collection.register_fonts(Blob::new(Arc::new(corpus::font_bytes().to_vec())), None);
    let mut lcx = LayoutContext::<()>::new();
    let label = "12,345 req/s";
    let log = corpus::heavy_log_lines(3).pop().unwrap();
    let para = "The quick brown fox jumps over the lazy dog while streaming tokens across the wire; \
        every paragraph is measured at two widths, wrapped greedily, and drawn once per frame. ".repeat(2);
    if let Ok(which) = std::env::var("SPIN") {
        let (text, size, w) = match which.as_str() { "label" => (label.to_string(), 12.0, None), "log" => (log.clone(), 12.0, None), _ => (para.clone(), 14.0, Some(400.0)) };
        let t = std::time::Instant::now();
        while t.elapsed().as_secs() < 6 { for _ in 0..100 { std::hint::black_box(lay(&mut fcx, &mut lcx, &text, size, w)); } }
        return;
    }
    for (name, text, size, w) in [("label", label.to_string(), 12.0, None), ("log line", log.clone(), 12.0, None), ("paragraph", para.clone(), 14.0, Some(400.0))] {
        let _ = lay(&mut fcx, &mut lcx, &text, size, w);
        let c = meter::count(20, || { std::hint::black_box(lay(&mut fcx, &mut lcx, &text, size, w)); });
        let mut l = lay(&mut fcx, &mut lcx, &text, size, w);
        let rb = meter::count(20, || { l.break_all_lines(w.map(|w| w - 37.0)); l.break_all_lines(w); });
        println!("{name:10} {:4} B  build+break+align {:9.0} instr ({:6.1}/B, {:7.0} ns)   re-break x2 {:8.0}  glyphs {}",
            text.len(), c.instr, c.instr / text.len() as f64, c.ns, rb.instr,
            l.lines().map(|ln| ln.runs().map(|r| r.clusters().count()).sum::<usize>()).sum::<usize>());
    }
}

#[allow(dead_code)]
pub fn spin() {}
