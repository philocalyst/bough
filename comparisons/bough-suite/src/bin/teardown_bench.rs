//! zgx `teardown_bench`, native: `N` rows of two texts in a column, mounted and laid out, then removed in
//! one frame (a re-description drops the `Option` holding them: one preorder run spliced out). Prints the
//! removal frame's instructions (this thread, zgx's `thread_selfcounts`) and time, the least of `REPEAT`
//! runs (5). bough frees everything inside the removal frame: there are no deferred frames after it.
use bough_suite::*;
use std::time::Instant;

fn run(n: usize) -> (f64, u64, usize) {
    let mut a = app_sized(Teardown { n, show: true }, 1200.0, 900.0, false, n, 3, 32);
    a.frame();
    let nodes = a.rt.bough.len();
    let (i0, t0) = (thread_instructions(), Instant::now());
    a.turn(|v| v.show = false);
    a.frame();
    let (t, spent) = (t0.elapsed().as_secs_f64(), thread_instructions() - i0);
    assert_eq!(a.rt.bough.len(), 0);
    (t, spent, nodes)
}

fn main() {
    let sizes = std::env::var("N").unwrap_or_else(|_| "2000,10000,30000".into());
    let repeat = env_usize("REPEAT", 5);
    for n in sizes.split(',').filter_map(|n| n.trim().parse().ok()) {
        let runs: Vec<_> = (0..repeat).map(|_| run(n)).collect();
        let nodes = runs[0].2;
        let t = runs.iter().map(|r| r.0).fold(f64::MAX, f64::min);
        let spent = runs.iter().map(|r| r.1).min().unwrap();
        println!("{n} rows ({nodes} records): removal frame {:.2} ms, {:.2} M instructions ({:.0} a record); after it, 0 frames", t * 1e3, spent as f64 / 1e6, spent as f64 / nodes as f64);
    }
}
