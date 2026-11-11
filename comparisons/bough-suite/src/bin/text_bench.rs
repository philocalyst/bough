//! zgx-gpu `text_bench`, on bough's text path (bough-text's public shaper and glyph pool through the
//! façade's measured-text leaf, `bough::layout::LeafCtx`): 1,000 paragraphs of 200-600 characters measured
//! at 736 pt, again at the same width, at a new width (700), and one paragraph streaming 3,000 characters
//! six at a time, measured at 736 and 968 each step. 14 pt, scale 1.
//!
//! Two forms: **from scratch** (each measurement shapes the text again, as zgx's default path does:
//! `ShapedText::with_runs` per call) and **retained** (a leaf per paragraph: the same width is the cached
//! wrap, a new width re-wraps the word table without shaping; a stream step reshapes its paragraph).
//! bough-text has one font here (DejaVu Sans): zgx's bold and monospace runs are measured in the regular
//! face (a gap: no font fallback / styles in bough-text).
use bough::layout::LeafCtx;
use bough::raw::*;
use bough_suite::*;
use std::time::Instant;

const WORDS: &[&str] = &[
    "the", "renderer", "keeps", "every", "glyph", "retained", "while", "streaming", "tokens", "arrive", "layout", "measures", "paragraphs", "at", "two", "widths", "per", "pass", "and",
    "virtualized", "transcripts", "need", "exact", "heights", "before", "mounting", "views", "fonts", "shape", "once", "wrapping", "is", "arithmetic", "over", "cached", "advances",
];

/// zgx's `paragraph(seed, chars)` text (its runs' fonts are not modelled).
fn paragraph(seed: usize, chars: usize) -> String {
    let mut text = String::new();
    let mut i = seed;
    while text.len() < chars {
        for _ in 0..(1 + i % 4) {
            text.push_str(WORDS[(i * 7 + seed) % WORDS.len()]);
            text.push(' ');
            i += 1;
        }
    }
    text
}

struct Empty;
impl View for Empty {
    fn view(&self) -> impl Element<Self> + '_ {}
}

fn main() {
    let mut a = app_sized(Empty, 1000.0, 800.0, false, 1, 1, 1 << 19);
    let size = 14.0;
    let lh = normal(size);
    let paragraphs: Vec<String> = (0..1000).map(|i| paragraph(i, 200 + (i * 37) % 400)).collect();
    let upem = a.rt.text.shaper.script.upem as f32;
    let units = |w: f32| (w / (size / upem)).floor() as i32;
    let bough::rt::Rt { text, glyphs, .. } = &mut a.rt;
    // warm the shaper once (fonts are loaded by then in zgx too)
    let mut warm = LeafCtx::new(size, lh, true);
    warm.set_text(text, glyphs, &paragraphs[0]);
    warm.wrap_at(units(736.0));
    warm.release(glyphs);
    let time = |label: &str, f: &mut dyn FnMut()| {
        let (i0, t0) = (instructions(), Instant::now());
        f();
        println!("{label:40} {:8.2} ms {:9.2} M instructions", t0.elapsed().as_secs_f64() * 1e3, (instructions() - i0) as f64 / 1e6);
    };
    let mut sink = 0usize;
    println!("from scratch (shaped at every measurement):");
    for (label, w) in [("1000 paragraphs, first measure", 736.0), ("same paragraphs, same width", 736.0), ("same paragraphs, new width", 700.0)] {
        time(label, &mut || {
            for p in &paragraphs {
                let mut c = LeafCtx::new(size, lh, true);
                c.set_text(text, glyphs, p);
                sink += c.wrap_at(units(w));
                c.release(glyphs);
            }
        });
    }
    let full = paragraph(99_999, 3000);
    time("stream 3000 chars, 6 per step", &mut || {
        let mut c = LeafCtx::new(size, lh, true);
        let mut end = 6;
        while end < full.len() {
            c.set_text(text, glyphs, &full[..end]);
            sink += c.wrap_at(units(736.0));
            sink += c.wrap_at(units(968.0));
            end += 6;
        }
        c.release(glyphs);
    });
    println!("retained (a leaf per paragraph):");
    let mut leaves: Vec<LeafCtx> = Vec::with_capacity(paragraphs.len());
    time("1000 paragraphs, first measure", &mut || {
        for p in &paragraphs {
            let mut c = LeafCtx::new(size, lh, true);
            c.set_text(text, glyphs, p);
            sink += c.wrap_at(units(736.0));
            leaves.push(c);
        }
    });
    time("same paragraphs, same width", &mut || {
        for c in &mut leaves {
            sink += c.wrap_at(units(736.0));
        }
    });
    time("same paragraphs, new width", &mut || {
        for c in &mut leaves {
            sink += c.wrap_at(units(700.0));
        }
    });
    std::hint::black_box(sink);
}
