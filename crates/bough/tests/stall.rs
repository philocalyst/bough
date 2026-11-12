//! A multi-second process stall must not kill the presenter (lane memory's finding: a pre-armed batch waiting on its
//! shared event errors with kIOGPUCommandBufferCallbackErrorTimeout when the process is stopped for seconds, and
//! every later frame waited forever). A child shell stops this process (SIGSTOP), sleeps 3 s and continues it
//! (SIGCONT) while frames are being drawn; afterwards frames must keep coming and every one must equal a full
//! redraw of the retained state.
#[path = "../examples/counter.rs"]
mod counter;

use bough::input::Button;
use bough::raw::{App, Config};
use bough::signal::Signal;
use counter::Counter;

const SCALE: f32 = 2.0;

fn app() -> App<Counter> {
    let font = std::fs::read(
        std::env::var("ZGX_FONT_FILE")
            .unwrap_or_else(|_| "/System/Library/Fonts/Supplemental/Arial.ttf".into()),
    )
    .unwrap();
    let mut app = App::new(
        Counter {
            count: Signal::new(0),
        },
        Config::new(480.0, 320.0, SCALE, font),
        true,
    );
    app.frame();
    app
}

fn button_centre(app: &App<Counter>) -> (f32, f32) {
    let r = (0..app.record_count())
        .map(|p| app.record_view(p))
        .find(|r| r.kind & 0x80 != 0)
        .expect("an interactive record");
    let [x, y, w, h] = r.layout.expect("the button is a layout box");
    ((x + w / 2.0) / SCALE, (y + h / 2.0) / SCALE)
}

fn click(app: &mut App<Counter>, (x, y): (f32, f32)) {
    app.pointer_down(x, y, Button::Primary);
    app.pointer_up(x, y, Button::Primary);
    assert!(app.frame());
}

#[test]
fn a_three_second_sigstop_does_not_kill_the_stream() {
    let mut app = app();
    let at = button_centre(&app);
    for _ in 0..20 {
        click(&mut app, at);
    }
    assert_eq!(
        app.presented_pixels(),
        app.reference_pixels(),
        "before the stall"
    );
    let pid = std::process::id();
    let mut child = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(format!(
            "sleep 0.3; kill -STOP {pid}; sleep 3; kill -CONT {pid}"
        ))
        .spawn()
        .expect("the stopping child");
    // draw through the stall: the process stops somewhere in here for 3 s
    let t0 = std::time::Instant::now();
    let mut frames = 0;
    while t0.elapsed() < std::time::Duration::from_secs(5) {
        click(&mut app, at);
        frames += 1;
        std::thread::sleep(std::time::Duration::from_millis(4));
    }
    child.wait().unwrap();
    assert!(
        t0.elapsed() >= std::time::Duration::from_secs(5),
        "the stall happened inside the loop"
    );
    for _ in 0..20 {
        click(&mut app, at);
    }
    assert_eq!(
        app.presented_pixels(),
        app.reference_pixels(),
        "after the stall ({frames} frames drawn through it)"
    );
}

/// The stall shape that kills the stream (reproduced: frame 83 never finished, the pre-armed batch errored with
/// kIOGPUCommandBufferCallbackErrorTimeout): the process keeps running but draws nothing for longer than the
/// driver lets a submitted command buffer wait on its event (`BOUGH_STALL_SECS`, default 12 s), then draws again.
#[test]
fn a_long_idle_with_a_pre_armed_batch_recovers() {
    let mut app = app();
    let at = button_centre(&app);
    for _ in 0..20 {
        click(&mut app, at);
    }
    let secs: u64 = std::env::var("BOUGH_STALL_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(12);
    std::thread::sleep(std::time::Duration::from_secs(secs));
    for _ in 0..20 {
        click(&mut app, at);
    }
    assert_eq!(
        app.presented_pixels(),
        app.reference_pixels(),
        "after {secs} s without frames"
    );
}
