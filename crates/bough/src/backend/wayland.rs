//! The Wayland host (Linux): bough-wayland's toplevel driven by one single-threaded loop, the CPU executor drawing
//! into its `wl_shm` buffers through [`Surfaces`].
//!
//! The loop, each pass: decode what the compositor sent (input becomes turns at once, through the portable input
//! API), run the timers that are due (a harness clock, the caret's blink), then draw if the epoch has damage and the
//! compositor is ready for a frame (no frame callback outstanding, not suspended), then wait in one `poll` for the
//! compositor or the next timer. Animations (glides, transitions, list moves) advance on frame callbacks, so they
//! run at the compositor's refresh and stop the moment nothing moves: an idle window wakes for nothing but its
//! timers and commits nothing.
//!
//! Damage: the façade computes, per present, the tiles where the presented buffer differs from the one on screen
//! (`App::draw_target`), and the shell sends exactly those as `damage_buffer` rectangles.
use super::{Run, Surfaces};
use crate::app::{App, Config, View};
use crate::input::{Button, Key, Mods};
use bough_wayland::{keysym, Event, Shell};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// The shell's buffer ring as the CPU executor's surfaces.
struct WlSurfaces(Rc<RefCell<Shell>>);

// SAFETY: `Shell::pixels(i)` is buffer `i` of the current memfd mapping (`h` rows of `4 w` bytes), valid until the
// next `resize_buffers` (`Surfaces::resize`); the compositor reads a buffer only between its attach and its release,
// which `in_use` reports.
unsafe impl Surfaces for WlSurfaces {
    fn count(&self) -> usize {
        self.0.borrow().count()
    }
    fn size(&self) -> (u32, u32) {
        self.0.borrow().size()
    }
    fn in_use(&self, i: usize) -> bool {
        let mut s = self.0.borrow_mut();
        if s.in_use(i) {
            // a release may be waiting on the socket: read it without blocking (the façade's ring waits for one
            // when every buffer is held, and nothing else would read the connection meanwhile)
            let _ = s.poll_now();
        }
        s.in_use(i)
    }
    fn pixels(&mut self, i: usize) -> (*mut u8, usize) {
        self.0.borrow_mut().pixels(i)
    }
    fn present(&mut self, i: usize, damage: &[u128], full: bool) {
        if let Err(e) = self.0.borrow_mut().present(i, damage, full) {
            eprintln!("bough: the compositor connection failed: {e}");
            std::process::exit(0);
        }
    }
    fn resize(&mut self, w: u32, h: u32) {
        self.0.borrow_mut().resize_buffers(w, h).expect("wl_shm buffers");
    }
}

/// bough's key for an xkb keysym (and the character it types). Function keys arrive as AppKit's function-key code
/// points (F1 = U+F704), as they do on macOS, so a key route reads them the same way on both platforms.
fn key_of(sym: u32, text: Option<char>) -> Option<Key> {
    use keysym::*;
    Some(match sym {
        BACKSPACE => Key::Backspace,
        TAB | ISO_LEFT_TAB => Key::Tab,
        RETURN | KP_ENTER => Key::Enter,
        ESCAPE => Key::Escape,
        DELETE => Key::Delete,
        LEFT => Key::Left,
        RIGHT => Key::Right,
        UP => Key::Up,
        DOWN => Key::Down,
        HOME => Key::Home,
        END => Key::End,
        PAGE_UP => Key::PageUp,
        PAGE_DOWN => Key::PageDown,
        F1..=F35 => Key::Char(char::from_u32(0xF704 + (sym - F1))?),
        _ => Key::Char(text?),
    })
}

/// Points one wheel detent scrolls (no glide: content moves at once, as gui-bench's spec asks of every framework).
const DETENT_PT: f32 = 15.0;

enum Kind {
    Clock,
    Exit,
    /// An animation frame when no frame callback will come (the last tick drew nothing).
    Anim,
}
struct Timer {
    at: Instant,
    period: Option<Duration>,
    kind: Kind,
}

/// Open the window and run until it closes (or `run.seconds` pass). See the module docs.
pub fn run<V: View>(title: &str, make_cfg: fn(f32) -> Config, view: V, run: Run<V>) -> ! {
    let Run { seconds, start, mut every, report, mut linked, app_id } = run;
    let probe = make_cfg(1.0);
    let (w, h, ring) = (probe.width_pt.round() as u32, probe.height_pt.round() as u32, probe.surfaces());
    drop(probe);
    let app_id = app_id.or_else(|| std::env::var("BOUGH_APP_ID").ok()).unwrap_or_else(|| "bough".into());
    let shell = match Shell::connect(title, &app_id, w, h, ring) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("bough: cannot open a Wayland window ({e}); is WAYLAND_DISPLAY set?");
            std::process::exit(1);
        }
    };
    let (lw, lh) = shell.logical();
    let scale = shell.scale() as f32;
    let mut cfg = make_cfg(scale);
    (cfg.width_pt, cfg.height_pt) = (lw as f32, lh as f32);
    let shell = Rc::new(RefCell::new(shell));
    let mut app = App::new(view, cfg, false);
    app.windowed = true;
    shell.borrow_mut().resize_buffers(app.rt.win_w as u32, app.rt.win_h as u32).expect("wl_shm buffers");
    app.target = Some(Box::new(WlSurfaces(shell.clone())));
    let t0 = Instant::now();
    if let Some(f) = linked.as_mut() {
        f(&mut app, 0.0);
    }
    app.frame();
    start(&mut app);
    let mut timers: Vec<Timer> = Vec::new();
    if let Some((p, _)) = &every {
        let d = Duration::from_secs_f64(*p);
        timers.push(Timer { at: t0 + d, period: Some(d), kind: Kind::Clock });
    }
    // the caret's blink: armed only while a field is focused (an unfocused window has no timer to wake for)
    let mut blink: Option<(Instant, f32)> = None;
    if seconds > 0.0 {
        timers.push(Timer { at: t0 + Duration::from_secs_f64(seconds), period: None, kind: Kind::Exit });
    }
    let mut report = Some(report);
    let mut last_tick: Option<Instant> = None;
    let mut scale_warned = false;
    loop {
        // 1. what the compositor sent
        let events = shell.borrow_mut().take_events();
        for e in events {
            match e {
                Event::Configure { w, h } => {
                    shell.borrow_mut().set_logical(w, h);
                    app.resize(w as f32, h as f32);
                }
                Event::Scale(s) => {
                    if (s as f32 - app.rt.scale).abs() > 1e-4 && !scale_warned {
                        scale_warned = true;
                        eprintln!("bough: the compositor asks for scale {s}; changing scale at run time is not built (the window keeps {})", app.rt.scale);
                    }
                }
                Event::Suspended(_) => {}
                Event::Close => finish(&mut app, &mut report),
                Event::Frame { .. } => {
                    if let Some(f) = linked.as_mut() {
                        f(&mut app, t0.elapsed().as_secs_f64());
                    }
                    if app.animating() {
                        let now = Instant::now();
                        let dt = last_tick.map_or(1.0 / 60.0, |l| now.duration_since(l).as_secs_f32().min(0.05));
                        last_tick = Some(now);
                        app.tick(dt);
                    } else {
                        last_tick = None;
                    }
                }
                Event::Motion { x, y } => app.pointer_move(x as f32, y as f32),
                Event::Leave => app.pointer_leave(),
                Event::Button { x, y, button, pressed } => {
                    let b = if button == bough_wayland::BTN_RIGHT { Button::Secondary } else { Button::Primary };
                    if pressed {
                        app.pointer_down(x as f32, y as f32, b);
                    } else {
                        app.pointer_up(x as f32, y as f32, b);
                    }
                }
                Event::Scroll { x, y, dy, finger, .. } => {
                    let d = if finger { dy as f32 } else { dy as f32 * DETENT_PT };
                    app.wheel_points(x as f32, y as f32, d);
                }
                Event::Key { sym, text, pressed: true, mods } => {
                    if let Some(k) = key_of(sym, text) {
                        app.key_down(k, Mods { shift: mods.shift, ctrl: mods.ctrl, alt: mods.alt, cmd: mods.logo });
                    }
                }
                Event::Key { .. } => {}
            }
        }
        // 2. timers that are due
        let now = Instant::now();
        for t in timers.iter_mut().filter(|t| t.at <= now) {
            match t.kind {
                Kind::Clock => {
                    if let Some((_, f)) = every.as_mut() {
                        f(&mut app);
                    }
                }
                Kind::Anim => {
                    let dt = last_tick.map_or(1.0 / 60.0, |l| now.duration_since(l).as_secs_f32().min(0.05));
                    last_tick = Some(now);
                    app.tick(dt);
                }
                Kind::Exit => finish(&mut app, &mut report),
            }
            match t.period {
                // the next period from the schedule, not from now (no drift), skipping missed ones
                Some(p) => {
                    while t.at <= now {
                        t.at += p;
                    }
                }
                None => t.at = now + Duration::from_secs(3600 * 24 * 365),
            }
        }
        timers.retain(|t| !matches!(t.kind, Kind::Anim) || t.at > now);
        if let Some((_, dt)) = blink.filter(|b| b.0 <= now) {
            blink = None;
            if app.next_blink().is_some() {
                app.tick(dt);
            }
        }
        if blink.is_none() {
            blink = app.next_blink().map(|s| (now + Duration::from_secs_f32(s.max(0.0)), s.max(0.0)));
        }
        // 3. draw when there is something to show and the compositor is ready for it
        let (pending, suspended) = {
            let s = shell.borrow();
            (s.frame_pending(), s.suspended())
        };
        if !suspended && !pending {
            if app.dirty() {
                app.frame();
            } else if app.animating() && !timers.iter().any(|t| matches!(t.kind, Kind::Anim)) {
                // an animation whose last tick drew nothing gets no frame callback: tick it on a timer instead
                timers.push(Timer { at: now + Duration::from_millis(16), period: None, kind: Kind::Anim });
            }
        }
        // 4. wait for the compositor or the next timer
        let next = timers.iter().map(|t| t.at).chain(blink.map(|b| b.0)).min();
        let timeout = next.map(|at| at.saturating_duration_since(Instant::now()));
        if let Err(e) = shell.borrow_mut().dispatch(timeout) {
            eprintln!("bough: the compositor connection closed: {e}");
            finish(&mut app, &mut report);
        }
    }
}

fn finish<V: View>(app: &mut App<V>, report: &mut Option<Box<dyn FnOnce(&mut App<V>)>>) -> ! {
    if let Some(r) = report.take() {
        r(app);
    }
    report_exit(app);
    std::process::exit(0);
}

/// The exit report (as on macOS): `BOUGH_STATS` prints the ring and executor counters, `BOUGH_CHECK` compares the
/// buffer last shown with a full redraw of the retained state, `BOUGH_DUMP=<file>` writes it (raw BGRA).
fn report_exit<V: View>(a: &mut App<V>) {
    if std::env::var_os("BOUGH_STATS").is_some() {
        eprintln!("surfaces: {:?}; cpu executor: {:?}", a.surf_stats, a.cpu.stats);
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
