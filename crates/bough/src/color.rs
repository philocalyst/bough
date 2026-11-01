/// RGBA8, red in the low byte (the shader's unpack order). Straight alpha.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Color(pub u32);

impl Color {
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color {
        Color(r as u32 | (g as u32) << 8 | (b as u32) << 16 | (a as u32) << 24)
    }
}

/// Opaque colour from `0xRRGGBB`.
pub const fn rgb(hex: u32) -> Color {
    Color::rgba((hex >> 16) as u8, (hex >> 8) as u8, hex as u8, 255)
}
