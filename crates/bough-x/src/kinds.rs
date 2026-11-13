//! Paint kinds: the closed set of things a record can be, each generic over the expressions of its operands.
//! A kind applied to expression types is an archetype ([`Arch`]).
use crate::expr::{Code, Color, Cx, Expr, Frame, Map, Slot, StageId, U, U_TIME, Words, later, latest, ramp_end};
use crate::reflect::Val;
use crate::store::{Arch, Backend, ColRef, Op, Rect};
use core::mem::offset_of;

pub trait FillArch: Arch {
    fn color(&self, u: &U) -> Color;
}
pub trait NumArch: Arch {
    const DECIMALS: u8;
    /// The least number of digits drawn, zeros on the left (`{:05}` is 5).
    const PAD: u8 = 0;
    /// The decimals of this record (a backend asks this, not the constant: a kind may keep them per record).
    #[inline(always)]
    fn decimals(&self) -> u8 {
        Self::DECIMALS
    }
    fn value(&self, u: &U) -> i32;
    fn color(&self, u: &U) -> Color;
}
pub trait LabelArch: Arch {
    fn text(&self) -> Txt;
    /// The text as bytes (what a backend draws).
    fn str(&self) -> &[u8];
    fn color(&self, u: &U) -> Color;
}
pub trait OutlineArch: Arch {
    const WIDTH: i32;
    /// The border's width in device pixels (a backend asks this, not the constant).
    #[inline(always)]
    fn width(&self, _u: &U) -> i32 {
        Self::WIDTH
    }
    fn color(&self, u: &U) -> Color;
}

/// A filled rectangle.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct Fill<E>(pub E);
// SAFETY: transparent over `E: Words`.
unsafe impl<E: Words> Words for Fill<E> {}
impl<E: Expr<Out = Color>> Arch for Fill<E> {
    const OPS: &'static [Op] = &[Op { name: "color", code: E::CODE, ty: core::any::type_name::<E>, stage: E::STAGE, reads: E::READS, word: 0, words: (size_of::<E>() / 4) as u16 }];
    const READS: u64 = E::READS;
    const NAME: &'static str = "fill";
    const STATE: bool = E::STATE;
    #[inline(always)]
    fn eq_ops(&self, o: &Self) -> bool {
        self.0.eq_ops(&o.0)
    }
    #[inline(always)]
    fn carry(&mut self, was: &Self, old: &U, new: &U) {
        self.0.carry(&was.0, &Cx::of(old), &Cx::of(new));
    }
    #[inline]
    fn slots(f: &mut dyn FnMut(Slot)) {
        E::slots(f);
    }
    const STAGE: StageId = E::STAGE;
    const TIMED: bool = E::READS >> U_TIME & 1 != 0;
    const ENDLESS: bool = !E::RESTS;
    #[inline(always)]
    fn until(&self, u: &U) -> u32 {
        self.0.until(&Cx::of(u))
    }
    #[inline(always)]
    fn changed(&self, old: &U, new: &U) -> bool {
        self.0.eval(&Cx::of(old)) != self.0.eval(&Cx::of(new))
    }
    #[inline(always)]
    fn paint<B: Backend>(b: &mut B, col: ColRef<'_, Self>, u: &U) {
        b.fill(col, u)
    }
    #[inline]
    fn val(&self, op: usize, u: &U) -> Val {
        if op == 0 { Val::Color(self.0.eval(&Cx::of(u)).0) } else { Val::None }
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
    const OPS: &'static [Op] = &[
        Op { name: "value", code: V::CODE, ty: core::any::type_name::<V>, stage: V::STAGE, reads: V::READS, word: 0, words: (size_of::<V>() / 4) as u16 },
        Op { name: "color", code: C::CODE, ty: core::any::type_name::<C>, stage: C::STAGE, reads: C::READS, word: (offset_of!(Self, 1) / 4) as u16, words: (size_of::<C>() / 4) as u16 },
    ];
    const READS: u64 = V::READS | C::READS;
    const NAME: &'static str = "num";
    const STATE: bool = V::STATE | C::STATE;
    #[inline(always)]
    fn eq_ops(&self, o: &Self) -> bool {
        self.0.eq_ops(&o.0) & self.1.eq_ops(&o.1)
    }
    #[inline(always)]
    fn carry(&mut self, was: &Self, old: &U, new: &U) {
        self.0.carry(&was.0, &Cx::of(old), &Cx::of(new));
        self.1.carry(&was.1, &Cx::of(old), &Cx::of(new));
    }
    #[inline]
    fn slots(f: &mut dyn FnMut(Slot)) {
        V::slots(f);
        C::slots(f);
    }
    const STAGE: StageId = later(&[V::STAGE, C::STAGE]);
    const TIMED: bool = (V::READS | C::READS) >> U_TIME & 1 != 0;
    const ENDLESS: bool = !(V::RESTS & C::RESTS);
    #[inline(always)]
    fn until(&self, u: &U) -> u32 {
        latest(self.0.until(&Cx::of(u)), self.1.until(&Cx::of(u)), u.w[U_TIME])
    }
    #[inline(always)]
    fn changed(&self, old: &U, new: &U) -> bool {
        (self.0.eval(&Cx::of(old)) != self.0.eval(&Cx::of(new))) | (self.1.eval(&Cx::of(old)) != self.1.eval(&Cx::of(new)))
    }
    #[inline(always)]
    fn paint<B: Backend>(b: &mut B, col: ColRef<'_, Self>, u: &U) {
        b.num(col, u)
    }
    #[inline]
    fn val(&self, op: usize, u: &U) -> Val {
        match op {
            0 => Val::Num(self.0.eval(&Cx::of(u)), D),
            1 => Val::Color(self.1.eval(&Cx::of(u)).0),
            _ => Val::None,
        }
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
    /// The text's bytes, in place.
    #[inline(always)]
    pub fn str(&self) -> &[u8] {
        // SAFETY: three words are twelve initialised bytes; on a little-endian target byte `i` is bits
        // `8 * (i % 4)` of word `i / 4`, which is how `new` packed them.
        let b = unsafe { core::slice::from_raw_parts(self.0.as_ptr() as *const u8, 12) };
        &b[..b.iter().position(|&c| c == 0).unwrap_or(12)]
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
    const OPS: &'static [Op] = &[
        Op { name: "text", code: &Code::Rec, ty: core::any::type_name::<Txt>, stage: StageId::Mount, reads: 0, word: 0, words: 3 },
        Op { name: "color", code: C::CODE, ty: core::any::type_name::<C>, stage: C::STAGE, reads: C::READS, word: (offset_of!(Self, 1) / 4) as u16, words: (size_of::<C>() / 4) as u16 },
    ];
    const READS: u64 = C::READS;
    const NAME: &'static str = "label";
    const STATE: bool = C::STATE;
    #[inline(always)]
    fn eq_ops(&self, o: &Self) -> bool {
        (self.0 == o.0) & self.1.eq_ops(&o.1)
    }
    #[inline(always)]
    fn carry(&mut self, was: &Self, old: &U, new: &U) {
        self.1.carry(&was.1, &Cx::of(old), &Cx::of(new));
    }
    #[inline]
    fn slots(f: &mut dyn FnMut(Slot)) {
        C::slots(f);
    }
    const STAGE: StageId = C::STAGE;
    const TIMED: bool = C::READS >> U_TIME & 1 != 0;
    const ENDLESS: bool = !C::RESTS;
    #[inline(always)]
    fn until(&self, u: &U) -> u32 {
        self.1.until(&Cx::of(u))
    }
    #[inline(always)]
    fn changed(&self, old: &U, new: &U) -> bool {
        self.1.eval(&Cx::of(old)) != self.1.eval(&Cx::of(new))
    }
    #[inline(always)]
    fn paint<B: Backend>(b: &mut B, col: ColRef<'_, Self>, u: &U) {
        b.label(col, u)
    }
    #[inline]
    fn val(&self, op: usize, u: &U) -> Val {
        match op {
            0 => Val::Text(self.0),
            1 => Val::Color(self.1.eval(&Cx::of(u)).0),
            _ => Val::None,
        }
    }
}
impl<C: Expr<Out = Color>> LabelArch for Label<C> {
    #[inline(always)]
    fn text(&self) -> Txt {
        self.0
    }
    #[inline(always)]
    fn str(&self) -> &[u8] {
        self.0.str()
    }
    #[inline(always)]
    fn color(&self, u: &U) -> Color {
        self.1.eval(&Cx::of(u))
    }
}

/// An expression that places a record: an offset. Its bound in `Time` is the expression's own `until`.
pub trait Motion: Expr<Out = (i32, i32)> {
    /// The corners of the box of offsets it stays inside from now until it rests. The default takes the path
    /// from here to the resting place to stay inside the box of the two (a segment, an eased glide); a motion
    /// that never rests is unbounded. A motion that overshoots says so here.
    #[inline(always)]
    fn bounds(&self, u: &U) -> ((i32, i32), (i32, i32)) {
        let (t, a) = (self.until(&Cx::of(u)), self.eval(&Cx::of(u)));
        if !crate::expr::ahead(t, u.w[U_TIME]) {
            return (a, a);
        }
        if t == u32::MAX {
            return ((Rect::EVERYWHERE.x0, Rect::EVERYWHERE.y0), (Rect::EVERYWHERE.x1, Rect::EVERYWHERE.y1));
        }
        let mut end = *u;
        end.w[U_TIME] = t;
        let b = self.eval(&Cx::of(&end));
        ((a.0.min(b.0), a.1.min(b.1)), (a.0.max(b.0), a.1.max(b.1)))
    }
}
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
    const RESTS: bool = F::RESTS & T::RESTS & S::RESTS;
    #[inline]
    fn slots(f: &mut dyn FnMut(Slot)) {
        F::slots(f);
        T::slots(f);
        S::slots(f)
    }
    const CODE: &'static Code = &Code::Call("x_glide", &[F::CODE, T::CODE, S::CODE, &Code::U(U_TIME), &Code::Lit(DUR)]);
    #[inline(always)]
    fn eval(&self, cx: &Cx) -> (i32, i32) {
        let (a, b) = (self.0.eval(cx), self.1.eval(cx));
        // progress in 1/256ths, saturating at both ends
        let dt = cx.u.w[U_TIME].wrapping_sub(self.2.eval(cx));
        let p = if dt >= DUR { 256 } else { (dt as u64 * 256 / DUR as u64) as i64 };
        (a.0 + ((b.0 as i64 - a.0 as i64) * p >> 8) as i32, a.1 + ((b.1 as i64 - a.1 as i64) * p >> 8) as i32)
    }
    #[inline(always)]
    fn until(&self, cx: &Cx) -> u32 {
        let now = cx.u.w[U_TIME];
        latest(latest(ramp_end(self.2.eval(cx), DUR, now), self.0.until(cx), now), self.1.until(cx), now)
    }
}
impl<const DUR: u32, F: Expr<Out = (i32, i32)>, T: Expr<Out = (i32, i32)>, S: Expr<Out = u32>> Motion for Glide<DUR, F, T, S> {}

/// A colour going from `from` to `to` over `DUR` ticks of `Time` starting at `t0`, per channel, in integers.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Tween<const DUR: u32, F, T, S>(pub F, pub T, pub S);
// SAFETY: `repr(C)` over `Words` operands.
unsafe impl<const DUR: u32, F: Words, T: Words, S: Words> Words for Tween<DUR, F, T, S> {}
impl<const DUR: u32, F: Expr<Out = Color>, T: Expr<Out = Color>, S: Expr<Out = u32>> Expr for Tween<DUR, F, T, S> {
    type Out = Color;
    type Stage = Frame;
    const READS: u64 = F::READS | T::READS | S::READS | 1 << U_TIME;
    const RESTS: bool = F::RESTS & T::RESTS & S::RESTS;
    #[inline]
    fn slots(f: &mut dyn FnMut(Slot)) {
        F::slots(f);
        T::slots(f);
        S::slots(f)
    }
    const CODE: &'static Code = &Code::Call("x_tween", &[F::CODE, T::CODE, S::CODE, &Code::U(U_TIME), &Code::Lit(DUR)]);
    #[inline(always)]
    fn eval(&self, cx: &Cx) -> Color {
        let (a, b) = (self.0.eval(cx).0, self.1.eval(cx).0);
        let dt = cx.u.w[U_TIME].wrapping_sub(self.2.eval(cx));
        let p = if dt >= DUR { 256 } else { (dt as u64 * 256 / DUR as u64) as u32 };
        let ch = |s: u32| {
            let (x, y) = ((a >> s & 0xff) as i32, (b >> s & 0xff) as i32);
            ((x + ((y - x) * p as i32 >> 8)) as u32 & 0xff) << s
        };
        Color(ch(0) | ch(8) | ch(16) | ch(24))
    }
    #[inline(always)]
    fn until(&self, cx: &Cx) -> u32 {
        let now = cx.u.w[U_TIME];
        latest(latest(ramp_end(self.2.eval(cx), DUR, now), self.0.until(cx), now), self.1.until(cx), now)
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
    const OPS: &'static [Op] = &[
        Op { name: "color", code: C::CODE, ty: core::any::type_name::<C>, stage: C::STAGE, reads: C::READS, word: 0, words: (size_of::<C>() / 4) as u16 },
        Op { name: "offset", code: T::CODE, ty: core::any::type_name::<T>, stage: T::STAGE, reads: T::READS, word: (offset_of!(Self, 1) / 4) as u16, words: (size_of::<T>() / 4) as u16 },
    ];
    const READS: u64 = C::READS | T::READS;
    const MOVES: bool = T::READS != 0;
    const TIMED: bool = (T::READS | C::READS) >> U_TIME & 1 != 0;
    const ENDLESS: bool = !(T::RESTS & C::RESTS);
    #[inline(always)]
    fn until(&self, u: &U) -> u32 {
        latest(self.1.until(&Cx::of(u)), self.0.until(&Cx::of(u)), u.w[U_TIME])
    }
    #[inline(always)]
    fn sweep(&self, r: Rect, u: &U) -> Rect {
        let (lo, hi) = self.1.bounds(u);
        #[cfg(feature = "mutant_moves")]
        let lo = hi;
        r.shift(lo).or(r.shift(hi))
    }
    const NAME: &'static str = "outline";
    const STATE: bool = C::STATE | T::STATE;
    #[inline(always)]
    fn eq_ops(&self, o: &Self) -> bool {
        self.0.eq_ops(&o.0) & self.1.eq_ops(&o.1)
    }
    #[inline(always)]
    fn carry(&mut self, was: &Self, old: &U, new: &U) {
        self.0.carry(&was.0, &Cx::of(old), &Cx::of(new));
        self.1.carry(&was.1, &Cx::of(old), &Cx::of(new));
    }
    #[inline]
    fn slots(f: &mut dyn FnMut(Slot)) {
        C::slots(f);
        T::slots(f);
    }
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
    #[inline]
    fn val(&self, op: usize, u: &U) -> Val {
        match op {
            0 => Val::Color(self.0.eval(&Cx::of(u)).0),
            1 => {
                let (x, y) = self.1.eval(&Cx::of(u));
                Val::Pair(x, y)
            }
            _ => Val::None,
        }
    }
}
impl<const W: i32, C: Expr<Out = Color>, T: Motion> OutlineArch for Outline<W, C, T> {
    const WIDTH: i32 = W;
    #[inline(always)]
    fn color(&self, u: &U) -> Color {
        self.0.eval(&Cx::of(u))
    }
}
