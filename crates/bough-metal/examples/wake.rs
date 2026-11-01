//! Post-idle wake cost (headless): warm 2 s at 120 Hz, then for each idle length D: idle D (a remedy may run),
//! then 10 one-colour changes at 10 Hz. Per change: process instructions from the event to 20 ms later (completion
//! and commit threads included) and present -> commit latency. Idle: process instructions the idle period cost.
//!   wake <strategy=cb:h:free:4> <remedy=none|warm100|warm500|residency>
use bough_metal::gpu::ShaderSource;
use bough_metal::perf::*;
use bough_metal::present::{Engine, Presenter};
use bough_metal::scene::Rng;
use bough_metal::v2::{Spec, V2};
use objc2::runtime::ProtocolObject;
use objc2_metal::*;
use std::sync::atomic::Ordering::Relaxed;
use std::time::{Duration, Instant};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let strat = args.get(1).cloned().unwrap_or("cb:h:free:4".into());
    let remedy = args.get(2).cloned().unwrap_or("none".into());
    let mut e = Engine::new(ShaderSource::Metallib);
    let mut v = V2::new(&mut e, Spec::parse(&strat).expect("spec"));
    let mut rng = Rng(3);
    // `blitN` / `rblitN`: every N ms of idle, a minimal REAL GPU job (4-byte fillBuffer); `r` = + residency set
    let tiny = e.gpu.device.newBufferWithLength_options(16, MTLResourceOptions::StorageModePrivate).unwrap();
    let blit_ms: Option<u64> = remedy.trim_start_matches('r').strip_prefix("blit").and_then(|x| x.parse().ok());
    if remedy == "residency" || remedy.starts_with("rblit") {
        let d = MTLResidencySetDescriptor::new();
        unsafe { d.setInitialCapacity(16) };
        let set = e.gpu.device.newResidencySetWithDescriptor_error(&d).expect("residency set");
        set.addAllocation(ProtocolObject::from_ref(&*e.gpu.pool));
        set.addAllocation(ProtocolObject::from_ref(&*e.gpu.atlas));
        set.addAllocation(ProtocolObject::from_ref(&*e.gpu.bins));
        set.addAllocation(ProtocolObject::from_ref(&*e.gpu.bin_counts));
        for s in &v.surfs {
            set.addAllocation(ProtocolObject::from_ref(&*s.tex));
        }
        set.commit();
        set.requestResidency();
        e.gpu.queue.addResidencySet(&set);
        std::mem::forget(set);
    }
    let warm_ms: Option<u64> = remedy.strip_prefix("warm").and_then(|x| x.parse().ok());
    let frame = |e: &mut Engine, v: &mut V2, rng: &mut Rng| {
        v.sync();
        v.poll(e);
        e.scene.op_one_colour(rng);
        e.frame(v);
    };
    // warm: 2 s at 120 Hz
    for _ in 0..240 {
        frame(&mut e, &mut v, &mut rng);
        std::thread::sleep(Duration::from_micros(8333));
    }
    v.sync();
    std::thread::sleep(Duration::from_millis(50));
    println!("strategy {strat}, remedy {remedy}");
    println!("| idle D s | idle cost instr (remedy) | change 1 instr | change 1 present->commit ms | changes 2-10 instr (mean) | changes 2-10 present->commit ms (med) |");
    println!("|---|---|---|---|---|---|");
    for d in [0.1f64, 0.5, 1.5, 3.0] {
        // idle
        let s0 = snap();
        let t0 = Instant::now();
        while t0.elapsed().as_secs_f64() < d {
            match (warm_ms, blit_ms) {
                (Some(ms), _) => {
                    std::thread::sleep(Duration::from_millis(ms));
                    let c = e.gpu.queue.commandBuffer().unwrap();
                    c.commit();
                }
                (None, Some(ms)) => {
                    std::thread::sleep(Duration::from_millis(ms));
                    let c = e.gpu.queue.commandBuffer().unwrap();
                    let b = c.blitCommandEncoder().unwrap();
                    b.fillBuffer_range_value(&tiny, objc2_foundation::NSRange::new(0, 4), 0);
                    b.endEncoding();
                    c.commit();
                }
                (None, None) => std::thread::sleep(Duration::from_secs_f64((d - t0.elapsed().as_secs_f64()).max(0.0))),
            }
        }
        let idle = snap().instrs - s0.instrs;
        v.sh.lat_ms.lock().unwrap().clear();
        let mut costs = vec![];
        for _ in 0..10 {
            let a = snap();
            frame(&mut e, &mut v, &mut rng);
            std::thread::sleep(Duration::from_millis(20));
            let b = snap();
            costs.push((b.instrs - a.instrs) as f64);
            std::thread::sleep(Duration::from_millis(80));
        }
        v.sync();
        let mut lat = v.sh.lat_ms.lock().unwrap().clone();
        let first_lat = lat.first().copied().unwrap_or(f64::NAN);
        let mut rest: Vec<f64> = lat.drain(1..).collect();
        println!(
            "| {d} | {idle} | {:.0} | {first_lat:.2} | {:.0} | {:.2} |",
            costs[0], costs[1..].iter().sum::<f64>() / 9.0, pct(&mut rest, 0.5)
        );
    }
    let _ = v.sh.pub_n.load(Relaxed);
}
