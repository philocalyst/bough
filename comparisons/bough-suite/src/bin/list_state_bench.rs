//! zgx-script `list_state_bench`, native: 4,000 messages, the inbox's 1,000 rows in a list, its unread
//! count; each tick one message's unread flag flips (`(tick * 7) % N + 1`). Instructions retired by the
//! process per tick (with its frame) after 10 warm-up ticks, over `TICKS` (200), and time per tick.
//! (zgx's list is virtualized to its 560-point viewport; bough's list mounts all 1,000 inbox rows and
//! clips them: the per-tick work is the same, the mount is larger.)
use bough_suite::*;
use std::time::Instant;

fn main() {
    let n = env_usize("N", 4000);
    let ticks = env_usize("TICKS", 200);
    let mut a = app_sized(ListState::new(n), 800.0, 600.0, false, n / 4, 4, 24);
    a.frame();
    for _ in 0..10 {
        a.turn(|v| v.tick());
        a.frame();
    }
    let (i0, t0) = (instructions(), Instant::now());
    for _ in 0..ticks {
        a.turn(|v| v.tick());
        a.frame();
    }
    let t = t0.elapsed().as_secs_f64();
    println!("{n} rows: {:.4} M instructions, {:.4} ms per tick", (instructions() - i0) as f64 / 1e6 / ticks as f64, t * 1e3 / ticks as f64);
}
