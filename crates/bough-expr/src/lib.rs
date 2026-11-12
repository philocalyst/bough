//! The vocabulary both round-8 lanes build on (`docs/r8/SEAM.md`, v1). No dependencies.
//!
//! * [`U`]: the uniform block, 64 words; all Frame-stage state. [`Lens`] names a word, [`LensArr`] a run.
//! * [`Words`]: a value that is whole 32-bit words: what a record is made of.
//! * [`Expr`]: an expression is a value whose *type* is its structure and whose *bytes* are the constants one
//!   record owns. Its stage is a type ([`Stage`], joined from the operands by [`Join`] / [`JoinAll`]) and a
//!   constant; its structure is also data ([`Code`]) for the plan and the generated GPU function.
//! * Leaves only: [`Uw`], [`Un`], [`UnAt`], [`K`], [`Ki`], [`Kc`], [`Rec`]. Combinators that reach the GPU are
//!   named after a leaf function and live with those functions (`bough-paint`). [`Map`] is the combinator for
//!   an arbitrary Rust function: it has no GPU code, so it is evaluated on the CPU.
#![no_std]
use core::marker::PhantomData;

/// Words in a uniform block. One `u64` names any subset.
pub const WORDS: usize = 64;
/// Words the raster owns: the application reads them and never writes them, and a turn-end compare skips them.
pub const U_PTR_Y: usize = 60;
pub const U_PTR_X: usize = 61;
pub const U_PICK: usize = 62;
pub const U_TIME: usize = 63;
/// The words an application may write.
pub const APP_WORDS: usize = 60;

/// The uniform block. The application writes it as plain data; nothing observes a write.
#[repr(C, align(64))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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
    #[inline(always)]
    pub fn at<L: LensArr>(&self, i: usize) -> L::T {
        assert!(i < L::LEN);
        L::T::from_word(self.w[L::BASE + i])
    }
    #[inline(always)]
    pub fn set_at<L: LensArr>(&mut self, i: usize, v: L::T) {
        assert!(i < L::LEN);
        self.w[L::BASE + i] = v.to_word();
    }
    /// The words among the first `n` that differ, as a mask. This compare is the whole of noticing a write.
    #[inline(always)]
    pub fn diff(&self, other: &U, n: usize) -> u64 {
        let n = if n < WORDS { n } else { WORDS };
        // nothing differs, usually: one pass that folds to a few vector compares
        let mut any = 0u32;
        let mut i = 0;
        while i < n {
            any |= self.w[i] ^ other.w[i];
            i += 1;
        }
        if any == 0 {
            return 0;
        }
        let mut m = 0u64;
        let mut i = 0;
        while i < n {
            m |= ((self.w[i] != other.w[i]) as u64) << i;
            i += 1;
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
impl Word for i32 {
    #[inline(always)]
    fn to_word(self) -> u32 {
        self as u32
    }
    #[inline(always)]
    fn from_word(w: u32) -> i32 {
        w as i32
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
/// A float may sit in the block as its bits; no Frame expression does arithmetic on it (SEAM 6.1, item 3).
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

/// 0xAARRGGBB as one word.
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

/// The name of one word of the block: how an expression reaches state by name and type.
pub trait Lens: Copy + 'static {
    type T: Word;
    const SLOT: usize;
}
/// The name of a run of words of the block (an array field).
pub trait LensArr: Copy + 'static {
    type T: Word;
    const BASE: usize;
    const LEN: usize;
}

/// `lens!(pub Hover: u32 = 1);` names word 1; `lens!(pub Sum[10]: i32 = 4);` names words 4..14.
#[macro_export]
macro_rules! lens {
    ($v:vis $name:ident : $t:ty = $slot:expr) => {
        #[derive(Clone, Copy, PartialEq)]
        $v struct $name;
        impl $crate::Lens for $name {
            type T = $t;
            const SLOT: usize = {
                assert!($slot < $crate::APP_WORDS);
                $slot
            };
        }
    };
    ($v:vis $name:ident [ $n:expr ] : $t:ty = $slot:expr) => {
        #[derive(Clone, Copy, PartialEq)]
        $v struct $name;
        impl $crate::LensArr for $name {
            type T = $t;
            const BASE: usize = {
                assert!($slot + $n <= $crate::APP_WORDS);
                $slot
            };
            const LEN: usize = $n;
        }
    };
}

/// Device pixels, half open.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Rect {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}
impl Rect {
    /// A box no record leaves: what an unbounded motion sweeps (cut to the space's clip by whoever marks it).
    pub const EVERYWHERE: Rect = Rect { x0: -(1 << 28), y0: -(1 << 28), x1: 1 << 28, y1: 1 << 28 };
    #[inline(always)]
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect { x0: x, y0: y, x1: x + w, y1: y + h }
    }
    #[inline(always)]
    pub fn shift(self, d: (i32, i32)) -> Rect {
        Rect { x0: self.x0 + d.0, y0: self.y0 + d.1, x1: self.x1 + d.0, y1: self.y1 + d.1 }
    }
    #[inline(always)]
    pub fn and(self, o: Rect) -> Rect {
        Rect { x0: self.x0.max(o.x0), y0: self.y0.max(o.y0), x1: self.x1.min(o.x1), y1: self.y1.min(o.y1) }
    }
    #[inline(always)]
    pub fn or(self, o: Rect) -> Rect {
        Rect { x0: self.x0.min(o.x0), y0: self.y0.min(o.y0), x1: self.x1.max(o.x1), y1: self.y1.max(o.y1) }
    }
    #[inline(always)]
    pub fn empty(self) -> bool {
        self.x0 >= self.x1 || self.y0 >= self.y1
    }
    #[inline(always)]
    pub fn hits(self, o: Rect) -> bool {
        self.x0 < o.x1 && o.x0 < self.x1 && self.y0 < o.y1 && o.y0 < self.y1
    }
    #[inline(always)]
    pub fn area(self) -> i64 {
        (self.x1 - self.x0) as i64 * (self.y1 - self.y0) as i64
    }
}

/// The words the raster owns, by name. An expression may read them; the application never writes them.
#[derive(Clone, Copy, PartialEq)]
pub struct Time;
impl Lens for Time {
    type T = u32;
    const SLOT: usize = U_TIME;
}
#[derive(Clone, Copy, PartialEq)]
pub struct PtrX;
impl Lens for PtrX {
    type T = i32;
    const SLOT: usize = U_PTR_X;
}
#[derive(Clone, Copy, PartialEq)]
pub struct PtrY;
impl Lens for PtrY {
    type T = i32;
    const SLOT: usize = U_PTR_Y;
}
#[derive(Clone, Copy, PartialEq)]
pub struct Pick;
impl Lens for Pick {
    type T = u32;
    const SLOT: usize = U_PICK;
}
/// The hot words: they change every frame or every pointer event, so they are never part of a turn's compare.
pub const HOT: u64 = 0xf << 60;
// ---- record words ------------------------------------------------------------------------------------------

/// A value whose bytes are whole 32-bit words with no padding and no invalid bit pattern: a record can be read
/// in place from a column, compared as words, and handed to the GPU.
///
/// # Safety
/// `size_of::<Self>() % 4 == 0`, `align_of::<Self>() <= 4`, no padding, and every bit pattern (zero included)
/// is a valid value.
pub unsafe trait Words: Copy + 'static {
    const N: usize = size_of::<Self>() / 4;
    /// The record as words.
    #[inline(always)]
    fn words(&self) -> &[u32] {
        // SAFETY: the trait's contract: `N` whole words, alignment at most 4 (and at least 1; a zero-sized value
        // gives an empty slice at a dangling but aligned-enough address only when N == 0, handled below).
        if Self::N == 0 { &[] } else { unsafe { core::slice::from_raw_parts(self as *const Self as *const u32, Self::N) } }
    }
    /// Equality of records is equality of their words.
    #[inline(always)]
    fn same(&self, o: &Self) -> bool {
        self.words() == o.words()
    }
}
// SAFETY: one word each; every bit pattern valid.
unsafe impl Words for u32 {}
// SAFETY: as above.
unsafe impl Words for i32 {}
// SAFETY: as above (any bits are a float).
unsafe impl Words for f32 {}
// SAFETY: transparent over `u32`.
unsafe impl Words for Color {}
// SAFETY: zero-sized.
unsafe impl Words for () {}
// SAFETY: an array of `Words` has no padding between elements whose size is a multiple of their alignment.
unsafe impl<T: Words, const N: usize> Words for [T; N] {}
// SAFETY: every field is whole words with alignment at most 4, so no order of the fields leaves padding. The
// order itself is unspecified for a tuple: a tuple may be a `Map` operand (CPU), never a GPU record.
unsafe impl<A: Words, B: Words> Words for (A, B) {}
// SAFETY: as pairs.
unsafe impl<A: Words, B: Words, C: Words> Words for (A, B, C) {}
// SAFETY: as pairs.
unsafe impl<A: Words, B: Words, C: Words, D: Words> Words for (A, B, C, D) {}

// ---- stages ------------------------------------------------------------------------------------------------

/// A stage as a value: what `Code` and the plan read.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum StageId {
    Const,
    Mount,
    Turn,
    Frame,
}
/// The later of several stages.
pub const fn later(s: &[StageId]) -> StageId {
    let mut m = StageId::Const;
    let mut i = 0;
    while i < s.len() {
        if s[i] as u8 > m as u8 {
            m = s[i];
        }
        i += 1;
    }
    m
}

/// A stage as a type: what a bound can refuse (`T::Stage: Cpu`).
pub trait Stage: 'static {
    const ID: StageId;
    /// The stage of a CPU function applied to a value of this stage: state read in a Rust closure is `Turn`.
    type OnCpu: Stage;
}
/// No inputs: part of the type.
pub struct Const;
/// Record constants only: written when the record is described.
pub struct Mount;
/// Reads state and is evaluated on the CPU when that state changes (feeds geometry, or is a Rust function).
pub struct Turn;
/// Reads the block and feeds paint only: evaluated while shading.
pub struct Frame;
impl Stage for Const {
    const ID: StageId = StageId::Const;
    type OnCpu = Const;
}
impl Stage for Mount {
    const ID: StageId = StageId::Mount;
    type OnCpu = Mount;
}
impl Stage for Turn {
    const ID: StageId = StageId::Turn;
    type OnCpu = Turn;
}
impl Stage for Frame {
    const ID: StageId = StageId::Frame;
    type OnCpu = Turn;
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
/// The latest of a tuple of stages: one bound for a combinator of any arity
/// (`where (A::Stage, B::Stage, C::Stage): JoinAll`).
pub trait JoinAll {
    type Out: Stage;
}
impl JoinAll for () {
    type Out = Const;
}
impl<A: Stage> JoinAll for (A,) {
    type Out = A;
}
macro_rules! join_all {
    ($a:ident $($b:ident)+) => {
        impl<$a: Stage, $($b: Stage),+> JoinAll for ($a, $($b),+)
        where
            ($($b,)+): JoinAll,
            $a: Join<<($($b,)+) as JoinAll>::Out>,
        {
            type Out = <$a as Join<<($($b,)+) as JoinAll>::Out>>::Out;
        }
    };
}
join_all!(A B);
join_all!(A B C);
join_all!(A B C D);
join_all!(A B C D E);
join_all!(A B C D E F);
join_all!(A B C D E F G);
join_all!(A B C D E F G H);
/// Stages whose value exists on the CPU without asking the raster: what layout may read.
pub trait Cpu: Stage {}
impl Cpu for Const {}
impl Cpu for Mount {}
impl Cpu for Turn {}

// ---- code as data ------------------------------------------------------------------------------------------

/// An expression's structure as data: what the GPU function and the compile-time plan are made from.
#[derive(Debug)]
pub enum Code {
    /// Word `.0` of the block.
    U(usize),
    /// One of the `.1` words of the block from `.0`, chosen by the next word of the record.
    UAt(usize, usize),
    /// The next word of the record (depth-first order of the type).
    Rec,
    /// The record's own rect lanes.
    Lane,
    Lit(u32),
    /// A leaf function applied to operands.
    Call(&'static str, &'static [&'static Code]),
    /// A Rust function with no GPU form: its value is computed on the CPU and reaches the GPU as a record word.
    Cpu(&'static [&'static Code]),
}

/// What an expression is evaluated against: the block, and the record's own rect lanes (x0, y0, x1, y1).
#[derive(Clone, Copy)]
pub struct Cx<'a> {
    pub u: &'a U,
    pub rect: [i32; 4],
}
impl<'a> Cx<'a> {
    /// For an expression that does not read the rect.
    #[inline(always)]
    pub fn of(u: &'a U) -> Cx<'a> {
        Cx { u, rect: [0; 4] }
    }
}

pub trait Expr: Words {
    type Out: Copy + PartialEq;
    type Stage: Stage;
    const STAGE: StageId = <Self::Stage as Stage>::ID;
    /// The words of the block it reads.
    const READS: u64;
    /// If the value is a colour, its alpha is provably full.
    const OPAQUE: bool = false;
    const CODE: &'static Code;
    fn eval(&self, cx: &Cx) -> Self::Out;

    /// The bound of this expression in `Time`: the value of `Time` from which on the expression no longer
    /// changes with time alone, under the other words of this block. 0: it is at rest now. `u32::MAX`: never.
    /// An expression that reads `Time` and says nothing is never at rest; a combinator forwards to its
    /// operands. This is what lets a frame cost nothing: time is never differentiated, its readers say when
    /// they stop.
    #[inline(always)]
    fn until(&self, _cx: &Cx) -> u32 {
        if Self::READS >> U_TIME & 1 != 0 { u32::MAX } else { 0 }
    }

    /// Apply a Rust function that captures nothing. What varies per record belongs in a `Rec` operand. The
    /// result is CPU stage: it is re-evaluated for exposed records when a word it reads changes.
    #[inline(always)]
    fn map<O: Copy + PartialEq, F: Fn(Self::Out) -> O + Copy + 'static>(self, f: F) -> Map<F, Self> {
        let _ = f;
        Map(self, PhantomData)
    }
}

/// Word `S` of the block, untyped.
#[derive(Clone, Copy)]
pub struct Uw<const S: usize>;
// SAFETY: zero-sized.
unsafe impl<const S: usize> Words for Uw<S> {}
impl<const S: usize> Expr for Uw<S> {
    type Out = u32;
    type Stage = Frame;
    const READS: u64 = 1 << S;
    const CODE: &'static Code = &Code::U(S);
    #[inline(always)]
    fn eval(&self, cx: &Cx) -> u32 {
        cx.u.w[S]
    }
}

/// A named, typed word of the block.
pub struct Un<L>(PhantomData<L>);
impl<L> Clone for Un<L> {
    #[inline(always)]
    fn clone(&self) -> Self {
        *self
    }
}
impl<L> Copy for Un<L> {}
// SAFETY: zero-sized.
unsafe impl<L: 'static> Words for Un<L> {}
#[inline(always)]
pub const fn un<L: Lens>() -> Un<L> {
    Un(PhantomData)
}
impl<L: Lens> Expr for Un<L> {
    type Out = L::T;
    type Stage = Frame;
    const READS: u64 = 1 << L::SLOT;
    const CODE: &'static Code = &Code::U(L::SLOT);
    #[inline(always)]
    fn eval(&self, cx: &Cx) -> L::T {
        cx.u.get::<L>()
    }
}

/// One word of a run, chosen by a word the record owns (`sums[c]`).
#[repr(transparent)]
pub struct UnAt<L>(pub u32, PhantomData<L>);
impl<L> Clone for UnAt<L> {
    #[inline(always)]
    fn clone(&self) -> Self {
        *self
    }
}
impl<L> Copy for UnAt<L> {}
// SAFETY: transparent over one `u32`.
unsafe impl<L: 'static> Words for UnAt<L> {}
#[inline(always)]
pub fn un_at<L: LensArr>(i: usize) -> UnAt<L> {
    assert!(i < L::LEN);
    UnAt(i as u32, PhantomData)
}
impl<L: LensArr> Expr for UnAt<L> {
    type Out = L::T;
    type Stage = Frame;
    const READS: u64 = ((1u64 << L::LEN) - 1) << L::BASE;
    const CODE: &'static Code = &Code::UAt(L::BASE, L::LEN);
    #[inline(always)]
    fn eval(&self, cx: &Cx) -> L::T {
        L::T::from_word(cx.u.w[(L::BASE + self.0 as usize) & (WORDS - 1)])
    }
}

/// A constant word.
#[derive(Clone, Copy)]
pub struct K<const V: u32>;
// SAFETY: zero-sized.
unsafe impl<const V: u32> Words for K<V> {}
impl<const V: u32> Expr for K<V> {
    type Out = u32;
    type Stage = Const;
    const READS: u64 = 0;
    const OPAQUE: bool = V >> 24 == 255;
    const CODE: &'static Code = &Code::Lit(V);
    #[inline(always)]
    fn eval(&self, _: &Cx) -> u32 {
        V
    }
}
/// A constant signed value.
#[derive(Clone, Copy)]
pub struct Ki<const V: i32>;
// SAFETY: zero-sized.
unsafe impl<const V: i32> Words for Ki<V> {}
impl<const V: i32> Expr for Ki<V> {
    type Out = i32;
    type Stage = Const;
    const READS: u64 = 0;
    const CODE: &'static Code = &Code::Call("x_i", &[&Code::Lit(V as u32)]);
    #[inline(always)]
    fn eval(&self, _: &Cx) -> i32 {
        V
    }
}
/// A constant colour.
#[derive(Clone, Copy)]
pub struct Kc<const V: u32>;
// SAFETY: zero-sized.
unsafe impl<const V: u32> Words for Kc<V> {}
impl<const V: u32> Expr for Kc<V> {
    type Out = Color;
    type Stage = Const;
    const READS: u64 = 0;
    const OPAQUE: bool = V >> 24 == 255;
    const CODE: &'static Code = &Code::Lit(V);
    #[inline(always)]
    fn eval(&self, _: &Cx) -> Color {
        Color(V)
    }
}

/// Words the record owns.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct Rec<T = u32>(pub T);
// SAFETY: transparent over `T: Words`.
unsafe impl<T: Words> Words for Rec<T> {}
#[inline(always)]
pub fn rec<T: Words + PartialEq>(v: T) -> Rec<T> {
    Rec(v)
}
impl<T: Words + PartialEq> Expr for Rec<T> {
    type Out = T;
    type Stage = Mount;
    const READS: u64 = 0;
    const CODE: &'static Code = &Code::Rec;
    #[inline(always)]
    fn eval(&self, _: &Cx) -> T {
        self.0
    }
}

/// A Rust function of an expression. `F` is zero-sized (asserted when the type is instantiated). It has no GPU
/// code: its stage is the operand's stage as seen from the CPU (`Frame` becomes `Turn`).
#[repr(transparent)]
pub struct Map<F, A>(pub A, PhantomData<F>);
impl<F, A: Copy> Clone for Map<F, A> {
    #[inline(always)]
    fn clone(&self) -> Self {
        *self
    }
}
impl<F, A: Copy> Copy for Map<F, A> {}
// SAFETY: transparent over `A: Words`; `F` is only a marker.
unsafe impl<F: 'static, A: Words> Words for Map<F, A> {}
impl<F, A, O> Expr for Map<F, A>
where
    A: Expr,
    O: Copy + PartialEq,
    F: Fn(A::Out) -> O + Copy + 'static,
{
    type Out = O;
    type Stage = <A::Stage as Stage>::OnCpu;
    const READS: u64 = A::READS;
    const CODE: &'static Code = &Code::Cpu(&[A::CODE]);
    #[inline(always)]
    fn eval(&self, cx: &Cx) -> O {
        const { assert!(size_of::<F>() == 0, "a mapped function must capture nothing: put per-record data in a Rec operand") };
        // SAFETY: `F` is zero-sized (asserted above) and `Copy`, and a `Map<F, _>` is only built by `Expr::map`,
        // which was handed a value of `F`: the type is inhabited and copying that value is what this is.
        let f: F = unsafe { core::mem::zeroed() };
        f(self.0.eval(cx))
    }
    #[inline(always)]
    fn until(&self, cx: &Cx) -> u32 {
        self.0.until(cx)
    }
}

// Tuples of operands, for a `map` of several arguments.
impl<A: Expr, B: Expr> Expr for (A, B)
where
    (A::Stage, B::Stage): JoinAll,
{
    type Out = (A::Out, B::Out);
    type Stage = <(A::Stage, B::Stage) as JoinAll>::Out;
    const READS: u64 = A::READS | B::READS;
    const CODE: &'static Code = &Code::Cpu(&[A::CODE, B::CODE]);
    #[inline(always)]
    fn eval(&self, cx: &Cx) -> Self::Out {
        (self.0.eval(cx), self.1.eval(cx))
    }
    #[inline(always)]
    fn until(&self, cx: &Cx) -> u32 {
        self.0.until(cx).max(self.1.until(cx))
    }
}
impl<A: Expr, B: Expr, C: Expr> Expr for (A, B, C)
where
    (A::Stage, B::Stage, C::Stage): JoinAll,
{
    type Out = (A::Out, B::Out, C::Out);
    type Stage = <(A::Stage, B::Stage, C::Stage) as JoinAll>::Out;
    const READS: u64 = A::READS | B::READS | C::READS;
    const CODE: &'static Code = &Code::Cpu(&[A::CODE, B::CODE, C::CODE]);
    #[inline(always)]
    fn eval(&self, cx: &Cx) -> Self::Out {
        (self.0.eval(cx), self.1.eval(cx), self.2.eval(cx))
    }
    #[inline(always)]
    fn until(&self, cx: &Cx) -> u32 {
        self.0.until(cx).max(self.1.until(cx)).max(self.2.until(cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    lens!(Hover: u32 = 1);
    lens!(Sum[10]: i32 = 4);
    fn pick((h, c): (u32, u32)) -> Color {
        Color(if h == c { 0xff00_00ff } else { 0xffff_ffff })
    }
    #[test]
    fn value_is_record_words_and_stage_is_inferred() {
        let e = (un::<Hover>(), rec(7u32)).map(pick);
        assert_eq!(size_of_val(&e), 4);
        assert_eq!(e.words(), &[7]);
        fn stage<E: Expr>(_: &E) -> StageId {
            E::STAGE
        }
        assert_eq!(stage(&e), StageId::Turn); // a Rust function over a block word: CPU, on change
        assert_eq!(stage(&un::<Hover>()), StageId::Frame);
        assert_eq!(stage(&rec(1u32)), StageId::Mount);
        assert_eq!(stage(&K::<3>), StageId::Const);
        let mut u = U::default();
        u.set::<Hover>(7);
        assert_eq!(e.eval(&Cx::of(&u)), Color(0xff00_00ff));
        u.set_at::<Sum>(3, -5);
        assert_eq!(un_at::<Sum>(3).eval(&Cx::of(&u)), -5);
        assert_eq!(<UnAt<Sum> as Expr>::READS, 0x3ff << 4);
        assert_eq!(U::default().diff(&u, APP_WORDS), (1 << 1) | (1 << 7));
    }
}
