//! The hot paths of the new core as cells, in the form `bench/r8/perf/hot.sh` drives (threads lane, round 8).
//!
//! The convention, for any binary measured by that script:
//!
//!   OP=<cell>      run that one cell (default: every cell, one table row each)
//!   REPS=<n>       repetitions of the cell after set-up; two values give a slope that subtracts set-up
//!   EVICT_KB=<k>   between repetitions, walk a buffer of `k` KiB, so the cell starts with every cache level
//!                  smaller than that cold (0: warm). The walk is outside the counted bracket.
//!   ROWS=<n>       scene size
//!   BOUGH_PMU=..   up to three more events for the in-process counters (Linux)
//!   CELLS=1        print the cell names and exit
//!
//! Output: `scene  cell  evict_kb  reps  <event>_p50 ...  <event>_max ...  tiles_p50  wall_ns_p50  wall_ns_max`, counted on the calling
//! thread with the bracket's own cost subtracted. Under `perf stat` or `perf record` the same loop runs and the
//! table is still printed; perf's counts then include set-up and the eviction walk, which the script removes
//! with the `floor` cell and a slope.
//!
//! The frame cells paint through `soft::Soft`, the expr lane's reference raster: an oracle, not a product.
//! They are here so the method covers "a full frame"; the raster lane's kernels are measured by the same
//! script from its own binary.
#[path = "../scenes/cells.rs"]
mod cells;
use bough_mem::pmu;
use cells::sheet::*;
use cells::{Bed, CELLS};
use std::hint::black_box;

fn env(name: &str, d: usize) -> usize {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(d)
}

/// Walk `buf` a line at a time, writing: afterwards the caches smaller than it hold it and not the scene.
/// Stores only, no load: a sampler of load misses (`lines.sh`) then sees the cell's misses and not the walk's.
#[inline(never)]
fn evict(buf: &mut [u8], k: u8) {
    let mut i = 0;
    while i < buf.len() {
        buf[i] = k;
        i += 64;
    }
    black_box(&mut *buf);
}

/// perf's control and acknowledgement FIFOs, when `PERF_CTL` and `PERF_ACK` name them.
struct PerfCtl(Option<(std::fs::File, std::fs::File)>);
fn perf_ctl() -> PerfCtl {
    let open = || {
        let c = std::fs::OpenOptions::new().write(true).open(std::env::var("PERF_CTL").ok()?).ok()?;
        let a = std::fs::File::open(std::env::var("PERF_ACK").ok()?).ok()?;
        Some((c, a))
    };
    PerfCtl(open())
}
impl PerfCtl {
    fn say(&self, what: &str) {
        use std::io::{Read, Write};
        if let Some((c, a)) = &self.0 {
            let (mut c, mut a) = (c, a);
            if c.write_all(format!("{what}\n").as_bytes()).is_ok() {
                let mut ack = [0u8; 8];
                let _ = a.read(&mut ack);
            }
        }
    }
}

fn main() {
    if std::env::var("CELLS").is_ok() {
        for c in CELLS {
            println!("{}\t{}", c.0, c.1);
        }
        return;
    }
    let (rows, reps, evict_kb) = (env("ROWS", 10_000), env("REPS", 64), env("EVICT_KB", 0));
    let only = std::env::var("OP").ok();
    let evs = pmu::sample_events();
    let n = evs.len();
    // the bracket's own cost: median of 64 empty brackets
    let mut br: Vec<[u64; pmu::MAX]> = (0..64)
        .map(|_| {
            let a = pmu::sample();
            pmu::sample().since(&a).v
        })
        .collect();
    let mut bracket = [0u64; pmu::MAX];
    for (i, b) in bracket.iter_mut().enumerate().take(n) {
        br.sort_by_key(|s| s[i]);
        *b = br[br.len() / 2][i];
    }
    let names: Vec<&str> = evs.iter().map(|e| e.name()).collect();
    println!("# rows={rows} cells={} window={W}x{H} evict_kb={evict_kb} reps={reps}", rows * COLS);
    println!(
        "scene\tcell\tevict_kb\treps{}{}\ttiles_p50\twall_ns_p50\twall_ns_max",
        names.iter().map(|e| format!("\t{e}_p50")).collect::<String>(),
        names.iter().map(|e| format!("\t{e}_max")).collect::<String>()
    );
    let mut bed = Bed::new(rows);
    let mut buf = vec![0u8; evict_kb * 1024];
    for &(name, _, f) in CELLS {
        if only.as_deref().is_some_and(|o| o != name) {
            continue;
        }
        // reach the cell's steady state: every state it alternates between has been visited
        for k in 0..16 {
            f(&mut bed, k);
            bed.world.tiles.clear();
        }
        // a sampler started disabled (`perf record --control fifo:CTL,ACK -D -1`) is switched on for the
        // measured repetitions only, so set-up and warm-up are not in its samples
        let ctl = perf_ctl();
        ctl.say("enable");
        let mut got: Vec<([u64; pmu::MAX], u32)> = Vec::with_capacity(reps);
        let mut wall: Vec<u64> = Vec::with_capacity(reps);
        for k in 0..reps {
            if evict_kb != 0 {
                evict(&mut buf, k as u8);
            }
            let t0 = std::time::Instant::now();
            let a = pmu::sample();
            f(&mut bed, k);
            let d = pmu::sample().since(&a);
            // wall time includes the two counter reads (a system call each on Linux): a floor of its own
            wall.push(t0.elapsed().as_nanos() as u64);
            let tiles = bed.world.tiles.count();
            bed.world.tiles.clear();
            let mut v = d.v;
            for i in 0..n {
                v[i] = v[i].saturating_sub(bracket[i]);
            }
            got.push((v, tiles));
        }
        ctl.say("disable");
        let mut line = format!("sheet\t{name}\t{evict_kb}\t{reps}");
        let col = |i: usize| {
            let mut x: Vec<u64> = got.iter().map(|g| g.0[i]).collect();
            x.sort();
            (x[x.len() / 2], x[x.len() - 1])
        };
        for i in 0..n {
            line += &format!("\t{}", col(i).0);
        }
        for i in 0..n {
            line += &format!("\t{}", col(i).1);
        }
        let mut t: Vec<u32> = got.iter().map(|g| g.1).collect();
        t.sort();
        wall.sort();
        println!("{line}\t{}\t{}\t{}", t[t.len() / 2], wall[wall.len() / 2], wall[wall.len() - 1]);
    }
    if std::env::var("REGIONS").is_ok() {
        // where everything is, for `lines.sh`: REGION name start bytes page rec_off
        let here = 0u8;
        let mut r = bed.world.regions();
        r.push(("model.struct".into(), &bed.sheet as *const _ as usize, size_of::<Sheet>(), 0, 0));
        r.push(("model.block".into(), &bed.sheet.u as *const _ as usize, 256, 0, 0));
        r.push(("model.rows".into(), bed.sheet.rows.items().as_ptr() as usize, bed.sheet.rows.len() * size_of::<[f32; COLS]>(), 0, 0));
        r.push(("harness.evict".into(), buf.as_ptr() as usize, buf.len(), 0, 0));
        r.push(("harness.samples".into(), 0, 0, 0, 0));
        r.push(("stack".into(), (&here as *const u8 as usize).saturating_sub(1 << 20), 2 << 20, 0, 0));
        for (name, at, len, page, off) in r {
            eprintln!("REGION\t{name}\t{at:#x}\t{len}\t{page}\t{off}");
        }
    }
    std::fs::remove_file(&bed.image).ok();
}
