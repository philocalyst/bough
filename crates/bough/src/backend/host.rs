//! The windowed host: bough-metal's raw AppKit app (no winit).
//!
//! * `run_window`: driven by a wall-clock timer (the workload's own 60 Hz tick, or a 530 ms blink); the
//!   display link is paused after the first tick and never runs: the only wakeups are the workload's timer
//!   (none at all when idle). A turn that left damage presents from inside the timer callback.
//! * `run_window_linked`: driven by the display link (animation: one frame per refresh at the frame's
//!   time). The link runs only while the callback keeps producing damage; a refresh with nothing to draw
//!   pauses it (`App::frame` returned false), so a finished animation costs nothing.
//!
//! Both arm one calm-window trim timer (pushed back while frames keep leaving written spare surfaces), which
//! makes the spares purgeable about a second after the last frame.
use crate::app::{App, Config, View};
use block2::RcBlock;
use objc2_foundation::{NSRunLoop, NSTimer};
use std::cell::{Cell, RefCell};
use std::ptr::NonNull;
use std::rc::Rc;
use std::time::Duration;

#[derive(Default)]
struct Trim {
    timer: RefCell<Option<objc2::rc::Retained<NSTimer>>>,
    pushed: Cell<Option<std::time::Instant>>,
}

const TRIM_AFTER: f64 = 1.0;

/// After a frame: arm (or push back) the trim timer if spare surfaces hold written pixels.
fn after_frame<V: View>(app: &Rc<RefCell<App<V>>>, st: &Rc<Trim>, a: &mut App<V>, drew: bool) {
    if !drew || !a.needs_trim() {
        return;
    }
    let now = std::time::Instant::now();
    if st.pushed.get().is_some_and(|p| now.duration_since(p).as_millis() < 250) {
        return;
    }
    st.pushed.set(Some(now));
    {
        let t = st.timer.borrow();
        if let Some(t) = t.as_ref().filter(|t| t.isValid()) {
            t.setFireDate(&objc2_foundation::NSDate::dateWithTimeIntervalSinceNow(TRIM_AFTER));
            return;
        }
    }
    let app = app.clone();
    let blk = RcBlock::new(move |_t: NonNull<NSTimer>| {
        if let Ok(mut a) = app.try_borrow_mut() {
            a.trim(false);
        }
    });
    let t = unsafe { NSTimer::timerWithTimeInterval_repeats_block(TRIM_AFTER, false, &blk) };
    t.setTolerance(0.25);
    unsafe { NSRunLoop::mainRunLoop().addTimer_forMode(&t, objc2_foundation::NSRunLoopCommonModes) };
    *st.timer.borrow_mut() = Some(t);
}

fn new_app<V: View>(host: &bough_metal::app::Host, make_cfg: fn(f32) -> Config, view: V) -> App<V> {
    let scale = host.scale() as f32;
    let cfg = make_cfg(scale);
    let mut app = App::new(view, cfg, true);
    app.windowed = true;
    let g = app.gpu.as_mut().unwrap();
    g.publish = std::env::var_os("BOUGH_NOPUBLISH").is_none();
    host.attach(&g.layer);
    app
}

/// The exit report: `BOUGH_STATS` prints the ring counters, `BOUGH_CHECK` compares the surface the window
/// last showed with a full redraw of the retained state, `BOUGH_DUMP=<file>` writes it (raw BGRA).
fn report_exit<V: View>(a: &mut App<V>) {
    if std::env::var_os("BOUGH_STATS").is_some() {
        let g = a.gpu.as_ref().unwrap();
        eprintln!("surfaces: {:?}; presenter: {:?}; surface MB {:.1}", a.surf_stats, g.stats, g.surface_bytes() as f64 / 1048576.0);
    }
    if std::env::var_os("BOUGH_CHECK").is_some() {
        let px = a.presented_pixels();
        let r = a.reference_pixels();
        let n = px.iter().zip(&r).filter(|(x, y)| x != y).count();
        eprintln!("window: presented vs full redraw: {n} differing bytes after {} frames; surfaces {:?}", a.frames, a.surf_stats);
    }
    if let Some(p) = std::env::var_os("BOUGH_DUMP") {
        let px = a.presented_pixels();
        std::fs::write(p, px).ok();
    }
}

/// Open a window of `cfg` points, mount `view`, and run. `tick` is called every `period` (never if
/// `None`) and should call `app.turn(..)`; the host presents when the turn left damage. After
/// `seconds` (if > 0) `report` runs, the window closes and the process exits 0.
pub fn run_window<V: View>(
    title: &str,
    make_cfg: fn(f32) -> Config,
    view: V,
    tick: impl FnMut(&mut App<V>) + 'static,
    period: Option<Duration>,
    seconds: f64,
    report: impl FnOnce() + 'static,
) -> ! {
    let probe = make_cfg(2.0);
    let (w, h) = (probe.width_pt as f64, probe.height_pt as f64);
    drop(probe);
    let mut init = Some((view, tick, report));
    bough_metal::app::run_sized(title, w, h, move |host, link| {
        let Some((view, tick, report)) = init.take() else { return };
        link.setPaused(true);
        let mut app = new_app(host, make_cfg, view);
        app.frame();
        let app = Rc::new(RefCell::new(app));
        let tick = Rc::new(RefCell::new(tick));
        let trim = Rc::new(Trim::default());
        let mut timers = Vec::new();
        if let Some(p) = period {
            let (app_rc, tick, trim) = (app.clone(), tick.clone(), trim.clone());
            let blk = RcBlock::new(move |_t: NonNull<NSTimer>| {
                let mut a = app_rc.borrow_mut();
                (tick.borrow_mut())(&mut a);
                let drew = a.frame();
                after_frame(&app_rc, &trim, &mut a, drew);
            });
            let t = unsafe { NSTimer::timerWithTimeInterval_repeats_block(p.as_secs_f64(), true, &blk) };
            t.setTolerance(0.0005);
            unsafe { NSRunLoop::mainRunLoop().addTimer_forMode(&t, objc2_foundation::NSRunLoopCommonModes) };
            timers.push(t);
        }
        if seconds > 0.0 {
            let host = host.clone();
            let app = app.clone();
            let report = Rc::new(RefCell::new(Some(report)));
            let blk = RcBlock::new(move |_t: NonNull<NSTimer>| {
                if let Some(r) = report.borrow_mut().take() {
                    r();
                }
                report_exit(&mut app.borrow_mut());
                host.close();
                std::process::exit(0);
            });
            let t = unsafe { NSTimer::timerWithTimeInterval_repeats_block(seconds, false, &blk) };
            unsafe { NSRunLoop::mainRunLoop().addTimer_forMode(&t, objc2_foundation::NSRunLoopCommonModes) };
            timers.push(t);
        }
        std::mem::forget(timers);
        std::mem::forget(app);
    })
}

/// Like `run_window`, but driven by the display link: `frame_cb(app, seconds)` is called on every display
/// refresh (seconds since the first) and the host presents when it left damage. A refresh that leaves no
/// damage pauses the link (nothing animates any more).
pub fn run_window_linked<V: View>(
    title: &str,
    make_cfg: fn(f32) -> Config,
    view: V,
    frame_cb: impl FnMut(&mut App<V>, f64) + 'static,
    seconds: f64,
    report: impl FnOnce() + 'static,
) -> ! {
    let probe = make_cfg(2.0);
    let (w, h) = (probe.width_pt as f64, probe.height_pt as f64);
    drop(probe);
    let mut init = Some((view, report));
    let mut state: Option<(Rc<RefCell<App<V>>>, std::time::Instant)> = None;
    let mut cb = frame_cb;
    let trim = Rc::new(Trim::default());
    bough_metal::app::run_sized(title, w, h, move |host, link| {
        if let Some((view, report)) = init.take() {
            let mut app = new_app(host, make_cfg, view);
            app.frame();
            let app = Rc::new(RefCell::new(app));
            if seconds > 0.0 {
                let host = host.clone();
                let report = Rc::new(RefCell::new(Some(report)));
                let stats_app = app.clone();
                let blk = RcBlock::new(move |_t: NonNull<NSTimer>| {
                    if let Some(r) = report.borrow_mut().take() {
                        r();
                    }
                    report_exit(&mut stats_app.borrow_mut());
                    host.close();
                    std::process::exit(0);
                });
                let t = unsafe { NSTimer::timerWithTimeInterval_repeats_block(seconds, false, &blk) };
                unsafe { NSRunLoop::mainRunLoop().addTimer_forMode(&t, objc2_foundation::NSRunLoopCommonModes) };
                std::mem::forget(t);
            }
            state = Some((app, std::time::Instant::now()));
            return;
        }
        if let Some((app, t0)) = &state {
            let mut a = app.borrow_mut();
            cb(&mut a, t0.elapsed().as_secs_f64());
            let drew = a.frame();
            if !drew {
                link.setPaused(true);
            }
            after_frame(app, &trim, &mut a, drew);
        }
    })
}

// =============================================================================== interactive windows

use crate::input::{Button, Key, Mods};
use objc2_app_kit::{NSEvent, NSEventMask, NSEventModifierFlags, NSEventType};

/// Points one precise (trackpad) scroll pixel moves: AppKit's deltas are already in points.
fn key_of(e: &NSEvent) -> Vec<Key> {
    match e.keyCode() {
        48 => vec![Key::Tab],
        51 => vec![Key::Backspace],
        117 => vec![Key::Delete],
        36 | 76 => vec![Key::Enter],
        53 => vec![Key::Escape],
        123 => vec![Key::Left],
        124 => vec![Key::Right],
        125 => vec![Key::Down],
        126 => vec![Key::Up],
        115 => vec![Key::Home],
        119 => vec![Key::End],
        116 => vec![Key::PageUp],
        121 => vec![Key::PageDown],
        _ => e.characters().map(|s| s.to_string().chars().filter(|c| !c.is_control()).map(Key::Char).collect()).unwrap_or_default(),
    }
}

fn mods_of(e: &NSEvent) -> Mods {
    let f = e.modifierFlags();
    Mods {
        shift: f.contains(NSEventModifierFlags::Shift),
        ctrl: f.contains(NSEventModifierFlags::Control),
        alt: f.contains(NSEventModifierFlags::Option),
        cmd: f.contains(NSEventModifierFlags::Command),
    }
}

/// Deliver one AppKit event to the app (window coordinates, top-left origin). Returns whether it was input.
pub fn dispatch_event<V: View>(a: &mut App<V>, e: &NSEvent, content_h: f64) -> bool {
    let loc = e.locationInWindow();
    let (x, y) = (loc.x as f32, (content_h - loc.y) as f32);
    match e.r#type() {
        NSEventType::MouseMoved | NSEventType::LeftMouseDragged | NSEventType::RightMouseDragged | NSEventType::OtherMouseDragged => a.pointer_move(x, y),
        NSEventType::LeftMouseDown => a.pointer_down(x, y, Button::Primary),
        NSEventType::LeftMouseUp => a.pointer_up(x, y, Button::Primary),
        NSEventType::RightMouseDown => a.pointer_down(x, y, Button::Secondary),
        NSEventType::RightMouseUp => a.pointer_up(x, y, Button::Secondary),
        NSEventType::MouseExited => a.pointer_leave(),
        NSEventType::ScrollWheel => {
            let dy = e.scrollingDeltaY() as f32;
            if e.hasPreciseScrollingDeltas() {
                a.wheel_points(x, y, -dy);
            } else {
                a.wheel(x, y, 0.0, -dy);
            }
        }
        NSEventType::KeyDown => {
            let m = mods_of(e);
            for k in key_of(e) {
                a.key_down(k, m);
            }
        }
        _ => return false,
    }
    true
}

/// An interactive window: AppKit's mouse, wheel and key events go to the app's input turns (a local event
/// monitor; the window accepts mouse-moved events), each followed by a frame; while an input animation runs
/// (a glide, a tooltip fade) the display link ticks it and draws, and it pauses when nothing animates.
/// `start(host, app)` runs once the first frame is up (a probe installs its timers there). After `seconds`
/// (if > 0) `report` runs and the process exits.
pub fn run_interactive<V: View>(
    title: &str,
    make_cfg: fn(f32) -> Config,
    view: V,
    seconds: f64,
    start: impl FnOnce(&bough_metal::app::Host, Rc<RefCell<App<V>>>) + 'static,
    report: impl FnOnce(&mut App<V>) + 'static,
) -> ! {
    let probe = make_cfg(2.0);
    let (w, h) = (probe.width_pt as f64, probe.height_pt as f64);
    drop(probe);
    let mut init = Some((view, start, report));
    let mut state: Option<(Rc<RefCell<App<V>>>, Option<std::time::Instant>)> = None;
    let trim = Rc::new(Trim::default());
    bough_metal::app::run_sized(title, w, h, move |host, link| {
        if let Some((view, start, report)) = init.take() {
            let mut app = new_app(host, make_cfg, view);
            app.frame();
            let app = Rc::new(RefCell::new(app));
            host.window.setAcceptsMouseMovedEvents(true);
            {
                let (app, trim, host2) = (app.clone(), trim.clone(), host.clone());
                let blk = RcBlock::new(move |e: NonNull<NSEvent>| -> *mut NSEvent {
                    // SAFETY: AppKit hands the monitor a live event for the duration of the call.
                    let ev = unsafe { e.as_ref() };
                    if let Ok(mut a) = app.try_borrow_mut() {
                        if dispatch_event(&mut a, ev, h) {
                            let drew = a.frame();
                            after_frame(&app, &trim, &mut a, drew);
                            if a.animating() {
                                host2.link.setPaused(false);
                            }
                        }
                    }
                    e.as_ptr()
                });
                let mask = NSEventMask::MouseMoved
                    | NSEventMask::LeftMouseDown
                    | NSEventMask::LeftMouseUp
                    | NSEventMask::LeftMouseDragged
                    | NSEventMask::RightMouseDown
                    | NSEventMask::RightMouseUp
                    | NSEventMask::RightMouseDragged
                    | NSEventMask::MouseExited
                    | NSEventMask::ScrollWheel
                    | NSEventMask::KeyDown;
                // SAFETY: the block returns the event it was given (dispatch continues).
                let monitor = unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &blk) };
                std::mem::forget(monitor);
                std::mem::forget(blk);
            }
            if seconds > 0.0 {
                let host = host.clone();
                let app = app.clone();
                let report = Rc::new(RefCell::new(Some(report)));
                let blk = RcBlock::new(move |_t: NonNull<NSTimer>| {
                    if let Some(r) = report.borrow_mut().take() {
                        r(&mut app.borrow_mut());
                    }
                    report_exit(&mut app.borrow_mut());
                    host.close();
                    std::process::exit(0);
                });
                let t = unsafe { NSTimer::timerWithTimeInterval_repeats_block(seconds, false, &blk) };
                unsafe { NSRunLoop::mainRunLoop().addTimer_forMode(&t, objc2_foundation::NSRunLoopCommonModes) };
                std::mem::forget(t);
            }
            start(host, app.clone());
            link.setPaused(true);
            state = Some((app, None));
            return;
        }
        if let Some((app, last)) = &mut state {
            let Ok(mut a) = app.try_borrow_mut() else { return };
            let now = std::time::Instant::now();
            let dt = last.map_or(1.0 / 120.0, |l| now.duration_since(l).as_secs_f32().min(0.05));
            *last = Some(now);
            if !a.tick(dt) {
                link.setPaused(true);
                *last = None;
                return;
            }
            let drew = a.frame();
            after_frame(app, &trim, &mut a, drew);
        }
    })
}
