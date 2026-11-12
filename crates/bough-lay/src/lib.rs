//! bough-lay (round 8): layout staged by type.
//!
//! An element built by the façade's calls (`row((..))`, `div().w(110.0).px(6.0).child(..)`) is a value whose
//! *type* is its tree. [`Lay`] is implemented over that type, so the layout of a row is one monomorphised
//! function: no style tree, no node ids, no cache. Values that are literals where the element is built are
//! constants to the compiler once builder and layout are inlined together.
//!
//! * [`flex`]: single-line flexbox in cephal's arithmetic; the freeze loop is one shared function.
//! * [`val`]: sizes that know how they depend on the available space; the validity interval of an answer.
//! * [`el`]: the builder (`LBox`, `Leaf`) and children (`Kids`: tuples, arrays, options).
//! * [`extent`]: where the rows of a list are without visiting them.
//! * [`tree`]: an element as a style tree, for `vendor/cephal`: the differential oracle and the fallback.
//!
//! The seam with the expr lane is `docs/r8/LAYOUT-SEAM.md`.
pub mod el;
pub mod extent;
pub mod flex;
pub mod tree;
pub mod val;

pub use el::{Chain, Flex, LBox, Leaf, Measure, Paint, Rec1, block, column, div, row, text};
pub use extent::{Extent, Heights, Uniform};
pub use flex::{Align, Dim, Hk, Justify, Kids, Lay, Out, Spec};
pub use val::{Av, Cx, V, Valid};

use bough_expr::U;

/// The list a row sits in: a column as wide as the list, the row its one child. Solving it is what gives the
/// row its width (stretched, or its own), its margins and its height, by the same code as any other container.
const LIST: Spec = Spec::new(true);

/// Lay a list row out. Rects are relative to the row's slot in the list (its margins included). Returns the
/// slot's height in whole pixels and the interval of list widths over which every rect and the height are the
/// same.
#[inline(always)]
pub fn place_row<L: Lay, O: Out + ?Sized>(l: &L, s: f32, u: &U, width: f32, out: &mut O) -> (i32, Valid) {
    let mut cx = Cx::new(s, u, [width, 0.0]);
    let w = V::avail(width, 0);
    let mut sol = flex::solve(&LIST, l, &mut cx, [Some(w), None], [None; 2], [Av::Def(w), Av::Max], false, true);
    flex::place_kids(l, &mut cx, &mut sol, [V::k(0.0), V::k(0.0)], out, 0);
    (val::round(sol.outer[1].v), cx.valid)
}

/// The height of a list row and its interval, with no rects: what an extent needs for a row that is not
/// exposed.
#[inline(always)]
pub fn size_row<L: Lay>(l: &L, s: f32, u: &U, width: f32) -> (i32, Valid) {
    let mut cx = Cx::new(s, u, [width, 0.0]);
    let w = V::avail(width, 0);
    let sol = flex::solve(&LIST, l, &mut cx, [Some(w), None], [None; 2], [Av::Def(w), Av::Max], false, false);
    (val::round(sol.outer[1].v), cx.valid)
}

/// Lay a root out in a window of `avail` pixels. Returns the interval of window sizes over which every rect
/// is the same.
#[inline(always)]
pub fn place_root<L: Lay, O: Out + ?Sized>(l: &L, s: f32, u: &U, avail: [f32; 2], out: &mut O) -> Valid {
    let mut cx = Cx::new(s, u, avail);
    let p = [Some(V::avail(avail[0], 0)), Some(V::avail(avail[1], 1))];
    let av = [Av::Def(V::avail(avail[0], 0)), Av::Def(V::avail(avail[1], 1))];
    let size = l.size(&mut cx, [None; 2], p, av, false);
    l.place(&mut cx, size, [V::k(0.0), V::k(0.0)], out, 0);
    cx.valid
}
