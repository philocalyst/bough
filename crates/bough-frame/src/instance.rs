//! The 48-byte instance every executor reads (`FInst` in shader.metal, `Instance` in the CPU executor).

pub const KIND_SOLID: u8 = 0;
pub const KIND_RRECT: u8 = 1;
pub const KIND_BORDER: u8 = 2;
pub const KIND_GLYPH: u8 = 3;
/// Premultiplied RGBA image from the image atlas, rotated about its centre by `radius` radians.
pub const KIND_IMAGE: u8 = 4;
/// Linear (`uv[2] == 0`, angle `bw`) or radial (`uv[2] != 0`, centre in `color`/`color2` as f32 bits) gradient
/// read from a 256-texel ramp in the image atlas, inside a rounded box.
pub const KIND_GRADIENT: u8 = 5;
/// Gaussian box shadow of a rounded box (blur sigma `bw`).
pub const KIND_SHADOW: u8 = 6;

#[repr(C, align(8))]
#[derive(Clone, Copy, Debug, Default)]
pub struct Instance {
    pub pos: [f32; 2],  //  0
    pub size: [f32; 2], //  8
    pub color: u32,     // 16 RGBA8, r in low byte
    pub color2: u32,    // 20
    pub radius: f32,    // 24
    pub bw: f32,        // 28
    pub uv: [u16; 4],   // 32
    pub space: u16,     // 40
    pub kind: u8,       // 42
    pub flags: u8,      // 43
    pub ext: u32,       // 44 clip index
}
const _: () = assert!(core::mem::size_of::<Instance>() == 48);
const _: () = assert!(core::mem::align_of::<Instance>() == 8);

#[inline]
pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> u32 {
    (r as u32) | (g as u32) << 8 | (b as u32) << 16 | (a as u32) << 24
}
