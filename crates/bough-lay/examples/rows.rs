//! What a row's layout costs: the monomorphised `place` of the scenes' row types against the general solver
//! on the same style tree. Instructions (and whatever else `BOUGH_PMU` names) per row, the measuring bracket
//! amortised over `N` rows, median of `REPS` batches.
//!
//!   cargo run --profile dist -p bough-lay --example rows
#[path = "../tests/common/mod.rs"]
mod common;
#[path = "../scenes/rows.rs"]
mod rows;
use bough_expr::U;
use bough_lay::*;
use bough_mem::pmu;
use std::hint::black_box;

/// Rect lanes of one page, as the store has them: four arrays, the record's slot the index.
#[repr(C, align(64))]
struct Lanes {
    x0: [i32; 64],
    y0: [i32; 64],
    x1: [i32; 64],
    y1: [i32; 64],
    at: i32,
}
impl Out for Lanes {
    #[inline(always)]
    fn put(&mut self, k: usize, r: [i32; 4]) {
        // (one row's records, at the row's y: what the list sink does)
        let k = k & 63;
        self.x0[k] = r[0];
        self.y0[k] = r[1] + self.at;
        self.x1[k] = r[2];
        self.y1[k] = r[3] + self.at;
    }
}

fn cell(name: &str, n: usize, reps: usize, mut f: impl FnMut(usize)) {
    let ev = pmu::sample_events();
    let mut rows: Vec<[u64; pmu::MAX]> = Vec::new();
    for _ in 0..reps {
        let a = pmu::sample();
        for i in 0..n {
            f(i);
        }
        rows.push(pmu::sample().since(&a).v);
    }
    let mut line = format!("{name:<44}");
    for (i, e) in ev.iter().enumerate() {
        let mut x: Vec<u64> = rows.iter().map(|r| r[i]).collect();
        x.sort();
        line += &format!("\t{}={:.1}", e.name(), x[x.len() / 2] as f64 / n as f64);
    }
    println!("{line}");
}

#[inline(never)]
fn sheet_memo(memo: &mut Memo, r: usize, width: f32, u: &U, out: &mut Lanes) -> i32 {
    memo.row(&rows::sheet_row(r), 2.0, u, width, (0, r as i32 * 44), out)
}
#[inline(never)]
fn sheet_hash(r: usize) -> u64 {
    bough_lay::shape::hash(&rows::sheet_row(r))
}
#[inline(never)]
fn sheet_place(r: usize, width: f32, u: &U, out: &mut Lanes) -> i32 {
    bough_lay::shape::place(&rows::sheet_row(r), 2.0, u, width, out).0
}
#[inline(never)]
fn sheet_copy(shape: &[[i32; 4]; 22], out: &mut Lanes) {
    for (k, r) in shape.iter().enumerate() {
        out.put(k, *r);
    }
}
#[inline(never)]
fn ide_memo(memo: &mut Memo, n: usize, toks: &[(f32, f32, f32); rows::MAX_TOK], tail: f32, width: f32, u: &U, out: &mut Lanes) -> i32 {
    memo.row(&rows::ide_line(n, toks, tail), 2.0, u, width, (0, 0), out)
}
#[inline(never)]
fn ide_hash(n: usize, toks: &[(f32, f32, f32); rows::MAX_TOK], tail: f32) -> u64 {
    bough_lay::shape::hash(&rows::ide_line(n, toks, tail))
}
#[inline(never)]
fn chat_place(m: &rows::Msg, width: f32, u: &U, out: &mut Lanes) -> (i32, Valid) {
    bough_lay::shape::place(&rows::chat_row(m), 2.0, u, width, out)
}
#[inline(never)]
fn chat_size(m: &rows::Msg, width: f32, u: &U) -> (i32, Valid) {
    bough_lay::shape::size(&rows::chat_row(m), 2.0, u, width)
}

fn main() {
    let reps: usize = std::env::var("REPS").ok().and_then(|v| v.parse().ok()).unwrap_or(15);
    let u = U::default();
    // SAFETY: plain integers; zero is a valid value of every field.
    let mut out: Box<Lanes> = unsafe { Box::new_zeroed().assume_init() };
    let width = black_box(2560.0f32);
    println!("# per row; events: {:?}", pmu::sample_events().iter().map(|e| e.name()).collect::<Vec<_>>());

    let mut memo = Memo::new();
    cell("sheet row: shape, memo hit, copy (22 rects)", 4096, reps, |i| {
        black_box(sheet_memo(&mut memo, black_box(i), width, &u, &mut out));
    });
    cell("sheet row: the shape alone (hash of its words)", 4096, reps, |i| {
        black_box(sheet_hash(black_box(i)));
    });
    let mut shape = [[0i32; 4]; 22];
    bough_lay::shape::place(&rows::sheet_row(0), 2.0, &u, 2560.0, &mut shape[..]);
    cell("sheet row: copy of a kept slice (22 rects)", 4096, reps, |i| {
        out.at = i as i32 * 44;
        sheet_copy(black_box(&shape), &mut out);
    });
    cell("sheet row: memo miss (words + layout)", 1024, reps, |i| {
        black_box(sheet_place(black_box(i), width, &u, &mut out));
    });
    let toks: Vec<(usize, [(f32, f32, f32); rows::MAX_TOK], f32)> = {
        let mut r = common::Rng(0x1de);
        (0..256).map(|_| (3 + r.below(6), std::array::from_fn(|_| (13.2 * (1 + r.below(9)) as f32, r.pick(&[0.0, 3.8, 14.0]), r.pick(&[0.0, 3.8]))), 26.4)).collect()
    };
    let mut memo = Memo::new();
    cell("ide line: shape + layout (every line its own)", 4096, reps, |i| {
        let (n, t, tail) = &toks[i & 255];
        black_box(ide_memo(&mut memo, *n, t, *tail, black_box(1860.0), &u, &mut out));
    });
    cell("ide line: the shape alone (hash of its words)", 4096, reps, |i| {
        let (n, t, tail) = &toks[i & 255];
        black_box(ide_hash(*n, t, *tail));
    });
    let msgs: Vec<rows::Msg> = (0..256).map(rows::msg).collect();
    let words: usize = msgs.iter().map(|m| m.text.words.len() + m.quote.as_ref().map_or(0, |q| q.1.words.len())).sum();
    println!("# chat: {} messages, {:.1} words each (the stand-in paragraph keeps its last answer: a question inside that interval is a range check, any other walks the words)", msgs.len(), words as f64 / 256.0);
    cell("chat row: words + layout, one width", 2048, reps, |i| {
        black_box(chat_place(&msgs[i & 255], black_box(1960.0), &u, &mut out));
    });
    let mut step = 0usize;
    cell("chat row: words + layout, a new width each time", 2048, reps, |i| {
        step += 1;
        black_box(chat_place(&msgs[i & 255], black_box(1500.0 + (step % 997) as f32), &u, &mut out));
    });
    cell("chat row: size only", 2048, reps, |i| {
        black_box(chat_size(&msgs[i & 255], black_box(1960.0), &u));
    });
    // how often an answer survives a one-pixel step
    let (mut held, mut total) = (0, 0);
    for m in &msgs {
        let mut v = Valid { lo: [1.0; 2], hi: [0.0; 2] };
        for w in 1500..1900 {
            total += 1;
            if v.holds([w as f32, 0.0]) {
                held += 1;
            } else {
                v = chat_size(m, w as f32, &u).1;
            }
        }
    }
    println!("# chat rows over 400 one-pixel steps: {:.1} % of (row, step) pairs are a range check", 100.0 * held as f64 / total as f64);

    // the general solver on the same trees
    let sheet_tree = bough_lay::shape::tree(&rows::sheet_row(3));
    cell("sheet row: cephal, cold (tree built each time)", 256, reps, |_| {
        black_box(common::oracle_row(&sheet_tree, 2.0, 2560.0));
    });
    let chat_trees: Vec<_> = msgs.iter().map(|m| bough_lay::shape::tree(&rows::chat_row(m))).collect();
    cell("chat row: cephal, cold (tree built each time)", 256, reps, |i| {
        black_box(common::oracle_row(&chat_trees[i & 255], 2.0, 1960.0));
    });
    // (a width no row has seen before, every time: the counter runs across batches)
    let mut step = 0usize;
    let mut warm = common::Warm::new(&sheet_tree, 2.0, 2560.0);
    cell("sheet row: cephal, kept tree, new width", 256, reps, |_| {
        step += 1;
        black_box(warm.relayout(2400.0 + (step % 997) as f32));
    });
    let mut warm: Vec<common::Warm> = chat_trees.iter().map(|t| common::Warm::new(t, 2.0, 1960.0)).collect();
    cell("chat row: cephal, kept tree, new width", 256, reps, |i| {
        step += 1;
        black_box(warm[i & 255].relayout(1500.0 + (step % 997) as f32));
    });
}
