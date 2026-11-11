//! The pacing lane's in-process probe (docs/lanes/pacing): AWAGY's dashboard and scroll workloads, the
//! bough-awagy apps unchanged and run through `Application::run`, driven by the harness's input schedule
//! (bench/awagy/tools/awagy_input.swift) posted into this process's own event queue from a driver thread.
//! Nothing reaches the window server: the user's pointer and keyboard are untouched and no permission is
//! needed. The window floats (never key, never active) so the window server does not throttle it, the
//! process opts out of App Nap, and the window closes when the run ends. The frame log (feature
//! `framelog`) is written to `BOUGH_FRAMELOG`.
//!
//!   pace_probe dashboard [rate=240] [phase_s=2] [warm_s=1] [tail_s=0]
//!   pace_probe scroll [rows=1000] [rate=144] [secs=5.5] [tail_s=0]
//!   pace_probe idle [secs=5]
#[path = "../dashboard.rs"]
#[allow(dead_code)]
mod dashboard;
#[path = "../scroll.rs"]
#[allow(dead_code)]
mod scroll;
#[path = "../scroll_config.rs"]
mod scroll_config;

use bough::backend::drive::{self, EvKind, Flags, Target, framelog as fl};
use bough::backend::host::harness;
use bough::prelude::*;

fn arg(i: usize, d: f64) -> f64 {
    std::env::args()
        .nth(i)
        .and_then(|s| s.parse().ok())
        .unwrap_or(d)
}

fn ms(x: f64) -> u64 {
    (x * 1e6) as u64
}

/// Idle accounting: process wakeups and instructions over `secs` of a window that shows a static frame.
fn idle_window(label: &'static str, secs: f64) {
    let (w0, i0) = drive::wakeups();
    fl::note(fl::N_WAKEUPS, w0, i0);
    std::thread::sleep(std::time::Duration::from_secs_f64(secs));
    let (w1, i1) = drive::wakeups();
    fl::note(fl::N_WAKEUPS, w1, i1);
    eprintln!(
        "{label}: {secs:.1} s idle: {} process wakeups ({:.1}/s), {} instructions",
        w1 - w0,
        (w1 - w0) as f64 / secs,
        i1 - i0
    );
}

fn click(t: &Target, x: f64, y: f64, seq: &mut isize) {
    for k in [EvKind::MOVE, EvKind::DOWN, EvKind::UP] {
        drive::mouse(t, k, x, y, *seq);
        *seq += 1;
    }
}

/// awagy_input.swift `dashboard`: prepare (activate click, focus the field, select-all + delete, idle
/// `warm`), then four phases of `phase` seconds at `rate` events per second: typing (a / backspace),
/// Refresh clicks, toggle clicks, and a pointer move plus one wheel line (down for the first half).
fn dashboard_script(t: Target, rate: f64, phase: f64, warm: f64, tail: f64) {
    let mut seq = 1isize;
    drive::sleep_until(fl::now_ns() + ms(500.0));
    click(&t, 860.0, 24.0, &mut seq);
    std::thread::sleep(std::time::Duration::from_millis(150));
    click(&t, 200.0, 90.0, &mut seq);
    std::thread::sleep(std::time::Duration::from_millis(50));
    drive::key(&t, 0, "a", Flags::CMD);
    drive::key(&t, 51, "\u{7f}", Flags::NONE);
    std::thread::sleep(std::time::Duration::from_secs_f64(warm));
    let vsyncs = drive::VsyncRecorder::start();
    let started = fl::now_ns();
    let step = (1e9 / rate) as u64;
    for i in 0..4u64 {
        let ps = started + i * (phase * 1e9) as u64;
        let pe = ps + (phase * 1e9) as u64;
        fl::note(fl::N_PHASE, i, ps);
        let (mut next, mut n) = (ps, 0u64);
        loop {
            let now = fl::now_ns();
            if now >= pe {
                break;
            }
            if now >= next {
                match i {
                    0 => {
                        if n % 2 == 0 {
                            drive::key(&t, 0, "a", Flags::NONE);
                        } else {
                            drive::key(&t, 51, "\u{7f}", Flags::NONE);
                        }
                    }
                    1 => click(&t, 496.0, 90.0, &mut seq),
                    2 => click(&t, 704.0, 90.0, &mut seq),
                    _ => {
                        drive::mouse(&t, EvKind::MOVE, 500.0, 400.0, seq);
                        seq += 1;
                        let lines = if now < ps + (phase * 0.5e9) as u64 {
                            -1
                        } else {
                            1
                        };
                        let (at, _) = drive::wheel(&t, 500.0, 400.0, lines);
                        if n == 0 {
                            eprintln!(
                                "wheel event: location in window ({:.1}, {:.1}) (want (500, {}))",
                                at.x,
                                at.y,
                                t.content_h - 400.0
                            );
                        }
                    }
                }
                n += 1;
                next = (next + step).max(fl::now_ns());
            }
            drive::sleep_until(next.min(pe));
        }
        eprintln!("phase {i}: {n} events");
    }
    fl::note(fl::N_PHASE, 4, fl::now_ns());
    drop(vsyncs);
    if tail > 0.0 {
        idle_window("tail", tail);
    }
}

/// awagy_input.swift `scroll`: the pointer to the window's centre, then one wheel line every 1/rate s,
/// reversing every 0.5 s (down first), for `secs`.
fn scroll_script(t: Target, rate: f64, secs: f64, tail: f64) {
    let (x, y) = (500.0, 350.0);
    drive::sleep_until(fl::now_ns() + ms(500.0));
    drive::mouse(&t, EvKind::MOVE, x - 1.0, y, 1);
    std::thread::sleep(std::time::Duration::from_millis(50));
    drive::mouse(&t, EvKind::MOVE, x, y, 2);
    std::thread::sleep(std::time::Duration::from_millis(50));
    let vsyncs = drive::VsyncRecorder::start();
    let start = fl::now_ns();
    fl::note(fl::N_PHASE, 0, start);
    let end = start + (secs * 1e9) as u64;
    let step = (1e9 / rate) as u64;
    let (mut next, mut n) = (start, 0u64);
    loop {
        let now = fl::now_ns();
        if now >= end {
            break;
        }
        if now >= next {
            let half = ((now - start) / 500_000_000) % 2;
            drive::wheel(&t, x, y, if half == 0 { -1 } else { 1 });
            n += 1;
            next += step;
            if next < now {
                next = now + step;
            }
        }
        drive::sleep_until(next.min(end));
    }
    fl::note(fl::N_PHASE, 1, fl::now_ns());
    drop(vsyncs);
    eprintln!("scroll: {n} wheel events");
    if tail > 0.0 {
        idle_window("tail", tail);
    }
}

fn main() {
    let what = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "dashboard".into());
    drive::no_app_nap();
    match what.as_str() {
        "dashboard" => {
            let (rate, phase, warm, tail) = (arg(2, 240.0), arg(3, 2.0), arg(4, 1.0), arg(5, 0.0));
            harness(move |host| {
                drive::float(host);
                let t = Target::of(host);
                drive::spawn_driver(move || dashboard_script(t, rate, phase, warm, tail));
                drive::exit_after(host, 0.5 + 0.25 + warm + 4.0 * phase + 0.6 + tail);
            });
            Application::run(
                Window::document(dashboard::app())
                    .title("bough Dashboard")
                    .size(1000.0, 700.0),
            );
        }
        "scroll" => {
            let (rows, rate, secs, tail) =
                (arg(2, 1000.0), arg(3, 144.0), arg(4, 5.5), arg(5, 0.0));
            // scroll_config reads SCROLL_ROWS once; set it before the view is made
            // SAFETY: single-threaded here (no other thread exists yet).
            unsafe { std::env::set_var("SCROLL_ROWS", (rows as usize).to_string()) };
            harness(move |host| {
                drive::float(host);
                let t = Target::of(host);
                drive::spawn_driver(move || scroll_script(t, rate, secs, tail));
                drive::exit_after(host, 0.5 + 0.1 + secs + 0.6 + tail);
            });
            Application::run(
                Window::document(scroll::app())
                    .title("bough Scroll")
                    .size(1000.0, 700.0),
            );
        }
        "idle" => {
            let secs = arg(2, 5.0);
            harness(move |host| {
                drive::float(host);
                drive::spawn_driver(move || {
                    std::thread::sleep(std::time::Duration::from_secs(1));
                    idle_window("idle", secs);
                });
                drive::exit_after(host, 1.0 + secs + 0.5);
            });
            Application::run(
                Window::document(dashboard::app())
                    .title("bough Dashboard")
                    .size(1000.0, 700.0),
            );
        }
        _ => {
            eprintln!("usage: pace_probe dashboard|scroll|idle ...");
            std::process::exit(2);
        }
    }
}
