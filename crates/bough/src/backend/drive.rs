//! In-process synthetic input (feature `framelog`): AppKit events posted into this process's own event
//! queue (`-[NSApplication postEvent:atStart:]`), from a driver thread, so they arrive the way real input
//! does (queued while the main thread is busy, dispatched through the run loop and the host's event
//! monitor). Nothing is posted to the window server: the user's pointer and keyboard are never touched,
//! no other process sees the events, and no permission is involved.
//!
//! Every event carries its post time as its `timestamp` (host clock, as real events carry their hardware
//! time), so the frame log's input-to-present is measured from the moment the event entered the queue.
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{class, msg_send};
use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSEventType};
use objc2_foundation::{NSPoint, NSString};

pub use bough_metal::framelog;

/// Process wakeups (package idle + interrupt) and instructions retired so far (`proc_pid_rusage`).
pub fn wakeups() -> (u64, u64) {
    let s = bough_metal::perf::snap();
    (s.wakeups, s.instrs)
}

/// A mouse event kind (the probes spell AppKit's through these; they depend only on bough).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EvKind(pub NSEventType);
impl EvKind {
    pub const MOVE: EvKind = EvKind(NSEventType::MouseMoved);
    pub const DOWN: EvKind = EvKind(NSEventType::LeftMouseDown);
    pub const UP: EvKind = EvKind(NSEventType::LeftMouseUp);
    pub const DRAG: EvKind = EvKind(NSEventType::LeftMouseDragged);
}

/// Key modifiers.
#[derive(Clone, Copy, Debug)]
pub struct Flags(pub NSEventModifierFlags);
impl Flags {
    pub const NONE: Flags = Flags(NSEventModifierFlags(0));
    pub const CMD: Flags = Flags(NSEventModifierFlags::Command);
}

/// Where the driver aims: the window's number, its content height (AppKit's y is bottom-up) and the main
/// screen's height (the wheel's CGEvent is placed in global, top-left coordinates).
#[derive(Clone, Copy, Debug)]
pub struct Target {
    pub window: isize,
    pub content_h: f64,
    pub screen_h: f64,
}

impl Target {
    /// Read from the host's window (main thread).
    pub fn of(host: &bough_metal::app::Host) -> Target {
        let w = &host.window;
        let content = w.contentRectForFrameRect(w.frame());
        let screen_h = w
            .screen()
            .map_or(content.origin.y + content.size.height, |s| {
                s.frame().size.height
            });
        Target {
            window: w.windowNumber(),
            content_h: content.size.height,
            screen_h,
        }
    }
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventCreateScrollWheelEvent2(
        source: *const std::ffi::c_void,
        units: u32,
        count: u32,
        w1: i32,
        w2: i32,
        w3: i32,
    ) -> *mut std::ffi::c_void;
    fn CGEventSetLocation(e: *mut std::ffi::c_void, p: NSPoint);
    fn CGEventSetTimestamp(e: *mut std::ffi::c_void, t: u64);
    fn CGEventSetIntegerValueField(e: *mut std::ffi::c_void, field: u32, v: i64);
    fn CFRelease(p: *const std::ffi::c_void);
}

/// The host clock now, in seconds (`NSEvent.timestamp`'s unit and clock).
pub fn host_secs() -> f64 {
    bough_metal::framelog::now_ns() as f64 * 1e-9
}

/// Post `e` to this process's queue; returns its own timestamp in host ns (as the host converts it) and
/// notes the post in the frame log (kind, timestamp).
fn post(e: &NSEvent, kind: u64) -> u64 {
    let ts = bough_metal::framelog::secs_to_ns(e.timestamp());
    bough_metal::framelog::note(bough_metal::framelog::N_POSTED, kind, ts);
    // SAFETY: `+[NSApplication sharedApplication]` returns the app object; `postEvent:atStart:` may be called
    // from any thread (AppKit: "events posted in subthreads bubble up in the main thread event queue").
    unsafe {
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let _: () = msg_send![app, postEvent: e, atStart: false];
    }
    ts
}

/// A mouse event at (x, y) content points (top-left origin) carrying `seq` as its event number. Returns
/// its timestamp (host ns).
pub fn mouse(t: &Target, kind: EvKind, x: f64, y: f64, seq: isize) -> u64 {
    use bough_metal::framelog as fl;
    let kind = kind.0;
    let up = matches!(kind, NSEventType::LeftMouseUp | NSEventType::RightMouseUp);
    let e = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
        kind,
        NSPoint::new(x, t.content_h - y),
        NSEventModifierFlags::empty(),
        host_secs(),
        t.window,
        None,
        seq,
        1,
        if up { 0.0 } else { 1.0 },
    )
    .expect("mouse event");
    let k = match kind {
        NSEventType::LeftMouseDown | NSEventType::RightMouseDown => fl::K_DOWN,
        NSEventType::LeftMouseUp | NSEventType::RightMouseUp => fl::K_UP,
        _ => fl::K_MOVE,
    };
    post(&e, k)
}

/// A key down (`chars`, virtual key `code`) with modifiers. Returns its timestamp (host ns).
pub fn key(t: &Target, code: u16, chars: &str, flags: Flags) -> u64 {
    let flags = flags.0;
    let s = NSString::from_str(chars);
    let e = NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
        NSEventType::KeyDown,
        NSPoint::new(0.0, 0.0),
        flags,
        host_secs(),
        t.window,
        None,
        &s,
        &s,
        false,
        code,
    )
    .expect("key event");
    post(&e, bough_metal::framelog::K_KEY)
}

/// A line-unit wheel event of `lines` (negative: down, as the harness's button 5) at (x, y) content
/// points. AppKit has no wheel constructor: the event is a CGEvent (made, never posted to the window
/// server) wrapped as an NSEvent and posted to this process's queue. Such an event belongs to no window,
/// so AppKit reports its location in screen coordinates; it is placed where that location reads as the
/// intended window location (the host reads `locationInWindow`). Returns that location as AppKit
/// reports it (the caller checks it once) and the event's timestamp (host ns).
pub fn wheel(t: &Target, x: f64, y: f64, lines: i32) -> (NSPoint, u64) {
    // SAFETY: CoreGraphics event constructors and setters on an event this function owns; released below.
    unsafe {
        let cg = CGEventCreateScrollWheelEvent2(std::ptr::null(), 1, 1, lines, 0, 0);
        CGEventSetLocation(cg, NSPoint::new(x, t.screen_h - (t.content_h - y)));
        // CGEventTimestamp is in ns of the host clock (NSEvent.timestamp reads it as such)
        CGEventSetTimestamp(cg, bough_metal::framelog::now_ns());
        // kCGMouseEventWindowUnderMousePointer(ThatCanHandleThisEvent): the window the event is for
        CGEventSetIntegerValueField(cg, 91, t.window as i64);
        CGEventSetIntegerValueField(cg, 92, t.window as i64);
        let e: Option<Retained<NSEvent>> = msg_send![class!(NSEvent), eventWithCGEvent: cg];
        CFRelease(cg);
        let e = e.expect("wheel event");
        let at = e.locationInWindow();
        (at, post(&e, bough_metal::framelog::K_WHEEL))
    }
}

/// Run `f` on a new thread at user-interactive QoS (the driver: its posts should keep their schedule
/// while other sessions build).
pub fn spawn_driver(f: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .name("bough-drive".into())
        .spawn(move || {
            // SAFETY: sets this thread's own QoS class.
            unsafe {
                libc::pthread_set_qos_class_self_np(
                    libc::qos_class_t::QOS_CLASS_USER_INTERACTIVE,
                    0,
                )
            };
            f()
        })
        .expect("driver thread");
}

/// Sleep until host-clock ns `t` (coarse sleeps, then a short spin for the last 200 µs).
pub fn sleep_until(t: u64) {
    loop {
        let n = bough_metal::framelog::now_ns();
        if n >= t {
            return;
        }
        let d = t - n;
        if d > 300_000 {
            std::thread::sleep(std::time::Duration::from_nanos(d - 200_000));
        } else {
            std::hint::spin_loop();
        }
    }
}

/// Keep this process out of App Nap and timer coalescing for the run (a background, never-active probe
/// would otherwise be throttled; the AWAGY harness made its apps active instead, which takes focus).
/// The activity token lives as long as the process.
pub fn no_app_nap() {
    use objc2_foundation::{NSActivityOptions, NSProcessInfo};
    let token = NSProcessInfo::processInfo().beginActivityWithOptions_reason(
        NSActivityOptions::UserInteractive,
        &NSString::from_str("bough frame-pacing probe"),
    );
    std::mem::forget(token);
}

/// Float the window above normal windows (it still never becomes key or active): a covered window is
/// throttled by the window server, which would invalidate the run (the AWAGY harness rejects such runs).
///
/// The window also ignores the user's real mouse (click-through): only events posted to this process reach
/// the app, and the user's pointer passes to whatever is beneath.
pub fn float(host: &bough_metal::app::Host) {
    host.window.setIgnoresMouseEvents(true);
    host.window.setLevel(objc2_app_kit::NSFloatingWindowLevel);
    host.window.orderFrontRegardless();
}

/// Is any part of the window visible (`NSWindowOcclusionStateVisible`)?
pub fn visible(host: &bough_metal::app::Host) -> bool {
    host.window
        .occlusionState()
        .contains(objc2_app_kit::NSWindowOcclusionState::Visible)
}

/// After `secs`: write the frame log (`BOUGH_FRAMELOG`), close the window and exit 0.
pub fn exit_after(host: &bough_metal::app::Host, secs: f64) {
    use block2::RcBlock;
    use objc2_foundation::{NSRunLoop, NSRunLoopCommonModes, NSTimer};
    let host = host.clone();
    let blk = RcBlock::new(move |_t: std::ptr::NonNull<NSTimer>| {
        bough_metal::framelog::note(bough_metal::framelog::N_OCCLUSION, visible(&host) as u64, 1);
        super::host::dump_framelog();
        host.close();
        std::process::exit(0);
    });
    // SAFETY: a repeating-false timer with a block that lives as long as the timer.
    let t = unsafe { NSTimer::timerWithTimeInterval_repeats_block(secs, false, &blk) };
    // SAFETY: the main run loop, on the main thread (the harness runs there).
    unsafe { NSRunLoop::mainRunLoop().addTimer_forMode(&t, NSRunLoopCommonModes) };
    std::mem::forget(t);
}

#[link(name = "CoreVideo", kind = "framework")]
unsafe extern "C" {
    fn CVDisplayLinkCreateWithCGDisplay(display: u32, out: *mut *mut std::ffi::c_void) -> i32;
    fn CVDisplayLinkSetOutputCallback(
        link: *mut std::ffi::c_void,
        cb: extern "C" fn(
            *mut std::ffi::c_void,
            *const u8,
            *const u8,
            u64,
            *mut u64,
            *mut std::ffi::c_void,
        ) -> i32,
        ctx: *mut std::ffi::c_void,
    ) -> i32;
    fn CVDisplayLinkStart(link: *mut std::ffi::c_void) -> i32;
    fn CVDisplayLinkStop(link: *mut std::ffi::c_void) -> i32;
}
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGMainDisplayID() -> u32;
}

/// `CVTimeStamp.hostTime` (offset 16) in host ns.
fn cv_host_ns(ts: *const u8) -> u64 {
    // SAFETY: `ts` points at a CVTimeStamp (80 bytes; hostTime is the u64 at offset 16), valid for the call.
    let ticks = unsafe { std::ptr::read_unaligned(ts.add(16) as *const u64) };
    let (n, d) = bough_metal::framelog::timebase();
    (ticks as u128 * n as u128 / d as u128) as u64
}

extern "C" fn on_vsync(
    _l: *mut std::ffi::c_void,
    now: *const u8,
    out: *const u8,
    _fi: u64,
    _fo: *mut u64,
    _c: *mut std::ffi::c_void,
) -> i32 {
    bough_metal::framelog::vsync_ns(cv_host_ns(now), cv_host_ns(out), 1);
    0
}

/// A measurement-only vsync recorder: a CVDisplayLink on the main display, on its own thread, writing
/// one `TAG_VSYNC` row (source 1) per refresh, so input and commit times can be placed against the
/// refresh timeline whether or not the host's own display link runs. It wakes the process once per
/// refresh while it runs: start it for a workload, stop it before an idle window.
pub struct VsyncRecorder(*mut std::ffi::c_void);
// SAFETY: a CVDisplayLink may be started and stopped from any thread.
unsafe impl Send for VsyncRecorder {}

impl VsyncRecorder {
    pub fn start() -> VsyncRecorder {
        let mut l = std::ptr::null_mut();
        // SAFETY: creates a link for the main display and starts it; the callback touches only the frame log.
        unsafe {
            CVDisplayLinkCreateWithCGDisplay(CGMainDisplayID(), &mut l);
            CVDisplayLinkSetOutputCallback(l, on_vsync, std::ptr::null_mut());
            CVDisplayLinkStart(l);
        }
        VsyncRecorder(l)
    }
}

impl Drop for VsyncRecorder {
    fn drop(&mut self) {
        // SAFETY: the link created in `start`; stopping waits for an in-flight callback.
        unsafe { CVDisplayLinkStop(self.0) };
    }
}
