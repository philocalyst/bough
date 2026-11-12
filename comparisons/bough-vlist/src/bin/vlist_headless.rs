//! The vlist scene on bough with no window, ticked as fast as it goes (the headless twin of zgx's
//! `vlist_workload` loop; zgx has no headless vlist driver, the one measured against is
//! `vlist_headless` built beside it): `ZGX_MODE` (scroll|fling|jump|probe), `TICKS` (900), `SCALE` (2),
//! `RENDER` (1: offscreen Metal render of every damaged frame; 0: everything up to a ready draw list),
//! `ROWS` (100000), `PHASES=1` per-phase instructions.
use bough::raw::*;
use bough_vlist::*;
use zgx_workload::vlist::{self, Mode};

fn env<T: std::str::FromStr>(name: &str, default: T) -> T {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

fn main() {
    let ticks: usize = env("TICKS", 900);
    let scale: f32 = env("SCALE", 2.0);
    let render = env("RENDER", 1) != 0;
    let phases = env("PHASES", 0) != 0;
    let rows: usize = env("ROWS", vlist::ROWS);
    let mode = Mode::from_env();
    let mut app = App::new(VList::new(rows), config(scale), render);
    if phases {
        app.prof.enable();
        app.rt.lay.prof.enable();
    }
    app.frame();
    if mode == Mode::Probe {
        eprintln!("{}", probe(&mut app));
        return;
    }
    let t0 = bough::sys::perf::snap();
    let start = std::time::Instant::now();
    let mut warm = (t0.instrs, 0usize, [0u64; 5], 0u64, [0u64; 8]);
    let mut frames = 0u64;
    let mut tiles = 0u64;
    for tick in 1..=ticks {
        if tick == 60 {
            warm = (bough::sys::perf::snap().instrs, tick, app.prof.acc, frames, app.rt.lay.prof.acc);
        }
        step(&mut app, mode, tick as u64);
        frames += app.frame() as u64;
        tiles += app.repaired_tiles as u64;
        #[cfg(target_os = "macos")]
        if render && tick % 60 == 0 {
            app.wait_presented();
        }
    }
    #[cfg(target_os = "macos")]
    if render {
        app.wait_presented();
    }
    let t1 = bough::sys::perf::snap();
    let measured = ticks.saturating_sub(60).max(1) as f64;
    let m = app.rt.lists.regs()[0].stats;
    println!("repaired tiles/frame {:.1}", tiles as f64 / frames.max(1) as f64);
    println!(
        "{mode:?} render={render}: {:.4} ms/tick, frames presented {frames}/{ticks}, instr/tick (after 60 warm-up ticks) {:.0}, ri_instructions total {}, footprint {:.1} MB, rows mounted {} unmounted {} passes {} height changes {}, layouts {} nodes written {}, fallbacks {}",
        start.elapsed().as_secs_f64() * 1e3 / ticks as f64,
        (t1.instrs - warm.0) as f64 / measured,
        t1.instrs,
        t1.footprint as f64 / 1048576.0,
        m.rows_mounted,
        m.rows_unmounted,
        m.passes,
        m.height_changes,
        app.rt.lay.stats.computes,
        app.rt.lay.stats.nodes_written,
        app.rt.text.fallbacks,
    );
    {
        use std::sync::atomic::Ordering::Relaxed;
        println!("measure calls {} wrap runs {} for {} rows mounted", bough::layout::MEASURE_CALLS.load(Relaxed), bough::layout::WRAP_RUNS.load(Relaxed), m.rows_mounted);
    }
    if phases {
        let n = measured;
        let mut tot = 0.0;
        print!("per-phase instr/frame (ticks 60..):");
        for (i, name) in bough::app::PHASE_NAMES.iter().enumerate() {
            let v = (app.prof.acc[i] - warm.2[i]) as f64 / n;
            tot += v;
            print!("  {name} {v:.0}");
        }
        println!("  | sum {tot:.0}");
        print!("layout path, instr/frame (included in update above):");
        for (i, name) in bough::layout::LP_NAMES.iter().enumerate() {
            print!("  {name} {:.0}", (app.rt.lay.prof.acc[i] - warm.4[i]) as f64 / n);
        }
        let mounted = app.rt.lists.regs()[0].stats.rows_mounted.max(1) as f64;
        print!("\nper mounted row ({} rows):", app.rt.lists.regs()[0].stats.rows_mounted);
        for (i, name) in bough::layout::LP_NAMES.iter().enumerate() {
            print!("  {name} {:.0}", app.rt.lay.prof.acc[i] as f64 / mounted);
        }
        println!();
    }
}
