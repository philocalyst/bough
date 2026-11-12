//! The quick bench's lifecycle protocol (`bench/quick/run.sh`), shared by `crates/bough/examples/quick_life.rs`
//! (the counter, cold mounts) and `comparisons/bough-heavy/src/bin/heavy_life.rs` (the heavy dashboard) through
//! `#[path]`. One cold process, headless, the way `Application::run` builds its app (`App::new(view, cfg, true)`,
//! no `prepare`):
//!
//! * startup: process start (the kernel's start time) to the first frame complete on the GPU;
//! * memory after that first frame and after `QUICK_FRAMES` (100) more turns, each one a change and a frame;
//! * shutdown: the close request to the process's exit, split into the presenter's drop (publisher thread,
//!   Metal objects), the rest of the app's drop (runtime, region, text) and the exit itself (the launcher sees
//!   its end: `bench/quick/quick.py` waits with `WNOWAIT` and reads the zombie's instructions).
//!
//! Prints one line, `quick_life key=value ...`. `QUICK_INJECT` / `QUICK_INJECT_MS` (`bough_metal::perf::inject`)
//! add a known cost inside every measured window: the bench's negative control.
use bough::raw::{App, View};
use bough_metal::perf::{inject, inject_ms, snap};

/// `CLOCK_UPTIME_RAW` (mach absolute time) in ns: the clock the kernel stamps a process's start and exit with
/// (`rusage_info_v4.ri_proc_{start,exit}_abstime`), so the launcher can subtract.
fn up_ns() -> u64 {
    let mut t = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `t` is a valid timespec to write.
    unsafe { libc::clock_gettime(libc::CLOCK_UPTIME_RAW, &mut t) };
    t.tv_sec as u64 * 1_000_000_000 + t.tv_nsec as u64
}

/// `make` builds the app (the measured cold start), `step(app, k)` changes something before frame `k`.
pub fn run<V: View>(make: impl FnOnce() -> App<V>, mut step: impl FnMut(&mut App<V>, u64)) {
    let frames: u64 = std::env::var("QUICK_FRAMES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(100);
    let up_main = up_ns();
    let s_main = snap();
    let mut app = make();
    inject_ms();
    inject();
    let s_new = snap();
    let up_new = up_ns();
    app.frame();
    app.gpu.as_mut().expect("a GPU app").sync();
    let up_first = up_ns();
    let s_first = snap();
    for k in 0..frames {
        step(&mut app, k);
        inject();
        app.frame();
        if k % 60 == 59 {
            app.gpu.as_mut().unwrap().sync();
        }
    }
    app.gpu.as_mut().unwrap().sync();
    let s_frames = snap();
    // the close request: from here to the process's exit is shutdown
    let up_close = up_ns();
    let i_close = snap().instrs;
    inject_ms();
    drop(app.gpu.take());
    let up_gpu = up_ns();
    let i_gpu = snap().instrs;
    drop(app);
    let up_drop = up_ns();
    let i_drop = snap().instrs;
    println!(
        "quick_life up_main={up_main} up_new={up_new} up_first={up_first} instr_main={} instr_new={} instr_first={} \
         footprint_first={} rss_first={} footprint_frames={} rss_frames={} frames={frames} instr_frames={} \
         up_close={up_close} up_gpu_drop={up_gpu} up_app_drop={up_drop} instr_close={i_close} instr_gpu_drop={i_gpu} \
         instr_app_drop={i_drop}",
        s_main.instrs,
        s_new.instrs,
        s_first.instrs,
        s_first.footprint,
        s_first.rss,
        s_frames.footprint,
        s_frames.rss,
        s_frames.instrs,
    );
    use std::io::Write;
    std::io::stdout().flush().ok();
}
