//! Render a scene offscreen and dump the presented frame (raw BGRA, `OUT`), after comparing it with a
//! full redraw of the same retained state. SCENE=lab|animate|blink|state|vscroll TICKS SCALE (2).
//! lab: `ZGX_MODE` ticks of workload (mode both unless set); animate: clock = tick/60 (as zgx's
//! render_parity); vscroll: scroll = tick * 12 pt.
use bough::raw::*;
use bough_scenes::*;
use zgx_workload::{Mode, Workload};

fn env<T: std::str::FromStr>(name: &str, default: T) -> T {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

fn finish<V: View>(mut app: App<V>, out: &str, label: &str) {
    let px = app.presented_pixels();
    let reference = app.reference_pixels();
    let diff = px.iter().zip(&reference).filter(|(a, b)| a != b).count();
    println!("{label}: frames {} differing bytes vs full redraw: {diff}", app.frames);
    std::fs::write(out, &px).unwrap();
}

fn main() {
    let scene: String = env("SCENE", "lab".to_string());
    let ticks: usize = env("TICKS", 121);
    let scale: f32 = env("SCALE", 2.0);
    let out: String = env("OUT", format!("{scene}.bgra"));
    match scene.as_str() {
        "lab" => {
            let mut work = Workload::from_env();
            let mode = if std::env::var_os("ZGX_MODE").is_some() { work.mode } else { Mode::Both };
            work.mode = mode;
            let mut app = App::new(Lab::new(&work), lab_config(scale), true);
            app.frame();
            for _ in 0..ticks {
                work.tick();
                app.turn(|l| lab_publish(l, &work, mode));
                app.frame();
            }
            finish(app, &out, "lab");
        }
        "animate" => {
            let mut app = App::new(Anim { clock: Signal::new(0.0) }, anim_config(scale), true);
            app.frame();
            for t in 1..=ticks {
                app.turn(|a| {
                    a.clock.set(t as f32 / 60.0);
                });
                app.frame();
            }
            finish(app, &out, "animate");
        }
        "blink" => {
            let mut app = App::new(Blink { shown: Signal::new(true) }, blink_config(scale), true);
            app.frame();
            for _ in 0..ticks {
                app.turn(|b| {
                    let s = !*b.shown;
                    b.shown.set(s);
                });
                app.frame();
            }
            finish(app, &out, "blink");
        }
        "state" => {
            let mut app = App::new(MicroState { counter: Signal::new(0) }, micro_config("state", scale), true);
            app.frame();
            for t in 1..=ticks {
                app.turn(|s| {
                    s.counter.set(t as u64);
                });
                app.frame();
            }
            finish(app, &out, "state");
        }
        _ => {
            let mut app = App::new(MicroScroll { scroll: Signal::new(0.0) }, micro_config("vscroll", scale), true);
            app.frame();
            for t in 1..=ticks {
                app.turn(|s| {
                    s.scroll.set(t as f32 * 12.0);
                });
                app.frame();
            }
            finish(app, &out, "vscroll");
        }
    }
}
