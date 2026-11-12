//! Measurements for `bough_text::stage` (round 8, text lane). Predictions and kill criteria:
//! `docs/lanes/text/JOURNAL.md` section 1. Output: one TSV row per cell on stdout.
//!
//!     cargo run --profile dist -p bough-text --example stage_bench
//!
//! Counters are the calling thread's (`bough_mem::pmu`): instructions and cycles everywhere; on Linux
//! `BOUGH_PMU=l1d-misses,branch-misses` (groups of three at most) adds columns. A cell is the median of five
//! batches, the bracket's own cost subtracted. Corpora are regenerated with the round-7 scenes' own
//! generators (`comparisons/bough-workloads/src/{chat,ide}.rs` on `r7/bench`: same seeds and tables).
//!
//! Every cell is preceded by its gate: the fast path's answer equals the reference's, or the run stops.
//! `STAGE_MUTANT=hi|lo|join` breaks one mechanism on purpose (an interval one unit too wide at either end; an
//! append that restarts at the appended bytes): each must stop the run at a gate.
use bough_text::atlas::GlyphCache;
use bough_text::stage::brk::{self, Block, Brk};
use bough_text::stage::digits::{self, Cell};
use bough_text::stage::doc::Doc;
use bough_text::stage::flow::{Flow, Set};
use bough_text::stage::line::{self, GlyphW, Strike};
use bough_text::stage::memo::{self, Image, Memo};
use bough_text::{GlyphRun, Shaper, SimpleScript};
use std::hint::black_box;

#[path = "common/stage.rs"]
mod common;
use common::{chat_messages, ide_lines, Meter, NUMS, STRS, VOCAB};

fn font() -> (Vec<u8>, String) {
    let cands = [std::env::var("BOUGH_FONT").ok(), Some("/System/Library/Fonts/Supplemental/Arial.ttf".into()), Some("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf".into()), Some("/usr/share/fonts/TTF/DejaVuSans.ttf".into())];
    for p in cands.into_iter().flatten() {
        if let Ok(b) = std::fs::read(&p) {
            return (b, p);
        }
    }
    panic!("a TrueType font (set BOUGH_FONT)");
}

/// English letter frequencies per thousand bytes of prose (space first): the per-font mean advance `guess`
/// uses is the font's advances weighted by these. It is not fitted to the corpus measured below.
const FREQ: [(u8, u32); 27] = [(b' ', 180), (b'e', 102), (b't', 75), (b'a', 65), (b'o', 62), (b'i', 57), (b'n', 57), (b's', 53), (b'r', 50), (b'h', 50), (b'l', 33), (b'd', 33), (b'u', 23), (b'c', 22), (b'm', 20), (b'f', 18), (b'w', 17), (b'g', 16), (b'y', 16), (b'p', 15), (b'b', 12), (b'v', 8), (b'k', 6), (b'x', 1), (b'j', 1), (b'q', 1), (b'z', 1)];

fn main() {
    let (bytes, path) = font();
    let script = SimpleScript::build_cached(&bytes, 0).expect("simple-script table");
    let upem = script.upem;
    let sh = Shaper::new(script);
    let m = Meter::new();
    m.fact("font", "upem", upem as f64, &path);

    let msgs = chat_messages(2_000);
    let code = ide_lines(3_000);
    let chars: usize = msgs.iter().map(|s| s.len()).sum();
    m.fact("chat.corpus", "chars", chars as f64, "2,000 message bodies (the scene has 436,280 with quotes and pills)");

    // The chat bubble's content width in font units for a window `wpx` device pixels wide (scale 2):
    // ((W - sidebar 300 - row padding 32 - avatar and gap 46) * 0.74 - bubble padding 20) points, 13 pt text.
    let size_px = 26.0f64;
    let units = |wpx: f64| (((wpx / 2.0 - 378.0) * 0.74 - 20.0) * 2.0 * upem as f64 / size_px) as i32;

    t1(&m, &sh, &msgs, &code);
    let memo = t2(&m, &sh, &msgs, &units, upem);
    t3(&m, &sh);
    t4(&m, &sh, &msgs, units(2560.0));
    t5(&m, &sh, &code);
    t6(&m, &sh, &code);
    g1(&m, &sh, &bytes, &code, upem);
    scenes(&m, &sh, &bytes, &msgs, &code, units(2560.0), upem);
    drop(memo);
}

// ---- T1: the memo ------------------------------------------------------------------------------------------

fn t1(m: &Meter, sh: &Shaper, msgs: &[String], code: &[(String, Vec<u8>)]) {
    // a realistically full memo: every code line and every message
    let mut memo = Memo::new(64);
    for (l, _) in code {
        memo.get(0, sh, l).expect("covered");
    }
    for s in msgs {
        memo.get(0, sh, s).expect("covered");
    }
    m.fact("t1.memo.entries", "count", memo.len() as f64, "distinct texts of 3,000 code lines + 2,000 messages");
    let (t, e, g, b) = memo.bytes();
    m.fact("t1.memo.bytes", "bytes", (t + e + g + b) as f64, &format!("table {t}, entries {e}, glyph lanes {g}, break lanes {b}"));

    let mut run = GlyphRun::default();
    for len in [8usize, 16, 32, 64, 256] {
        // texts of exactly `len` bytes from the corpus, 64 of them
        let pool: Vec<&str> = msgs.iter().filter(|s| s.len() >= len).take(64).map(|s| &s[..len]).collect();
        for s in &pool {
            memo.get(0, sh, s).expect("covered");
        }
        // gate: a hit's glyphs are the shaper's
        for s in &pool {
            let off = memo.get(0, sh, s).unwrap();
            let e = memo.view().ent(off);
            assert!(sh.shape_into(s, 1.0, &mut run));
            assert_eq!(&memo.gid[e.g0 as usize..(e.g0 + e.gn) as usize], &run.gid[..]);
            assert_eq!(&memo.x[e.g0 as usize..(e.g0 + e.gn) as usize], &run.x[..]);
            assert_eq!(e.width, run.width);
        }
        let shape = m.cell(&format!("t1.shape.{len}"), "instr/text", 200, 64.0, "re-shaping into a reused run (the baseline a hit must beat)", || {
            for s in &pool {
                black_box(sh.shape_into(black_box(s), 1.0, &mut run));
            }
        });
        let v = memo.view();
        let hit = m.cell(&format!("t1.hit.{len}"), "instr/text", 200, 64.0, "key hashed at the call", || {
            for s in &pool {
                black_box(v.find(memo::key(0, s.as_bytes()), 0, black_box(s.as_bytes())));
            }
        });
        let keys: Vec<u64> = pool.iter().map(|s| memo::key(0, s.as_bytes())).collect();
        m.cell(&format!("t1.hit_keyed.{len}"), "instr/text", 200, 64.0, "key known (a constant string)", || {
            for (s, k) in pool.iter().zip(&keys) {
                black_box(v.find(*k, 0, black_box(s.as_bytes())));
            }
        });
        m.fact(&format!("t1.hit_over_shape.{len}"), "ratio", hit / shape, "kill at 16 bytes: above 1");
    }

    // a miss against shaping alone, on the messages
    let total: usize = msgs.iter().map(|s| s.len()).sum();
    let shape = m.cell("t1.shape.chat", "instr/byte", 3, total as f64, "shape every message into a reused run", || {
        for s in msgs {
            black_box(sh.shape_into(s, 1.0, &mut run));
        }
    });
    m.cell("t1.miss.chat.cold", "instr/byte", 3, total as f64, "a fresh memo per pass: hash, probe, shape, glyph lanes, entry; with growth and the first touch of its memory", || {
        let mut fresh = Memo::new(4096);
        for s in msgs {
            black_box(fresh.get(0, sh, s));
        }
    });
    let mut warm = Memo::new(4096);
    let miss = m.cell("t1.miss.chat", "instr/byte", 3, total as f64, "the same into a memo whose memory is already resident (cleared, not freed); no break table: a label's miss", || {
        warm.clear();
        for s in msgs {
            black_box(warm.get(0, sh, s));
        }
    });
    m.fact("t1.miss_over_shape", "ratio", miss / shape, "prediction: at most 1.15");
    m.cell("t1.miss_and_breaks.chat", "instr/byte", 3, total as f64, "and the break table of each (a paragraph's miss)", || {
        warm.clear();
        for s in msgs {
            let off = warm.get(0, sh, s).unwrap();
            black_box(warm.breaks(off));
        }
    });
}

// ---- T2: block(w), intervals, guess ------------------------------------------------------------------------

fn t2(m: &Meter, sh: &Shaper, msgs: &[String], units: &dyn Fn(f64) -> i32, upem: u16) -> Memo {
    let mutant = std::env::var("STAGE_MUTANT").ok();
    let mut memo = Memo::new(4096);
    let offs: Vec<u32> = msgs.iter().map(|s| memo.get(0, sh, s).unwrap()).collect();
    let ents: Vec<memo::Ent> = offs.iter().map(|&o| memo.breaks(o)).collect();
    let tab = |e: &memo::Ent| -> &[Brk] { &memo.brk[e.b0 as usize..(e.b0 + e.bn) as usize] };
    let chars: usize = msgs.iter().map(|s| s.len()).sum();
    m.fact("t2c.break_bytes_per_char.table", "bytes", (memo.brk.len() * 8 + memo.first.len() * 4) as f64 / chars as f64, "[end, next] and the segment's first glyph; prediction at most 2");
    m.fact("t2c.break_bytes_per_char.bytes", "bytes", 0.0, "block_bytes reads the pens and the text: nothing is stored for breaking");
    // the same paragraphs as (text, pens, width): what block_bytes reads
    let xs = |e: &memo::Ent| -> &[i32] { &memo.x[e.g0 as usize..(e.g0 + e.gn) as usize] };
    let txt: Vec<&[u8]> = msgs.iter().map(|s| s.as_bytes()).collect();

    // gate: the break table by bitmask edges is the walk's, on every message and on awkward texts
    {
        let mut run = GlyphRun::default();
        let odd = ["", " ", "a", " a", "a ", "  lead and trail  ", "two  spaces", "x y z", "12345678 ", " 2345678", "1234567 9", "hard\nbreak here", "abcdefgh ijklmnop  qrstuvwx yz"];
        for s in msgs.iter().map(|s| s.as_str()).chain(odd) {
            if !sh.shape_into(s, 1.0, &mut run) {
                // a control byte is outside the table shaper: give the walk a run of unit advances
                run.x = (0..s.len() as i32).map(|i| i * 100).collect();
                run.gid = vec![1; s.len()];
                run.width = s.len() as i32 * 100;
                run.identity_clusters = true;
            }
            let (mut a, mut af, mut b, mut bf) = (vec![[7, 7]], vec![7u32], vec![[7, 7]], vec![7u32]);
            let ea = brk::build(s.as_bytes(), &run, 100, 5, &mut a, &mut af);
            let eb = brk::build_walk(s.as_bytes(), &run.x, run.width, None, 100, 5, &mut b, &mut bf);
            assert_eq!((&a, &af, ea), (&b, &bf, eb), "{s:?}");
        }
    }
    // gate: block == the walk, and the interval is exact, at many widths
    for wpx in (1400..3300).step_by(37) {
        let w = units(wpx as f64);
        for e in &ents {
            let mut b = brk::block(tab(e), e.ext.hard, w);
            assert_eq!(b.lines, brk::block_walk(tab(e), w), "lines at {w}");
            match mutant.as_deref() {
                Some("hi") if b.hi < i32::MAX => b.hi += 1,
                Some("lo") if b.lo > 0 => b.lo -= 1,
                _ => {}
            }
            // the form with no table gives the same answer and the same line starts
            let t = txt[ents.iter().position(|x| x.g0 == e.g0).unwrap()];
            let mut starts = Vec::new();
            let bb = brk::block_from(t, xs(e), e.width, w, 0, 0, |g| starts.push(g));
            if mutant.is_none() {
                assert_eq!(bb, b, "block_bytes at {w}");
            }
            let f = &memo.first[e.b0 as usize..(e.b0 + e.bn) as usize];
            assert_eq!(starts, lines_at(tab(e), w).iter().map(|&i| f[i as usize]).collect::<Vec<_>>());
            assert!(b.lo <= w.max(b.lo) && w < b.hi, "{w} inside {b:?}");
            if b.lo > 0 {
                assert_eq!(lines_at(tab(e), b.lo), lines_at(tab(e), w), "holds at lo");
                assert_ne!(lines_at(tab(e), b.lo - 1), lines_at(tab(e), b.lo), "breaks change below lo");
                assert!(b.lo <= w, "lo above w");
            }
            if b.hi < i32::MAX {
                assert_eq!(brk::block_walk(tab(e), b.hi - 1), b.lines, "holds below hi");
                assert_ne!(lines_at(tab(e), b.hi), lines_at(tab(e), b.hi - 1), "breaks change at hi");
            }
        }
    }

    let w0 = units(2560.0);
    let mut sum = 0u64;
    let n = ents.len() as f64;
    let mut k = 0i32;
    m.cell("t2.block.new_width", "instr/para", 20, n, "block_bytes: no table, interpolation in the pen lane per line, with the interval; prediction at most 150", || {
        k += 1;
        for (e, t) in ents.iter().zip(&txt) {
            sum += black_box(brk::block_bytes(t, xs(e), e.width, w0 - (k & 15) * 40)).lines as u64;
        }
    });
    m.cell("t2.block_table.new_width", "instr/para", 20, n, "the same over a break table (the general form)", || {
        k += 1;
        for e in &ents {
            sum += black_box(brk::block(tab(e), e.ext.hard, w0 - (k & 15) * 40)).lines as u64;
        }
    });
    m.cell("t2.walk.new_width", "instr/para", 20, n, "every segment visited (the façade's greedy, on this table)", || {
        k += 1;
        for e in &ents {
            sum += black_box(brk::block_walk(tab(e), w0 - (k & 15) * 40)) as u64;
        }
    });
    let lines: u64 = ents.iter().map(|e| brk::block(tab(e), e.ext.hard, w0).lines as u64).sum();
    m.fact("t2.lines_per_para", "lines", lines as f64 / n, "at the 1280 pt window");
    m.fact("t2.segments_per_para", "segments", memo.brk.len() as f64 / n, "");

    // the sweep: one device pixel of window width per step, narrowing then widening
    let mut st: Vec<Block> = ents.iter().map(|e| brk::block(tab(e), e.ext.hard, w0)).collect();
    let (mut lo, mut hi): (Vec<i32>, Vec<i32>) = (st.iter().map(|b| b.lo).collect(), st.iter().map(|b| b.hi).collect());
    let mut redone = 0u64;
    let mut steps = 0u64;
    let mut height_changes = 0u64;
    let mut wpx = 2560i32;
    let mut dir = -1;
    let step = |wpx: i32, lo: &mut [i32], hi: &mut [i32], st: &mut [Block], redone: &mut u64, hc: &mut u64| {
        let w = units(wpx as f64);
        // classify 64 paragraphs into a mask (two vector compares), then act on the set bits only
        let mut c = 0;
        while c < lo.len() {
            let e = (c + 64).min(lo.len());
            let mut mask = 0u64;
            for k in c..e {
                mask |= (((w < lo[k]) | (w >= hi[k])) as u64) << (k - c);
            }
            while mask != 0 {
                let i = c + mask.trailing_zeros() as usize;
                mask &= mask - 1;
                let b = brk::block_bytes(txt[i], xs(&ents[i]), ents[i].width, w);
                *hc += (b.lines != st[i].lines) as u64;
                (lo[i], hi[i], st[i]) = (b.lo, b.hi, b);
                *redone += 1;
            }
            c = e;
        }
    };
    m.cell("t2.sweep.1px", "instr/step", 400, 1.0, "2,000 paragraphs: interval check for all, block() for those outside", || {
        wpx += dir;
        if wpx <= 2000 || wpx >= 2700 {
            dir = -dir;
        }
        step(wpx, &mut lo, &mut hi, &mut st, &mut redone, &mut height_changes);
        steps += 1;
    });
    m.fact("t2.sweep.1px.rebroken", "fraction", redone as f64 / (steps as f64 * n), "paragraphs outside their interval per step; prediction under 0.05, kill above 0.25");
    m.fact("t2.sweep.1px.height_changed", "fraction", height_changes as f64 / (steps as f64 * n), "of all paragraphs per step: what layout must act on");
    // gate: after the sweep every retained answer is the true one
    let w = units(wpx as f64);
    for (i, e) in ents.iter().enumerate() {
        assert_eq!(st[i].lines, brk::block_walk(tab(e), w));
    }

    // the harness's own resize sweep: a triangle wave between 60 % and 125 % of 1280 pt, 1/24 per step
    let tri = |t: f32| 1.0 - (2.0 * (t - (t + 0.5).floor())).abs();
    let sizes: Vec<i32> = (0..32).map(|k| ((1280.0 * (0.60 + 0.65 * tri(k as f32 / 24.0))).round() * 2.0) as i32).collect();
    let (mut redone, mut steps, mut hc, mut k) = (0u64, 0u64, 0u64, 0usize);
    m.cell("t2.sweep.harness", "instr/step", 32, 1.0, "the r7 resize phase's widths (about 69 pt per step)", || {
        step(sizes[k % 32], &mut lo, &mut hi, &mut st, &mut redone, &mut hc);
        k += 1;
        steps += 1;
    });
    m.fact("t2.sweep.harness.rebroken", "fraction", redone as f64 / (steps as f64 * n), "");

    // T2b: guess against the truth, and against a running mean of the first 15 rows
    let mut num = 0u64;
    let mut den = 0u64;
    for (c, f) in FREQ {
        num += sh.script.adv[c as usize] as u64 * f as u64;
        den += f as u64;
    }
    let mean_adv = (num / den) as i32;
    m.fact("t2b.mean_adv", "font units", mean_adv as f64, &format!("English letter frequencies; upem {upem}"));
    for wpx in [2000.0, 2560.0, 3200.0] {
        let w = units(wpx);
        let truth: Vec<u32> = ents.iter().map(|e| brk::block(tab(e), e.ext.hard, w).lines).collect();
        let total: u64 = truth.iter().map(|&l| l as u64).sum();
        let g: Vec<u32> = msgs.iter().map(|s| brk::guess(s.len() as u32, mean_adv, w)).collect();
        let gt: u64 = g.iter().map(|&l| l as u64).sum();
        let exact = g.iter().zip(&truth).filter(|(a, b)| a == b).count();
        let within1 = g.iter().zip(&truth).filter(|(a, b)| a.abs_diff(**b) <= 1).count();
        let mean15: f64 = truth[..15].iter().map(|&l| l as f64).sum::<f64>() / 15.0;
        let mt = mean15 * n;
        let m_exact = truth.iter().filter(|&&l| (l as f64 - mean15).abs() < 0.5).count();
        let m_within1 = truth.iter().filter(|&&l| (l as f64 - mean15).abs() <= 1.0).count();
        m.fact(&format!("t2b.guess.total_err.{wpx}"), "fraction", (gt as f64 - total as f64) / total as f64, "extent error before any row is measured; prediction within 0.03");
        m.fact(&format!("t2b.guess.rows_exact.{wpx}"), "fraction", exact as f64 / n, "");
        m.fact(&format!("t2b.guess.rows_within1.{wpx}"), "fraction", within1 as f64 / n, "prediction at least 0.90");
        m.fact(&format!("t2b.mean15.total_err.{wpx}"), "fraction", (mt - total as f64) / total as f64, "running mean of the first 15 rows (the layout lane's stand-in)");
        m.fact(&format!("t2b.mean15.rows_exact.{wpx}"), "fraction", m_exact as f64 / n, "");
        m.fact(&format!("t2b.mean15.rows_within1.{wpx}"), "fraction", m_within1 as f64 / n, "");
    }
    let mut s = 0u64;
    m.cell("t2b.guess", "instr/row", 50, n, "", || {
        for t in msgs {
            s += black_box(brk::guess(black_box(t.len() as u32), mean_adv, w0)) as u64;
        }
    });
    black_box((sum, s));
    memo
}

/// The line starts at a width, as a fingerprint of the break structure.
fn lines_at(b: &[Brk], w: i32) -> Vec<u32> {
    let mut v = Vec::new();
    brk::block_with(b, b.iter().any(|k| k[1] < 0), w, |i| v.push(i));
    v
}

// ---- T3: digits --------------------------------------------------------------------------------------------

fn t3(m: &Meter, sh: &Shaper) {
    for p in [
        "/System/Library/Fonts/Supplemental/Arial.ttf",
        "/System/Library/Fonts/Helvetica.ttc",
        "/System/Library/Fonts/Menlo.ttc",
        "/System/Library/Fonts/SFNS.ttf",
        "/System/Library/Fonts/Supplemental/Times New Roman.ttf",
        "/System/Library/Fonts/Supplemental/Georgia.ttf",
        "/System/Library/Fonts/Supplemental/Verdana.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
        "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
    ] {
        let Ok(b) = std::fs::read(p) else { continue };
        let note = match SimpleScript::build(&b, 0) {
            Some(sc) => format!("{:?}", Cell::of(&sc)),
            None => "no simple-script table".into(),
        };
        m.fact("t3.cell", "", 0.0, &format!("{p}: {note}"));
    }
    let Ok(cell) = Cell::of(&sh.script) else {
        m.fact("t3.skipped", "", 0.0, "the bench font's digits are not a cell");
        return;
    };
    // gate: glyph ids are the shaper's for the formatted string; pens are the sum of advances (the cell);
    // where the face kerns digit pairs the shaper's proportional pens differ, and by how much is reported
    let (mut s, mut run) = (String::new(), GlyphRun::default());
    let mut r = 0x1234_5678u32;
    let (mut checked, mut differ, mut worst) = (0u64, 0u64, 0i32);
    for i in 0..100_000 {
        r = r.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let v = match i % 3 {
            0 => (r >> 8) as i32 % 1_000_000,
            1 => -((r >> 12) as i32),
            _ => (r >> 3) as i32,
        };
        for d in [0u32, 2, 3] {
            digits::format(v, d, &mut s);
            assert!(sh.shape_into(&s, 1.0, &mut run));
            let n = digits::count(v, d);
            assert_eq!(n as usize, run.len());
            let mut pen = 0i32;
            let mut off = false;
            for (g, &c) in s.as_bytes().iter().enumerate() {
                let k = n - 1 - g as u32;
                assert_eq!(digits::glyph(v, d, k), c);
                assert_eq!(sh.script.gid[c as usize], run.gid[g], "{s} k={k}");
                assert_eq!(digits::pen(&cell, v, d, k), pen, "{s} k={k}");
                off |= run.x[g] != pen;
                worst = worst.max((run.x[g] - pen).abs());
                pen += sh.script.adv[c as usize] as i32;
            }
            assert_eq!(digits::width(&cell, v, d), pen, "{s}");
            assert!(cell.kern != 0 || !off, "a face with no digit kerning: the shaper's pens are the cell's");
            differ += off as u64;
            checked += 1;
        }
    }
    m.fact("t3.gate", "numbers", checked as f64, "glyph ids == the shaper's; pens and width == the sum of advances");
    m.fact("t3.kerned_numbers", "fraction", differ as f64 / checked as f64, &format!("numbers whose proportional setting (the shaper's, with digit-pair kerns) differs from the cell; worst pen difference {worst} font units; cell.kern {}", cell.kern));
    let vals: Vec<i32> = (0..256).map(|i| 1_000_000 + i * 7919).collect();
    let mut acc = 0i64;
    m.cell("t3.glyph_and_pen", "instr/glyph", 200, 256.0 * 8.0, "7 digits and a point, d = 2; prediction at most 25", || {
        for &v in &vals {
            let num = digits::Num::new(black_box(v), 2);
            for k in 0..num.count() {
                acc += num.glyph(k) as i64 + num.pen(&cell, k) as i64;
            }
        }
    });
    let mut out = [0u8; 16];
    let fmt = bough_text::numeric::NumFmt::new("", Some(2), "");
    m.cell("t3.facade_render_and_shape", "instr/number", 200, 256.0, "what a number write does in text today before any patching: render to bytes, shape", || {
        for &v in &vals {
            let n = fmt.render_f32(black_box(v) as f32 / 100.0, &mut out).unwrap();
            black_box(sh.shape_into(bough_text::numeric::as_str(&out, n), 1.0, &mut run));
        }
    });
    black_box(acc);
}

// ---- T4: appends -------------------------------------------------------------------------------------------

fn t4(m: &Meter, sh: &Shaper, msgs: &[String], w: i32) {
    let stream: String = msgs.iter().take(600).flat_map(|s| [s.as_str(), " "]).collect();
    // gate: a text appended six bytes at a time is the text shaped whole, and broken whole
    {
        let mut f = Flow::new(w);
        f.mutant_join = std::env::var("STAGE_MUTANT").as_deref() == Ok("join");
        let mut whole = String::new();
        for chunk in stream.as_bytes()[..6_000].chunks(6) {
            whole.push_str(std::str::from_utf8(chunk).unwrap());
            assert_eq!(f.set(sh, &whole), Set::Appended);
        }
        let mut g = Flow::new(w);
        assert_eq!(g.set(sh, &whole), Set::Appended);
        assert_eq!(f.gid, g.gid);
        assert_eq!(f.x, g.x);
        assert_eq!(f.lines, g.lines);
        let mut starts = Vec::new();
        brk::block_from(&f.text, &f.x, f.width, w, 0, 0, |g| starts.push(g));
        assert_eq!(f.lines, starts);
        assert!(f.lines.len() > 20);
        let mut run = GlyphRun::default();
        assert!(sh.shape_into(&whole, 1.0, &mut run));
        assert_eq!(f.x, run.x);
    }
    for kb in [1usize, 10, 100] {
        let base = &stream[..kb * 1024];
        let tail = &stream[kb * 1024..kb * 1024 + 6 * 400];
        let mut f = Flow::new(w);
        f.set(sh, base);
        let mut at = 0;
        let mut shaped = 0usize;
        m.cell(&format!("t4.append.{kb}k"), "instr/append", 80, 1.0, "6 bytes appended to a Flow; prediction at most 25 per byte + 300", || {
            f.append(sh, &tail[at..at + 6]);
            shaped += f.shaped;
            at = (at + 6) % (6 * 400 - 6);
        });
        m.fact(&format!("t4.append.{kb}k.shaped"), "bytes/append", shaped as f64 / 400.0, "the last segment plus the appended bytes");
        // through `set`, as a plain String field would arrive: the prefix compare is included
        let mut whole = String::from(base);
        let mut f = Flow::new(w);
        f.set(sh, &whole);
        let mut at = 0;
        m.cell(&format!("t4.set.{kb}k"), "instr/append", 80, 1.0, "the whole String handed over; the compare is the only term in the length", || {
            whole.push_str(&tail[at..at + 6]);
            black_box(f.set(sh, &whole));
            at = (at + 6) % (6 * 400 - 6);
        });
    }
}

// ---- T5: an edit touches one line --------------------------------------------------------------------------

fn t5(m: &Meter, sh: &Shaper, code: &[(String, Vec<u8>)]) {
    let mut run = GlyphRun::default();
    for n in [1_000usize, 300_000] {
        // the same place in a block and the same lines under the caret at both sizes (blocks are built 48
        // lines long), so that what differs between the two rows is the document's size alone
        let mid = (n / 2 / 48) * 48;
        let shift = code.len() - mid % code.len();
        let mut d = Doc::from_lines((0..n).map(|i| code[(i + shift) % code.len()].0.as_str()));
        let mut k = 0usize;
        m.cell(&format!("t5.type.{n}"), "instr/key", 2_000, 1.0, "one character into a line near the middle: find the line, storage splice, re-shape of that line; prediction at most 1,500", || {
            let line = mid + (k * 7) % 50;
            let at = d.at(line);
            let len = d.line_at(at).len();
            if len > 90 {
                d.splice_at(at, 3, len - 30, "");
            }
            d.splice_at(at, len.min(3), 0, "x");
            black_box(sh.shape_into(d.line_at(at), 1.0, &mut run));
            k += 1;
        });
        m.cell(&format!("t5.find.{n}"), "instr/line", 2_000, 1.0, "the line's text", || {
            black_box(d.line(mid + (k * 13) % 50).len());
            k += 1;
        });
        m.cell(&format!("t5.newline.{n}"), "instr/op", 1_000, 2.0, "split a line and join it back, re-shaping both halves; prediction at most 3,000 for the split", || {
            let line = mid + (k * 7) % 50;
            d.split(line, 4);
            black_box(sh.shape_into(d.line(line), 1.0, &mut run));
            black_box(sh.shape_into(d.line(line + 1), 1.0, &mut run));
            d.join(line);
            k += 1;
        });
        assert_eq!(d.len(), n);
    }
}

// ---- T6: the memo as an image ------------------------------------------------------------------------------

const K_MAIN: u64 = memo::key(0, b"main.rs");
const K_CARGO: u64 = memo::key(0, b"Cargo.toml");
const K_TITLE: u64 = memo::key(0, b"Release notes");
const K_LONG: u64 = memo::key(0, b"Deploys now run in the region you picked.");

fn t6(m: &Meter, sh: &Shaper, code: &[(String, Vec<u8>)]) {
    // the constant strings of a program: here tab names, vocabulary, tokens, titles (about 100) among the
    // 3,000 code lines, shaped by a build step
    let consts: Vec<&str> = ["main.rs", "lib.rs", "view.rs", "paint.rs", "state.rs", "theme.rs", "Cargo.toml", "list.rs", "input.rs", "scene.rs", "layout.rs", "README.md", "Release notes", "Deploys now run in the region you picked."]
        .into_iter()
        .chain(VOCAB)
        .chain(NUMS)
        .chain(STRS)
        .collect();
    let mut built = Memo::new(64);
    for s in &consts {
        built.get(0, sh, s).unwrap();
    }
    for (l, _) in code {
        built.get(0, sh, l).unwrap();
    }
    built.build_all_breaks();
    let words = built.to_image();
    m.fact("t6.image.bytes", "bytes", (words.len() * 8) as f64, &format!("{} entries", built.len()));
    // stand-in for rodata: the image on a 64-byte boundary, alive for the program
    let lay = std::alloc::Layout::from_size_align(words.len() * 8, 64).unwrap();
    // SAFETY: a fresh allocation of the image's size and alignment, filled before it is read, never freed.
    let img: &'static [u64] = unsafe {
        let p = std::alloc::alloc(lay) as *mut u64;
        assert!(!p.is_null());
        core::ptr::copy_nonoverlapping(words.as_ptr(), p, words.len());
        core::slice::from_raw_parts(p, words.len())
    };
    m.cell("t6.image.open", "instr", 1_000, 1.0, "header check; nothing parsed or copied", || {
        black_box(Image::new(black_box(img)).is_some());
    });
    let im = Image::new(img).expect("image");
    // gate: every text of the build is found in the image with the build's glyphs
    for s in consts.iter().copied().chain(code.iter().map(|c| c.0.as_str())) {
        let (a, b) = (built.view().find(memo::key(0, s.as_bytes()), 0, s.as_bytes()), im.view().find(memo::key(0, s.as_bytes()), 0, s.as_bytes()));
        assert!(a != 0 && a == b);
        let e = im.view().ent(b);
        assert_eq!(&im.lanes().gid[e.g0 as usize..(e.g0 + e.gn) as usize], &built.gid[e.g0 as usize..(e.g0 + e.gn) as usize]);
        assert_eq!(im.view().text(b), s.as_bytes());
    }
    let v = im.view();
    let keys: Vec<u64> = consts.iter().map(|s| memo::key(0, s.as_bytes())).collect();
    let n = consts.len() as f64;
    let mut acc = 0u64;
    m.cell("t6.mount.const_label", "instr/label", 500, n, "lookup in the image with the key known, entry read: all a constant label's mount needs of text; prediction at most 40", || {
        for (s, k) in consts.iter().zip(&keys) {
            let off = v.find(*k, 0, black_box(s.as_bytes()));
            acc += v.ent(off).gn as u64;
        }
    });
    m.cell("t6.mount.hashed_label", "instr/label", 500, n, "the same with the key hashed at mount", || {
        for s in &consts {
            let off = v.find(memo::key(0, black_box(s.as_bytes())), 0, s.as_bytes());
            acc += v.ent(off).gn as u64;
        }
    });
    // does the key of a literal fold? Four literals with `const` keys against the same four hashed in line.
    m.cell("t6.literal.const_key", "instr/label", 2_000, 4.0, "", || {
        acc += v.find(K_MAIN, 0, b"main.rs") as u64 + v.find(K_CARGO, 0, b"Cargo.toml") as u64 + v.find(K_TITLE, 0, b"Release notes") as u64 + v.find(K_LONG, 0, b"Deploys now run in the region you picked.") as u64;
        black_box(acc);
    });
    m.cell("t6.literal.inline_key", "instr/label", 2_000, 4.0, "equal to the row above if the compiler folds key() of a literal", || {
        acc += lit(v, "main.rs") as u64 + lit(v, "Cargo.toml") as u64 + lit(v, "Release notes") as u64 + lit(v, "Deploys now run in the region you picked.") as u64;
        black_box(acc);
    });
    let mut run = GlyphRun::default();
    m.cell("t6.shape.const_label", "instr/label", 500, n, "shaping the same labels at mount instead", || {
        for s in &consts {
            black_box(sh.shape_into(black_box(s), 1.0, &mut run));
        }
    });
    black_box(acc);
}

/// What `text("..")` would do at mount for a literal: the key is computed here, inlined into the caller.
#[inline(always)]
fn lit(v: memo::View<'_>, s: &'static str) -> u32 {
    v.find(memo::key(0, s.as_bytes()), 0, s.as_bytes())
}

// ---- G1: the two glyph forms -------------------------------------------------------------------------------

/// A strike with a slot for every glyph the shaper produces for `texts` (ligatures included) and for ASCII.
fn strike_for<'a>(sh: &Shaper, font: &[u8], size: f32, upem: u16, texts: impl Iterator<Item = &'a str>) -> (GlyphCache, Strike) {
    let mut cache = GlyphCache::new(font);
    let sid = cache.strike(size);
    let mut gids: Vec<u16> = (0x20u8..0x7f).map(|c| sh.script.gid[c as usize]).collect();
    let mut run = GlyphRun::default();
    for t in texts {
        if sh.shape_into(t, 1.0, &mut run) {
            gids.extend_from_slice(&run.gid);
        }
    }
    gids.sort_unstable();
    gids.dedup();
    let st = Strike::build(&mut cache, sid, upem, gids.into_iter());
    (cache, st)
}

fn g1(m: &Meter, sh: &Shaper, font: &[u8], code: &[(String, Vec<u8>)], upem: u16) {
    let (cache, st) = strike_for(sh, font, 24.0, upem, code.iter().map(|c| c.0.as_str()));
    let mut memo = Memo::new(64);
    let offs: Vec<u32> = code.iter().map(|(l, _)| memo.get(0, sh, l).unwrap()).collect();
    let ents: Vec<memo::Ent> = offs.iter().map(|&o| memo.view().ent(o)).collect();
    let glyphs: usize = ents.iter().map(|e| e.gn as usize).sum();
    let range = |e: &memo::Ent| (e.g0 as usize, (e.g0 + e.gn) as usize);

    // placed form: produced per line at mount
    let mut placed: Vec<GlyphW> = Vec::with_capacity(glyphs);
    let mut spans = Vec::with_capacity(ents.len());
    for e in &ents {
        let (a, b) = range(e);
        let p0 = placed.len();
        line::place(&st, &memo.gid[a..b], &memo.x[a..b], 0, 0, &mut placed);
        spans.push((p0, placed.len()));
    }
    let inked = placed.len();
    m.fact("g1.glyphs", "count", glyphs as f64, &format!("{} with ink, 3,000 code lines at 24 px", inked));
    m.fact("g1.bytes_per_glyph.placed", "bytes", 12.0 * inked as f64 / glyphs as f64, "12 per inked glyph, plus the shaped lanes kept for hit testing");
    m.fact("g1.bytes_per_glyph.shaped", "bytes", 6.0, "the memo's lanes; nothing per mount");

    // gate: both forms emit the same boxes and texels, and cover the same pixels
    let (tw, th) = (2048i32, 40i32);
    let (mut ta, mut tb) = (vec![0u8; (tw * th) as usize], vec![0u8; (tw * th) as usize]);
    let (atlas, aw) = (&cache.atlas.pixels[..], cache.atlas.width);
    for (e, &(p0, p1)) in ents.iter().zip(&spans).take(400) {
        let (a, b) = range(e);
        let (mut va, mut vb) = (Vec::new(), Vec::new());
        line::each_placed(&placed[p0..p1], (10, 30), |x, y, w, h, t| va.push((x, y, w, h, t)));
        line::each_shaped(&st, &memo.gid[a..b], &memo.x[a..b], 0, (10, 30), |x, y, w, h, t| vb.push((x, y, w, h, t)));
        assert_eq!(va, vb);
        ta.fill(0);
        tb.fill(0);
        for &(x, y, w, h, t) in &va {
            line::cover(&mut ta, tw, atlas, aw, x, y, w, h, t);
        }
        line::each_shaped(&st, &memo.gid[a..b], &memo.x[a..b], 0, (10, 30), |x, y, w, h, t| line::cover(&mut tb, tw, atlas, aw, x, y, w, h, t));
        assert_eq!(ta, tb);
        assert!(ta.iter().any(|&c| c != 0) || e.gn == 0);
    }

    let g = glyphs as f64;
    let mut out: Vec<GlyphW> = Vec::with_capacity(glyphs);
    m.cell("g1.place", "instr/glyph", 20, g, "producing the placed form (every mount, re-break and size)", || {
        out.clear();
        for e in &ents {
            let (a, b) = range(e);
            line::place(&st, &memo.gid[a..b], &memo.x[a..b], 0, 0, &mut out);
        }
        black_box(out.len());
    });
    let mut acc = 0u64;
    let a_setup = m.cell("g1.setup.placed", "instr/glyph", 20, g, "the kernel's per-glyph work before the blit, from 12-byte glyph words", || {
        for &(p0, p1) in &spans {
            line::each_placed(&placed[p0..p1], (10, 30), |x, y, w, h, t| acc = acc.wrapping_add((x + y) as u64 + (w + h + t) as u64));
        }
    });
    let b_setup = m.cell("g1.setup.shaped", "instr/glyph", 20, g, "the same from the memo's lanes and the strike's slots (a multiply and a table load)", || {
        for e in &ents {
            let (a, b) = range(e);
            line::each_shaped(&st, &memo.gid[a..b], &memo.x[a..b], 0, (10, 30), |x, y, w, h, t| acc = acc.wrapping_add((x + y) as u64 + (w + h + t) as u64));
        }
    });
    m.fact("g1.setup.placed_over_shaped", "ratio", a_setup / b_setup, "prediction: at most 0.85; under 0.95 keeps the placed form");
    m.cell("g1.paint.placed", "instr/glyph", 5, g, "setup and blit into an R8 line target", || {
        for &(p0, p1) in &spans {
            line::each_placed(&placed[p0..p1], (10, 30), |x, y, w, h, t| line::cover(&mut ta, tw, atlas, aw, x, y, w, h, t));
        }
    });
    m.cell("g1.paint.shaped", "instr/glyph", 5, g, "", || {
        for e in &ents {
            let (a, b) = range(e);
            line::each_shaped(&st, &memo.gid[a..b], &memo.x[a..b], 0, (10, 30), |x, y, w, h, t| line::cover(&mut tb, tw, atlas, aw, x, y, w, h, t));
        }
    });
    black_box(acc);
}

// ---- S1, S2: the text side of first paint ------------------------------------------------------------------

fn scenes(m: &Meter, sh: &Shaper, font: &[u8], msgs: &[String], code: &[(String, Vec<u8>)], w: i32, upem: u16) {
    let (_cache, st) = strike_for(sh, font, 26.0, upem, msgs.iter().map(|s| s.as_str()));
    let mut keep = Memo::new(4096);
    let mut sum = 0u64;
    let exposed = &msgs[msgs.len() - 20..];
    let ex_chars: usize = exposed.iter().map(|s| s.len()).sum();
    let rest = &msgs[..msgs.len() - 20];
    let rest_chars: usize = rest.iter().map(|s| s.len()).sum();
    let tree: Vec<String> = (0..40).map(|i| format!("{}_{}.rs", VOCAB[i % 24], VOCAB[(i * 7 + 3) % 24])).collect();
    let term: Vec<String> = (0..30).map(|i| format!("[{:>4}] build {} ok in 0.{}s", i * 17, VOCAB[i % 24], 10 + i)).collect();
    let n_chars: usize = code[..60].iter().map(|c| c.0.len()).sum::<usize>() + tree.iter().chain(&term).map(|s| s.len()).sum::<usize>();
    // A line record over the memo's lanes: (first glyph, one past the last, the pen the line starts at).
    let mut lines_out: Vec<(u32, u32, i32)> = Vec::new();
    let mut placed: Vec<GlyphW> = Vec::new();
    let mut styles: Vec<u8> = Vec::new();
    let mut recs: Vec<(u32, u32)> = Vec::new();
    // The lines of one message at `w`, as records.
    fn lines_of(memo: &mut Memo, sh: &Shaper, s: &str, w: i32, out: &mut Vec<(u32, u32, i32)>) {
        let off = memo.get(0, sh, s).unwrap();
        let e = memo.view().ent(off);
        let x = &memo.x[e.g0 as usize..(e.g0 + e.gn) as usize];
        let at = out.len();
        brk::block_from(s.as_bytes(), x, e.width, w, 0, 0, |g| out.push((e.g0 + g, 0, x[g as usize])));
        for k in at..out.len() {
            out[k].1 = if k + 1 < out.len() { out[k + 1].0 } else { e.g0 + e.gn };
        }
    }
    // cold: a fresh memo per pass (growth and the first touch of its memory are inside the cell, as at a
    // process's first paint); warm: the same memo cleared (the text work alone)
    for (tag, cold) in [("cold", true), ("warm", false)] {
        macro_rules! memo {
            ($buckets:expr) => {{
                if cold {
                    keep = Memo::new($buckets);
                } else {
                    keep.clear();
                }
                &mut keep
            }};
        }
        // S1: the 20 newest messages are exposed
        let s1 = m.cell(&format!("s1.chat.first_paint.shaped.{tag}"), "instr", 20, 1.0, &format!("20 exposed messages ({ex_chars} chars): memo, shape, lines at the width, line records over the memo's lanes; prediction at most 100,000"), || {
            let memo = memo!(64);
            lines_out.clear();
            for s in exposed {
                lines_of(memo, sh, s, w, &mut lines_out);
            }
            black_box(lines_out.len());
        });
        m.fact(&format!("s1.chat.first_paint.per_char.{tag}"), "instr/char", s1 / ex_chars as f64, &format!("{} line records", lines_out.len()));
        m.cell(&format!("s1.chat.first_paint.placed.{tag}"), "instr", 20, 1.0, "the same, and every line's glyphs placed into 12-byte words", || {
            let memo = memo!(64);
            lines_out.clear();
            placed.clear();
            for s in exposed {
                let at = lines_out.len();
                lines_of(memo, sh, s, w, &mut lines_out);
                for &(a, z, x0) in &lines_out[at..] {
                    line::place(&st, &memo.gid[a as usize..z as usize], &memo.x[a as usize..z as usize], x0, 0, &mut placed);
                }
            }
            black_box(placed.len());
        });
        // the other 1,980 messages, measured for their heights: glyphs kept in the memo, or nothing kept
        m.cell(&format!("s1.chat.sweep.memo.{tag}"), "instr", 3, 1.0, &format!("1,980 unexposed messages ({rest_chars} chars) measured through the memo (glyph lanes kept); prediction at most 6 M"), || {
            let memo = memo!(4096);
            for s in rest {
                let off = memo.get(0, sh, s).unwrap();
                let e = memo.view().ent(off);
                sum += brk::block_bytes(s.as_bytes(), &memo.x[e.g0 as usize..(e.g0 + e.gn) as usize], e.width, w).lines as u64;
            }
        });
        // S2: the ide's exposed text: 60 code lines (one line record each, a style lane beside the glyphs),
        // 40 tree rows, 30 terminal lines
        m.cell(&format!("s2.ide.first_paint.{tag}"), "instr", 50, 1.0, &format!("130 line records ({n_chars} chars): memo, shape, style lane copied; prediction at most 150,000"), || {
            let memo = memo!(64);
            styles.clear();
            recs.clear();
            for (l, classes) in &code[..60] {
                let off = memo.get(0, sh, l).unwrap();
                let e = memo.view().ent(off);
                styles.extend_from_slice(classes);
                recs.push((e.g0, e.gn));
            }
            for l in tree.iter().chain(&term) {
                let off = memo.get(0, sh, l).unwrap();
                let e = memo.view().ent(off);
                recs.push((e.g0, e.gn));
            }
            black_box((recs.len(), styles.len()));
        });
    }
    let mut run = GlyphRun::default();
    m.cell("s1.chat.sweep.size_only", "instr", 3, 1.0, "the same messages, keeping nothing but (lines, lo, hi): shape into a scratch run, block_bytes over it", || {
        for s in rest {
            sh.shape_into(s, 1.0, &mut run);
            sum += brk::block_bytes(s.as_bytes(), &run.x, run.width, w).lines as u64;
        }
    });
    black_box(sum);
}
