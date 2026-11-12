//! The open end of the paint kinds (e2e lane): a record that draws itself by continuation.
//!
//! The four kinds of `kinds.rs` are a closed set, each a method of [`Backend`](crate::store::Backend). A kind
//! added from outside this crate cannot add a method, so it describes itself instead: [`Draws::draw`] drives a
//! [`Prims`] with primitives whose kind is in the method's name, and the backend's one generic method
//! ([`Backend::draws`](crate::store::Backend::draws)) is monomorphised per archetype and per primitive sink, so
//! nothing is switched on per record. This is `bough-paint`'s `Paint` / `Sink` pair in the vocabulary this
//! crate already has (device-pixel rects, colours as words); when rows are described through `bough-paint`
//! (SEAM 6.3) the two traits here become those two.
use crate::expr::U;
use crate::store::{Arch, Rect};

/// Where text sits in its box.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// What a record can ask a backend for. Rects are device pixels on screen; `cut` is what may be touched.
pub trait Prims {
    /// A box of one colour (0xAARRGGBB; drawn opaque).
    fn solid(&mut self, r: Rect, c: u32);
    /// A line of text (UTF-8) in the box `r`, centred vertically.
    fn text(&mut self, r: Rect, cut: Rect, s: &[u8], align: Align, c: u32);
    /// A rounded box of `radius` turned by `turn` (a full turn is 65,536) about its centre, antialiased.
    fn rbox(&mut self, r: Rect, cut: Rect, radius: i32, turn: u32, c: u32);
}

/// A record that draws itself.
pub trait Draws: Arch {
    /// Draw the record whose rect on screen is `r`, cut to `cut`, under block `u`.
    fn draw<P: Prims>(&self, r: Rect, cut: Rect, u: &U, p: &mut P);
}
