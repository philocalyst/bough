//! The memory lane's whole-scene ledger on the bench lane's standing workloads (round 7): one scene, headless,
//! sampled after each phase: `startup` (mount + first frame), `steady` (REPS steps of the scene's own steady
//! interaction), `hover` (the fan-out hover), `theme` (theme switches), `resize` (the harness's sweep), `idle`
//! (5 s after the last frame, the calm trim at 1 s). Each sample: one JSON line in `SCENE_OUT` with the phase,
//! the process totals and every ledger line, plus `footprint -j` beside it (`bench/memory/scene_table.py` prints
//! them).
//!
//!   scene_ledger <sheet|ide|dash|canvas|chat> ; env REPS (default 64), SCENE_SCALE (1), NOGPU=1 (no Metal device:
//!   the kernel's CPU executor into plain-memory surfaces), SCENE_OUT (default scene_ledger.jsonl)
#[global_allocator]
static ALLOC: bough::ledger::Counting = bough::ledger::Counting;

use bough::app::{App, Config};
use bough_workloads::Scene;
use std::io::Write;
use std::time::{Duration, Instant};

struct Out {
    f: std::fs::File,
    path: String,
    t0: Instant,
    scene: &'static str,
}

impl Out {
    fn sample<S: Scene>(&mut self, app: &App<S>, phase: &str, extra: &str) {
        let (l, p) = app.ledger();
        let json = bough::ledger::to_json(&l, &p);
        let fp = format!("{}.{}.{}.footprint.json", self.path, self.scene, phase);
        let _ = std::process::Command::new("footprint").args(["-j", &fp, "-f", "bytes", "--wired", "--swapped", &std::process::id().to_string()]).output();
        let line = format!("{{\"scene\":\"{}\",\"phase\":\"{phase}\",\"t\":{:.3},{extra}\"footprint_file\":\"{fp}\",\"ledger\":{json}}}\n", self.scene, self.t0.elapsed().as_secs_f64());
        self.f.write_all(line.as_bytes()).unwrap();
        eprintln!("{} {phase}: footprint {:.1} MiB, rust live {:.1} MiB", self.scene, p.phys_footprint as f64 / 1048576.0, p.rust_live as f64 / 1048576.0);
    }
}

fn instr() -> u64 {
    bough::sys::perf::thread_instructions()
}

fn run<S: Scene>() {
    let path = std::env::var("SCENE_OUT").unwrap_or_else(|_| "scene_ledger.jsonl".into());
    let f = std::fs::OpenOptions::new().create(true).append(true).open(&path).unwrap();
    let mut out = Out { f, path, t0: Instant::now(), scene: S::NAME };
    let reps: usize = std::env::var("REPS").ok().and_then(|v| v.parse().ok()).unwrap_or(64);
    let scale: f32 = std::env::var("SCENE_SCALE").ok().and_then(|v| v.parse().ok()).unwrap_or(1.0);
    let nogpu = std::env::var_os("NOGPU").is_some();
    let model = S::build(scale);
    let (w, h) = S::SIZE;
    let mut c = Config::new(w, h, 2.0, bough_workloads::font());
    c.caps = model.caps();
    let ring = c.surfaces();
    let (i0, w0) = (instr(), Instant::now());
    let mut app = App::new(model, c, !nogpu);
    if nogpu {
        app.target = Some(Box::new(bough::backend::MemSurfaces::new((w * 2.0).round() as u32, (h * 2.0).round() as u32, ring)));
    }
    app.windowed = true;
    let mi = instr() - i0;
    app.frame();
    app.wait_presented();
    let (di, dw) = (instr() - i0, w0.elapsed());
    let recs = app.rt.bough.len();
    eprintln!("{}: startup {:.1} ms, {:.1} M instr (mount {:.1} M), {recs} records", S::NAME, dw.as_secs_f64() * 1e3, di as f64 / 1e6, mi as f64 / 1e6);
    out.sample(&app, "startup", &format!("\"startup_ns\":{},\"startup_instr\":{di},\"records\":{recs},", dw.as_nanos()));
    let frame = |app: &mut App<S>| {
        app.frame();
        app.wait_presented();
    };
    let phase = |out: &mut Out, app: &mut App<S>, name: &str, step: &dyn Fn(&mut App<S>, usize)| {
        let (i0, w0) = (instr(), Instant::now());
        for k in 0..reps {
            step(app, k);
            frame(app);
        }
        let (di, dw) = (instr() - i0, w0.elapsed());
        out.sample(app, name, &format!("\"steps\":{reps},\"ns\":{},\"instr\":{di},", dw.as_nanos()));
    };
    phase(&mut out, &mut app, "steady", &|a, k| S::steady(a, k));
    phase(&mut out, &mut app, "hover", &|a, k| S::hover(a, k));
    phase(&mut out, &mut app, "theme", &|a, k| S::theme(a, k));
    let sizes = bough_workloads::harness::sweep(S::SIZE, reps);
    phase(&mut out, &mut app, "resize", &|a, k| a.resize(sizes[k].0, sizes[k].1));
    app.resize(w, h);
    frame(&mut app);
    let last = Instant::now();
    std::thread::sleep(Duration::from_secs(1));
    app.trim(false);
    std::thread::sleep(Duration::from_secs(5).saturating_sub(last.elapsed()));
    out.sample(&app, "idle", "");
}

fn main() {
    let name = std::env::args().nth(1).unwrap_or_else(|| "sheet".into());
    match name.as_str() {
        "sheet" => run::<bough_workloads::sheet::Sheet>(),
        "ide" => run::<bough_workloads::ide::Ide>(),
        "dash" => run::<bough_workloads::dash::Dash>(),
        "canvas" => run::<bough_workloads::canvas::Canvas>(),
        "chat" => run::<bough_workloads::chat::Chat>(),
        s => panic!("unknown scene {s}"),
    }
}
