//! The text half of zgx-script's `code_editor_bench` (crates/zgx-script/examples/code_editor_bench.rs) on
//! bough-text (`doc.rs`): a Rust file of `LINES` (100,000) lines from zgx's own generator, opened (read, line
//! index, first screen shaped, highlighted and numbered), a keystroke in the middle of the file (20, `x`/`y`
//! alternately, the line re-highlighted), a scroll by a page (600 pt) and by a line (19 pt), 30 frames each, and
//! the page scroll again with soft wrap on; plus the bytes a line takes. 1100 x 760 pt at 2x, 13 pt DejaVu Sans
//! Mono (zgx's stock run resolves "mono" to Menlo), 19 pt lines.
//!
//! Scope: this is the text engine's part of each frame (line index, shaping, highlighting, wrapping, run
//! headers, tile damage), not zgx's whole frame (its Luau script host, editor widget and GPU render). bough's
//! GPU frame for such damage is ~200 K instructions of Metal command-buffer floor (STATUS.md §1).
use bough_text::lines::LineStyle;
use bough_text::pool::GlyphPool;
use bough_text::{Shaper, SimpleScript};
use bough_text_bench::doc::*;
use bough_text_bench::{dejavu, env, instrs};
use std::time::Instant;

fn main() {
    let lines: usize = env("LINES", 100_000);
    let (scale, size_pt, line_pt, h_pt) = (2.0f32, 13.0f32, 19.0f32, 760.0f32);
    let path = std::env::temp_dir().join(format!("bough-code-bench-{lines}.rs"));
    std::fs::write(&path, source(lines)).unwrap();
    let mono = dejavu("DejaVuSansMono.ttf");
    let sh = Shaper::new(SimpleScript::build_cached(&mono, 0).expect("font"));
    let mut cache = bough_text::atlas::GlyphCache::new(&mono);
    let strike = cache.strike(size_pt * scale);
    let tt = sh.script.tokens.as_ref().unwrap();
    cache.prepare_tokens(strike, &tt.gid, 0x20..0x7F);
    let b = cache.line_bounds(strike, tt, sh.script.upem);
    let rows = (h_pt / line_pt).ceil() as usize + 1;
    let lh = line_pt * scale;
    let st = |ox: f32| LineStyle { ox, oy: 30.0, lh, scale: size_pt * scale / sh.script.upem as f32, color: TEXT, space: 0, strike: strike as u16 };
    let mut plane = vec![0u128; 64];
    let mut pool = GlyphPool::new();
    let fp0 = bough::sys::perf::snap().footprint;

    // open: the file read, the line index, the first screen
    let (t0, i0) = (Instant::now(), instrs());
    let doc_text = std::fs::read_to_string(&path).unwrap();
    let mut doc = Document::new(doc_text);
    let mut view = View::new(&mut pool, rows, st(120.0), st(16.0), doc.lines());
    view.show(&mut pool, &sh, &doc, 0, None, &b, &mut plane);
    let (open, oi) = (t0.elapsed().as_secs_f64() * 1e3, (instrs() - i0) as f64 / 1e6);
    let per_line = (bough::sys::perf::snap().footprint as f64 - fp0 as f64) / lines as f64;
    println!("{lines} lines: open to first screen {open:.2} ms ({oi:.2} M instructions), {per_line:.0} bytes a line (document {} B, line index 4 B, highlight state 1 B a line)", doc.len() / lines);

    // a keystroke in the middle of the file
    let mut caret = doc.len() / 2;
    let mut frame = |doc: &mut Document, view: &mut View, pool: &mut GlyphPool, first: usize, edited: Option<usize>| {
        plane.iter_mut().for_each(|r| *r = 0);
        view.show(pool, &sh, doc, first, edited, &b, &mut plane)
    };
    let mut first;
    {
        let line = doc.insert(caret, b"");
        first = line.saturating_sub(rows / 2);
        frame(&mut doc, &mut view, &mut pool, first, None);
        frame(&mut doc, &mut view, &mut pool, first, None);
    }
    let mean = |n: usize, f: &mut dyn FnMut(usize) -> (u32, u32)| {
        let (t, i) = (Instant::now(), instrs());
        let mut shaped = 0;
        for k in 0..n {
            shaped += f(k).0;
        }
        (t.elapsed().as_secs_f64() * 1e3 / n as f64, (instrs() - i) as f64 / n as f64 / 1e6, shaped as f64 / n as f64)
    };
    let (t, i, s) = mean(20, &mut |k| {
        let line = doc.insert(caret, if k % 2 == 0 { b"x" } else { b"y" });
        caret += 1;
        frame(&mut doc, &mut view, &mut pool, first, Some(line))
    });
    println!("keystroke to frame: {t:.3} ms, {i:.3} M instructions ({s:.1} lines shaped a frame)");
    for (label, step) in [("a page", 600.0f32), ("a line", 19.0)] {
        let mut scroll = first as f32 * line_pt;
        let (t, i, s) = mean(30, &mut |_| {
            scroll += step;
            first = ((scroll / line_pt) as usize).min(doc.lines() - rows);
            frame(&mut doc, &mut view, &mut pool, first, None)
        });
        println!("scroll {label}: {t:.3} ms a frame, {i:.3} M instructions ({s:.1} lines shaped a frame)");
    }
    view.wrap = true;
    frame(&mut doc, &mut view, &mut pool, first, None);
    let mut scroll = first as f32 * line_pt;
    let (t, i, s) = mean(30, &mut |_| {
        scroll += 600.0;
        first = ((scroll / line_pt) as usize).min(doc.lines() - rows);
        frame(&mut doc, &mut view, &mut pool, first, None)
    });
    println!("scroll a page, wrapped: {t:.3} ms a frame, {i:.3} M instructions ({s:.1} lines shaped a frame)");
    let _ = std::fs::remove_file(&path);
}
