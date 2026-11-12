//! Checks the counter: a loop of known length, the call overhead, and the slope method.
use cmpkit::*;
use std::hint::black_box;
fn main() {
    println!("overhead per instr() call: {}", instr_overhead());
    for n in [1_000u64, 100_000, 10_000_000] {
        let a = instr();
        let mut s = 0u64;
        for i in 0..black_box(n) {
            s = s.wrapping_add(black_box(i));
        }
        black_box(s);
        let d = instr() - a;
        println!("loop {n}: {d} instructions ({:.2} per iteration)", d as f64 / n as f64);
    }
    let per = slope(|reps| {
        let t = instr();
        for _ in 0..reps {
            let mut s = 0u64;
            for i in 0..black_box(1000u64) {
                s = s.wrapping_add(black_box(i));
            }
            black_box(s);
        }
        instr() - t
    });
    println!("slope of a 1000-iteration loop: {per}");
    println!("rss {}", rss_bytes());
}
