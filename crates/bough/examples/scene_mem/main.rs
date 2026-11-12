//! Whole-scene memory ledger (lane memory, round 7): one heavy scene, headless (no window, no input events:
//! `App::new(.., gpu = true)` renders into the presenter's IOSurfaces), sampled at four moments:
//!
//! 1. `startup`: after the mount, the first frame and its completion;
//! 2. `steady`: after `STEADY` frames of interaction (in-process wheel scrolling, 30 pt per frame, reversing);
//! 3. `resize`: after a storm of `STORM` window resizes through five sizes, one frame each;
//! 4. `idle`: 5 s after the last frame, the host's calm trim (`App::trim(false)`) done at 1 s.
//!
//! Each sample is one JSON line (`SCENE_OUT`, default `scene_mem.jsonl`): the phase, wall time, the process
//! (`phys_footprint`, resident, malloc, Rust live heap through the counting allocator), and every ledger line;
//! plus `footprint -j --unmapped` of the process at that moment beside it (`<out>.<phase>.footprint.json`).
//!
//!   scene_mem <feed|board|gallery-atlas> [n]
//!
//! The surface ring runs in the windowed policy (`App::windowed = true`: content-addressed, contention-sized),
//! as a window's does, not the round-robin of tests.
#[global_allocator]
static ALLOC: bough::ledger::Counting = bough::ledger::Counting;

mod scenes;

use bough::app::{App, Config, View};
use std::io::Write;
use std::time::{Duration, Instant};

fn font() -> Vec<u8> {
    let c = [std::env::var("BOUGH_FONT").ok(), Some("/System/Library/Fonts/Supplemental/Arial.ttf".into())];
    c.into_iter().flatten().find_map(|p| std::fs::read(p).ok()).expect("a font")
}

/// The applications' region capacities (`ui::APP_CAPS`).
const CAPS: bough::raw::Caps = bough::raw::Caps { instances: 1 << 16, pages: 1 << 13, glyphs: 1 << 19, runs: 1 << 16, spaces: 1 << 12, clips: 1 << 12, strikes: 16 };

struct Out {
    f: std::fs::File,
    path: String,
    t0: Instant,
    scene: String,
}

/// `SCENE_DUMP=<prefix>`: the presented surface of each phase as `<prefix>-<scene>-<phase>.png` (RGBA).
fn dump<V: View>(app: &mut App<V>, scene: &str, phase: &str) {
    let Some(pre) = std::env::var_os("SCENE_DUMP") else { return };
    let (w, h) = (app.rt.win_w as u32, app.rt.win_h as u32);
    let mut px = app.presented_pixels();
    for c in px.chunks_exact_mut(4) {
        c.swap(0, 2);
    }
    let f = std::fs::File::create(format!("{}-{scene}-{phase}.png", pre.to_string_lossy())).unwrap();
    let mut e = png::Encoder::new(std::io::BufWriter::new(f), w, h);
    e.set_color(png::ColorType::Rgba);
    e.write_header().unwrap().write_image_data(&px[..(w * h * 4) as usize]).unwrap();
}

impl Out {
    fn sample<V: View>(&mut self, app: &mut App<V>, phase: &str, extra: &str) {
        dump(app, &self.scene, phase);
        let (l, p) = app.ledger();
        let json = bough::ledger::to_json(&l, &p);
        let fp = format!("{}.{}.footprint.json", self.path, phase);
        let _ = std::process::Command::new("footprint").args(["-j", &fp, "-f", "bytes", "--wired", "--swapped", &std::process::id().to_string()]).output();
        let line = format!("{{\"scene\":\"{}\",\"phase\":\"{phase}\",\"t\":{:.3},{extra}\"footprint_file\":\"{fp}\",\"ledger\":{json}}}\n", self.scene, self.t0.elapsed().as_secs_f64());
        self.f.write_all(line.as_bytes()).unwrap();
        eprintln!("{phase}: footprint {:.1} MiB, resident {:.1} MiB, rust live {:.1} MiB, metal {:.1} MiB", p.phys_footprint as f64 / 1048576.0, p.resident as f64 / 1048576.0, p.rust_live as f64 / 1048576.0, p.metal_allocated as f64 / 1048576.0);
    }
}

fn instr() -> u64 {
    bough_metal::fused::thread_instructions()
}

fn run<V: View>(view: V, scene: &str, w: f32, h: f32) {
    let path = std::env::var("SCENE_OUT").unwrap_or_else(|_| "scene_mem.jsonl".into());
    let f = std::fs::OpenOptions::new().create(true).append(true).open(&path).unwrap();
    let mut out = Out { f, path, t0: Instant::now(), scene: scene.into() };
    let steady: usize = std::env::var("STEADY").ok().and_then(|v| v.parse().ok()).unwrap_or(600);
    let storm: usize = std::env::var("STORM").ok().and_then(|v| v.parse().ok()).unwrap_or(40);

    let mut cfg = Config::new(w, h, 2.0, font());
    cfg.caps = CAPS;
    let (i0, w0) = (instr(), Instant::now());
    let mut app = App::new(view, cfg, true);
    app.windowed = true;
    app.frame();
    app.wait_presented();
    let (di, dw) = (instr() - i0, w0.elapsed());
    let recs = app.rt.bough.len();
    eprintln!("{scene}: startup {:.1} ms, {:.1} M instr, {} records", dw.as_secs_f64() * 1e3, di as f64 / 1e6, recs);
    out.sample(&mut app, "startup", &format!("\"startup_ns\":{},\"startup_instr\":{di},\"records\":{recs},", dw.as_nanos()));

    // steady interaction: wheel 30 pt per frame, 300 down then 300 up
    let (i0, w0) = (instr(), Instant::now());
    for k in 0..steady {
        let dir = if (k / 300) % 2 == 0 { 1.0 } else { -1.0 };
        app.wheel_points(w * 0.5, h * 0.5, 30.0 * dir);
        app.frame();
        app.wait_presented();
    }
    let (di, dw) = (instr() - i0, w0.elapsed());
    let recs = app.rt.bough.len();
    out.sample(&mut app, "steady", &format!("\"frames\":{steady},\"ns\":{},\"instr\":{di},\"records\":{recs},", dw.as_nanos()));

    // resize storm
    let sizes = [(w, h), (w * 1.3, h * 1.2), (w * 0.9, h * 0.9), (w * 1.44, h * 1.3), (w * 0.76, h * 0.8)];
    let (i0, w0) = (instr(), Instant::now());
    for k in 0..storm {
        let (sw, sh) = sizes[(k + 1) % sizes.len()];
        app.resize(sw.round(), sh.round());
        app.frame();
        app.wait_presented();
    }
    app.resize(w, h);
    app.frame();
    app.wait_presented();
    let (di, dw) = (instr() - i0, w0.elapsed());
    let recs = app.rt.bough.len();
    out.sample(&mut app, "resize", &format!("\"resizes\":{storm},\"ns\":{},\"instr\":{di},\"records\":{recs},", dw.as_nanos()));

    // idle: the host's calm trim 1 s after the last frame, sampled 5 s after it
    let last = Instant::now();
    std::thread::sleep(Duration::from_secs(1));
    app.trim(false);
    std::thread::sleep(Duration::from_secs(5).saturating_sub(last.elapsed()));
    out.sample(&mut app, "idle", "");
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let scene = a.get(1).map(String::as_str).unwrap_or("feed").to_string();
    let n: usize = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
    match scene.as_str() {
        "feed" => run(scenes::feed(if n == 0 { 2000 } else { n }), &scene, 1000.0, 700.0),
        "board" => run(scenes::board(if n == 0 { 40 } else { n }, 30), &scene, 1280.0, 800.0),
        "gallery-atlas" => run(scenes::gallery_atlas(if n == 0 { 10000 } else { n }, std::env::var("DISTINCT").ok().and_then(|v| v.parse().ok()).unwrap_or(6)), &scene, 1000.0, 700.0),
        s => panic!("unknown scene {s}"),
    }
}
