//! The heavy scenes (sheet, canvas) in GPUI: one window per run that shows the scene the command line names.
//! Each scene is a view (an entity that implements `Render`), a key context with actions for `t` / `e` / `m`
//! and a root that holds the focus, as the table scenario does for its function keys.

mod canvas;
mod sheet;
#[path = "../../../shared/scenes.rs"]
mod spec;

use gpui_kit::*;

pub use self::spec::{Scene, from_env};

actions!(scene, [ToggleTheme, ToggleEdit, ToggleMove]);

/// The key context of a scene's root.
pub const CONTEXT: &str = "Scene";

/// Binds the scene keys (the spec's `t`, `e` and `m`) to their actions.
fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new(spec::THEME_KEY, ToggleTheme, Some(CONTEXT)),
        KeyBinding::new(spec::sheet::EDIT_KEY, ToggleEdit, Some(CONTEXT)),
        KeyBinding::new(spec::canvas::MOVE_KEY, ToggleMove, Some(CONTEXT)),
    ]);
}

/// `0xrrggbb` as a colour.
pub fn c(color: u32) -> Rgba {
    rgb(color)
}

/// `0xaarrggbb` as a colour.
pub fn ca(color: u32) -> Rgba {
    rgba(color << 8 | color >> 24)
}

/// Opens the window of `scene` and runs the application.
pub fn run(scene: Scene, name: &'static str, app_id: &'static str) {
    eprintln!("{name}: scene {scene:?}");
    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);
        bind_keys(cx);
        let options = WindowOptions {
            titlebar: Some(TitlebarOptions {
                title: Some(scene.title().into()),
                ..Default::default()
            }),
            app_id: Some(app_id.into()),
            window_decorations: Some(WindowDecorations::Server),
            ..Default::default()
        };
        let opened = match scene {
            Scene::Sheet { rows } => gpui_kit::open_window(options, cx, |window, cx| {
                cx.new(|cx| sheet::Sheet::new(rows, window, cx))
            })
            .map(drop),
            Scene::Canvas { nodes } => gpui_kit::open_window(options, cx, |window, cx| {
                cx.new(|cx| canvas::Canvas::new(nodes, window, cx))
            })
            .map(drop),
            other => panic!("{other:?}: not ported to this framework yet"),
        };
        opened.expect("failed to open window");
        cx.activate(true);
    });
}
