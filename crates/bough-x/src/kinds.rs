//! Paint kinds: the closed set of things a record can be, each generic over the expressions of its operands.
//! A kind applied to expression types is an archetype ([`Arch`]).
use crate::expr::{Code, Color, Cx, Expr, Frame, Map, StageId, U, U_TIME, Words, later};
use crate::store::{Arch, Backend, ColRef};

pub trait FillArch: Arch {
    fn color(&self, u: &U) -> Color;
}
pub trait NumArch: Arch {
    const DECIMALS: u8;
    fn value(&self, u: &U) -> i32;
    fn color(&self, u: &U) -> Color;
}
pub trait LabelArch: Arch {
    fn text(&self) -> Txt;
    fn color(&self, u: &U) -> Color;
}
pub trait OutlineArch: Arch {
    const WIDTH: i32;
    fn color(&self, u: &U) -> Color;
}

/// A filled rectangle.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct Fill<E>(pub E);
// SAFETY: transparent over `E: Words`.
unsafe impl<E: Words> Words for Fill<E> {}
impl<E: Expr<Out = Color>> Arch for Fill<E> {
    const READS: u64 = E::READS;
    const NAME: &'static str = "fill";
    const STAGE: StageId = E::STAGE;
    #[inline(always)]
    fn changed(&self, old: &U, new: &U) -> bool {
        self.0.eval(&Cx::of(old)) != self.0.eval(&Cx::of(new))
    }
    #[inline(always)]
    fn paint<B: Backend>(b: &mut B, col: ColRef<'_, Self>, u: &U) {
        b.fill(col, u)
    }
}
impl<E: Expr<Out = Color>> FillArch for Fill<E> {
    #[inline(always)]
    fn color(&self, u: &U) -> Color {
        self.0.eval(&Cx::of(u))
    }
}

/// A number with `D` decimals in tabular digits. The value is an integer scaled by `10^D` (hundredths for
/// `D = 2`), from an expression: a record constant for a cell, a word of the block for a total. It is formatted
/// while shading, in integer arithmetic, so there is no shaped text to keep.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Num<const D: u8, V, C>(pub V, pub C);
// SAFETY: `repr(C)` over `Words` operands: whole words at alignment 4, so no padding.
unsafe impl<const D: u8, V: Words, C: Words> Words for Num<D, V, C> {}
impl<const D: u8, V: Expr<Out = i32>, C: Expr<Out = Color>> Arch for Num<D, V, C> {
    const READS: u64 = V::READS | C::READS;
    const NAME: &'static str = "num";
    const STAGE: StageId = later(&[V::STAGE, C::STAGE]);
    #[inline(always)]
    fn changed(&self, old: &U, new: &U) -> bool {
        (self.0.eval(&Cx::of(old)) != self.0.eval(&Cx::of(new))) | (self.1.eval(&Cx::of(old)) != self.1.eval(&Cx::of(new)))
    }
    #[inline(always)]
    fn paint<B: Backend>(b: &mut B, col: ColRef<'_, Self>, u: &U) {
        b.num(col, u)
    }
}
impl<const D: u8, V: Expr<Out = i32>, C: Expr<Out = Color>> NumArch for Num<D, V, C> {
    const DECIMALS: u8 = D;
    #[inline(always)]
    fn value(&self, u: &U) -> i32 {
        self.0.eval(&Cx::of(u))
    }
    #[inline(always)]
    fn color(&self, u: &U) -> Color {
        self.1.eval(&Cx::of(u))
    }
}

/// Up to 12 bytes of text inline, as three words. A stand-in for the text lane's handle to shaped text.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub struct Txt(pub [u32; 3]);
// SAFETY: three words.
unsafe impl Words for Txt {}
impl Txt {
    pub const fn new(s: &str) -> Txt {
        let b = s.as_bytes();
        assert!(b.len() <= 12);
        let mut t = [0u32; 3];
        let mut i = 0;
        while i < b.len() {
            t[i / 4] |= (b[i] as u32) << (8 * (i % 4));
            i += 1;
        }
        Txt(t)
    }
    /// The bytes and how many of them are text.
    pub fn bytes(&self) -> ([u8; 12], usize) {
        let mut b = [0u8; 12];
        for i in 0..12 {
            b[i] = (self.0[i / 4] >> (8 * (i % 4))) as u8;
        }
        (b, b.iter().position(|&c| c == 0).unwrap_or(12))
    }
}

/// A short constant label.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Label<C>(pub Txt, pub C);
// SAFETY: `repr(C)` over `Words` operands.
unsafe impl<C: Words> Words for Label<C> {}
impl<C: Expr<Out = Color>> Arch for Label<C> {
    const READS: u64 = C::READS;
    const NAME: &'static str = "label";
    const STAGE: StageId = C::STAGE;
    #[inline(always)]
    fn changed(&self, old: &U, new: &U) -> bool {
        self.1.eval(&Cx::of(old)) != self.1.eval(&Cx::of(new))
    }
    #[inline(always)]
    fn paint<B: Backend>(b: &mut B, col: ColRef<'_, Self>, u: &U) {
        b.label(col, u)
    }
}
impl<C: Expr<Out = Color>> LabelArch for Label<C> {
    #[inline(always)]
    fn text(&self) -> Txt {
        self.0
    }
    #[inline(always)]
    fn color(&self, u: &U) -> Color {
        self.1.eval(&Cx::of(u))
    }
}

/// An expression that places a record: an offset, and until when it is still moving.
pub trait Motion: Expr<Out = (i32, i32)> {
    /// 0 when at rest under `u`.
    #[inline(always)]
    fn until(&self, _u: &U) -> u32 {
        0
    }
}
/// A function of state moves a record only when that state is written: it is always at rest between turns.
impl<F, A> Motion for Map<F, A> where Map<F, A>: Expr<Out = (i32, i32)> {}

/// From `from` to `to` over `DUR` ticks of `Time` starting at `t0`, linearly, in integers: a transition as an
/// expression. It costs no turn while it runs.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Glide<const DUR: u32, F, T, S>(pub F, pub T, pub S);
// SAFETY: `repr(C)` over `Words` operands.
unsafe impl<const DUR: u32, F: Words, T: Words, S: Words> Words for Glide<DUR, F, T, S> {}
impl<const DUR: u32, F: Expr<Out = (i32, i32)>, T: Expr<Out = (i32, i32)>, S: Expr<Out = u32>> Expr for Glide<DUR, F, T, S> {
    type Out = (i32, i32);
    type Stage = Frame;
    const READS: u64 = F::READS | T::READS | S::READS | 1 << U_TIME;
    const CODE: &'static Code = &Code::Call("x_glide", &[F::CODE, T::CODE, S::CODE, &Code::U(U_TIME), &Code::Lit(DUR)]);
    #[inline(always)]
    fn eval(&self, cx: &Cx) -> (i32, i32) {
        let (a, b) = (self.0.eval(cx), self.1.eval(cx));
        // progress in 1/256ths, saturating at both ends
        let dt = cx.u.w[U_TIME].wrapping_sub(self.2.eval(cx));
        let p = if dt >= DUR { 256 } else { (dt * 256 / DUR) as i64 };
        (a.0 + ((b.0 - a.0) as i64 * p >> 8) as i32, a.1 + ((b.1 - a.1) as i64 * p >> 8) as i32)
    }
}
impl<const DUR: u32, F: Expr<Out = (i32, i32)>, T: Expr<Out = (i32, i32)>, S: Expr<Out = u32>> Motion for Glide<DUR, F, T, S> {
    #[inline(always)]
    fn until(&self, u: &U) -> u32 {
        let end = self.2.eval(&Cx::of(u)).wrapping_add(DUR);
        if u.w[U_TIME].wrapping_sub(self.2.eval(&Cx::of(u))) < DUR { end } else { 0 }
    }
}

/// A border of `W` device pixels, translated by a Frame-stage expression: it moves without a turn of layout,
/// and its damage is where it was and where it is.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Outline<const W: i32, C, T>(pub C, pub T);
// SAFETY: `repr(C)` over `Words` operands.
unsafe impl<const W: i32, C: Words, T: Words> Words for Outline<W, C, T> {}
impl<const W: i32, C: Expr<Out = Color>, T: Motion> Arch for Outline<W, C, T> {
    const READS: u64 = C::READS | T::READS;
    const MOVES: bool = T::READS != 0;
    const TIMED: bool = T::READS & (1 << U_TIME) != 0;
    #[inline(always)]
    fn until(&self, u: &U) -> u32 {
        self.1.until(u)
    }
    const NAME: &'static str = "outline";
    const STAGE: StageId = later(&[T::STAGE, C::STAGE]);
    #[inline(always)]
    fn changed(&self, old: &U, new: &U) -> bool {
        self.0.eval(&Cx::of(old)) != self.0.eval(&Cx::of(new))
    }
    #[inline(always)]
    fn offset(&self, u: &U) -> (i32, i32) {
        self.1.eval(&Cx::of(u))
    }
    #[inline(always)]
    fn paint<B: Backend>(b: &mut B, col: ColRef<'_, Self>, u: &U) {
        b.outline(col, u)
    }
}
impl<const W: i32, C: Expr<Out = Color>, T: Motion> OutlineArch for Outline<W, C, T> {
    const WIDTH: i32 = W;
    #[inline(always)]
    fn color(&self, u: &U) -> Color {
        self.0.eval(&Cx::of(u))
    }
}
