//! The animation scene offscreen (zgx `animation_headless`): scale 2, clock advanced one 120 Hz
//! refresh per frame, `FRAMES` (2400), `RENDER` (1/0). Reports CPU per frame by phase.
use bough::raw::*;
use bough_scenes::*;

fn env<T: std::str::FromStr>(name: &str, default: T) -> T {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

fn main() {
    let frames: usize = env("FRAMES", 2400);
    let render = env("RENDER", 1) != 0;
    let mut app = App::new(Anim { clock: bough::signal::Signal::new(0.0) }, anim_config(2.0), render);
    // the throughput mode (pre-armed command stream) unless BOUGH_STREAM=0
    #[cfg(target_os = "macos")]
    if render {
        app.frame();
        let k = env("BOUGH_STREAM", 8usize);
        if k > 0 {
            app.stream(k);
        }
    }
    let warmup = 120;
    let (mut w0, mut c0) = (bough::sys::perf::snap(), 0u64);
    let mut drawn = 0u64;
    for frame in 0..frames {
        if frame == warmup {
            w0 = bough::sys::perf::snap();
            c0 = drawn;
        }
        app.turn(|a| {
            a.clock.set(frame as f32 / 120.0);
        });
        drawn += app.frame() as u64;
        #[cfg(target_os = "macos")]
        if render && frame % 120 == 0 {
            app.wait_presented();
        }
    }
    #[cfg(target_os = "macos")]
    if render {
        app.wait_presented();
    }
    let w1 = bough::sys::perf::snap();
    let n = frames.saturating_sub(warmup).max(1) as f64;
    println!(
        "animate: {:.0} instr/frame after {warmup} warm-up, frames drawn {}/{} render={render}, ri_instructions total {}, footprint {:.1} MB",
        (w1.instrs - w0.instrs) as f64 / n,
        drawn - c0,
        frames - warmup,
        w1.instrs,
        w1.footprint as f64 / 1048576.0
    );
}
