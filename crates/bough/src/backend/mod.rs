//! The backend boundary. Everything above it (the runtime, elements, layout, text, input routing, damage and the
//! surface ring's policy in `app.rs`) is portable; below it are three kinds of backend, chosen per platform:
//!
//! * **Executors** turn a frame (the region, `bough-frame`'s slot of damaged tile rows) into pixels: the Metal
//!   presenter (bough-metal, macOS), the wgpu-hal presenter (bough-gpu), and the CPU executor (`cpu_raster.rs`,
//!   every platform).
//! * **Surfaces** hold the pixels the CPU executor writes and show them: [`Surfaces`], implemented by a window
//!   host's buffers (Wayland `wl_shm`), by plain memory offscreen ([`MemSurfaces`]: tests and headless renders on
//!   any platform), and on macOS by the Metal presenter's IOSurfaces.
//! * **Hosts** own the window and the event loop and feed the portable input API (`App::pointer_move`,
//!   `key_down`, `resize`, ...): AppKit ([`host`], macOS) and Wayland (`wayland`, Linux).
//!
//! A new platform (Windows) is one host plus, if it has one, a GPU executor; the CPU executor and [`Surfaces`]
//! make it show correct frames before any GPU code exists.
#[cfg(target_os = "macos")]
pub mod host;
pub mod surfaces;
#[cfg(target_os = "linux")]
pub mod wayland;

pub use surfaces::{MemSurfaces, Surfaces};

use crate::app::{App, Config, View};

/// What a window run asks of its host besides the view (applications use `Application::run`; harnesses and
/// benchmarks use [`run`] with these): a hook once the first frame is up, a clock, a deadline and a report.
pub struct Run<V: View> {
    /// Exit after this many seconds (after `report`); 0: run until the window closes.
    pub seconds: f64,
    /// Once the first frame is up (a harness focuses an input, starts a probe).
    pub start: Box<dyn FnOnce(&mut App<V>)>,
    /// A clock: every `period` seconds `tick(app)` runs; the host presents what it damaged.
    pub every: Option<(f64, Box<dyn FnMut(&mut App<V>)>)>,
    /// Before exiting (the deadline or the window closing).
    pub report: Box<dyn FnOnce(&mut App<V>)>,
    /// Driven by the display (macOS: the display link; Wayland: frame callbacks): `frame(app, seconds since the
    /// first)` runs once per refresh while the frames it leaves keep drawing; a refresh with nothing to draw stops
    /// it (a finished animation costs nothing).
    pub linked: Option<Box<dyn FnMut(&mut App<V>, f64)>>,
    /// The application id the compositor sees (Wayland `xdg_toplevel.app_id`); default `BOUGH_APP_ID`, then "bough".
    pub app_id: Option<String>,
}

impl<V: View> Default for Run<V> {
    fn default() -> Self {
        Run { seconds: 0.0, start: Box::new(|_| {}), every: None, report: Box::new(|_| {}), linked: None, app_id: None }
    }
}

impl<V: View> Run<V> {
    pub fn seconds(mut self, s: f64) -> Self {
        self.seconds = s;
        self
    }
    pub fn start(mut self, f: impl FnOnce(&mut App<V>) + 'static) -> Self {
        self.start = Box::new(f);
        self
    }
    pub fn every(mut self, period: f64, f: impl FnMut(&mut App<V>) + 'static) -> Self {
        self.every = Some((period, Box::new(f)));
        self
    }
    pub fn report(mut self, f: impl FnOnce(&mut App<V>) + 'static) -> Self {
        self.report = Box::new(f);
        self
    }
    pub fn app_id(mut self, id: impl Into<String>) -> Self {
        self.app_id = Some(id.into());
        self
    }
    pub fn linked(mut self, f: impl FnMut(&mut App<V>, f64) + 'static) -> Self {
        self.linked = Some(Box::new(f));
        self
    }
}

/// A window driven by a wall-clock timer: `tick(app)` every `period` (never if `None`), the host presenting what
/// it damaged; after `seconds` (if > 0) `report` runs and the process exits. (The workload drivers' entry.)
pub fn run_window<V: View>(title: &str, make_cfg: fn(f32) -> Config, view: V, tick: impl FnMut(&mut App<V>) + 'static, period: Option<std::time::Duration>, seconds: f64, report: impl FnOnce() + 'static) -> ! {
    #[cfg(target_os = "macos")]
    {
        host::run_window(title, make_cfg, view, tick, period, seconds, report)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let mut r = Run::default().seconds(seconds).report(move |_| report());
        if let Some(p) = period {
            r = r.every(p.as_secs_f64(), tick);
        }
        run(title, make_cfg, view, r)
    }
}

/// A window driven by the display: `frame_cb(app, seconds)` once per refresh while it leaves damage (see
/// [`Run::linked`]); after `seconds` (if > 0) `report` runs and the process exits.
pub fn run_window_linked<V: View>(title: &str, make_cfg: fn(f32) -> Config, view: V, frame_cb: impl FnMut(&mut App<V>, f64) + 'static, seconds: f64, report: impl FnOnce() + 'static) -> ! {
    #[cfg(target_os = "macos")]
    {
        host::run_window_linked(title, make_cfg, view, frame_cb, seconds, report)
    }
    #[cfg(not(target_os = "macos"))]
    {
        run(title, make_cfg, view, Run::default().seconds(seconds).report(move |_| report()).linked(frame_cb))
    }
}

/// Open a window of `make_cfg(scale)`'s size titled `title`, mount `view`, and run it through this platform's
/// host (AppKit on macOS, Wayland on Linux) with input, resize and the extras of `run`.
pub fn run<V: View>(title: &str, make_cfg: fn(f32) -> Config, view: V, run: Run<V>) -> ! {
    #[cfg(target_os = "macos")]
    {
        let Run { seconds, start, every, report, linked, app_id: _ } = run;
        assert!(linked.is_none(), "Run::linked on macOS: use run_window_linked");
        host::run_interactive(
            title,
            make_cfg,
            view,
            seconds,
            move |_, app| {
                start(&mut app.borrow_mut());
                if let Some((period, f)) = every {
                    let f = std::cell::RefCell::new(f);
                    host::timer(period, true, move || {
                        let mut a = app.borrow_mut();
                        (f.borrow_mut())(&mut a);
                        a.frame_paced();
                    });
                }
            },
            report,
        )
    }
    #[cfg(target_os = "linux")]
    {
        wayland::run(title, make_cfg, view, run)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = (title, make_cfg, view, run);
        panic!("bough: no window host on this platform yet")
    }
}
