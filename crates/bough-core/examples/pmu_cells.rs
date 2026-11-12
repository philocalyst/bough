//! Hardware counters per core operation, on both hosts (bench lane, round 7; docs/lanes/bench/MEASUREMENT.md).
//!
//! A tree of N records (a 100 x 100 grid of 32 px tiles, one 12 x 12 ink per record, flat or in chains of 32);
//! for each operation the calling thread's counter group (`bough_mem::pmu`: instructions, cycles, then the events
//! in `BOUGH_PMU`) is read around a batch, and the per-operation value is the slope between a batch of `LO` and
//! one of `HI` operations, so the bracket's own cost and any first-touch work in the first batch cancel. Each
//! cell is the median of five slopes. On macOS only instructions and cycles exist; on Linux add e.g.
//! `BOUGH_PMU=l1d-misses,cache-misses,dc-fill-dram` (Zen 2: L1d misses, L2 misses, lines refilled from DRAM).
//!
//!     cargo run --profile dist -p bough-core --example pmu_cells -- [N...]     (default 1 64 256 100000)
//!     ilo-prod: taskset -c 2 $CARGO_TARGET_DIR/dist/examples/pmu_cells
//!
//! `REPS=k OP=name` runs only that operation, k times per size, and prints nothing: the shape for an external
//! counter (`bench/perf/pstat.sh -s REPS=1000,11000 -- pmu_cells 100000` with `OP=set_ink` in the environment).
//!
//! Operations: `set_ink` (one record's ink moves by a pixel, with damage), `damage_rect` (a 12 px rect),
//! `front_insert` (insert one record at position 1 and remove it again: one pair), `translate_all` (the whole
//! run moves by one pixel), `repair` (cull: one damaged rect, `repair_into(1)`, next epoch).
use bough_core::rng::Rng;
use bough_core::{Bough, NRect, Plane, RecordInit};
use bough_mem::pmu::{self, Reading};
use std::hint::black_box;

const SIDE: i32 = 3200;
const LO: usize = 64;
const HI: usize = 320;

fn records(n: usize, chains: bool) -> Vec<RecordInit> {
    (0..n)
        .map(|i| {
            if i == 0 {
                return RecordInit::new(0, NRect::EMPTY, 1, 0);
            }
            let tile = (i - 1) * 9973 % 10000;
            let (x, y) = ((tile % 100) as i32 * 32, (tile / 100) as i32 * 32);
            let depth = if chains { (1 + (i - 1) % 32) as u16 } else { 1 };
            RecordInit::new(depth, NRect::new(x + 2, y + 2, x + 14, y + 14), 1, i as u32)
        })
        .collect()
}

fn build(n: usize, chains: bool) -> Bough {
    let mut b = Bough::new(SIDE, SIDE);
    b.insert_run(0, &records(n, chains));
    b.next_epoch();
    b
}

/// Counter group delta over `count` calls of `f`.
#[inline(never)]
fn batch(f: &mut impl FnMut(usize), count: usize, base: &mut usize) -> Reading {
    let a = pmu::sample();
    for _ in 0..count {
        f(*base);
        *base += 1;
    }
    pmu::sample().since(&a)
}

/// `REPS` mode: `count` calls of `f`, nothing measured in-process.
fn plain(f: &mut impl FnMut(usize), count: usize) {
    for k in 0..count {
        f(k);
    }
}

/// Median over five (HI - LO) slopes, per event; `complete` is false if any batch was multiplexed.
fn cell(mut f: impl FnMut(usize)) -> (Vec<f64>, bool) {
    if let Some(reps) = reps() {
        plain(&mut f, reps);
        return (Vec::new(), true);
    }
    let mut k = 0usize;
    batch(&mut f, LO, &mut k); // warm: code, first touches
    let n = pmu::sample_events().len();
    let mut per: Vec<Vec<f64>> = vec![Vec::new(); n];
    let mut complete = true;
    for _ in 0..5 {
        let lo = batch(&mut f, LO, &mut k);
        let hi = batch(&mut f, HI, &mut k);
        complete &= lo.complete() && hi.complete();
        for e in 0..n {
            per[e].push((hi.v[e] as f64 - lo.v[e] as f64) / (HI - LO) as f64);
        }
    }
    let med = per
        .into_iter()
        .map(|mut v| {
            v.sort_by(|a, b| a.total_cmp(b));
            v[v.len() / 2]
        })
        .collect();
    (med, complete)
}

fn reps() -> Option<usize> {
    std::env::var("REPS").ok().and_then(|v| v.parse().ok())
}

fn main() {
    let only = std::env::var("OP").ok();
    let quiet = reps().is_some();
    let sizes: Vec<usize> = std::env::args().skip(1).filter_map(|s| s.parse().ok()).collect();
    let sizes = if sizes.is_empty() { vec![1, 64, 256, 100_000] } else { sizes };
    if !quiet {
        let evs = pmu::sample_events();
        print!("n\tshape\top");
        for e in &evs {
            print!("\t{}", e.name());
        }
        println!("\tcomplete");
    }
    for &n in &sizes {
        for chains in [false, true] {
            let shape = if chains { "chains" } else { "flat" };
            let want = |op: &str| only.as_deref().is_none_or(|o| o == op) && (!quiet || !chains);
            let emit = |op: &str, (v, ok): (Vec<f64>, bool)| {
                if quiet {
                    return;
                }
                print!("{n}\t{shape}\t{op}");
                for x in v {
                    print!("\t{x:.2}");
                }
                println!("\t{ok}");
            };
            let real = n.saturating_sub(1).max(1);
            if want("set_ink") {
                let mut b = build(n, chains);
                let mut rng = Rng::new(7);
                emit("set_ink", cell(|k| {
                    if n > 1 {
                        let pos = 1 + rng.range(0, real as i32) as usize % real;
                        let r = b.ink_at(pos);
                        let d = if k % 2 == 0 { 1 } else { -1 };
                        let [x0, y0, x1, y1] = r.to_xyxy();
                        b.set_ink(pos, NRect::new(x0 + d, y0, x1 + d, y1));
                    }
                }));
                black_box(&b);
            }
            if want("damage_rect") {
                let mut b = build(n, chains);
                let mut rng = Rng::new(9);
                emit("damage_rect", cell(|_| {
                    let (x, y) = (rng.range(0, SIDE - 40), rng.range(0, SIDE - 40));
                    b.damage_rect(NRect::new(x, y, x + 12, y + 12));
                }));
                black_box(&b);
            }
            if n > 1 && want("front_insert") {
                let mut b = build(n, chains);
                let one = [RecordInit::new(1, NRect::new(40, 40, 52, 52), 1, 0)];
                emit("front_insert", cell(|_| {
                    b.insert_run(1, &one);
                    b.remove_run(1, 1);
                }));
                black_box(&b);
            }
            if want("translate_all") {
                let mut b = build(n, chains);
                emit("translate_all", cell(|k| {
                    let d = if k % 2 == 0 { 1 } else { -1 };
                    b.translate_run(0, n, d, 0);
                }));
                black_box(&b);
            }
            if want("repair") {
                let mut b = build(n, chains);
                let mut out = Plane::EMPTY;
                let mut rng = Rng::new(11);
                emit("repair", cell(|_| {
                    let (x, y) = (rng.range(0, SIDE - 40), rng.range(0, SIDE - 40));
                    b.damage_rect(NRect::new(x, y, x + 12, y + 12));
                    black_box(b.repair_into(1, &mut out));
                    b.next_epoch();
                }));
                black_box(&b);
            }
        }
    }
}
