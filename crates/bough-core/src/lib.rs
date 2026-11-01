//! bough-core: the Bough, one integrated display tree (see DESIGN.md sections 1, 2, 9).
//!
//! Records live in paint preorder inside 64-slot SoA pages drawn from a type-stable pool;
//! a flat directory with SoA page summaries sits on top (two levels total).
#![allow(clippy::too_many_arguments, clippy::needless_range_loop)]

pub mod arena;
pub mod bough;
pub mod damage;
pub mod nrect;
pub mod oracle;
pub mod page;
pub mod prefix;
pub mod rng;
pub mod shapes;

pub use bough::{Bough, Handle, RecordInit};
pub use damage::Plane;
pub use nrect::NRect;

/// Non-inlined entry points so the hot loops can be read in `--emit asm` output.
#[doc(hidden)]
pub mod probe {
    use crate::{Bough, NRect, Plane, bough::Handle};
    use crate::nrect::{Thresh, lt_mask, union_all};
    #[inline(never)]
    pub fn lt_mask_64(p: &[NRect; 64], t: Thresh) -> u64 {
        unsafe { lt_mask(p.as_ptr(), 64, t) }
    }
    #[inline(never)]
    pub fn union_all_64(p: &[NRect; 64]) -> NRect {
        union_all(p)
    }
    #[inline(never)]
    pub fn subtree_end(b: &Bough, pos: usize) -> usize {
        b.subtree_end(pos)
    }
    #[inline(never)]
    pub fn parent(b: &Bough, pos: usize) -> Option<usize> {
        b.parent(pos)
    }
    #[inline(never)]
    pub fn hit(b: &Bough, x: i32, y: i32) -> Option<usize> {
        b.hit(x, y)
    }
    #[inline(never)]
    pub fn cull(b: &Bough, r: NRect, out: &mut Vec<u32>) {
        b.cull(r, None, out)
    }
    #[inline(never)]
    pub fn touch_handle(b: &mut Bough, h: Handle) -> bool {
        b.touch_handle(h)
    }
    #[inline(never)]
    pub fn translate_run(b: &mut Bough, s: usize, n: usize, dx: i32, dy: i32) {
        b.translate_run(s, n, dx, dy)
    }
    #[inline(never)]
    pub fn plane_add(p: &mut Plane, r: NRect, w: NRect) {
        p.add(r, w)
    }
    #[inline(never)]
    pub fn repair_into(b: &Bough, age: u32, p: &mut Plane) {
        b.repair_into(age, p)
    }
}
