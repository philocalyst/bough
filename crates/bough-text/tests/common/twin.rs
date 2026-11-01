//! The parley twin: the vendored, grafted parley (`parley`) and unmodified upstream parley 0.11.1 from crates.io
//! (`parley_upstream`) driven by the same code on the same input. A graft is correct when every dump is equal:
//! layout size, every line (range, break reason, metrics, offset), every item (run font/size/range/direction,
//! offset, baseline, advance), every glyph (id, x, y, advance as f32 bits) and every cluster (range, advance,
//! boundary and ligature flags).
#![allow(dead_code)]

use std::fmt::Write;
use std::ops::Range;

/// A style property applied to a byte range (or, with `0..usize::MAX`, as the default).
#[derive(Clone, Debug)]
pub enum Prop {
    Family(&'static str),
    /// A CSS font-family list (`"'Webdings', 'DejaVu Sans'"`).
    Families(&'static str),
    Size(f32),
    Weight(f32),
    LetterSpacing(f32),
    WordSpacing(f32),
    /// Font variations as a CSS source string (`"'wght' 700"`).
    Variations(&'static str),
    /// Font features as a CSS source string (`"'liga' 0"`).
    Features(&'static str),
    Locale(&'static str),
}

#[derive(Clone, Debug)]
pub struct Case<'a> {
    pub text: &'a str,
    pub defaults: Vec<Prop>,
    pub spans: Vec<(Range<usize>, Prop)>,
    pub width: Option<f32>,
}

impl<'a> Case<'a> {
    pub fn plain(text: &'a str, family: &'static str, size: f32, width: Option<f32>) -> Self {
        Case { text, defaults: vec![Prop::Family(family), Prop::Size(size)], spans: vec![], width }
    }
}

macro_rules! engine {
    ($m:ident, $p:ident) => {
        pub mod $m {
            use super::{Case, Prop};
            use std::fmt::Write;
            use std::sync::Arc;
            use $p as parley;
            use parley::fontique::{Blob, Collection, CollectionOptions, SourceCache};
            use parley::{Alignment, AlignmentOptions, FontContext, FontFamily, Layout, LayoutContext, PositionedLayoutItem, StyleProperty};

            pub struct Eng {
                pub fcx: FontContext,
                pub lcx: LayoutContext<()>,
            }

            impl Eng {
                /// `fonts`: font files registered in order; `system`: also discover the system's fonts (fallback).
                pub fn new(fonts: &[Vec<u8>], system: bool) -> Self {
                    let mut fcx = FontContext {
                        collection: Collection::new(CollectionOptions { shared: false, system_fonts: system }),
                        source_cache: SourceCache::default(),
                    };
                    for f in fonts {
                        fcx.collection.register_fonts(Blob::new(Arc::new(f.clone())), None);
                    }
                    Eng { fcx, lcx: LayoutContext::new() }
                }

                fn prop(p: &Prop) -> StyleProperty<'static, ()> {
                    match p {
                        Prop::Family(f) => StyleProperty::FontFamily(FontFamily::named(f)),
                        Prop::Families(f) => StyleProperty::FontFamily(FontFamily::Source((*f).into())),
                        Prop::Size(s) => StyleProperty::FontSize(*s),
                        Prop::Weight(w) => StyleProperty::FontWeight(parley::FontWeight::new(*w)),
                        Prop::LetterSpacing(v) => StyleProperty::LetterSpacing(*v),
                        Prop::WordSpacing(v) => StyleProperty::WordSpacing(*v),
                        Prop::Variations(s) => StyleProperty::FontVariations(parley::FontVariations::Source((*s).into())),
                        Prop::Features(s) => StyleProperty::FontFeatures(parley::FontFeatures::Source((*s).into())),
                        Prop::Locale(l) => StyleProperty::Locale(Some(l.parse().expect("locale"))),
                    }
                }

                /// Builder + `build` only (analysis, styles, shaping), no line breaking.
                pub fn build(&mut self, c: &Case) -> Layout<()> {
                    let mut b = self.lcx.ranged_builder(&mut self.fcx, c.text, 1.0, true);
                    for p in &c.defaults {
                        b.push_default(Self::prop(p));
                    }
                    for (r, p) in &c.spans {
                        b.push(Self::prop(p), r.clone());
                    }
                    b.build(c.text)
                }

                pub fn layout(&mut self, c: &Case) -> Layout<()> {
                    let mut b = self.lcx.ranged_builder(&mut self.fcx, c.text, 1.0, true);
                    for p in &c.defaults {
                        b.push_default(Self::prop(p));
                    }
                    for (r, p) in &c.spans {
                        b.push(Self::prop(p), r.clone());
                    }
                    let mut l: Layout<()> = b.build(c.text);
                    l.break_all_lines(c.width);
                    l.align(Alignment::Start, AlignmentOptions::default());
                    l
                }

                pub fn dump(&mut self, c: &Case) -> String {
                    let l = self.layout(c);
                    dump(&l)
                }
            }

            /// A font's identity independent of its collection (blob ids are per-process counters).
            fn fingerprint(d: &[u8]) -> u64 {
                let mut h = d.len() as u64;
                for &b in d.iter().step_by(97).take(4096) {
                    h = (h ^ b as u64).wrapping_mul(0x100000001b3);
                }
                h
            }

            pub fn dump(l: &Layout<()>) -> String {
                let b = |v: f32| v.to_bits();
                let mut o = String::new();
                let _ = writeln!(o, "layout w {:x} fw {:x} h {:x} lines {}", b(l.width()), b(l.full_width()), b(l.height()), l.len());
                for (li, line) in l.lines().enumerate() {
                    let m = line.metrics();
                    let _ = writeln!(
                        o,
                        "line {li} {:?} {:?} asc {:x} desc {:x} lead {:x} lh {:x} base {:x} off {:x} adv {:x} ta {:x} min {:x} max {:x}",
                        line.text_range(), line.break_reason(), b(m.ascent), b(m.descent), b(m.leading), b(m.line_height),
                        b(m.baseline), b(m.offset), b(m.advance), b(m.trailing_whitespace), b(m.inline_min_coord), b(m.inline_max_coord)
                    );
                    for item in line.items() {
                        match item {
                            PositionedLayoutItem::GlyphRun(gr) => {
                                let run = gr.run();
                                let _ = writeln!(
                                    o,
                                    "  run font {} idx {} size {:x} coords {:?} text {:?} rtl {} off {:x} base {:x} adv {:x}",
                                    fingerprint(run.font().data.as_ref()), run.font().index, b(run.font_size()), run.normalized_coords(), run.text_range(),
                                    run.is_rtl(), b(gr.offset()), b(gr.baseline()), b(gr.advance())
                                );
                                for g in gr.positioned_glyphs() {
                                    let _ = writeln!(o, "    g {} x {:x} y {:x} a {:x} s {}", g.id, b(g.x), b(g.y), b(g.advance), g.style_index());
                                }
                                for cl in run.clusters() {
                                    let _ = writeln!(
                                        o,
                                        "    c {:?} a {:x} wb {} soft {} hard {} ls {} lc {} sp {} n {}",
                                        cl.text_range(), b(cl.advance()), cl.is_word_boundary() as u8, cl.is_soft_line_break() as u8,
                                        cl.is_hard_line_break() as u8, cl.is_ligature_start() as u8, cl.is_ligature_continuation() as u8,
                                        cl.is_space_or_nbsp() as u8, cl.glyphs().count()
                                    );
                                }
                            }
                            PositionedLayoutItem::InlineBox(ib) => {
                                let _ = writeln!(o, "  box {} x {:x} y {:x} w {:x} h {:x}", ib.id, b(ib.x), b(ib.y), b(ib.width), b(ib.height));
                            }
                        }
                    }
                }
                o
            }
        }
    };
}

engine!(grafted, parley);
engine!(upstream, parley_upstream);

/// Both engines over the same fonts.
pub struct Twin {
    pub g: grafted::Eng,
    pub u: upstream::Eng,
    pub cases: u64,
}

impl Twin {
    pub fn new(fonts: &[Vec<u8>], system: bool) -> Self {
        Twin { g: grafted::Eng::new(fonts, system), u: upstream::Eng::new(fonts, system), cases: 0 }
    }

    /// Panics with the first differing line when the grafted layout differs from upstream's.
    pub fn check(&mut self, c: &Case) {
        let (g, u) = (self.g.dump(c), self.u.dump(c));
        self.cases += 1;
        if g != u {
            let mut msg = String::new();
            for (i, (a, b)) in g.lines().zip(u.lines()).enumerate() {
                if a != b {
                    let _ = writeln!(msg, "line {i}:\n  grafted:  {a}\n  upstream: {b}");
                    break;
                }
            }
            if msg.is_empty() {
                let _ = writeln!(msg, "dumps differ in length ({} vs {} lines)", g.lines().count(), u.lines().count());
            }
            if std::env::var("TWIN_FULL").is_ok() {
                let _ = writeln!(msg, "--- grafted\n{g}--- upstream\n{u}");
            }
            panic!("grafted parley != upstream on {c:?}\n{msg}");
        }
    }
}

pub fn read(path: &str) -> Option<Vec<u8>> {
    std::fs::read(path).ok()
}

pub const DEJAVU_DIR: &str = "/Library/Fonts/Nix Fonts/vl9mbzk97wmh1s1yj3766b4zai8msjq6-dejavu-fonts-2.37/share/fonts/truetype";

/// DejaVu Sans, Bold, Sans Mono (the families "DejaVu Sans", "DejaVu Sans Mono").
pub fn dejavu() -> Vec<Vec<u8>> {
    let dir = std::env::var("BOUGH_DEJAVU_DIR").unwrap_or_else(|_| DEJAVU_DIR.into());
    let mut v = vec![zgx_workload::FONT_BYTES.to_vec()];
    for f in ["DejaVuSans-Bold.ttf", "DejaVuSansMono.ttf"] {
        if let Some(b) = read(&format!("{dir}/{f}")) {
            v.push(b);
        }
    }
    v
}
