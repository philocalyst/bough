//! Foreign elements with real Core Animation layers (`foreign.rs`, `backend/layers.rs`): a "web view" (an
//! IOSurface test pattern on a plain CALayer) between two bough panels inside a scroll view, composited
//! *under* bough's surface through a rounded hole, with a translucent bough badge and a label painted over
//! it; and a second layer composited *over* the surface. The scroll offset oscillates for `SECONDS`, then the
//! layer tree is read back and checked against the placements, the surface against a full redraw, and the
//! process exits (0: PASS). The window never takes focus (accessory activation policy) and closes itself.
//!
//! `cargo run --release -p bough --example foreign_layers`
use block2::RcBlock;
use bough::backend::layers::LayerHost;
use bough::prelude::*;
use bough::raw::Config;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_core_foundation::{CFDictionary, CFRetained};
use objc2_foundation::{NSDictionary, NSNumber, NSRunLoop, NSString, NSTimer, ns_string};
use objc2_io_surface::{IOSurfaceLockOptions, IOSurfaceRef};
use objc2_quartz_core::CALayer;
use std::ptr::NonNull;

const SECONDS: f64 = 2.0;

struct Page {
    y: Signal<f32>,
}

impl View for Page {
    fn view(&self) -> impl Element<Self> + '_ {
        div()
            .w_pct(1.0)
            .h_pct(1.0)
            .bg(rgb(0x202830))
            .p(20.0)
            .gap(10.0)
            .child(
                div()
                    .w(440.0)
                    .h(240.0)
                    .bg(rgb(0xffffff))
                    .scroll(sig!(self.y))
                    .child(div().w(420.0).h(80.0).p(10.0).bg(rgb(0xcc3333)).child(text("a bough panel above the web view").color(rgb(0xffffff))))
                    .child(
                        div()
                            .relative()
                            .w(420.0)
                            .h(120.0)
                            .child(foreign(1).w(420.0).h(120.0).abs(0.0, 0.0).rounded(16.0))
                            .child(div().abs(16.0, 16.0).px(10.0).py(4.0).bg(Color::rgba(0, 0, 255, 128)).rounded(8.0).child(text("bough, over the web view").color(rgb(0xffffff)))),
                    )
                    .child(div().w(420.0).h(260.0).p(10.0).bg(rgb(0x33aa33)).child(text("a bough panel below the web view").color(rgb(0xffffff)))),
            )
            .child(row((text("a layer over the surface:").color(rgb(0xffffff)), foreign(2).mode(Mode::Over).w(120.0).h(40.0).rounded(8.0))).gap(10.0).items(Align::Center))
    }
}

/// A BGRA IOSurface filled with a checker of `a` and `b` (opaque).
fn pattern(w: usize, h: usize, a: [u8; 4], b: [u8; 4]) -> CFRetained<IOSurfaceRef> {
    let keys: [&NSString; 4] = [ns_string!("IOSurfaceWidth"), ns_string!("IOSurfaceHeight"), ns_string!("IOSurfaceBytesPerElement"), ns_string!("IOSurfacePixelFormat")];
    let vals = [NSNumber::new_i32(w as i32), NSNumber::new_i32(h as i32), NSNumber::new_i32(4), NSNumber::new_u32(0x4247_5241)];
    let refs: Vec<&AnyObject> = vals.iter().map(|v| v.as_ref() as &AnyObject).collect();
    let dict: Retained<NSDictionary<NSString, AnyObject>> = NSDictionary::from_slices(&keys, &refs);
    // SAFETY: NSDictionary is toll-free bridged to CFDictionary; a well-formed property dictionary.
    let io = unsafe { IOSurfaceRef::new(&*(Retained::as_ptr(&dict) as *const CFDictionary)) }.expect("IOSurface");
    // SAFETY: the surface is locked for writing; rows are `bytes_per_row` apart and `w * 4` long.
    unsafe {
        io.lock(IOSurfaceLockOptions(0), std::ptr::null_mut());
        let (bpr, base) = (io.bytes_per_row(), io.base_address().as_ptr() as *mut u8);
        for y in 0..h {
            for x in 0..w {
                let c = if (x / 16 + y / 16) % 2 == 0 { a } else { b };
                std::ptr::copy_nonoverlapping(c.as_ptr(), base.add(y * bpr + x * 4), 4);
            }
        }
        io.unlock(IOSurfaceLockOptions(0), std::ptr::null_mut());
    }
    io
}

fn config(scale: f32) -> Config {
    let font = std::fs::read(std::env::var("ZGX_FONT_FILE").unwrap_or_else(|_| "/System/Library/Fonts/Supplemental/Arial.ttf".into())).unwrap();
    Config::new(480.0, 360.0, scale, font)
}

fn main() {
    bough::backend::host::run_interactive(
        "bough: foreign layers",
        config,
        Page { y: Signal::new(0.0) },
        SECONDS,
        |_host, app| {
            {
                let mut a = app.borrow_mut();
                let surface = a.gpu.as_ref().unwrap().layer.clone();
                a.set_foreign_host(Box::new(LayerHost::new(&surface, |id| {
                    let l = CALayer::new();
                    let io = if id == 1 { pattern(256, 256, [60, 200, 60, 255], [200, 60, 200, 255]) } else { pattern(64, 64, [0, 140, 255, 255], [0, 140, 255, 255]) };
                    // SAFETY: an IOSurfaceRef is a valid CALayer `contents` object.
                    unsafe { l.setContents(Some(&*(&*io as *const IOSurfaceRef as *const AnyObject))) };
                    l
                })));
                // the frame that places the layers: publishing the host is not damage, so make some
                a.turn(|p| *p.y = 1.0);
                a.frame();
            }
            // the scroll offset oscillates on a 60 Hz timer
            let t0 = std::time::Instant::now();
            let blk = RcBlock::new(move |_t: NonNull<NSTimer>| {
                if let Ok(mut a) = app.try_borrow_mut() {
                    let t = t0.elapsed().as_secs_f32();
                    a.turn(|p| *p.y = 110.0 * (1.0 - (t * 2.5).cos()));
                    a.frame_paced();
                }
            });
            // SAFETY: a main-run-loop timer whose block owns what it uses.
            let t = unsafe { NSTimer::timerWithTimeInterval_repeats_block(1.0 / 60.0, true, &blk) };
            unsafe { NSRunLoop::mainRunLoop().addTimer_forMode(&t, objc2_foundation::NSRunLoopCommonModes) };
            std::mem::forget(t);
        },
        |a| {
            a.wait_presented();
            let mut ok = true;
            let mut check = |name: &str, pass: bool, detail: String| {
                println!("{} {name}: {detail}", if pass { "ok  " } else { "FAIL" });
                ok &= pass;
            };
            let surface = a.gpu.as_ref().unwrap().layer.clone();
            let root = surface.superlayer().unwrap();
            // SAFETY: read on the main thread, no transaction in flight on it.
            let subs = unsafe { root.sublayers() }.unwrap();
            let names: Vec<String> = subs.iter().map(|l| l.name().map_or("?".into(), |n| n.to_string())).collect();
            check("layer order (bottom to top)", names == ["foreign-1", "bough", "clip-2"], format!("{names:?}"));
            check("surface layer is not opaque (its alpha composes the layer beneath)", !surface.isOpaque(), format!("isOpaque = {}", surface.isOpaque()));
            let views = a.reflect_foreign();
            let h = surface.frame().size.height;
            for (v, layer_name) in views.iter().zip(["foreign-1", "clip-2"]) {
                let p = v.placement.expect("placed");
                let s = p.scale as f64;
                let l = subs.iter().find(|l| l.name().is_some_and(|n| n.to_string() == layer_name)).unwrap();
                let f = l.frame();
                // Below: the layer's frame is the box; Above: the wrapper's frame is the clip
                let r = if p.plane == Plane::Below { p.rect.map(|v| v as f64) } else { p.clip.map(|v| v as f64) };
                let want = [r[0] / s, h - r[3] / s, (r[2] - r[0]) / s, (r[3] - r[1]) / s];
                let got = [f.origin.x, f.origin.y, f.size.width, f.size.height];
                check(&format!("{layer_name} frame == placement of element {} ({:?}, order {}, covered {})", p.id, p.plane, p.order, p.covered), got == want, format!("{got:?} vs {want:?} (pt, y up)"));
            }
            let (px, rf) = (a.presented_pixels(), a.reference_pixels());
            let n = px.iter().zip(&rf).filter(|(x, y)| x != y).count();
            check("presented surface == full redraw", n == 0, format!("{n} differing bytes after {} frames", a.frames));
            // the hole: alpha 0 at the centre of the web view's visible part
            let p = views[0].placement.unwrap();
            if let Some(v) = p.visible() {
                let (cx, cy) = ((v[0] + v[2]) / 2, (v[1] + v[3] * 3) / 4);
                let i = ((cy * (480.0 * p.scale) as i32 + cx) * 4) as usize;
                check("the surface is transparent in the hole", px[i + 3] == 0, format!("alpha {} at ({cx}, {cy})", px[i + 3]));
            }
            println!("foreign_layers: {}", if ok { "PASS" } else { "FAIL" });
            if !ok {
                std::process::exit(1);
            }
        },
    )
}
