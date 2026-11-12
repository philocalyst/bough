//! Opus collaborator probes (round 6, not part of the presenter):
//! * `poll`   : a bounded persistent kernel spins on a word of a shared buffer; the CPU releases a "frame" by
//!              storing a sequence number (no Metal call). Round-trip latency CPU store -> GPU sees -> GPU ack ->
//!              CPU sees, per idle gap; process instructions per release.
//! * `event`  : the same round trip through today's mechanism (pre-armed batch, wait(ev) -> kernel -> signal).
//! * `life`   : how long a polling kernel may live (status / error at the end), spin rate, and whether it delays
//!              a nop command buffer on another queue.
//! * `io`     : IOSurfaceLock/Unlock and a CPU 32x32 tile write into an IOSurface; lossless-compressed formats.
//! * `atlas`  : replaceRegion of the whole 1024x512 R8 glyph atlas (what `sync_atlas` does on any dirty byte)
//!              vs one 16-row band; a linear texture over a shared buffer (no upload at all).
use bough_metal::perf::snap;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2_core_foundation::{CFDictionary, CFRetained};
use objc2_foundation::{ns_string, NSDictionary, NSNumber, NSString};
use objc2_io_surface::{IOSurfaceLockOptions, IOSurfaceRef};
use objc2_metal::*;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU32, Ordering::*};
use std::time::{Duration, Instant};

const MSL: &str = r#"
#include <metal_stdlib>
using namespace metal;
// ctl[0]: sequence (CPU), ctl[1]: iteration cap, ctl[32]: ack (GPU), ctl[64]: iterations run
kernel void poll(device atomic_uint* ctl [[buffer(0)]], uint tid [[thread_position_in_grid]]) {
    if (tid != 0) return;
    uint cap = atomic_load_explicit(&ctl[1], memory_order_relaxed);
    uint last = atomic_load_explicit(&ctl[0], memory_order_relaxed);
    uint i = 0;
    for (; i < cap; i++) {
        uint s = atomic_load_explicit(&ctl[0], memory_order_relaxed);
        if (s != last) {
            last = s;
            atomic_store_explicit(&ctl[32], s, memory_order_relaxed);
            if (s == 0xffffffffu) break;
        }
    }
    atomic_store_explicit(&ctl[64], i, memory_order_relaxed);
}
kernel void pollrmw(device atomic_uint* ctl [[buffer(0)]], uint tid [[thread_position_in_grid]]) {
    if (tid != 0) return;
    uint cap = atomic_load_explicit(&ctl[1], memory_order_relaxed);
    uint last = atomic_fetch_or_explicit(&ctl[0], 0u, memory_order_relaxed);
    uint i = 0;
    for (; i < cap; i++) {
        uint s = atomic_fetch_or_explicit(&ctl[0], 0u, memory_order_relaxed);
        if (s != last) {
            last = s;
            atomic_store_explicit(&ctl[32], s, memory_order_relaxed);
            if (s == 0xffffffffu) break;
        }
    }
    atomic_store_explicit(&ctl[64], i, memory_order_relaxed);
}
kernel void ack(device atomic_uint* ctl [[buffer(0)]], uint tid [[thread_position_in_grid]]) {
    if (tid == 0) atomic_store_explicit(&ctl[32], atomic_load_explicit(&ctl[0], memory_order_relaxed), memory_order_relaxed);
}
kernel void nop(device atomic_uint* ctl [[buffer(0)]], uint tid [[thread_position_in_grid]]) {
    if (tid == 0) atomic_fetch_add_explicit(&ctl[96], 1u, memory_order_relaxed);
}
kernel void wtex(texture2d<float, access::write> t [[texture(0)]], uint2 p [[thread_position_in_grid]]) {
    t.write(float4(float(p.x & 255u) / 255.0, float(p.y & 255u) / 255.0, 0.5, 1.0), p);
}
"#;

type Dev = Retained<ProtocolObject<dyn MTLDevice>>;
type Pso = Retained<ProtocolObject<dyn MTLComputePipelineState>>;
type Buf = Retained<ProtocolObject<dyn MTLBuffer>>;
type Queue = Retained<ProtocolObject<dyn MTLCommandQueue>>;

fn pso(dev: &Dev, lib: &ProtocolObject<dyn MTLLibrary>, name: &str) -> Pso {
    let f = lib.newFunctionWithName(&NSString::from_str(name)).expect("fn");
    dev.newComputePipelineStateWithFunction_error(&f).expect("pso")
}

fn one(q: &Queue, p: &Pso, b: &Buf) -> Retained<ProtocolObject<dyn MTLCommandBuffer>> {
    let c = q.commandBuffer().unwrap();
    let e = c.computeCommandEncoder().unwrap();
    e.setComputePipelineState(p);
    unsafe { e.setBuffer_offset_atIndex(Some(b), 0, 0) };
    let s = MTLSize { width: 1, height: 1, depth: 1 };
    e.dispatchThreadgroups_threadsPerThreadgroup(s, s);
    e.endEncoding();
    c
}

fn pct(v: &mut [f64], p: f64) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[((v.len() - 1) as f64 * p).round() as usize]
}

struct Ctl {
    seq: &'static AtomicU32,
    cap: &'static AtomicU32,
    ack: &'static AtomicU32,
    iters: &'static AtomicU32,
}
fn ctl(b: &Buf) -> Ctl {
    let p = b.contents().as_ptr() as *mut u32;
    // SAFETY: a 4 KiB shared buffer that lives for the process (leaked by the caller); u32-aligned words.
    unsafe {
        Ctl {
            seq: &*(p as *const AtomicU32),
            cap: &*(p.add(1) as *const AtomicU32),
            ack: &*(p.add(32) as *const AtomicU32),
            iters: &*(p.add(64) as *const AtomicU32),
        }
    }
}

const GAPS_US: [u64; 4] = [0, 100, 1000, 8000];

fn wait_gap(us: u64) {
    if us > 0 {
        std::thread::sleep(Duration::from_micros(us));
    }
}

/// Round trips: store seq (then `release` for the event path), spin on the ack. Returns per-gap latencies.
fn round_trips(name: &str, c: &Ctl, n: usize, mut release: impl FnMut(u32)) {
    let mut s = c.seq.load(Relaxed);
    for &gap in &GAPS_US {
        let mut lat = Vec::with_capacity(n);
        let mut fails = 0;
        for _ in 0..n {
            wait_gap(gap);
            s += 1;
            let t0 = Instant::now();
            c.seq.store(s, Release);
            release(s);
            loop {
                if c.ack.load(Acquire) == s {
                    lat.push(t0.elapsed().as_secs_f64() * 1e6);
                    break;
                }
                if t0.elapsed() > Duration::from_millis(200) {
                    fails += 1;
                    break;
                }
                std::hint::spin_loop();
            }
        }
        if lat.is_empty() {
            println!("{name:6} gap {gap:5} us: NO ACK in {n} tries");
            continue;
        }
        let (p50, p90, p99, mx) = (pct(&mut lat, 0.5), pct(&mut lat, 0.9), pct(&mut lat, 0.99), pct(&mut lat, 1.0));
        println!("{name:6} gap {gap:5} us: round trip p50 {p50:7.1} us  p90 {p90:7.1}  p99 {p99:7.1}  max {mx:8.1}  fails {fails}/{n}");
    }
}

/// Process instructions per release with no spin: release, sleep 1 ms, check the ack once.
fn instr_per_release(name: &str, c: &Ctl, n: usize, mut release: impl FnMut(u32)) {
    let mut s = c.seq.load(Relaxed);
    // baseline: the same loop without a release
    let a = snap();
    for _ in 0..n {
        std::thread::sleep(Duration::from_micros(1000));
        std::hint::black_box(c.ack.load(Acquire));
    }
    let b = snap();
    let base = (b.instrs - a.instrs) as f64 / n as f64;
    let a = snap();
    let mut missed = 0;
    for _ in 0..n {
        s += 1;
        c.seq.store(s, Release);
        release(s);
        std::thread::sleep(Duration::from_micros(1000));
        if c.ack.load(Acquire) != s {
            missed += 1;
        }
    }
    let b = snap();
    println!("{name:6} process instr per release (minus a 1-ms sleep loop's {base:.0}): {:.0}; acks missing after 1 ms: {missed}/{n}", (b.instrs - a.instrs) as f64 / n as f64 - base);
}

fn make_iosurface(w: usize, h: usize, fmt: u32, extra_compress: bool) -> Option<CFRetained<IOSurfaceRef>> {
    let mut keys: Vec<&NSString> = vec![ns_string!("IOSurfaceWidth"), ns_string!("IOSurfaceHeight"), ns_string!("IOSurfaceBytesPerElement"), ns_string!("IOSurfacePixelFormat")];
    let mut vals = vec![NSNumber::new_i32(w as i32), NSNumber::new_i32(h as i32), NSNumber::new_i32(4), NSNumber::new_u32(fmt)];
    if extra_compress {
        keys.push(ns_string!("IOSurfaceBytesPerRow"));
        vals.push(NSNumber::new_i32((w * 4).next_multiple_of(64) as i32));
    }
    let refs: Vec<&AnyObject> = vals.iter().map(|v| v.as_ref() as &AnyObject).collect();
    let dict: Retained<NSDictionary<NSString, AnyObject>> = NSDictionary::from_slices(&keys, &refs);
    // SAFETY: NSDictionary is toll-free bridged to CFDictionary.
    let cf: &CFDictionary = unsafe { &*(Retained::as_ptr(&dict) as *const CFDictionary) };
    // SAFETY: a well-formed property dictionary.
    unsafe { IOSurfaceRef::new(cf) }
}

fn main() {
    let modes: Vec<String> = std::env::args().skip(1).collect();
    let modes = if modes.is_empty() { vec!["poll".into(), "event".into(), "life".into(), "io".into(), "atlas".into()] } else { modes };
    let dev: Dev = MTLCreateSystemDefaultDevice().expect("device");
    let lib = dev.newLibraryWithSource_options_error(&NSString::from_str(MSL), None).expect("msl");
    let (p_poll, p_ack, p_nop, p_wtex) = (pso(&dev, &lib, if std::env::var_os("RMW").is_some() { "pollrmw" } else { "poll" }), pso(&dev, &lib, "ack"), pso(&dev, &lib, "nop"), pso(&dev, &lib, "wtex"));
    let cap_env: u32 = std::env::var("CAP").ok().and_then(|v| v.parse().ok()).unwrap_or(u32::MAX);
    let q: Queue = dev.newCommandQueue().unwrap();
    let q2: Queue = dev.newCommandQueue().unwrap();
    let opts = if std::env::var_os("WC").is_some() { MTLResourceOptions::StorageModeShared | MTLResourceOptions::CPUCacheModeWriteCombined } else { MTLResourceOptions::StorageModeShared };
    let buf: Buf = dev.newBufferWithLength_options(4096, opts).unwrap();
    let c = ctl(&buf);
    let n: usize = std::env::var("N").ok().and_then(|v| v.parse().ok()).unwrap_or(300);
    for m in &modes {
        match m.as_str() {
            "poll" => {
                c.seq.store(0, Relaxed);
                c.ack.store(0, Relaxed);
                c.cap.store(cap_env, Relaxed);
                let cmd = one(&q, &p_poll, &buf);
                cmd.commit();
                // started? (the first store may precede the kernel's first load: send until acked)
                let t0 = Instant::now();
                let mut s = 0;
                loop {
                    s += 1;
                    c.seq.store(s, Release);
                    std::thread::sleep(Duration::from_micros(200));
                    if c.ack.load(Acquire) == s || t0.elapsed() > Duration::from_secs(3) {
                        break;
                    }
                }
                println!("poll   first ack after {:.2} ms (kernel start + first observation), status {:?}", t0.elapsed().as_secs_f64() * 1e3, cmd.status());
                round_trips("poll", &c, n, |_| {});
                instr_per_release("poll", &c, n, |_| {});
                c.seq.store(u32::MAX, Release);
                cmd.waitUntilCompleted();
                println!("poll   kernel ended: status {:?} error {:?} gpu {:.3} s iterations {}", cmd.status(), cmd.error(), cmd.GPUEndTime() - cmd.GPUStartTime(), c.iters.load(Relaxed));
            }
            "event" => {
                c.seq.store(0, Relaxed);
                c.ack.store(0, Relaxed);
                let ev = dev.newSharedEvent().unwrap();
                let mut armed = 0u64;
                let mut released = 0u64;
                let mut arm = |armed: &mut u64| {
                    let cb = q.commandBufferWithUnretainedReferences().unwrap();
                    for f in *armed + 1..=*armed + 16 {
                        cb.encodeWaitForEvent_value(ProtocolObject::from_ref(&*ev), 2 * f - 1);
                        let e = cb.computeCommandEncoder().unwrap();
                        e.setComputePipelineState(&p_ack);
                        unsafe { e.setBuffer_offset_atIndex(Some(&buf), 0, 0) };
                        let s = MTLSize { width: 1, height: 1, depth: 1 };
                        e.dispatchThreadgroups_threadsPerThreadgroup(s, s);
                        e.endEncoding();
                        cb.encodeSignalEvent_value(ProtocolObject::from_ref(&*ev), 2 * f);
                    }
                    cb.commit();
                    *armed += 16;
                };
                arm(&mut armed);
                let mut rel = |_s: u32| {
                    let f = released + 1;
                    if f > 1 && ev.signaledValue() < 2 * (f - 1) {
                        ev.waitUntilSignaledValue_timeoutMS(2 * (f - 1), 5000);
                    }
                    ev.setSignaledValue(2 * f - 1);
                    released = f;
                    if armed < f + 8 {
                        arm(&mut armed);
                    }
                };
                round_trips("event", &c, n, &mut rel);
                instr_per_release("event", &c, n, &mut rel);
            }
            "life" => {
                // spin rate: a cap and no CPU updates
                c.seq.store(7, Relaxed);
                c.ack.store(0, Relaxed);
                for cap in [1u32 << 16, 1 << 20, 1 << 22] {
                    c.cap.store(cap, Relaxed);
                    let cmd = one(&q, &p_poll, &buf);
                    cmd.commit();
                    cmd.waitUntilCompleted();
                    let g = cmd.GPUEndTime() - cmd.GPUStartTime();
                    println!("life   cap {cap}: gpu {:.3} ms, {:.1} ns per poll iteration, status {:?}", g * 1e3, g * 1e9 / cap as f64, cmd.status());
                }
                // a nop on another queue, alone
                let mut alone = Vec::new();
                for _ in 0..50 {
                    let t = Instant::now();
                    let cb = one(&q2, &p_nop, &buf);
                    cb.commit();
                    cb.waitUntilCompleted();
                    alone.push(t.elapsed().as_secs_f64() * 1e6);
                    std::thread::sleep(Duration::from_millis(2));
                }
                // long-lived pollers
                let secs: Vec<f64> = std::env::var("LIFE").ok().map(|s| s.split(',').filter_map(|x| x.parse().ok()).collect()).unwrap_or(vec![1.0, 4.0, 12.0]);
                for d in secs {
                    c.cap.store(cap_env, Relaxed);
                    let cmd = one(&q, &p_poll, &buf);
                    cmd.commit();
                    let t0 = Instant::now();
                    let mut s = 100u32;
                    let mut beside = Vec::new();
                    let mut lost = 0;
                    while t0.elapsed().as_secs_f64() < d {
                        s += 1;
                        c.seq.store(s, Release);
                        std::thread::sleep(Duration::from_millis(8));
                        if c.ack.load(Acquire) != s {
                            lost += 1;
                        }
                        if beside.len() < 50 && t0.elapsed().as_secs_f64() > 0.2 {
                            let t = Instant::now();
                            let cb = one(&q2, &p_nop, &buf);
                            cb.commit();
                            cb.waitUntilCompleted();
                            beside.push(t.elapsed().as_secs_f64() * 1e6);
                        }
                        if cmd.status() == MTLCommandBufferStatus::Error || cmd.status() == MTLCommandBufferStatus::Completed {
                            break;
                        }
                    }
                    let lived = t0.elapsed().as_secs_f64();
                    c.seq.store(u32::MAX, Release);
                    cmd.waitUntilCompleted();
                    println!(
                        "life   target {d:4.1} s: lived {lived:.2} s, status {:?}, error {:?}, gpu {:.2} s, iterations {}, frames not acked within 8 ms {lost}",
                        cmd.status(),
                        cmd.error().map(|e| e.localizedDescription().to_string()),
                        cmd.GPUEndTime() - cmd.GPUStartTime(),
                        c.iters.load(Relaxed)
                    );
                    if !beside.is_empty() {
                        println!("life   nop on a second queue commit->completed: alone p50 {:.0} us p90 {:.0}; beside the poller p50 {:.0} us p90 {:.0}", pct(&mut alone.clone(), 0.5), pct(&mut alone.clone(), 0.9), pct(&mut beside.clone(), 0.5), pct(&mut beside, 0.9));
                    }
                }
            }
            "io" => {
                let (w, h) = (2048usize, 1408usize);
                let io = make_iosurface(w, h, 0x4247_5241, false).unwrap();
                // touch it once
                unsafe {
                    io.lock(IOSurfaceLockOptions(0), std::ptr::null_mut());
                    io.unlock(IOSurfaceLockOptions(0), std::ptr::null_mut());
                }
                let k = 20000;
                let a = snap();
                let t = Instant::now();
                for _ in 0..k {
                    unsafe {
                        io.lock(IOSurfaceLockOptions(0), std::ptr::null_mut());
                        io.unlock(IOSurfaceLockOptions(0), std::ptr::null_mut());
                    }
                }
                let b = snap();
                println!("io     IOSurfaceLock+Unlock (read-write): {:.0} instr, {:.2} us per pair", (b.instrs - a.instrs) as f64 / k as f64, t.elapsed().as_secs_f64() * 1e6 / k as f64);
                let a = snap();
                let t = Instant::now();
                for _ in 0..k {
                    unsafe {
                        io.lock(IOSurfaceLockOptions::AvoidSync, std::ptr::null_mut());
                        io.unlock(IOSurfaceLockOptions::AvoidSync, std::ptr::null_mut());
                    }
                }
                let b = snap();
                println!("io     IOSurfaceLock+Unlock (AvoidSync): {:.0} instr, {:.2} us per pair", (b.instrs - a.instrs) as f64 / k as f64, t.elapsed().as_secs_f64() * 1e6 / k as f64);
                // a CPU tile write (4 KiB, 32 rows) into the surface, lock included
                let bpr = io.bytes_per_row();
                let tile = [0x5au8; 128];
                let a = snap();
                let t = Instant::now();
                for i in 0..k {
                    unsafe {
                        io.lock(IOSurfaceLockOptions::AvoidSync, std::ptr::null_mut());
                        let base = io.base_address().as_ptr() as *mut u8;
                        let (tx, ty) = (i % 64, (i / 64) % 44);
                        for y in 0..32 {
                            std::ptr::copy_nonoverlapping(tile.as_ptr(), base.add((ty * 32 + y) * bpr + tx * 128), 128);
                        }
                        io.unlock(IOSurfaceLockOptions::AvoidSync, std::ptr::null_mut());
                    }
                }
                let b = snap();
                println!("io     lock + one 32x32 tile memcpy + unlock: {:.0} instr, {:.2} us", (b.instrs - a.instrs) as f64 / k as f64, t.elapsed().as_secs_f64() * 1e6 / k as f64);
                println!("io     plain BGRA {w}x{h}: alloc {} bytes", io.alloc_size());
                // lossless / lossy compressed pixel formats (CoreVideo's '&BGA', '&b38'... as IOSurface formats)
                for (name, fmt) in [("&BGA lossless BGRA", u32::from_be_bytes(*b"&BGA")), ("-BGA lossy BGRA", u32::from_be_bytes(*b"-BGA")), ("&8v0", u32::from_be_bytes(*b"&8v0"))] {
                    match make_iosurface(w, h, fmt, false) {
                        None => println!("io     {name}: IOSurface creation refused"),
                        Some(cs) => {
                            let d = unsafe { MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(MTLPixelFormat::BGRA8Unorm, w, h, false) };
                            d.setUsage(MTLTextureUsage::ShaderRead | MTLTextureUsage::ShaderWrite);
                            d.setStorageMode(MTLStorageMode::Shared);
                            let t = dev.newTextureWithDescriptor_iosurface_plane(&d, &cs, 0);
                            print!("io     {name}: IOSurface alloc {} bytes, bpr {}, planes {}; Metal BGRA8 ShaderWrite texture: {}", cs.alloc_size(), cs.bytes_per_row(), cs.plane_count(), if t.is_some() { "created" } else { "refused" });
                            if let Some(t) = t {
                                let cb = q2.commandBuffer().unwrap();
                                let e = cb.computeCommandEncoder().unwrap();
                                e.setComputePipelineState(&p_wtex);
                                unsafe { e.setTexture_atIndex(Some(&t), 0) };
                                e.dispatchThreads_threadsPerThreadgroup(MTLSize { width: w, height: h, depth: 1 }, MTLSize { width: 16, height: 16, depth: 1 });
                                e.endEncoding();
                                cb.commit();
                                cb.waitUntilCompleted();
                                print!(", compute write: {:?} {:?}", cb.status(), cb.error().map(|e| e.localizedDescription().to_string()));
                            }
                            println!();
                        }
                    }
                }
            }
            "atlas" => {
                let d = unsafe { MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(MTLPixelFormat::R8Unorm, 1024, 512, false) };
                let t = dev.newTextureWithDescriptor(&d).unwrap();
                println!("atlas  default storage mode {:?}", t.storageMode());
                let px = vec![0x33u8; 1024 * 512];
                for (name, rows) in [("whole atlas 1024x512", 512usize), ("one 16-row band", 16)] {
                    let k = 400;
                    let a = snap();
                    let tm = Instant::now();
                    for i in 0..k {
                        let y0 = if rows == 512 { 0 } else { (i * 16) % 512 };
                        unsafe {
                            t.replaceRegion_mipmapLevel_withBytes_bytesPerRow(
                                MTLRegion { origin: MTLOrigin { x: 0, y: y0, z: 0 }, size: MTLSize { width: 1024, height: rows, depth: 1 } },
                                0,
                                NonNull::new(px.as_ptr().add(y0 * 1024) as *mut _).unwrap(),
                                1024,
                            )
                        };
                    }
                    let b = snap();
                    println!("atlas  replaceRegion {name}: {:.0} instr, {:.1} us per call", (b.instrs - a.instrs) as f64 / k as f64, tm.elapsed().as_secs_f64() * 1e6 / k as f64);
                }
                let align = dev.minimumLinearTextureAlignmentForPixelFormat(MTLPixelFormat::R8Unorm);
                let lb = dev.newBufferWithLength_options(1024 * 512, MTLResourceOptions::StorageModeShared).unwrap();
                let ld = unsafe { MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(MTLPixelFormat::R8Unorm, 1024, 512, false) };
                ld.setStorageMode(MTLStorageMode::Shared);
                let lt = unsafe { lb.newTextureWithDescriptor_offset_bytesPerRow(&ld, 0, 1024) };
                println!("atlas  linear R8 texture over a shared buffer (CPU writes glyphs in place, no upload): {} (alignment {align})", if lt.is_some() { "created" } else { "refused" });
            }
            _ => println!("unknown mode {m}"),
        }
    }
    std::mem::forget(buf);
}
