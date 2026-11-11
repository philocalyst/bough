//! zgx's built-in dark theme (`crates/zgx/themes/zgx-dark.json`), the theme its headless apps run in.
use bough::color::{rgb, Color};

pub const BACKGROUND: Color = rgb(0x10141c);
pub const SURFACE_SUNKEN: Color = rgb(0x0b0e14);
pub const SURFACE_BASE: Color = rgb(0x1a2230);
pub const SURFACE_RAISED: Color = rgb(0x232e40);
pub const TEXT_PRIMARY: Color = rgb(0xe5edf7);
pub const TEXT_SECONDARY: Color = rgb(0x8a9ab0);
pub const TEXT_TERTIARY: Color = rgb(0x66758a);
pub const BORDER: Color = rgb(0x2f3d52);
pub const BORDER_STRONG: Color = rgb(0x46576f);
pub const ACCENT: Color = rgb(0x5ea5ff);
pub const ACCENT_TEXT: Color = rgb(0x08111f);
pub const DANGER: Color = rgb(0xff6b6b);
pub const WARNING: Color = rgb(0xf5b544);
pub const SUCCESS: Color = rgb(0x4cc38a);
pub const INFO: Color = rgb(0x62b6f0);
/// `#3b527080`.
pub const HOVER: Color = Color::rgba(0x3b, 0x52, 0x70, 0x80);
/// `#38629eaa`.
pub const SELECTION: Color = Color::rgba(0x38, 0x62, 0x9e, 0xaa);
pub const CLEAR: Color = Color(0);
/// An input's background (zgx's `input.bg` defaults to the sunken surface).
pub const INPUT_BG: Color = SURFACE_SUNKEN;

/// `font.size.*` / `font.line.*` (points).
pub const SIZE_SM: f32 = 13.0;
pub const LINE_SM: f32 = 18.0;
pub const SIZE_MD: f32 = 16.0;
pub const LINE_MD: f32 = 22.0;
/// A button's and an input's height (zgx's controls: `md` text, 6 pt above and below).
pub const CONTROL_H: f32 = 34.0;
pub const RADIUS_MD: f32 = 6.0;

/// `#rrggbb` or `#rrggbbaa` as a colour.
pub fn hex(s: &str) -> Color {
    let h = s.trim_start_matches('#');
    let v = u32::from_str_radix(h, 16).unwrap_or(0);
    match h.len() {
        8 => Color::rgba((v >> 24) as u8, (v >> 16) as u8, (v >> 8) as u8, v as u8),
        _ => rgb(v),
    }
}

/// `c` at `a` of its alpha (a `opacity`).
pub const fn alpha(c: Color, a: u8) -> Color {
    Color((c.0 & 0x00ff_ffff) | (a as u32) << 24)
}
