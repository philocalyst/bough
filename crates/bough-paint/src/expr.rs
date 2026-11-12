//! Expressions that reach the GPU. The vocabulary (`U`, lenses, `Words`, `Code`, the stage types, `Expr` and
//! the leaves `Uw`, `Un`, `K`, `Ki`, `Rec`) is `bough-expr`, shared with the expr lane (`docs/r8/SEAM.md`).
//! What lives here is what is named after a leaf function of `ops.rs`: the combinators, and `Lane`.
//!
//! An expression value is its own per-record constants: reads of the block and constants are zero-sized,
//! `Rec` holds one word, a combinator is a `repr(C)` struct of its operands. So a record of an archetype is a
//! `[u32; N]` whose word `k` is the `k`-th `Rec` met walking the type depth first, which is how the generated
//! GPU function names it.
//!
//! A combinator has no body of its own. `op!` takes a leaf of `ops.rs` and emits both faces from that one
//! identifier: `eval` calls the leaf on the operands' values; `CODE` is a static tree naming the leaf and the
//! operands' trees. The CPU kernel is `eval` monomorphised and inlined; the GPU function is `CODE` rendered
//! (`emit.rs`). Neither can say something the other does not.
use crate::ops::ops as x;
pub use bough_expr::{Code, Cx, Expr, Frame, JoinAll, K, Ki, Lens, Mount, Rec, Stage, StageId, U, U_PICK, U_PTR_X, U_PTR_Y, U_TIME, Un, Uw, Words, lens, un};
use wgsl_rs::std::{Vec4, Vec4i, Vec4u};

/// The type of an expression's value, for generated code.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ty {
    U32,
    I32,
    V4u,
    V4i,
}
pub trait Val: Copy + PartialEq {
    const TY: Ty;
}
impl Val for u32 {
    const TY: Ty = Ty::U32;
}
impl Val for i32 {
    const TY: Ty = Ty::I32;
}
impl Val for Vec4u {
    const TY: Ty = Ty::V4u;
}
impl Val for Vec4i {
    const TY: Ty = Ty::V4i;
}

/// The record's rect lanes (Q8) as a value.
#[derive(Clone, Copy)]
pub struct Lane;
// SAFETY: zero-sized.
unsafe impl Words for Lane {}
impl Expr for Lane {
    type Out = Vec4i;
    type Stage = Mount;
    const READS: u64 = 0;
    const CODE: &'static Code = &Code::Lane;
    #[inline(always)]
    fn eval(&self, cx: &Cx) -> Vec4i {
        Vec4 { x: cx.rect[0], y: cx.rect[1], z: cx.rect[2], w: cx.rect[3] }
    }
}

/// A combinator from a leaf: `op!(Name = leaf(field: Type: OperandOut, ..) -> Out; opaque = <const expr>)`.
macro_rules! op {
    ($(#[$m:meta])* $name:ident = $leaf:ident ( $($f:ident : $T:ident : $ty:ty),* ) -> $out:ty ; opaque = $opq:expr) => {
        $(#[$m])*
        #[repr(C)]
        #[derive(Clone, Copy)]
        pub struct $name<$($T),*> { $(pub $f: $T),* }
        // SAFETY: `repr(C)` over operands that are `Words` (whole words, alignment <= 4): no padding arises.
        unsafe impl<$($T: Words),*> Words for $name<$($T),*> {}
        impl<$($T: Expr<Out = $ty>),*> Expr for $name<$($T),*>
        where
            ($($T::Stage,)*): JoinAll,
        {
            type Out = $out;
            type Stage = <($($T::Stage,)*) as JoinAll>::Out;
            const READS: u64 = 0 $(| $T::READS)*;
            const OPAQUE: bool = $opq;
            const CODE: &'static Code = &Code::Call(stringify!($leaf), &[$($T::CODE),*]);
            #[inline(always)]
            fn eval(&self, cx: &Cx) -> $out {
                x::$leaf($(self.$f.eval(cx)),*)
            }
        }
    };
}

op!(/// 1 when two words are equal.
    Eq = x_eq(a: A: u32, b: B: u32) -> u32; opaque = false);
op!(Sel = x_sel(c: C: u32, a: A: u32, b: B: u32) -> u32; opaque = A::OPAQUE && B::OPAQUE);
op!(SelI = x_seli(c: C: u32, a: A: i32, b: B: i32) -> i32; opaque = false);
op!(Inv = x_inv(t: T: u32) -> u32; opaque = false);
op!(SelV = x_selv(c: C: u32, a: A: Vec4u, b: B: Vec4u) -> Vec4u; opaque = A::OPAQUE && B::OPAQUE);
op!(/// A packed RGBA8 word as a premultiplied Q15 colour.
    Pm = x_pm(c: C: u32) -> Vec4u; opaque = C::OPAQUE);
op!(/// The same with alpha forced full: opaque whatever the word holds.
    Pmo = x_pmo(c: C: u32) -> Vec4u; opaque = true);
op!(MixV = x_mixv(a: A: Vec4u, b: B: Vec4u, t: T: u32) -> Vec4u; opaque = A::OPAQUE && B::OPAQUE);
op!(Fade = x_fade(a: A: Vec4u, k: T: u32) -> Vec4u; opaque = false);
op!(/// A looping there-and-back progress of (now + phase) over `period` ticks, Q15.
    Tri = x_tri(now: N: u32, phase: P: u32, period: D: u32) -> u32; opaque = false);
op!(Smooth = x_smooth(t: T: u32) -> u32; opaque = false);
op!(Prog = x_prog(now: N: u32, t0: A: u32, dur: D: u32) -> u32; opaque = false);
op!(/// A rect from four words (Q8 x0, y0, x1, y1).
    RectW = x_rect(a: A: u32, b: B: u32, c: C: u32, d: D: u32) -> Vec4i; opaque = false);
op!(Off = x_off(b: B: Vec4i, dx: X: i32, dy: Y: i32) -> Vec4i; opaque = false);
op!(Grow = x_grow(b: B: Vec4i, d: D: i32) -> Vec4i; opaque = false);
op!(MixR = x_mixr(a: A: Vec4i, b: B: Vec4i, t: T: u32) -> Vec4i; opaque = false);
op!(MixI = x_mixi(a: A: i32, b: B: i32, t: T: u32) -> i32; opaque = false);
op!(Top = x_top(b: B: Vec4i, h: H: i32) -> Vec4i; opaque = false);
op!(And = x_and(a: A: Vec4i, b: B: Vec4i) -> Vec4i; opaque = false);
op!(/// A word read as a signed value.
    I = x_i(a: A: u32) -> i32; opaque = false);

/// A record's four words as a rect.
pub type RecRect = RectW<Rec, Rec, Rec, Rec>;
pub fn rec_rect(r: [i32; 4]) -> RecRect {
    RectW { a: Rec(r[0] as u32), b: Rec(r[1] as u32), c: Rec(r[2] as u32), d: Rec(r[3] as u32) }
}
