//! What creates the Metal driver's ~69 MiB first-submission block, and what releases it (lane memory, round 6).
//! One variant per process (the block is per process):
//!
//!   drvprobe <variant> [idle_ms=2500]
//!
//! Variants (each ends with one wait and then samples the footprint every 50 ms while idle):
//! * `blit`        default queue, a 4-byte fill of a private buffer
//! * `blit1`       a queue with maxCommandBufferCount = 1, the same fill
//! * `empty`       a committed command buffer with no encoder
//! * `enc`         a compute encoder with nothing dispatched
//! * `event`       a command buffer that only signals a shared event
//! * `shared`      the fill on a shared buffer instead of a private one
//! * `dropq`       `blit`, then the queue released
//! * `dropall`     `blit`, then the queue, the buffer and the device released
//! * `cpu`         no GPU work at all: an IOSurface written by the CPU (lock, write, unlock)
use bough_metal::perf::snap;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2_core_foundation::CFDictionary;
use objc2_foundation::{NSDictionary, NSNumber, NSString, ns_string};
use objc2_io_surface::IOSurfaceRef;
use objc2_metal::*;

fn mb() -> f64 {
    snap().footprint as f64 / 1048576.0
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let v = a.get(1).map(String::as_str).unwrap_or("blit").to_string();
    let idle: u64 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(2500);
    println!("{v}: start {:.1}", mb());
    if v == "cpu" {
        let keys: [&NSString; 4] = [ns_string!("IOSurfaceWidth"), ns_string!("IOSurfaceHeight"), ns_string!("IOSurfaceBytesPerElement"), ns_string!("IOSurfacePixelFormat")];
        let vals = [NSNumber::new_i32(960), NSNumber::new_i32(640), NSNumber::new_i32(4), NSNumber::new_u32(0x4247_5241)];
        let refs: Vec<&AnyObject> = vals.iter().map(|v| v.as_ref() as &AnyObject).collect();
        let dict: Retained<NSDictionary<NSString, AnyObject>> = NSDictionary::from_slices(&keys, &refs);
        // SAFETY: toll-free bridged; a well-formed property dictionary.
        let cf: &CFDictionary = unsafe { &*(Retained::as_ptr(&dict) as *const CFDictionary) };
        let io = unsafe { IOSurfaceRef::new(cf) }.unwrap();
        // SAFETY: locked for writing; the whole allocation.
        unsafe {
            io.lock(objc2_io_surface::IOSurfaceLockOptions::empty(), std::ptr::null_mut());
            std::ptr::write_bytes(io.base_address().as_ptr() as *mut u8, 0x80, io.alloc_size());
            io.unlock(objc2_io_surface::IOSurfaceLockOptions::empty(), std::ptr::null_mut());
        }
        println!("{v}: CPU-written surface ({} B) {:.1}", io.alloc_size(), mb());
        return;
    }
    let device = MTLCreateSystemDefaultDevice().unwrap();
    let mut queue = Some(if v == "blit1" { device.newCommandQueueWithMaxCommandBufferCount(1).unwrap() } else { device.newCommandQueue().unwrap() });
    let opts = if v == "shared" { MTLResourceOptions::StorageModeShared } else { MTLResourceOptions::StorageModePrivate };
    let mut buf = Some(device.newBufferWithLength_options(16, opts).unwrap());
    println!("{v}: device, queue, buffer {:.1}", mb());
    {
        let q = queue.as_ref().unwrap();
        let c = q.commandBuffer().unwrap();
        match v.as_str() {
            "empty" => {}
            "enc" => {
                let e = c.computeCommandEncoder().unwrap();
                e.endEncoding();
            }
            "event" => {
                let ev = device.newSharedEvent().unwrap();
                c.encodeSignalEvent_value(ProtocolObject::from_ref(&*ev), 1);
            }
            _ => {
                let e = c.blitCommandEncoder().unwrap();
                e.fillBuffer_range_value(buf.as_ref().unwrap(), objc2_foundation::NSRange::new(0, 4), 7);
                e.endEncoding();
            }
        }
        c.commit();
        c.waitUntilCompleted();
    }
    println!("{v}: after the submission {:.1}", mb());
    let mut device = Some(device);
    if v == "dropq2" {
        // a second queue that never submitted: is the block the device's (kept by any queue) or the submitter's?
        let q2 = device.as_ref().unwrap().newCommandQueue().unwrap();
        queue = None;
        println!("{v}: first queue released, a second alive {:.1}", mb());
        std::thread::sleep(std::time::Duration::from_millis(200));
        println!("{v}: 200 ms later {:.1}", mb());
        let c = q2.commandBuffer().unwrap();
        let e = c.blitCommandEncoder().unwrap();
        e.fillBuffer_range_value(buf.as_ref().unwrap(), objc2_foundation::NSRange::new(0, 4), 7);
        e.endEncoding();
        c.commit();
        c.waitUntilCompleted();
        println!("{v}: the second queue submitted {:.1}", mb());
        drop(q2);
        std::thread::sleep(std::time::Duration::from_millis(100));
        println!("{v}: second released, 100 ms later {:.1}", mb());
        // and a fresh queue's first frame: what re-creating costs
        let t = std::time::Instant::now();
        let q3 = device.as_ref().unwrap().newCommandQueue().unwrap();
        let t_new = t.elapsed();
        let c = q3.commandBuffer().unwrap();
        let e = c.blitCommandEncoder().unwrap();
        e.fillBuffer_range_value(buf.as_ref().unwrap(), objc2_foundation::NSRange::new(0, 4), 7);
        e.endEncoding();
        c.commit();
        c.waitUntilCompleted();
        println!("{v}: a new queue ({:.3} ms) and its first blit ({:.3} ms total) {:.1}", t_new.as_secs_f64() * 1e3, t.elapsed().as_secs_f64() * 1e3, mb());
        queue = Some(q3);
    }
    if v == "recreate" {
        // the idle protocol a presenter could follow: release the queue when idle, make a new one for the next
        // frame. What the next frame costs once the block is gone (wall time, process instructions).
        queue = None;
        std::thread::sleep(std::time::Duration::from_millis(300));
        println!("{v}: queue released, 300 ms later {:.1}", mb());
        for k in 0..3 {
            let (t, i0) = (std::time::Instant::now(), snap().instrs);
            let q = device.as_ref().unwrap().newCommandQueue().unwrap();
            let c = q.commandBuffer().unwrap();
            let e = c.blitCommandEncoder().unwrap();
            e.fillBuffer_range_value(buf.as_ref().unwrap(), objc2_foundation::NSRange::new(0, 4), 7);
            e.endEncoding();
            c.commit();
            c.waitUntilCompleted();
            println!("{v}: [{k}] new queue + first blit: {:.3} ms, {} instructions, footprint {:.1}", t.elapsed().as_secs_f64() * 1e3, snap().instrs - i0, mb());
            drop(c);
            drop(q);
            std::thread::sleep(std::time::Duration::from_millis(300));
            println!("{v}: [{k}] released again, 300 ms later {:.1}", mb());
        }
        // control: the same blit on a kept queue after 300 ms and after 2 s of idle
        let q = device.as_ref().unwrap().newCommandQueue().unwrap();
        for wait in [300u64, 2000] {
            let c = q.commandBuffer().unwrap();
            let e = c.blitCommandEncoder().unwrap();
            e.fillBuffer_range_value(buf.as_ref().unwrap(), objc2_foundation::NSRange::new(0, 4), 7);
            e.endEncoding();
            c.commit();
            c.waitUntilCompleted();
            drop(c);
            std::thread::sleep(std::time::Duration::from_millis(wait));
            let (t, i0) = (std::time::Instant::now(), snap().instrs);
            let c = q.commandBuffer().unwrap();
            let e = c.blitCommandEncoder().unwrap();
            e.fillBuffer_range_value(buf.as_ref().unwrap(), objc2_foundation::NSRange::new(0, 4), 7);
            e.endEncoding();
            c.commit();
            c.waitUntilCompleted();
            println!("{v}: kept queue, blit after {wait} ms idle: {:.3} ms, {} instructions, footprint before {:.1}", t.elapsed().as_secs_f64() * 1e3, snap().instrs - i0, mb());
        }
        queue = Some(q);
    }
    if v == "dropq" || v == "dropall" {
        queue = None;
        if v == "dropall" {
            buf = None;
            device = None;
        }
        println!("{v}: released {:.1}", mb());
    }
    let t0 = std::time::Instant::now();
    let mut last = mb();
    while t0.elapsed().as_millis() < idle as u128 {
        std::thread::sleep(std::time::Duration::from_millis(50));
        let m = mb();
        if (m - last).abs() > 1.0 {
            println!("{v}: idle {} ms: {:.1}", t0.elapsed().as_millis(), m);
            last = m;
        }
    }
    println!("{v}: idle {idle} ms end {:.1}", mb());
    drop((queue, buf, device));
}
