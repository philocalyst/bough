//! zgx-gpu's `text_bench` (crates/zgx-gpu/examples/text_bench.rs) on bough-text: 1000 rich paragraphs (runs of
//! 1-4 words in DejaVu Sans, every 11th in Bold, every 11th + 4 in Mono, 14 px) measured cold at 736 px, again at
//! the same width, at a new width (700 px, a resize), and one paragraph of 3000 bytes streamed 6 bytes per step
//! and measured at 736 and 968 px each step. Same generator, same widths, same phases, same timing output as
//! zgx's, plus instructions per phase. zgx's own parity pass (200 paragraphs at 9 widths, two measuring paths
//! that must agree) is mirrored: the retained paragraph (incremental) against one set from scratch.
//!
//! A paragraph is a retained `para::Para`: shaped once into byte slots (the GPU-visible pool, so the same block
//! would draw it), a word table, and its wraps cached per width. The stream appends: only the new bytes (and the
//! word they continue) are shaped and the wrap restarts one line before the end.
//!
//! Fonts: zgx resolves these styles to the system's SF NS and Menlo on macOS; bough's table shaper needs a font
//! without variations, so both sides are measured with DejaVu (bench/zgx/text_bench_dejavu.rs registers the same
//! three files in zgx); zgx's stock run is reported too.
use bough_text::para::{Para, Run};
use bough_text::pool::GlyphPool;
use bough_text::{Shaper, SimpleScript};
use bough_text_bench::{dejavu, instrs};
use std::time::Instant;

const WORDS: &[&str] = &[
    "the", "renderer", "keeps", "every", "glyph", "retained", "while", "streaming", "tokens", "arrive", "layout", "measures", "paragraphs",
    "at", "two", "widths", "per", "pass", "and", "virtualized", "transcripts", "need", "exact", "heights", "before", "mounting", "views",
    "fonts", "shape", "once", "wrapping", "is", "arithmetic", "over", "cached", "advances",
];

/// zgx's `paragraph(seed, chars)`: the text and its font runs (0 body, 1 bold, 2 code), adjacent equal fonts
/// merged (one shaping run, as a shaper does across spans of equal attributes).
fn paragraph(seed: usize, chars: usize) -> (String, Vec<Run>) {
    let (mut text, mut runs) = (String::new(), Vec::<Run>::new());
    let mut i = seed;
    while text.len() < chars {
        let font = match i % 11 {
            3 => 1,
            7 => 2,
            _ => 0,
        };
        if runs.last().is_none_or(|r| r.font != font) {
            runs.push(Run { start: text.len() as u32, font });
        }
        for _ in 0..(1 + i % 4) {
            text.push_str(WORDS[(i * 7 + seed) % WORDS.len()]);
            text.push(' ');
            i += 1;
        }
    }
    (text, runs)
}

const SIZE: f32 = 14.0;
const LINE: f32 = 17.0;

/// Font units per px at 14 px (DejaVu: 2048 units per em).
fn units(width: Option<f32>, upem: f32) -> i32 {
    match width {
        Some(w) => ((w + 1e-3) / (SIZE / upem)).floor() as i32,
        None => i32::MAX,
    }
}

fn size(p: &mut Para, width: Option<f32>, upem: f32) -> (f32, f32) {
    let w = p.wrap(units(width, upem));
    (w.widest as f32 * SIZE / upem, w.len() as f32 * LINE)
}

fn main() {
    let t_fonts = Instant::now();
    // STOCK=1: zgx's own faces on macOS: SF NS at the instances cosmic-text picks (wght 400 / 700, opsz = the
    // 14 px size, which the font clamps to its 17 minimum); the code face stays DejaVu Sans Mono (zgx: Menlo, whose
    // AAT `morx` table the table shaper does not model).
    let shapers: [Shaper; 3] = if bough_text_bench::env("STOCK", 0) != 0 {
        let sf = std::fs::read("/System/Library/Fonts/SFNS.ttf").expect("SF NS");
        let inst = |w: f32| Shaper::new(SimpleScript::build_instance_cached(&sf, 0, &[(*b"wght", w), (*b"opsz", 14.0)]).expect("SF NS instance"));
        [inst(400.0), inst(700.0), Shaper::new(SimpleScript::build_cached(&dejavu("DejaVuSansMono.ttf"), 0).expect("font"))]
    } else {
        [dejavu("DejaVuSans.ttf"), dejavu("DejaVuSans-Bold.ttf"), dejavu("DejaVuSansMono.ttf")].map(|b| Shaper::new(SimpleScript::build_cached(&b, 0).expect("font")))
    };
    let fonts: Vec<&Shaper> = shapers.iter().collect();
    let upem = shapers[0].script.upem as f32;
    assert!(shapers.iter().all(|s| s.script.upem as f32 == upem));
    eprintln!("font tables: {:.1} ms", t_fonts.elapsed().as_secs_f64() * 1e3);
    let mut pool = GlyphPool::new();
    let paragraphs: Vec<(String, Vec<Run>)> = (0..1000).map(|i| paragraph(i, 200 + (i * 37) % 400)).collect();

    // zgx's parity pass: both paths must agree on every size (here: retained/incremental vs from scratch).
    let widths = [None, Some(736.), Some(700.), Some(320.), Some(211.3), Some(90.), Some(41.7), Some(12.), Some(1.)];
    for (text, runs) in &paragraphs[..200] {
        let mut a = Para::default();
        a.set(&mut pool, &fonts, text, runs).unwrap();
        for w in widths {
            let mut b = Para::default();
            b.set(&mut pool, &fonts, text, runs).unwrap();
            assert_eq!(size(&mut a, w, upem), size(&mut b, w, upem), "width {w:?}: {text:?}");
            b.release(&mut pool);
        }
        a.release(&mut pool);
    }

    if bough_text_bench::env("PARTS", 0) != 0 {
        // where a first measure goes: set into a fresh paragraph, set again into a retained one, the slot kernel
        // alone (one font, the whole text), the wrap alone
        let n = paragraphs.len() as f64;
        let mut ps: Vec<Para> = (0..paragraphs.len()).map(|_| Para::default()).collect();
        let i0 = instrs();
        for (p, (t, r)) in ps.iter_mut().zip(&paragraphs) {
            p.set(&mut pool, &fonts, t, r).unwrap();
        }
        let i1 = instrs();
        for (p, (t, r)) in ps.iter_mut().zip(&paragraphs) {
            p.set(&mut pool, &fonts, t, r).unwrap();
        }
        let i2 = instrs();
        let (mut x, mut e) = (vec![0i32; 1024], vec![0u16; 1024]);
        for (t, _) in &paragraphs {
            // SAFETY: 1024 slots >= every paragraph (< 640 bytes) rounded to 16.
            std::hint::black_box(unsafe { bough_text::slots::any_slots(fonts[0], t.as_bytes(), 0, x.as_mut_ptr(), e.as_mut_ptr()) }.unwrap());
        }
        let i3 = instrs();
        for p in ps.iter_mut() {
            std::hint::black_box(size(p, Some(736.), upem));
        }
        let i4 = instrs();
        let bytes: usize = paragraphs.iter().map(|p| p.0.len()).sum();
        println!("per paragraph ({:.0} bytes): set fresh {:.0}, set retained {:.0}, slot kernel alone {:.0}, wrap {:.0}", bytes as f64 / n, (i1 - i0) as f64 / n, (i2 - i1) as f64 / n, (i3 - i2) as f64 / n, (i4 - i3) as f64 / n);
    }
    let mut paras: Vec<Para> = (0..paragraphs.len()).map(|_| Para::default()).collect();
    // PROFILE=n: repeat the first measure n times first (for a sampling profiler)
    for _ in 0..bough_text_bench::env("PROFILE", 0usize) {
        for (p, (text, runs)) in paras.iter_mut().zip(&paragraphs) {
            p.set(&mut pool, &fonts, text, runs).unwrap();
            std::hint::black_box(size(p, Some(736.), upem));
        }
    }
    let mut checksum = 0.0f64;
    let mut time = |label: &str, run: &mut dyn FnMut(&mut f64)| {
        let (i0, t0) = (instrs(), Instant::now());
        run(&mut checksum);
        let (dt, di) = (t0.elapsed(), instrs() - i0);
        println!("{label:32} {:8.2} ms  {:>12} instructions", dt.as_secs_f64() * 1e3, di);
    };
    time("1000 paragraphs, first measure", &mut |c| {
        for (p, (text, runs)) in paras.iter_mut().zip(&paragraphs) {
            p.set(&mut pool, &fonts, text, runs).unwrap();
            let (w, h) = size(p, Some(736.), upem);
            *c += (w + h) as f64;
        }
    });
    time("same paragraphs, same width", &mut |c| {
        for p in paras.iter_mut() {
            let (w, h) = size(p, Some(736.), upem);
            *c += (w + h) as f64;
        }
    });
    time("same paragraphs, new width", &mut |c| {
        for p in paras.iter_mut() {
            let (w, h) = size(p, Some(700.), upem);
            *c += (w + h) as f64;
        }
    });
    let (full, runs) = paragraph(99_999, 3000);
    time("stream 3000 chars, 6 per step", &mut |c| {
        let mut p = Para::default();
        let mut done = 0usize;
        let mut end = 6;
        while end < full.len() {
            while !full.is_char_boundary(end) {
                end += 1;
            }
            // the new bytes, run by run
            let mut k = done;
            while k < end {
                let r = runs.partition_point(|r| r.start as usize <= k) - 1;
                let stop = runs.get(r + 1).map_or(end, |r| (r.start as usize).min(end));
                p.append(&mut pool, &fonts, &full[k..stop], runs[r].font).unwrap();
                k = stop;
            }
            done = end;
            let (w1, h1) = size(&mut p, Some(736.), upem);
            let (w2, h2) = size(&mut p, Some(968.), upem);
            *c += (w1 + h1 + w2 + h2) as f64;
            end += 6;
        }
        p.release(&mut pool);
    });
    let slots = pool.x.len();
    println!("pool: {:.1} MB of slots (6 B a byte), checksum {checksum:.1}", (slots * 6) as f64 / 1048576.);
}
