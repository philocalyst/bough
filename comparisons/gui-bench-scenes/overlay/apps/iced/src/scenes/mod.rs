//! The heavy scenes in iced: a spreadsheet and a design canvas, each an Elm application (state, `Message`,
//! `update`, `view`) over the shared spec (`crate::scenes`). Their window is the undecorated 1280x800 one of every
//! gui-bench scenario; the theme (light or dark) comes from the state through `Application::theme`.

mod canvas;
mod sheet;

use iced::{Color, Size, window};

use crate::scenes::{Scene, WINDOW};

/// Runs the scene the command line named.
pub fn run(scene: Scene) -> iced::Result {
    match scene {
        Scene::Sheet { rows } => sheet::run(rows, scene.title()),
        Scene::Canvas { nodes } => canvas::run(nodes, scene.title()),
    }
}

/// The color of `0xrrggbb`.
pub fn rgb(hex: u32) -> Color {
    let (r, g, b) = crate::scenes::rgb(hex);
    Color::from_rgb8(r, g, b)
}

/// The color of `0xaarrggbb`.
pub fn argb(hex: u32) -> Color {
    let (r, g, b) = crate::scenes::rgb(hex);
    Color::from_rgba8(r, g, b, (hex >> 24) as f32 / 255.0)
}

/// The undecorated 1280x800 window of a scene.
pub fn window() -> window::Settings {
    let mut window = crate::page::window(crate::APP_ID);
    window.size = Size::new(WINDOW.0, WINDOW.1);
    window
}

/// Which keys the scenes bind: `t` toggles the theme, and each scene has one more.
pub fn key_of(event: iced::keyboard::Event) -> Option<String> {
    match event {
        iced::keyboard::Event::KeyPressed {
            key: iced::keyboard::Key::Character(c),
            ..
        } => Some(c.to_string()),
        _ => None,
    }
}
