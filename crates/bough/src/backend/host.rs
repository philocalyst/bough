//! The windowed host: bough-metal's raw AppKit app (no winit), but driven by a wall-clock timer
//! instead of a display link. The link is paused after the first tick and never runs again:
//! the only wakeups are the workload's own 60 Hz timer (none at all when idle) and the one-shot
//! that ends the run. Frames are committed straight from the timer turn when (and only when) the
//! turn produced damage.
use crate::app::{App, Config, View};
use block2::RcBlock;
use objc2_foundation::{NSRunLoop, NSTimer};
use std::cell::RefCell;
use std::ptr::NonNull;
use std::rc::Rc;
use std::time::Duration;

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
            g.publish = true;
            host.attach(&g.layer);
        }
        app.frame();
        let app = Rc::new(RefCell::new(app));
        let tick = Rc::new(RefCell::new(tick));
        let mut timers = Vec::new();
        if let Some(p) = period {
            let (app, tick) = (app.clone(), tick.clone());
            let blk = RcBlock::new(move |_t: NonNull<NSTimer>| {
                let mut a = app.borrow_mut();
                (tick.borrow_mut())(&mut a);
                a.frame();
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
