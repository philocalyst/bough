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
use bough_metal::framelog as fl;
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
            fl::note(fl::N_TRIM, 0, 0);
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
    dump_framelog();
}

/// `BOUGH_FRAMELOG=<file>`: write the frame log there (feature `framelog`; nothing without it).
pub fn dump_framelog() {
    if fl::ON {
        if let Some(p) = std::env::var_os("BOUGH_FRAMELOG") {
            fl::dump(std::path::Path::new(&p));
        }
    }
}

/// The frame log's kind for an AppKit event.
fn turn_kind(e: &NSEvent) -> u64 {
    match e.r#type() {
        NSEventType::MouseMoved | NSEventType::LeftMouseDragged | NSEventType::RightMouseDragged | NSEventType::OtherMouseDragged => fl::K_MOVE,
        NSEventType::LeftMouseDown | NSEventType::RightMouseDown => fl::K_DOWN,
        NSEventType::LeftMouseUp | NSEventType::RightMouseUp => fl::K_UP,
        NSEventType::ScrollWheel => fl::K_WHEEL,
        NSEventType::KeyDown => fl::K_KEY,
        NSEventType::MouseExited => fl::K_LEAVE,
        _ => fl::K_OTHER,
    }
}

#[cfg(feature = "framelog")]
thread_local! {
    static HARNESS: RefCell<Option<Box<dyn FnOnce(&bough_metal::app::Host)>>> = const { RefCell::new(None) };
}

/// A measurement harness (feature `framelog`): `f` runs once on the main thread when an interactive window
/// (`run_interactive`, so `Application::run`) has shown its first frame. Set it before `Application::run`.
#[cfg(feature = "framelog")]
pub fn harness(f: impl FnOnce(&bough_metal::app::Host) + 'static) {
    HARNESS.with(|h| *h.borrow_mut() = Some(Box::new(f)));
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
                let drew = a.frame_paced();
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
            let drew = a.frame_paced();
            if !drew {
                link.setPaused(true);
            }
            after_frame(app, &trim, &mut a, drew);
        }
    })
}

// =============================================================================== interactive windows

use super::pace::{self, In};
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

/// The inputs of one AppKit event (window coordinates, top-left origin), in order; none if it is not input.
pub fn inputs_of(e: &NSEvent, content_h: f64, mut f: impl FnMut(In)) {
    let loc = e.locationInWindow();
    let (x, y) = (loc.x as f32, (content_h - loc.y) as f32);
    match e.r#type() {
        NSEventType::MouseMoved | NSEventType::LeftMouseDragged | NSEventType::RightMouseDragged | NSEventType::OtherMouseDragged => f(In::Move { x, y }),
        NSEventType::LeftMouseDown => f(In::Down { x, y, button: Button::Primary }),
        NSEventType::LeftMouseUp => f(In::Up { x, y, button: Button::Primary }),
        NSEventType::RightMouseDown => f(In::Down { x, y, button: Button::Secondary }),
        NSEventType::RightMouseUp => f(In::Up { x, y, button: Button::Secondary }),
        NSEventType::MouseExited => f(In::Leave),
        NSEventType::ScrollWheel => f(In::Wheel { x, y, dy: -e.scrollingDeltaY() as f32, precise: e.hasPreciseScrollingDeltas() }),
        NSEventType::KeyDown => {
            let mods = mods_of(e);
            for key in key_of(e) {
                f(In::Key { key, mods });
            }
        }
        _ => {}
    }
}

/// Deliver one AppKit event to the app at once (window coordinates, top-left origin). Returns whether it
/// was input. (The interactive host queues instead: `run_interactive`.)
pub fn dispatch_event<V: View>(a: &mut App<V>, e: &NSEvent, content_h: f64) -> bool {
    let mut any = false;
    inputs_of(e, content_h, |i| {
        pace::apply(a, i);
        any = true;
    });
    any
}

/// Window resize (live or not): every size step relays the view out (`App::resize`) and draws a full frame
/// at the new size into surfaces of that size; the publisher commits it with the layer's new frame in one
/// transaction (bough-metal `Fused::resize`), so no frame is dropped and none is shown stretched.
fn observe_resize<V: View>(host: &bough_metal::app::Host, app: &Rc<RefCell<App<V>>>, trim: &Rc<Trim>, content_h: &Rc<Cell<f64>>, pacer: &Rc<Pacer>) {
    use objc2_foundation::{NSNotification, NSNotificationCenter};
    let (app, trim, content_h, window, pacer) = (app.clone(), trim.clone(), content_h.clone(), host.window.clone(), pacer.clone());
    let blk = RcBlock::new(move |_n: NonNull<NSNotification>| {
        let size = window.contentView().map_or(window.frame().size, |v| v.frame().size);
        content_h.set(size.height);
        if let Ok(mut a) = app.try_borrow_mut() {
            // the inputs queued before the resize go first
            pace::drain(&pacer.q, &mut a);
            fl::turn_begin(fl::K_RESIZE, 0);
            a.resize(size.width as f32, size.height as f32);
            let drew = a.frame();
            after_frame(&app, &trim, &mut a, drew);
            fl::turn_end();
            if drew {
                pacer.presented();
            }
        }
    });
    // SAFETY: AppKit posts window notifications on the main thread, where the app lives, and the block runs
    // synchronously there; the observer and the block live as long as the process (forgotten).
    let token = unsafe {
        NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
            Some(objc2_app_kit::NSWindowDidResizeNotification),
            Some(&host.window),
            None, // synchronously, as AppKit posts it (also inside live resize's tracking loop)
            &blk,
        )
    };
    std::mem::forget(token);
    std::mem::forget(blk);
}

/// The interactive host's frame pacer (main thread only): docs/lanes/pacing/cards/2026-11-11-display-paced-input.md.
///
/// * Input is queued by the event monitor and drained when the run loop is about to wait (`pump`): one turn
///   per queued item, as soon as the main thread is free (state is current; only frames wait).
/// * A drain that left damage presents at once if the display link is idle and the last present is at least
///   one refresh old: sparse input keeps frame-per-event's latency and cost (no link wake, RESULTS 12.2).
/// * Otherwise one latch is armed at T - `lead`, T the first refresh target whose latch is still ahead (the
///   link's last target, or the vsync phase it left, extrapolated): the latch drains the queue again, steps
///   the input animations to T (they are sampled for the moment they are shown) and presents through the
///   publisher thread. So at most one present per refresh and input sampled as late as the deadline allows.
/// * The display link runs while input or animation keeps coming: it keeps the phase and arms the latch
///   for animations. After `IDLE_CALLBACKS` callbacks with nothing pending or animating it pauses: idle is
///   zero wakeups.
struct Pacer {
    q: RefCell<pace::InputQueue>,
    /// Damage waits for the latch.
    pending: Cell<bool>,
    link_on: Cell<bool>,
    /// Host-clock seconds (`CACurrentMediaTime`) of the last present.
    last_present: Cell<f64>,
    /// The refresh period (the link's target - timestamp; at first 1 / the screen's maximum rate).
    period: Cell<f64>,
    /// How long before the target vsync the latch runs: the timer's wake, the frame's CPU part, the GPU, the
    /// publisher's wake and the commit. Adaptive: the largest of the last `SAMPLES` measured spans from a
    /// latch's scheduled time to its commit's end (so a loaded machine latches earlier, a quiet one later).
    lead: Cell<f64>,
    /// `BOUGH_LEAD_US` (frame-log builds): a fixed lead, for comparison.
    fixed_lead: bool,
    samples: RefCell<[f64; SAMPLES]>,
    n_samples: Cell<usize>,
    /// The last latch present not yet sampled: (its scheduled time, when it started).
    probe: Cell<Option<(f64, f64)>>,
    /// The armed latch's scheduled time.
    latch_at: Cell<f64>,
    /// The link's last target vsync (the refresh phase), host seconds.
    last_target: Cell<Option<f64>>,
    /// The target the animations were last stepped to.
    anim_target: Cell<Option<f64>>,
    idle_callbacks: Cell<u32>,
    latch: RefCell<Option<objc2::rc::Retained<NSTimer>>>,
    /// The target the armed latch presents for (`None`: not armed).
    latch_target: Cell<Option<f64>>,
    /// `BOUGH_NOFRAME`: turns only (drag_probe's "frames off" accounting).
    noframe: bool,
    /// Frame-log builds only (`BOUGH_LATCH=tick`): present at the link callback instead of a latch.
    at_tick: bool,
    /// Deadline commits (card 2026-11-11-deadline-commit): frames are drawn as soon as input is drained and
    /// the presenter commits at most once per refresh, at the deadline, on the phase this pacer gives it.
    /// Otherwise (X4) frames wait for the latch. Frame-log builds choose with `BOUGH_PACE=deadline|latch|
    /// deadline-tick`.
    deadline: bool,
    /// Deadline mode: input animations are stepped and drawn at the latch (`deadline-latch`, measured worse for
    /// discrete input), not at the link callback (the default).
    anim_at_latch: bool,
    /// The caret-blink timer: armed only while a field is focused (otherwise it would wake an idle window
    /// twice a second), and whether it is armed.
    blink: RefCell<Option<objc2::rc::Retained<NSTimer>>>,
    blink_on: Cell<bool>,
    blink_half: Cell<f64>,
}

/// Callbacks with nothing to do before the link pauses (hysteresis: a 240 Hz storm's gaps never pause it).
const IDLE_CALLBACKS: u32 = 3;
/// A latch closer than this is run at once.
const LATCH_MIN: f64 = 0.0002;
/// Latch-to-commit spans the adaptive lead keeps.
const SAMPLES: usize = 32;
/// The lead before any sample, and its bounds (the upper one: a refresh less 1 ms).
const LEAD_START: f64 = 0.003;
const LEAD_MIN: f64 = 0.0005;

impl Pacer {
    fn new(max_fps: f64) -> Pacer {
        let mut lead = LEAD_START;
        let mut at_tick = false;
        let mut fixed_lead = false;
        if fl::ON {
            if let Some(us) = std::env::var("BOUGH_LEAD_US").ok().and_then(|s| s.parse::<f64>().ok()) {
                lead = us * 1e-6;
                fixed_lead = true;
            }
            at_tick = std::env::var("BOUGH_LATCH").is_ok_and(|v| v == "tick");
        }
        let pace = if fl::ON { std::env::var("BOUGH_PACE").unwrap_or_default() } else { String::new() };
        let deadline = pace != "latch";
        let anim_at_latch = pace == "deadline-latch";
        Pacer {
            q: RefCell::new(pace::InputQueue::new()),
            pending: Cell::new(false),
            link_on: Cell::new(false),
            last_present: Cell::new(0.0),
            period: Cell::new(1.0 / max_fps.max(1.0)),
            lead: Cell::new(lead),
            fixed_lead,
            samples: RefCell::new([0.0; SAMPLES]),
            n_samples: Cell::new(0),
            probe: Cell::new(None),
            latch_at: Cell::new(0.0),
            last_target: Cell::new(None),
            anim_target: Cell::new(None),
            idle_callbacks: Cell::new(0),
            latch: RefCell::new(None),
            latch_target: Cell::new(None),
            noframe: std::env::var_os("BOUGH_NOFRAME").is_some(),
            at_tick,
            deadline,
            anim_at_latch,
            blink: RefCell::new(None),
            blink_on: Cell::new(false),
            blink_half: Cell::new(0.0),
        }
    }

    /// Arm the caret-blink timer while a field is focused, disarm it otherwise.
    fn sync_blink<V: View>(&self, a: &App<V>) {
        let want = a.next_blink().is_some();
        if want == self.blink_on.get() {
            return;
        }
        self.blink_on.set(want);
        if let Some(t) = self.blink.borrow().as_ref() {
            let d = if want { objc2_foundation::NSDate::dateWithTimeIntervalSinceNow(self.blink_half.get()) } else { objc2_foundation::NSDate::distantFuture() };
            t.setFireDate(&d);
        }
    }

    fn presented(&self) {
        self.last_present.set(objc2_quartz_core::CACurrentMediaTime());
    }

    fn start_link(&self, link: &objc2_quartz_core::CADisplayLink) {
        self.idle_callbacks.set(0);
        if !self.link_on.replace(true) {
            link.setPaused(false);
            fl::note(fl::N_LINK, 1, 0);
        }
    }

    fn stop_link(&self, link: &objc2_quartz_core::CADisplayLink) {
        if self.link_on.replace(false) {
            link.setPaused(true);
            self.anim_target.set(None);
            fl::note(fl::N_LINK, 0, 0);
        }
    }

    /// Arm the latch for the first target whose latch time is still ahead, from the refresh phase the link
    /// reported (extrapolated over at most a second). Returns false without a phase (the next callback arms).
    fn arm(&self, now: f64) -> bool {
        let Some(mut t) = self.last_target.get() else { return false };
        let p = self.period.get();
        if now - t > 1.0 {
            return false;
        }
        let lead = self.lead.get();
        if t - lead < now + LATCH_MIN {
            t += ((now + LATCH_MIN + lead - t) / p).ceil() * p;
        }
        if self.latch_target.get().is_some_and(|armed| armed <= t + 1e-4) {
            return true; // an earlier (or the same) latch presents it
        }
        self.latch_target.set(Some(t));
        self.latch_at.set(t - lead);
        fl::note(fl::N_LATCH, fl::secs_to_ns(t), fl::secs_to_ns(t - lead));
        if let Some(timer) = self.latch.borrow().as_ref() {
            timer.setFireDate(&objc2_foundation::NSDate::dateWithTimeIntervalSinceNow(t - lead - now));
        }
        true
    }

    /// Fold the last latch's span (scheduled time to commit end) into the adaptive lead, once its commit ended.
    fn sample<V: View>(&self, a: &App<V>) {
        let Some((at, start)) = self.probe.get() else { return };
        let Some(c) = a.last_commit_time() else { return };
        if c < start {
            return; // not committed yet (the publisher runs behind the main thread)
        }
        self.probe.set(None);
        if self.fixed_lead {
            return;
        }
        let n = self.n_samples.get();
        let mut s = self.samples.borrow_mut();
        s[n % SAMPLES] = c - at;
        self.n_samples.set(n + 1);
        let worst = s[..(n + 1).min(SAMPLES)].iter().copied().fold(0.0f64, f64::max);
        self.lead.set(worst.clamp(LEAD_MIN, (self.period.get() - 0.001).max(LEAD_MIN)));
    }

    /// Step the input animations to target `t` (they are sampled for the vsync that shows them).
    fn animate<V: View>(&self, a: &mut App<V>, t: f64) -> bool {
        let dt = self.anim_target.replace(Some(t)).map_or(self.period.get(), |l| (t - l).clamp(0.0, 0.05)) as f32;
        a.tick(dt)
    }

    /// Present now through the publisher thread.
    fn present<V: View>(&self, app: &Rc<RefCell<App<V>>>, a: &mut App<V>, trim: &Rc<Trim>) {
        self.pending.set(false);
        fl::turn_begin(fl::K_PACED, 0);
        let drew = a.frame();
        after_frame(app, trim, a, drew);
        fl::turn_end();
        if drew {
            self.presented();
        }
    }

    /// The run loop is about to wait: drain what AppKit delivered, then present or defer.
    fn pump<V: View>(&self, app: &Rc<RefCell<App<V>>>, link: &objc2_quartz_core::CADisplayLink, trim: &Rc<Trim>) {
        if self.q.borrow().is_empty() {
            return;
        }
        let Ok(mut a) = app.try_borrow_mut() else { return };
        pace::drain(&self.q, &mut a);
        self.sync_blink(&a);
        if self.noframe {
            if a.animating() {
                self.start_link(link);
            }
            return;
        }
        let now = objc2_quartz_core::CACurrentMediaTime();
        if self.deadline {
            // draw now; the presenter paces the commit. A second present within two refreshes is a burst:
            // the link runs to give the presenter the refresh phase.
            if a.has_damage() {
                let burst = now - self.last_present.get() < 2.0 * self.period.get();
                if burst && !self.link_on.get() {
                    // a burst begins: pace from this frame on, on the phase the link last reported
                    // (extrapolated over at most a second) until its first callback refreshes it
                    if let Some(t) = self.last_target.get().filter(|t| now - t < 1.0) {
                        let p = self.period.get();
                        let next = t + ((now - t) / p).ceil().max(0.0) * p;
                        a.set_refresh(next, p, 3.0 * p);
                    }
                }
                self.present(app, &mut a, trim);
                if burst {
                    self.start_link(link);
                }
            }
            if a.animating() {
                self.start_link(link);
                if self.anim_at_latch {
                    self.arm(now);
                }
            }
            return;
        }
        if a.has_damage() {
            if !self.link_on.get() && now - self.last_present.get() >= self.period.get() {
                // the first present of a refresh: from the event
                self.present(app, &mut a, trim);
            } else {
                self.pending.set(true);
            }
        }
        let animating = a.animating();
        if self.pending.get() || animating {
            self.start_link(link);
            if !self.at_tick {
                self.arm(now);
            }
        }
    }

    /// The display link's callback: keep the refresh phase; arm the latch while anything is pending or
    /// animating, else count towards pausing.
    fn on_vsync<V: View>(&self, app: &Rc<RefCell<App<V>>>, link: &objc2_quartz_core::CADisplayLink, trim: &Rc<Trim>) {
        let (v, t) = (link.timestamp(), link.targetTimestamp());
        if fl::ON {
            fl::vsync(v, t);
        }
        if t > v && t - v < 0.05 {
            self.period.set(t - v);
        }
        self.last_target.set(Some(t));
        let Ok(mut a) = app.try_borrow_mut() else { return };
        self.sample(&a);
        if self.noframe {
            fl::turn_begin(fl::K_TICK, 0);
            if !self.animate(&mut a, t) {
                self.stop_link(link);
            }
            fl::turn_end();
            return;
        }
        if self.deadline {
            a.set_refresh(t, self.period.get(), 3.0 * self.period.get());
            let now = objc2_quartz_core::CACurrentMediaTime();
            let mut busy = now - self.last_present.get() < 2.0 * self.period.get();
            if a.animating() {
                busy = true;
                if self.anim_at_latch {
                    self.arm(now);
                } else {
                    fl::turn_begin(fl::K_TICK, 0);
                    self.animate(&mut a, t);
                    fl::turn_end();
                }
            }
            if a.has_damage() {
                self.present(app, &mut a, trim);
                busy = true;
            }
            if busy {
                self.idle_callbacks.set(0);
            } else {
                self.idle_callbacks.set(self.idle_callbacks.get() + 1);
                if self.idle_callbacks.get() >= IDLE_CALLBACKS {
                    self.stop_link(link);
                }
            }
            return;
        }
        let busy = self.pending.get() || a.has_damage() || a.animating();
        if !busy {
            self.idle_callbacks.set(self.idle_callbacks.get() + 1);
            if self.idle_callbacks.get() >= IDLE_CALLBACKS {
                self.stop_link(link);
            }
            return;
        }
        self.idle_callbacks.set(0);
        if self.at_tick {
            // the comparison variant: animations and the frame at the callback
            fl::turn_begin(fl::K_TICK, 0);
            self.animate(&mut a, t);
            fl::turn_end();
            if self.pending.get() || a.has_damage() {
                self.present(app, &mut a, trim);
            }
            return;
        }
        let now = objc2_quartz_core::CACurrentMediaTime();
        self.arm(now);
    }

    /// The latch: the last inputs, the animations stepped to the target, then one frame.
    fn on_latch<V: View>(&self, app: &Rc<RefCell<App<V>>>, trim: &Rc<Trim>) {
        let Some(t) = self.latch_target.take() else { return };
        let Ok(mut a) = app.try_borrow_mut() else { return };
        self.sample(&a);
        let start = objc2_quartz_core::CACurrentMediaTime();
        pace::drain(&self.q, &mut a);
        self.sync_blink(&a);
        if a.animating() {
            fl::turn_begin(fl::K_TICK, 0);
            self.animate(&mut a, t);
            fl::turn_end();
        }
        if a.has_damage() {
            self.present(app, &mut a, trim);
            if self.probe.get().is_none() {
                self.probe.set(Some((self.latch_at.get(), start)));
            }
        }
        self.pending.set(false);
    }
}

thread_local! {
    static PUMP: RefCell<Option<Box<dyn Fn()>>> = const { RefCell::new(None) };
}

unsafe extern "C-unwind" fn before_waiting(_o: *mut objc2_core_foundation::CFRunLoopObserver, _a: objc2_core_foundation::CFRunLoopActivity, _i: *mut std::ffi::c_void) {
    PUMP.with(|p| {
        if let Some(f) = p.borrow().as_ref() {
            f();
        }
    });
}

/// Run `f` on the main thread whenever its run loop is about to wait (every mode: live resize, menus).
fn on_before_waiting(f: impl Fn() + 'static) {
    use objc2_core_foundation::{kCFRunLoopCommonModes, CFRunLoop, CFRunLoopActivity, CFRunLoopObserver};
    PUMP.with(|p| *p.borrow_mut() = Some(Box::new(f)));
    // SAFETY: a repeating observer with a plain callback and no context; it lives as long as the process.
    let obs = unsafe { CFRunLoopObserver::new(None, CFRunLoopActivity::BeforeWaiting.0, true, 0, Some(before_waiting), std::ptr::null_mut()) }.expect("run-loop observer");
    // SAFETY: the main run loop and the common modes constant.
    let modes = unsafe { kCFRunLoopCommonModes };
    CFRunLoop::main().expect("main run loop").add_observer(Some(&obs), modes);
    std::mem::forget(obs);
}

/// An interactive window: AppKit's mouse, wheel and key events are queued by a local event monitor (the
/// window accepts mouse-moved events) and run as input turns when the run loop is about to wait; frames
/// are paced by [`Pacer`] (at most one present per refresh; the first of a burst straight from the event,
/// the rest at a latch just before the vsync; input animations step on the display link, which pauses when
/// nothing is pending or animating). `start(host, app)` runs once the first frame is up (a probe installs
/// its timers there). After `seconds` (if > 0) `report` runs and the process exits.
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
    let mut state: Option<(Rc<RefCell<App<V>>>, Rc<Pacer>)> = None;
    let trim = Rc::new(Trim::default());
    bough_metal::app::run_sized(title, w, h, move |host, link| {
        if let Some((view, start, report)) = init.take() {
            let mut app = new_app(host, make_cfg, view);
            app.frame();
            let app = Rc::new(RefCell::new(app));
            host.window.setAcceptsMouseMovedEvents(true);
            // the display's own rate for input motion (ProMotion: the link otherwise follows the system's choice)
            let max_fps = host.window.screen().map_or(120, |s| s.maximumFramesPerSecond()) as f32;
            link.setPreferredFrameRateRange(objc2_quartz_core::CAFrameRateRange { minimum: max_fps, maximum: max_fps, preferred: max_fps });
            let pacer = Rc::new(Pacer::new(max_fps as f64));
            pacer.presented();
            // the content height flips AppKit's coordinates; it follows the window (live resize)
            let content_h = Rc::new(Cell::new(h));
            observe_resize(host, &app, &trim, &content_h, &pacer);
            {
                // the monitor only queues: the turns run when the run loop is about to wait
                let (pacer, content_h) = (pacer.clone(), content_h.clone());
                let blk = RcBlock::new(move |e: NonNull<NSEvent>| -> *mut NSEvent {
                    // SAFETY: AppKit hands the monitor a live event for the duration of the call.
                    let ev = unsafe { e.as_ref() };
                    let t = if fl::ON { fl::secs_to_ns(ev.timestamp()) } else { 0 };
                    if fl::ON {
                        let k = turn_kind(ev);
                        let seq = if k == fl::K_KEY || k == fl::K_WHEEL { 0 } else { ev.eventNumber() as u64 };
                        fl::event(k, t, seq, 0);
                    }
                    let mut q = pacer.q.borrow_mut();
                    inputs_of(ev, content_h.get(), |i| q.push(i, t));
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
            {
                let (app, pacer, trim, host2) = (app.clone(), pacer.clone(), trim.clone(), host.clone());
                on_before_waiting(move || pacer.pump(&app, &host2.link, &trim));
            }
            {
                // the latch: one reusable timer, armed per refresh by `on_vsync` (a repeating timer whose next
                // fire date is set; between latches it waits a year)
                let (app, trim, weak) = (app.clone(), trim.clone(), Rc::downgrade(&pacer));
                let blk = RcBlock::new(move |_t: NonNull<NSTimer>| {
                    if let Some(p) = weak.upgrade() {
                        p.on_latch(&app, &trim);
                    }
                });
                let year = 365.0 * 86400.0;
                let t = unsafe { NSTimer::timerWithTimeInterval_repeats_block(year, true, &blk) };
                t.setTolerance(0.0);
                unsafe { NSRunLoop::mainRunLoop().addTimer_forMode(&t, objc2_foundation::NSRunLoopCommonModes) };
                *pacer.latch.borrow_mut() = Some(t);
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
            {
                // the caret blink: a flip every half period while a field is focused (paint writes: after two
                // frames the ring's surfaces hold both states and a flip costs no GPU work). A clock source: its
                // frame is paced, unless the display link is running, when the latch publishes it with the rest
                let (app, trim, pacer) = (app.clone(), trim.clone(), pacer.clone());
                let half = app.borrow().rt.input.blink_half as f64;
                if half > 0.0 {
                    let (pacer2, app2) = (pacer.clone(), app.clone());
                    let blk = RcBlock::new(move |_t: NonNull<NSTimer>| {
                        let app = &app2;
                        if let Ok(mut a) = app.try_borrow_mut() {
                            fl::turn_begin(fl::K_TIMER, 0);
                            pacer2.sync_blink(&a);
                            if a.next_blink().is_some() && a.tick(half as f32) {
                                if pacer2.link_on.get() {
                                    if a.has_damage() {
                                        pacer2.pending.set(true);
                                        pacer2.arm(objc2_quartz_core::CACurrentMediaTime());
                                    }
                                } else {
                                    let drew = a.frame_paced();
                                    after_frame(app, &trim, &mut a, drew);
                                    if drew {
                                        pacer2.presented();
                                    }
                                }
                            }
                            fl::turn_end();
                        }
                    });
                    let t = unsafe { NSTimer::timerWithTimeInterval_repeats_block(half, true, &blk) };
                    t.setTolerance(0.01);
                    // disarmed until a field is focused (`Pacer::sync_blink`)
                    t.setFireDate(&objc2_foundation::NSDate::distantFuture());
                    unsafe { NSRunLoop::mainRunLoop().addTimer_forMode(&t, objc2_foundation::NSRunLoopCommonModes) };
                    pacer.blink_half.set(half);
                    *pacer.blink.borrow_mut() = Some(t);
                    pacer.sync_blink(&app.borrow());
                }
            }
            start(host, app.clone());
            #[cfg(feature = "framelog")]
            if let Some(h) = HARNESS.with(|h| h.borrow_mut().take()) {
                h(host);
            }
            link.setPaused(true);
            state = Some((app, pacer));
            return;
        }
        if let Some((app, pacer)) = &state {
            pacer.on_vsync(app, link, &trim);
        }
    })
}
