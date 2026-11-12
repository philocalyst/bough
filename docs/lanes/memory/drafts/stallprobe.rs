//! Does a pre-armed command buffer survive a long wait between two of its frames? (lane memory, round 6: two of
//! nine windowed heavy runs died with kIOGPUCommandBufferCallbackErrorTimeout about a second after `footprint` /
//! `vmmap` had suspended the process.) The presenter's stream is K frames in one command buffer, frame f waiting on
//! a shared event (`ev >= 2f - 1`) and signalling `2f`. This probe arms such a buffer, releases its first frame,
//! waits for it, then lets the buffer sit mid-way (waiting on frame 2's event) for D seconds before releasing frame
//! 2; one process per D:
//!
//!   stallprobe <D seconds> [busy]
//!
//! `busy`: the process spins on the CPU during the wait instead of sleeping (a stalled main thread rather than an
//! idle one). Prints whether frame 2 completed and the command buffer's status and error.
use objc2::runtime::ProtocolObject;
use objc2_metal::*;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let d: f64 = a.get(1).and_then(|s| s.parse().ok()).unwrap_or(3.0);
    let busy = a.get(2).is_some_and(|s| s == "busy");
    let device = MTLCreateSystemDefaultDevice().unwrap();
    let queue = device.newCommandQueue().unwrap();
    let buf = device.newBufferWithLength_options(64, MTLResourceOptions::StorageModePrivate).unwrap();
    let ev = device.newSharedEvent().unwrap();
    let c = queue.commandBufferWithUnretainedReferences().unwrap();
    for f in 1..=4u64 {
        c.encodeWaitForEvent_value(ProtocolObject::from_ref(&*ev), 2 * f - 1);
        let e = c.blitCommandEncoder().unwrap();
        e.fillBuffer_range_value(&buf, objc2_foundation::NSRange::new(0, 4), f as u8);
        e.endEncoding();
        c.encodeSignalEvent_value(ProtocolObject::from_ref(&*ev), 2 * f);
    }
    c.commit();
    ev.setSignaledValue(1);
    let ok1 = ev.waitUntilSignaledValue_timeoutMS(2, 5000);
    let t = std::time::Instant::now();
    if busy {
        let mut x = 0u64;
        while t.elapsed().as_secs_f64() < d {
            x = x.wrapping_mul(6364136223846793005).wrapping_add(1);
        }
        std::hint::black_box(x);
    } else {
        std::thread::sleep(std::time::Duration::from_secs_f64(d));
    }
    ev.setSignaledValue(3);
    let ok2 = ev.waitUntilSignaledValue_timeoutMS(4, 5000);
    ev.setSignaledValue(5);
    ev.setSignaledValue(7);
    c.waitUntilCompleted();
    println!("wait {d} s ({}): frame 1 done {ok1}, frame 2 done after the wait {ok2}; buffer status {:?} error {:?}", if busy { "busy" } else { "sleeping" }, c.status(), c.error().map(|e| e.localizedDescription().to_string()));
}
