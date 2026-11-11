//! zgx-script `mount_bench`, native: `N` rows (2,000) of two texts, padded, rounded and coloured, in a
//! column. Phases as zgx's: build (the description's state: here nothing but the row count, the rows are
//! described lazily at mount), mount + layout (`App::new` + the first frame: records, text shaping, cephal
//! layout, write-back, damage, tile list), teardown (`drop`). Instructions retired by the process, per
//! phase (zgx's `instructions()`), after a warm-up run of 100 rows.
use bough_suite::*;
use std::time::Instant;

fn run(n: usize) -> ([(u64, f64); 3], usize) {
    let mut ph = [(0u64, 0f64); 3];
    let (i0, t0) = (instructions(), Instant::now());
    let v = MountRows { n };
    ph[0] = (instructions() - i0, t0.elapsed().as_secs_f64());
    let (i0, t0) = (instructions(), Instant::now());
    let mut a = app_sized(v, 1200.0, 900.0, false, n, 3, 32);
    a.frame();
    ph[1] = (instructions() - i0, t0.elapsed().as_secs_f64());
    let nodes = a.rt.bough.len();
    if std::env::var_os("BOUGH_NEWPROF").is_some() {
        use std::sync::atomic::Ordering::Relaxed;
        eprintln!("measure calls {} wrap runs {} stats {:?}", bough::layout::MEASURE_CALLS.load(Relaxed), bough::layout::WRAP_RUNS.load(Relaxed), a.rt.lay.stats);
    }
    if a.rt.lay.prof.on {
        let names = bough::layout::LP_NAMES;
        let parts: Vec<String> = a.rt.lay.prof.acc.iter().enumerate().map(|(i, v)| format!("{} {:.1}M", names[i], *v as f64 / 1e6)).collect();
        eprintln!("lprof: {}", parts.join(", "));
    }
    let (i0, t0) = (instructions(), Instant::now());
    drop(a);
    ph[2] = (instructions() - i0, t0.elapsed().as_secs_f64());
    (ph, nodes)
}

fn main() {
    let n = env_usize("N", 2000);
    println!("{n} rows; instructions (M) and time per phase:");
    run(n.min(100));
    for _ in 1..env_usize("LOOP", 1) {
        run(n);
    }
    let (ph, nodes) = run(n);
    let cell = |p: &(u64, f64)| format!("{:8.1} M {:7.2} ms", p.0 as f64 / 1e6, p.1 * 1e3);
    println!("  bough build {} | mount+layout {} | teardown {} | {nodes} records", cell(&ph[0]), cell(&ph[1]), cell(&ph[2]));
}
