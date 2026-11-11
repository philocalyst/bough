//! The memory-ledger harness (lane memory, round 6): one of the workloads, run exactly as its binary runs it
//! (`Application::run`'s window, configuration and input path, through `Application::run_harness`), with the
//! counting allocator installed and an in-process driver:
//!
//!   ledger <counter|scroll|dashboard> [active seconds = 6] [total seconds = 10]
//!
//! * scroll / dashboard: `App::wheel_points` at 120 Hz (the path a precise wheel event takes after the event
//!   monitor; no synthetic system input), 30 pt per tick, reversing at the ends; counter: one `inc` turn every
//!   100 ms. Each turn is followed by `App::frame`, as the host's event monitor does.
//! * After the active phase: nothing, then the host's calm trim (`App::trim(false)`) 1 s later, as the host's
//!   trim timer does after a second without frames.
//! * `BOUGH_LEDGER=<file>` and `BOUGH_LEDGER_AT=<s,...>` (the host) write the ledger lines; this harness also
//!   prints, on exit, the allocation counts of the active phase's steady part (frames 60.. of the drive).
//!
//! The window: Accessory activation policy and `orderFrontRegardless` (bough-metal `app::run_sized`): it never
//! takes focus; it closes when the run ends.
#[global_allocator]
static ALLOC: Tracing = Tracing;

/// The counting allocator, plus (`LEDGER_TRACE=1`) a backtrace of every allocation made during the steady part
/// of the drive, grouped by stack: what the steady-state zero-allocation gate must remove.
struct Tracing;
static TRACE: AtomicBool = AtomicBool::new(false);
static TRACES: Mutex<Vec<(String, usize, usize)>> = Mutex::new(Vec::new());
thread_local! {
    static IN: Cell<bool> = const { Cell::new(false) };
}

fn note(n: usize) {
    if !TRACE.load(Relaxed) {
        return;
    }
    let _ = IN.try_with(|f| {
        if f.get() {
            return;
        }
        f.set(true);
        let bt = std::backtrace::Backtrace::force_capture().to_string();
        // the frames in this repository's crates, innermost first (skip the allocator and std)
        let key: String = bt
            .lines()
            .filter(|l| l.contains("bough") && !l.contains("ledger::note") && !l.contains("GlobalAlloc"))
            .filter(|l| !l.trim_start().starts_with("at "))
            .take(6)
            .map(|l| l.trim())
            .collect::<Vec<_>>()
            .join(" <- ");
        if let Ok(mut t) = TRACES.lock() {
            match t.iter_mut().find(|e| e.0 == key) {
                Some(e) => {
                    e.1 += 1;
                    e.2 += n;
                }
                None => t.push((key, 1, n)),
            }
        }
        f.set(false);
    });
}

// SAFETY: forwards every call to the counting allocator with the caller's layout; `note` only reads a flag
// and (when tracing) records a backtrace under a reentrancy guard.
unsafe impl GlobalAlloc for Tracing {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        note(l.size());
        // SAFETY: forwarded contract.
        unsafe { bough::ledger::Counting.alloc(l) }
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        note(l.size());
        // SAFETY: forwarded contract.
        unsafe { bough::ledger::Counting.alloc_zeroed(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        // SAFETY: forwarded contract.
        unsafe { bough::ledger::Counting.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, new: usize) -> *mut u8 {
        note(new);
        // SAFETY: forwarded contract.
        unsafe { bough::ledger::Counting.realloc(p, l, new) }
    }
}

#[path = "../../../crates/bough/examples/counter.rs"]
#[allow(dead_code)]
mod counter;
#[path = "../src/dashboard.rs"]
#[allow(dead_code)]
mod dashboard;
#[path = "../src/scroll.rs"]
#[allow(dead_code)]
mod scroll;
#[path = "../src/scroll_config.rs"]
mod scroll_config;

use bough::app::{App, View};
use bough::prelude::*;
use std::alloc::{GlobalAlloc, Layout};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};

/// Steady-state allocation counts: (allocations at drive tick 60, at the last drive tick, ticks between).
#[derive(Default, Clone, Copy)]
struct Steady {
    a0: usize,
    a1: usize,
    t0: u64,
    t1: u64,
}

use bough::backend::host::timer;

fn drive<V: View>(app: Rc<RefCell<App<V>>>, active: f64, step: impl Fn(&mut App<V>, u64) + 'static, steady: Rc<Cell<Steady>>) {
    let t0 = std::time::Instant::now();
    let tick = Rc::new(Cell::new(0u64));
    let done = Rc::new(Cell::new(false));
    {
        let (app, tick, done, steady) = (app.clone(), tick.clone(), done.clone(), steady.clone());
        timer(1.0 / 120.0, true, move || {
            if done.get() {
                return;
            }
            if t0.elapsed().as_secs_f64() >= active {
                done.set(true);
                TRACE.store(false, Relaxed);
                let mut s = steady.get();
                s.a1 = bough::ledger::count::allocs();
                s.t1 = tick.get();
                steady.set(s);
                return;
            }
            let Ok(mut a) = app.try_borrow_mut() else { return };
            let k = tick.get();
            if k == 60 {
                let mut s = steady.get();
                s.a0 = bough::ledger::count::allocs();
                s.t0 = k;
                steady.set(s);
                TRACE.store(std::env::var_os("LEDGER_TRACE").is_some(), Relaxed);
            }
            step(&mut a, k);
            a.frame();
            tick.set(k + 1);
        });
    }
    // the host's calm trim, one second after the last frame
    timer(active + 1.0, false, move || {
        if let Ok(mut a) = app.try_borrow_mut() {
            a.trim(false);
        }
    });
}

fn wheel<V: View>(a: &mut App<V>, k: u64) {
    // down for 300 ticks, then up, and so on: 30 pt per tick at 120 Hz
    let dir = if (k / 300) % 2 == 0 { 1.0 } else { -1.0 };
    a.wheel_points(500.0, 500.0, 30.0 * dir);
}

fn report<V: View>(steady: &Rc<Cell<Steady>>) -> impl FnOnce(&mut App<V>) + 'static {
    let steady = steady.clone();
    move |a| {
        let s = steady.get();
        let p = bough::ledger::Proc::now();
        eprintln!(
            "ledger-harness: frames {} steady ticks {}..{} allocations {} ({:.2}/tick); rust live {} B, allocs {}; footprint {} B peak {} B",
            a.frames_presented(),
            s.t0,
            s.t1,
            s.a1.saturating_sub(s.a0),
            s.a1.saturating_sub(s.a0) as f64 / (s.t1.saturating_sub(s.t0)).max(1) as f64,
            p.rust_live,
            p.rust_allocs,
            p.phys_footprint,
            p.phys_footprint_peak
        );
        if let Ok(t) = TRACES.lock() {
            let mut t = t.clone();
            t.sort_by(|a, b| b.1.cmp(&a.1));
            for (k, n, b) in t.iter().take(20) {
                eprintln!("  steady alloc x{n} ({b} B): {k}");
            }
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let what = args.get(1).map(String::as_str).unwrap_or("scroll").to_string();
    let active: f64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(6.0);
    let total: f64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(10.0);
    let steady = Rc::new(Cell::new(Steady::default()));
    match what.as_str() {
        "counter" => {
            let st = steady.clone();
            Application::run_harness(
                Window::document(counter::Counter { count: Signal::new(0) }).title("Counter"),
                total,
                move |_, app| {
                    drive(
                        app,
                        active,
                        |a, k| {
                            if k % 12 == 0 {
                                a.turn(|c| c.inc());
                            }
                        },
                        st,
                    )
                },
                report(&steady),
            )
        }
        "dashboard" => {
            let st = steady.clone();
            Application::run_harness(dashboard::window(), total, move |_, app| drive(app, active, wheel, st), report(&steady))
        }
        _ => {
            let st = steady.clone();
            Application::run_harness(scroll::window(), total, move |_, app| drive(app, active, wheel, st), report(&steady))
        }
    }
}
