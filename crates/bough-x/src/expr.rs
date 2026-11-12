//! Expressions as types (ROUND8 sections 1 to 3).
//!
//! An expression is a value whose *type* is its structure and whose *bytes* are the constants one record owns:
//! `Un<L>` (a word of the uniform block) and `K<V>` (a literal) are zero-sized, `Rec<T>` holds a `T`, and a
//! combinator is a `#[repr(C)]` struct of its operands. `eval` is pure, so the derivative of an expression with
//! respect to the block is a finite difference (`eval(old) != eval(new)`), exact for every node including an
//! opaque function. The stage is an associated type joined from the operands' stages by trait resolution.
use core::marker::PhantomData;

/// Words in a uniform block. One `u64` names any subset.
pub const WORDS: usize = 64;

/// The uniform block: all Frame-stage state. The application writes it as plain data; nothing observes a write.
#[repr(C, align(64))]
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct U {
    pub w: [u32; WORDS],
}

impl Default for U {
    fn default() -> U {
        U { w: [0; WORDS] }
    }
}

impl U {
    #[inline(always)]
    pub fn get<L: Lens>(&self) -> L::T {
        L::T::from_word(self.w[L::SLOT])
    }
    #[inline(always)]
    pub fn set<L: Lens>(&mut self, v: L::T) {
        self.w[L::SLOT] = v.to_word();
    }
    /// The words among the first `n` that differ, as a mask. This compare is the whole of noticing a write.
    #[inline]
    pub fn diff(&self, other: &U, n: usize) -> u64 {
        let mut m = 0u64;
        for i in 0..n.min(WORDS) {
            m |= ((self.w[i] != other.w[i]) as u64) << i;
        }
        m
    }
}

/// A value that fits one word of the block.
pub trait Word: Copy + PartialEq + 'static {
    fn to_word(self) -> u32;
    fn from_word(w: u32) -> Self;
}
impl Word for u32 {
    #[inline(always)]
    fn to_word(self) -> u32 {
        self
    }
    #[inline(always)]
    fn from_word(w: u32) -> u32 {
        w
    }
}
impl Word for f32 {
    #[inline(always)]
    fn to_word(self) -> u32 {
        self.to_bits()
    }
    #[inline(always)]
    fn from_word(w: u32) -> f32 {
        f32::from_bits(w)
    }
}
impl Word for bool {
    #[inline(always)]
    fn to_word(self) -> u32 {
        self as u32
    }
    #[inline(always)]
    fn from_word(w: u32) -> bool {
        w != 0
    }
}

/// 0xRRGGBB, opaque.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Color(pub u32);
impl Word for Color {
    #[inline(always)]
    fn to_word(self) -> u32 {
        self.0
    }
    #[inline(always)]
    fn from_word(w: u32) -> Color {
        Color(w)
    }
}

/// The name of one word of the block: the only way an expression reaches state.
pub trait Lens: Copy + 'static {
    type T: Word;
    const SLOT: usize;
}

/// `lens!(pub Hover: u32 = 1);` names word 1; `lens!(pub Sum[10]: f32 = 6);` names words 6..16 as `Sum<I>`.
#[macro_export]
macro_rules! lens {
    ($v:vis $name:ident : $t:ty = $slot:expr) => {
        #[derive(Clone, Copy, PartialEq)]
        $v struct $name;
        impl $crate::expr::Lens for $name {
            type T = $t;
            const SLOT: usize = $slot;
        }
    };
    ($v:vis $name:ident [ $n:expr ] : $t:ty = $slot:expr) => {
        #[derive(Clone, Copy, PartialEq)]
        $v struct $name;
        impl $crate::expr::LensArr for $name {
            type T = $t;
            const BASE: usize = $slot;
            const LEN: usize = $n;
        }
    };
}

/// The name of a run of words of the block (an array field).
pub trait LensArr: Copy + 'static {
    type T: Word;
    const BASE: usize;
    const LEN: usize;
}

// ---- stages, joined by the type system ---------------------------------------------------------------------

/// When an expression is evaluated. Decided by its type alone.
pub trait Stage: 'static {
    const RANK: u8;
    const NAME: &'static str;
}
/// No inputs: part of the type.
pub struct Const;
/// Record constants only: written once, when the record is described.
pub struct Mount;
/// Reads state and feeds geometry: evaluated on the CPU when that state changes.
pub struct Turn;
/// Reads the uniform block and feeds paint only: evaluated while shading, never on the CPU for its own sake.
pub struct Frame;
impl Stage for Const {
    const RANK: u8 = 0;
    const NAME: &'static str = "const";
}
impl Stage for Mount {
    const RANK: u8 = 1;
    const NAME: &'static str = "mount";
}
impl Stage for Turn {
    const RANK: u8 = 2;
    const NAME: &'static str = "turn";
}
impl Stage for Frame {
    const RANK: u8 = 3;
    const NAME: &'static str = "frame";
}
/// The later of two stages.
pub trait Join<B: Stage>: Stage {
    type Out: Stage;
}
macro_rules! join {
    ($($a:ident $b:ident => $o:ident;)*) => { $(impl Join<$b> for $a { type Out = $o; })* };
}
join! {
    Const Const => Const; Const Mount => Mount; Const Turn => Turn; Const Frame => Frame;
    Mount Const => Mount; Mount Mount => Mount; Mount Turn => Turn; Mount Frame => Frame;
    Turn Const => Turn; Turn Mount => Turn; Turn Turn => Turn; Turn Frame => Frame;
    Frame Const => Frame; Frame Mount => Frame; Frame Turn => Frame; Frame Frame => Frame;
}
/// Stages whose value exists on the CPU without asking the raster: what geometry may read.
pub trait Cpu: Stage {}
impl Cpu for Const {}
impl Cpu for Mount {}
impl Cpu for Turn {}

// ---- the trait ---------------------------------------------------------------------------------------------

pub trait Expr: Copy + PartialEq + 'static {
    type Out: Copy + PartialEq;
    type Stage: Stage;
    /// The words of the block this expression reads.
    const READS: u64;
    fn eval(&self, u: &U) -> Self::Out;

    #[inline(always)]
    fn is<B: Expr<Out = Self::Out>>(self, b: B) -> Eq<Self, B>
    where
        Self::Stage: Join<B::Stage>,
    {
        Eq(self, b)
    }
    #[inline(always)]
    fn select<A: Expr, B: Expr<Out = A::Out>>(self, a: A, b: B) -> Select<Self, A, B>
    where
        Self: Expr<Out = bool>,
    {
        Select(self, a, b)
    }
    /// Apply a function that captures nothing. Per-record data belongs in a `Rec` operand, not in a capture.
    #[inline(always)]
    fn map<O: Copy + PartialEq, F: Fn(Self::Out) -> O + Copy + 'static>(self, f: F) -> Map<F, Self> {
        let _ = f;
        Map(self, PhantomData)
    }
}

/// A word of the block.
#[derive(Clone, Copy)]
pub struct Un<L>(PhantomData<L>);
impl<L> PartialEq for Un<L> {
    #[inline(always)]
    fn eq(&self, _: &Self) -> bool {
        true
    }
}
#[inline(always)]
pub fn un<L: Lens>() -> Un<L> {
    Un(PhantomData)
}
impl<L: Lens> Expr for Un<L> {
    type Out = L::T;
    type Stage = Frame;
    const READS: u64 = 1 << L::SLOT;
    #[inline(always)]
    fn eval(&self, u: &U) -> L::T {
        u.get::<L>()
    }
}

/// One word of a run, chosen by a constant the record owns (`sums[c]`).
#[repr(transparent)]
pub struct UnAt<L>(pub u8, PhantomData<L>);
impl<L> Clone for UnAt<L> {
    #[inline(always)]
    fn clone(&self) -> Self {
        *self
    }
}
impl<L> Copy for UnAt<L> {}
impl<L> PartialEq for UnAt<L> {
    #[inline(always)]
    fn eq(&self, o: &Self) -> bool {
        self.0 == o.0
    }
}
#[inline(always)]
pub fn un_at<L: LensArr>(i: usize) -> UnAt<L> {
    assert!(i < L::LEN);
    UnAt(i as u8, PhantomData)
}
impl<L: LensArr> Expr for UnAt<L> {
    type Out = L::T;
    type Stage = Frame;
    const READS: u64 = ((1u64 << L::LEN) - 1) << L::BASE;
    #[inline(always)]
    fn eval(&self, u: &U) -> L::T {
        L::T::from_word(u.w[(L::BASE + self.0 as usize) & (WORDS - 1)])
    }
}

/// A constant owned by the record.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq)]
pub struct Rec<T>(pub T);
#[inline(always)]
pub fn rec<T: Copy + PartialEq + 'static>(v: T) -> Rec<T> {
    Rec(v)
}
impl<T: Copy + PartialEq + 'static> Expr for Rec<T> {
    type Out = T;
    type Stage = Mount;
    const READS: u64 = 0;
    #[inline(always)]
    fn eval(&self, _: &U) -> T {
        self.0
    }
}

/// A literal `u32` in the type.
#[derive(Clone, Copy, PartialEq)]
pub struct K<const V: u32>;
impl<const V: u32> Expr for K<V> {
    type Out = u32;
    type Stage = Const;
    const READS: u64 = 0;
    #[inline(always)]
    fn eval(&self, _: &U) -> u32 {
        V
    }
}
/// A literal colour in the type.
#[derive(Clone, Copy, PartialEq)]
pub struct Kc<const V: u32>;
impl<const V: u32> Expr for Kc<V> {
    type Out = Color;
    type Stage = Const;
    const READS: u64 = 0;
    #[inline(always)]
    fn eval(&self, _: &U) -> Color {
        Color(V)
    }
}

#[repr(C)]
#[derive(Clone, Copy, PartialEq)]
pub struct Eq<A, B>(pub A, pub B);
impl<A: Expr, B: Expr<Out = A::Out>> Expr for Eq<A, B>
where
    A::Stage: Join<B::Stage>,
{
    type Out = bool;
    type Stage = <A::Stage as Join<B::Stage>>::Out;
    const READS: u64 = A::READS | B::READS;
    #[inline(always)]
    fn eval(&self, u: &U) -> bool {
        self.0.eval(u) == self.1.eval(u)
    }
}

#[repr(C)]
#[derive(Clone, Copy, PartialEq)]
pub struct Select<C, A, B>(pub C, pub A, pub B);
impl<C: Expr<Out = bool>, A: Expr, B: Expr<Out = A::Out>> Expr for Select<C, A, B>
where
    C::Stage: Join<A::Stage>,
    <C::Stage as Join<A::Stage>>::Out: Join<B::Stage>,
{
    type Out = A::Out;
    type Stage = <<C::Stage as Join<A::Stage>>::Out as Join<B::Stage>>::Out;
    const READS: u64 = C::READS | A::READS | B::READS;
    #[inline(always)]
    fn eval(&self, u: &U) -> A::Out {
        if self.0.eval(u) { self.1.eval(u) } else { self.2.eval(u) }
    }
}

/// A function of an expression. `F` is zero-sized (asserted when the type is instantiated): what varies per
/// record is in the operand.
#[repr(transparent)]
pub struct Map<F, A>(pub A, PhantomData<F>);
impl<F, A: Copy> Clone for Map<F, A> {
    #[inline(always)]
    fn clone(&self) -> Self {
        *self
    }
}
impl<F, A: Copy> Copy for Map<F, A> {}
impl<F, A: PartialEq> PartialEq for Map<F, A> {
    #[inline(always)]
    fn eq(&self, o: &Self) -> bool {
        self.0 == o.0
    }
}
impl<F, A, O> Expr for Map<F, A>
where
    A: Expr,
    O: Copy + PartialEq,
    F: Fn(A::Out) -> O + Copy + 'static,
{
    type Out = O;
    type Stage = A::Stage;
    const READS: u64 = A::READS;
    #[inline(always)]
    fn eval(&self, u: &U) -> O {
        const { assert!(size_of::<F>() == 0, "a mapped function must capture nothing: put per-record data in a Rec operand") };
        // SAFETY: `F` is zero-sized (asserted above) and `Copy`, and a `Map<F, _>` is only built by `Expr::map`,
        // which was handed a value of `F`: the type is inhabited and copying that value is what this is.
        let f: F = unsafe { core::mem::zeroed() };
        f(self.0.eval(u))
    }
}

// Tuples of operands for a `map` of several arguments. Their stage is the join; spelled out for pairs and
// triples, the arities the scenes use.
impl<A: Expr, B: Expr> Expr for (A, B)
where
    A::Stage: Join<B::Stage>,
{
    type Out = (A::Out, B::Out);
    type Stage = <A::Stage as Join<B::Stage>>::Out;
    const READS: u64 = A::READS | B::READS;
    #[inline(always)]
    fn eval(&self, u: &U) -> Self::Out {
        (self.0.eval(u), self.1.eval(u))
    }
}
impl<A: Expr, B: Expr, C: Expr> Expr for (A, B, C)
where
    A::Stage: Join<B::Stage>,
    <A::Stage as Join<B::Stage>>::Out: Join<C::Stage>,
{
    type Out = (A::Out, B::Out, C::Out);
    type Stage = <<A::Stage as Join<B::Stage>>::Out as Join<C::Stage>>::Out;
    const READS: u64 = A::READS | B::READS | C::READS;
    #[inline(always)]
    fn eval(&self, u: &U) -> Self::Out {
        (self.0.eval(u), self.1.eval(u), self.2.eval(u))
    }
}
