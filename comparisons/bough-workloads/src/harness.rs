//! The phase driver: every scene goes through the same phases, each step measured on its own.
//!
//! A step is one turn's worth of work (the scene's interaction, a resize, a tick) followed by its frame
//! (`App::frame`: damage, tile list and, with `RENDER=1`, the GPU's repair of the offscreen surface; the step then
//! waits for the GPU, so wall time is input to pixels). Each step is bracketed by the calling thread's counters
//! (`bough_mem::pmu`, bracket cost subtracted) and a monotonic clock; a phase prints medians and the 95th
//! percentile over its steps. Counters count this thread only: the GPU and any worker threads are outside them
//! (their cost shows in wall time).
//!
//! Phases (`PHASES`, comma separated; default all but idle5):
//! * `cold`: build the model, mount (`App::new`), first frame. Three rows: `build` (the application's own data,
//!   not the framework's), `mount`, `first_frame`, plus `since_exec` (process exec to first frame, wall).
//! * `steady`: `Scene::steady` steps.
//! * `hover`: pointer onto / off the element with thousands of dependents.
//! * `theme`: theme switches.
//! * `resize`: a sweep of window sizes (a triangle wave between 60 % and 125 % of the scene's size in both axes).
//! * `resize_anim`: the same sweep while the scene's transitions run (cross-fades during resize): `animate`
//!   every 6 steps, the frame clock advanced 1/60 s per step.
//! * `anim`: transitions alone: `animate` every 20 steps, `tick(1/60)` and a frame per step.
//! * `idle`: turns that change nothing, each with its frame.
//! * `idle5`: five seconds asleep, then the idle phase again (the memory lane's "idle after five seconds").
//!
//! Environment: `REPS` (steps per phase, default 48), `SCENE_SCALE` (default 1), `MODE` (`once`, the default:
//! wires only; `redescribe`: the owner re-describes every turn), `RENDER` (default 1 on macOS: the GPU draws;
//! 0: frames end at the tile list), `SCALE` (device pixels per point, default 2), `PHASES`, `BOUGH_PMU` (extra
//! counter events, Linux), `STEPS=1` (also print every step: `#step scene phase k instructions cycles wall_us`).
use crate::{Scene, sys};
use bough::raw::{App, Config};
use bough_mem::pmu;
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Cold,
    Steady,
    Hover,
    Theme,
    Resize,
    ResizeAnim,
    Anim,
    Idle,
    Idle5,
}

impl Phase {
    pub const DEFAULT: [Phase; 8] = [Phase::Cold, Phase::Steady, Phase::Hover, Phase::Theme, Phase::Resize, Phase::ResizeAnim, Phase::Anim, Phase::Idle];
    pub fn name(self) -> &'static str {
        match self {
            Phase::Cold => "cold",
            Phase::Steady => "steady",
            Phase::Hover => "hover",
            Phase::Theme => "theme",
            Phase::Resize => "resize",
            Phase::ResizeAnim => "resize_anim",
            Phase::Anim => "anim",
            Phase::Idle => "idle",
            Phase::Idle5 => "idle5",
        }
    }
    pub fn parse(s: &str) -> Option<Phase> {
        [Phase::Cold, Phase::Steady, Phase::Hover, Phase::Theme, Phase::Resize, Phase::ResizeAnim, Phase::Anim, Phase::Idle, Phase::Idle5].into_iter().find(|p| p.name() == s.trim())
    }
}

/// One run's settings (from the environment: [`RunCfg::from_env`]).
#[derive(Clone, Debug)]
pub struct RunCfg {
    pub scale: f32,
    pub reps: usize,
    pub render: bool,
    pub once: bool,
    pub device_scale: f32,
    pub phases: Vec<Phase>,
}

impl RunCfg {
    pub fn from_env() -> RunCfg {
        let env = |k: &str| std::env::var(k).ok();
        RunCfg {
            scale: env("SCENE_SCALE").and_then(|v| v.parse().ok()).unwrap_or(1.0),
            reps: env("REPS").and_then(|v| v.parse().ok()).unwrap_or(48),
            render: cfg!(target_os = "macos") && env("RENDER").is_none_or(|v| v != "0"),
            once: env("MODE").is_none_or(|v| v != "redescribe"),
            device_scale: env("SCALE").and_then(|v| v.parse().ok()).unwrap_or(2.0),
            phases: match env("PHASES") {
                Some(p) => p.split(',').filter_map(Phase::parse).collect(),
                None => Phase::DEFAULT.to_vec(),
            },
        }
    }
}

/// One measured step.
#[derive(Clone, Copy, Default)]
struct Step {
    ev: [u64; pmu::MAX],
    ns: u64,
    allocs: u64,
}

/// Counters, clock and allocation count around `f` (bracket cost subtracted from the counters).
struct Meter {
    n: usize,
    bracket: [u64; pmu::MAX],
}

impl Meter {
    fn new() -> Meter {
        let n = pmu::sample_events().len();
        // the bracket's own cost: median of 64 empty brackets per event
        let mut v: Vec<[u64; pmu::MAX]> = (0..64)
            .map(|_| {
                let a = pmu::sample();
                let b = pmu::sample().since(&a);
                b.v
            })
            .collect();
        let mut bracket = [0u64; pmu::MAX];
        for e in 0..n {
            v.sort_by_key(|x| x[e]);
            bracket[e] = v[v.len() / 2][e];
        }
        Meter { n, bracket }
    }
    #[inline(never)]
    fn step(&self, f: impl FnOnce()) -> Step {
        let al = bough_mem::count::allocs();
        let t = Instant::now();
        let a = pmu::sample();
        f();
        let d = pmu::sample().since(&a);
        let ns = t.elapsed().as_nanos() as u64;
        let mut s = Step { ns, allocs: (bough_mem::count::allocs() - al) as u64, ..Step::default() };
        for e in 0..self.n {
            s.ev[e] = d.v[e].saturating_sub(self.bracket[e]);
        }
        s
    }
}

fn pct(v: &mut [u64], p: f64) -> u64 {
    if v.is_empty() {
        return 0;
    }
    v.sort_unstable();
    v[((v.len() - 1) as f64 * p).round() as usize]
}

/// The phase table's header.
pub fn header() -> String {
    let evs = pmu::sample_events();
    let mut h = String::from("scene\tphase\tsteps\tinstr_p50\tinstr_p95\tcycles_p50");
    for e in evs.iter().skip(2) {
        h += &format!("\t{}_p50", e.name());
    }
    h += "\twall_us_p50\twall_us_p95\twall_us_max\tallocs_per_step\tfootprint_mib\tpeak_mib";
    h
}

fn row(scene: &str, phase: &str, m: &Meter, steps: &[Step]) {
    if std::env::var("STEPS").is_ok_and(|v| v == "1") {
        for (k, st) in steps.iter().enumerate() {
            println!("#step\t{scene}\t{phase}\t{k}\t{}\t{}\t{:.1}", st.ev[0], st.ev[1], st.ns as f64 / 1e3);
        }
    }
    let col = |e: usize| steps.iter().map(|s| s.ev[e]).collect::<Vec<u64>>();
    let mut ins = col(0);
    let mut cyc = col(1);
    let mut wall: Vec<u64> = steps.iter().map(|s| s.ns).collect();
    let allocs: u64 = steps.iter().map(|s| s.allocs).sum();
    let (fp, peak) = sys::memory_mib();
    let mut line = format!("{scene}\t{phase}\t{}\t{}\t{}\t{}", steps.len(), pct(&mut ins, 0.5), pct(&mut ins, 0.95), pct(&mut cyc, 0.5));
    for e in 2..m.n {
        line += &format!("\t{}", pct(&mut col(e), 0.5));
    }
    let us = |ns: u64| ns as f64 / 1e3;
    let counted = bough_mem::count::installed();
    line += &format!(
        "\t{:.1}\t{:.1}\t{:.1}\t{}\t{:.1}\t{:.1}",
        us(pct(&mut wall, 0.5)),
        us(pct(&mut wall, 0.95)),
        us(pct(&mut wall, 1.0)),
        if counted { format!("{:.1}", allocs as f64 / steps.len().max(1) as f64) } else { "-".into() },
        fp,
        peak
    );
    println!("{line}");
}

/// The resize sweep's sizes: a triangle wave between 60 % and 125 % of `base`, `n` steps, x and y out of phase.
pub fn sweep(base: (f32, f32), n: usize) -> Vec<(f32, f32)> {
    (0..n)
        .map(|k| {
            let tri = |t: f32| 1.0 - (2.0 * (t - (t + 0.5).floor())).abs(); // 0..1..0, period 1
            let t = k as f32 / 24.0;
            let fx = 0.60 + 0.65 * tri(t);
            let fy = 0.60 + 0.65 * tri(t + 0.25);
            ((base.0 * fx).round(), (base.1 * fy).round())
        })
        .collect()
}

/// Mount `S` per `cfg` and run its phases, printing one row per phase (after a `#` line describing the model).
pub fn run<S: Scene>(cfg: &RunCfg) {
    let m = Meter::new();
    let name = S::NAME;
    // cold: build the model, mount, first frame
    let t = Instant::now();
    let model = S::build(cfg.scale);
    let build_ns = t.elapsed().as_nanos() as u64;
    let (w, h) = S::SIZE;
    let mut c = Config::new(w, h, cfg.device_scale, crate::font());
    c.caps = model.caps();
    if cfg.once {
        c = c.once();
    }
    println!(
        "# scene={name} model=\"{}\" fanout={} scene_scale={} mode={} render={} device_scale={} window={}x{} reps={}",
        model.describe(),
        model.fanout(),
        cfg.scale,
        if cfg.once { "once" } else { "redescribe" },
        cfg.render as u8,
        cfg.device_scale,
        w,
        h,
        cfg.reps
    );
    let mut app: Option<App<S>> = None;
    let mount = m.step(|| app = Some(App::new(model, c, cfg.render)));
    let mut app = app.unwrap();
    let render = cfg.render;
    let frame = move |app: &mut App<S>| {
        app.frame();
        if render {
            app.wait_presented();
        }
    };
    let first = m.step(|| frame(&mut app));
    println!("# records={} since_exec_ms={:.1} build_ms={:.1}", app.rt.bough.len(), sys::since_exec_ms(), build_ns as f64 / 1e6);
    if cfg.phases.contains(&Phase::Cold) {
        row(name, "mount", &m, &[mount]);
        row(name, "first_frame", &m, &[first]);
    }
    let reps = cfg.reps.max(1);
    let mut k_anim = 0usize;
    for &p in &cfg.phases {
        let steps: Vec<Step> = match p {
            Phase::Cold => continue,
            Phase::Steady => (0..reps).map(|k| m.step(|| { S::steady(&mut app, k); frame(&mut app) })).collect(),
            Phase::Hover => (0..reps).map(|k| m.step(|| { S::hover(&mut app, k); frame(&mut app) })).collect(),
            Phase::Theme => (0..reps).map(|k| m.step(|| { S::theme(&mut app, k); frame(&mut app) })).collect(),
            Phase::Resize | Phase::ResizeAnim => {
                let anim = p == Phase::ResizeAnim;
                let sizes = sweep(S::SIZE, reps);
                let v = sizes
                    .iter()
                    .enumerate()
                    .map(|(k, &(w, h))| {
                        m.step(|| {
                            if anim {
                                if k % 6 == 0 {
                                    S::animate(&mut app, k_anim);
                                    k_anim += 1;
                                }
                                app.tick(1.0 / 60.0);
                            }
                            app.resize(w, h);
                            frame(&mut app)
                        })
                    })
                    .collect();
                // back to the scene's size, settled, for the phases after
                app.resize(S::SIZE.0, S::SIZE.1);
                for _ in 0..30 {
                    app.tick(1.0 / 60.0);
                }
                frame(&mut app);
                v
            }
            Phase::Anim => (0..reps)
                .map(|k| {
                    m.step(|| {
                        if k % 20 == 0 {
                            S::animate(&mut app, k_anim);
                            k_anim += 1;
                        }
                        app.tick(1.0 / 60.0);
                        frame(&mut app)
                    })
                })
                .collect(),
            Phase::Idle | Phase::Idle5 => {
                if p == Phase::Idle5 {
                    std::thread::sleep(std::time::Duration::from_secs(5));
                }
                (0..reps).map(|_| m.step(|| { app.turn(|_| {}); frame(&mut app) })).collect()
            }
        };
        row(name, p.name(), &m, &steps);
    }
}

/// Run every named scene (`all` = [`crate::SCENES`]); unknown names are reported. Returns false if any was.
pub fn run_all(names: &[String], cfg: &RunCfg) -> bool {
    let names: Vec<String> = if names.is_empty() || names.iter().any(|n| n == "all") { crate::SCENES.iter().map(|s| s.to_string()).collect() } else { names.to_vec() };
    println!("{}", header());
    let mut ok = true;
    for n in &names {
        if !crate::run_scene(n, cfg) {
            eprintln!("workload: unknown scene {n} (known: {})", crate::SCENES.join(", "));
            ok = false;
        }
    }
    ok
}
