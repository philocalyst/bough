//! zgx's microbenchmarks (`zgx_workload::micro`) on bough, with `micro_bench`'s exact semantics:
//! CPU time per operation (process CPU clock, all threads) for the operation alone ("write") and
//! through to a frame ready to present ("frame"), median / p10 / p90 / min / max over batches.
//!   SCENE=state|vscroll-fixed OPS=2000 RENDER=1|0 WARMUP=200 WRITE_BATCH=50 FRAME_BATCH=10 CLOCK=thread
//! write = the scene's signal write only (`App::write`); frame = settle + text + damage + cull + draw
//! list (+ encode and commit when RENDER=1), `App::flush` + `App::frame`.
use bough::raw::*;
use bough_scenes::*;
use zgx_workload::micro::{self, *};

fn env<T: std::str::FromStr>(name: &str, default: T) -> T {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

fn cpu_ns() -> u64 {
    #[repr(C)]
    struct Timespec {
        s: i64,
        ns: i64,
    }
    unsafe extern "C" {
        fn clock_gettime(clock: i32, t: *mut Timespec) -> i32;
    }
    let thread = std::env::var("CLOCK").is_ok_and(|v| v == "thread");
    let clock = if thread { 16 } else { 12 }; // CLOCK_THREAD_CPUTIME_ID / CLOCK_PROCESS_CPUTIME_ID on macOS
    let mut t = Timespec { s: 0, ns: 0 };
    unsafe { clock_gettime(clock, &mut t) };
    t.s as u64 * 1_000_000_000 + t.ns as u64
}

trait Scene {
    fn step(&mut self, op: usize);
    fn frame(&mut self);
    fn settle(&mut self);
}

struct Bench<V: View> {
    app: App<V>,
    write: fn(&mut V, usize),
}
impl<V: View> Scene for Bench<V> {
    fn step(&mut self, op: usize) {
        let w = self.write;
        self.app.write(|v| w(v, op));
    }
    fn frame(&mut self) {
        self.app.flush();
        self.app.frame();
    }
    fn settle(&mut self) {
        #[cfg(target_os = "macos")]
        if let Some(g) = self.app.gpu.as_mut() {
            g.sync();
        }
    }
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

fn measure(b: &mut dyn Scene, ops: usize, batch: usize, frame: bool, next: &mut usize) -> Vec<f64> {
    let one = |b: &mut dyn Scene, next: &mut usize| {
        b.step(*next);
        *next += 1;
        if frame {
            b.frame();
        }
    };
    for _ in 0..env("WARMUP", WARMUP) {
        one(b, next);
    }
    b.settle();
    let mut samples = Vec::with_capacity(ops / batch);
    for _ in 0..ops / batch {
        let started = cpu_ns();
        for _ in 0..batch {
            one(b, next);
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
    let scene: String = env("SCENE", "state".to_string());
    let ops: usize = env("OPS", micro::OPS);
    let render = env("RENDER", 1) != 0;
    let clock = if std::env::var("CLOCK").as_deref() == Ok("thread") { "thread_cpu" } else { "process_cpu" };
    let cfg = micro_config(&scene, 2.0);
    let mut bench: Box<dyn Scene> = match scene.as_str() {
        "state" => {
            let mut b = Bench { app: App::new(MicroState { counter: Signal::new(0) }, cfg, render), write: |v: &mut MicroState, op| {
                v.counter.set(op as u64 + 1);
            } };
            b.app.frame();
            Box::new(b)
        }
        "vscroll-fixed" => {
            let mut b = Bench { app: App::new(MicroScroll { scroll: Signal::new(0.0) }, cfg, render), write: |v: &mut MicroScroll, _| {
                let end = ROWS as f32 * FIXED_ROW - VIEWPORT;
                let next = *v.scroll + STEP;
                v.scroll.set(if next > end { 0.0 } else { next });
            } };
            b.app.frame();
            Box::new(b)
        }
        other => panic!("SCENE {other}: state or vscroll-fixed (vscroll-variable needs the layout engine)"),
    };
    let mut next = 0;
    for (figure, frame, batch) in [("write", false, env("WRITE_BATCH", 50)), ("frame", true, env("FRAME_BATCH", 10))] {
        let mut samples = measure(&mut *bench, ops, batch, frame, &mut next);
        println!(
            "{{\"framework\":\"bough\",\"scene\":\"{scene}\",\"figure\":\"{figure}\",\"render\":{render},\"clock\":\"{clock}\",\"ops\":{ops},\"batch\":{batch},{}}}",
            summary(&mut samples)
        );
    }
}
