//! The windowed host: bough-metal's raw AppKit app (no winit), but driven by a wall-clock timer
//! instead of a display link. The link is paused after the first tick and never runs again:
//! the only wakeups are the workload's own 60 Hz timer (none at all when idle) and the one-shot
//! that ends the run. Frames are committed straight from the timer turn when (and only when) the
//! turn produced damage.
use crate::app::{App, Config, View};
use block2::RcBlock;
use objc2_foundation::{NSRunLoop, NSTimer};
use std::cell::{Cell, RefCell};
use std::ptr::NonNull;
use std::rc::Rc;
use std::time::Duration;

/// The calm-window trim: ONE one-shot timer, pushed back (at most four times a second) while frames
/// keep leaving written spare surfaces, so it fires only after about a second without a frame, and
/// then makes the spares purgeable (`App::trim`). It never fires under steady activity (no wakeups
/// added to a 60 Hz or 530 ms-blink window), and a window that draws one surface arms nothing.
#[derive(Default)]
struct Trim {
    timer: RefCell<Option<objc2::rc::Retained<NSTimer>>>,
    pushed: Cell<Option<std::time::Instant>>,
}
const TRIM_AFTER: f64 = 1.0;

fn push_trim<V: View>(app: &Rc<RefCell<App<V>>>, st: &Rc<Trim>) {
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
        let scale = host.scale() as f32;
        let mut app = App::new(view, make_cfg(scale), true);
        {
            let g = app.gpu.as_mut().unwrap();
            g.publish = std::env::var_os("BOUGH_NOPUBLISH").is_none();
            host.attach(&g.layer);
        }
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
                if a.frame() && a.needs_trim() {
                    push_trim(&app_rc, &trim);
                }
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
                if std::env::var_os("BOUGH_STATS").is_some() {
                    eprintln!("surfaces: {:?}", app.borrow().surf_stats);
                }
                if std::env::var_os("BOUGH_CHECK").is_some() {
                    // the surface the real window last drew into vs a from-scratch full redraw of the retained state
                    let mut a = app.borrow_mut();
                    let px = a.presented_pixels();
                    let r = a.reference_pixels();
                    let n = px.iter().zip(&r).filter(|(x, y)| x != y).count();
                    eprintln!("window: presented vs full redraw: {n} differing bytes after {} frames; surfaces {:?}", a.frames, a.surf_stats);
                }

                if let Some(p) = std::env::var_os("BOUGH_DUMP") {
                    // the last presented surface (what the window shows), raw BGRA
                    let px = app.borrow_mut().presented_pixels();
                    std::fs::write(p, px).ok();
                }
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

/// Like `run_window`, but driven by the display link instead of a timer: `frame_cb(app, seconds)` is
/// called on every display refresh (seconds since the first), the host presents when it left damage
/// (animation scenes: "redrawn every display refresh at the frame's time"). The link is never paused.
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
    bough_metal::app::run_sized(title, w, h, move |host, _link| {
        if let Some((view, report)) = init.take() {
            let scale = host.scale() as f32;
            let mut app = App::new(view, make_cfg(scale), true);
            {
                let g = app.gpu.as_mut().unwrap();
                g.publish = std::env::var_os("BOUGH_NOPUBLISH").is_none();
                host.attach(&g.layer);
            }
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
                    if std::env::var_os("BOUGH_STATS").is_some() {
                        eprintln!("surfaces: {:?}", stats_app.borrow().surf_stats);
                    }
                    if std::env::var_os("BOUGH_CHECK").is_some() {
                        // the surface the real window last drew into vs a from-scratch full redraw of the retained state
                        let mut a = stats_app.borrow_mut();
                        let px = a.presented_pixels();
                        let r = a.reference_pixels();
                        let n = px.iter().zip(&r).filter(|(x, y)| x != y).count();
                        eprintln!("window: presented vs full redraw: {n} differing bytes after {} frames; surfaces {:?}", a.frames, a.surf_stats);
                    }

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
            if a.frame() && a.needs_trim() {
                push_trim(app, &trim);
            }
        }
    })
}
