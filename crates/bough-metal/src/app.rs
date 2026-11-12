//! Raw AppKit window + CADisplayLink driver (no winit).
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{define_class, msg_send, sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::*;
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::*;
use objc2_quartz_core::{CADisplayLink, CALayer, CATransaction};
use std::cell::RefCell;

thread_local! {
    static TICK: RefCell<Option<Box<dyn FnMut(&Host, &CADisplayLink)>>> = const { RefCell::new(None) };
    static HOST: RefCell<Option<Host>> = const { RefCell::new(None) };
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "BoughTickTarget"]
    pub struct TickTarget;
    impl TickTarget {
        #[unsafe(method(tick:))]
        fn tick(&self, link: &CADisplayLink) {
            let host = HOST.with(|h| h.borrow().clone());
            if let Some(host) = host {
                // The closure may re-enter (e.g. attach layers); take it out while it runs.
                let f = TICK.with(|t| t.borrow_mut().take());
                if let Some(mut f) = f {
                    f(&host, link);
                    TICK.with(|t| {
                        let mut t = t.borrow_mut();
                        if t.is_none() {
                            *t = Some(f);
                        }
                    });
                }
            }
        }
    }
);

#[derive(Clone)]
pub struct Host {
    pub window: Retained<NSWindow>,
    pub root: Retained<CALayer>,
    pub link: Retained<CADisplayLink>,
    pub mtm: MainThreadMarker,
}

impl Host {
    pub fn attach(&self, layer: &CALayer) {
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        if let Some(subs) = unsafe { self.root.sublayers() } {
            for s in subs.iter() {
                s.removeFromSuperlayer();
            }
        }
        self.root.addSublayer(layer);
        CATransaction::commit();
    }
    pub fn detach_all(&self) {
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        if let Some(subs) = unsafe { self.root.sublayers() } {
            for s in subs.iter() {
                s.removeFromSuperlayer();
            }
        }
        CATransaction::commit();
    }
    /// Number of CALayers in the window's whole tree (theme frame and all), i.e. what one commit may visit.
    pub fn layer_census(&self) -> usize {
        fn count(l: &CALayer) -> usize {
            1 + unsafe { l.sublayers() }.map(|s| s.iter().map(|c| count(&c)).sum()).unwrap_or(0)
        }
        let cv = self.window.contentView().unwrap();
        let mut top: Retained<NSView> = cv;
        while let Some(s) = unsafe { top.superview() } {
            top = s;
        }
        top.layer().map(|l| count(&l)).unwrap_or(0)
    }
    pub fn scale(&self) -> f64 {
        self.window.backingScaleFactor()
    }
    pub fn close(&self) {
        self.window.close();
    }
}

/// Create the window (960x720 points) and run the main loop; `f` is called on every display-link tick.
pub fn run(title: &str, f: impl FnMut(&Host, &CADisplayLink) + 'static) -> ! {
    run_sized(title, 960.0, 720.0, f)
}

/// Windows opened from here are benchmark windows unless the application facade says otherwise
/// (`bough::ui::Application::run` clears it): they go to a private Space (bench/tools/space, `BOUGH_BENCH_SPACE`).
pub static BENCH_WINDOWS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
static PLACED: std::sync::OnceLock<bough_space::Placed> = std::sync::OnceLock::new();

/// Where the window went (`None` before `run_sized` ordered it in).
pub fn placement() -> Option<&'static bough_space::Placed> {
    PLACED.get()
}

/// `run` with an explicit content size in points (the façade's windows are not 960x720).
pub fn run_sized(title: &str, w: f64, h: f64, f: impl FnMut(&Host, &CADisplayLink) + 'static) -> ! {
    let mtm = MainThreadMarker::new().expect("main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory); // no Dock icon, no focus steal
    let rect = NSRect::new(NSPoint::new(120.0, 120.0), NSSize::new(w, h));
    let bench = BENCH_WINDOWS.load(std::sync::atomic::Ordering::Relaxed);
    let space = bough_space::mode_from_env(if bench { bough_space::Mode::Other } else { bough_space::Mode::Active });
    // a window kept off the user's Space is borderless too (ROUND6: windows must not disturb the user)
    let borderless = std::env::var_os("BOUGH_BORDERLESS").is_some() || space != bough_space::Mode::Active;
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            rect,
            if borderless { NSWindowStyleMask::Borderless } else { NSWindowStyleMask::Titled | NSWindowStyleMask::Closable | NSWindowStyleMask::Resizable },
            NSBackingStoreType::Buffered,
            false,
        )
    };
    unsafe { window.setReleasedWhenClosed(false) };
    window.setTitle(&NSString::from_str(title));
    let view = window.contentView().expect("content view");
    let root = CALayer::new();
    root.setFrame(CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(w, h)));
    view.setLayer(Some(&root)); // layer-hosting view
    view.setWantsLayer(true);
    app.finishLaunching();
    let placed = bough_space::show(&window, space);
    if std::env::var_os("BOUGH_SPACE_REPORT").is_some() {
        eprintln!("bough-space: {placed:?}");
    }
    let _ = PLACED.set(placed);

    let target: Retained<TickTarget> = unsafe { msg_send![TickTarget::alloc(mtm), init] };
    let link = unsafe { view.displayLinkWithTarget_selector(&target, sel!(tick:)) };
    unsafe { link.addToRunLoop_forMode(&NSRunLoop::mainRunLoop(), NSRunLoopCommonModes) };
    let host = Host { window, root, link, mtm };
    HOST.with(|h| *h.borrow_mut() = Some(host));
    TICK.with(|t| *t.borrow_mut() = Some(Box::new(f)));
    std::mem::forget(target);
    let _: &AnyObject = unsafe { &*(&*app as *const NSApplication as *const AnyObject) };
    app.run();
    std::process::exit(0)
}
