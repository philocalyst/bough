//! The gui-bench edition of the canvas (`comparisons/bough-guibench-scenes/src/canvas.rs`, drawn to the shared spec
//! `guibench_spec::canvas`) as a standing workload, so the scene measured under fakeland on Linux has a Mac reference
//! in this harness (Metal, instructions per step). The view is that crate's source, included here.
//!
//! Phases: steady = the spec's edit (`m`: the selected node moves); hover = the pointer onto the layers panel's
//! "Component 3" row and up into the toolbar (1,000 instance fills at 5,000 nodes); theme = `t`; animate: none (the
//! spec's canvas has no transitions). Keys reach the root's `.on_key` route, which the scene focuses first, as the
//! gui-bench host does.
use crate::guibench_spec::canvas as sc;
use crate::Scene;
use bough::app::View;
use bough::prelude::{Key, Mods};
use bough::raw::{App, Caps};

#[path = "../../bough-guibench-scenes/src/canvas.rs"]
mod view;
pub use view::Canvas;

/// Focus the root (its `.on_key` route is the scene's only tab stop) with one Tab, once per process, as the
/// gui-bench host focuses it at start; the Tab is outside every measured step but the first.
fn focus<V: View>(a: &mut App<V>) {
    static FOCUSED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if !FOCUSED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        a.key_down(Key::Tab, Mods::default());
    }
}

fn key<V: View>(a: &mut App<V>, c: char) {
    focus(a);
    a.key_down(Key::Char(c), Mods::default());
}

impl Scene for Canvas {
    const NAME: &'static str = "gbcanvas";

    fn build(scale: f32) -> Self {
        Canvas::new(crate::scaled(5000, scale, sc::FRAMES * 5))
    }
    fn caps(&self) -> Caps {
        Canvas::caps(self)
    }
    fn describe(&self) -> String {
        format!("{} nodes in {} frames (the gui-bench spec's canvas)", self.nodes(), sc::FRAMES)
    }
    fn fanout(&self) -> usize {
        self.nodes() / 5
    }
    fn steady(app: &mut App<Self>, _k: usize) {
        key(app, 'm');
    }
    fn hover(app: &mut App<Self>, k: usize) {
        let y = if k % 2 == 0 { sc::component_row_y(2) } else { sc::TOOLBAR_H / 2.0 };
        app.pointer_move(120.0, y);
    }
    fn theme(app: &mut App<Self>, _k: usize) {
        key(app, 't');
    }
    fn animate(_app: &mut App<Self>, _k: usize) {}
}
