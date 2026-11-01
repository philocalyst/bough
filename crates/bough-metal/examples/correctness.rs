//! Byte-exact correctness of every strategy x damage mode against a full redraw.
//! usage: correctness [frames=300] [a,b,c] [bbox,multi-shared,multi-culled,stencil]
use bough_metal::gpu::*;
use bough_metal::instance::*;
use bough_metal::present::*;
use bough_metal::scene::Rng;
use std::time::Instant;

fn random_ops(e: &mut Engine, rng: &mut Rng) {
    if e.scene.heavy_text {
        match rng.below(4) {
            0 => e.scene.op_numeric(rng),
            1 => e.scene.op_log(rng),
            _ => {}
        }
    }
    let r = rng.below(100);
    match r {
        0..=14 => {}
        15..=39 => e.scene.op_one_colour(rng),
        40..=59 => {
            let k = 2 + rng.below(79);
            e.scene.op_scattered(rng, k)
        }
        60..=71 => e.scene.op_move_card(rng),
        72..=83 => {
            let d = rng.below(120) as f32 - 40.0;
            e.scene.op_scroll(d)
        }
        84..=91 => e.scene.op_resize(rng),
        92..=94 => {
            let c = rng.color();
            e.scene.set_background(c)
        }
        95..=96 => e.scene.invalidate_all(),
        _ => {
            // combined
            e.scene.op_one_colour(rng);
            e.scene.op_move_card(rng);
            e.scene.op_scroll(37.0);
            e.scene.op_resize(rng);
        }
    }
}

fn diff(a: &[u8], b: &[u8]) -> Option<(usize, usize, usize)> {
    if a == b {
        return None;
    }
    let mut n = 0;
    let mut first = 0;
    for i in (0..a.len()).step_by(4) {
        if a[i..i + 4] != b[i..i + 4] {
            if n == 0 {
                first = i / 4;
            }
            n += 1;
        }
    }
    Some((n, first % W as usize, first / W as usize))
}

struct Run {
    strategy: String,
    mode: DamageMode,
    e: Engine,
    p: Box<dyn Presenter>,
    rng: Rng,
    f: usize,
    frames: usize,
    presented: usize,
    bad: usize,
    full_repairs: usize,
    t0: Instant,
}

impl Run {
    fn new(strategy: &str, mode: DamageMode, frames: usize) -> Run {
        let mut e = Engine::new(ShaderSource::Metallib);
        e.mode = mode;
        e.enable_capture();
        let p: Box<dyn Presenter> = match strategy {
            "a" => Box::new(Control::new(&e)),
            "b" => Box::new(IoRing::new(&e, 3, PublishSync::Wait)),
            "bs" => Box::new(IoRing::new(&e, 3, PublishSync::Scheduled)),
            "bh" => Box::new(IoRing::new(&e, 3, PublishSync::Handler)),
            "c" => Box::new(MetalAge::new(&e, false, DrawableId::IOSurface)),
            s => Box::new(bough_metal::v2::V2::new(&mut e, bough_metal::v2::Spec::parse(s).expect("strategy: a|b|bs|bh|c|raster:publish:pick:n"))),
        };
        Run { strategy: strategy.into(), mode, e, p, rng: Rng(0x1234_5678_9abc_def1 ^ (mode as u64 + 1) * 77), f: 0, frames, presented: 0, bad: 0, full_repairs: 0, t0: Instant::now() }
    }
    /// One scripted frame; returns true when finished.
    fn step(&mut self) -> bool {
        let (e, p, rng) = (&mut self.e, &mut self.p, &mut self.rng);
        p.sync();
        p.poll(e);
        random_ops(e, rng);
        if e.frame(&mut **p).is_some() {
            self.presented += 1;
            if e.region.full {
                self.full_repairs += 1;
            }
            p.sync();
            let got = p.presented_pixels(e);
            let want = p.reference(e);
            if let Some((n, x, y)) = diff(&got, &want) {
                self.bad += 1;
                if self.bad <= 3 {
                    println!("  MISMATCH {}/{} frame {}: {n} px differ, first at ({x},{y}), region full={} bbox={:?} rects={}", self.strategy, self.mode.name(), self.f, e.region.full, e.region.bbox, e.region.rects.len());
                }
            }
        }
        self.f += 1;
        self.f >= self.frames
    }
    fn report(&self) {
        let d = self.p.diag();
        if !d.is_empty() && self.strategy.contains(':') {
            println!("    {d}");
        }
        println!("{:<8} {:<14} {:>9} {:>9} {:>11} {:>8.1}", self.strategy, self.mode.name(), self.presented, self.bad, self.full_repairs, self.t0.elapsed().as_secs_f64());
    }
}

fn main() {
    let args: Vec<String> = std::env::args().filter(|a| a != "--window").collect();
    let windowed = std::env::args().any(|a| a == "--window");
    let frames: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(300);
    let strategies: Vec<String> = args.get(2).map(|s| s.split(',').map(String::from).collect()).unwrap_or(vec!["a".into(), "b".into(), "c".into()]);
    let modes: Vec<DamageMode> = match args.get(3) {
        Some(s) => DamageMode::ALL.iter().copied().filter(|m| s.split(',').any(|x| x == m.name())).collect(),
        None => DamageMode::ALL.to_vec(),
    };
    println!("{} presentation", if windowed { "WINDOWED (visible window, display-link paced)" } else { "headless (detached layers)" });
    println!("{:<8} {:<14} {:>9} {:>9} {:>11} {:>8}", "strategy", "damage-mode", "presented", "mismatch", "full-repair", "secs");
    let mut jobs: Vec<(String, DamageMode)> = vec![];
    for s in &strategies {
        for m in &modes {
            jobs.push((s.clone(), *m));
        }
    }
    let finish = |fail: usize| {
        if fail > 0 {
            println!("FAIL: {fail} mismatching frames");
            std::process::exit(1);
        }
        println!("OK: all presented frames byte-identical to a full redraw");
        std::process::exit(0);
    };
    if !windowed {
        let mut fail = 0;
        for (s, m) in &jobs {
            let mut r = Run::new(s, *m, frames);
            while !r.step() {}
            r.report();
            fail += r.bad;
        }
        finish(fail);
    }
    let mut jobs = jobs.into_iter();
    let mut cur: Option<Run> = None;
    let mut fail = 0;
    bough_metal::app::run("bough-metal correctness", move |host, _| {
        if cur.is_none() {
            match jobs.next() {
                Some((s, m)) => {
                    let r = Run::new(&s, m, frames);
                    host.attach(&r.p.layer());
                    cur = Some(r);
                }
                None => {
                    host.close();
                    finish(fail);
                }
            }
        }
        if let Some(r) = cur.as_mut() {
            if r.step() {
                r.report();
                fail += r.bad;
                host.detach_all();
                cur = None;
            }
        }
    });
}
