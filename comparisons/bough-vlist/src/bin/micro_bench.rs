//! zgx's `micro_bench` for `SCENE=vscroll-variable` on bough (same method and JSON shape):
//! 10,000 rows (the vlist rows) in a 720 x 800 pt window, an operation scrolls 12 pt (back to the top
//! past the end). CPU time of the whole process per operation, medians over batches, for the
//! operation alone ("write": the turn, i.e. handler + wires + row mount/measure/place + text + layout
//! write-back, retained state ready to cull) and through to a frame ready to present ("frame": the turn
//! plus damage, cull, draw list and, with `RENDER=1`, the Metal encode + commit).
//!   SCENE=vscroll-variable OPS=2000 RENDER=0|1 micro_bench
use bough::raw::*;
use bough_vlist::*;
use zgx_workload::{micro, vlist};

fn env<T: std::str::FromStr>(name: &str, default: T) -> T {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

/// The process's CPU time (all threads), ns; `CLOCK=thread` for this thread alone.
fn cpu_ns() -> u64 {
    #[repr(C)]
    struct Timespec {
        s: i64,
        ns: i64,
    }
    let thread = std::env::var("CLOCK").is_ok_and(|v| v == "thread");
    let clock: i32 = if thread { 16 } else { 12 };
    let mut t = Timespec { s: 0, ns: 0 };
    unsafe { libc::clock_gettime(clock as libc::clockid_t, &mut t as *mut Timespec as *mut libc::timespec) };
    t.s as u64 * 1_000_000_000 + t.ns as u64
}

fn summary(samples: &mut [f64]) -> String {
    samples.sort_by(f64::total_cmp);
    let at = |q: f64| samples[((samples.len() - 1) as f64 * q).round() as usize];
    format!(
        "\"median_ns\":{:.1},\"p10_ns\":{:.1},\"p90_ns\":{:.1},\"min_ns\":{:.1},\"max_ns\":{:.1},\"batches\":{}",
        at(0.5),
        at(0.1),
        at(0.9),
        samples[0],
        samples[samples.len() - 1],
        samples.len()
    )
}

struct Bench {
    app: App<VList>,
    render: bool,
}
impl Bench {
    /// The scene's operation: scroll `STEP` points, past the end back to the top.
    fn step(&mut self) {
        let end = self.app.rt.list_content_height_pt(0) - micro::VIEWPORT;
        let next = *self.app.owner.scroll + micro::STEP;
        set_scroll(&mut self.app, if next > end { 0. } else { next });
    }
    fn frame(&mut self) {
        self.app.frame();
    }
    fn settle(&mut self) {
        #[cfg(target_os = "macos")]
        if self.render {
            self.app.wait_presented();
        }
    }
}

fn measure(b: &mut Bench, ops: usize, batch: usize, frame: bool) -> Vec<f64> {
    let one = |b: &mut Bench| {
        b.step();
        if frame {
            b.frame();
        }
    };
    for _ in 0..env("WARMUP", micro::WARMUP) {
        one(b);
    }
    b.settle();
    let mut samples = Vec::with_capacity(ops / batch);
    for _ in 0..ops / batch {
        let started = cpu_ns();
        for _ in 0..batch {
            one(b);
        }
        samples.push((cpu_ns() - started) as f64 / batch as f64);
        b.settle();
    }
    if !frame {
        b.frame();
        b.settle();
    }
    samples
}

fn main() {
    let scene: String = env("SCENE", "vscroll-variable".to_string());
    assert_eq!(scene, "vscroll-variable", "bough's micro_bench covers vscroll-variable (state and vscroll-fixed are not ported here)");
    let ops: usize = env("OPS", micro::OPS);
    let render = env("RENDER", 1) != 0;
    let clock = if std::env::var("CLOCK").is_ok_and(|v| v == "thread") { "thread_cpu" } else { "process_cpu" };
    let mut app = App::new(VList::new(micro::ROWS), config(2.0), render);
    app.frame();
    let mut bench = Bench { app, render };
    for (figure, frame, batch) in [("write", false, env("WRITE_BATCH", 50)), ("frame", true, env("FRAME_BATCH", 10))] {
        let mut samples = measure(&mut bench, ops, batch, frame);
        println!(
            "{{\"framework\":\"bough\",\"scene\":\"{scene}\",\"figure\":\"{figure}\",\"render\":{render},\"clock\":\"{clock}\",\"ops\":{ops},\"batch\":{batch},{}}}",
            summary(&mut samples)
        );
    }
    let _ = vlist::WIDTH;
}
