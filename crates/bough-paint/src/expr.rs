//! Expressions as types: a local mirror of the expr lane's trait (`docs/r8/SEAM.md`), so the swap to
//! `bough-x` is this module. An expression value is its own per-record constants: reads of the block and
//! constants are zero-sized, `Rec` holds one word, a combinator is a `repr(C)` struct of its operands. So a
//! record of an archetype is a `[u32; N]` whose word `k` is the `k`-th `Rec` met walking the type depth
//! first, which is how the generated GPU function names it.
//!
//! A combinator has no body of its own. `op!` takes a leaf of `ops.rs` and emits both faces from that one
//! identifier: `eval` calls the leaf on the operands' values; `CODE` is a static tree naming the leaf and the
//! operands' trees. The CPU kernel is `eval` monomorphised and inlined; the GPU function is `CODE` rendered
//! (`gen.rs`). Neither can say something the other does not.
use crate::ops::ops as x;
use wgsl_rs::std::{Vec4i, Vec4u};

/// The uniform block: every word of state a `Frame` expression may read.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct U {
    pub w: [u32; 64],
}
impl Default for U {
    fn default() -> U {
        U { w: [0; 64] }
    }
}
impl U {
    /// The words that differ between two blocks.
    pub fn diff(&self, o: &U) -> u64 {
        let mut m = 0u64;
        for i in 0..64 {
            m |= ((self.w[i] != o.w[i]) as u64) << i;
        }
        m
    }
}
/// Block words the raster owns (the application reads them and never writes them).
pub const U_TIME: usize = 63;
pub const U_PICK: usize = 62;
pub const U_PTR_X: usize = 61;
pub const U_PTR_Y: usize = 60;

/// What an expression is evaluated against: the block, and the record's own rect lanes (Q8, x0 y0 x1 y1).
#[derive(Clone, Copy)]
pub struct Cx<'a> {
    pub u: &'a U,
    pub rect: Vec4i,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Stage {
    Const,
    Mount,
    Turn,
    Frame,
}
pub const fn later(s: &[Stage]) -> Stage {
    let mut m = Stage::Const;
    let mut i = 0;
    while i < s.len() {
        if s[i] as u8 > m as u8 {
            m = s[i];
        }
        i += 1;
    }
    m
}

/// An expression's structure as data: what the GPU function and the plan are made from.
#[derive(Debug)]
pub enum Code {
    /// Word `.0` of the block.
    U(usize),
    /// The next word of the record (depth-first order).
    Rec,
    /// The record's rect lanes.
    Lane,
    Lit(u32),
    /// A leaf of `ops.rs` applied to operands.
    Call(&'static str, &'static [&'static Code]),
}

/// A value whose bytes are whole 32-bit words with no padding and no invalid bit pattern: a record can be
/// read in place from a column and handed to the GPU as words.
///
/// # Safety
/// `size_of::<Self>() % 4 == 0`, `align_of::<Self>() <= 4`, every bit pattern is a valid value.
pub unsafe trait Words: Copy + 'static {}
// SAFETY: one word; no fields.
unsafe impl Words for u32 {}

pub trait Expr: Words {
    type Out: Copy + PartialEq;
    const STAGE: Stage;
    /// The block words it reads.
    const READS: u64;
    /// If the value is a colour, its alpha is provably full.
    const OPAQUE: bool = false;
    const CODE: &'static Code;
    fn eval(&self, cx: &Cx) -> Self::Out;
}

/// Word `S` of the block.
#[derive(Clone, Copy)]
pub struct Uw<const S: usize>;
// SAFETY: zero-sized.
unsafe impl<const S: usize> Words for Uw<S> {}
impl<const S: usize> Expr for Uw<S> {
    type Out = u32;
    const STAGE: Stage = Stage::Frame;
    const READS: u64 = 1 << S;
    const CODE: &'static Code = &Code::U(S);
    #[inline(always)]
    fn eval(&self, cx: &Cx) -> u32 {
        cx.u.w[S]
    }
}
/// A constant word.
#[derive(Clone, Copy)]
pub struct K<const V: u32>;
// SAFETY: zero-sized.
unsafe impl<const V: u32> Words for K<V> {}
impl<const V: u32> Expr for K<V> {
    type Out = u32;
    const STAGE: Stage = Stage::Const;
    const READS: u64 = 0;
    const OPAQUE: bool = V >> 24 == 255;
    const CODE: &'static Code = &Code::Lit(V);
    #[inline(always)]
    fn eval(&self, _: &Cx) -> u32 {
        V
    }
}
/// A constant signed value (Q8 lengths).
#[derive(Clone, Copy)]
pub struct Ki<const V: i32>;
// SAFETY: zero-sized.
unsafe impl<const V: i32> Words for Ki<V> {}
impl<const V: i32> Expr for Ki<V> {
    type Out = i32;
    const STAGE: Stage = Stage::Const;
    const READS: u64 = 0;
    const CODE: &'static Code = &Code::Call("x_i", &[&Code::Lit(V as u32)]);
    #[inline(always)]
    fn eval(&self, _: &Cx) -> i32 {
        V
    }
}
/// One word the record owns.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Rec(pub u32);
// SAFETY: one `u32`.
unsafe impl Words for Rec {}
impl Expr for Rec {
    type Out = u32;
    const STAGE: Stage = Stage::Mount;
    const READS: u64 = 0;
    const CODE: &'static Code = &Code::Rec;
    #[inline(always)]
    fn eval(&self, _: &Cx) -> u32 {
        self.0
    }
}
/// The record's rect lanes (Q8).
#[derive(Clone, Copy)]
pub struct Lane;
// SAFETY: zero-sized.
unsafe impl Words for Lane {}
impl Expr for Lane {
    type Out = Vec4i;
    const STAGE: Stage = Stage::Mount;
    const READS: u64 = 0;
    const CODE: &'static Code = &Code::Lane;
    #[inline(always)]
    fn eval(&self, cx: &Cx) -> Vec4i {
        cx.rect
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
        impl<$($T: Expr<Out = $ty>),*> Expr for $name<$($T),*> {
            type Out = $out;
            const STAGE: Stage = later(&[$($T::STAGE),*]);
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
