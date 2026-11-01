//! Windowed benchmark: one strategy per process, driven by a CADisplayLink at display rate.
//!
//!   bench [--strategy a|b|bh|c] [--mode bbox|multi-shared|multi-culled|stencil] [--secs 2]
//!         [--sweep]      (run all damage modes for the strategy, 1 s per scenario)
//!         [--startup metallib|runtime]   (headless: shader/pipeline startup costs)
use bough_metal::app;
use bough_metal::gpu::*;
use bough_metal::perf::*;
use bough_metal::present::*;
use bough_metal::scene::Rng;
use block2::RcBlock;
use objc2::Message;
use objc2_foundation::NSTimer;
use objc2_metal::MTLDevice;
use objc2_quartz_core::CACurrentMediaTime;
use std::cell::RefCell;
use std::rc::Rc;

const SCENARIOS: [&str; 5] = ["idle", "one-colour", "scatter-80", "scroll-400", "full"];

#[derive(Default)]
struct Scn {
    name: &'static str,
    frames: u64,
    ticks: u64,
    op_instr: Vec<f64>,
    present_instr: Vec<f64>,
    total_instr: Vec<f64>,
    cpu_us: Vec<f64>,
    tick_ms: Vec<f64>,
    gpu_ms: Vec<f64>,
    e2e_ms: Vec<f64>,
    interval_ms: Vec<f64>,
    draws: u64,
    msgs: u64,
    area_px: u64,
    prof: [u64; 10],
}

struct State {
    e: Engine,
    p: Box<dyn Presenter>,
    rng: Rng,
    scns: Vec<Scn>,
    cur: usize,
    scn_start: f64,
    tick_no: u64,
    settle: u32,
    last_cur: usize,
    prev: Option<(usize, Snap, f64, bool)>, // scenario, snap at tick start, media time, frame produced
    pending_gpu: Option<(usize, f64, bool)>,       // scenario, tick-start time of the frame awaiting its GPU times
    ovh: u64,
    secs: f64,
    mode: DamageMode,
    modes: Vec<DamageMode>,
    mode_idx: usize,
    phase: Phase,
    refresh_ms: f64,
    paused_result: Option<(f64, f64, f64)>, // instr/s, cpu ms/s, wake... while link paused
    results: Vec<(DamageMode, Vec<Scn>)>,
    setup_ms: f64,
    mem: Vec<String>,
    strategy: String,
    threads: bool,
    thr0: Option<std::collections::HashMap<u32, (String, u64)>>,
    thr_lines: Vec<String>,
}
#[derive(PartialEq)]
enum Phase {
    Warm(u32),
    Run,
    Paused,
    Done,
}

fn med(v: &mut Vec<f64>) -> f64 {
    pct(v, 0.5)
}

fn apply_op(e: &mut Engine, rng: &mut Rng, scn: usize) {
    match scn {
        0 => {}
        1 => e.scene.op_one_colour(rng),
        2 => e.scene.op_scattered(rng, 80),
        3 => e.scene.op_scroll(7.0),
        4 => e.scene.invalidate_all(),
        _ => unreachable!(),
    }
}

fn make_presenter(name: &str, e: &Engine) -> Box<dyn Presenter> {
    match name {
        "a" => Box::new(Control::new(e)),
        "b" => Box::new(IoRing::new(e, 3, PublishSync::Wait)),
        "bh" => Box::new(IoRing::new(e, 3, PublishSync::Handler)),
        "bs" => Box::new(IoRing::new(e, 3, PublishSync::Scheduled)),
        "c" => Box::new(MetalAge::new(e, true, DrawableId::IOSurface)),
        _ => panic!("strategy a|b|bh|c"),
    }
}

fn fresh_scns() -> Vec<Scn> {
    SCENARIOS.iter().map(|n| Scn { name: n, ..Default::default() }).collect()
}

fn print_results(st: &mut State) {
    println!("\n### strategy {} (refresh {:.2} ms, setup {:.1} ms)\n", st.strategy, st.refresh_ms, st.setup_ms);
    for (mode, scns) in st.results.iter_mut() {
        println!("damage mode `{}`\n", mode.name());
        println!("| scenario | frames/ticks | update instr | present instr | proc instr/tick (all threads) | CPU us/tick | GPU ms med / p99 | tick->GPU-done ms | tick interval ms med / p99 | draws | ObjC msgs | repair px |");
        println!("|---|---|---|---|---|---|---|---|---|---|---|---|");
        for s in scns.iter_mut() {
            let fr = s.frames.max(1);
            println!(
                "| {} | {}/{} | {:.0} | {:.0} | {:.0} | {:.1} | {:.3} / {:.3} | {:.2} | {:.2} / {:.2} | {:.1} | {:.0} | {:.0} |",
                s.name, s.frames, s.ticks,
                med(&mut s.op_instr), med(&mut s.present_instr), med(&mut s.total_instr), med(&mut s.cpu_us),
                med(&mut s.gpu_ms), pct(&mut s.gpu_ms, 0.99), med(&mut s.e2e_ms),
                med(&mut s.interval_ms), pct(&mut s.interval_ms, 0.99),
                s.draws as f64 / fr as f64, s.msgs as f64 / fr as f64, s.area_px as f64 / fr as f64,
            );
        }
        if scns[1].prof.iter().any(|&x| x > 0) {
            println!("phase profile, mean instr/frame: region-build | cull | encode | commit(+present) | wait | publish | handler-blk | nextDrawable | blit-encode");
            for s in scns.iter() {
                let f = s.frames.max(1) as f64;
                let p = &s.prof;
                println!("  {:<11} {:>8.0} | {:>7.0} | {:>7.0} | {:>7.0} | {:>7.0} | {:>7.0} | {:>7.0} | {:>7.0} | {:>7.0}", s.name, p[0] as f64 / f, p[1] as f64 / f, p[2] as f64 / f, p[3] as f64 / f, p[4] as f64 / f, p[5] as f64 / f, p[6] as f64 / f, p[7] as f64 / f, p[8] as f64 / f);
            }
        }
        println!();
    }
    for l in &st.thr_lines {
        println!("{l}");
    }
    if let Some((ips, cpu, _)) = st.paused_result {
        println!("idle with display link PAUSED (1 s): {:.0} instr/s, {:.2} ms CPU/s", ips, cpu);
    }
    for m in &st.mem {
        println!("{m}");
    }
}

fn tick(st: &mut State, host: &app::Host, link: &objc2_quartz_core::CADisplayLink) {
    let t0 = unsafe { CACurrentMediaTime() };
    let s0 = snap();
    st.tick_no += 1;
    if st.tick_no == 2 {
        st.refresh_ms = (link.targetTimestamp() - link.timestamp()) * 1e3;
    }
    let recording = st.phase == Phase::Run;
    if let Some((pi, ps, pt, prec)) = st.prev.take() {
        if recording && prec && pi == st.cur {
            let sc = &mut st.scns[pi];
            sc.total_instr.push((s0.instrs - ps.instrs).saturating_sub(st.ovh) as f64);
            sc.interval_ms.push((t0 - pt) * 1e3);
        }
    }
    st.p.sync();
    if let Some((si, ft, frec)) = st.pending_gpu.take() {
        if let Some((gs, ge)) = st.p.gpu_times() {
            // GPUStart/EndTime occasionally returns garbage (seen once: 2e8 ms); drop implausible samples
            if recording && frec && si == st.cur && (ge - gs) > 0.0 && (ge - gs) < 0.5 {
                st.scns[si].gpu_ms.push((ge - gs) * 1e3);
                st.scns[si].e2e_ms.push((ge - ft) * 1e3);
            }
        }
    }

    // advance phases
    match st.phase {
        Phase::Warm(n) => {
            // exercise every scenario once so pipelines/JIT/ring are warm
            let sc = (n as usize) % 5;
            apply_op(&mut st.e, &mut st.rng, sc);
            st.e.frame(&mut *st.p);
            st.phase = if n == 0 { Phase::Run } else { Phase::Warm(n - 1) };
            if n == 0 {
                st.scn_start = t0;
                st.cur = 0;
            }
            st.prev = Some((st.cur, s0, t0, false));
            return;
        }
        Phase::Run => {
            if st.threads && st.thr0.is_none() {
                st.thr0 = Some(bough_metal::threads::snapshot());
            }
            if t0 - st.scn_start > st.secs {
                if let Some(a) = st.thr0.take() {
                    let b = bough_metal::threads::snapshot();
                    let t = st.scns[st.cur].ticks;
                    st.thr_lines.push(format!("threads [{}]: {}", SCENARIOS[st.cur], bough_metal::threads::diff_top(&a, &b, t, 6)));
                }
                st.cur += 1;
                st.scn_start = t0;
                if st.cur == 1 && st.scns[0].ticks > 0 && st.paused_result.is_none() {
                    // measure idle with the display link paused, once, then continue at scenario 1
                    st.cur = 0;
                    start_paused_measure(st, link);
                    return;
                }
                if st.cur >= SCENARIOS.len() {
                    finish_mode(st, host, link);
                    return;
                }
            }
        }
        Phase::Paused | Phase::Done => return,
    }

    let sc = st.cur;
    if st.cur != st.last_cur {
        st.last_cur = st.cur;
        st.settle = 0;
    }
    let rec = st.settle >= 10;
    st.settle += 1;
    let m0 = st.e.gpu.msgs.get();
    let d0 = st.e.gpu.draws.get();
    let c0 = snap();
    apply_op(&mut st.e, &mut st.rng, sc);
    let c1 = snap();
    st.e.mode = st.mode;
    let produced = st.e.frame(&mut *st.p).is_some();
    let c2 = snap();
    let ovh = st.ovh;
    let s = &mut st.scns[sc];
    if !rec {
        if st.e.gpu.prof.on.get() {
            let _ = st.e.gpu.prof.take();
        }
        st.prev = Some((sc, s0, t0, false));
        if produced {
            st.pending_gpu = Some((sc, t0, false));
        }
        return;
    }
    s.ticks += 1;
    s.op_instr.push((c1.instrs - c0.instrs).saturating_sub(ovh) as f64);
    s.present_instr.push((c2.instrs - c1.instrs).saturating_sub(ovh) as f64);
    s.cpu_us.push((c2.cpu_ns - s0.cpu_ns) as f64 / 1e3);
    s.tick_ms.push((unsafe { CACurrentMediaTime() } - t0) * 1e3);
    if produced {
        s.frames += 1;
        s.msgs += (st.e.gpu.msgs.get() - m0) as u64;
        s.draws += (st.e.gpu.draws.get() - d0) as u64;
        let r = &st.e.region;
        s.area_px += if r.full || st.mode == DamageMode::Bbox { r.bbox.area() } else { r.rects.iter().map(|x| x.area()).sum() } as u64;
        st.pending_gpu = Some((sc, t0, true));
        if st.e.gpu.prof.on.get() {
            let p = st.e.gpu.prof.take();
            for i in 0..10 {
                s.prof[i] += p[i];
            }
        }
    }
    st.prev = Some((sc, s0, t0, true));
}

fn start_paused_measure(st: &mut State, link: &objc2_quartz_core::CADisplayLink) {
    st.phase = Phase::Paused;
    link.setPaused(true);
    let a = snap();
    let ta = unsafe { CACurrentMediaTime() };
    let cell: Rc<RefCell<*mut State>> = Rc::new(RefCell::new(st as *mut State));
    let link2 = link.retain();
    let blk = RcBlock::new(move |_t: std::ptr::NonNull<NSTimer>| {
        let b = snap();
        let tb = unsafe { CACurrentMediaTime() };
        let st = unsafe { &mut **cell.borrow() };
        let dt = tb - ta;
        st.paused_result = Some(((b.instrs - a.instrs) as f64 / dt, (b.cpu_ns - a.cpu_ns) as f64 / 1e6 / dt, dt));
        st.phase = Phase::Run;
        st.scn_start = tb;
        st.cur = 1;
        st.prev = None;
        link2.setPaused(false);
    });
    let _t = unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(1.0, false, &blk) };
    std::mem::forget(blk);
}

fn finish_mode(st: &mut State, host: &app::Host, _link: &objc2_quartz_core::CADisplayLink) {
    let scns = std::mem::replace(&mut st.scns, fresh_scns());
    st.results.push((st.mode, scns));
    st.mode_idx += 1;
    if st.mode_idx < st.modes.len() {
        st.mode = st.modes[st.mode_idx];
        st.cur = 0;
        st.phase = Phase::Warm(10);
        st.paused_result = st.paused_result; // keep
        return;
    }
    let s = snap();
    st.mem.push(format!(
        "memory: RSS {:.1} MB, phys_footprint {:.1} MB, MTLDevice.currentAllocatedSize {:.1} MB, strategy surface bytes {:.1} MB",
        s.rss as f64 / 1e6,
        s.footprint as f64 / 1e6,
        st.e.gpu.device.currentAllocatedSize() as f64 / 1e6,
        st.p.surface_bytes() as f64 / 1e6
    ));
    st.mem.push(st.p.diag());
    st.phase = Phase::Done;
    print_results(st);
    host.detach_all();
    host.close();
    std::process::exit(0);
}

fn startup(kind: &str) {
    let t = std::time::Instant::now();
    let src = if kind == "runtime" { ShaderSource::Runtime } else { ShaderSource::Metallib };
    let mut e = Engine::new(src);
    let total = t.elapsed().as_secs_f64() * 1e3;
    // first frame (forces pipeline residency / first command buffer)
    let t = std::time::Instant::now();
    let mut p = IoRing::new(&e, 3, PublishSync::Wait);
    e.scene.invalidate_all();
    e.frame(&mut p);
    p.sync();
    let first = t.elapsed().as_secs_f64() * 1e3;
    let g = &e.gpu.times;
    println!(
        "{kind}: device+queue {:.2} ms, library {:.2} ms, 3 pipelines {:.2} ms, pool+atlas+stencil {:.2} ms | Engine::new total {:.2} ms | ring+first full frame {:.2} ms",
        g.device_ms, g.library_ms, g.pipelines_ms, g.buffers_atlas_ms, total, first
    );
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let get = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned();
    if let Some(k) = get("--startup") {
        return startup(&k);
    }
    let strategy = get("--strategy").unwrap_or("b".into());
    let secs: f64 = get("--secs").and_then(|s| s.parse().ok()).unwrap_or(2.0);
    let sweep = args.iter().any(|a| a == "--sweep");
    let profile = args.iter().any(|a| a == "--profile");
    let threads = args.iter().any(|a| a == "--threads");
    let mode = get("--mode").and_then(|m| DamageMode::ALL.iter().copied().find(|d| d.name() == m)).unwrap_or(DamageMode::Stencil);
    let modes = if sweep { DamageMode::ALL.to_vec() } else { vec![mode] };
    let secs = if sweep { secs.min(1.0) } else { secs };

    let mut state: Option<State> = None;
    let strat2 = strategy.clone();
    app::run(&format!("bough-metal bench {strategy}"), move |host, link| {
        if state.is_none() {
            let t = std::time::Instant::now();
            let e = Engine::new(ShaderSource::Metallib);
            let p = make_presenter(&strat2, &e);
            host.attach(&p.layer());
            if profile {
                e.gpu.prof.enable();
            }
            let setup_ms = t.elapsed().as_secs_f64() * 1e3;
            println!("snap overhead {} instr; window scale factor {}, display link {:?}", snap_overhead(), host.scale(), link.duration() * 1e3);
            state = Some(State {
                e, p, rng: Rng(0xB0A5), scns: fresh_scns(), cur: 0, scn_start: 0.0, tick_no: 0, settle: 0, last_cur: 99, prev: None, pending_gpu: None,
                ovh: snap_overhead(), secs, mode: modes[0], modes: modes.clone(), mode_idx: 0, phase: Phase::Warm(30),
                refresh_ms: 0.0, paused_result: None, results: vec![], setup_ms, mem: vec![], strategy: strat2.clone(), threads, thr0: None, thr_lines: vec![],
            });
        }
        let st = state.as_mut().unwrap();
        tick(st, host, link);
    });
}
