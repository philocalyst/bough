//! Layout lane probe (round 7): where a scene's layout instructions go, at mount and per step of the resize
//! sweep. Not a benchmark row: an attribution tool.
//!
//!     cargo build --release -p bough-workloads --features cephal-stats --bin lay_probe
//!     SCENE_SCALE=0.3 REPS=24 target/release/lay_probe sheet
//!
//! Per line: the step's thread instructions for `App::resize` (which lays out) and for the frame, then the
//! layout profiler's split of the resize (`BOUGH_LPROF=1`: cephal compute, write-back; process counters with
//! their bracket subtracted), the write-back's visited / written nodes, the text measure calls, and with
//! `--features cephal-stats` cephal's query counters for the step. `LPROF=0` turns the profiler off (its brackets
//! are system calls: totals are then clean).
use bough::raw::{App, Config};
use bough_workloads::harness::sweep;
use bough_workloads::Scene;
use std::sync::atomic::Ordering::Relaxed;

fn instr() -> u64 {
    bough_mem::pmu::instructions_cycles().0
}

fn stats() -> String {
    #[cfg(feature = "cephal-stats")]
    {
        // only the counters that moved
        cephal::compute::stats::report().split(' ').filter(|kv| !kv.ends_with("=0")).collect::<Vec<_>>().join(" ")
    }
    #[cfg(not(feature = "cephal-stats"))]
    String::new()
}

/// `DUMP=path` (feature `dump`): the scene's layout trees as JSON for cephal's replay example
/// (`vendor/cephal/examples/replay.rs`): interned styles, nodes (style, children, measured text as its word
/// table), group roots with their available space, and per resize step every root's available space.
#[cfg(feature = "dump")]
mod dump {
    use bough::raw::App;
    use serde_json::{json, Value};
    use std::collections::HashMap;

    pub fn roots<V: bough::app::View>(app: &App<V>) -> Value {
        let f = |v: f32| if v.is_nan() { Value::Null } else { json!(v) };
        let lay = &app.rt.lay;
        let v: Vec<Value> = (0..lay.info.len()).filter(|&i| lay.info[i].live && lay.info[i].parent == bough::layout::NONE).map(|i| json!([i, f(lay.info[i].avail_w), f(lay.info[i].avail_h)])).collect();
        Value::Array(v)
    }

    pub fn tree<V: bough::app::View>(app: &mut App<V>) -> Value {
        let mut styles: Vec<Value> = Vec::new();
        let mut sid: HashMap<u32, usize> = HashMap::new();
        let mut nodes: Vec<Value> = Vec::new();
        let n = app.rt.lay.info.len();
        for i in 0..n {
            if !app.rt.lay.info[i].live {
                continue;
            }
            let id = i as u32;
            let st = app.rt.lay.eng.style_of(id);
            let si = *sid.entry(st).or_insert_with(|| {
                styles.push(serde_json::to_value(app.rt.lay.eng.style(id)).unwrap());
                styles.len() - 1
            });
            let kids: Vec<u32> = app.rt.lay.eng.kids(id).to_vec();
            let leaf = match app.rt.lay.eng.ctx_mut(id) {
                Some(c) if c.user.is_none() => {
                    let k = c.size_px / c.upem.max(1.0);
                    let words: Vec<[i32; 2]> = if c.fast { c.para.words.iter().map(|w| [w.x0, w.x1]).collect() } else { Vec::new() };
                    let lw = if c.fast { c.line_w } else { (c.label_size().0 / k.max(1e-6)) as i32 };
                    json!({"k": k, "lh": c.lh_px, "wrap": c.wrap, "line_w": lw, "words": words})
                }
                _ => Value::Null,
            };
            nodes.push(json!([id, si, kids, leaf]));
        }
        json!({"styles": styles, "nodes": nodes, "roots": roots(app)})
    }
}

fn run<S: Scene>() {
    let env = |k: &str| std::env::var(k).ok();
    let scale: f32 = env("SCENE_SCALE").and_then(|v| v.parse().ok()).unwrap_or(0.3);
    let reps: usize = env("REPS").and_then(|v| v.parse().ok()).unwrap_or(24);
    let lprof = env("LPROF").is_none_or(|v| v != "0");
    // `MOUNTS=n`: mount n times first (for a sampling profiler; each app dropped), then the measured run
    for _ in 0..env("MOUNTS").and_then(|v| v.parse::<usize>().ok()).unwrap_or(0) {
        let model = S::build(scale);
        let mut c = Config::new(S::SIZE.0, S::SIZE.1, 2.0, bough_workloads::font());
        c.caps = model.caps();
        drop(App::new(model, c.once(), false));
    }
    let model = S::build(scale);
    println!("# scene={} model=\"{}\" scale={scale} lprof={lprof}", S::NAME, model.describe());
    let mut c = Config::new(S::SIZE.0, S::SIZE.1, 2.0, bough_workloads::font());
    c.caps = model.caps();
    c = c.once();
    let _ = stats();
    let m0 = bough::layout::MEASURE_CALLS.load(Relaxed);
    let i0 = instr();
    let mut app = App::new(model, c, false);
    let mount = instr() - i0;
    if lprof {
        app.rt.lay.prof.enable();
    }
    let ls = app.rt.lay.stats;
    println!(
        "mount\tinstr={mount}\tcompute={}\twriteback={}\tlay_nodes={}\tlabels/texts={}\tmeasure={}\twb_visited={}\twb_written={}\t{}",
        app.rt.lay.prof.acc[bough::layout::LP_COMPUTE],
        app.rt.lay.prof.acc[bough::layout::LP_WRITE],
        app.rt.lay.info.iter().filter(|l| l.live).count(),
        app.rt.texts.len(),
        bough::layout::MEASURE_CALLS.load(Relaxed) - m0,
        ls.nodes_visited,
        ls.nodes_written,
        stats()
    );
    let i0 = instr();
    app.frame();
    println!("first_frame\tinstr={}", instr() - i0);
    #[cfg(feature = "dump")]
    let mut dumped = env("DUMP").map(|p| (p, dump::tree(&mut app), Vec::<serde_json::Value>::new()));
    let _ = stats();
    for (k, &(w, h)) in sweep(S::SIZE, reps).iter().enumerate() {
        let (p0, s0, m0) = (app.rt.lay.prof.acc, app.rt.lay.stats, bough::layout::MEASURE_CALLS.load(Relaxed));
        let i0 = instr();
        app.resize(w, h);
        let i1 = instr();
        app.frame();
        let i2 = instr();
        let (p1, s1) = (app.rt.lay.prof.acc, app.rt.lay.stats);
        #[cfg(feature = "dump")]
        if let Some((_, _, steps)) = dumped.as_mut() {
            steps.push(serde_json::json!([w, h, dump::roots(&app)]));
        }
        use bough::layout::{LP_COMPUTE, LP_WRITE};
        println!(
            "step {k:2}\t{w}x{h}\tresize={}\tframe={}\tcompute={}\twriteback={}\tvisited={}\twritten={}\tmeasure={}\t{}",
            i1 - i0,
            i2 - i1,
            p1[LP_COMPUTE] - p0[LP_COMPUTE],
            p1[LP_WRITE] - p0[LP_WRITE],
            s1.nodes_visited - s0.nodes_visited,
            s1.nodes_written - s0.nodes_written,
            bough::layout::MEASURE_CALLS.load(Relaxed) - m0,
            stats()
        );
    }
    #[cfg(feature = "dump")]
    if let Some((path, mut tree, steps)) = dumped {
        tree["steps"] = serde_json::Value::Array(steps);
        tree["scene"] = serde_json::json!(S::NAME);
        std::fs::write(&path, serde_json::to_vec(&tree).unwrap()).unwrap();
        eprintln!("dumped {path}");
    }
}

fn main() {
    let name = std::env::args().nth(1).unwrap_or_else(|| "sheet".into());
    match name.as_str() {
        #[cfg(feature = "sheet")]
        "sheet" => run::<bough_workloads::sheet::Sheet>(),
        #[cfg(feature = "chat")]
        "chat" => run::<bough_workloads::chat::Chat>(),
        #[cfg(feature = "ide")]
        "ide" => run::<bough_workloads::ide::Ide>(),
        #[cfg(feature = "dash")]
        "dash" => run::<bough_workloads::dash::Dash>(),
        #[cfg(feature = "canvas")]
        "canvas" => run::<bough_workloads::canvas::Canvas>(),
        _ => eprintln!("lay_probe: unknown scene {name}"),
    }
}
