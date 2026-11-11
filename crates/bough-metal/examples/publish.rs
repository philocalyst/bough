//! Presentation floor: what does it cost to put a NEW surface on screen, with no GPU rendering at all?
//! One windowed process, one display-link tick per publish, each variant ~1.1 s. All surfaces hold identical
//! pixels (no flicker for whoever is using the machine); the compositor cannot know that, so costs are real.
//!
//!   publish [secs-per-variant=1.1] [variant,variant,...]
//!
//! Metrics per variant: main-thread instr per tick (thread_selfcounts, overhead subtracted), publishing-thread
//! instr per publish (bg variants), process-wide instr per tick (proc_pid_rusage at variant start/end only),
//! CPU us per tick by thread, IOSurfaceIsInUse population (how many of our surfaces the compositor holds).
use bough_metal::app;
use bough_metal::gpu::{Gpu, ShaderSource};
use bough_metal::instance::{H, W};
use bough_metal::perf::*;
use bough_metal::present::make_iosurface;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, ProtocolObject};
use objc2::{define_class, msg_send, AnyThread, DefinedClass, MainThreadOnly};
use objc2_core_foundation::{CFRetained, CGPoint, CGRect, CGSize};
use objc2_foundation::{ns_string, NSDictionary, NSNull, NSNumber, NSObject, NSObjectProtocol, NSRunLoop, NSRunLoopCommonModes, NSString};
use objc2_io_surface::{IOSurfaceLockOptions, IOSurfaceRef};
use objc2_metal::*;
use objc2_quartz_core::*;
use std::cell::RefCell;
use std::ffi::c_void;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

static BG_INSTR: AtomicU64 = AtomicU64::new(0);
static BG_N: AtomicU64 = AtomicU64::new(0);
static MDL_TICKS: AtomicU64 = AtomicU64::new(0);
static MDL_INSTR: AtomicU64 = AtomicU64::new(0);

#[link(name = "AVFoundation", kind = "framework")]
#[link(name = "CoreMedia", kind = "framework")]
#[link(name = "CoreVideo", kind = "framework")]
unsafe extern "C" {
    fn CVPixelBufferCreateWithIOSurface(a: *const c_void, s: *const IOSurfaceRef, attrs: *const c_void, out: *mut *mut c_void) -> i32;
    fn CMVideoFormatDescriptionCreateForImageBuffer(a: *const c_void, ib: *mut c_void, out: *mut *mut c_void) -> i32;
    fn CMSampleBufferCreateReadyWithImageBuffer(a: *const c_void, ib: *mut c_void, fd: *mut c_void, timing: *const [u8; 72], out: *mut *mut c_void) -> i32;
    fn CMSampleBufferGetSampleAttachmentsArray(sb: *mut c_void, create: u8) -> *const c_void;
    fn CFArrayGetValueAtIndex(a: *const c_void, i: isize) -> *mut c_void;
    fn CFDictionarySetValue(d: *mut c_void, k: *const c_void, v: *const c_void);
    static kCMSampleAttachmentKey_DisplayImmediately: *const c_void;
    static kCFBooleanTrue: *const c_void;
}

struct SendBox<T>(T);
unsafe impl<T> Send for SendBox<T> {}

const SMALL: i32 = 240; // grid tile (8 x 6 tiles of 240x240 px)

fn fill(io: &IOSurfaceRef, v: u8) {
    unsafe {
        io.lock(IOSurfaceLockOptions::empty(), std::ptr::null_mut());
        let n = io.bytes_per_row() * io.height();
        std::ptr::write_bytes(io.base_address().as_ptr() as *mut u8, v, n);
        io.unlock(IOSurfaceLockOptions::empty(), std::ptr::null_mut());
    }
}

fn make_small() -> CFRetained<IOSurfaceRef> {
    let keys: [&NSString; 4] = [ns_string!("IOSurfaceWidth"), ns_string!("IOSurfaceHeight"), ns_string!("IOSurfaceBytesPerElement"), ns_string!("IOSurfacePixelFormat")];
    let vals = [NSNumber::new_i32(SMALL), NSNumber::new_i32(SMALL), NSNumber::new_i32(4), NSNumber::new_u32(0x4247_5241)];
    let refs: Vec<&AnyObject> = vals.iter().map(|v| v.as_ref() as &AnyObject).collect();
    let dict: Retained<NSDictionary<NSString, AnyObject>> = NSDictionary::from_slices(&keys, &refs);
    let cf: &objc2_core_foundation::CFDictionary = unsafe { &*(Retained::as_ptr(&dict) as *const objc2_core_foundation::CFDictionary) };
    unsafe { IOSurfaceRef::new(cf) }.expect("IOSurface")
}

fn plain_layer(x: f64, y: f64, w: f64, h: f64) -> Retained<CALayer> {
    let l = CALayer::new();
    l.setContentsScale(2.0);
    l.setOpaque(true);
    l.setFrame(CGRect::new(CGPoint::new(x, y), CGSize::new(w, h)));
    l
}

fn as_obj(io: &IOSurfaceRef) -> &AnyObject {
    unsafe { &*(io as *const IOSurfaceRef as *const AnyObject) }
}

fn txn(f: impl FnOnce()) {
    CATransaction::begin();
    CATransaction::setDisableActions(true);
    f();
    CATransaction::commit();
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "BoughPublishMdlDelegate"]
    #[ivars = RefCell<Option<Retained<ProtocolObject<dyn MTLCommandQueue>>>>]
    struct MdlDelegate;
    unsafe impl NSObjectProtocol for MdlDelegate {}
    unsafe impl CAMetalDisplayLinkDelegate for MdlDelegate {
        #[unsafe(method(metalDisplayLink:needsUpdate:))]
        fn needs_update(&self, _link: &CAMetalDisplayLink, update: &CAMetalDisplayLinkUpdate) {
            let t0 = tinstr();
            let q = self.ivars().borrow();
            let q = q.as_ref().unwrap();
            let d = update.drawable();
            let cmd = q.commandBuffer().unwrap();
            clear_pass(&cmd, &d.texture());
            cmd.presentDrawable(ProtocolObject::from_ref(&*d));
            cmd.commit();
            MDL_INSTR.fetch_add(tinstr() - t0, Relaxed);
            MDL_TICKS.fetch_add(1, Relaxed);
        }
    }
);

fn clear_pass(cmd: &ProtocolObject<dyn MTLCommandBuffer>, tex: &ProtocolObject<dyn MTLTexture>) {
    let pd = MTLRenderPassDescriptor::renderPassDescriptor();
    let c0 = unsafe { pd.colorAttachments().objectAtIndexedSubscript(0) };
    c0.setTexture(Some(tex));
    c0.setLoadAction(MTLLoadAction::Clear);
    c0.setClearColor(MTLClearColor { red: 0x20 as f64 / 255.0, green: 0x20 as f64 / 255.0, blue: 0x20 as f64 / 255.0, alpha: 1.0 });
    c0.setStoreAction(MTLStoreAction::Store);
    let enc = cmd.renderCommandEncoderWithDescriptor(&pd).unwrap();
    enc.endEncoding();
}

#[derive(Default)]
struct Res {
    name: String,
    ticks: u64,
    main: Vec<f64>,
    proc_per_tick: f64,
    bg_per: f64,
    cpu: String,
    inuse_hist: [u64; 5],
    wake_per_s: f64,
    footprint_mb: f64,
}

struct Lab {
    gpu: Gpu,
    big: Vec<CFRetained<IOSurfaceRef>>,
    small: Vec<CFRetained<IOSurfaceRef>>, // 48 tiles x 2
    one: Retained<CALayer>,
    flip: (Retained<CALayer>, Vec<Retained<CALayer>>),
    grid: (Retained<CALayer>, Vec<Retained<CALayer>>),
    implicit: Retained<CALayer>,
    metal: Retained<CAMetalLayer>,
    avs: Retained<CALayer>,
    sbufs: Vec<*mut c_void>,
    q: dispatch2::DispatchRetained<dispatch2::DispatchQueue>,
    variants: Vec<&'static str>,
    vi: usize,
    k: u64,
    settle: u32,
    t_start: f64,
    s_start: Snap,
    thr_start: std::collections::HashMap<u32, (String, u64)>,
    cur: Res,
    out: Vec<Res>,
    secs: f64,
    ovh: u64,
    mdl: Option<(Retained<CAMetalDisplayLink>, Retained<MdlDelegate>)>,
}

impl Lab {
    fn layer_for(&self, v: &str) -> Retained<CALayer> {
        match v {
            "hidden-flip" => self.flip.0.clone(),
            "grid-1" | "grid-48" => self.grid.0.clone(),
            "main-implicit" => self.implicit.clone(),
            "metal-layer" | "metal-display-link" => Retained::into_super(self.metal.clone()),
            "avsbdl" => self.avs.clone(),
            _ => self.one.clone(),
        }
    }

    /// The per-tick publish of variant `v`. Returns nothing; main-thread cost is bracketed by the caller.
    fn publish(&mut self, v: &str) {
        let k = self.k as usize;
        match v {
            "tick-only" => {}
            "main-explicit" => {
                let io = &self.big[k % 4];
                txn(|| unsafe { self.one.setContents(Some(as_obj(io))) });
            }
            "main-implicit" => unsafe { self.implicit.setContents(Some(as_obj(&self.big[k % 4]))) },
            "bg-explicit" => {
                let sb = SendBox((self.one.clone(), self.big[k % 4].clone()));
                self.q.exec_async(move || {
                    let sb = sb;
                    let (layer, io) = &sb.0;
                    let t0 = tinstr();
                    txn(|| unsafe { layer.setContents(Some(as_obj(&io))) });
                    BG_INSTR.fetch_add(tinstr() - t0, Relaxed);
                    BG_N.fetch_add(1, Relaxed);
                });
            }
            "same-contents" => {
                let io = &self.big[0];
                txn(|| unsafe { self.one.setContents(Some(as_obj(io))) });
            }
            "contents-changed" => {
                txn(|| unsafe {
                    let _: () = msg_send![&*self.one, setContentsChanged];
                });
            }
            "hidden-flip" => {
                let (a, b) = (&self.flip.1[k % 4], &self.flip.1[(k + 1) % 4]);
                txn(|| {
                    b.setHidden(false);
                    a.setHidden(true);
                });
            }
            "grid-1" => {
                let t = k % 48;
                let io = &self.small[t * 2 + (k / 48) % 2];
                txn(|| unsafe { self.grid.1[t].setContents(Some(as_obj(io))) });
            }
            "grid-48" => {
                txn(|| {
                    for t in 0..48 {
                        unsafe { self.grid.1[t].setContents(Some(as_obj(&self.small[t * 2 + k % 2]))) };
                    }
                });
            }
            "metal-layer" => {
                let d = self.metal.nextDrawable().unwrap();
                let cmd = self.gpu.queue.commandBuffer().unwrap();
                clear_pass(&cmd, &d.texture());
                cmd.presentDrawable(ProtocolObject::from_ref(&*d));
                cmd.commit();
            }
            "avsbdl" => {
                let sb = self.sbufs[k % 4];
                let _: () = unsafe { msg_send![&*self.avs, enqueueSampleBuffer: sb] };
            }
            _ => panic!("variant {v}"),
        }
    }
}

fn tick(lab: &mut Lab, host: &app::Host, link: &CADisplayLink) {
    let t0i = tinstr();
    let now = CACurrentMediaTime();
    let v = lab.variants[lab.vi];
    if lab.settle < 12 {
        lab.settle += 1;
        if lab.settle == 12 {
            lab.t_start = now;
            lab.s_start = snap();
            lab.thr_start = bough_metal::threads::snapshot();
            BG_INSTR.store(0, Relaxed);
            BG_N.store(0, Relaxed);
            MDL_INSTR.store(0, Relaxed);
            MDL_TICKS.store(0, Relaxed);
            lab.cur = Res { name: v.to_string(), ..Default::default() };
        }
        if v != "metal-display-link" {
            lab.publish(v);
        }
        lab.k += 1;
        return;
    }
    if now - lab.t_start >= lab.secs {
        // close the variant
        let s = snap();
        let thr = bough_metal::threads::snapshot();
        let ticks = if v == "metal-display-link" { MDL_TICKS.load(Relaxed) } else { lab.cur.ticks };
        lab.cur.ticks = ticks;
        let dt = now - lab.t_start;
        lab.cur.proc_per_tick = (s.instrs - lab.s_start.instrs) as f64 / ticks.max(1) as f64;
        lab.cur.wake_per_s = (s.wakeups - lab.s_start.wakeups) as f64 / dt;
        lab.cur.footprint_mb = s.footprint as f64 / 1e6;
        lab.cur.cpu = bough_metal::threads::diff_top(&lab.thr_start, &thr, ticks, 5);
        if BG_N.load(Relaxed) > 0 {
            lab.cur.bg_per = BG_INSTR.load(Relaxed) as f64 / BG_N.load(Relaxed) as f64 - lab.ovh as f64;
        }
        if v == "metal-display-link" {
            lab.cur.main = vec![MDL_INSTR.load(Relaxed) as f64 / ticks.max(1) as f64 - lab.ovh as f64];
            let (l, _) = lab.mdl.take().unwrap();
            l.invalidate();
            link.setPaused(false);
        }
        lab.out.push(std::mem::take(&mut lab.cur));
        lab.vi += 1;
        lab.settle = 0;
        if lab.vi >= lab.variants.len() {
            report(lab);
            host.detach_all();
            host.close();
            std::process::exit(0);
        }
        let nv = lab.variants[lab.vi];
        host.attach(&lab.layer_for(nv));
        if nv == "metal-display-link" {
            // the CADisplayLink keeps running only to drive variant bookkeeping at low cost? No: pause it so
            // the process cost is the metal display link's alone; a 1-s NSTimer ends the variant.
            let d: Retained<MdlDelegate> = {
                let this = MdlDelegate::alloc(host.mtm).set_ivars(RefCell::new(Some(lab.gpu.queue.clone())));
                unsafe { msg_send![super(this), init] }
            };
            let l = CAMetalDisplayLink::initWithMetalLayer(CAMetalDisplayLink::alloc(), &lab.metal);
            l.setDelegate(Some(ProtocolObject::from_ref(&*d)));
            unsafe { l.addToRunLoop_forMode(&NSRunLoop::mainRunLoop(), NSRunLoopCommonModes) };
            lab.mdl = Some((l, d));
            // keep ticking the CADisplayLink: its own cost is measured by tick-only and subtracted in the report
        }
        return;
    }
    // inside a variant
    let (sample_inuse, nbig) = (lab.k % 8 == 0, 4);
    if v != "metal-display-link" {
        lab.publish(v);
    }
    lab.k += 1;
    if v != "metal-display-link" {
        lab.cur.ticks += 1;
        lab.cur.main.push((tinstr() - t0i).saturating_sub(lab.ovh) as f64);
    }
    if sample_inuse {
        let n = if v.starts_with("grid") { (0..96).filter(|&i| lab.small[i].is_in_use()).count().min(4) } else { (0..nbig).filter(|&i| lab.big[i].is_in_use()).count() };
        lab.cur.inuse_hist[n] += 1;
    }
}

fn report(lab: &mut Lab) {
    let tick_only = lab.out.iter().find(|r| r.name == "tick-only").map(|r| r.proc_per_tick).unwrap_or(0.0);
    println!("thread_selfcounts overhead {} instr (subtracted); display link {:.2} ms", lab.ovh, 8.33);
    println!("\n| variant | ticks | main-thread instr/tick med | publish-thread instr | process instr/tick | process minus tick-only | wakeups/s | in-use count hist (0..4 of 4 big surfaces) | CPU us/tick by thread |");
    println!("|---|---|---|---|---|---|---|---|---|");
    for r in lab.out.iter_mut() {
        let m = pct(&mut r.main, 0.5);
        let extra = if r.name == "metal-display-link" { "(CADisplayLink kept running: subtract tick-only)" } else { "" };
        println!(
            "| {} | {} | {:.0} | {} | {:.0} | {:.0} {} | {:.0} | {:?} | {} |",
            r.name, r.ticks, m, if r.bg_per > 0.0 { format!("{:.0}", r.bg_per) } else { "-".into() }, r.proc_per_tick,
            r.proc_per_tick - tick_only, extra, r.wake_per_s, r.inuse_hist, r.cpu
        );
    }
    let s = snap();
    println!("\nphys_footprint {:.1} MB", s.footprint as f64 / 1e6);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let secs: f64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(1.1);
    let all = [
        "tick-only", "main-explicit", "main-implicit", "bg-explicit", "same-contents", "contents-changed", "hidden-flip", "grid-1", "grid-48",
        "metal-layer", "metal-display-link", "avsbdl",
    ];
    let variants: Vec<&'static str> = match args.get(2) {
        Some(s) => all.iter().copied().filter(|v| s.split(',').any(|x| x == *v)).collect(),
        None => all.to_vec(),
    };
    let mut lab: Option<Lab> = None;
    app::run("bough-metal publish floor", move |host, link| {
        if lab.is_none() {
            let gpu = Gpu::new(ShaderSource::Metallib);
            let big: Vec<_> = (0..4).map(|_| make_iosurface()).collect();
            for b in &big {
                fill(b, 0x20);
            }
            let small: Vec<_> = (0..96).map(|_| make_small()).collect();
            for s in &small {
                fill(s, 0x20);
            }
            let (w, h) = (W as f64 / 2.0, H as f64 / 2.0);
            let one = plain_layer(0.0, 0.0, w, h);
            unsafe { one.setContents(Some(as_obj(&big[0]))) };
            let implicit = plain_layer(0.0, 0.0, w, h);
            {
                let keys: [&NSString; 1] = [ns_string!("contents")];
                let null = NSNull::null();
                let vals: [&AnyObject; 1] = [null.as_ref()];
                let d: Retained<NSDictionary<NSString, AnyObject>> = NSDictionary::from_slices(&keys, &vals);
                let _: () = unsafe { msg_send![&*implicit, setActions: &*d] };
            }
            let froot = plain_layer(0.0, 0.0, w, h);
            let flips: Vec<_> = (0..4)
                .map(|i| {
                    let l = plain_layer(0.0, 0.0, w, h);
                    unsafe { l.setContents(Some(as_obj(&big[i]))) };
                    l.setHidden(i != 0);
                    froot.addSublayer(&l);
                    l
                })
                .collect();
            let groot = plain_layer(0.0, 0.0, w, h);
            let tiles: Vec<_> = (0..48)
                .map(|t| {
                    let (tx, ty) = ((t % 8) as f64, (t / 8) as f64);
                    let s = SMALL as f64 / 2.0;
                    let l = plain_layer(tx * s, ty * s, s, s);
                    unsafe { l.setContents(Some(as_obj(&small[t * 2]))) };
                    groot.addSublayer(&l);
                    l
                })
                .collect();
            let metal = bough_metal::present::make_metal_layer(&gpu.device, true);
            // AVSampleBufferDisplayLayer fed with CMSampleBuffers wrapping our IOSurfaces (built once, re-enqueued).
            let cls = AnyClass::get(c"AVSampleBufferDisplayLayer").expect("AVSampleBufferDisplayLayer");
            let avs: Retained<CALayer> = unsafe { msg_send![cls, new] };
            avs.setFrame(CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(w, h)));
            avs.setContentsScale(2.0);
            let want_avs = variants.contains(&"avsbdl");
            let sbufs: Vec<*mut c_void> = big
                .iter()
                .filter(|_| want_avs)
                .map(|io| unsafe {
                    let mut pb = std::ptr::null_mut();
                    assert_eq!(CVPixelBufferCreateWithIOSurface(std::ptr::null(), &**io, std::ptr::null(), &mut pb), 0);
                    let mut fd = std::ptr::null_mut();
                    assert_eq!(CMVideoFormatDescriptionCreateForImageBuffer(std::ptr::null(), pb, &mut fd), 0);
                    let timing = [0u8; 72]; // kCMTimeInvalid x3
                    let mut sb = std::ptr::null_mut();
                    assert_eq!(CMSampleBufferCreateReadyWithImageBuffer(std::ptr::null(), pb, fd, &timing, &mut sb), 0);
                    let arr = CMSampleBufferGetSampleAttachmentsArray(sb, 1);
                    let d = CFArrayGetValueAtIndex(arr, 0);
                    CFDictionarySetValue(d, kCMSampleAttachmentKey_DisplayImmediately, kCFBooleanTrue);
                    sb
                })
                .collect();
            let q = dispatch2::DispatchQueue::new("bough.publish", None);
            let first = variants[0];
            let l = Lab {
                gpu, big, small, one, flip: (froot, flips), grid: (groot, tiles), implicit, metal, avs, sbufs, q,
                variants: variants.clone(), vi: 0, k: 0, settle: 0, t_start: 0.0, s_start: snap(), thr_start: Default::default(),
                cur: Res::default(), out: vec![], secs, ovh: tinstr_overhead(), mdl: None,
            };
            host.attach(&l.layer_for(first));
            println!("window layer census: {} layers (BOUGH_BORDERLESS={})", host.layer_census(), std::env::var_os("BOUGH_BORDERLESS").is_some());
            lab = Some(l);
        }
        tick(lab.as_mut().unwrap(), host, link);
    });
}
