//! What a frame costs on the CPU between "the slot is written" and "the pixels exist", call by call, and what of
//! it Metal makes irreducible (round 6, lane gpu). Every mode releases one frame of the smallest real GPU work
//! (the one-thread `fl_nop` kernel bumping a counter in shared memory), so the numbers are the mechanism's, not a
//! kernel's.
//!
//! `frameprobe [frames=2000] [gap_us=0] [modes,...]`
//!
//! `gap_us`: sleep between frames (0: back to back; 2000: a UI cadence at which the GPU is done before the next
//! frame, so nothing ever blocks).
//!
//! Modes (K = frames encoded ahead per command buffer):
//! * `cmd`            a command buffer + encoder + commit per frame;
//! * `ev<K>`          the presenter's stream: K x (wait ev 2f-1, encoder, signal ev 2f), released by
//!                    `setSignaledValue`, the previous frame awaited on the event;
//! * `ev<K>-mem`      the same without the per-frame signal: completion is the counter word the kernel writes
//!                    (read from shared memory; awaited, when it must be, by yielding);
//! * `ev<K>-norel`    frames are armed but never released or awaited: the arming alone (encode + commit);
//! * `none`           the loop with no Metal call (the probe's own cost).
//!
//! Printed per mode: process instructions per frame, the calling thread's split per call (thread_selfcounts,
//! its own cost subtracted), every other thread's share, and the release -> done latency (p50 / p99 / max, by
//! spinning on the counter word in a separate pass, so the spin is not in the instruction counts).
use bough_metal::perf::{snap, tinstr, tinstr_overhead};
use bough_metal::threads::{instr_diff, instr_snapshot};
use objc2::runtime::ProtocolObject;
use objc2_metal::*;
use std::time::{Duration, Instant};

struct Split {
    names: Vec<&'static str>,
    acc: Vec<u64>,
    calls: Vec<u64>,
    ovh: u64,
}
impl Split {
    fn new(names: &[&'static str]) -> Split {
        Split {
            names: names.to_vec(),
            acc: vec![0; names.len()],
            calls: vec![0; names.len()],
            ovh: tinstr_overhead(),
        }
    }
    #[inline]
    fn time<R>(&mut self, i: usize, f: impl FnOnce() -> R) -> R {
        let a = tinstr();
        let r = f();
        self.acc[i] += (tinstr() - a).saturating_sub(self.ovh);
        self.calls[i] += 1;
        r
    }
    fn report(&self, frames: usize) -> String {
        self.names
            .iter()
            .zip(&self.acc)
            .zip(&self.calls)
            .map(|((n, a), c)| {
                format!(
                    "{n} {:.0} ({:.2}/frame)",
                    *a as f64 / frames as f64,
                    *c as f64 / frames as f64
                )
            })
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn pctl(v: &mut [f64], p: f64) -> f64 {
    v.sort_by(f64::total_cmp);
    v[((v.len() - 1) as f64 * p).round() as usize]
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let frames: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(2000);
    let gap = Duration::from_micros(args.get(2).and_then(|s| s.parse().ok()).unwrap_or(0));
    let modes: Vec<String> = args
        .get(3)
        .map(|s| s.split(',').map(String::from).collect())
        .unwrap_or_else(|| {
            [
                "none",
                "cmd",
                "ev16",
                "ev16-mem",
                "ev64",
                "ev64-mem",
                "ev16-norel",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect()
        });
    let dev = MTLCreateSystemDefaultDevice().expect("no Metal device");
    let lib = bough_metal::gpu::load_library(&dev);
    let f = lib
        .newFunctionWithName(objc2_foundation::ns_string!("fl_nop"))
        .unwrap();
    let pso = dev.newComputePipelineStateWithFunction_error(&f).unwrap();
    let one = MTLSize {
        width: 1,
        height: 1,
        depth: 1,
    };
    println!(
        "load {:.0}; frames {frames}, gap {} us; thread_selfcounts overhead {} instr (subtracted)",
        load(),
        gap.as_micros(),
        tinstr_overhead()
    );
    for m in &modes {
        // a fresh queue, event and counter per mode
        let q = dev.newCommandQueue().unwrap();
        let buf = dev
            .newBufferWithLength_options(64, MTLResourceOptions::StorageModeShared)
            .unwrap();
        let word = buf.contents().as_ptr() as *const u32;
        // SAFETY: the buffer holds 64 bytes of shared storage; the kernel only adds to its first word.
        let done = || unsafe { std::ptr::read_volatile(word) } as u64;
        let ev = dev.newSharedEvent().unwrap();
        let enc = |sp: &mut Split, c: &ProtocolObject<dyn MTLCommandBuffer>| {
            let e = sp.time(5, || c.computeCommandEncoder().unwrap());
            sp.time(6, || e.setComputePipelineState(&pso));
            sp.time(7, || unsafe {
                e.setBuffer_offset_atIndex(Some(&buf), 0, 0)
            });
            sp.time(8, || e.dispatchThreadgroups_threadsPerThreadgroup(one, one));
            sp.time(9, || e.endEncoding());
        };
        let mem = m.ends_with("-mem");
        let norel = m.ends_with("-norel");
        let k: u64 = m
            .trim_start_matches("ev")
            .split('-')
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let mut sp = Split::new(&[
            "wait",
            "release",
            "cmdbuf",
            "encodeWait",
            "commit",
            "newEncoder",
            "setPSO",
            "setBuffer",
            "dispatch",
            "endEncoding",
            "encodeSignal",
        ]);
        let mut armed = 0u64;
        let mut released = 0u64;
        let mut last: Option<bough_metal::gpu::Cmd> = None;
        // one frame; `lat`: spin until it is done and return the release -> done time
        let mut frame = |sp: &mut Split, lat: bool| -> f64 {
            let mut t = 0.0;
            match m.as_str() {
                "none" => {}
                "cmd" => {
                    sp.time(0, || {
                        if let Some(c) = last.take() {
                            c.waitUntilCompleted();
                        }
                    });
                    let c = sp.time(2, || q.commandBufferWithUnretainedReferences().unwrap());
                    enc(sp, &c);
                    let t0 = Instant::now();
                    sp.time(4, || c.commit());
                    released += 1;
                    if lat {
                        while done() < released {
                            std::hint::spin_loop();
                        }
                        t = t0.elapsed().as_secs_f64() * 1e6;
                    }
                    last = Some(c);
                }
                _ => {
                    let f = released + 1;
                    if !norel {
                        sp.time(0, || {
                            if mem {
                                // the previous frame's pixels exist when the kernel's word says so
                                while done() < f - 1 {
                                    std::thread::yield_now();
                                }
                            } else if f > 1 && ev.signaledValue() < 2 * (f - 1) {
                                ev.waitUntilSignaledValue_timeoutMS(2 * (f - 1), 5000);
                            }
                        });
                        let t0 = Instant::now();
                        sp.time(1, || ev.setSignaledValue(2 * f - 1));
                        if lat {
                            while done() < f {
                                std::hint::spin_loop();
                            }
                            t = t0.elapsed().as_secs_f64() * 1e6;
                        }
                    }
                    released = f;
                    if armed < f + k / 2 {
                        let c = sp.time(2, || q.commandBufferWithUnretainedReferences().unwrap());
                        for g in armed + 1..=armed + k {
                            sp.time(3, || {
                                c.encodeWaitForEvent_value(
                                    ProtocolObject::from_ref(&*ev),
                                    2 * g - 1,
                                )
                            });
                            enc(sp, &c);
                            if !mem {
                                sp.time(10, || {
                                    c.encodeSignalEvent_value(ProtocolObject::from_ref(&*ev), 2 * g)
                                });
                            }
                        }
                        sp.time(4, || c.commit());
                        armed += k;
                    }
                }
            }
            t
        };
        for _ in 0..200 {
            frame(&mut sp, false);
            std::thread::sleep(gap);
        }
        sp.acc.fill(0);
        sp.calls.fill(0);
        let (a, ta) = (snap(), instr_snapshot());
        let t0 = Instant::now();
        for _ in 0..frames {
            frame(&mut sp, false);
            if !gap.is_zero() {
                std::thread::sleep(gap);
            }
        }
        let wall = t0.elapsed().as_secs_f64() / frames as f64 * 1e6;
        let (b, tb) = (snap(), instr_snapshot());
        // the split's own probes: 2 thread_selfcounts per timed call
        let probes: u64 = sp.calls.iter().sum::<u64>() * 2 * sp.ovh;
        let total = (b.instrs - a.instrs).saturating_sub(probes) as f64 / frames as f64;
        let split = sp.report(frames);
        let mut lat: Vec<f64> = Vec::new();
        if m != "none" && !norel {
            for _ in 0..frames.min(1000) {
                lat.push(frame(&mut sp, true));
                if !gap.is_zero() {
                    std::thread::sleep(gap);
                }
            }
        }
        let l = if lat.is_empty() {
            String::new()
        } else {
            format!(
                " | release->done us p50 {:.0} p99 {:.0} max {:.0}",
                pctl(&mut lat, 0.5),
                pctl(&mut lat, 0.99),
                pctl(&mut lat, 1.0)
            )
        };
        println!(
            "{m:12} {total:8.0} instr/frame  wall {wall:7.1} us{l}\n             this thread: {}\n             threads: {}",
            split,
            instr_diff(&ta, &tb, frames as u64, 1000.0)
        );
        // drain what is armed so the queue can be dropped
        if k > 0 {
            for g in released + 1..=armed {
                ev.setSignaledValue(2 * g - 1);
            }
            while done() < armed && !norel {
                std::thread::yield_now();
            }
        }
        if let Some(c) = last.take() {
            c.waitUntilCompleted();
        }
    }
}

fn load() -> f64 {
    let mut l = [0f64; 3];
    // SAFETY: getloadavg fills up to 3 doubles.
    unsafe { libc::getloadavg(l.as_mut_ptr(), 3) };
    l[0]
}
