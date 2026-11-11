//! Where is the compositor's deadline relative to the display link's target? (pacing lane, X4 card)
//!
//! A CAMetalLayer drawable is rendered (a clear, finished on the GPU) at the display link's callback, then
//! presented (`-[MTLDrawable present]`, the Core Animation side of a present) at a controlled time `x` before
//! the callback's `targetTimestamp` T; `presentedTime` (the system's own record of when the drawable reached
//! the glass) says which refresh showed it. One drawable outstanding at a time. The deadline is the largest
//! `x`... the smallest `x` for which the drawable still appears at T.
//!
//! This is the CAMetalLayer path; bough publishes IOSurfaces as layer contents in its own transaction, which
//! goes through the same render server, so the deadline found here is assumed, not measured, for bough.
//!
//!   latch_cal [reps per phase = 24]      (a small floating window that never takes focus; ~20 s)
use bough_metal::app;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_metal::*;
use objc2_quartz_core::{CACurrentMediaTime, CAMetalDrawable, CAMetalLayer};
use std::sync::{Arc, Mutex};

const XS_MS: [f64; 14] = [
    0.25, 0.5, 1.0, 1.5, 2.0, 2.5, 3.0, 3.5, 4.0, 5.0, 6.0, 7.0, 8.0, 8.3,
];

struct St {
    device: Retained<ProtocolObject<dyn MTLDevice>>,
    queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
    layer: Retained<CAMetalLayer>,
    k: usize,
    tick: usize,
    outstanding: Arc<Mutex<bool>>,
    /// (x ms, target, present call returned, presentedTime)
    rows: Arc<Mutex<Vec<(f64, f64, f64, f64)>>>,
}

fn main() {
    let reps: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(24);
    {
        use objc2_foundation::{NSActivityOptions, NSProcessInfo, NSString};
        let t = NSProcessInfo::processInfo().beginActivityWithOptions_reason(
            NSActivityOptions::UserInteractive,
            &NSString::from_str("latch calibration"),
        );
        std::mem::forget(t);
    }
    let mut st: Option<St> = None;
    app::run_sized("latch calibration", 320.0, 200.0, move |host, link| {
        let s = st.get_or_insert_with(|| {
            host.window.setLevel(objc2_app_kit::NSFloatingWindowLevel);
            let device = MTLCreateSystemDefaultDevice().expect("device");
            let queue = device.newCommandQueue().expect("queue");
            let layer = CAMetalLayer::new();
            layer.setDevice(Some(&device));
            layer.setPixelFormat(MTLPixelFormat::BGRA8Unorm);
            layer.setFramebufferOnly(true);
            layer.setDrawableSize(objc2_core_foundation::CGSize {
                width: 640.0,
                height: 400.0,
            });
            layer.setContentsScale(2.0);
            layer.setFrame(objc2_core_foundation::CGRect::new(
                objc2_core_foundation::CGPoint::new(0.0, 0.0),
                objc2_core_foundation::CGSize::new(320.0, 200.0),
            ));
            layer.setMaximumDrawableCount(3);
            host.attach(&layer);
            St {
                device,
                queue,
                layer,
                k: 0,
                tick: 0,
                outstanding: Arc::new(Mutex::new(false)),
                rows: Arc::new(Mutex::new(Vec::new())),
            }
        });
        let _ = &s.device;
        s.tick += 1;
        // settle for a second, then one drawable every third refresh while none is outstanding
        if s.tick < 120 || s.tick % 3 != 0 || *s.outstanding.lock().unwrap() {
            return;
        }
        if s.k >= XS_MS.len() * reps {
            std::thread::sleep(std::time::Duration::from_millis(100));
            report(&s.rows.lock().unwrap(), link.duration());
            host.close();
            std::process::exit(0);
        }
        let x = XS_MS[s.k % XS_MS.len()] * 1e-3;
        s.k += 1;
        let t = link.targetTimestamp();
        let Some(d) = s.layer.nextDrawable() else {
            return;
        };
        let tex = d.texture();
        let pass = MTLRenderPassDescriptor::new();
        let ca = unsafe { pass.colorAttachments().objectAtIndexedSubscript(0) };
        ca.setTexture(Some(&tex));
        ca.setLoadAction(MTLLoadAction::Clear);
        ca.setStoreAction(MTLStoreAction::Store);
        let g = (s.k % 7) as f64 / 7.0;
        ca.setClearColor(MTLClearColor {
            red: 0.2,
            green: g,
            blue: 0.4,
            alpha: 1.0,
        });
        let cmd = s.queue.commandBuffer().expect("command buffer");
        let enc = cmd
            .renderCommandEncoderWithDescriptor(&pass)
            .expect("encoder");
        enc.endEncoding();
        cmd.commit();
        cmd.waitUntilCompleted();
        *s.outstanding.lock().unwrap() = true;
        let (rows, out) = (s.rows.clone(), s.outstanding.clone());
        let blk = block2::RcBlock::new(
            move |dr: std::ptr::NonNull<ProtocolObject<dyn MTLDrawable>>| {
                // SAFETY: Metal hands the handler a live drawable.
                let pt = unsafe { dr.as_ref() }.presentedTime();
                if let Some(r) = rows.lock().unwrap().iter_mut().rev().find(|r| r.3 < 0.0) {
                    r.3 = pt;
                }
                *out.lock().unwrap() = false;
            },
        );
        // SAFETY: the block is copied by Metal and called once.
        unsafe { d.addPresentedHandler(block2::RcBlock::as_ptr(&blk)) };
        while CACurrentMediaTime() < t - x {
            std::hint::spin_loop();
        }
        let late = CACurrentMediaTime() - (t - x);
        d.present();
        let done = CACurrentMediaTime();
        if late > 0.0003 {
            // the callback came too late for this phase: keep the row, flagged by its present time
        }
        s.rows.lock().unwrap().push((x * 1e3, t, done, -1.0));
    })
}

fn report(rows: &[(f64, f64, f64, f64)], period: f64) {
    println!(
        "refresh period {:.3} ms; presentedTime - T in refreshes (0 = shown at the target), per present phase x = T - present:",
        period * 1e3
    );
    println!(
        "{:>7} {:>5} {:>6} {:>6} {:>6} {:>7} {:>9}",
        "x ms", "n", "at T", "T+1", "T+2+", "dropped", "lateness"
    );
    for &x in &XS_MS {
        let rs: Vec<_> = rows.iter().filter(|r| (r.0 - x).abs() < 1e-9).collect();
        let (mut at, mut p1, mut p2, mut dr) = (0, 0, 0, 0);
        let mut lat = Vec::new();
        for r in &rs {
            if r.3 <= 0.0 {
                dr += 1;
                continue;
            }
            let k = ((r.3 - r.1) / period).round() as i64;
            match k {
                i64::MIN..=0 => at += 1,
                1 => p1 += 1,
                _ => p2 += 1,
            }
            lat.push((r.2 - (r.1 - x * 1e-3)) * 1e6);
        }
        lat.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let med = lat.get(lat.len() / 2).copied().unwrap_or(0.0);
        println!(
            "{:>7.2} {:>5} {:>6} {:>6} {:>6} {:>7} {:>7.0}us",
            x,
            rs.len(),
            at,
            p1,
            p2,
            dr,
            med
        );
    }
}
