//! zgx `teardown_bench`, native: `N` rows of two texts in a column, mounted and laid out, then removed in
//! one frame (a re-description drops the `Option` holding them: one preorder run spliced out). Prints the
//! removal frame's instructions (this thread, zgx's `thread_selfcounts`) and time, the least of `REPEAT`
//! runs (5), then the frames after it until the released resources are back on the free lists (zgx's
//! `Ui::reclaiming`): their count, the slowest, and their instructions in all.
use bough::raw::{App, Config};
use bough_suite::*;
use std::time::Instant;

fn run(n: usize) -> (f64, u64, usize, usize, f64, u64) {
    // `app_sized`'s columns, not mounted once: the removal is a re-description (the `Option` dropped)
    let cfg = Config { mount_once: false, ..config_sized(1200.0, 900.0, n, 3, 32) };
    let mut a = App::new(Teardown { n, show: true }, cfg, false);
    a.frame();
    let nodes = a.rt.bough.len();
    let (i0, t0) = (thread_instructions(), Instant::now());
    a.turn(|v| v.show = false);
    a.frame();
    let (t, spent) = (t0.elapsed().as_secs_f64(), thread_instructions() - i0);
    assert_eq!(a.rt.bough.len(), 0);
    // the frames after it: what deferred freeing costs each, until done (zgx's `Ui::reclaiming`)
    let (mut frames, mut slowest, mut after) = (0, 0f64, 0u64);
    while frames < 600 && a.reclaiming() {
        let (i0, t0) = (thread_instructions(), Instant::now());
        a.frame();
        slowest = slowest.max(t0.elapsed().as_secs_f64());
        after += thread_instructions() - i0;
        frames += 1;
    }
    (t, spent, nodes, frames, slowest, after)
}

fn main() {
    let sizes = std::env::var("N").unwrap_or_else(|_| "2000,10000,30000".into());
    let repeat = env_usize("REPEAT", 5);
    for n in sizes.split(',').filter_map(|n| n.trim().parse().ok()) {
        let runs: Vec<_> = (0..repeat).map(|_| run(n)).collect();
        let nodes = runs[0].2;
        let t = runs.iter().map(|r| r.0).fold(f64::MAX, f64::min);
        let spent = runs.iter().map(|r| r.1).min().unwrap();
        let frames = runs.iter().map(|r| r.3).max().unwrap();
        let slowest = runs.iter().map(|r| r.4).fold(f64::MAX, f64::min);
        let after = runs.iter().map(|r| r.5).min().unwrap();
        println!("{n} rows ({nodes} records): removal frame {:.2} ms, {:.2} M instructions ({:.0} a record); after it, {frames} frames, slowest {:.2} ms, {:.2} M instructions in all", t * 1e3, spent as f64 / 1e6, spent as f64 / nodes as f64, slowest * 1e3, after as f64 / 1e6);
    }
}
