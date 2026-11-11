//! Presentation latency/pacing probe.
//!   (c) CAMetalLayer: drawable.presentedTime - tick start  (ground truth from the system)
//!   (b) IOSurface ring: the compositor releases a surface (IOSurfaceIsInUse -> false) when it has latched a newer
//!       commit; a spinning poller thread timestamps those releases. Calibrated against (c) the same way.
//! usage: latency [b|bh|bs|c] [frames=300]
use bough_metal::app;
use bough_metal::gpu::*;
use bough_metal::perf::pct;
use bough_metal::present::*;
use bough_metal::scene::Rng;
use objc2_core_foundation::CFRetained;
use objc2_io_surface::IOSurfaceRef;
use objc2_quartz_core::CACurrentMediaTime;
use std::sync::{Arc, Mutex};

struct SendRef(CFRetained<IOSurfaceRef>);
unsafe impl Send for SendRef {}

fn now() -> f64 {
    CACurrentMediaTime()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let kind = args.get(1).cloned().unwrap_or("bh".into());
    let frames: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(300);
    let maxd: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(3);
    let maxq: usize = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(99); // c only: max presents outstanding
    let mut issued = 0usize;
    let mut engine: Option<(Engine, Box<dyn Presenter>)> = None;
    let mut rng = Rng(7);
    let presented: Arc<Mutex<Vec<f64>>> = Arc::new(Mutex::new(vec![]));
    let rel: Arc<Mutex<Vec<(usize, f64, bool)>>> = Arc::new(Mutex::new(vec![]));
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut tick_times: Vec<f64> = vec![];
    let mut publish_idx: Vec<usize> = vec![];
    let mut targets: Vec<f64> = vec![];
    let (presented2, rel2, stop2) = (presented.clone(), rel.clone(), stop.clone());
    let mut n = 0usize;
    app::run("bough-metal latency", move |host, link| {
        if engine.is_none() {
            let e = Engine::new(ShaderSource::Metallib);
            let p: Box<dyn Presenter> = match kind.as_str() {
                "c" => {
                    let mut m = MetalAge::new(&e, true, DrawableId::IOSurface);
                    m.presented_log = Some(presented2.clone());
                    m.layer.setMaximumDrawableCount(maxd);
                    Box::new(m)
                }
                "bs" => Box::new(IoRing::new(&e, 3, PublishSync::Scheduled)),
                "b" => Box::new(IoRing::new(&e, 3, PublishSync::Wait)),
                _ => Box::new(IoRing::new(&e, 3, PublishSync::Handler)),
            };
            host.attach(&p.layer());
            engine = Some((e, p));
        }
        let (e, p) = engine.as_mut().unwrap();
        if kind == "c" && issued.saturating_sub(presented2.lock().unwrap().len()) >= maxq {
            return; // throttle: too many frames outstanding
        }
        issued += 1;
        p.sync();
        // first frames: spin up the poller on the ring's surfaces once they exist
        if n == 5 && kind != "c" {
            // IoRing surfaces are only reachable through the concrete type; recreate pointers via downcast-free trick:
            // the presenter exposes them through `diag`-independent accessor below.
        }
        n += 1;
        let t = now();
        e.scene.op_one_colour(&mut rng);
        tick_times.push(t);
        targets.push(link.targetTimestamp());
        e.frame(&mut **p);
        if n == 8 && kind != "c" {
            // start poller: look the surfaces up by id (IOSurfaceLookup) so no concrete type is needed
            let ids = p.surface_ids();
            let (rel3, stop3) = (rel2.clone(), stop2.clone());
            std::thread::spawn(move || {
                let refs: Vec<(usize, SendRef)> = ids.iter().filter_map(|&id| objc2_io_surface::IOSurfaceRef::lookup(id as u32).map(|r| (id, SendRef(r)))).collect();
                let mut last: Vec<bool> = refs.iter().map(|r| r.1 .0.is_in_use()).collect();
                while !stop3.load(std::sync::atomic::Ordering::Relaxed) {
                    for (k, (id, r)) in refs.iter().enumerate() {
                        let u = r.0.is_in_use();
                        if u != last[k] {
                            rel3.lock().unwrap().push((*id, now(), u));
                            last[k] = u;
                        }
                    }
                }
            });
        }
        if kind != "c" {
            publish_idx.push(p.last_surface_id());
        }
        if n >= frames {
            stop2.store(true, std::sync::atomic::Ordering::Relaxed);
            std::thread::sleep(std::time::Duration::from_millis(50));
            let skip = 20;
            if kind == "c" {
                let pr = presented2.lock().unwrap().clone();
                let mut l: Vec<f64> = pr.iter().zip(tick_times.iter()).skip(skip).filter(|(p, _)| **p > 0.0).map(|(p, t)| (p - t) * 1e3).collect();
                let mut to_target: Vec<f64> = pr.iter().zip(targets.iter()).skip(skip).filter(|(p, _)| **p > 0.0).map(|(p, t)| (p - t) * 1e3).collect();
                println!("(c) CAMetalLayer: tick-start -> presentedTime: median {:.2} ms, p99 {:.2} ms, min {:.2}, max {:.2}  (n={})", pct(&mut l.clone(), 0.5), pct(&mut l.clone(), 0.99), pct(&mut l.clone(), 0.0), pct(&mut l, 1.0), pr.len());
                println!("    presentedTime - displayLink.targetTimestamp: median {:.2} ms, p99 {:.2} ms", pct(&mut to_target.clone(), 0.5), pct(&mut to_target, 0.99));
            } else {
                let ev = rel2.lock().unwrap().clone();
                let rels: Vec<f64> = ev.iter().filter(|(_, t, u)| !*u && *t > targets[skip]).map(|x| x.1).collect();
                let period = (targets[skip + 100] - targets[skip]) / 100.0;
                let mut bins = [0u32; 8];
                for r in &rels {
                    let ph = (r - targets[skip]).rem_euclid(period) / period;
                    bins[((ph * 8.0) as usize).min(7)] += 1;
                }
                println!("({}) IOSurface ring: {} compositor releases over {} publishes; phase of each release within the vsync period (8 bins, 0 = on a vsync target): {:?}", kind, rels.len(), publish_idx.len() - skip, bins);
                let _ = &publish_idx;
                let mut tt: Vec<f64> = (skip..targets.len()).map(|k| (targets[k] - tick_times[k]) * 1e3).collect();
                println!("    displayLink.targetTimestamp - tick start (time to next vsync deadline): median {:.2} ms", pct(&mut tt, 0.5));
            }
            host.close();
            std::process::exit(0);
        }
    });
}
