//! Round 3 gates for the ring of lines (`lines::LineSet`):
//!
//! * a **streamed log** (heavy's pane: the zgx workload's 5-line window, one new line per tick, `set_text` of the
//!   joined string, and the structural `push_line` form) equals the same text shaped in one go (a fresh set) at
//!   every tick, slot for slot and header for header, over 3,000 ticks (which include the `ff` ligature lines of
//!   the hex checksums and their Latin-1 twins);
//! * **random line edits** (insert, delete, replace, shift, append, empty lines, Latin-1, ligatures) leave the
//!   retained set equal to a fresh one, and the marked tiles cover every pixel that changed (instances expanded
//!   by the shader's arithmetic, `GlyphPool::expand`).
#[path = "../examples/common/corpus.rs"]
mod corpus;

use bough_text::atlas::{GlyphCache, Instance};
use bough_text::lines::{LineSet, LineStyle};
use bough_text::pool::GlyphPool;
use bough_text::{Shaper, SimpleScript};

const FONT: &[u8] = zgx_workload::FONT_BYTES;

struct Env {
    sh: Shaper,
    cache: GlyphCache,
    strike: usize,
    bounds: bough_text::atlas::LineBounds,
}

fn env() -> Env {
    let sh = Shaper::new(SimpleScript::build(FONT, 0).unwrap());
    let mut cache = GlyphCache::new(FONT);
    let strike = cache.strike(22.0);
    let tt = sh.script.tokens.as_ref().unwrap();
    cache.prepare_tokens(strike, &tt.gid, 0..256u16);
    let bounds = cache.line_bounds(strike, tt, sh.script.upem);
    Env { sh, cache, strike, bounds }
}

fn style(e: &Env) -> LineStyle {
    LineStyle { ox: 24.5, oy: 1385.0, lh: 32.0, scale: 22.0 / e.sh.script.upem as f32, color: 0xff8b98ab, space: 0, strike: e.strike as u16 }
}

/// What the set draws: per header, its expanded instances (the shader's arithmetic) and its slots.
fn drawn(pool: &GlyphPool, cache: &GlyphCache, set: &LineSet) -> Vec<(Vec<Instance>, Vec<i32>, Vec<u16>)> {
    (0..set.cap)
        .map(|i| {
            let r = set.first + i;
            let mut out = Vec::new();
            pool.expand(cache, r, &mut out);
            let h = pool.runs[r as usize];
            let (a, b) = (h.base as usize, (h.base + h.len) as usize);
            (out, pool.x[a..b].iter().map(|v| v - h.x0).collect::<Vec<i32>>(), pool.ent[a..b].to_vec())
        })
        .filter(|d| !d.1.is_empty() || !d.0.is_empty())
        .collect()
}

fn fresh(e: &Env, text: &str) -> (GlyphPool, LineSet) {
    let mut pool = GlyphPool::new();
    let mut set = LineSet::new(style(e));
    set.set_text::<[u128]>(&mut pool, &e.sh, text, &e.bounds, None).expect("covered");
    (pool, set)
}

fn tiles_of(ins: &[Instance]) -> Vec<(i32, i32)> {
    let mut t = Vec::new();
    for g in ins {
        let (x0, y0, x1, y1) = (g.xy[0] as i32, g.xy[1] as i32, (g.xy[0] + g.wh[0]).ceil() as i32, (g.xy[1] + g.wh[1]).ceil() as i32);
        for ty in (y0 >> 5)..=((y1 - 1) >> 5) {
            for tx in (x0 >> 5)..=((x1 - 1) >> 5) {
                t.push((tx, ty));
            }
        }
    }
    t
}

#[test]
fn streamed_log_equals_the_text_shaped_in_one_go() {
    let e = env();
    let lines = corpus::heavy_log_lines(3005);
    // Latin-1 and ligature twins of some lines (the token lanes and the scalar tokenizer in a stream)
    let lines: Vec<String> = lines
        .into_iter()
        .enumerate()
        .map(|(i, l)| match i % 7 {
            3 => l.replace("queue", "quéue"),
            5 => l.replace("latency", "efficient latency"),
            _ => l,
        })
        .collect();
    let mut pool = GlyphPool::new();
    let mut set = LineSet::new(style(&e));
    let mut pool2 = GlyphPool::new();
    let mut ring = LineSet::new(style(&e));
    ring.max_lines = 5;
    let mut plane = vec![0u128; 64];
    let (mut kept, mut shaped, mut lig) = (0, 0, 0);
    for t in 0..3000 {
        let text = lines[t..t + 5].join("\n");
        let st = set.set_text(&mut pool, &e.sh, &text, &e.bounds, Some(plane.as_mut_slice())).unwrap();
        if t > 0 {
            assert_eq!((st.shaped, st.kept), (1, 4), "tick {t}: one line shaped, four kept");
            ring.push_line(&mut pool2, &e.sh, &lines[t + 4], &e.bounds, Some(plane.as_mut_slice())).unwrap();
        } else {
            for l in &lines[0..5] {
                ring.push_line(&mut pool2, &e.sh, l, &e.bounds, Some(plane.as_mut_slice())).unwrap();
            }
        }
        kept += st.kept;
        shaped += st.shaped;
        lig += text.contains("ff") as u32;
        let (fp, fs) = fresh(&e, &text);
        let want = drawn(&fp, &e.cache, &fs);
        assert_eq!(drawn(&pool, &e.cache, &set), want, "tick {t}: retained set differs from the text shaped in one go");
        assert_eq!(drawn(&pool2, &e.cache, &ring), want, "tick {t}: push_line ring differs");
        assert_eq!(set.text(), text.as_bytes());
        assert_eq!(ring.text(), text.as_bytes());
    }
    println!("3000 ticks: {kept} lines kept, {shaped} shaped, {lig} ticks with a ligature line");
    assert!(lig > 100);
}

#[test]
fn random_line_edits_equal_a_fresh_set_and_marks_cover_the_change() {
    let e = env();
    let words = ["alpha", "beta", "office", "fluff", "café", "Zürich", "AVAT", "x", "", "1234.5 ms", "naïve", "ffi", "Ta"];
    let mut seed = 0x2545_F491_4F6C_DD1Du64;
    let mut rnd = move |n: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed % n.max(1) as u64) as usize
    };
    let line = |rnd: &mut dyn FnMut(usize) -> usize| -> String {
        let k = rnd(6);
        (0..k).map(|_| words[rnd(words.len())]).collect::<Vec<_>>().join(" ")
    };
    let mut pool = GlyphPool::new();
    let mut set = LineSet::new(style(&e));
    let mut cur: Vec<String> = (0..6).map(|_| line(&mut rnd)).collect();
    set.set_text::<[u128]>(&mut pool, &e.sh, &cur.join("\n"), &e.bounds, None).unwrap();
    let mut stats = [0u32; 2];
    for step in 0..20_000 {
        match rnd(7) {
            0 => {
                let i = rnd(cur.len() + 1);
                cur.insert(i, line(&mut rnd));
            }
            1 if cur.len() > 1 => {
                let i = rnd(cur.len());
                cur.remove(i);
            }
            2 => {
                let i = rnd(cur.len());
                cur[i] = line(&mut rnd);
            }
            3 if cur.len() > 2 => {
                let k = 1 + rnd(2);
                cur.drain(..k);
                for _ in 0..k {
                    cur.push(line(&mut rnd));
                }
            }
            4 => cur.push(line(&mut rnd)),
            5 => {} // unchanged
            _ => {
                let i = rnd(cur.len());
                cur[i].push_str(words[rnd(words.len())]);
            }
        }
        if cur.len() > 40 {
            cur.truncate(20);
        }
        let text = cur.join("\n");
        let before = drawn_by_header(&pool, &e.cache, &set);
        let mut plane = vec![0u128; 128];
        let st = set.set_text(&mut pool, &e.sh, &text, &e.bounds, Some(plane.as_mut_slice())).unwrap();
        stats[0] += st.kept;
        stats[1] += st.shaped;
        let (fp, fs) = fresh(&e, &text);
        assert_eq!(drawn(&pool, &e.cache, &set), drawn(&fp, &e.cache, &fs), "step {step}: {text:?}");
        // damage: every tile of an instance that differs (old or new, by header) is marked
        let after = drawn_by_header(&pool, &e.cache, &set);
        let n = before.len().max(after.len());
        for i in 0..n {
            let (a, b) = (before.get(i).cloned().unwrap_or_default(), after.get(i).cloned().unwrap_or_default());
            if a != b {
                for (tx, ty) in tiles_of(&a).into_iter().chain(tiles_of(&b)) {
                    if (0..128).contains(&tx) && (0..128).contains(&ty) {
                        assert!(plane[ty as usize] >> tx & 1 == 1, "step {step}: line {i} changed but tile ({tx},{ty}) not marked");
                    }
                }
            }
        }
    }
    println!("random edits: {} lines kept, {} shaped", stats[0], stats[1]);
}

/// Expanded instances by header index (absolute pixels: line origins included).
fn drawn_by_header(pool: &GlyphPool, cache: &GlyphCache, set: &LineSet) -> Vec<Vec<Instance>> {
    (0..set.lines.len() as u32)
        .map(|i| {
            let mut out = Vec::new();
            pool.expand(cache, set.first + i, &mut out);
            out
        })
        .collect()
}
