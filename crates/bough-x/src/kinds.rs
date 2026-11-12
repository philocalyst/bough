//! Paint kinds: the closed set of things a record can be, each generic over the expressions of its operands.
//! A kind applied to expression types is an archetype ([`Arch`]).
use crate::expr::{Color, Expr, Stage, U};
use crate::store::{Arch, Backend, ColRef};

pub trait FillArch: Arch {
    fn color(&self, u: &U) -> Color;
}
pub trait NumArch: Arch {
    const DECIMALS: u8;
    fn value(&self, u: &U) -> f32;
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
#[derive(Clone, Copy, PartialEq)]
pub struct Fill<E>(pub E);
impl<E: Expr<Out = Color>> Arch for Fill<E> {
    const READS: u64 = E::READS;
    const NAME: &'static str = "fill";
    const STAGE: &'static str = E::Stage::NAME;
    #[inline(always)]
    fn changed(&self, old: &U, new: &U) -> bool {
        self.0.eval(old) != self.0.eval(new)
    }
    #[inline(always)]
    fn paint<B: Backend>(b: &mut B, col: ColRef<'_, Self>, u: &U) {
        b.fill(col, u)
    }
}
impl<E: Expr<Out = Color>> FillArch for Fill<E> {
    #[inline(always)]
    fn color(&self, u: &U) -> Color {
        self.0.eval(u)
    }
}

/// A number with `D` decimals in tabular digits. The value is an expression (a record constant for a cell, a
/// word of the block for a total); it is formatted while shading, so there is no shaped text to keep.
#[repr(C)]
#[derive(Clone, Copy, PartialEq)]
pub struct Num<const D: u8, V, C>(pub V, pub C);
impl<const D: u8, V: Expr<Out = f32>, C: Expr<Out = Color>> Arch for Num<D, V, C> {
    const READS: u64 = V::READS | C::READS;
    const NAME: &'static str = "num";
    const STAGE: &'static str = if <V::Stage as Stage>::RANK > <C::Stage as Stage>::RANK { V::Stage::NAME } else { C::Stage::NAME };
    #[inline(always)]
    fn changed(&self, old: &U, new: &U) -> bool {
        (self.0.eval(old).to_bits() != self.0.eval(new).to_bits()) | (self.1.eval(old) != self.1.eval(new))
    }
    #[inline(always)]
    fn paint<B: Backend>(b: &mut B, col: ColRef<'_, Self>, u: &U) {
        b.num(col, u)
    }
}
impl<const D: u8, V: Expr<Out = f32>, C: Expr<Out = Color>> NumArch for Num<D, V, C> {
    const DECIMALS: u8 = D;
    #[inline(always)]
    fn value(&self, u: &U) -> f32 {
        self.0.eval(u)
    }
    #[inline(always)]
    fn color(&self, u: &U) -> Color {
        self.1.eval(u)
    }
}

/// Up to 12 bytes of text inline. A stand-in for the text lane's handle to shaped text.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub struct Txt(pub [u8; 12]);
impl Txt {
    pub const fn new(s: &str) -> Txt {
        let b = s.as_bytes();
        assert!(b.len() <= 12);
        let mut t = [0u8; 12];
        let mut i = 0;
        while i < b.len() {
            t[i] = b[i];
            i += 1;
        }
        Txt(t)
    }
    pub fn bytes(&self) -> &[u8] {
        let n = self.0.iter().position(|&b| b == 0).unwrap_or(12);
        &self.0[..n]
    }
}

/// A short constant label.
#[repr(C)]
#[derive(Clone, Copy, PartialEq)]
pub struct Label<C>(pub Txt, pub C);
impl<C: Expr<Out = Color>> Arch for Label<C> {
    const READS: u64 = C::READS;
    const NAME: &'static str = "label";
    const STAGE: &'static str = C::Stage::NAME;
    #[inline(always)]
    fn changed(&self, old: &U, new: &U) -> bool {
        self.1.eval(old) != self.1.eval(new)
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
        self.1.eval(u)
    }
}

/// A border of `W` device pixels, translated by a Frame-stage expression: it moves without a turn of layout,
/// and its damage is where it was and where it is.
#[repr(C)]
#[derive(Clone, Copy, PartialEq)]
pub struct Outline<const W: i32, C, T>(pub C, pub T);
impl<const W: i32, C: Expr<Out = Color>, T: Expr<Out = (i32, i32)>> Arch for Outline<W, C, T> {
    const READS: u64 = C::READS | T::READS;
    const MOVES: bool = T::READS != 0;
    const NAME: &'static str = "outline";
    const STAGE: &'static str = if <T::Stage as Stage>::RANK > <C::Stage as Stage>::RANK { T::Stage::NAME } else { C::Stage::NAME };
    #[inline(always)]
    fn changed(&self, old: &U, new: &U) -> bool {
        self.0.eval(old) != self.0.eval(new)
    }
    #[inline(always)]
    fn offset(&self, u: &U) -> (i32, i32) {
        self.1.eval(u)
    }
    #[inline(always)]
    fn paint<B: Backend>(b: &mut B, col: ColRef<'_, Self>, u: &U) {
        b.outline(col, u)
    }
}
impl<const W: i32, C: Expr<Out = Color>, T: Expr<Out = (i32, i32)>> OutlineArch for Outline<W, C, T> {
    const WIDTH: i32 = W;
    #[inline(always)]
    fn color(&self, u: &U) -> Color {
        self.0.eval(u)
    }
}
