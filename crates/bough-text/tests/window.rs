//! Invention A: runs as windows onto shaped slots. Every visible line of the lab stream, at
//! every tick, and every wrapped line of the vlist paragraphs must expand to exactly the
//! instances of that line shaped on its own by the reference path.

#[path = "../examples/common/corpus.rs"]
mod corpus;

use bough_text::atlas::{GlyphCache, Instance};
use bough_text::pool::GlyphPool;
use bough_text::window::{wrap_slots, Shaped, Stream};
use bough_text::wrap::wrap_greedy;
use bough_text::{GlyphRun, Shaper, SimpleScript};

const FONT: &[u8] = zgx_workload::FONT_BYTES;

fn full() -> bool {
    !cfg!(debug_assertions) || std::env::var("BOUGH_FULL").is_ok()
}

fn reference(sh: &Shaper, cache: &mut GlyphCache, text: &str, size: f32, origin: [f32; 2]) -> Vec<Instance> {
    let mut run = GlyphRun::default();
    assert!(sh.shape_into(text, size, &mut run), "{text:?}");
    let st = cache.strike(size);
    let mut v = vec![];
    cache.instances(st, &run, origin[0], origin[1], 0xff00ff00, &mut v);
    v
}

fn expand(pool: &GlyphPool, cache: &GlyphCache, run: u32) -> Vec<Instance> {
    let mut v = vec![];
    pool.expand(cache, run, &mut v);
    for i in v.iter_mut() {
        i.space = 0;
    }
    v
}

#[test]
fn lab_stream_lines_are_windows() {
    let sh = Shaper::new(SimpleScript::build(FONT, 0).unwrap());
    let mut cache = GlyphCache::new(FONT);
    let size = 14.0;
    let st = cache.strike(size);
    let tt = sh.script.tokens.as_ref().unwrap();
    cache.prepare_tokens(st, &tt.gid, 0x20..0x7F);
    let mut pool = GlyphPool::new();
    let mut stream = Stream::new(&mut pool, 1024);
    let runs: Vec<u32> = (0..7).map(|_| pool.alloc_run([0.0, 0.0], size / sh.script.upem as f32, 0xff00ff00, 0, st as u16)).collect();
    let mut w = zgx_workload::Workload::from_env();
    w.mode = zgx_workload::Mode::Stream;
    let ticks = if full() { 3000 } else { 300 };
    let mut checked = 0usize;
    let mut glyphs = 0usize;
    for _ in 0..ticks {
        w.tick();
        let chunk = format!("{:06} {}", w.frames, zgx_workload::TOKEN);
        assert!(stream.append(&mut pool, &sh, chunk.as_bytes()));
        let total = stream.total;
        let vis = total - total.min(735);
        let lines: Vec<String> = w.visible_text().split('\n').map(str::to_string).collect();
        for (k, line) in lines.iter().enumerate() {
            let from = vis + 105 * k as u64;
            let to = (from + 105).min(total);
            assert_eq!(stream.bytes(from, to), line.as_bytes());
            let origin = [12.0 + 0.25 * k as f32, 20.0 + 18.0 * k as f32];
            stream.window(&mut pool, &sh, runs[k], from, to, origin);
            let got = expand(&pool, &cache, runs[k]);
            glyphs += got.len();
            assert_eq!(got, reference(&sh, &mut cache, line, size, origin), "tick {} line {k}", w.frames);
            checked += 1;
        }
    }
    eprintln!("stream: {checked} line windows, {glyphs} glyph instances checked");
    assert!(checked > ticks);
}

#[test]
fn vlist_paragraph_lines_are_windows() {
    let sh = Shaper::new(SimpleScript::build(FONT, 0).unwrap());
    let mut cache = GlyphCache::new(FONT);
    let size = 14.0;
    let st = cache.strike(size);
    let tt = sh.script.tokens.as_ref().unwrap();
    cache.prepare_tokens(st, &tt.gid, 0x20..0x7F);
    let mut pool = GlyphPool::new();
    let max_units = (688.0 / size * sh.script.upem as f32) as i32;
    let rows = if full() { 2000 } else { 100 };
    let mut starts = vec![];
    let mut starts_ref = vec![];
    let mut run = GlyphRun::default();
    let line_runs: Vec<u32> = (0..16).map(|_| pool.alloc_run([0.0, 0.0], size / sh.script.upem as f32, 0xff00ff00, 0, st as u16)).collect();
    let (mut lines_checked, mut glyphs) = (0usize, 0usize);
    for text in corpus::vlist_paragraphs(0..rows) {
        let mut sp = Shaped::default();
        assert!(pool.shape_block(&sh, text.as_bytes(), &mut sp));
        let b = sp.base as usize;
        let n = wrap_slots(&pool.x[b..b + sp.len as usize], sp.width, text.as_bytes(), max_units, &mut starts);
        assert!(sh.shape_into(&text, size, &mut run));
        wrap_greedy(&run, text.as_bytes(), max_units, &mut starts_ref);
        assert_eq!(starts, starts_ref, "{text:?}");
        for l in 0..n {
            let a = starts[l];
            let e = if l + 1 < n { starts[l + 1] } else { sp.len };
            let origin = [16.0, 30.0 + 20.0 * l as f32];
            pool.set_window(line_runs[l], sp.base, a, e, origin);
            let line = &text[a as usize..e as usize];
            let got = expand(&pool, &cache, line_runs[l]);
            lines_checked += 1;
            glyphs += got.len();
            assert_eq!(got, reference(&sh, &mut cache, line, size, origin), "{line:?}");
        }
        pool.free_shaped(sp);
    }
    eprintln!("vlist: {lines_checked} wrapped lines, {glyphs} glyph instances checked");
    assert!(lines_checked > rows);
}
