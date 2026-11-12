//! Independent check of the `--cost-r6` harness: the same operations in the plainest loop that can
//! measure them, sharing none of the harness's code (no closure, no `Edit` enum, no overhead
//! subtraction per call: at 1e3, 32 fresh trees are edited between ONE counter pair, so the pair's
//! own cost is amortised 32 ways instead of calibrated away; an empty pass over the same trees is
//! subtracted).
//!
//!     cargo run --release -p bough-core --example r6_check
use bough_core::bough::RecordInit;
use bough_core::{Bough, NRect};
use bough_damage_lab::measure::counters;
use std::hint::black_box;

fn tree(n: usize) -> Bough {
    let mut v = Vec::with_capacity(n);
    v.push(RecordInit::new(0, NRect::EMPTY, 1, 0));
    for i in 1..n {
        let tile = (i - 1) * 10000 / (n - 1);
        let (x, y) = ((tile % 100) as i32 * 32, (tile / 100) as i32 * 32);
        v.push(RecordInit::new(
            1,
            NRect::new(x + 2, y + 2, x + 14, y + 14),
            1,
            i as u32,
        ));
    }
    let mut b = Bough::new(3200, 3200);
    b.insert_run(0, &v);
    // take the directory columns' one-time reallocation here (the harness's `R6_WARM=1`)
    let pad = [RecordInit::new(1, NRect::EMPTY, 1, 0); 64];
    b.insert_run(n, &pad);
    b.remove_run(n, 64);
    b.next_epoch();
    b
}

/// `reps` fresh trees edited between one counter pair; the least of five such trials. Several live
/// 1e6-record trees inflate the count (563 k -> 658 k -> 828 k per front insert with 1, 2, 4 trees:
/// the kernel's soft faults on page headers it aged out are retired instructions too), so the large
/// size uses one tree per trial.
fn edit(n: usize, reps: usize, f: impl Fn(&mut Bough)) -> f64 {
    let mut best = f64::MAX;
    for _ in 0..5 {
        let mut trees: Vec<Bough> = (0..reps).map(|_| tree(n)).collect();
        let a = counters().0;
        for t in trees.iter_mut() {
            black_box(t);
        }
        let empty = counters().0 - a;
        let a = counters().0;
        for t in trees.iter_mut() {
            f(t);
        }
        let z = counters().0;
        best = best.min((z - a).saturating_sub(empty) as f64 / reps as f64);
    }
    best
}

fn main() {
    assert!(!cfg!(debug_assertions));
    bough_damage_lab::measure::prefer_p_cores();
    println!("cell,n,instructions_per_op");
    for n in [1000usize, 1_000_000] {
        let reps = if n == 1000 { 32 } else { 1 };
        let new: Vec<_> = (0..8)
            .map(|i| RecordInit::new(1, NRect::new(2, 2, 14, 14), 1, (n + i) as u32))
            .collect();
        for (name, at) in [("insert8-front", 1usize), ("insert8-middle", n / 2)] {
            let v = edit(n, reps, |t| t.insert_run(black_box(at), black_box(&new)));
            println!("{name},{n},{v:.1}");
        }
        let v = edit(n, reps, |t| t.remove_run(black_box(n / 2), 8));
        println!("remove8-middle,{n},{v:.1}");

        // the prefix rebuild alone, through the non-inlined probe (also what `objdump` shows): a
        // directory of n / 64 pages rebuilt from page 0 and from its last page
        let pages = n.div_ceil(64);
        let lens = vec![64u32; pages];
        let mut p = bough_core::prefix::BlockedPrefix::new();
        for _ in 0..pages {
            p.grow();
        }
        for from in [0, pages - 1] {
            bough_core::probe::prefix_invalidate_from(&mut p, from, &lens);
            let a = counters().0;
            for _ in 0..64 {
                bough_core::probe::prefix_invalidate_from(
                    black_box(&mut p),
                    black_box(from),
                    black_box(&lens),
                );
            }
            let z = counters().0;
            println!(
                "prefix-rebuild-from-{},{n},{:.1}",
                if from == 0 { "front" } else { "end" },
                (z - a) as f64 / 64.0
            );
        }

        let b = tree(n);
        const Q: usize = 1 << 16;
        let a = counters().0;
        for _ in 0..Q {
            black_box(black_box(&b).len());
        }
        let qe = counters().0 - a;
        let a = counters().0;
        for _ in 0..Q {
            black_box(b.parent(black_box(n - 1)));
        }
        let z = counters().0;
        println!("parent-last,{n},{:.1}", (z - a - qe) as f64 / Q as f64);
        let a = counters().0;
        for _ in 0..Q {
            black_box(b.hit(black_box(3), black_box(3)));
        }
        let z = counters().0;
        println!("hit-present,{n},{:.1}", (z - a - qe) as f64 / Q as f64);
    }
}
