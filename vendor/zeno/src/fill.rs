//! bough (vendor/README.md): a filled path as an 8-bit alpha mask, with no stroker, transform or subpixel format
//! reachable from it. `fill_alpha(data, fill, origin, scratch, buf)` is, step for step,
//!
//! ```text
//! Mask::with_scratch(data, scratch).format(Format::Alpha).origin(origin).style(fill)
//!     .inspect(|f, w, h| buf.resize(f.buffer_size(w, h), 0)).render_into(&mut buf[..], None)
//! ```
//!
//! with no transform and zero offsets (the bounds of `Inner::apply`'s fill arm, `Mask::placement` with and then
//! without a known size, `render`'s alpha branch), so the coverage bytes and the placement are the same. `Mask`
//! keeps its style as a run-time `Style`, which keeps the stroker linked into every caller; this function is the
//! fill-only path for callers that never stroke.
use super::geometry::{BoundsBuilder, Origin, Placement, Vector};
use super::path_data::PathData;
use super::raster::Rasterizer;
use super::scratch::Scratch;
use super::style::Fill;
#[allow(unused)]
use super::F32Ext;
use crate::lib::Vec;

/// Fills `data` with `fill` into `buf` (resized to `width * height` bytes, new bytes zero) and returns the mask's
/// placement. See the module documentation for the `Mask` call it reproduces.
pub fn fill_alpha<D: PathData>(data: D, fill: Fill, origin: Origin, scratch: &mut Scratch, buf: &mut Vec<u8>) -> Placement {
    // `Mask::inspect` -> `ensure_size` -> `placement()` with no size yet: the bounds of the path as the fill arm
    // of `Inner::apply` emits it (no transform), offset by the (zero) mask offset, floored and ceiled
    let mut bb = BoundsBuilder::new();
    (&data).copy_to(&mut bb);
    let mut bounds = bb.build();
    let mask_offset = Vector::ZERO;
    bounds.min = (bounds.min + mask_offset).floor();
    bounds.max = (bounds.max + mask_offset).ceil();
    let offset = Vector::new(-bounds.min.x, -bounds.min.y);
    let width = bounds.width() as u32;
    let height = bounds.height() as u32;
    // the `inspect` closure of the call this reproduces: `Format::Alpha.buffer_size(w, h)`
    buf.resize((width * height) as usize, 0);
    // `render_into` -> `placement()` with the size known: the stored offset and size
    let left = -offset.x as i32;
    let top = if origin == Origin::BottomLeft { (-offset.y).floor() + height as f32 } else { -offset.y } as i32;
    let placement = Placement { left, top, width, height };
    // `render`: the alpha branch with scratch memory; the shift adds the (zero) render offset
    let y_up = origin == Origin::BottomLeft;
    let shift = offset + Vector::ZERO;
    let pitch = placement.width as usize;
    let mut ras = Rasterizer::new(&mut scratch.render);
    ras.rasterize(
        shift,
        width,
        height,
        &mut |r| {
            (&data).copy_to(r);
        },
        fill,
        &mut buf[..],
        pitch,
        y_up,
    );
    placement
}
