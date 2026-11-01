//! zgx-script `script_layout_bench`, native: 1,000 keyed rows of three heights reversed 20 times (each
//! reorder alternates the reversed and the original order). bough's list replays a reorder as one
//! permutation of the rows' record run and re-places the rows by a prefix sum of their heights (one space
//! write and one ink translation per moved row; no row is laid out again). Times and instructions (this
//! thread) of the change (the turn: settle, replay, placement) and of its frame (damage, tile list), mean
//! per reorder. Without `animate` only: list move animation is not built (zgx's `animate = true` variant
//! is not ported; see RESULTS).
use bough_suite::*;
use std::time::Instant;

fn main() {
    let n = env_usize("N", 1000);
    let reorders = env_usize("REORDERS", 20);
    let mut a = app(Reorder::new(n), 1200.0, 900.0, false);
    a.frame();
    let (mut change, mut frame, mut ci, mut fi) = (0f64, 0f64, 0u64, 0u64);
    for _ in 0..reorders {
        let (i0, t0) = (thread_instructions(), Instant::now());
        a.turn(|v| v.rows.reverse());
        change += t0.elapsed().as_secs_f64();
        ci += thread_instructions() - i0;
        let (i0, t0) = (thread_instructions(), Instant::now());
        a.frame();
        frame += t0.elapsed().as_secs_f64();
        fi += thread_instructions() - i0;
        // settled: a frame finds nothing to do
        assert!(!a.frame(), "idle after the rows settle");
    }
    let r = reorders as f64;
    println!("{n} rows reversed, {reorders} times, mean:");
    println!("  without animate: change {:.3} ms ({:.2} M instructions), its frame {:.3} ms ({:.2} M instructions)", change * 1e3 / r, ci as f64 / 1e6 / r, frame * 1e3 / r, fi as f64 / 1e6 / r);
}
