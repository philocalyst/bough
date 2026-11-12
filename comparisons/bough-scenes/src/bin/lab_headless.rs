//! The lab with no window, ticked as fast as it goes (zgx `workload_headless` contract): `ZGX_MODE`
//! (default stream), `TICKS` (900), `SCALE` (1, as zgx's), `RENDER` (1: offscreen Metal render of every
//! damaged frame into the IOSurface ring; 0: everything up to a ready draw list).
use bough::raw::*;
use bough_scenes::*;
use zgx_workload::{Mode, Workload};

fn env<T: std::str::FromStr>(name: &str, default: T) -> T {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

fn main() {
    let mode = match std::env::var("ZGX_MODE").as_deref() {
        Ok("idle") => Mode::Idle,
        Ok("scroll") => Mode::Scroll,
        Ok("both") => Mode::Both,
        _ => Mode::Stream,
    };
    let ticks: usize = env("TICKS", 900);
    let scale: f32 = env("SCALE", 1.0);
    let render = env("RENDER", 1) != 0;
    let mut work = Workload::from_env();
    work.mode = mode;
    let mut app = App::new(Lab::new(&work), lab_config(scale), render);
    let stages = std::env::var_os("MEMSTAGES").is_some();
    let mb = || bough::sys::perf::snap().footprint as f64 / 1048576.0;
    if stages {
        println!("after App::new: {:.1} MB", mb());
    }
    let t0 = bough::sys::perf::snap();
    let start = std::time::Instant::now();
    let mut frames = 0u64;
    let mut tiles = 0u64;
    for tick in 0..ticks {
        if tick > 0 {
            work.tick();
            app.turn(|l| lab_publish(l, &work, mode));
        }
        frames += app.frame() as u64;
        tiles += app.repaired_tiles as u64;
        #[cfg(target_os = "macos")]
        if stages && render && tick < 6 {
            app.wait_presented();
            println!("after frame {tick} (synced): {:.1} MB", mb());
        }
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
    println!("repaired tiles/frame {:.1}", tiles as f64 / frames.max(1) as f64);
    println!(
        "{mode:?} render={render}: {:.4} ms/tick, frames {frames}/{ticks}, ri_instructions total {}, instr/tick {:.0}, footprint {:.1} MB, fallbacks {}, text shaped {}, rows mounted {}",
        start.elapsed().as_secs_f64() * 1e3 / ticks as f64,
        t1.instrs,
        (t1.instrs - t0.instrs) as f64 / ticks as f64,
        t1.footprint as f64 / 1048576.0,
        app.rt.text.fallbacks,
        app.rt.ctr.text_set,
        app.rt.ctr.rows_mounted,
    );
}
