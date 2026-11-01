//! Windowed lab for the Opus iteration (one strategy per process, <= 11 s). Measurement design, fixed from the
//! prototype's bench: the only `proc_pid_rusage` calls are at scenario boundaries (process-wide instr / frame =
//! delta / frames, so no per-tick snap inflates it); in-tick main-thread work is `thread_selfcounts` (1.45 K, its
//! overhead subtracted); the publishing thread's work is counted inside the publish itself.
//!
//!   lab --strategy bh|<raster:publish:pick:n> [--mode stencil|bbox] [--secs 1.4] [--pause] [--scn a,b,..]
//!
//! `--pause` = idle-is-zero driver: the display link is paused whenever there is no damage and the presenter has
//! nothing pending; damage (here: the sparse scenario's timer, i.e. an input event) un-pauses it.
//! Scenarios: idle, one-colour, scatter-80, scroll-400, full, toggle (one instance alternates A/B: caret/hover),
//! sparse (one colour change every 100 ms from a timer, link woken per change).
use bough_metal::app;
use bough_metal::gpu::*;
use bough_metal::perf::*;
use bough_metal::present::*;
use bough_metal::scene::Rng;
use bough_metal::v2::{Spec, V2};
use block2::RcBlock;
use objc2::Message;
use objc2_foundation::NSTimer;
use objc2_metal::MTLDevice;
use objc2_quartz_core::{CACurrentMediaTime, CADisplayLink};
use std::collections::HashMap;
use std::sync::atomic::Ordering::Relaxed;

// (`lab` is registered in Cargo.toml)
const ALL: [&str; 8] = ["idle", "one-colour", "scatter-80", "scroll-400", "full", "toggle", "sparse", "text"];

#[derive(Default)]
struct Scn {
    name: &'static str,
    frames: u64,
    ticks: u64,
    changes: u64,
    main_instr: Vec<f64>,
    gpu_ms: Vec<f64>,
    interval_ms: Vec<f64>,
    pub_lat_ms: Vec<f64>,
    pub_interval_ms: Vec<f64>,
    dt: f64,
    proc_instr: u64,
    wakeups: u64,
    cpu_ns: u64,
    pub_instr: u64,
    pub_n: u64,
    threads: String,
}

struct St {
    e: Engine,
    p: Box<dyn Presenter>,
    v2: Option<std::sync::Arc<bough_metal::v2::Shared>>,
    rng: Rng,
    scns: Vec<Scn>,
    order: Vec<&'static str>,
    cur: usize,
    t_start: f64,
    s_start: Snap,
    thr_start: HashMap<u32, (String, u64)>,
    pub0: (u64, u64),
    warm: u32,
    settle: u32,
    ovh: u64,
    secs: f64,
    pause: bool,
    paused: bool,
    last_ts: f64,
    pending_gpu: bool,
    last_pub_time: f64,
    toggle: bool,
    timer: Option<objc2::rc::Retained<NSTimer>>,
    direct: bool,
    idle_armed: bool,
    link: Option<objc2::rc::Retained<CADisplayLink>>,
    done: bool,
}

fn apply(st: &mut St, name: &str) {
    match name {
        "one-colour" => st.e.scene.op_one_colour(&mut st.rng),
        "scatter-80" => st.e.scene.op_scattered(&mut st.rng, 80),
        "scroll-400" => st.e.scene.op_scroll(7.0),
        "full" => st.e.scene.invalidate_all(),
        "text" => {
            // heavy text tick (BOUGH_HEAVYTEXT=1): 84 numeric labels change 3 digits + one new log line
            st.e.scene.op_numeric(&mut st.rng);
            st.e.scene.op_log(&mut st.rng);
        }
        "toggle" => {
            st.toggle = !st.toggle;
            let i = st.e.scene.small[17] as usize;
            st.e.scene.set_color(i, if st.toggle { 0xff30_60e0 } else { 0xff20_a040 });
        }
        _ => {}
    }
}

fn begin_scn(st: &mut St, now: f64) {
    st.t_start = now;
    st.thr_start = bough_metal::threads::snapshot();
    st.s_start = snap();
    st.settle = 0;
    st.idle_armed = false;
    st.pub0 = st.v2.as_ref().map(|v| (v.pub_instr.load(Relaxed), v.pub_n.load(Relaxed))).unwrap_or((0, 0));
    if let Some(v) = st.v2.clone() {
        v.lat_ms.lock().unwrap().clear();
    }
    st.last_pub_time = 0.0;
}

fn end_scn(st: &mut St, now: f64) {
    let s = snap();
    let thr = bough_metal::threads::snapshot();
    let c = &mut st.scns[st.cur];
    c.dt = now - st.t_start;
    c.proc_instr = s.instrs - st.s_start.instrs;
    c.wakeups = s.wakeups - st.s_start.wakeups;
    c.cpu_ns = s.cpu_ns - st.s_start.cpu_ns;
    let per = if c.name == "idle" { 1 } else { c.frames.max(1) };
    c.threads = bough_metal::threads::diff_top(&st.thr_start, &thr, per, 5);
    if let Some(v) = st.v2.clone() {
        let (a, b) = (v.pub_instr.load(Relaxed), v.pub_n.load(Relaxed));
        c.pub_instr = a - st.pub0.0;
        c.pub_n = b - st.pub0.1;
        c.pub_lat_ms = std::mem::take(&mut *v.lat_ms.lock().unwrap());
    }
}

/// Advance to the next scenario; returns false when finished.
fn next_scn(st: &mut St, now: f64) -> bool {
    end_scn(st, now);
    st.cur += 1;
    if st.cur >= st.order.len() {
        return false;
    }
    begin_scn(st, now);
    let name = st.order[st.cur];
    if name == "idle" || name == "sparse" {
        arm_timer(st, name);
    }
    true
}

fn wake(st: &mut St) {
    if st.paused {
        st.paused = false;
        st.link.as_ref().unwrap().setPaused(false);
    }
}

fn arm_timer(st: &mut St, name: &'static str) {
    let ptr = st as *mut St;
    let period = if name == "idle" { st.secs } else { 0.1 };
    let blk = RcBlock::new(move |_t: std::ptr::NonNull<NSTimer>| {
        let st = unsafe { &mut *ptr };
        if st.done {
            return;
        }
        let now = unsafe { CACurrentMediaTime() };
        let name = st.order[st.cur];
        if (name == "idle" && now - st.t_start >= st.secs * 0.9) || (name != "idle" && now - st.t_start >= st.secs) || (name == "idle" && !st.pause) {
            if let Some(t) = st.timer.take() {
                t.invalidate();
            }
            // leave the scenario on the next tick (wake the link so the tick-driven state machine continues)
            if !next_scn(st, now) {
                st.done = true;
            }
            wake(st);
            return;
        }
        // sparse: one input-like change. Default: wake the display link to present it. --direct: present right here
        // in the event callback (no display link at all; the commit lands on the next vsync anyway).
        st.e.scene.op_one_colour(&mut st.rng);
        st.scns[st.cur].changes += 1;
        if st.direct {
            let m0 = tinstr();
            st.p.sync();
            st.p.poll(&mut st.e);
            let produced = st.e.frame(&mut *st.p);
            let m1 = tinstr();
            let c = &mut st.scns[st.cur];
            c.main_instr.push((m1 - m0).saturating_sub(st.ovh) as f64);
            if produced.is_some() {
                c.frames += 1;
            }
            if st.p.busy() {
                wake(st); // only a NextTick publish needs one more tick
            }
        } else {
            wake(st);
        }
    });
    let t = unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(period, name == "sparse", &blk) };
    st.timer = Some(t);
    std::mem::forget(blk);
}

fn tick(st: &mut St, host: &app::Host, link: &CADisplayLink) {
    let now = unsafe { CACurrentMediaTime() };
    if st.done {
        report(st);
        host.detach_all();
        host.close();
        std::process::exit(0);
    }
    if st.link.is_none() {
        st.link = Some(link.retain());
    }
    // ---- warm-up: every scenario op once, rings + pipelines hot
    if st.warm > 0 {
        st.warm -= 1;
        st.p.sync();
        st.p.poll(&mut st.e);
        let k = (st.warm % 5) as usize;
        let n = ["one-colour", "scatter-80", "scroll-400", "full", "toggle"][k];
        apply(st, n);
        st.e.frame(&mut *st.p);
        if st.warm == 0 {
            st.p.sync();
            st.p.poll(&mut st.e);
            begin_scn(st, now);
            let name = st.order[0];
            if name == "idle" || name == "sparse" {
                arm_timer(st, name);
            }
        }
        st.last_ts = link.timestamp();
        return;
    }
    let name = st.order[st.cur];
    let continuous = !(name == "idle" || name == "sparse");
    if continuous && now - st.t_start >= st.secs {
        if !next_scn(st, now) {
            st.done = true;
            return;
        }
    }
    let name = st.order[st.cur];
    let continuous = !(name == "idle" || name == "sparse");
    let sc = st.cur;
    let rec = st.settle >= 8;
    st.settle += 1;
    // ---- the tick body (this is what a real app does per tick)
    let m0 = tinstr();
    st.p.sync();
    let gt = if st.pending_gpu { st.p.gpu_times() } else { None };
    st.p.poll(&mut st.e);
    if continuous {
        apply(st, name);
    }
    let produced = st.e.frame(&mut *st.p);
    let m1 = tinstr();
    // ---- bookkeeping (outside the bracket; inside the process-wide numbers, small)
    let ts = link.timestamp();
    let (ovh, last_ts) = (st.ovh, st.last_ts);
    let c = &mut st.scns[sc];
    c.ticks += 1;
    if rec || !continuous {
        if continuous || produced.is_some() || st.p.busy() {
            c.main_instr.push((m1 - m0).saturating_sub(ovh) as f64);
        }
        if continuous {
            c.interval_ms.push((ts - last_ts) * 1e3);
        }
        if let Some((gs, ge)) = gt {
            if ge > gs && ge - gs < 0.5 {
                c.gpu_ms.push((ge - gs) * 1e3);
            }
        }
    }
    if produced.is_some() {
        c.frames += 1;
    }
    st.pending_gpu = produced.is_some();
    // publish interval (v2 only): successive CATransaction commits; latency is collected inside the publish
    if let Some(v) = st.v2.clone() {
        let pt = f64::from_bits(v.pub_time.load(Relaxed));
        if pt > 0.0 && pt != st.last_pub_time {
            if st.last_pub_time > 0.0 && rec && continuous {
                st.scns[sc].pub_interval_ms.push((pt - st.last_pub_time) * 1e3);
            }
            st.last_pub_time = pt;
        }
    }
    st.last_ts = ts;
    // ---- idle is zero: nothing damaged, nothing pending -> stop the display link
    if st.pause && !st.e.scene.acc.any && !st.p.busy() && !continuous {
        st.paused = true;
        link.setPaused(true);
        if name == "idle" && !st.idle_armed {
            // the idle window starts once the link is stopped: what is left is the true idle floor
            st.idle_armed = true;
            st.thr_start = bough_metal::threads::snapshot();
            st.s_start = snap();
            st.t_start = unsafe { CACurrentMediaTime() };
        }
    }
}

fn report(st: &mut St) {
    let s = snap();
    println!("\n### strategy `{}` damage-mode `{}` link-pause {}\n", st.p.name(), st.e.mode.name(), st.pause);
    println!("| scenario | frames | process instr / frame | main-thread in-tick instr (med) | publish-thread instr / publish | GPU ms med / p99 | tick interval med / p99 | publish interval med / p99 | present->publish-commit ms med | CPU us / frame (all threads) | wakeups / frame |");
    println!("|---|---|---|---|---|---|---|---|---|---|---|");
    for c in st.scns.iter_mut() {
        if c.name == "idle" {
            println!(
                "| idle | {} ticks in {:.2} s | **{:.0} instr/s** | {:.0} (tick body) | - | - | - | - | - | {:.1} ms CPU/s | {:.1} wakeups/s |",
                c.ticks, c.dt, c.proc_instr as f64 / c.dt, pct(&mut c.main_instr, 0.5), c.cpu_ns as f64 / 1e6 / c.dt, c.wakeups as f64 / c.dt
            );
            continue;
        }
        let per = if c.name == "sparse" { c.changes.max(1) } else { c.frames.max(1) } as f64;
        println!(
            "| {}{} | {} | **{:.0}** | {:.0} | {} | {:.3} / {:.3} | {:.2} / {:.2} | {:.2} / {:.2} | {:.2} | {:.1} | {:.2} |",
            c.name,
            if c.name == "sparse" { format!(" ({} changes, {} ticks)", c.changes, c.ticks) } else { String::new() },
            c.frames,
            c.proc_instr as f64 / per,
            pct(&mut c.main_instr, 0.5),
            if c.pub_n > 0 { format!("{:.0}", c.pub_instr as f64 / c.pub_n as f64 - st.ovh as f64) } else { "-".into() },
            pct(&mut c.gpu_ms, 0.5), pct(&mut c.gpu_ms, 0.99),
            pct(&mut c.interval_ms, 0.5), pct(&mut c.interval_ms, 0.99),
            pct(&mut c.pub_interval_ms, 0.5), pct(&mut c.pub_interval_ms, 0.99),
            pct(&mut c.pub_lat_ms, 0.5),
            c.cpu_ns as f64 / 1e3 / per,
            c.wakeups as f64 / per,
        );
    }
    println!();
    for c in &st.scns {
        println!("threads [{}] (us per {}): {}", c.name, if c.name == "idle" { "run" } else { "frame" }, c.threads);
    }
    println!(
        "memory: RSS {:.1} MB, phys_footprint {:.1} MB, MTLDevice.currentAllocatedSize {:.1} MB, surfaces {:.1} MB",
        s.rss as f64 / 1e6, s.footprint as f64 / 1e6, st.e.gpu.device.currentAllocatedSize() as f64 / 1e6, st.p.surface_bytes() as f64 / 1e6
    );
    let d = st.p.diag();
    if !d.is_empty() {
        println!("{d}");
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let get = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned();
    let strategy = get("--strategy").unwrap_or("bh".into());
    let secs: f64 = get("--secs").and_then(|s| s.parse().ok()).unwrap_or(1.4);
    let pause = args.iter().any(|a| a == "--pause");
    let direct = args.iter().any(|a| a == "--direct");
    let mode = get("--mode").and_then(|m| DamageMode::ALL.iter().copied().find(|d| d.name() == m)).unwrap_or(DamageMode::Stencil);
    let order: Vec<&'static str> = match get("--scn") {
        Some(s) => ALL.iter().copied().filter(|n| s.split(',').any(|x| x == *n)).collect(),
        None => ALL.iter().copied().filter(|n| *n != "text").collect(),
    };
    let mut st: Option<Box<St>> = None;
    app::run(&format!("bough-metal lab {strategy}"), move |host, link| {
        if st.is_none() {
            let mut e = Engine::new(ShaderSource::Metallib);
            e.mode = mode;
            let (p, v2): (Box<dyn Presenter>, Option<std::sync::Arc<bough_metal::v2::Shared>>) = match strategy.as_str() {
                "bh" => (Box::new(IoRing::new(&e, 3, PublishSync::Handler)), None),
                "b" => (Box::new(IoRing::new(&e, 3, PublishSync::Wait)), None),
                "a" => (Box::new(Control::new(&e)), None),
                s => {
                    let v = Box::new(V2::new(&mut e, Spec::parse(s).expect("strategy")));
                    let sh = v.sh.clone();
                    (v, Some(sh))
                }
            };
            host.attach(&p.layer());
            let scns = order.iter().map(|n| Scn { name: n, ..Default::default() }).collect();
            st = Some(Box::new(St {
                e, p, v2, rng: Rng(0xB0A5), scns, order: order.clone(), cur: 0, t_start: 0.0, s_start: snap(), thr_start: HashMap::new(), pub0: (0, 0),
                warm: 30, settle: 0, ovh: tinstr_overhead(), secs, pause, paused: false, last_ts: 0.0, pending_gpu: false,
                last_pub_time: 0.0, toggle: false, timer: None, direct, idle_armed: false, link: None, done: false,
            }));
        }
        tick(st.as_mut().unwrap(), host, link);
    });
}
