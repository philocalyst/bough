//! Headless floor: instructions per Metal operation, amortised over many iterations.
//! Answers "what does a bare submission cost on this machine?" independent of our scene.
use bough_metal::gpu::*;
use bough_metal::instance::*;
use bough_metal::damage::Region;
use bough_metal::perf::*;
use bough_metal::present::*;
use block2::RcBlock;
use objc2::runtime::ProtocolObject;
use objc2_metal::*;
use std::ptr::NonNull;

fn bench(name: &str, n: usize, mut f: impl FnMut()) {
    for _ in 0..50 {
        f();
    }
    let a = snap();
    let t = std::time::Instant::now();
    for _ in 0..n {
        f();
    }
    let dt = t.elapsed();
    let b = snap();
    println!(
        "| {name} | {:.0} | {:.1} | {:.1} |",
        (b.instrs - a.instrs) as f64 / n as f64,
        (b.cpu_ns - a.cpu_ns) as f64 / n as f64 / 1e3,
        dt.as_secs_f64() / n as f64 * 1e6
    );
}

fn main() {
    let mut e = Engine::new(ShaderSource::Metallib);
    let q = e.gpu.queue.clone();
    let tex = e.gpu.new_texture(MTLStorageMode::Shared, true);
    let tex2 = e.gpu.new_texture(MTLStorageMode::Private, true);
    let tex3 = e.gpu.new_texture(MTLStorageMode::Private, true);
    let n = 2000;
    println!("| operation (per iteration) | instr | CPU us | wall us |\n|---|---|---|---|");
    bench("snap() itself (proc_pid_rusage)", n, || {
        let _ = snap();
    });
    bench("commandBuffer + commit + waitUntilCompleted (empty)", n, || {
        let c = q.commandBuffer().unwrap();
        c.commit();
        c.waitUntilCompleted();
    });
    bench("commandBuffer + commit, no wait (empty)", n, || {
        let c = q.commandBuffer().unwrap();
        c.commit();
    });
    // tiny repair region 16x16 tile, 1 draw
    let mut r = Region::default();
    r.full = false;
    r.bbox = IRect::new(160, 160, 176, 176);
    r.rects = vec![r.bbox];
    e.scene.invalidate_all();
    e.scene.acc.clear();
    for (name, wait) in [("wait=completed", 0), ("wait=scheduled", 1), ("no wait (poll)", 2), ("completed handler", 3)] {
        bench(&format!("tiny repair (16x16, 1 draw): encode+commit, {name}"), n, || {
            let c = q.commandBuffer().unwrap();
            e.gpu.encode_repair(&c, &tex, &e.scene, &r, DamageMode::Bbox);
            match wait {
                3 => {
                    let b = RcBlock::new(|_c: NonNull<ProtocolObject<dyn MTLCommandBuffer>>| {});
                    unsafe { c.addCompletedHandler(RcBlock::as_ptr(&b)) };
                    c.commit();
                    c.waitUntilCompleted();
                }
                _ => {
                    c.commit();
                    match wait {
                        0 => c.waitUntilCompleted(),
                        1 => c.waitUntilScheduled(),
                        _ => {}
                    }
                }
            }
        });
        // drain
        let c = q.commandBuffer().unwrap();
        c.commit();
        c.waitUntilCompleted();
    }
    bench("tiny repair: UNRETAINED-refs cmdbuf, encode+commit+wait=scheduled", n, || {
        let c = q.commandBufferWithUnretainedReferences().unwrap();
        e.gpu.encode_repair(&c, &tex, &e.scene, &r, DamageMode::Bbox);
        c.commit();
        c.waitUntilScheduled();
    });
    bench("tiny repair: retained cmdbuf, encode+commit+wait=scheduled (same loop, for A/B)", n, || {
        let c = q.commandBuffer().unwrap();
        e.gpu.encode_repair(&c, &tex, &e.scene, &r, DamageMode::Bbox);
        c.commit();
        c.waitUntilScheduled();
    });
    bench("tiny repair: encode only, no commit (cmdbuf dropped via enqueue-less path is invalid, so: encode+commit+drain every 8)", n, || {
        let c = q.commandBuffer().unwrap();
        e.gpu.encode_repair(&c, &tex, &e.scene, &r, DamageMode::Bbox);
        c.commit();
    });
    {
        let c = q.commandBuffer().unwrap();
        c.commit();
        c.waitUntilCompleted();
    }
    e.scene.invalidate_all();
    let mut full = Region::full();
    full.set_full();
    e.scene.acc.clear();
    bench("full repaint (1470 inst, 1 draw): encode+commit+wait", 500, || {
        let c = q.commandBuffer().unwrap();
        e.gpu.encode_repair(&c, &tex, &e.scene, &full, DamageMode::Bbox);
        c.commit();
        c.waitUntilCompleted();
    });
    bench("whole-texture blit copy 1920x1440 (private->private) + wait", 500, || {
        let c = q.commandBuffer().unwrap();
        let b = c.blitCommandEncoder().unwrap();
        unsafe { b.copyFromTexture_toTexture(&tex2, &tex3) };
        b.endEncoding();
        c.commit();
        c.waitUntilCompleted();
    });
    // GPU time of a tiny repair vs full vs blit, measured by GPUStartTime/EndTime
    let gt = |label: &str, f: &dyn Fn(&Cmd)| {
        let mut v = vec![];
        for _ in 0..300 {
            let c = q.commandBuffer().unwrap();
            f(&c);
            c.commit();
            c.waitUntilCompleted();
            v.push((c.GPUEndTime() - c.GPUStartTime()) * 1e3);
        }
        println!("GPU ms (median of 300) {label}: {:.3}  p99 {:.3}", pct(&mut v.clone(), 0.5), pct(&mut v, 0.99));
    };
    gt("empty command buffer", &|_c| {});
    gt("tiny 16x16 repair", &|c| {
        e.gpu.encode_repair(c, &tex, &e.scene, &r, DamageMode::Bbox);
    });
    let mut r400 = Region::default();
    r400.bbox = IRect::new(368, 960, 1856, 1360);
    r400.rects = vec![r400.bbox];
    gt("400px-tall viewport repair (1488x400)", &|c| {
        e.gpu.encode_repair(c, &tex, &e.scene, &r400, DamageMode::Bbox);
    });
    gt("full repaint (DontCare load)", &|c| {
        e.gpu.encode_repair(c, &tex, &e.scene, &full, DamageMode::Bbox);
    });
    gt("whole-texture blit copy", &|c| {
        let b = c.blitCommandEncoder().unwrap();
        unsafe { b.copyFromTexture_toTexture(&tex2, &tex3) };
        b.endEncoding();
    });
}
