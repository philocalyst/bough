//! What a chain of the façade's box modifiers lowers to: one composed paint record.
//!
//! The author writes `div().bg(c).rounded(8.0).border(1.0, line).shadow(..)` (the façade's `LBox`, `Quad`
//! and `DecoBox`: `crates/bough/src/{layout,elem,deco}.rs`) and never spells a paint type. Underneath, the
//! element holds a [`Bx`]: each modifier is a method that changes one type parameter, in any order, and
//! [`Bx::paint`] folds the parameters into a single `Paint` value, which is one record of one archetype:
//!
//! | chain | paint |
//! |---|---|
//! | `.bg(c)` | `Solid<B, C>` |
//! | `.bg(c).rounded(r)` | `RRect<B, R, C>` |
//! | `.bg(c).border(w, l)` (with or without `.rounded`) | `Border<B, R, W, L, C>` |
//! | `.border(w, l)` | `Border<B, R, W, L, clear>` |
//! | `.gradient(dx, dy, c, d)` | `Grad<B, R, X, Y, C, D>` |
//! | `.gradient(..).border(w, l)` | `Over<Grad<..>, Border<.., clear>>` |
//! | any of these after `.shadow(a).shadow(b)` | `Over<Over<Shadow b, Shadow a>, body>` (the first shadow on top, as in CSS; the body's opaque core hides them: the static cut) |
//! | `.opacity(o)` | every colour above becomes `Fade<colour, O>` |
//! | `.translate(x, y)` | every rect above becomes `Off<rect, X, Y>` |
//!
//! then `.clip(k)` and `.area()` on the result (`Clip<P, K>`; a pick-only paint is the surface lane's, SEAM
//! section 8). Operands are expressions: a constant colour is a record word (`color(c)`), a state word is a
//! lens, a transition is [`tr_color`] / [`tr_i32`] / [`tr_rect`] over the clock. Whether the box covers what
//! is beneath it is not in the chain: `Alpha::full` decides it per record from the colour's own words.
//!
//! No paint yet for: radial and multi-stop gradients (`deco::Grad` ramps), images and SVG icons, rotation
//! and `Xf` transforms, group opacity (the façade has none either), the façade's `Ease::Smooth` (a Hermite
//! retarget from the displayed velocity: needs a velocity word and a leaf), text (glyph-instance columns are
//! the layout lane's seam).
use crate::expr::*;
use crate::paint::*;
use wgsl_rs::std::{Vec4i, Vec4u};

/// Square corners.
#[derive(Clone, Copy)]
pub struct Sq;
/// Rounded by `R` (Q8).
#[derive(Clone, Copy)]
pub struct Rd<R>(pub R);
#[derive(Clone, Copy)]
pub struct NoFill;
#[derive(Clone, Copy)]
pub struct Flat<C>(pub C);
/// A two-stop linear gradient: direction (as `ops::mk_grad` takes it) and the two colours.
#[derive(Clone, Copy)]
pub struct Linear<X, Y, C, D> {
    pub dx: X,
    pub dy: Y,
    pub c: C,
    pub c2: D,
}
#[derive(Clone, Copy)]
pub struct NoLine;
#[derive(Clone, Copy)]
pub struct Line<W, L> {
    pub w: W,
    pub c: L,
}
#[derive(Clone, Copy)]
pub struct NoSh;
/// One more shadow beneath those in `rest`: spread, offset, blur (Q8) and colour.
#[derive(Clone, Copy)]
pub struct Sh<Rest, SP, DX, DY, BL, C> {
    pub rest: Rest,
    pub spread: SP,
    pub dx: DX,
    pub dy: DY,
    pub blur: BL,
    pub c: C,
}
/// No opacity operand.
#[derive(Clone, Copy)]
pub struct Whole;
/// Paint alpha scaled by `T` (Q15).
#[derive(Clone, Copy)]
pub struct Op<T>(pub T);

/// A box being described: rect, corners, fill, border, shadows, opacity.
#[derive(Clone, Copy)]
pub struct Bx<B, R, F, L, S, O> {
    pub b: B,
    pub r: R,
    pub fill: F,
    pub line: L,
    pub sh: S,
    pub op: O,
}
/// A box at its record's rect lanes, with nothing painted yet.
pub fn bx() -> Bx<Lane, Sq, NoFill, NoLine, NoSh, Whole> {
    Bx { b: Lane, r: Sq, fill: NoFill, line: NoLine, sh: NoSh, op: Whole }
}

impl<B, R, F, L, S, O> Bx<B, R, F, L, S, O> {
    pub fn bg<C>(self, c: C) -> Bx<B, R, Flat<C>, L, S, O> {
        Bx { b: self.b, r: self.r, fill: Flat(c), line: self.line, sh: self.sh, op: self.op }
    }
    pub fn gradient<X, Y, C, D>(self, dx: X, dy: Y, c: C, c2: D) -> Bx<B, R, Linear<X, Y, C, D>, L, S, O> {
        Bx { b: self.b, r: self.r, fill: Linear { dx, dy, c, c2 }, line: self.line, sh: self.sh, op: self.op }
    }
    pub fn rounded<R2>(self, r: R2) -> Bx<B, Rd<R2>, F, L, S, O> {
        Bx { b: self.b, r: Rd(r), fill: self.fill, line: self.line, sh: self.sh, op: self.op }
    }
    pub fn border<W, C>(self, w: W, c: C) -> Bx<B, R, F, Line<W, C>, S, O> {
        Bx { b: self.b, r: self.r, fill: self.fill, line: Line { w, c }, sh: self.sh, op: self.op }
    }
    /// One more shadow, beneath the ones already given.
    pub fn shadow<SP, DX, DY, BL, C>(self, spread: SP, dx: DX, dy: DY, blur: BL, c: C) -> Bx<B, R, F, L, Sh<S, SP, DX, DY, BL, C>, O> {
        Bx { b: self.b, r: self.r, fill: self.fill, line: self.line, sh: Sh { rest: self.sh, spread, dx, dy, blur, c }, op: self.op }
    }
    pub fn opacity<T>(self, t: T) -> Bx<B, R, F, L, S, Op<T>> {
        Bx { b: self.b, r: self.r, fill: self.fill, line: self.line, sh: self.sh, op: Op(t) }
    }
    /// The paint moves by (x, y) (Q8); the layout's rect does not.
    pub fn translate<X, Y>(self, x: X, y: Y) -> Bx<Off<B, X, Y>, R, F, L, S, O> {
        Bx { b: Off { b: self.b, dx: x, dy: y }, r: self.r, fill: self.fill, line: self.line, sh: self.sh, op: self.op }
    }
}

/// The corner operand as an expression.
pub trait Corner: Copy {
    type E: Expr<Out = i32>;
    fn e(self) -> Self::E;
}
impl Corner for Sq {
    type E = Ki<0>;
    fn e(self) -> Ki<0> {
        Ki
    }
}
impl<R: Expr<Out = i32>> Corner for Rd<R> {
    type E = R;
    fn e(self) -> R {
        self.0
    }
}

/// A colour under the box's opacity.
pub trait Tint<X>: Copy {
    type Out: Alpha;
    fn tint(self, x: X) -> Self::Out;
}
impl<X: Alpha> Tint<X> for Whole {
    type Out = X;
    fn tint(self, x: X) -> X {
        x
    }
}
impl<X: Expr<Out = Vec4u>, T: Expr<Out = u32>> Tint<X> for Op<T>
where
    Fade<X, T>: Alpha,
{
    type Out = Fade<X, T>;
    fn tint(self, x: X) -> Fade<X, T> {
        Fade { a: x, k: self.0 }
    }
}

/// A transparent colour (no record word).
pub type Clear = Pm<K<0>>;

/// (corners, fill, border) as the box's own layers over rect `B` under opacity `O`.
pub trait Body<B, O> {
    type P: Paint;
    /// The corner expression the shadows share.
    type R: Expr<Out = i32>;
    fn body(self, b: B, o: O) -> (Self::P, Self::R);
}
impl<B: Expr<Out = Vec4i>, O: Tint<C>, C> Body<B, O> for (Sq, Flat<C>, NoLine)
where
    Solid<B, O::Out>: Paint,
{
    type P = Solid<B, O::Out>;
    type R = Ki<0>;
    fn body(self, b: B, o: O) -> (Self::P, Ki<0>) {
        (Solid { b, c: o.tint(self.1.0) }, Ki)
    }
}
impl<B: Expr<Out = Vec4i>, R: Expr<Out = i32>, O: Tint<C>, C> Body<B, O> for (Rd<R>, Flat<C>, NoLine)
where
    RRect<B, R, O::Out>: Paint,
{
    type P = RRect<B, R, O::Out>;
    type R = R;
    fn body(self, b: B, o: O) -> (Self::P, R) {
        (RRect { b, r: self.0.0, c: o.tint(self.1.0) }, self.0.0)
    }
}
impl<B: Expr<Out = Vec4i>, Rk: Corner, W: Expr<Out = i32>, O: Tint<C> + Tint<Lc>, C, Lc> Body<B, O> for (Rk, Flat<C>, Line<W, Lc>)
where
    Border<B, Rk::E, W, <O as Tint<Lc>>::Out, <O as Tint<C>>::Out>: Paint,
{
    type P = Border<B, Rk::E, W, <O as Tint<Lc>>::Out, <O as Tint<C>>::Out>;
    type R = Rk::E;
    fn body(self, b: B, o: O) -> (Self::P, Rk::E) {
        (Border { b, r: self.0.e(), w: self.2.w, c: o.tint(self.2.c), fill: o.tint(self.1.0) }, self.0.e())
    }
}
impl<B: Expr<Out = Vec4i>, Rk: Corner, W: Expr<Out = i32>, O: Tint<Lc>, Lc> Body<B, O> for (Rk, NoFill, Line<W, Lc>)
where
    Border<B, Rk::E, W, O::Out, Clear>: Paint,
{
    type P = Border<B, Rk::E, W, O::Out, Clear>;
    type R = Rk::E;
    fn body(self, b: B, o: O) -> (Self::P, Rk::E) {
        (Border { b, r: self.0.e(), w: self.2.w, c: o.tint(self.2.c), fill: Pm { c: K } }, self.0.e())
    }
}
impl<B: Expr<Out = Vec4i>, Rk: Corner, X: Expr<Out = i32>, Y: Expr<Out = i32>, O: Tint<C> + Tint<D>, C, D> Body<B, O> for (Rk, Linear<X, Y, C, D>, NoLine)
where
    Grad<B, Rk::E, X, Y, <O as Tint<C>>::Out, <O as Tint<D>>::Out>: Paint,
{
    type P = Grad<B, Rk::E, X, Y, <O as Tint<C>>::Out, <O as Tint<D>>::Out>;
    type R = Rk::E;
    fn body(self, b: B, o: O) -> (Self::P, Rk::E) {
        (Grad { b, r: self.0.e(), dx: self.1.dx, dy: self.1.dy, c: o.tint(self.1.c), c2: o.tint(self.1.c2) }, self.0.e())
    }
}
impl<B: Expr<Out = Vec4i>, Rk: Corner, X: Expr<Out = i32>, Y: Expr<Out = i32>, W: Expr<Out = i32>, O: Tint<C> + Tint<D> + Tint<Lc>, C, D, Lc> Body<B, O> for (Rk, Linear<X, Y, C, D>, Line<W, Lc>)
where
    Grad<B, Rk::E, X, Y, <O as Tint<C>>::Out, <O as Tint<D>>::Out>: Paint,
    Border<B, Rk::E, W, <O as Tint<Lc>>::Out, Clear>: Paint,
{
    type P = Over<Grad<B, Rk::E, X, Y, <O as Tint<C>>::Out, <O as Tint<D>>::Out>, Border<B, Rk::E, W, <O as Tint<Lc>>::Out, Clear>>;
    type R = Rk::E;
    fn body(self, b: B, o: O) -> (Self::P, Rk::E) {
        (Over(Grad { b, r: self.0.e(), dx: self.1.dx, dy: self.1.dy, c: o.tint(self.1.c), c2: o.tint(self.1.c2) }, Border { b, r: self.0.e(), w: self.2.w, c: o.tint(self.2.c), fill: Pm { c: K } }), self.0.e())
    }
}

/// One shadow of box `B` with corners `R`.
pub type ShadowOf<B, R, SP, DX, DY, BL, C> = Shadow<Off<Grow<B, SP>, DX, DY>, R, BL, C, Tab>;

/// A non-empty list of shadows as one paint (the last given is the deepest).
pub trait Stack<B, R, O> {
    type P: Paint;
    fn stack(self, b: B, r: R, o: O) -> Self::P;
}
impl<B: Copy, R: Copy, O: Tint<C>, SP, DX, DY, BL, C> Stack<B, R, O> for Sh<NoSh, SP, DX, DY, BL, C>
where
    ShadowOf<B, R, SP, DX, DY, BL, O::Out>: Paint,
{
    type P = ShadowOf<B, R, SP, DX, DY, BL, O::Out>;
    fn stack(self, b: B, r: R, o: O) -> Self::P {
        Shadow { b: Off { b: Grow { b, d: self.spread }, dx: self.dx, dy: self.dy }, r, blur: self.blur, c: o.tint(self.c), tab: Tab::NONE }
    }
}
impl<B: Copy, R: Copy, O: Tint<C>, Rest: Stack<B, R, O>, SP, DX, DY, BL, C> Stack<B, R, O> for Sh<Rest, SP, DX, DY, BL, C>
where
    ShadowOf<B, R, SP, DX, DY, BL, O::Out>: Paint,
{
    type P = Over<ShadowOf<B, R, SP, DX, DY, BL, O::Out>, Rest::P>;
    fn stack(self, b: B, r: R, o: O) -> Self::P {
        Over(Shadow { b: Off { b: Grow { b, d: self.spread }, dx: self.dx, dy: self.dy }, r, blur: self.blur, c: o.tint(self.c), tab: Tab::NONE }, self.rest.stack(b, r, o))
    }
}

/// The shadows (if any) beneath a body.
pub trait Under<B, R, O, Top: Paint> {
    type P: Paint;
    fn under(self, b: B, r: R, o: O, top: Top) -> Self::P;
}
impl<B, R, O, Top: Paint> Under<B, R, O, Top> for NoSh {
    type P = Top;
    fn under(self, _: B, _: R, _: O, top: Top) -> Top {
        top
    }
}
impl<B, R, O, Top: Paint, Rest, SP, DX, DY, BL, C> Under<B, R, O, Top> for Sh<Rest, SP, DX, DY, BL, C>
where
    Self: Stack<B, R, O>,
{
    type P = Over<<Self as Stack<B, R, O>>::P, Top>;
    fn under(self, b: B, r: R, o: O, top: Top) -> Self::P {
        Over(self.stack(b, r, o), top)
    }
}

impl<B: Copy, R, F, L, S, O: Copy> Bx<B, R, F, L, S, O>
where
    (R, F, L): Body<B, O>,
    S: Under<B, <(R, F, L) as Body<B, O>>::R, O, <(R, F, L) as Body<B, O>>::P>,
{
    /// The chain as one paint value: one record.
    pub fn paint(self) -> <S as Under<B, <(R, F, L) as Body<B, O>>::R, O, <(R, F, L) as Body<B, O>>::P>>::P {
        let (body, r) = (self.r, self.fill, self.line).body(self.b, self.op);
        self.sh.under(self.b, r, self.op, body)
    }
}

/// What a finished paint can still be wrapped in.
pub trait PaintExt: Paint + Sized {
    fn clip<K: Expr<Out = Vec4i>>(self, k: K) -> Clip<Self, K> {
        Clip { p: self, k }
    }
    fn hit<I: Expr<Out = u32>>(self, id: I) -> Hit<Self, I> {
        Hit { p: self, id }
    }
}
impl<P: Paint> PaintExt for P {}

// ---- operands ----

/// A colour the author gave as a value: one record word. It covers when its alpha is 255 (`Alpha::full`).
pub fn color(c: u32) -> Pm<Rec> {
    Pm { c: Rec(c) }
}
/// A length in Q8 the author gave as a value: one record word.
pub fn len(q8: i32) -> I<Rec> {
    I { a: Rec(q8 as u32) }
}
pub type Clock = Uw<U_TIME>;
/// The progress of a transition a turn started at `t0` (clock ticks) lasting `dur`: two record words, written
/// by the turn that retargets it; every frame after that is the clock word alone.
pub type Progress = Prog<Clock, Rec, Rec>;
pub fn progress(t0: u32, dur: u32) -> Progress {
    Prog { now: Uw, t0: Rec(t0), dur: Rec(dur) }
}
/// `Ease::OutCubic` of the façade (`Curve::out`).
pub fn ease_out(p: Progress) -> EOut<Progress> {
    EOut { t: p }
}
/// `Ease::InOutCubic` (`Curve::in_out`).
pub fn ease_in_out(p: Progress) -> EInOut<Progress> {
    EInOut { t: p }
}
/// `Ease::InCubic`.
pub fn ease_in(p: Progress) -> EIn<Progress> {
    EIn { t: p }
}
/// A colour in transition (`.bg(sig!(..).transition(curve))`): from, to, and an eased progress.
pub fn tr_color<E: Expr<Out = u32>>(from: u32, to: u32, e: E) -> MixV<Pm<Rec>, Pm<Rec>, E> {
    MixV { a: color(from), b: color(to), t: e }
}
/// A length in transition (a radius, a border width, a translation).
pub fn tr_i32<E: Expr<Out = u32>>(from: i32, to: i32, e: E) -> MixI<I<Rec>, I<Rec>, E> {
    MixI { a: len(from), b: len(to), t: e }
}
/// An opacity in transition (Q15).
pub fn tr_q15<E: Expr<Out = u32>>(from: u32, to: u32, e: E) -> I2U<MixI<I<Rec>, I<Rec>, E>> {
    I2U { a: MixI { a: len(from as i32), b: len(to as i32), t: e } }
}
