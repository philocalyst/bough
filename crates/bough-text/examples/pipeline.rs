//! The whole text update path, measured: value change -> shaped -> GPU-visible slots -> damage.
//! Instructions (`proc_pid_rusage` RUSAGE_INFO_V4 `ri_instructions`, counter overhead
//! subtracted, best of rounds) are the primary metric; ns are noisy on this shared machine.
//!
//!   cargo run --release --example pipeline [-- quick]

#[path = "common/corpus.rs"]
mod corpus;
#[path = "common/meter.rs"]
mod meter;

use bough_text::atlas::{GlyphCache, Instance};
use bough_text::numeric::{as_str, NumFmt};
use bough_text::pool::{GlyphPool, Label};
use bough_text::window::{wrap_slots, Shaped, Stream};
use bough_text::wrap::wrap_greedy;
use bough_text::{GlyphRun, Shaper, SimpleScript};
use meter::{count, Count};
use std::hint::black_box;

const SIZES: [f32; 6] = [10.0, 11.0, 12.0, 14.0, 16.0, 20.0];

struct Labels {
    pool: GlyphPool,
    labels: Vec<Label>,
}

fn make_labels(sh: &Shaper, cache: &mut GlyphCache, init: &[(String, f32)]) -> Labels {
    let mut pool = GlyphPool::new();
    let labels = init
        .iter()
        .enumerate()
        .map(|(i, (s, z))| {
            let st = cache.strike(*z);
            pool.set_space(i as u16 + 1, [16 + 128 * (i as i32 % 6), 120 + 24 * (i as i32 / 6)]);
            let mut lb = pool.alloc_label(sh, st, *z, [12.0, 20.0], 0xffe5edf7, i as u16 + 1, cache);
            let mut plane = [0u128; 64];
            assert!(pool.set_text(sh, cache, &mut lb, s, &mut plane));
            lb
        })
        .collect();
    Labels { pool, labels }
}

#[inline(never)]
fn update_tick<const D: bool>(sh: &Shaper, cache: &mut GlyphCache, l: &mut Labels, tick: &[(String, f32)], plane: &mut [u128]) {
    for (lb, (s, _)) in l.labels.iter_mut().zip(tick) {
        black_box(l.pool.set_text_d::<D, _>(sh, cache, lb, black_box(s), plane));
    }
}

/// One update, not inlined, so its code can be read with objdump.
#[inline(never)]
pub fn one_update<const D: bool>(pool: &mut GlyphPool, sh: &Shaper, cache: &mut GlyphCache, lb: &mut Label, s: &str, plane: &mut [u128]) -> bool {
    pool.set_text_d::<D, _>(sh, cache, lb, s, plane)
}

/// A heavy label's value and format (what the view code has before it formats).
#[derive(Clone, Copy)]
enum Val {
    F(f32, u8, &'static str),
    U(u32),
}

fn heavy_values(live: u64) -> Vec<Val> {
    use zgx_workload::heavy::{sample, CARDS, TILES};
    let mut v = vec![];
    for t in 0..TILES {
        let s = sample(900 + t, live);
        v.push(match t {
            0 => Val::F(12_000. + 40_000. * s, 0, ""),
            1 => Val::F(3. * s, 2, "%"),
            2 => Val::F(4. + 30. * s, 1, " ms"),
            3 => Val::F(40. + 200. * s, 1, " ms"),
            4 => Val::U(800 + (s * 400.) as u32),
            _ => Val::U((s * 5_000.) as u32),
        });
    }
    for c in 0..CARDS {
        v.push(Val::F(4. + 96. * sample(c, live), 1, " ms"));
    }
    for r in zgx_workload::heavy::row_range(0.) {
        v.push(Val::F(2. + 120. * sample(r + 2000, live), 1, " ms"));
        v.push(Val::F(0.2 + 48. * sample(r + 3000, live), 2, " MB/s"));
    }
    v
}

fn fmt_of(v: &Val) -> NumFmt {
    match *v {
        Val::F(_, d, suf) => NumFmt::new("", Some(d), suf),
        Val::U(_) => NumFmt::new("", None, ""),
    }
}

#[inline(always)]
fn std_format(v: &Val, out: &mut String) {
    use std::fmt::Write;
    out.clear();
    match *v {
        Val::F(x, 0, suf) => write!(out, "{x:.0}{suf}").unwrap(),
        Val::F(x, 1, suf) => write!(out, "{x:.1}{suf}").unwrap(),
        Val::F(x, _, suf) => write!(out, "{x:.2}{suf}").unwrap(),
        Val::U(u) => write!(out, "{u}").unwrap(),
    }
}

#[inline(always)]
fn render(f: &NumFmt, v: &Val, buf: &mut [u8; 16]) -> usize {
    match *v {
        Val::F(x, _, _) => f.render_f32(x, buf).unwrap(),
        Val::U(u) => f.render_u32(u, buf).unwrap(),
    }
}

/// An Fx-style hasher (multiply-rotate per 8 bytes), so the cache experiment is not
/// measuring SipHash.
#[derive(Default)]
struct FxH(u64);
impl std::hash::Hasher for FxH {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, b: &[u8]) {
        let mut c = b.chunks_exact(8);
        for w in &mut c {
            self.0 = (self.0.rotate_left(5) ^ u64::from_le_bytes(w.try_into().unwrap())).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
        }
        for &x in c.remainder() {
            self.0 = (self.0.rotate_left(5) ^ x as u64).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
        }
    }
}
type Fx = std::hash::BuildHasherDefault<FxH>;

fn c_line_shape(sh: &Shaper, lab: &[String], run: &mut GlyphRun) -> f64 {
    let c = count(3, || {
        for t in lab {
            black_box(sh.shape_into(black_box(t), 14.0, run));
        }
    });
    c.instr / lab.len() as f64
}

fn c_line_fused(sh: &Shaper, cache: &mut GlyphCache, lab: &[String], out: &mut Vec<Instance>) -> f64 {
    let st = cache.strike(14.0);
    let c = count(3, || {
        for t in lab {
            out.clear();
            black_box(cache.shape_instances_ascii(sh, st, black_box(t), 12., 20., 0xffe5edf7, out));
        }
    });
    c.instr / lab.len() as f64
}

fn main() {
    let quick = std::env::args().any(|a| a == "quick");
    let reps = if quick { 3 } else { 7 };
    let font = corpus::font_bytes();
    let sh = Shaper::new(SimpleScript::build(font, 0).unwrap());
    let mut cache = GlyphCache::new(font);
    for z in SIZES {
        cache.strike(z);
    }
    let strike_of = |cache: &mut GlyphCache, z: f32| cache.strike(z);
    println!("# bough-text pipeline ({} build), counter overhead {} instr\n", if cfg!(debug_assertions) { "debug" } else { "optimized" }, meter::overhead());

    // ------------------------------------------------------------------ numeric labels
    // Consecutive ticks of heavy (live increments by one per tick, as the workload does).
    let nt = 240usize;
    let ticks: Vec<Vec<(String, f32)>> = (0..=nt as u64).map(corpus::heavy_numeric).collect();
    let per_tick = ticks[0].len();
    let labels_total = (nt * per_tick) as f64;
    // Warm every strike/token the run will use.
    {
        let mut l = make_labels(&sh, &mut cache, &ticks[0]);
        for t in &ticks[1..] {
            let mut plane = vec![0u128; 64];
            update_tick::<true>(&sh, &mut cache, &mut l, t, &mut plane);
        }
    }

    // Before: the prototype's per-label paths.
    let mut run = GlyphRun::default();
    let mut out: Vec<Instance> = Vec::with_capacity(256);
    for t in &ticks {
        for (s, z) in t {
            let st = strike_of(&mut cache, *z);
            out.clear();
            cache.shape_instances_ascii(&sh, st, s, 12., 20., 0xffe5edf7, &mut out);
            sh.shape_into(s, *z, &mut run);
            cache.instances(st, &run, 12., 20., 0xffe5edf7, &mut out);
        }
    }
    let strikes: Vec<Vec<usize>> = ticks.iter().map(|t| t.iter().map(|(_, z)| cache.strike(*z)).collect()).collect();
    let c_shape = count(reps, || {
        for t in &ticks[1..] {
            for (s, z) in t {
                black_box(sh.shape_into(black_box(s), *z, &mut run));
            }
        }
    });
    let c_two = count(reps, || {
        for (t, ks) in ticks[1..].iter().zip(&strikes[1..]) {
            for ((s, z), &k) in t.iter().zip(ks) {
                if sh.shape_into(black_box(s), *z, &mut run) {
                    out.clear();
                    black_box(cache.instances(k, &run, 12., 20., 0xffe5edf7, &mut out));
                }
            }
        }
    });
    let c_fused = count(reps, || {
        for (t, ks) in ticks[1..].iter().zip(&strikes[1..]) {
            for ((s, _), &k) in t.iter().zip(ks) {
                out.clear();
                black_box(cache.shape_instances_ascii(&sh, k, black_box(s), 12., 20., 0xffe5edf7, &mut out));
            }
        }
    });
    // After: the pool, delta updates over the same consecutive ticks. State is reset outside
    // the counted region.
    let mut plane = vec![0u128; 64];
    let mut pool_run = |damage: bool| -> Count {
        let mut best = Count::worst();
        for _ in 0..reps {
            let mut l = make_labels(&sh, &mut cache, &ticks[0]);
            let c = count(1, || {
                for t in &ticks[1..] {
                    if damage {
                        update_tick::<true>(&sh, &mut cache, &mut l, t, &mut plane);
                    } else {
                        update_tick::<false>(&sh, &mut cache, &mut l, t, &mut plane);
                    }
                }
            });
            best = best.min(c);
        }
        best
    };
    let c_pool = pool_run(false);
    let c_pool_d = pool_run(true);
    println!("## Numeric label change (heavy, {nt} consecutive ticks x {per_tick} labels)\n");
    println!("| path | instr / label | ns / label | instr / tick |");
    println!("|---|---|---|---|");
    for (name, c) in [
        ("prototype: shape only (GlyphRun)", c_shape),
        ("prototype: shape + 48 B instances, two steps", c_two),
        ("prototype: fused shape -> 48 B instances", c_fused),
        ("pool delta: slot rewrite only", c_pool),
        ("pool delta: slot rewrite + tile marks", c_pool_d),
    ] {
        println!("| {name} | {:.0} | {:.1} | {:.0} |", c.instr / labels_total, c.ns / labels_total, c.instr / nt as f64);
    }
    println!();

    // ------------------------------------------------------------------ value in (invention B)
    {
        let vals: Vec<Vec<Val>> = (0..=nt as u64).map(heavy_values).collect();
        let fmts: Vec<NumFmt> = vals[0].iter().map(fmt_of).collect();
        // Check: the values reproduce the workload strings.
        let mut buf = [0u8; 16];
        for (t, vs) in vals.iter().enumerate() {
            for (k, v) in vs.iter().enumerate() {
                let len = render(&fmts[k], v, &mut buf);
                assert_eq!(as_str(&buf, len), ticks[t][k].0);
            }
        }
        let mut sbuf = String::with_capacity(32);
        let c_fmt_alloc = count(reps, || {
            for vs in &vals[1..] {
                for v in vs {
                    let s = match *black_box(v) {
                        Val::F(x, 0, suf) => format!("{x:.0}{suf}"),
                        Val::F(x, 1, suf) => format!("{x:.1}{suf}"),
                        Val::F(x, _, suf) => format!("{x:.2}{suf}"),
                        Val::U(u) => format!("{u}"),
                    };
                    black_box(s);
                }
            }
        });
        let c_fmt = count(reps, || {
            for vs in &vals[1..] {
                for v in vs {
                    std_format(black_box(v), &mut sbuf);
                    black_box(&sbuf);
                }
            }
        });
        let c_render = count(reps, || {
            for vs in &vals[1..] {
                for (k, v) in vs.iter().enumerate() {
                    black_box(render(&fmts[k], black_box(v), &mut buf));
                }
            }
        });
        let mut plane = vec![0u128; 64];
        let ov = meter::overhead();
        let mut e2e = |use_render: bool| -> Count {
            let mut best = Count::worst();
            for _ in 0..reps {
                let mut l = make_labels(&sh, &mut cache, &ticks[0]);
                let t0 = std::time::Instant::now();
                let a = meter::instructions();
                for vs in &vals[1..] {
                    for ((lb, v), f) in l.labels.iter_mut().zip(vs).zip(&fmts) {
                        if use_render {
                            let len = render(f, black_box(v), &mut buf);
                            black_box(l.pool.set_text_d::<true, _>(&sh, &mut cache, lb, as_str(&buf, len), &mut plane));
                        } else {
                            std_format(black_box(v), &mut sbuf);
                            black_box(l.pool.set_text_d::<true, _>(&sh, &mut cache, lb, &sbuf, &mut plane));
                        }
                    }
                }
                let b = meter::instructions();
                let ns = t0.elapsed().as_nanos() as f64;
                best = best.min(Count { instr: (b - a).saturating_sub(ov) as f64, ns });
            }
            best
        };
        let c_e2e_fmt = e2e(false);
        let c_e2e_render = e2e(true);
        println!("## Value -> instances + tile marks (invention B), heavy numeric labels\n");
        println!("| step | instr / label | instr / tick |");
        println!("|---|---|---|");
        for (name, c) in [
            ("format!() -> new String (what view code usually does)", c_fmt_alloc),
            ("write!() into a reused String", c_fmt),
            ("NumFmt::render into 16 bytes", c_render),
            ("write!() + pool delta + tile marks", c_e2e_fmt),
            ("NumFmt::render + pool delta + tile marks", c_e2e_render),
        ] {
            println!("| {name} | {:.0} | {:.0} |", c.instr / labels_total, c.instr / nt as f64);
        }
        println!();
    }

    // ------------------------------------------------------------------ long text
    let lab: Vec<String> = (1..=60).flat_map(|t| corpus::lab_stream_lines(t * 17)).take(280).collect();
    let logs: Vec<String> = corpus::heavy_log_lines(200);
    let vlist: Vec<String> = corpus::vlist_paragraphs(0..120);
    println!("## Long text: a new string into GPU-visible instances (+ damage), per string\n");
    println!("| workload | prototype shape only | prototype shape + 48 B instances (fused / two-step) | pool: new text -> slots | pool: slots + tile marks |");
    println!("|---|---|---|---|---|");
    for (name, items, size) in [("stream line (lab, 105 ch)", &lab, 14.0f32), ("log line (heavy, 84.5 ch)", &logs, 11.0)] {
        let st = cache.strike(size);
        let k = items.len() as f64;
        let c_shape = count(reps, || {
            for t in items.iter() {
                black_box(sh.shape_into(black_box(t), size, &mut run));
            }
        });
        let c_fused = count(reps, || {
            for t in items.iter() {
                out.clear();
                black_box(cache.shape_instances_ascii(&sh, st, black_box(t), 12., 20., 0xffe5edf7, &mut out));
            }
        });
        let c_two = count(reps, || {
            for t in items.iter() {
                if sh.shape_into(black_box(t), size, &mut run) {
                    out.clear();
                    black_box(cache.instances(st, &run, 12., 20., 0xffe5edf7, &mut out));
                }
            }
        });
        // Pool: one label receiving each new line in turn (the replace path).
        let mut pool = GlyphPool::new();
        let mut lb = pool.alloc_label(&sh, st, size, [12.0, 20.0], 0xffe5edf7, 1, &mut cache);
        let mut plane = vec![0u128; 64];
        for t in items.iter() {
            one_update::<true>(&mut pool, &sh, &mut cache, &mut lb, t, &mut plane);
        }
        let mut pc = |d: bool| {
            count(reps, || {
                for t in items.iter() {
                    if d {
                        black_box(one_update::<true>(&mut pool, &sh, &mut cache, &mut lb, black_box(t), &mut plane));
                    } else {
                        black_box(one_update::<false>(&mut pool, &sh, &mut cache, &mut lb, black_box(t), &mut plane));
                    }
                }
            })
        };
        let (p0, p1) = (pc(false), pc(true));
        println!(
            "| {name} | {:.0} | {:.0} / {:.0} | {:.0} | {:.0} |",
            c_shape.instr / k, c_fused.instr / k, c_two.instr / k, p0.instr / k, p1.instr / k
        );
    }
    {
        let size = 14.0;
        let st = cache.strike(size);
        let k = vlist.len() as f64;
        let max_units = (688.0 / size * sh.script.upem as f32) as i32;
        let mut starts = vec![];
        let c_shape = count(reps, || {
            for t in vlist.iter() {
                if sh.shape_into(black_box(t), size, &mut run) {
                    black_box(wrap_greedy(&run, t.as_bytes(), max_units, &mut starts));
                }
            }
        });
        let c_two = count(reps, || {
            for t in vlist.iter() {
                if sh.shape_into(black_box(t), size, &mut run) {
                    black_box(wrap_greedy(&run, t.as_bytes(), max_units, &mut starts));
                    out.clear();
                    black_box(cache.instances(st, &run, 12., 20., 0xffe5edf7, &mut out));
                }
            }
        });
        let mut pool = GlyphPool::new();
        let runs: Vec<u32> = (0..16).map(|_| pool.alloc_run([0.0, 0.0], size / sh.script.upem as f32, 0xffe5edf7, 0, st as u16)).collect();
        let lbnd = cache.line_bounds(st, sh.script.tokens.as_ref().unwrap(), sh.script.upem);
        let mut plane = vec![0u128; 64];
        let mut pc = |d: bool| {
            count(reps, || {
                for t in vlist.iter() {
                    let mut sp = Shaped::default();
                    if pool.shape_block(&sh, black_box(t.as_bytes()), &mut sp) {
                        let b = sp.base as usize;
                        let n = wrap_slots(&pool.x[b..b + sp.len as usize], sp.width, t.as_bytes(), max_units, &mut starts);
                        for l in 0..n {
                            let e = if l + 1 < n { starts[l + 1] } else { sp.len };
                            pool.set_window(runs[l], sp.base, starts[l], e, [16.0, 30.0 + 20.0 * l as f32]);
                            if d {
                                let w = if e < sp.len { pool.x[b + e as usize] } else { sp.width } - pool.x[b + starts[l] as usize];
                                pool.mark_window(runs[l], w, &lbnd, &mut plane);
                            }
                        }
                        black_box(&pool.runs);
                        pool.free_shaped(sp);
                    }
                }
            })
        };
        let (p0, p1) = (pc(false), pc(true));
        println!(
            "| vlist paragraph (399 ch, wrapped) | {:.0} (shape + wrap) | - / {:.0} (shape + wrap + instances) | {:.0} (shape + wrap + line windows) | {:.0} |",
            c_shape.instr / k, c_two.instr / k, p0.instr / k, p1.instr / k
        );
    }
    println!();

    // Per tick: the retained forms (invention A).
    println!("## Per tick: retained streams and lists (invention A)\n");
    {
        let size = 14.0;
        let st = cache.strike(size);
        let mut w = zgx_workload::Workload::from_env();
        w.mode = zgx_workload::Mode::Stream;
        let chunks: Vec<String> = (1..=400u64).map(|f| format!("{:06} {}", f, zgx_workload::TOKEN)).collect();
        let mut plane = vec![0u128; 64];
        let lbnd = cache.line_bounds(st, sh.script.tokens.as_ref().unwrap(), sh.script.upem);
        let setup = || {
            let mut pool = GlyphPool::new();
            let mut stream = Stream::new(&mut pool, 1024);
            let runs: Vec<u32> = (0..7).map(|_| pool.alloc_run([0.0, 0.0], size / sh.script.upem as f32, 0xffe5edf7, 0, st as u16)).collect();
            for c in &chunks[..40] {
                stream.append(&mut pool, &sh, c.as_bytes());
            }
            (pool, stream, runs)
        };
        let ticks = &chunks[40..];
        let c = meter::count_stateful(reps, setup, |(pool, stream, runs)| {
            for c in ticks {
                stream.append(pool, &sh, black_box(c.as_bytes()));
                let total = stream.total;
                let vis = total - total.min(735);
                for k in 0..7u64 {
                    let from = vis + 105 * k;
                    let to = (from + 105).min(total);
                    let wdt = stream.window(pool, &sh, runs[k as usize], from, to, [12.0, 20.0 + 18.0 * k as f32]);
                    pool.mark_window(runs[k as usize], wdt, &lbnd, &mut plane);
                }
            }
        });
        let nt = ticks.len() as f64;
        println!("| lab stream tick (append 44 B, 7 visible lines re-pointed, 7 line marks) | {:.0} instr / tick | {:.0} per visible line |", c.instr / nt, c.instr / nt / 7.0);
        println!("| (before: 7 lines re-shaped, prototype shape only / fused 48 B) | {:.0} / {:.0} instr / tick | |", 7.0 * c_line_shape(&sh, &lab, &mut run), 7.0 * c_line_fused(&sh, &mut cache, &lab, &mut out));
    }
    println!();
    // Log pane: a retained list of line labels. Per tick one line is shaped (the new one), the
    // oldest is released, the others move up by rewriting their space (one write each), and
    // the pane is marked once (a moved subtree marks clip ∩ old ∪ clip ∩ new, not its glyphs).
    {
        let size = 11.0;
        let st = cache.strike(size);
        let lines = corpus::heavy_log_lines(400);
        let mut plane = vec![0u128; 64];
        let ov = meter::overhead();
        let mut best = Count::worst();
        for _ in 0..reps {
            let mut pool = GlyphPool::new();
            let mut q: std::collections::VecDeque<(Label, u16)> = std::collections::VecDeque::new();
            for (k, l) in lines[..5].iter().enumerate() {
                let mut lb = pool.alloc_label(&sh, st, size, [12.0, 0.0], 0xffe5edf7, k as u16 + 1, &mut cache);
                pool.set_space(k as u16 + 1, [16, 692 + 16 * k as i32]);
                pool.set_text(&sh, &mut cache, &mut lb, l, &mut plane);
                q.push_back((lb, k as u16 + 1));
            }
            let t0 = std::time::Instant::now();
            let a = meter::instructions();
            for l in &lines[5..] {
                // The line leaving the top is recycled as the new bottom line.
                let (mut lb, sp) = q.pop_front().unwrap();
                pool.set_text(&sh, &mut cache, &mut lb, black_box(l), &mut plane);
                q.push_back((lb, sp));
                for (k, (_, s)) in q.iter().enumerate() {
                    pool.set_space(*s, [16, 692 + 16 * k as i32]);
                }
                bough_text::pool::mark(&mut plane, 16, 684, 16 + 1248, 684 + 100);
            }
            let b = meter::instructions();
            best = best.min(Count { instr: (b - a).saturating_sub(ov) as f64, ns: t0.elapsed().as_nanos() as f64 });
        }
        let nt = (lines.len() - 5) as f64;
        println!("| heavy log pane tick (ring of 5 line labels: 1 new line shaped, 5 space writes, 1 pane mark) | {:.0} instr / tick | (before: 5 lines re-shaped, shape only / fused 48 B: {:.0} / {:.0}) |", best.instr / nt, 5.0 * 877.0, 5.0 * 2922.0);
    }
    println!();

    // ------------------------------------------------------------------ Attempt 4: tokens
    {
        println!("## Attempt 4: Latin-1 and ligatures through the token table\n");
        println!("| strings | eligible before -> after | prototype path | token path | rustybuzz + plan |");
        println!("|---|---|---|---|---|");
        let rb_face = rustybuzz::Face::from_slice(font, 0).unwrap();
        let plan = {
            let mut b = rustybuzz::UnicodeBuffer::new();
            b.push_str("a");
            b.guess_segment_properties();
            rustybuzz::ShapePlan::new(&rb_face, b.direction(), Some(b.script()), b.language().as_ref(), &[])
        };
        let latin1: Vec<String> = ["Caf\u{e9} cr\u{e8}me", "na\u{ef}ve r\u{e9}sum\u{e9}", "Z\u{fc}rich \u{b7} 12\u{b0}C", "\u{a9} 2026 Bj\u{f6}rk", "\u{c0} la carte", "10 \u{d7} 20 \u{b1} 3"]
            .iter().map(|s| s.to_string()).collect();
        let lig_logs: Vec<String> = corpus::heavy_log_lines(2000).into_iter().filter(|l| l.contains("ff") || l.contains("fi") || l.contains("fl")).collect();
        let words: Vec<String> = std::fs::read_to_string("/usr/share/dict/words").unwrap_or_default().lines()
            .filter(|w| w.contains("ff") || w.contains("fi") || w.contains("fl")).step_by(37).map(str::to_string).collect();
        for (name, items) in [("Latin-1 phrases (11.3 ch)", &latin1), ("log lines with ff (84 ch)", &lig_logs), ("dictionary words with ff/fi/fl", &words)] {
            if items.is_empty() {
                continue;
            }
            let k = items.len() as f64;
            let before_ok = items.iter().filter(|t| sh.shape_latin1_legacy(t, 14., &mut run) && (!t.is_ascii() || !t.contains('f') || !(t.contains("ff") || t.contains("fi") || t.contains("fl")))).count();
            let after_ok = items.iter().filter(|t| sh.shape_into(t, 14., &mut run)).count();
            let c_legacy = count(reps, || {
                for t in items.iter() {
                    black_box(sh.shape_latin1_legacy(black_box(t), 14., &mut run));
                }
            });
            let c_tok = count(reps, || {
                for t in items.iter() {
                    black_box(sh.shape_into(black_box(t), 14., &mut run));
                }
            });
            let mut buf = Some(rustybuzz::UnicodeBuffer::new());
            let c_rb = count(2, || {
                for t in items.iter() {
                    let mut b = buf.take().unwrap();
                    b.push_str(black_box(t));
                    b.guess_segment_properties();
                    let g = rustybuzz::shape_with_plan(&rb_face, &plan, b);
                    black_box(g.glyph_positions().len());
                    buf = Some(g.clear());
                }
            });
            println!(
                "| {name}: {} | {before_ok} -> {after_ok} | {:.0} (char_indices path{}) | {:.0} | {:.0} |",
                items.len(), c_legacy.instr / k, if name.starts_with("Latin") { "" } else { "; refuses ligatures, so these went to rustybuzz" }, c_tok.instr / k, c_rb.instr / k
            );
        }
        println!();
    }

    // ------------------------------------------------------------------ Attempt 5: caches
    {
        println!("## Attempt 5: caches vs re-shaping\n");
        use std::collections::HashMap;
        let size = 14.0;
        let max_units = (688.0 / size * sh.script.upem as f32) as i32;
        let dense = sh.dense();
        // Word cache: word bytes -> (first slot in a store, len). Pens are word-relative; the
        // step into a word's first glyph (from the space before it) and out of its last glyph
        // are re-read from the dense table, so the result is exact.
        let mut store_x: Vec<i32> = vec![];
        let mut store_e: Vec<u16> = vec![];
        let mut cache_w: HashMap<Vec<u8>, (u32, u32, i32), Fx> = HashMap::default();
        let mut px: Vec<i32> = vec![0; 1024];
        let mut pe: Vec<u16> = vec![0; 1024];
        let (mut hits, mut misses) = (0u64, 0u64);
        let mut word_shape = |t: &str, px: &mut Vec<i32>, pe: &mut Vec<u16>, hits: &mut u64, misses: &mut u64| -> i32 {
            let b = t.as_bytes();
            let mut x = 0i32;
            let mut i = 0usize;
            while i < b.len() {
                let ws = i;
                while i < b.len() && b[i] != b' ' {
                    i += 1;
                }
                let w = &b[ws..i];
                let (o, l, wd) = match cache_w.get(w) {
                    Some(&e) => {
                        *hits += 1;
                        e
                    }
                    None => {
                        *misses += 1;
                        let o = store_x.len() as u32;
                        store_x.resize(store_x.len() + w.len() + 16, 0);
                        store_e.resize(store_e.len() + w.len() + 16, 0);
                        let (wd, _) = unsafe { bough_text::window::slots(&sh, w, 0, 0, store_x.as_mut_ptr().add(o as usize), store_e.as_mut_ptr().add(o as usize)) };
                        let e = (o, w.len() as u32, wd);
                        cache_w.insert(w.to_vec(), e);
                        e
                    }
                };
                for k in 0..l as usize {
                    px[ws + k] = x + store_x[o as usize + k];
                    pe[ws + k] = store_e[o as usize + k];
                }
                // last glyph's step: the cached one ends at the pad; re-read with the real next byte.
                let last = w[w.len() - 1] as usize;
                let nxt = if i < b.len() { b[i] as usize } else { 0 };
                x += wd - dense[last << 7] as i32 + dense[last << 7 | nxt] as i32;
                while i < b.len() && b[i] == b' ' {
                    let nxt = if i + 1 < b.len() { b[i + 1] as usize } else { 0 };
                    px[i] = x;
                    pe[i] = b' ' as u16;
                    x += dense[(b' ' as usize) << 7 | nxt] as i32;
                    i += 1;
                }
            }
            x
        };
        // exactness of the word path against the slot kernel
        for t in &vlist {
            let w = word_shape(t, &mut px, &mut pe, &mut hits, &mut misses);
            let mut sp = Shaped::default();
            let mut pool = GlyphPool::new();
            assert!(pool.shape_block(&sh, t.as_bytes(), &mut sp));
            assert_eq!(w, sp.width);
            assert_eq!(&px[..t.len()], &pool.x[..t.len()]);
            assert_eq!(&pe[..t.len()], &pool.ent[..t.len()]);
        }
        let (h0, m0) = (hits, misses);
        let k = vlist.len() as f64;
        let mut starts = vec![];
        let c_word = count(reps, || {
            for t in vlist.iter() {
                let w = word_shape(black_box(t), &mut px, &mut pe, &mut hits, &mut misses);
                black_box(wrap_slots(&px[..t.len()], w, t.as_bytes(), max_units, &mut starts));
            }
        });
        let mut pool = GlyphPool::new();
        let c_shape = count(reps, || {
            for t in vlist.iter() {
                let mut sp = Shaped::default();
                pool.shape_block(&sh, black_box(t.as_bytes()), &mut sp);
                let b = sp.base as usize;
                black_box(wrap_slots(&pool.x[b..b + sp.len as usize], sp.width, t.as_bytes(), max_units, &mut starts));
                pool.free_shaped(sp);
            }
        });
        println!("| (both caches use an Fx-style multiplicative hash, not SipHash) | | |");
        println!("| vlist paragraphs, 48-word vocabulary | word cache: {:.0} instr / paragraph (hit rate {:.1}% after warm-up) | NEON re-shape: {:.0} instr / paragraph |", c_word.instr / k, 100.0 * h0 as f64 / (h0 + m0) as f64, c_shape.instr / k);
        // Whole-string cache for labels: string -> shaped slots (hit = copy).
        let mut cache_s: HashMap<String, (Vec<i32>, Vec<u16>, i32), Fx> = HashMap::default();
        let numeric: Vec<&String> = ticks.iter().flatten().map(|(s, _)| s).collect();
        let (mut sh_hits, mut sh_total) = (0u64, 0u64);
        for s in &numeric {
            sh_total += 1;
            if cache_s.contains_key(*s) {
                sh_hits += 1;
            } else {
                let mut x = vec![0i32; 32];
                let mut e = vec![0u16; 32];
                let (w, _) = unsafe { bough_text::window::slots(&sh, s.as_bytes(), 0, 0, x.as_mut_ptr(), e.as_mut_ptr()) };
                cache_s.insert((*s).clone(), (x, e, w));
            }
        }
        let labels_k = numeric.len() as f64;
        let c_hit = count(reps, || {
            for s in &numeric {
                let (x, e, w) = &cache_s[*s];
                px[..s.len()].copy_from_slice(&x[..s.len()]);
                pe[..s.len()].copy_from_slice(&e[..s.len()]);
                black_box(w);
            }
        });
        let c_reshape = count(reps, || {
            for s in &numeric {
                black_box(unsafe { bough_text::window::slots(&sh, black_box(s.as_bytes()), 0, 0, px.as_mut_ptr(), pe.as_mut_ptr()) });
            }
        });
        println!("| heavy numeric labels (240 ticks) | string cache, all hits: {:.0} instr / label (natural hit rate {:.1}%) | re-shape into slots: {:.0} instr / label |", c_hit.instr / labels_k, 100.0 * sh_hits as f64 / sh_total as f64, c_reshape.instr / labels_k);
        println!();
    }

    // ------------------------------------------------------------------ per-path micro
    println!("## Per-path cost of one label update (alternating two values, 14 px)\n");
    println!("| case | rewrite only | rewrite + tile marks |");
    println!("|---|---|---|");
    for (name, a, b) in [
        ("no change", "12.3 ms", "12.3 ms"),
        ("one digit (class patch)", "12.3 ms", "12.4 ms"),
        ("three digits (class patch)", "12.3 ms", "19.8 ms"),
        ("length change (window from byte 0)", "9.9 ms", "10.0 ms"),
        ("unit change (window, suffix)", "12.3 ms", "12.3 us"),
        ("16-byte label, last digit", "worker-01234.iad", "worker-01235.iad"),
    ] {
        let st = cache.strike(14.0);
        let mut pool = GlyphPool::new();
        let mut lb = pool.alloc_label(&sh, st, 14.0, [12.0, 20.0], 0xffe5edf7, 1, &mut cache);
        let mut plane = vec![0u128; 64];
        pool.set_text(&sh, &mut cache, &mut lb, a, &mut plane);
        pool.set_text(&sh, &mut cache, &mut lb, b, &mut plane);
        let reps_n = 2000;
        let mut one = |d: bool| {
            count(reps, || {
                for _ in 0..reps_n {
                    for t in [a, b] {
                        if d {
                            black_box(one_update::<true>(&mut pool, &sh, &mut cache, &mut lb, black_box(t), &mut plane));
                        } else {
                            black_box(one_update::<false>(&mut pool, &sh, &mut cache, &mut lb, black_box(t), &mut plane));
                        }
                    }
                }
            })
        };
        let (c0, c1) = (one(false), one(true));
        let k = (2 * reps_n) as f64;
        println!("| {name} | {:.0} | {:.0} |", c0.instr / k, c1.instr / k);
    }
    println!();
}
