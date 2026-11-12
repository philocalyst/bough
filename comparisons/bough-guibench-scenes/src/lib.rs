//! bough's editions of the heavy scenes in gui-bench (`comparisons/gui-bench-scenes`): the scene the command line
//! names (`spec::from_env`), in a window through bough's own host (Wayland on Linux), as an application would run
//! it: re-describing mode (`Config::once()` is the harness's switch, not an application's), the view focused so the
//! spec's keys reach it.
//!
//! * sheet: `bough_workloads::sheet::Sheet`, the standing workload itself (its data and keys follow the spec);
//! * canvas: [`canvas::Canvas`], drawn to the spec (the standing workload's canvas has another geometry).
#[path = "../../gui-bench-scenes/overlay/apps/shared/scenes.rs"]
pub mod spec;

pub mod canvas;

use bough::app::View;
use bough::raw::{App, Caps, Config};
use bough_workloads::Scene as _;
use std::sync::OnceLock;

/// The region capacities of the scene being run (`make_cfg` is a plain `fn`, so they travel through a static).
static CAPS: OnceLock<Caps> = OnceLock::new();

fn make_cfg(scale: f32) -> Config {
    let mut c = Config::new(spec::WINDOW.0, spec::WINDOW.1, scale, bough::ui::default_font());
    c.caps = *CAPS.get().expect("caps set before the window");
    // `BOUGH_ONCE=1`: mount once, wires only (the harness's switch, for diagnosis; never the reported default)
    if std::env::var("BOUGH_ONCE").is_ok_and(|v| v == "1") {
        c = c.once();
    }
    c
}

/// Focus the first focusable record (the scene's root, which holds the key route).
fn focus_root<V: View>(a: &mut App<V>) {
    if let Some(pos) = (0..a.record_count()).find(|&p| a.record_view(p).kind & 0x40 != 0) {
        let h = a.rt.bough.handle_at(pos);
        a.set_focus(h, false);
    }
}

fn window<V: View>(title: &str, view: V, caps: Caps) -> ! {
    let _ = CAPS.set(caps);
    let run = bough::backend::Run::default().app_id("gui-bench.bough").start(|a: &mut App<V>| focus_root(a));
    bough::backend::run(title, make_cfg, view, run)
}

/// Show `scene` until the window closes.
pub fn run(scene: spec::Scene) -> ! {
    eprintln!("bough-bench: {scene:?}");
    match scene {
        spec::Scene::Sheet { rows } => {
            let v = bough_workloads::sheet::Sheet::with_rows(rows);
            let caps = v.caps();
            window(scene.title(), v, caps)
        }
        spec::Scene::Canvas { nodes } => {
            let v = canvas::Canvas::new(nodes);
            let caps = v.caps();
            window(scene.title(), v, caps)
        }
    }
}
