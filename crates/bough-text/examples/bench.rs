//! Retired-instruction and time benchmarks. macOS only for instruction counts
//! (`proc_pid_rusage` -> `ri_instructions`).
//!
//!   cargo run --release --example bench            (or --profile dist)
//!   cargo run --release --example bench -- quick

#[path = "common/corpus.rs"]
mod corpus;

use bough_text::atlas::{GlyphCache, Instance};
use bough_text::wrap::wrap_greedy;
use bough_text::{GlyphRun, Shaper, SimpleScript};
use std::hint::black_box;
use std::time::Instant;

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn proc_pid_rusage(pid: i32, flavor: i32, buffer: *mut core::ffi::c_void) -> i32;
}

#[cfg(target_os = "macos")]
fn instructions() -> u64 {
    // SAFETY: plain syscall wrapper writing a rusage_info_v4 (RUSAGE_INFO_V4 = 4).
    unsafe {
        let mut info: libc::rusage_info_v4 = core::mem::zeroed();
        let r = proc_pid_rusage(libc::getpid(), 4, (&mut info as *mut libc::rusage_info_v4).cast());
        assert_eq!(r, 0, "proc_pid_rusage failed");
        info.ri_instructions
    }
}

#[cfg(not(target_os = "macos"))]
fn instructions() -> u64 {
    bough_damage_lab::measure::counters().0
}

/// Vendored parley has a NEON shaping kernel only; elsewhere the "neon" column measures the scalar kernel.
#[cfg(not(target_arch = "aarch64"))]
trait ShapeSimd {
    fn shape_simd(&self, text: &str, font_size: f32, out: &mut GlyphRun) -> bool;
}
#[cfg(not(target_arch = "aarch64"))]
impl ShapeSimd for Shaper {
    fn shape_simd(&self, text: &str, font_size: f32, out: &mut GlyphRun) -> bool {
        self.shape_scalar(text, font_size, out)
    }
}

/// Instructions consumed by the counter read itself (subtracted from every measurement).
fn counter_overhead() -> u64 {
    let mut best = u64::MAX;
    for _ in 0..50 {
        let a = instructions();
        let b = instructions();
        best = best.min(b - a);
    }
    best
}

struct Measure {
    instr: f64,
    ns: f64,
}

/// Run `f` once per item over `items`, `rounds` times; returns per-item instructions and ns.
fn measure<T>(items: &[T], rounds: usize, mut f: impl FnMut(&T)) -> Measure {
    let total = (items.len() * rounds) as f64;
    // warm
    for it in items {
        f(it);
    }
    let ov = counter_overhead();
    let mut best_instr = u64::MAX;
    for _ in 0..3 {
        let a = instructions();
        for _ in 0..rounds {
            for it in items {
                f(it);
            }
        }
        let b = instructions();
        best_instr = best_instr.min((b - a).saturating_sub(ov));
    }
    let mut best_ns = f64::MAX;
    for _ in 0..5 {
        let t = Instant::now();
        for _ in 0..rounds {
            for it in items {
                f(it);
            }
        }
        best_ns = best_ns.min(t.elapsed().as_nanos() as f64);
    }
    Measure { instr: best_instr as f64 / total, ns: best_ns / total }
}

fn main() {
    let quick = std::env::args().any(|a| a == "quick");
    let rounds_scale = if quick { 1 } else { 4 };
    let font = corpus::font_bytes();
    let sh = Shaper::new(SimpleScript::build(font, 0).unwrap());
    let rb_face = rustybuzz::Face::from_slice(font, 0).unwrap();

    // ---------------- workloads ----------------
    let ticks: Vec<Vec<(String, f32)>> = (0..60u64).map(|t| corpus::heavy_numeric(t * 3 + 1)).collect();
    let numeric: Vec<(String, f32)> = ticks.iter().flatten().cloned().collect();
    let statics = corpus::heavy_static(100);
    let lab: Vec<(String, f32)> =
        (1..=60).flat_map(|t| corpus::lab_stream_lines(t * 17)).take(300).map(|s| (s, 14.)).collect();
    let heavy_logs: Vec<(String, f32)> = corpus::heavy_log_lines(200).into_iter().map(|s| (s, 11.)).collect();
    let vlist: Vec<(String, f32)> = corpus::vlist_paragraphs(0..120).into_iter().map(|s| (s, 14.)).collect();

    let workloads: Vec<(&str, &Vec<(String, f32)>, usize)> = vec![
        ("numeric (heavy tick values)", &numeric, 20),
        ("stream line (lab, 105 ch)", &lab, 40),
        ("log line (heavy)", &heavy_logs, 40),
        ("short UI labels", &statics, 60),
        ("vlist paragraphs", &vlist, 8),
    ];

    println!("# bough-text bench ({} build)", if cfg!(debug_assertions) { "debug" } else { "optimized" });
    println!("counter overhead: {} instr\n", counter_overhead());

    // Eligibility + sizes.
    println!("## Workloads and fast-path eligibility\n");
    println!("| workload | strings | avg chars | avg glyph bytes | fast-path eligible |");
    println!("|---|---|---|---|---|");
    let mut run = GlyphRun::default();
    for (name, items, _) in &workloads {
        let ok = items.iter().filter(|(s, z)| sh.shape_into(s, *z, &mut run)).count();
        let chars: usize = items.iter().map(|(s, _)| s.len()).sum();
        println!(
            "| {name} | {} | {:.1} | - | {:.2}% |",
            items.len(),
            chars as f64 / items.len() as f64,
            100. * ok as f64 / items.len() as f64
        );
    }
    if let Ok(t) = std::fs::read_to_string("/usr/share/dict/words") {
        let words: Vec<&str> = t.lines().collect();
        let ok = words.iter().filter(|w| sh.shape_into(w, 14., &mut run)).count();
        println!(
            "| English dictionary words (/usr/share/dict/words) | {} | {:.1} | - | {:.2}% |",
            words.len(),
            words.iter().map(|w| w.len()).sum::<usize>() as f64 / words.len() as f64,
            100. * ok as f64 / words.len() as f64
        );
        let wl: Vec<&str> = words.iter().copied().filter(|w| w.chars().all(|c| c.is_ascii_lowercase())).collect();
        let inel = wl.iter().filter(|w| !sh.shape_into(w, 14., &mut run)).count();
        println!("| (lowercase words refused, all from f-ligature pairs) | {} | - | - | {:.2}% refused |", inel, 100. * inel as f64 / wl.len() as f64);
    }
    println!();

    // ---------------- shaping benchmarks ----------------
    // cosmic-text, as zgx drives it.
    use cosmic_text::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping};
    let mut db = cosmic_text::fontdb::Database::new();
    db.load_font_source(cosmic_text::fontdb::Source::Binary(std::sync::Arc::new(font.to_vec())));
    let mut fs = FontSystem::new_with_locale_and_db("en-US".into(), db);
    let attrs = Attrs::new().family(Family::Name("DejaVu Sans"));

    println!("## Shaping: instructions and ns per string (lower is better)\n");
    println!("| workload | shape_into (hybrid) | NEON only | scalar only | rustybuzz shape() | rustybuzz + plan | cosmic-text Buffer (zgx path) | cosmic / hybrid (instr) |");
    println!("|---|---|---|---|---|---|---|---|");
    let mut summary: Vec<(String, f64, f64, f64)> = vec![];
    for (name, items, rounds) in &workloads {
        let rounds = rounds * rounds_scale;
        let is_vlist = name.starts_with("vlist");
        let wrap_w = 688.0f32;

        let mut run = GlyphRun::default();
        let mut starts = Vec::new();
        let m_simd = measure(items, rounds, |(s, z)| {
            let ok = sh.shape_into(black_box(s), *z, &mut run);
            if is_vlist && ok {
                wrap_greedy(&run, s.as_bytes(), (wrap_w / *z * sh.script.upem as f32) as i32, &mut starts);
            }
            black_box(ok);
            black_box(&run);
        });
        let m_neon = measure(items, rounds, |(s, z)| {
            let ok = sh.shape_simd(black_box(s), *z, &mut run);
            if is_vlist && ok {
                wrap_greedy(&run, s.as_bytes(), (wrap_w / *z * sh.script.upem as f32) as i32, &mut starts);
            }
            black_box(ok);
            black_box(&run);
        });
        let m_scalar = measure(items, rounds, |(s, z)| {
            let ok = sh.shape_scalar(black_box(s), *z, &mut run);
            if is_vlist && ok {
                wrap_greedy(&run, s.as_bytes(), (wrap_w / *z * sh.script.upem as f32) as i32, &mut starts);
            }
            black_box(ok);
            black_box(&run);
        });

        let mut buf = Some(rustybuzz::UnicodeBuffer::new());
        let m_rb = measure(items, rounds / 2 + 1, |(s, _)| {
            let mut b = buf.take().unwrap();
            b.push_str(black_box(s));
            b.guess_segment_properties();
            let g = rustybuzz::shape(&rb_face, &[], b);
            black_box(g.glyph_positions().len());
            buf = Some(g.clear());
        });
        // With a reused plan.
        let plan = {
            let mut b = rustybuzz::UnicodeBuffer::new();
            b.push_str("a");
            b.guess_segment_properties();
            rustybuzz::ShapePlan::new(&rb_face, b.direction(), Some(b.script()), b.language().as_ref(), &[])
        };
        let m_rbp = measure(items, rounds / 2 + 1, |(s, _)| {
            let mut b = buf.take().unwrap();
            b.push_str(black_box(s));
            b.guess_segment_properties();
            let g = rustybuzz::shape_with_plan(&rb_face, &plan, b);
            black_box(g.glyph_positions().len());
            buf = Some(g.clear());
        });

        // cosmic-text: one retained Buffer per label slot (zgx keeps shaped text per node and
        // recycles lines), set_text + shape_until_scroll + read layout runs; the word cache
        // is trimmed once per "frame" (here per pass over the items).
        let slots = items.len().min(128);
        let mut bufs: Vec<Buffer> = (0..slots)
            .map(|i| {
                let size = items[i].1;
                let mut b = Buffer::new_empty(Metrics::new(size, (size * 1.25).round()));
                b.set_size(if is_vlist { Some(wrap_w + 1e-3) } else { None }, None);
                b
            })
            .collect();
        let idx: Vec<usize> = (0..items.len()).collect();
        let mut n = 0usize;
        let m_cos = measure(&idx, (rounds / 2).max(1), |&i| {
            let (s, z) = &items[i];
            let b = &mut bufs[i % slots];
            if b.metrics().font_size != *z {
                b.set_metrics(Metrics::new(*z, (*z * 1.25).round()));
            }
            b.set_text(black_box(s), &attrs, Shaping::Advanced, None);
            b.shape_until_scroll(&mut fs, false);
            let mut g = 0;
            for r in b.layout_runs() {
                g += r.glyphs.len();
            }
            black_box(g);
            n += 1;
            if n % slots == 0 {
                fs.shape_run_cache.trim(2);
            }
        });

        println!(
            "| {name} | {:.0} i / {:.0} ns | {:.0} i / {:.0} ns | {:.0} i / {:.0} ns | {:.0} i / {:.0} ns | {:.0} i / {:.0} ns | {:.0} i / {:.0} ns | {:.0}x |",
            m_simd.instr, m_simd.ns, m_neon.instr, m_neon.ns, m_scalar.instr, m_scalar.ns, m_rb.instr, m_rb.ns, m_rbp.instr, m_rbp.ns,
            m_cos.instr, m_cos.ns, m_cos.instr / m_simd.instr
        );
        summary.push((name.to_string(), m_simd.instr, m_cos.instr, m_rbp.instr));
    }
    println!();

    // Per-glyph normalised numbers for the numeric workload.
    {
        let chars: f64 = numeric.iter().map(|(s, _)| s.len() as f64).sum::<f64>() / numeric.len() as f64;
        println!("numeric labels average {chars:.1} glyphs\n");
    }

    // ---------------- by length ----------------
    println!("## Fast SIMD vs scalar by string length (instructions / string)\n");
    println!("| length | SIMD | scalar |");
    println!("|---|---|---|");
    for len in [1usize, 4, 8, 12, 16, 17, 32, 64, 105, 200] {
        let s: String = "Hello wor1d 0123456789 abcdeghijk xyz."
            .chars()
            .cycle()
            .take(len)
            .collect();
        let items = [s.clone()];
        let m1 = measure(&items, 20000 * rounds_scale / (1 + len / 16), |s| {
            black_box(sh.shape_simd(black_box(s), 14., &mut run));
        });
        let m2 = measure(&items, 20000 * rounds_scale / (1 + len / 16), |s| {
            black_box(sh.shape_scalar(black_box(s), 14., &mut run));
        });
        println!("| {len} | {:.0} i / {:.1} ns | {:.0} i / {:.1} ns |", m1.instr, m1.ns, m2.instr, m2.ns);
    }
    println!();

    // Latin-1 text (UTF-8 two-byte sequences) takes the scalar decoding path.
    {
        let l1: Vec<String> = ["Caf\u{e9} cr\u{e8}me", "na\u{ef}ve r\u{e9}sum\u{e9}", "Z\u{fc}rich \u{b7} 12\u{b0}C", "\u{a9} 2026 Bj\u{f6}rk", "\u{c0} la carte", "10 \u{d7} 20 \u{b1} 3"]
            .iter().map(|s| s.to_string()).collect();
        let elig = l1.iter().filter(|s| sh.shape_into(s, 14., &mut run)).count();
        let m = measure(&l1, 4000 * rounds_scale, |s| {
            black_box(sh.shape_into(black_box(s), 14., &mut run));
        });
        let avg = l1.iter().map(|s| s.chars().count()).sum::<usize>() as f64 / l1.len() as f64;
        println!("Latin-1 strings ({elig}/{} eligible, {avg:.1} chars avg, UTF-8 decode path): {:.0} instr / string, {:.1} ns\n", l1.len(), m.instr, m.ns);
    }

    // ---------------- glyph instances ----------------
    println!("## Glyph instance generation (48-byte instances, warm atlas)\n");
    let mut cache = GlyphCache::new(font);
    let strike_ids: Vec<usize> = [10.0, 11.0, 12.0, 14.0, 16.0, 20.0].iter().map(|&z| cache.strike(z)).collect();
    let runs: Vec<(GlyphRun, usize)> = numeric
        .iter()
        .chain(statics.iter())
        .chain(lab.iter())
        .filter_map(|(s, z)| {
            let mut r = GlyphRun::default();
            let k = [10.0f32, 11.0, 12.0, 14.0, 16.0, 20.0].iter().position(|&x| x == *z)?;
            sh.shape_into(s, *z, &mut r).then(|| (r, strike_ids[k]))
        })
        .collect();
    let mut out: Vec<Instance> = Vec::with_capacity(4096);
    for (r, k) in &runs {
        cache.instances(*k, r, 0., 0., 0, &mut out);
    }
    let glyphs: usize = runs.iter().map(|(r, _)| r.len()).sum();
    let m = measure(&runs, 20 * rounds_scale, |(r, k)| {
        out.clear();
        black_box(cache.instances(*k, r, 12., 20., 0xffe5edf7, &mut out));
    });
    let per_glyph = m.instr * runs.len() as f64 / glyphs as f64;
    println!(
        "{} runs, {} glyphs: {:.0} instr / run, {:.2} instr / glyph, {:.1} ns / run, {:.2} ns / glyph\n",
        runs.len(),
        glyphs,
        m.instr,
        per_glyph,
        m.ns,
        m.ns * runs.len() as f64 / glyphs as f64
    );

    // End to end: shape + instances for the numeric workload.
    let m = measure(&numeric, 10 * rounds_scale, |(s, z)| {
        let k = strike_ids[[10.0f32, 11.0, 12.0, 14.0, 16.0, 20.0].iter().position(|&x| x == *z).unwrap()];
        if sh.shape_into(black_box(s), *z, &mut run) {
            out.clear();
            black_box(cache.instances(k, &run, 12., 20., 0xffe5edf7, &mut out));
        }
    });
    println!("numeric label shape+instances end to end (two steps): {:.0} instr / label, {:.1} ns / label", m.instr, m.ns);
    for (s, z) in &numeric {
        let k = strike_ids[[10.0f32, 11.0, 12.0, 14.0, 16.0, 20.0].iter().position(|&x| x == *z).unwrap()];
        out.clear();
        cache.shape_instances_ascii(&sh, k, s, 12., 20., 0xffe5edf7, &mut out);
    }
    let m = measure(&numeric, 10 * rounds_scale, |(s, z)| {
        let k = strike_ids[[10.0f32, 11.0, 12.0, 14.0, 16.0, 20.0].iter().position(|&x| x == *z).unwrap()];
        out.clear();
        black_box(cache.shape_instances_ascii(&sh, k, black_box(s), 12., 20., 0xffe5edf7, &mut out));
    });
    println!("numeric label shape+instances fused (no GlyphRun): {:.0} instr / label, {:.1} ns / label\n", m.instr, m.ns);

    // Rasterization (miss path).
    let ids: Vec<u16> = (0x21u8..0x7f).map(|c| sh.script.gid[c as usize]).collect();
    let t = Instant::now();
    let a = instructions();
    let mut c2 = GlyphCache::new(font);
    let st = c2.strike(14.);
    for &g in &ids {
        c2.entry(st, g);
    }
    let b = instructions();
    let el = t.elapsed();
    println!(
        "swash rasterization (14 px, unhinted alpha): {} glyphs, {:.0} instr / glyph, {:.0} ns / glyph (cold, includes atlas packing), atlas {}x{} {:.1}% used\n",
        ids.len(),
        (b - a) as f64 / ids.len() as f64,
        el.as_nanos() as f64 / ids.len() as f64,
        c2.atlas.width,
        c2.atlas.height,
        100. * c2.atlas.utilization()
    );

    // ---------------- tick model ----------------
    println!("## One heavy tick: all ~{} numeric labels\n", ticks[0].len());
    let tick_items: Vec<usize> = (0..ticks.len()).collect();
    let m_fast = measure(&tick_items, 20 * rounds_scale, |&t| {
        for (s, z) in &ticks[t] {
            black_box(sh.shape_into(black_box(s), *z, &mut run));
        }
    });
    println!("fast SIMD: {:.0} instr / tick ({:.0} per label), {:.2} us / tick", m_fast.instr, m_fast.instr / ticks[0].len() as f64, m_fast.ns / 1000.);
    let mut bufs: Vec<Buffer> = ticks[0]
        .iter()
        .map(|(_, z)| {
            let mut b = Buffer::new_empty(Metrics::new(*z, (*z * 1.25).round()));
            b.set_size(None, None);
            b
        })
        .collect();
    let m_cos = measure(&tick_items, 5 * rounds_scale, |&t| {
        for (i, (s, _)) in ticks[t].iter().enumerate() {
            let b = &mut bufs[i];
            b.set_text(black_box(s), &attrs, Shaping::Advanced, None);
            b.shape_until_scroll(&mut fs, false);
            let mut g = 0;
            for r in b.layout_runs() {
                g += r.glyphs.len();
            }
            black_box(g);
        }
        fs.shape_run_cache.trim(2);
    });
    println!("cosmic-text: {:.0} instr / tick ({:.0} per label), {:.2} us / tick", m_cos.instr, m_cos.instr / ticks[0].len() as f64, m_cos.ns / 1000.);
    println!("ratio: {:.0}x instructions\n", m_cos.instr / m_fast.instr);

    // Wrap vs cosmic line counts (sanity for the wrap helper).
    {
        let mut agree = 0;
        let mut total = 0;
        let mut starts = Vec::new();
        for (s, z) in &vlist {
            if !sh.shape_into(s, *z, &mut run) {
                continue;
            }
            let lines = wrap_greedy(&run, s.as_bytes(), (688.0 / *z * sh.script.upem as f32) as i32, &mut starts);
            let mut b = Buffer::new_empty(Metrics::new(*z, 20.));
            b.set_size(Some(688.001), None);
            b.set_text(s, &attrs, Shaping::Advanced, None);
            b.shape_until_scroll(&mut fs, false);
            let cl = b.layout_runs().count();
            total += 1;
            agree += (cl == lines) as usize;
        }
        println!("wrap helper vs cosmic-text line counts: {agree}/{total} paragraphs agree\n");
    }
    for (name, s, c, r) in summary {
        eprintln!("{name}: simd {s:.0} cosmic {c:.0} rustybuzz+plan {r:.0}");
    }
}
