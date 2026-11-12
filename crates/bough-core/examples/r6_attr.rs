//! Round 6 attribution: where the instructions of one structural edit go (feature `attr`).
//!
//!     cargo run --release -p bough-core --features attr --example r6_attr
//!
//! Each row is one fresh tree and one edit; the columns are the pieces of the directory's maintenance,
//! each between its own counter pair (pair cost subtracted), and `page work` is the remainder of the
//! whole call (locate, record copies, group/page summaries, damage, handle fixups).
use bough_core::bough::RecordInit;
use bough_core::{Bough, NRect, attr};
use bough_damage_lab::measure;
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
    if std::env::var("R6_WARM").is_ok_and(|s| s == "1") {
        // take the columns' one-time reallocation outside the interval (see cost_r6.rs)
        let pad = [RecordInit::new(1, NRect::EMPTY, 1, 0); 64];
        b.insert_run(n, &pad);
        b.remove_run(n, 64);
    }
    b.next_epoch();
    b
}

fn main() {
    assert!(!cfg!(debug_assertions));
    measure::prefer_p_cores();
    let ov = measure::overhead();
    print!("n,edit,pages,total");
    for name in attr::NAMES {
        print!(",{name}");
    }
    println!(",page work");
    for n in [1000usize, 10_000, 100_000, 1_000_000] {
        let new: Vec<_> = (0..64)
            .map(|i| RecordInit::new(1, NRect::new(2, 2, 14, 14), 1, (n + i) as u32))
            .collect();
        for (name, at, k, remove) in [
            ("insert8-front", 1usize, 8usize, false),
            ("insert8-middle", n / 2, 8, false),
            ("insert64-end", n, 64, false),
            ("remove64-front", 1, 64, true),
            ("remove64-middle", n / 2, 64, true),
            ("remove64-end", n - 64, 64, true),
        ] {
            let mut rows = Vec::new();
            for _ in 0..3 {
                let mut b = tree(n);
                attr::take();
                let a = measure::counters().0;
                if remove {
                    b.remove_run(black_box(at), k);
                } else {
                    b.insert_run(black_box(at), black_box(&new[..k]));
                }
                let z = measure::counters().0;
                let parts = attr::take();
                // an inner pair is two counter calls inside the outer interval (the "overhead" of a pair
                // is one call's worth: from one call's sample point to the next's)
                let inner: u64 = parts.iter().map(|p| p.1).sum();
                let total = (z - a).saturating_sub(ov * (1 + 2 * inner));
                rows.push((total, parts, b.num_pages()));
            }
            let (total, parts, pages) = rows[2];
            let sum: u64 = parts.iter().map(|p| p.0).sum();
            print!("{n},{name},{pages},{total}");
            for p in parts {
                print!(",{}", p.0);
            }
            println!(",{}", total as i64 - sum as i64);
        }
    }
}
