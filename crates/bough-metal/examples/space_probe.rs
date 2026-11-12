//! Where does a benchmark window go, and does its display link still tick there? Opens the bench window
//! (`app::run_sized`, placed by bench/tools/space under `BOUGH_BENCH_SPACE`), lets it settle 0.25 s, counts the
//! view's display-link ticks over `PROBE_MS` (1000), then closes the window and exits. Prints the placement, the
//! ticks per second, the display's maximum refresh rate, and whether the window was ever key or the app active.
//!
//!   BOUGH_BENCH_SPACE=other cargo run --profile gate -p bough-metal --example space_probe
use objc2_app_kit::{NSApplication, NSScreen};
use std::time::Instant;

fn main() {
    let probe_ms: u64 = std::env::var("PROBE_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1000);
    let t0 = Instant::now();
    let mut start: Option<(Instant, u32)> = None;
    let mut ticks = 0u32;
    let mut ever_key = false;
    // a timer ends the run even when the display link never ticks (off every display)
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(250 + probe_ms + 1500));
        let p = bough_metal::app::placement().cloned();
        println!(
            "space_probe mode={:?} ticks_per_s=0 (the display link never ticked) placed={p:?}",
            p.as_ref().map(|p| p.mode)
        );
        std::process::exit(0);
    });
    bough_metal::app::run_sized("space_probe", 480.0, 320.0, move |host, _link| {
        ticks += 1;
        ever_key |= host.window.isKeyWindow();
        let now = Instant::now();
        if start.is_none() && now.duration_since(t0).as_millis() >= 250 {
            start = Some((now, ticks));
        }
        if let Some((s, n0)) = start {
            let dt = now.duration_since(s);
            if dt.as_millis() as u64 >= probe_ms {
                let app = NSApplication::sharedApplication(host.mtm);
                let hz = NSScreen::mainScreen(host.mtm).map_or(0, |s| s.maximumFramesPerSecond());
                let p = bough_metal::app::placement().cloned();
                let on = bough_space::on_screen(host.window.windowNumber());
                println!(
                    "space_probe mode={:?} ticks_per_s={:.1} display_max_fps={hz} window_screen={} on_screen_now={on} ever_key={ever_key} app_active={} placed={p:?}",
                    p.as_ref().map(|p| p.mode),
                    (ticks - n0) as f64 / dt.as_secs_f64(),
                    host.window.screen().is_some(),
                    app.isActive(),
                );
                host.close();
                std::process::exit(0);
            }
        }
    })
}
