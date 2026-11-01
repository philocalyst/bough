//! The per-frame floor of the fused presenter, headless: process-wide instructions per frame for each way of
//! submitting a frame and learning that it finished, with the per-thread split (who pays).
//! `fprobe [frames=2000] [modes,...]`; one damaged 32-px tile per frame on a 512x512 window, empty scene.
use bough_metal::fused::{Fused, Layout};
use bough_metal::perf::snap;
use bough_metal::threads::{instr_diff, instr_snapshot};
use objc2_metal::*;

fn fused() -> Fused {
    let (w, h) = (512u32, 512u32);
    let sb = Layout::slot_bytes(w, h) as u32;
    let len = (256 + 3 * sb as usize + 65536).next_multiple_of(16384);
    let mut f = Fused::new(w as usize, h as usize, 1.0, len, 4, 1024);
    let base = 4096u32;
    let lay = Layout { w, h, slot: [base, base + sb, base + 2 * sb], ..Default::default() };
    f.set_layout(lay);
    f
}

fn run(name: &str, frames: usize, mut f: impl FnMut(usize)) {
    for i in 0..100 {
        f(i);
    }
    let (a, ta) = (snap(), instr_snapshot());
    let t = std::time::Instant::now();
    for i in 0..frames {
        f(100 + i);
    }
    let dt = t.elapsed();
    let (b, tb) = (snap(), instr_snapshot());
    let n = frames as f64;
    println!(
        "{name:44} {:8.0} instr/frame  cpu {:6.1} us  wall {:6.1} us | {}",
        (b.instrs - a.instrs) as f64 / n,
        (b.cpu_ns - a.cpu_ns) as f64 / n / 1e3,
        dt.as_secs_f64() / n * 1e6,
        instr_diff(&ta, &tb, frames as u64, 2000.0)
    );
}

/// Raw kicks with the one-thread `fl_nop` kernel:
/// * `kick-cmd`: a command buffer + encoder + dispatch per frame, committed; the next frame waits for it;
/// * `kick-b8-enc`: per 8 frames one command buffer of 8 encoders (one dispatch each), no events;
/// * `kick-b8-1enc`: per 8 frames one command buffer, ONE encoder with 8 dispatches;
/// * `kick-b8-ev`: the stream: 8 x (wait ev 2f-1, encoder, signal 2f) pre-armed, released per frame,
///   waited per frame (`-nosig`: only the batch's last frame signals; `-nowait`: never waited per frame).
fn kick(g: &Fused, s: &str, frames: usize) {
    use bough_metal::gpu::METALLIB;
    use dispatch2::DispatchData;
    use objc2::runtime::ProtocolObject;
    let dev = &g.device;
    let q = dev.newCommandQueue().unwrap();
    let lib = dev.newLibraryWithData_error(&DispatchData::from_static_bytes(METALLIB)).unwrap();
    let f = lib.newFunctionWithName(objc2_foundation::ns_string!("fl_nop")).unwrap();
    let pso = dev.newComputePipelineStateWithFunction_error(&f).unwrap();
    let buf = dev.newBufferWithLength_options(64, MTLResourceOptions::StorageModeShared).unwrap();
    let one = MTLSize { width: 1, height: 1, depth: 1 };
    let tex = s.contains("tex");
    let texs: Vec<*const ProtocolObject<dyn MTLTexture>> = g.surfs.iter().map(|s| objc2::rc::Retained::as_ptr(&s.tex)).collect();
    let enc_nop = |c: &ProtocolObject<dyn MTLCommandBuffer>, n: usize| {
        let e = c.computeCommandEncoder().unwrap();
        e.setComputePipelineState(&pso);
        unsafe { e.setBuffer_offset_atIndex(Some(&buf), 0, 0) };
        if tex {
            unsafe { e.setTextures_withRange(std::ptr::NonNull::new(texs.as_ptr() as *mut _).unwrap(), objc2_foundation::NSRange::new(1, texs.len())) };
        }
        for _ in 0..n {
            e.dispatchThreadgroups_threadsPerThreadgroup(one, one);
        }
        e.endEncoding();
    };
    match s {
        "kick-cmd" => {
            let mut last: Option<bough_metal::gpu::Cmd> = None;
            run(s, frames, |_| {
                if let Some(c) = last.take() {
                    c.waitUntilCompleted();
                }
                let c = q.commandBuffer().unwrap();
                enc_nop(&c, 1);
                c.commit();
                last = Some(c);
            });
        }
        "kick-b8-enc" | "kick-b8-1enc" => {
            let per_enc = s == "kick-b8-enc";
            let mut last: Option<bough_metal::gpu::Cmd> = None;
            run(s, frames, |i| {
                if i % 8 == 0 {
                    if let Some(c) = last.take() {
                        c.waitUntilCompleted();
                    }
                    let c = q.commandBuffer().unwrap();
                    if per_enc {
                        for _ in 0..8 {
                            enc_nop(&c, 1);
                        }
                    } else {
                        enc_nop(&c, 8);
                    }
                    c.commit();
                    last = Some(c);
                }
            });
        }
        _ => {
            let nosig = s.contains("nosig");
            let nowait = s.contains("nowait") || nosig;
            let ev = dev.newSharedEvent().unwrap();
            let mut armed = 0u64;
            let mut released = 0u64;
            let arm = |armed: &mut u64| {
                let c = q.commandBuffer().unwrap();
                for f in *armed + 1..=*armed + 8 {
                    c.encodeWaitForEvent_value(ProtocolObject::from_ref(&*ev), 2 * f - 1);
                    enc_nop(&c, 1);
                    if !nosig || f % 8 == 0 {
                        c.encodeSignalEvent_value(ProtocolObject::from_ref(&*ev), 2 * f);
                    }
                }
                c.commit();
                *armed += 8;
            };
            arm(&mut armed);
            run(s, frames, |_| {
                let f = released + 1;
                if nowait {
                    // before re-using slot f % 2, frame f - 2 must be done (a batch boundary waits for its signal)
                    if f % 8 == 1 && f > 8 && ev.signaledValue() < 2 * (f - 1) {
                        ev.waitUntilSignaledValue_timeoutMS(2 * (f - 1), 5000);
                    }
                } else if f > 1 && ev.signaledValue() < 2 * (f - 1) {
                    ev.waitUntilSignaledValue_timeoutMS(2 * (f - 1), 5000);
                }
                ev.setSignaledValue(2 * f - 1);
                released = f;
                if armed < f + 4 {
                    arm(&mut armed);
                }
            });
            // drain
            for f in released + 1..=armed {
                ev.setSignaledValue(2 * f - 1);
            }
            ev.waitUntilSignaledValue_timeoutMS(2 * armed, 5000);
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let frames: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(2000);
    let modes: Vec<String> = args.get(2).map(|s| s.split(',').map(String::from).collect()).unwrap_or_else(|| {
        ["cmd", "cmd-wait", "stream8", "stream32", "stream8-wait", "ev-set", "ev-read", "ev-wait-done", "encode-only"].iter().map(|s| s.to_string()).collect()
    });
    let mut rows = vec![0u128; 16];
    rows[3] = 1 << 2;
    for m in &modes {
        let mut g = fused();
        match m.as_str() {
            // today's RENDER=1 path: a command buffer per frame, the next frame's sync waits
            "cmd" => run(m, frames, |i| {
                g.draw(i % 2, &rows, false, 0);
            }),
            // the windowed path: commit, then wait in the same turn (publish after completion)
            "cmd-wait" => run(m, frames, |i| {
                g.draw(i % 2, &rows, false, 0);
                g.sync();
            }),
            s if s.starts_with("stream") => {
                let wait = s.ends_with("-wait");
                let k: usize = s.trim_start_matches("stream").trim_end_matches("-wait").parse().unwrap();
                g.set_batch(k);
                run(m, frames, |i| {
                    g.draw(i % 2, &rows, false, 0);
                    if wait {
                        g.sync();
                    }
                })
            }
            "ev-set" => {
                let ev = g.device.newSharedEvent().unwrap();
                run(m, frames, |i| ev.setSignaledValue(i as u64 + 1))
            }
            "ev-read" => {
                let ev = g.device.newSharedEvent().unwrap();
                ev.setSignaledValue(5);
                let mut s = 0u64;
                run(m, frames, |_| s = s.wrapping_add(ev.signaledValue()));
                std::hint::black_box(s);
            }
            "ev-wait-done" => {
                let ev = g.device.newSharedEvent().unwrap();
                ev.setSignaledValue(u64::MAX / 2);
                run(m, frames, |i| {
                    ev.waitUntilSignaledValue_timeoutMS(i as u64, 100);
                })
            }
            "encode-only" => {
                let q = g.device.newCommandQueue().unwrap();
                run(m, frames, |_| {
                    let c = q.commandBuffer().unwrap();
                    let e = c.computeCommandEncoder().unwrap();
                    e.endEncoding();
                })
            }
            "cb-only" => {
                let q = g.device.newCommandQueue().unwrap();
                run(m, frames, |_| {
                    let c = q.commandBuffer().unwrap();
                    std::hint::black_box(&c);
                })
            }
            s if s.starts_with("kick") => kick(&g, s, frames),
            _ => eprintln!("unknown mode {m}"),
        }
    }
}
