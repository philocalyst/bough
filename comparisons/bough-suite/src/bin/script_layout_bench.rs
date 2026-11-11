//! zgx-script `script_layout_bench`, native: 1,000 keyed rows of three heights reversed 20 times (each
//! reorder alternates the reversed and the original order), without and with `animate`. bough's list replays
//! a reorder as one permutation of the rows' record run and re-places the rows by a prefix sum of their
//! heights (one space write and one ink translation per moved row; no row is laid out again). With
//! `animate` (`.animate_moves()`, FLIP) the reorder holds every moved row where it was (no translation at
//! all), and each frame at 120 Hz (`App::tick(1/120)` + `frame`) writes the moving rows' spaces and translates
//! their records after one spring evaluation per frame, until the spring settles (zgx's default
//! `spring { bounce = 0.1, duration = 0.4 }`); rows whose whole path stays outside the 900-point viewport
//! snap at once. Times and instructions (this thread) of the change (the turn: settle, replay, placement),
//! of its frame (damage, tile list) and of the moving frames, mean per reorder / per frame.
use bough_suite::*;
use std::time::Instant;

fn main() {
    let n = env_usize("N", 1000);
    let reorders = env_usize("REORDERS", 20);
    println!("{n} rows reversed, {reorders} times, mean:");
    for animate in [false, true] {
        let mut a = app(if animate { Reorder::animated(n) } else { Reorder::new(n) }, 1200.0, 900.0, false);
        a.frame();
        let (mut change, mut frame, mut ci, mut fi) = (0f64, 0f64, 0u64, 0u64);
        let (mut moving, mut mi, mut frames) = (0f64, 0u64, 0u64);
        for _ in 0..reorders {
            let (i0, t0) = (thread_instructions(), Instant::now());
            a.turn(|v| v.rows.reverse());
            change += t0.elapsed().as_secs_f64();
            ci += thread_instructions() - i0;
            let (i0, t0) = (thread_instructions(), Instant::now());
            a.frame();
            frame += t0.elapsed().as_secs_f64();
            fi += thread_instructions() - i0;
            while a.animating() {
                let (i0, t0) = (thread_instructions(), Instant::now());
                a.tick(1.0 / 120.0);
                a.frame();
                moving += t0.elapsed().as_secs_f64();
                mi += thread_instructions() - i0;
                frames += 1;
            }
            // settled: nothing asks for a frame and a frame finds nothing to do
            assert!(!a.animating());
            assert!(!a.frame(), "idle after the rows settle");
        }
        let r = reorders as f64;
        let name = if animate { "with animate" } else { "without animate" };
        let moving = if frames > 0 {
            format!("{:.4} ms ({:.1} k instructions) over {} frames", moving * 1e3 / frames as f64, mi as f64 / 1e3 / frames as f64, frames as f64 / r)
        } else {
            "none".into()
        };
        println!(
            "  {name:16}: change {:.3} ms ({:.2} M instructions), its layout frame {:.3} ms ({:.2} M instructions), moving frames {moving}",
            change * 1e3 / r,
            ci as f64 / 1e6 / r,
            frame * 1e3 / r,
            fi as f64 / 1e6 / r
        );
    }
}
