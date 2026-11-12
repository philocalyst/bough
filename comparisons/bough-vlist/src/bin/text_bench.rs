//! Text pipeline costs per paragraph (instructions, proc_pid_rusage) on a vlist paragraph: the leaf's
//! `set_text` (shaped straight into a glyph-pool block + word table) vs the table shaper's `shape_run`, the
//! word-table wrap vs `wrap_greedy`, and (for reference) 48-byte glyph-instance emission per line, which the
//! fused system no longer does for covered text (lines are run-header windows onto the block).
use bough::layout::LeafCtx;
use bough::text::TextSys;
use zgx_workload::vlist;

fn instrs() -> u64 {
    bough::sys::perf::snap().instrs
}
fn per<F: FnMut()>(name: &str, n: usize, mut f: F) {
    for _ in 0..50 {
        f();
    }
    let a = instrs();
    for _ in 0..n {
        f();
    }
    println!("{name:44} {:>9.0} instr", (instrs() - a) as f64 / n as f64);
}

fn main() {
    let mut ts = TextSys::new(zgx_workload::FONT_BYTES, bough_mem::Col::owned("toks"));
    let mut pool = bough_text::pool::GlyphPool::new();
    // a paragraph of about the average vlist length
    let row = (0..200).find(|&r| vlist::lines(r) == 4).unwrap();
    let text = vlist::text(row);
    println!("paragraph: row {row}, {} bytes", text.len());
    let size = 28.0;
    let mut ctx = LeafCtx::new(size, 40.0, true);
    per("set_text (pool block + word table)", 5000, || ctx.set_text(&mut ts, &mut pool, &text));
    let mut run = bough_text::GlyphRun::default();
    per("shape_run (table shaper, GlyphRun)", 5000, || ts.shape_run(&text, size, &mut run));
    let mut n = 0;
    per("wrap_at (word table), 1 width", 5000, || {
        n += 1;
        ctx.wrapped_at = i32::MIN;
        ctx.wrap_at(100_649);
    });
    let mut starts = Vec::new();
    per("wrap_greedy (glyph run)", 5000, || {
        bough_text::wrap::wrap_greedy(&run, text.as_bytes(), 100_649, &mut starts);
    });
    ctx.wrapped_at = i32::MIN;
    let lines = ctx.wrap_at(100_649);
    let strike = ts.strike(size);
    let mut out = Vec::new();
    per("fused emit, per line (all lines)", 5000, || {
        out.clear();
        for i in 0..lines {
            let a = ctx.starts[i] as usize;
            let b = if i + 1 < lines { ctx.starts[i + 1] as usize } else { text.len() };
            ts.cache.shape_instances_ascii(&ts.shaper, strike, &text[a..b], 3.0, 40.0, 0xffffffff, &mut out);
        }
    });
    let mut line = bough_text::GlyphRun::default();
    per("rebased line run + instances (all lines)", 5000, || {
        out.clear();
        for i in 0..lines {
            let a = ctx.starts[i] as usize;
            let b = if i + 1 < lines { ctx.starts[i + 1] as usize } else { text.len() };
            line.gid.clear();
            line.x.clear();
            let x0 = run.x[a];
            line.gid.extend_from_slice(&run.gid[a..b]);
            line.x.extend(run.x[a..b].iter().map(|v| v - x0));
            line.upem = run.upem;
            line.size = run.size;
            line.identity_clusters = true;
            line.width = (if b < text.len() { run.x[b] } else { run.width }) - x0;
            ts.cache.instances(strike, &line, 3.0, 40.0, 0xffffffff, &mut out);
        }
    });
    println!("lines {lines}, instances {}", out.len());
}
