//! The element layer: the façade's way of writing a view, on this core.
//!
//! The author writes what the façade has: `fn view(&self) -> impl Element<Self> + '_`, `div()`, `row((..))`,
//! `.bg(sig!(self.look).map(|l| ..))`, `text(..)`, `.on_hover(|v, e| ..)`, `each(iter.map(row))`, plain state in
//! [`Signal`] fields that handlers set. Nothing here is a second runtime: an element is a value whose type is
//! both its records ([`Row`]) and its layout ([`bough_lay::Lay`]), and [`Element::drive`] hands it to whatever
//! sink is asking (mount, re-description, a turn's list pass, the immediate-mode oracle).
//!
//! What each construct becomes:
//!
//! * `sig!(self.field)` is a lens whose word of the block is found from the field's byte offset in the view
//!   (`offset_of!`, a constant of the type). The block is gathered from the view's own fields before a turn is
//!   settled, so a handler's write is a plain store and is noticed by the compare. No field is numbered.
//! * `.map(closure)` keeps the closure's captures as the record's own words (a `Copy` closure is plain bytes),
//!   so `move |l| fill(*l, col, row)` is one archetype for every cell, with `col` and `row` in its record.
//! * `.transition(curve)` keeps where the value started from, and when, in the record. A turn that moves the
//!   target restarts it from what the record shows; frames in between evaluate nothing on the CPU.
//! * `each(iter)` under a scrolled box is a list by exposure: the iterator is indexed, never walked
//!   (`TrustedRandomAccess`), so a hundred thousand rows cost what is on screen.
//! * a handler is a component of the box's record; its captures are a lane beside the column.
//!
//! What cannot be carried from the façade unchanged is listed in the expr journal, with the reason for each.
use crate::event::{Click, Drag, Handler, Hover, On, OnFocus, OnKey, OnText, Press, Target, Wheel};
use crate::expr::{APP_WORDS, Code, Color, Cx as ECx, Expr, Lens, Slot, Turn, U, U_SCALE, U_TIME, Un, Word, Words};
use crate::kinds::{Fill, LabelArch, Motion, NumArch, OutlineArch, Txt};
use crate::sink::{Cx, Geo, Leaf, Mk, Row, Sink};
use crate::store::{Arch, Backend, ColRef, POINTS, Rect, Site, Space};
use bough_lay::flex::{Spec, rect};
use bough_lay::val::{Av, Cx as LCx, V as LV};
use bough_lay::{Chain, LBox, Lay, Out, Rec1};
use core::marker::PhantomData;
use core::sync::atomic::{AtomicU32, Ordering};
use std::iter::TrustedRandomAccessNoCoerce;

pub use bough_lay::{Align, Flex, Justify, block, column, div, row};

// ---- state ---------------------------------------------------------------------------------------------------

/// How many [`Signal::set`] calls changed a value, ever. A turn compares the count before and after its
/// handlers with the number of block words that changed: if every set is accounted for by a changed word of
/// the block, the damage is the block's alone; if not, some state the view reads elsewhere changed and the
/// exposed view is described again.
pub static SETS: AtomicU32 = AtomicU32::new(0);

/// A field of state. It is the value and nothing else: no subscribers, no wires. `set` is a store.
#[repr(transparent)]
#[derive(Clone, Copy, Default, Debug)]
pub struct Signal<T>(T);
impl<T> Signal<T> {
    pub const fn new(v: T) -> Signal<T> {
        Signal(v)
    }
    pub fn into_inner(self) -> T {
        self.0
    }
    /// Overwrite without counting: for a value the framework itself wrote back.
    #[inline(always)]
    pub fn set_silent(&mut self, v: T) {
        self.0 = v;
    }
}
impl<T: PartialEq> Signal<T> {
    /// Store `v`. Returns whether the value changed.
    #[inline(always)]
    pub fn set(&mut self, v: T) -> bool {
        if self.0 == v {
            return false;
        }
        self.0 = v;
        SETS.fetch_add(1, Ordering::Relaxed);
        true
    }
}
impl<T> core::ops::Deref for Signal<T> {
    type Target = T;
    #[inline(always)]
    fn deref(&self) -> &T {
        &self.0
    }
}

/// A field of the view named by its path: `sig!(self.look)`. The offset is a constant of the type.
pub struct Sig<'a, V, T, const OFF: usize> {
    r: &'a T,
    _v: PhantomData<fn(&V)>,
}
/// SAFETY (of the macro's use, not of this function): `OFF` is `offset_of!(V, path)` for the path `p` reads.
#[inline(always)]
pub fn prove<'a, V, T, const OFF: usize>(r: &'a T, _p: fn(&V) -> &T) -> Sig<'a, V, T, OFF> {
    Sig { r, _v: PhantomData }
}
/// `sig!(self.field)`: the field as an operand. In a view whose type has generic parameters the compiler
/// cannot name `Self` in a constant: write `sig!(self: TheType => field)`.
#[macro_export]
macro_rules! sig {
    ($o:ident : $t:ty => $($f:ident).+) => {{
        let r = &$o.$($f).+;
        let p: fn(&$t) -> &_ = |o: &$t| &o.$($f).+;
        $crate::el::prove::<$t, _, { ::core::mem::offset_of!($t, $($f).+) }>(r, p)
    }};
    ($s:tt . $($f:ident).+) => {{
        let r = &$s.$($f).+;
        let p: fn(&Self) -> &_ = |o: &Self| &o.$($f).+;
        $crate::el::prove::<Self, _, { ::core::mem::offset_of!(Self, $($f).+) }>(r, p)
    }};
}

/// The lens of a field at byte offset `OFF` of the view `V`: its word of the block is the offset's word,
/// folded into the application's words.
pub struct At<V, X, const OFF: usize>(PhantomData<fn(&V) -> X>);
impl<V, X, const OFF: usize> Clone for At<V, X, OFF> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<V, X, const OFF: usize> Copy for At<V, X, OFF> {}
impl<V: 'static, X: Word, const OFF: usize> Lens for At<V, X, OFF> {
    type T = X;
    const SLOT: usize = (OFF / 4) % APP_WORDS;
    const AT: Option<(usize, usize)> = {
        assert!(OFF % 4 == 0 || size_of::<X>() == 1, "Frame state is whole words: a field read by sig! is aligned to 4");
        assert!(size_of::<X>() <= 4);
        Some((OFF, size_of::<X>()))
    };
}

/// An operand whose block words (if any) are fields of the view `V`. A field path proved against one view
/// cannot end up in another view's records.
pub trait Rooted<V> {}
impl<V> Rooted<V> for Color {}

impl<'a, V: 'static, X: Word, const OFF: usize> Sig<'a, V, Signal<X>, OFF> {
    /// The field through a function. The closure may capture: its captures are the record's own words.
    #[inline(always)]
    pub fn map<O: Copy + PartialEq, F: Fn(&X) -> O + Copy + 'static>(self, f: F) -> MapRef<F, At<V, X, OFF>> {
        MapRef(f, PhantomData)
    }
    /// The field itself as an expression.
    #[inline(always)]
    pub fn get(self) -> Un<At<V, X, OFF>> {
        crate::expr::un()
    }
    /// The field as a number to show (read now: a record's own value).
    #[inline(always)]
    pub fn num<F: Fn(&X) -> f32>(self, f: F, fmt: NumFmt) -> NumText {
        fmt.of(f(&self.r.0))
    }
    /// The word of the block the field is.
    #[inline(always)]
    pub fn word(&self) -> u8 {
        <At<V, X, OFF> as Lens>::SLOT as u8
    }
}
impl<'a, V: 'static, X: Word, const N: usize, const OFF: usize> Sig<'a, V, [Signal<X>; N], OFF> {
    /// One element of an array field.
    #[inline(always)]
    pub fn at(self, i: usize) -> SigAt<'a, X> {
        SigAt(&self.r[i].0)
    }
}
/// One element of an array field of signals: its value now.
pub struct SigAt<'a, X>(&'a X);
impl<X: Word> SigAt<'_, X> {
    #[inline(always)]
    pub fn num<F: Fn(&X) -> f32>(self, f: F, fmt: NumFmt) -> NumText {
        fmt.of(f(self.0))
    }
}

/// A field through a closure. The value of this type is the closure: its captures are the record's words.
#[repr(transparent)]
pub struct MapRef<F, L>(F, PhantomData<L>);
impl<F: Copy, L> Clone for MapRef<F, L> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<F: Copy, L> Copy for MapRef<F, L> {}
// SAFETY: transparent over `F`, whose size and alignment are asserted to be whole words where it is evaluated
// (`eval` below). A closure's captures must not leave padding between them: `u32`, `i32`, `f32` and `Color`
// captures do not; this is the one thing the compiler is not asked to prove (see the journal).
unsafe impl<F: Copy + 'static, L: 'static> Words for MapRef<F, L> {}
impl<F, L, O> Expr for MapRef<F, L>
where
    L: Lens,
    O: Copy + PartialEq,
    F: Fn(&L::T) -> O + Copy + 'static,
{
    type Out = O;
    type Stage = Turn;
    const READS: u64 = 1 << L::SLOT;
    const CODE: &'static Code = &Code::Cpu(&[&Code::U(L::SLOT)]);
    #[inline(always)]
    fn eval(&self, cx: &ECx) -> O {
        const { assert!(size_of::<F>() % 4 == 0 && align_of::<F>() <= 4, "a mapped closure's captures are record words: capture u32, i32, f32 or Color values (not references, usize or u8)") };
        (self.0)(&cx.u.get::<L>())
    }
    #[inline]
    fn slots(f: &mut dyn FnMut(Slot)) {
        <Un<L> as Expr>::slots(f)
    }
}
impl<V: 'static, X: Word, F, const OFF: usize> Rooted<V> for MapRef<F, At<V, X, OFF>> {}
impl<V: 'static, X: Word, const OFF: usize> Rooted<V> for Un<At<V, X, OFF>> {}

// ---- transitions ---------------------------------------------------------------------------------------------

/// How a transition runs: its length and its easing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Curve {
    pub ms: u32,
    pub ease_out: bool,
}
impl Curve {
    /// Ease out over `secs` seconds.
    pub const fn out(secs: f32) -> Curve {
        Curve { ms: (secs * 1000.0) as u32, ease_out: true }
    }
    pub const fn linear(secs: f32) -> Curve {
        Curve { ms: (secs * 1000.0) as u32, ease_out: false }
    }
}

/// A value that can be part way between two values. `p` is progress in 1/256ths.
pub trait Lerp: Copy + PartialEq {
    type W: Words;
    fn to_w(self) -> Self::W;
    fn from_w(w: Self::W) -> Self;
    fn lerp(a: Self, b: Self, p: u32) -> Self;
}
impl Lerp for Color {
    type W = u32;
    #[inline(always)]
    fn to_w(self) -> u32 {
        self.0
    }
    #[inline(always)]
    fn from_w(w: u32) -> Color {
        Color(w)
    }
    #[inline(always)]
    fn lerp(a: Color, b: Color, p: u32) -> Color {
        let ch = |s: u32| {
            let (x, y) = ((a.0 >> s & 0xff) as i32, (b.0 >> s & 0xff) as i32);
            ((x + ((y - x) * p as i32 >> 8)) as u32 & 0xff) << s
        };
        Color(ch(0) | ch(8) | ch(16) | ch(24))
    }
}
impl Lerp for (f32, f32) {
    type W = [f32; 2];
    #[inline(always)]
    fn to_w(self) -> [f32; 2] {
        [self.0, self.1]
    }
    #[inline(always)]
    fn from_w(w: [f32; 2]) -> (f32, f32) {
        (w[0], w[1])
    }
    #[inline(always)]
    fn lerp(a: (f32, f32), b: (f32, f32), p: u32) -> (f32, f32) {
        let t = p as f32 / 256.0;
        (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
    }
}

/// `e.transition(curve)`: the value of `e`, reached from wherever it was over the curve's length. Where it
/// started from and when are words of the record, written by the core in the turn that moves the target
/// ([`Expr::carry`]); `t0 == 0` is "at rest".
#[repr(C)]
pub struct Tr<E: Expr>
where
    E::Out: Lerp,
{
    e: E,
    from: <E::Out as Lerp>::W,
    t0: u32,
    /// Milliseconds; the top bit is ease-out.
    dur: u32,
}
impl<E: Expr> Clone for Tr<E>
where
    E::Out: Lerp,
{
    fn clone(&self) -> Self {
        *self
    }
}
impl<E: Expr> Copy for Tr<E> where E::Out: Lerp {}
// SAFETY: `repr(C)` over `Words` fields of alignment at most 4: no padding.
unsafe impl<E: Expr> Words for Tr<E> where E::Out: Lerp {}
impl<E: Expr> Tr<E>
where
    E::Out: Lerp,
{
    #[inline(always)]
    fn progress(&self, now: u32) -> Option<u32> {
        let dur = self.dur & 0x7fff_ffff;
        let dt = now.wrapping_sub(self.t0);
        if self.t0 == 0 || dt >= dur {
            return None;
        }
        let p = (dt as u64 * 256 / dur as u64) as u32;
        Some(if self.dur >> 31 != 0 {
            // ease out, cubic: 1 - (1 - p)^3
            let q = 256 - p;
            256 - (q * q * q >> 16)
        } else {
            p
        })
    }
}
impl<E: Expr> Expr for Tr<E>
where
    E::Out: Lerp,
{
    type Out = E::Out;
    type Stage = Turn;
    const READS: u64 = E::READS | 1 << U_TIME;
    const RESTS: bool = E::RESTS;
    const STATE: bool = true;
    const CODE: &'static Code = &Code::Cpu(&[E::CODE, &Code::U(U_TIME)]);
    #[inline(always)]
    fn eval(&self, cx: &ECx) -> E::Out {
        let to = self.e.eval(cx);
        match self.progress(cx.u.w[U_TIME]) {
            None => to,
            Some(p) => Lerp::lerp(<E::Out as Lerp>::from_w(self.from), to, p),
        }
    }
    #[inline(always)]
    fn until(&self, cx: &ECx) -> u32 {
        let now = cx.u.w[U_TIME];
        let mine = if self.t0 == 0 { 0 } else { crate::expr::ramp_end(self.t0, self.dur & 0x7fff_ffff, now) };
        crate::expr::latest(mine, self.e.until(cx), now)
    }
    #[inline]
    fn slots(f: &mut dyn FnMut(Slot)) {
        E::slots(f)
    }
    #[inline(always)]
    fn eq_ops(&self, o: &Self) -> bool {
        (self.dur == o.dur) & if E::STATE { self.e.eq_ops(&o.e) } else { self.e.same(&o.e) }
    }
    #[inline(always)]
    fn carry(&mut self, was: &Self, old: &ECx, new: &ECx) {
        if E::STATE {
            self.e.carry(&was.e, old, new);
        }
        if self.e.eval(new) != was.e.eval(old) {
            // another target: go there from what is on screen now
            self.from = was.eval(old).to_w();
            self.t0 = new.u.w[U_TIME].max(1);
        } else {
            (self.from, self.t0) = (was.from, was.t0);
        }
    }
}
impl<V, E: Expr + Rooted<V>> Rooted<V> for Tr<E> where E::Out: Lerp {}

/// `.transition(curve)` on any operand whose value can be part way.
pub trait Transition: Expr + Sized
where
    Self::Out: Lerp,
{
    #[inline(always)]
    fn transition(self, c: Curve) -> Tr<Self> {
        // SAFETY: `W: Words`: zero bytes are a valid value.
        Tr { e: self, from: unsafe { core::mem::zeroed() }, t0: 0, dur: c.ms.max(1) | (c.ease_out as u32) << 31 }
    }
}
impl<E: Expr> Transition for E where E::Out: Lerp {}

/// Points to device pixels: an offset in points as the pixels a record moves by.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct Px<E>(pub E);
// SAFETY: transparent over `E: Words`.
unsafe impl<E: Words> Words for Px<E> {}
impl<E: Expr<Out = (f32, f32)>> Expr for Px<E> {
    type Out = (i32, i32);
    type Stage = Turn;
    const READS: u64 = E::READS | 1 << U_SCALE;
    const RESTS: bool = E::RESTS;
    const STATE: bool = E::STATE;
    const CODE: &'static Code = &Code::Cpu(&[E::CODE, &Code::U(U_SCALE)]);
    #[inline(always)]
    fn eval(&self, cx: &ECx) -> (i32, i32) {
        let (x, y) = self.0.eval(cx);
        let s = f32::from_bits(cx.u.w[U_SCALE]);
        (bough_lay::val::round(x * s), bough_lay::val::round(y * s))
    }
    #[inline(always)]
    fn until(&self, cx: &ECx) -> u32 {
        self.0.until(cx)
    }
    #[inline]
    fn slots(f: &mut dyn FnMut(Slot)) {
        E::slots(f)
    }
    #[inline(always)]
    fn eq_ops(&self, o: &Self) -> bool {
        self.0.eq_ops(&o.0)
    }
    #[inline(always)]
    fn carry(&mut self, was: &Self, old: &ECx, new: &ECx) {
        self.0.carry(&was.0, old, new)
    }
}
impl<E: Expr<Out = (f32, f32)>> Motion for Px<E> {}
impl<V, E: Rooted<V>> Rooted<V> for Px<E> {}

/// A record that does not move.
#[derive(Clone, Copy)]
pub struct Still;
// SAFETY: zero-sized.
unsafe impl Words for Still {}
impl Expr for Still {
    type Out = (i32, i32);
    type Stage = crate::expr::Const;
    const READS: u64 = 0;
    const CODE: &'static Code = &Code::Lit(0);
    #[inline(always)]
    fn eval(&self, _: &ECx) -> (i32, i32) {
        (0, 0)
    }
}
impl Motion for Still {}
impl<V> Rooted<V> for Still {}

// ---- paints --------------------------------------------------------------------------------------------------

/// `rgb(0xRRGGBB)`.
#[inline(always)]
pub const fn rgb(v: u32) -> Color {
    Color(v)
}

/// What `.bg(..)` was given: the record it makes.
pub trait Paints<V> {
    type A: Arch;
    fn arch(&self) -> Self::A;
}
impl<V, E: Expr<Out = Color> + Rooted<V>> Paints<V> for E {
    type A = Fill<E>;
    #[inline(always)]
    fn arch(&self) -> Fill<E> {
        Fill(*self)
    }
}

/// A border of `w` points in colour `c`, moved by `t` (pixels).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Bord<C, T> {
    pub w: f32,
    pub c: C,
    pub t: T,
}
// SAFETY: `repr(C)` over `Words` fields.
unsafe impl<C: Words, T: Words> Words for Bord<C, T> {}
impl<C: Expr<Out = Color>, T: Motion> Arch for Bord<C, T> {
    const READS: u64 = C::READS | T::READS;
    const MOVES: bool = T::READS != 0;
    const TIMED: bool = (T::READS | C::READS) >> U_TIME & 1 != 0;
    const ENDLESS: bool = !(T::RESTS & C::RESTS);
    const NAME: &'static str = "outline";
    const STAGE: crate::expr::StageId = crate::expr::later(&[T::STAGE, C::STAGE]);
    const STATE: bool = C::STATE | T::STATE;
    #[inline(always)]
    fn eq_ops(&self, o: &Self) -> bool {
        (self.w == o.w) & self.c.eq_ops(&o.c) & self.t.eq_ops(&o.t)
    }
    #[inline(always)]
    fn carry(&mut self, was: &Self, old: &U, new: &U) {
        self.c.carry(&was.c, &ECx::of(old), &ECx::of(new));
        self.t.carry(&was.t, &ECx::of(old), &ECx::of(new));
    }
    #[inline]
    fn slots(f: &mut dyn FnMut(Slot)) {
        C::slots(f);
        T::slots(f)
    }
    #[inline(always)]
    fn until(&self, u: &U) -> u32 {
        crate::expr::latest(self.t.until(&ECx::of(u)), self.c.until(&ECx::of(u)), u.w[U_TIME])
    }
    #[inline(always)]
    fn sweep(&self, r: Rect, u: &U) -> Rect {
        let (lo, hi) = self.t.bounds(u);
        r.shift(lo).or(r.shift(hi))
    }
    #[inline(always)]
    fn changed(&self, old: &U, new: &U) -> bool {
        self.c.eval(&ECx::of(old)) != self.c.eval(&ECx::of(new))
    }
    #[inline(always)]
    fn offset(&self, u: &U) -> (i32, i32) {
        self.t.eval(&ECx::of(u))
    }
    #[inline(always)]
    fn paint<B: Backend>(b: &mut B, col: ColRef<'_, Self>, u: &U) {
        b.outline(col, u)
    }
}
impl<C: Expr<Out = Color>, T: Motion> OutlineArch for Bord<C, T> {
    const WIDTH: i32 = 0;
    #[inline(always)]
    fn width(&self, u: &U) -> i32 {
        bough_lay::val::round(self.w * f32::from_bits(u.w[U_SCALE])).max(1)
    }
    #[inline(always)]
    fn color(&self, u: &U) -> Color {
        self.c.eval(&ECx::of(u))
    }
}
impl<V, C: Expr<Out = Color> + Rooted<V>, T: Motion + Rooted<V>> Paints<V> for Bord<C, T> {
    type A = Bord<C, T>;
    #[inline(always)]
    fn arch(&self) -> Self {
        *self
    }
}

/// The façade's calls on a box that are paint, not layout.
pub trait Deco<P, C, H>: Sized {
    /// A border of `w` points. (First cut: a bordered box has no fill.)
    fn border<Cl>(self, w: f32, c: Cl) -> LBox<Rec1<Bord<Cl, Still>>, C, H>;
    /// Positioned by its parent's layout as usual. (The façade's marker for an `abs` child's container.)
    fn relative(self) -> Self {
        self
    }
}
impl<P, C, H> Deco<P, C, H> for LBox<P, C, H> {
    #[inline(always)]
    fn border<Cl>(self, w: f32, c: Cl) -> LBox<Rec1<Bord<Cl, Still>>, C, H> {
        self.bg(Bord { w, c, t: Still })
    }
}
/// `.translate(offset in points)` on a bordered box: it moves without layout.
pub trait Translate<Cl, C, H> {
    fn translate<E: Expr<Out = (f32, f32)>>(self, e: E) -> LBox<Rec1<Bord<Cl, Px<E>>>, C, H>;
}
impl<Cl: Copy, C, H> Translate<Cl, C, H> for LBox<Rec1<Bord<Cl, Still>>, C, H> {
    #[inline(always)]
    fn translate<E: Expr<Out = (f32, f32)>>(self, e: E) -> LBox<Rec1<Bord<Cl, Px<E>>>, C, H> {
        let b = self.paint.0;
        self.bg(Bord { w: b.w, c: b.c, t: Px(e) })
    }
}

// ---- text ----------------------------------------------------------------------------------------------------

/// How a number is shown. (The façade's: a prefix, decimals, a suffix. Prefix and suffix are not drawn yet.)
#[derive(Clone, Copy, Debug)]
pub struct NumFmt {
    pub decimals: u8,
}
impl NumFmt {
    pub const fn new(_prefix: &'static str, decimals: Option<u8>, _suffix: &'static str) -> NumFmt {
        NumFmt { decimals: match decimals {
            Some(d) => d,
            None => 0,
        } }
    }
    #[inline(always)]
    pub fn of(self, v: f32) -> NumText {
        let mut k = 1.0f64;
        let mut i = 0;
        while i < self.decimals {
            k *= 10.0;
            i += 1;
        }
        NumText { value: (v as f64 * k).round() as i32, decimals: self.decimals as u32 }
    }
}
/// A number to show: an integer scaled by `10^decimals`, drawn with tabular digits while shading.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NumText {
    pub value: i32,
    pub decimals: u32,
}

/// What a text shows.
pub trait Content {
    type A<C: Expr<Out = Color>>: Arch;
    fn arch<C: Expr<Out = Color>>(&self, c: C) -> Self::A<C>;
    /// Characters, for the stand-in measure (the text lane's `Measure` replaces it).
    fn chars(&self) -> usize;
}

/// A number record: the value and its decimals are the record's.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct NumRec<C> {
    v: i32,
    d: u32,
    c: C,
}
// SAFETY: `repr(C)` over `Words` fields.
unsafe impl<C: Words> Words for NumRec<C> {}
/// A text record: interned bytes (address and length) and a colour.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct StrRec<C> {
    at: [u32; 2],
    len: u32,
    c: C,
}
// SAFETY: `repr(C)` over `Words` fields.
unsafe impl<C: Words> Words for StrRec<C> {}

macro_rules! color_arch {
    ($t:ident, $name:literal, $method:ident, |$s:ident, $o:ident| $eq:expr) => {
        impl<C: Expr<Out = Color>> Arch for $t<C> {
            const READS: u64 = C::READS;
            const NAME: &'static str = $name;
            const STAGE: crate::expr::StageId = C::STAGE;
            const TIMED: bool = C::READS >> U_TIME & 1 != 0;
            const ENDLESS: bool = !C::RESTS;
            const STATE: bool = C::STATE;
            #[inline(always)]
            fn eq_ops(&self, o: &Self) -> bool {
                let ($s, $o) = (self, o);
                $eq & self.c.eq_ops(&o.c)
            }
            #[inline(always)]
            fn carry(&mut self, was: &Self, old: &U, new: &U) {
                self.c.carry(&was.c, &ECx::of(old), &ECx::of(new));
            }
            #[inline]
            fn slots(f: &mut dyn FnMut(Slot)) {
                C::slots(f)
            }
            #[inline(always)]
            fn until(&self, u: &U) -> u32 {
                self.c.until(&ECx::of(u))
            }
            #[inline(always)]
            fn changed(&self, old: &U, new: &U) -> bool {
                self.c.eval(&ECx::of(old)) != self.c.eval(&ECx::of(new))
            }
            #[inline(always)]
            fn paint<B: Backend>(b: &mut B, col: ColRef<'_, Self>, u: &U) {
                b.$method(col, u)
            }
        }
    };
}
color_arch!(NumRec, "num", num, |s, o| (s.v == o.v) & (s.d == o.d));
color_arch!(StrRec, "label", label, |s, o| (s.at == o.at) & (s.len == o.len));
impl<C: Expr<Out = Color>> NumArch for NumRec<C> {
    const DECIMALS: u8 = 0;
    #[inline(always)]
    fn decimals(&self) -> u8 {
        self.d as u8
    }
    #[inline(always)]
    fn value(&self, _: &U) -> i32 {
        self.v
    }
    #[inline(always)]
    fn color(&self, u: &U) -> Color {
        self.c.eval(&ECx::of(u))
    }
}
impl<C: Expr<Out = Color>> LabelArch for StrRec<C> {
    fn text(&self) -> Txt {
        let b = self.str();
        let mut t = [0u32; 3];
        for (i, &c) in b.iter().take(12).enumerate() {
            t[i / 4] |= (c as u32) << (8 * (i % 4));
        }
        Txt(t)
    }
    #[inline(always)]
    fn str(&self) -> &[u8] {
        // SAFETY: `at` and `len` were taken from an interned string, which lives for the process.
        unsafe { core::slice::from_raw_parts((self.at[0] as usize | (self.at[1] as usize) << 32) as *const u8, self.len as usize) }
    }
    #[inline(always)]
    fn color(&self, u: &U) -> Color {
        self.c.eval(&ECx::of(u))
    }
}
impl Content for NumText {
    type A<C: Expr<Out = Color>> = NumRec<C>;
    #[inline(always)]
    fn arch<C: Expr<Out = Color>>(&self, c: C) -> NumRec<C> {
        NumRec { v: self.value, d: self.decimals, c }
    }
    fn chars(&self) -> usize {
        12
    }
}
/// A string kept for the life of the process: equal strings are one address, so a text record is three words
/// and comparing two is comparing those. (First cut: nothing is ever released; the text lane's shaped-run
/// store is what a record will hold.)
pub fn intern(s: &str) -> &'static str {
    use std::collections::HashSet;
    use std::sync::Mutex;
    static SET: Mutex<Option<HashSet<&'static str>>> = Mutex::new(None);
    let mut g = SET.lock().unwrap();
    let set = g.get_or_insert_with(HashSet::new);
    if let Some(&k) = set.get(s) {
        return k;
    }
    let k: &'static str = Box::leak(s.to_owned().into_boxed_str());
    set.insert(k);
    k
}
/// Interned text.
#[derive(Clone, Copy)]
pub struct Str(&'static str);
impl Content for Str {
    type A<C: Expr<Out = Color>> = StrRec<C>;
    #[inline(always)]
    fn arch<C: Expr<Out = Color>>(&self, c: C) -> StrRec<C> {
        let p = self.0.as_ptr() as usize;
        StrRec { at: [p as u32, (p >> 32) as u32], len: self.0.len() as u32, c }
    }
    fn chars(&self) -> usize {
        self.0.chars().count()
    }
}
/// What `text(..)` accepts.
pub trait IntoContent {
    type K: Content;
    fn content(self) -> Self::K;
}
impl IntoContent for &'static str {
    type K = Str;
    #[inline(always)]
    fn content(self) -> Str {
        // a literal is already where it will stay
        Str(self)
    }
}
impl IntoContent for String {
    type K = Str;
    fn content(self) -> Str {
        Str(intern(&self))
    }
}
impl IntoContent for NumText {
    type K = NumText;
    #[inline(always)]
    fn content(self) -> NumText {
        self
    }
}

/// Text. One record.
#[derive(Clone, Copy)]
pub struct Text<K, C> {
    pub spec: Spec,
    k: K,
    c: C,
    /// Line height in points (0: the content's).
    line: f32,
}
/// `text("label")`, `text(format!(..))`, `text(sig!(self.cells).at(i).num(|v| *v, FMT))`.
#[inline(always)]
pub fn text<T: IntoContent>(t: T) -> Text<T::K, Color> {
    Text { spec: Spec::new(false), k: t.content(), c: Color(0), line: 0.0 }
}
impl<K, C> Text<K, C> {
    /// Size and line height, in points. (The stand-in raster has one size; the line height is laid out.)
    #[inline(always)]
    pub fn font(mut self, _size: f32, line: f32) -> Self {
        self.line = line;
        self
    }
    #[inline(always)]
    pub fn color<C2>(self, c: C2) -> Text<K, C2> {
        Text { spec: self.spec, k: self.k, c, line: self.line }
    }
}
/// The stand-in measure: the reference raster's glyphs are 12 device pixels apart and 15 tall, after 12 of
/// inset. The text lane's `Measure` takes this type's place.
#[derive(Clone, Copy)]
struct Fixed12 {
    chars: usize,
    line: f32,
}
impl bough_lay::Measure for Fixed12 {
    const WRAPS: bool = false;
    fn inline(&self) -> (f32, f32) {
        let w = 24.0 + 12.0 * self.chars as f32;
        (w, w)
    }
    fn block(&self, w: f32) -> bough_lay::el::Block {
        bough_lay::el::Block { w: self.inline().0.min(w.max(0.0)).max(self.inline().0), h: self.line, lo: 0.0, hi: f32::INFINITY }
    }
}
impl<K: Content, C> Text<K, C> {
    #[inline(always)]
    fn leaf(&self, s: f32) -> bough_lay::Leaf<Fixed12, Rec1<()>> {
        bough_lay::text(Fixed12 { chars: self.k.chars(), line: if self.line > 0.0 { self.line * s } else { 15.0 } }, ()).with_spec(self.spec)
    }
}
impl<K: Content, C> Lay for Text<K, C> {
    const LEAVES: usize = 1;
    const WRAPS: bool = false;
    #[inline(always)]
    fn spec(&self) -> &Spec {
        &self.spec
    }
    #[inline(always)]
    fn size(&self, cx: &mut LCx, known: [Option<LV>; 2], parent: [Option<LV>; 2], avail: [Av; 2], content: bool) -> [LV; 2] {
        self.leaf(cx.s).size(cx, known, parent, avail, content)
    }
    #[inline(always)]
    fn place<O: Out + ?Sized>(&self, cx: &mut LCx, size: [LV; 2], org: [LV; 2], out: &mut O, k: usize) -> usize {
        self.leaf(cx.s).place(cx, size, org, out, k)
    }
    fn tree(&self) -> bough_lay::tree::Node {
        self.leaf(1.0).tree()
    }
}
impl<V, K: Content, C: Expr<Out = Color> + Rooted<V>> Row<V> for Text<K, C> {
    const LEAVES: u32 = 1;
    #[inline(always)]
    fn visit<L: Leaf<V>>(&self, l: &mut L) {
        <K::A<C> as Row<V>>::visit(&self.k.arch(self.c), l)
    }
    #[inline(always)]
    unsafe fn write(&self, sites: *const Site, slot: u32) {
        // SAFETY: the caller's contract, for this one record.
        unsafe { <K::A<C> as Row<V>>::write(&self.k.arch(self.c), sites, slot) }
    }
    #[inline(always)]
    unsafe fn patch(&self, sites: *const Site, slot: u32, mk: &Mk, cx: &mut Cx<'_>) {
        // SAFETY: as `write`.
        unsafe { <K::A<C> as Row<V>>::patch(&self.k.arch(self.c), sites, slot, mk, cx) }
    }
}

// ---- boxes as rows -------------------------------------------------------------------------------------------

impl<V> Row<V> for () {
    const LEAVES: u32 = 0;
    #[inline(always)]
    fn visit<L: Leaf<V>>(&self, _: &mut L) {}
    #[inline(always)]
    unsafe fn write(&self, _: *const Site, _: u32) {}
    #[inline(always)]
    unsafe fn patch(&self, _: *const Site, _: u32, _: &Mk, _: &mut Cx<'_>) {}
}
impl<V, C: Row<V>, E: Row<V>> Row<V> for Chain<C, E> {
    const LEAVES: u32 = C::LEAVES + E::LEAVES;
    #[inline(always)]
    fn visit<L: Leaf<V>>(&self, l: &mut L) {
        self.0.visit(l);
        self.1.visit(l);
    }
    #[inline(always)]
    unsafe fn write(&self, sites: *const Site, slot: u32) {
        // SAFETY: the first part owns the first `C::LEAVES` sites of the caller's range, the second the rest.
        unsafe {
            self.0.write(sites, slot);
            self.1.write(sites.add(C::LEAVES as usize), slot);
        }
    }
    #[inline(always)]
    unsafe fn patch(&self, sites: *const Site, slot: u32, mk: &Mk, cx: &mut Cx<'_>) {
        // SAFETY: as `write`.
        unsafe {
            self.0.patch(sites, slot, mk, cx);
            self.1.patch(sites.add(C::LEAVES as usize), slot, mk, cx);
        }
    }
}
impl<V, C: Row<V>, H> Row<V> for LBox<(), C, H> {
    const LEAVES: u32 = C::LEAVES;
    #[inline(always)]
    fn visit<L: Leaf<V>>(&self, l: &mut L) {
        self.kids.visit(l)
    }
    #[inline(always)]
    unsafe fn write(&self, sites: *const Site, slot: u32) {
        // SAFETY: the caller's contract; the box has no record of its own.
        unsafe { self.kids.write(sites, slot) }
    }
    #[inline(always)]
    unsafe fn patch(&self, sites: *const Site, slot: u32, mk: &Mk, cx: &mut Cx<'_>) {
        // SAFETY: as `write`.
        unsafe { self.kids.patch(sites, slot, mk, cx) }
    }
}
impl<V, P: Paints<V>, C: Row<V>, H> Row<V> for LBox<Rec1<P>, C, H> {
    const LEAVES: u32 = 1 + C::LEAVES;
    #[inline(always)]
    fn visit<L: Leaf<V>>(&self, l: &mut L) {
        <P::A as Row<V>>::visit(&self.paint.0.arch(), l);
        self.kids.visit(l)
    }
    #[inline(always)]
    unsafe fn write(&self, sites: *const Site, slot: u32) {
        // SAFETY: the box's record is the first site of the caller's range, its children's the rest.
        unsafe {
            <P::A as Row<V>>::write(&self.paint.0.arch(), sites, slot);
            self.kids.write(sites.add(1), slot);
        }
    }
    #[inline(always)]
    unsafe fn patch(&self, sites: *const Site, slot: u32, mk: &Mk, cx: &mut Cx<'_>) {
        // SAFETY: as `write`.
        unsafe {
            <P::A as Row<V>>::patch(&self.paint.0.arch(), sites, slot, mk, cx);
            self.kids.patch(sites.add(1), slot, mk, cx);
        }
    }
}

// ---- handlers ------------------------------------------------------------------------------------------------

/// A painted box with one more handler on its record.
#[derive(Clone, Copy)]
pub struct Hd<X, H>(pub X, pub H);

/// A box whose own record can carry handlers.
pub trait Boxy<V> {
    type Tgt: Target<V>;
    type Kids: Row<V>;
    fn tgt(&self) -> Self::Tgt;
    fn kids(&self) -> &Self::Kids;
}
impl<V, P: Paints<V>, C: Row<V>, H> Boxy<V> for LBox<Rec1<P>, C, H> {
    type Tgt = P::A;
    type Kids = C;
    #[inline(always)]
    fn tgt(&self) -> P::A {
        self.paint.0.arch()
    }
    #[inline(always)]
    fn kids(&self) -> &C {
        &self.kids
    }
}
impl<V, X: Boxy<V>, H: Handler<V, <X::Tgt as Target<V>>::Rec>> Boxy<V> for Hd<X, H> {
    type Tgt = On<X::Tgt, H>;
    type Kids = X::Kids;
    #[inline(always)]
    fn tgt(&self) -> On<X::Tgt, H> {
        On(self.0.tgt(), self.1)
    }
    #[inline(always)]
    fn kids(&self) -> &X::Kids {
        self.0.kids()
    }
}
impl<V: 'static, X: Boxy<V>, H: Handler<V, <X::Tgt as Target<V>>::Rec>> Row<V> for Hd<X, H> {
    const LEAVES: u32 = 1 + <X::Kids as Row<V>>::LEAVES;
    #[inline(always)]
    fn visit<L: Leaf<V>>(&self, l: &mut L) {
        <On<X::Tgt, H> as Row<V>>::visit(&self.tgt(), l);
        self.kids().visit(l)
    }
    #[inline(always)]
    unsafe fn write(&self, sites: *const Site, slot: u32) {
        // SAFETY: the box's record (with its handlers' captures) is the first site, its children's the rest.
        unsafe {
            <On<X::Tgt, H> as Row<V>>::write(&self.tgt(), sites, slot);
            self.kids().write(sites.add(1), slot);
        }
    }
    #[inline(always)]
    unsafe fn patch(&self, sites: *const Site, slot: u32, mk: &Mk, cx: &mut Cx<'_>) {
        // SAFETY: as `write`.
        unsafe {
            <On<X::Tgt, H> as Row<V>>::patch(&self.tgt(), sites, slot, mk, cx);
            self.kids().patch(sites.add(1), slot, mk, cx);
        }
    }
}
impl<X: Lay, H> Lay for Hd<X, H> {
    const LEAVES: usize = X::LEAVES;
    const WRAPS: bool = X::WRAPS;
    const HK: bough_lay::Hk = X::HK;
    #[inline(always)]
    fn spec(&self) -> &Spec {
        self.0.spec()
    }
    #[inline(always)]
    fn size(&self, cx: &mut LCx, known: [Option<LV>; 2], parent: [Option<LV>; 2], avail: [Av; 2], content: bool) -> [LV; 2] {
        self.0.size(cx, known, parent, avail, content)
    }
    #[inline(always)]
    fn reported(&self, cx: &mut LCx, size: [LV; 2]) -> [LV; 2] {
        self.0.reported(cx, size)
    }
    #[inline(always)]
    fn place<O: Out + ?Sized>(&self, cx: &mut LCx, size: [LV; 2], org: [LV; 2], out: &mut O, k: usize) -> usize {
        self.0.place(cx, size, org, out, k)
    }
    fn tree(&self) -> bough_lay::tree::Node {
        self.0.tree()
    }
}

/// The façade's interaction calls, on a painted box.
pub trait Interactive: Sized {
    /// Activation (a primary press and release on the box).
    fn on_click<F>(self, f: F) -> Hd<Self, Click<F>> {
        Hd(self, Click(f))
    }
    /// The press itself.
    fn on_press<F>(self, f: F) -> Hd<Self, Press<F>> {
        Hd(self, Press(f))
    }
    /// The pointer enters (`ev.on`) or leaves.
    fn on_hover<F>(self, f: F) -> Hd<Self, Hover<F>> {
        Hd(self, Hover(f))
    }
    /// Keys while focused; a tab stop.
    fn on_key<F>(self, f: F) -> Hd<Self, OnKey<F>> {
        Hd(self, OnKey(f))
    }
    /// Pointer travel while pressed.
    fn on_drag<F>(self, f: F) -> Hd<Self, Drag<F>> {
        Hd(self, Drag(f))
    }
    /// Text typed while focused; a tab stop.
    fn on_text<F>(self, f: F) -> Hd<Self, OnText<F>> {
        Hd(self, OnText(f))
    }
    /// The wheel over the box; return `true` to consume it.
    fn on_wheel<F>(self, f: F) -> Hd<Self, Wheel<F>> {
        Hd(self, Wheel(f))
    }
    /// Focus gained or lost.
    fn on_focus<F>(self, f: F) -> Hd<Self, OnFocus<F>> {
        Hd(self, OnFocus(f))
    }
}
impl<P, C, H> Interactive for LBox<Rec1<P>, C, H> {}
impl<X, H> Interactive for Hd<X, H> {}

// ---- scrolled boxes and lists ----------------------------------------------------------------------------------

/// `each(iter)`: one child per item. Under a scrolled box it is a list by exposure: the iterator is indexed
/// (`TrustedRandomAccess`: ranges, slices, `map`, `enumerate`, `zip`, `copied`), so a row that is not exposed
/// is never asked for.
pub struct Each<I>(pub I);
#[inline(always)]
pub fn each<I: IntoIterator>(i: I) -> Each<I::IntoIter> {
    Each(i.into_iter())
}

/// A box whose content scrolls: its own record (if painted), then a hole where the content's space is.
pub struct Scroll<P, C> {
    pub spec: Spec,
    pub paint: P,
    pub content: C,
    /// The block word of the scroll offset (an `f32` of points).
    pub word: u8,
}
/// `.scroll(sig!(self.y))` on a box: its children scroll by the field's value (points).
pub trait Scrolls<P, C>: Sized {
    fn scroll<V: 'static, const OFF: usize>(self, y: Sig<'_, V, Signal<f32>, OFF>) -> Scroll<P, C>;
}
impl<P, C, H> Scrolls<P, C> for LBox<P, C, H> {
    #[inline(always)]
    fn scroll<V: 'static, const OFF: usize>(self, y: Sig<'_, V, Signal<f32>, OFF>) -> Scroll<P, C> {
        Scroll { spec: self.spec, paint: self.paint, content: self.kids, word: y.word() }
    }
}
impl<P: bough_lay::Paint, C> Lay for Scroll<P, C> {
    const LEAVES: usize = P::N + 1;
    const WRAPS: bool = false;
    #[inline(always)]
    fn spec(&self) -> &Spec {
        &self.spec
    }
    #[inline(always)]
    fn size(&self, cx: &mut LCx, known: [Option<LV>; 2], parent: [Option<LV>; 2], avail: [Av; 2], content: bool) -> [LV; 2] {
        // the box's own size: its content does not size it (it scrolls)
        let mut b = column(());
        b.spec = self.spec;
        b.size(cx, known, parent, avail, content)
    }
    #[inline(always)]
    fn place<O: Out + ?Sized>(&self, cx: &mut LCx, size: [LV; 2], org: [LV; 2], out: &mut O, k: usize) -> usize {
        let r = rect(cx, size, org);
        if P::N == 1 {
            out.put(k, r);
        }
        out.put(k + P::N, r);
        k + P::N + 1
    }
    fn tree(&self) -> bough_lay::tree::Node {
        let mut b = column(());
        b.spec = self.spec;
        let mut n = b.tree();
        n.leaves = P::N + 1;
        n.total = P::N + 1;
        n
    }
}
impl<V, C> Row<V> for Scroll<(), C> {
    const LEAVES: u32 = 1;
    #[inline(always)]
    fn visit<L: Leaf<V>>(&self, l: &mut L) {
        l.hole()
    }
    #[inline(always)]
    unsafe fn write(&self, _: *const Site, _: u32) {}
    #[inline(always)]
    unsafe fn patch(&self, _: *const Site, _: u32, _: &Mk, _: &mut Cx<'_>) {}
}
impl<V, P: Paints<V>, C> Row<V> for Scroll<Rec1<P>, C> {
    const LEAVES: u32 = 2;
    #[inline(always)]
    fn visit<L: Leaf<V>>(&self, l: &mut L) {
        <P::A as Row<V>>::visit(&self.paint.0.arch(), l);
        l.hole()
    }
    #[inline(always)]
    unsafe fn write(&self, sites: *const Site, slot: u32) {
        // SAFETY: the box's record is the first site of the caller's range.
        unsafe { <P::A as Row<V>>::write(&self.paint.0.arch(), sites, slot) }
    }
    #[inline(always)]
    unsafe fn patch(&self, sites: *const Site, slot: u32, mk: &Mk, cx: &mut Cx<'_>) {
        // SAFETY: as `write`.
        unsafe { <P::A as Row<V>>::patch(&self.paint.0.arch(), sites, slot, mk, cx) }
    }
}

/// Where the content of a scrolled box is being put: its width and how far down the next part starts, in
/// device pixels of the box's space. `None` when the sink keeps the layout it was told.
pub struct Flow {
    pub w: i32,
    pub y: i32,
    pub lay: bool,
}

/// One part of a scrolled box's content: a list, or an element.
pub trait Seg<V> {
    fn seg<S: Sink<V>>(&mut self, s: &mut S, f: &mut Flow);
}
impl<V, I> Seg<V> for Each<I>
where
    I: Iterator + TrustedRandomAccessNoCoerce,
    I::Item: Row<V> + Lay,
{
    #[inline(always)]
    fn seg<S: Sink<V>>(&mut self, s: &mut S, f: &mut Flow) {
        let n = self.0.size();
        let it = &mut self.0;
        let geo = if f.lay { Some(Geo { x: 0, y: f.y, w: f.w }) } else { None };
        // SAFETY: `r < size()` (the list pass asks only for rows under `n`), each row is asked at most once
        // per pass (a pass visits distinct rows) and the iterator is not advanced otherwise: the contract of
        // `__iterator_get_unchecked`. A row's value is a plain element, so being asked again by a later pass
        // on a fresh iterator is an ordinary call of the author's closure.
        f.y += s.rows(n as u32, geo, |r| {
            assert!(r < n);
            unsafe { it.__iterator_get_unchecked(r) }
        });
    }
}
/// An element in a scrolled box's content: laid out as a row of the content's width, where the flow is.
#[inline(always)]
fn seg_el<V, E: Part<V> + Lay, S: Sink<V>>(e: &mut E, s: &mut S, f: &mut Flow) {
    if !f.lay {
        return e.holes(None, &mut 0, s);
    }
    let env = s.env();
    let mut out = vec![[0i32; 4]; <E as Lay>::LEAVES];
    let (h, _) = bough_lay::place_row(&*e, env.scale, env.u, f.w as f32, &mut out[..]);
    let rects: Vec<Rect> = out.iter().map(|r| Rect { x0: r[0], y0: r[1] + f.y, x1: r[2], y1: r[3] + f.y }).collect();
    s.row(|| (&rects[..], ByRef(&*e)));
    e.holes(Some(&rects), &mut 0, s);
    if e.spec().abs.is_none() {
        f.y += h;
    }
}
macro_rules! seg_node {
    ([$($g:tt)*] $t:ty) => {
        impl<V, $($g)*> Seg<V> for $t
        where
            $t: Part<V> + Lay,
        {
            #[inline(always)]
            fn seg<S: Sink<V>>(&mut self, s: &mut S, f: &mut Flow) {
                seg_el(self, s, f)
            }
        }
    };
}
/// A box in a scrolled box's content: an element like any other, or (a box that only wraps `each(..)`, the
/// façade's `column(each(rows))`) the list itself.
pub trait SegKids<V, P, H>: Sized {
    fn seg_box<S: Sink<V>>(b: &mut LBox<P, Self, H>, s: &mut S, f: &mut Flow);
}
/// Children that are not (and do not hold) a bare `each(..)`: what tells a box of elements from a box that
/// is a list. An auto trait with one negative impl, so the two impls below are disjoint to the compiler.
pub auto trait NotEach {}
impl<I> !NotEach for Each<I> {}
// a scrolled box is an element whatever its content holds
impl<P, C> NotEach for Scroll<P, C> {}
impl<V, P, H, C: NotEach> SegKids<V, P, H> for C
where
    LBox<P, C, H>: Part<V> + Lay,
{
    #[inline(always)]
    fn seg_box<S: Sink<V>>(b: &mut LBox<P, C, H>, s: &mut S, f: &mut Flow) {
        seg_el(b, s, f)
    }
}
impl<V, H, I> SegKids<V, (), H> for Each<I>
where
    Each<I>: Seg<V>,
{
    #[inline(always)]
    fn seg_box<S: Sink<V>>(b: &mut LBox<(), Each<I>, H>, s: &mut S, f: &mut Flow) {
        b.kids.seg(s, f)
    }
}
impl<V, P, C: SegKids<V, P, H>, H> Seg<V> for LBox<P, C, H> {
    #[inline(always)]
    fn seg<S: Sink<V>>(&mut self, s: &mut S, f: &mut Flow) {
        C::seg_box(self, s, f)
    }
}
seg_node!([X, H] Hd<X, H>);
seg_node!([K, C] Text<K, C>);
seg_node!([P, C] Scroll<P, C>);
macro_rules! seg_tuple {
    ($($t:ident)+) => {
        impl<V, $($t: Seg<V>),+> Seg<V> for ($($t,)+) {
            #[inline(always)]
            #[allow(non_snake_case)]
            fn seg<S: Sink<V>>(&mut self, s: &mut S, f: &mut Flow) {
                let ($($t,)+) = self;
                $($t.seg(s, f);)+
            }
        }
    };
}
seg_tuple!(A);
seg_tuple!(A B);
seg_tuple!(A B C);
seg_tuple!(A B C D);
impl<V, C: Seg<V>, E: Seg<V>> Seg<V> for Chain<C, E> {
    #[inline(always)]
    fn seg<S: Sink<V>>(&mut self, s: &mut S, f: &mut Flow) {
        self.0.seg(s, f);
        self.1.seg(s, f);
    }
}
impl<V> Seg<V> for () {
    #[inline(always)]
    fn seg<S: Sink<V>>(&mut self, _: &mut S, _: &mut Flow) {}
}

// ---- driving an element ----------------------------------------------------------------------------------------

/// A row by reference: what an element hands a sink while keeping itself for its lists.
pub struct ByRef<'a, E>(pub &'a E);
impl<V, E: Row<V>> Row<V> for ByRef<'_, E> {
    const LEAVES: u32 = E::LEAVES;
    #[inline(always)]
    fn visit<L: Leaf<V>>(&self, l: &mut L) {
        self.0.visit(l)
    }
    #[inline(always)]
    unsafe fn write(&self, sites: *const Site, slot: u32) {
        // SAFETY: the caller's contract, for the row behind the reference.
        unsafe { self.0.write(sites, slot) }
    }
    #[inline(always)]
    unsafe fn patch(&self, sites: *const Site, slot: u32, mk: &Mk, cx: &mut Cx<'_>) {
        // SAFETY: as `write`.
        unsafe { self.0.patch(sites, slot, mk, cx) }
    }
}

/// A part of an element: its records are a row; it may hold scrolled boxes (holes), which are driven after
/// the row, each with the rect the layout gave it.
pub trait Part<V>: Row<V> {
    const HOLES: bool;
    /// Drive the scrolled boxes under this part. `rects` is the row's layout when the sink asked for one;
    /// `k` is this part's first position in it.
    fn holes<S: Sink<V>>(&mut self, rects: Option<&[Rect]>, k: &mut usize, s: &mut S);
}
macro_rules! no_holes {
    () => {
        const HOLES: bool = false;
        #[inline(always)]
        fn holes<S: Sink<V>>(&mut self, _: Option<&[Rect]>, k: &mut usize, _: &mut S) {
            *k += <Self as Row<V>>::LEAVES as usize;
        }
    };
}
impl<V> Part<V> for () {
    no_holes!();
}
impl<V, K: Content, C: Expr<Out = Color> + Rooted<V>> Part<V> for Text<K, C> {
    no_holes!();
}
impl<V, C: Part<V>, H> Part<V> for LBox<(), C, H> {
    const HOLES: bool = C::HOLES;
    #[inline(always)]
    fn holes<S: Sink<V>>(&mut self, rects: Option<&[Rect]>, k: &mut usize, s: &mut S) {
        self.kids.holes(rects, k, s)
    }
}
impl<V, P: Paints<V>, C: Part<V>, H> Part<V> for LBox<Rec1<P>, C, H> {
    const HOLES: bool = C::HOLES;
    #[inline(always)]
    fn holes<S: Sink<V>>(&mut self, rects: Option<&[Rect]>, k: &mut usize, s: &mut S) {
        *k += 1;
        self.kids.holes(rects, k, s)
    }
}
impl<V: 'static, X: Boxy<V> + Part<V>, H: Handler<V, <X::Tgt as Target<V>>::Rec>> Part<V> for Hd<X, H> {
    const HOLES: bool = X::HOLES;
    #[inline(always)]
    fn holes<S: Sink<V>>(&mut self, rects: Option<&[Rect]>, k: &mut usize, s: &mut S) {
        self.0.holes(rects, k, s)
    }
}
impl<V, C: Part<V>, E: Part<V>> Part<V> for Chain<C, E> {
    const HOLES: bool = C::HOLES | E::HOLES;
    #[inline(always)]
    fn holes<S: Sink<V>>(&mut self, rects: Option<&[Rect]>, k: &mut usize, s: &mut S) {
        self.0.holes(rects, k, s);
        self.1.holes(rects, k, s);
    }
}
impl<V, X: Part<V>, const N: usize> Part<V> for [X; N] {
    const HOLES: bool = X::HOLES;
    #[inline(always)]
    fn holes<S: Sink<V>>(&mut self, rects: Option<&[Rect]>, k: &mut usize, s: &mut S) {
        if !X::HOLES {
            *k += <Self as Row<V>>::LEAVES as usize;
            return;
        }
        for x in self {
            x.holes(rects, k, s);
        }
    }
}
macro_rules! part_tuple {
    ($($t:ident)+) => {
        impl<V, $($t: Part<V>),+> Part<V> for ($($t,)+) {
            const HOLES: bool = false $(| $t::HOLES)+;
            #[inline(always)]
            #[allow(non_snake_case)]
            fn holes<S: Sink<V>>(&mut self, rects: Option<&[Rect]>, k: &mut usize, s: &mut S) {
                let ($($t,)+) = self;
                $($t.holes(rects, k, s);)+
            }
        }
    };
}
part_tuple!(A);
part_tuple!(A B);
part_tuple!(A B C);
part_tuple!(A B C D);
part_tuple!(A B C D E);
part_tuple!(A B C D E F);
part_tuple!(A B C D E F G);
part_tuple!(A B C D E F G H);
part_tuple!(A B C D E F G H I);
part_tuple!(A B C D E F G H I J);
part_tuple!(A B C D E F G H I J K);
part_tuple!(A B C D E F G H I J K L);

/// The scrolled box itself: the hole is where its content's space is.
macro_rules! scroll_part {
    ($p:ty, $own:expr, $($bound:tt)*) => {
        impl<V, C: Seg<V>, $($bound)*> Part<V> for Scroll<$p, C> {
            const HOLES: bool = true;
            #[inline(always)]
            fn holes<S: Sink<V>>(&mut self, rects: Option<&[Rect]>, k: &mut usize, s: &mut S) {
                let at = *k + $own;
                *k = at + 1;
                let (space, w) = match rects {
                    Some(r) => (Space { clip: r[at], base: (r[at].x0, r[at].y0), scroll: self.word | POINTS }, r[at].x1 - r[at].x0),
                    // a sink that keeps what it was told ignores the space
                    None => (Space::root(0, 0), 0),
                };
                let content = &mut self.content;
                s.scope(space, |s| content.seg(s, &mut Flow { w, y: 0, lay: rects.is_some() }));
            }
        }
    };
}
scroll_part!((), 0,);
scroll_part!(Rec1<P>, 1, P: Paints<V>);

/// What `view` returns.
pub trait Element<V> {
    fn drive<S: Sink<V>>(self, s: &mut S);
}
impl<V, E: Part<V> + Lay> Element<V> for E {
    #[inline]
    fn drive<S: Sink<V>>(mut self, s: &mut S) {
        const { assert!(<E as Row<V>>::LEAVES as usize == <E as Lay>::LEAVES, "an element's layout has one rect per record") };
        if !S::FIXED {
            // a turn: the fixed records are expressions over the block and are not described; only lists are
            return self.holes(None, &mut 0, s);
        }
        let env = s.env();
        let mut out = vec![[0i32; 4]; <E as Lay>::LEAVES];
        bough_lay::place_root(&self, env.scale, env.u, env.size, &mut out[..]);
        let rects: Vec<Rect> = out.iter().map(|r| Rect { x0: r[0], y0: r[1], x1: r[2], y1: r[3] }).collect();
        s.row(|| (&rects[..], ByRef(&self)));
        self.holes(Some(&rects), &mut 0, s);
    }
}

/// A view, as the façade has it.
pub trait View: Sized + 'static {
    fn view(&self) -> impl Element<Self> + '_;
}
static NO_BLOCK: U = U { w: [0; 64] };
impl<V: View> crate::sink::View for V {
    fn block(&self) -> &U {
        &NO_BLOCK
    }
    /// The block is the view's own fields: each word the columns' lenses named is read from its field.
    #[inline]
    fn gathered<'a>(&'a self, slots: &[Slot], out: &'a mut U) -> &'a U {
        let base = self as *const V as *const u8;
        for s in slots {
            debug_assert!(s.off as usize + s.bytes as usize <= size_of::<V>());
            let mut w = [0u8; 4];
            // SAFETY: the slot was named by a lens `At<V, X, OFF>` of a record of this view (`Rooted<V>`), so
            // `off..off + bytes` is a field of `V` holding an initialised `X`.
            unsafe { core::ptr::copy_nonoverlapping(base.add(s.off as usize), w.as_mut_ptr(), s.bytes as usize) };
            out.w[s.slot as usize] = u32::from_le_bytes(w);
        }
        out
    }
    #[inline]
    fn view<S: Sink<Self>>(&self, s: &mut S) {
        View::view(self).drive(s)
    }
}

// ---- the application --------------------------------------------------------------------------------------------

/// A view and its world: what the façade's `App` is to a scene. Handlers run in `turn` (or are dispatched by
/// the pointer and key calls); what they changed is found afterwards, by comparing.
pub struct App<V: View, B: Backend> {
    pub v: V,
    pub world: crate::world::World<B>,
    /// Mount-once: the view is described once and state reaches it through `sig!` operands and `Signal`s. If
    /// false, the exposed view is described again after every turn (the façade's default).
    pub once: bool,
    sets: u32,
}
impl<V: View, B: Backend> App<V, B> {
    /// A window of `w` x `h` device pixels at `scale` device pixels per point.
    pub fn new(v: V, w: i32, h: i32, scale: f32) -> App<V, B> {
        let mut world = crate::world::World::<B>::new(w, h, APP_WORDS);
        world.set_scale(scale);
        // Transitions keep the time they started at, and 0 means at rest: time starts after 0.
        world.tick(1);
        world.mount(&v);
        App { v, world, once: false, sets: SETS.load(Ordering::Relaxed) }
    }
    pub fn once(mut self) -> Self {
        self.once = true;
        self
    }
    /// Run `f` on the state, then find what it changed.
    #[inline]
    pub fn turn(&mut self, f: impl FnOnce(&mut V)) {
        f(&mut self.v);
        self.settle();
    }
    /// After handlers: the block's compare, and a re-description of the exposed view when the compare cannot
    /// account for every change.
    #[inline]
    pub fn settle(&mut self) {
        let now = SETS.load(Ordering::Relaxed);
        let sets = now.wrapping_sub(self.sets);
        self.sets = now;
        if self.once && sets == self.world.changed_words(&self.v).count_ones() {
            self.world.settle(&self.v);
        } else {
            self.world.redescribe(&self.v);
        }
    }
    /// A frame at time `now` (milliseconds, after 0).
    #[inline]
    pub fn frame(&mut self, now: u32) {
        self.world.tick(now.max(1));
    }
    pub fn pointer_move(&mut self, x: f32, y: f32) {
        self.world.pointer(&mut self.v, x as i32, y as i32);
        self.settle();
    }
    pub fn key(&mut self, key: crate::event::Key) {
        self.world.key_down(&mut self.v, key, crate::event::Mods::default());
        self.settle();
    }
}

/// What a scene imports.
pub mod prelude {
    pub use super::{Align, App, Curve, Deco, Element, Flex, Interactive, Justify, NumFmt, Scrolls, Signal, Transition, Translate, View, block, column, div, each, rgb, row, text};
    pub use crate::event::{Ev, Key};
    pub use crate::expr::Color;
    pub use crate::sig;
}
