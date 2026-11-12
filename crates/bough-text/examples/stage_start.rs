//! What text costs on the way to the first frame of the rows scene (gui-bench `rows`: a header and 24 exposed
//! rows of "Row 00012", a three-word label and a value, 14 px), with a prepared face image and without.
//!
//!     BOUGH_FONT=.../DejaVuSans.ttf cargo run --profile dist -p bough-text --example stage_start
//!
//! * image: the face is rodata (`bough_face::Face::new` is `const`); the frame's lines are shaped from it and
//!   their coverage laid into a target. No file, no parse, no shaper, no rasteriser.
//! * no image: what the façade does before its first text pixel: read the font file, build the shaping tables
//!   (`SimpleScript::build_cached`, which reads its file cache when it is warm; `build` when it is not),
//!   construct the shaper, make the strike and rasterise the glyphs the frame uses, shape the same lines.
//!   Font discovery (`fc-match`, a child process, twice in the façade: about 18 ms, e2e journal section 1) is
//!   not in these numbers.
//!
//! Each row is the median of `REPS` (default 15) runs: instructions and nanoseconds of the calling thread.
use bough_face::{num::Num, Face, Glyphs};
use bough_mem::pmu;
use bough_text::atlas::GlyphCache;
use bough_text::stage::bake;
use bough_text::{GlyphRun, Shaper, SimpleScript};
use std::hint::black_box;
use std::time::Instant;

const WORDS: [&str; 12] = ["quick", "amber", "river", "office", "fluffy", "violet", "stone", "bright", "copper", "field", "harbour", "meadow"];
fn label(id: u32) -> String {
    format!("{} {} {}", WORDS[(id * 7 % 12) as usize], WORDS[(id * 13 % 12) as usize], WORDS[(id * 17 % 12) as usize])
}

fn row(name: &str, reps: usize, mut f: impl FnMut()) {
    let mut v: Vec<(u64, u64)> = (0..reps)
        .map(|_| {
            let (a, t) = (pmu::sample(), Instant::now());
            f();
            (pmu::sample().since(&a).v[0], t.elapsed().as_nanos() as u64)
        })
        .collect();
    v.sort();
    let i = v[v.len() / 2].0;
    v.sort_by_key(|x| x.1);
    println!("{name}\t{i}\t{}", v[v.len() / 2].1);
}

fn main() {
    let path = std::env::var("BOUGH_FONT").expect("BOUGH_FONT: the font the image is baked from");
    let reps = std::env::var("REPS").ok().and_then(|v| v.parse().ok()).unwrap_or(15);
    let bytes = std::fs::read(&path).expect("font");
    let img: &'static [u8] = Box::leak(bake::bake(&bytes, 14.0).expect("bake").into_boxed_slice());
    let labels: Vec<String> = (0..24).map(label).collect();
    let (w, h) = (1280i32, 800i32);
    let mut target = vec![0u8; (w * h) as usize];
    println!("# font {path}; image {} bytes; 24 rows + header; REPS={reps}", img.len());
    println!("cell\tinstructions\tns");

    // ---- with the image ----
    row("image: open the face, shape the frame's lines (memo), numbers by arithmetic", reps, || {
        let face = Face::new(black_box(img));
        let mut g = Glyphs::new();
        let mut n = 0;
        n += g.line(0, &face, "rows").unwrap().0.len();
        for l in &labels {
            n += g.line(0, &face, "Row ").unwrap().0.len();
            n += g.line(0, &face, l).unwrap().0.len();
        }
        black_box((n, g.bytes()));
    });
    row("image: the same, and every glyph's coverage laid into the frame", reps, || {
        let face = Face::new(black_box(img));
        let cell = face.cell().unwrap();
        let mut g = Glyphs::new();
        let clip = [0, 0, w, h];
        let mut put = |x: i32, y: i32, c: &[u8]| {
            let d = &mut target[(y * w + x) as usize..][..c.len()];
            for (d, s) in d.iter_mut().zip(c) {
                *d = (*d).max(*s);
            }
        };
        let base = face.baseline(32);
        let (r, _) = g.line(0, &face, "rows").unwrap();
        let (s, x) = g.of(r);
        face.spans(s, x, 0, (92, face.baseline(40)), clip, &mut put);
        for (i, l) in labels.iter().enumerate() {
            let y = 40 + i as i32 * 32 + base;
            let (r, adv) = g.line(0, &face, "Row ").unwrap();
            let (s, x) = g.of(r);
            face.spans(s, x, 0, (12, y), clip, &mut put);
            Num::new(i as i32, 0, 5).spans(&face, &cell, (12 + face.px(adv), y), false, clip, &mut put);
            let (r, _) = g.line(0, &face, l).unwrap();
            let (s, x) = g.of(r);
            face.spans(s, x, 0, (120, y), clip, &mut put);
            Num::new(i as i32 * 7919, 2, 0).spans(&face, &cell, (560, y), false, clip, &mut put);
        }
        black_box(target[0]);
    });

    // ---- without ----
    let texts: Vec<String> = std::iter::once("rows".to_string()).chain((0..24).flat_map(|i| [format!("Row {i:05}"), label(i), format!("{:.2}", i as f64 * 79.19)])).collect();
    row("no image: read the font file", reps, || {
        black_box(std::fs::read(&path).unwrap().len());
    });
    row("no image: shaping tables, file cache warm (SimpleScript::build_cached)", reps, || {
        black_box(SimpleScript::build_cached(&bytes, 0).unwrap().upem);
    });
    row("no image: shaping tables from the font (SimpleScript::build)", reps.min(5), || {
        black_box(SimpleScript::build(&bytes, 0).unwrap().upem);
    });
    let sc = SimpleScript::build_cached(&bytes, 0).unwrap();
    let mut keep = Some(sc);
    let mut sh = None;
    row("no image: construct the shaper (dense tables)", 1, || {
        sh = Some(Shaper::new(keep.take().unwrap()));
    });
    let sh = sh.unwrap();
    row("no image: strike, and rasterise the glyphs the frame uses (cold)", reps, || {
        let mut cache = GlyphCache::new(&bytes);
        let strike = cache.strike(14.0);
        let mut run = GlyphRun::default();
        let mut n = 0;
        for t in &texts {
            sh.shape_into(t, 14.0, &mut run);
            for &g in &run.gid {
                n += cache.entry(strike, g).uv[2] as usize;
            }
        }
        black_box(n);
    });
    let mut run = GlyphRun::default();
    row("no image: shape the frame's lines (table shaper, no memo)", reps, || {
        let mut n = 0;
        for t in &texts {
            sh.shape_into(t, 14.0, &mut run);
            n += run.len();
        }
        black_box(n);
    });
}
