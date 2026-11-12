//! The heavy scenes under egui: the sheet and the canvas of `apps/shared/scenes.rs`.
//!
//! egui is immediate mode: each app owns plain data, `App::ui` builds the whole frame from it, and an
//! edit changes the data and the next pass shows it. Panels (`Panel`, `CentralPanel`) cut the window,
//! the theme is a pair of `Visuals` (one per `Theme`) plus the spec's colours, and keys are read from
//! the input of the pass.

mod canvas;
mod sheet;

use eframe::egui::{Color32, Context, Key, Theme, Visuals, vec2};

use crate::heavy::{self, Scene};
use crate::{font, window};

/// The egui colour of `0xrrggbb`.
pub const fn color(c: u32) -> Color32 {
    let (r, g, b) = heavy::rgb(c);
    Color32::from_rgb(r, g, b)
}

/// The egui colour of `0xaarrggbb` (alpha not premultiplied).
pub fn color_argb(c: u32) -> Color32 {
    let (r, g, b) = heavy::rgb(c);
    Color32::from_rgba_unmultiplied(r, g, b, (c >> 24) as u8)
}

/// The theme index of the spec's colour tables: 0 light, 1 dark.
pub fn theme_index(ctx: &Context) -> usize {
    match ctx.theme() {
        Theme::Light => 0,
        Theme::Dark => 1,
    }
}

/// True when the key was pressed in this pass.
pub fn pressed(ctx: &Context, key: Key) -> bool {
    ctx.input(|i| i.key_pressed(key))
}

/// Toggles between the light and the dark theme.
pub fn toggle_theme(ctx: &Context) {
    ctx.set_theme(match ctx.theme() {
        Theme::Light => Theme::Dark,
        Theme::Dark => Theme::Light,
    });
}

/// The visuals of a theme: egui's own light or dark visuals with the spec's chrome and ink colours.
fn visuals(theme: usize) -> Visuals {
    let mut v = if theme == 0 { Visuals::light() } else { Visuals::dark() };
    v.override_text_color = Some(color(heavy::sheet::INK[theme]));
    v.panel_fill = color(heavy::sheet::CHROME[theme]);
    v.window_fill = v.panel_fill;
    v.extreme_bg_color = color(heavy::sheet::FILL_EVEN[theme]);
    v
}

/// Styles the context: the system font, the spec's visuals for both themes, no animation, no spacing
/// between items, solid scroll bars and labels that do not select text. Starts in the light theme.
pub fn setup(ctx: &Context) {
    ctx.set_fonts(font::definitions());
    ctx.set_visuals_of(Theme::Light, visuals(0));
    ctx.set_visuals_of(Theme::Dark, visuals(1));
    ctx.set_theme(Theme::Light);
    ctx.all_styles_mut(|style| {
        style.animation_time = 0.0;
        style.spacing.item_spacing = vec2(0.0, 0.0);
        style.spacing.scroll = eframe::egui::style::ScrollStyle::solid();
        style.interaction.selectable_labels = false;
    });
}

/// Opens the window of a scene and runs it until the window closes.
pub fn run(scene: Scene) -> eframe::Result {
    let title = scene.title();
    match scene {
        Scene::Sheet { rows } => window::run(title, setup, move || Box::new(sheet::Sheet::new(rows))),
        Scene::Canvas { nodes } => window::run(title, setup, move || Box::new(canvas::Canvas::new(nodes))),
    }
}
