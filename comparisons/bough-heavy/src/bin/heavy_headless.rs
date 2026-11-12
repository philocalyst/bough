//! The heavy dashboard on bough with no window, ticked as fast as it goes (zgx's `heavy_headless`
//! contract): `ZGX_MODE`, `TICKS` (900), `SCALE` (2), `RENDER` (1: an offscreen Metal render of every
//! damaged frame into the IOSurface ring, like zgx's RENDER=1; 0: everything up to a ready draw list
//! and instance writes, nothing submitted). `PHASES=1` prints the per-phase breakdown in
//! instructions per frame (proc_pid_rusage ri_instructions, calibrated bracket overhead subtracted),
//! `ri_instructions` for the whole run is printed so the slope method (TICKS=300 vs 600) works on it.
use bough::raw::*;
use bough_heavy::*;
use zgx_workload::heavy::*;

fn env<T: std::str::FromStr>(name: &str, default: T) -> T {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

fn main() {
    let ticks: usize = env("TICKS", 900);
    let scale: f32 = env("SCALE", 2.0);
    let render = env("RENDER", 1) != 0;
    let phases = env("PHASES", 0) != 0;
    let mut work = Heavy::from_env();
    let mode = work.mode;
    let mut app = App::new(Dash::new(&work), config(scale), render);
    if phases {
        app.prof.enable();
    }
    let t0 = bough::sys::perf::snap();
    let start = std::time::Instant::now();
    let mut warm = (t0.instrs, 0usize, [0u64; 5], 0u64);
    let mut frames = 0u64;
    let mut tiles = 0u64;
    for tick in 0..ticks {
        if tick == 60 {
            warm = (bough::sys::perf::snap().instrs, tick, app.prof.acc, frames);
        }
        if tick > 0 {
            work.tick();
            app.turn(|d| publish(d, &work, mode));
        }
        frames += app.frame() as u64;
        if tick >= 60 {
            tiles += app.repaired_tiles as u64;
        }
        #[cfg(target_os = "macos")]
        if render && tick % 60 == 0 {
            // a window presents every frame; here the GPU is only settled now and then
            app.wait_presented();
        }
    }
    #[cfg(target_os = "macos")]
    if render {
        app.wait_presented();
    }
    let t1 = bough::sys::perf::snap();
    let measured = ticks.saturating_sub(60).max(1) as f64;
    println!(
        "{mode:?} render={render}: {:.4} ms/tick, frames presented {frames}/{ticks}, instr/tick (after 60 warm-up ticks) {:.0}, ri_instructions total {}, footprint {:.1} MB, fallbacks {}, wires run {}, text set {}, rows mounted {}, repaired tiles/frame {:.1} (ring {})",
        start.elapsed().as_secs_f64() * 1e3 / ticks as f64,
        (t1.instrs - warm.0) as f64 / measured,
        t1.instrs,
        t1.footprint as f64 / 1048576.0,
        app.rt.text.fallbacks,
        app.rt.ctr.wires_run,
        app.rt.ctr.text_set,
        app.rt.ctr.rows_mounted,
        tiles as f64 / measured,
        env("BOUGH_RING", 0usize),
    );
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
    }
}
