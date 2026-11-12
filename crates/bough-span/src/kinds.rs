//! Kinds that draw themselves (`bough_x::draw`): text held in the record, text held in a [`Line`], a bar inside
//! a box (a caret, a selection), a turning rounded box.
use crate::line::LineRef;
use bough_expr::{Code, Color, Cx, Expr, StageId, U, U_TIME, Words, later};
use bough_x::draw::{Align, Draws, Prims};
use bough_x::store::{Arch, Backend, ColRef, Rect};

const fn align(a: u8) -> Align {
    match a {
        0 => Align::Left,
        1 => Align::Center,
        _ => Align::Right,
    }
}
pub const LEFT: u8 = 0;
pub const CENTER: u8 = 1;
pub const RIGHT: u8 = 2;

/// Up to `4 N` bytes of UTF-8 held in the record (shorter text ends at the first zero byte).
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Str<const N: usize>(pub [u32; N]);
// SAFETY: `N` words.
unsafe impl<const N: usize> Words for Str<N> {}
impl<const N: usize> Str<N> {
    /// The text, cut at a character boundary if it is longer than the record holds.
    #[inline]
    pub fn new(s: &str) -> Str<N> {
        let mut n = s.len().min(4 * N);
        while !s.is_char_boundary(n) {
            n -= 1;
        }
        let mut w = [0u32; N];
        for (i, &b) in s.as_bytes()[..n].iter().enumerate() {
            w[i / 4] |= (b as u32) << (8 * (i % 4));
        }
        Str(w)
    }
    /// Whether `s` fits.
    pub const fn fits(s: &str) -> bool {
        s.len() <= 4 * N
    }
    #[inline(always)]
    pub fn bytes(&self) -> &[u8] {
        // SAFETY: `[u32; N]` is `4 N` initialised bytes; on a little-endian target byte `i` is byte `i % 4` of
        // word `i / 4`, which is how `new` packed them.
        let b = unsafe { core::slice::from_raw_parts(self.0.as_ptr() as *const u8, 4 * N) };
        let n = b.iter().position(|&c| c == 0).unwrap_or(4 * N);
        &b[..n]
    }
}
const _: () = assert!(cfg!(target_endian = "little"), "Str packs bytes little-endian");

/// Text an expression can evaluate to.
pub trait Bytes: Copy + PartialEq {
    fn bytes(&self) -> &[u8];
}
impl<const N: usize> Bytes for Str<N> {
    #[inline(always)]
    fn bytes(&self) -> &[u8] {
        Str::bytes(self)
    }
}
impl<const N: usize> Str<N> {
    /// Decimal digits of `v` (the low digits if it has more than the record holds).
    #[inline]
    pub fn num(v: u32) -> Str<N> {
        let mut d = [0u8; 10];
        let (mut n, mut v) = (0, v);
        loop {
            d[n] = b'0' + (v % 10) as u8;
            n += 1;
            v /= 10;
            if v == 0 {
                break;
            }
        }
        let n = n.min(4 * N);
        let mut w = [0u32; N];
        for i in 0..n {
            w[i / 4] |= (d[n - 1 - i] as u32) << (8 * (i % 4));
        }
        Str(w)
    }
    /// Append bytes (what does not fit is dropped).
    #[inline]
    pub fn push(&mut self, s: &[u8]) {
        let at = self.bytes().len();
        for (i, &b) in s.iter().enumerate().take(4 * N - at) {
            self.0[(at + i) / 4] |= (b as u32) << (8 * ((at + i) % 4));
        }
    }
}

/// A line of text, aligned `AL` in its box. The text is the value of an expression: a constant of the record
/// (`rec(Str::new(..))`), or a function of state evaluated when that state changes
/// (`(un::<Len>(), un::<Tick>()).map(status)`), in which case the derivative is the usual one: the record is
/// damaged when the text differs.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Text<const AL: u8, E, C>(pub E, pub C);
// SAFETY: `repr(C)` over `Words` operands.
unsafe impl<const AL: u8, E: Words, C: Words> Words for Text<AL, E, C> {}
impl<const AL: u8, E: Expr<Out: Bytes>, C: Expr<Out = Color>> Arch for Text<AL, E, C> {
    const READS: u64 = E::READS | C::READS;
    const NAME: &'static str = "text";
    const STAGE: StageId = later(&[E::STAGE, C::STAGE]);
    const TIMED: bool = (E::READS | C::READS) >> U_TIME & 1 != 0;
    #[inline(always)]
    fn until(&self, u: &U) -> u32 {
        self.0.until(&Cx::of(u)).max(self.1.until(&Cx::of(u)))
    }
    #[inline(always)]
    fn changed(&self, old: &U, new: &U) -> bool {
        (E::READS != 0 && self.0.eval(&Cx::of(old)) != self.0.eval(&Cx::of(new))) | (self.1.eval(&Cx::of(old)) != self.1.eval(&Cx::of(new)))
    }
    #[inline(always)]
    fn paint<B: Backend>(b: &mut B, col: ColRef<'_, Self>, u: &U) {
        b.draws(col, u)
    }
}
impl<const AL: u8, E: Expr<Out: Bytes>, C: Expr<Out = Color>> Draws for Text<AL, E, C> {
    #[inline(always)]
    fn draw<P: Prims>(&self, r: Rect, cut: Rect, u: &U, p: &mut P) {
        p.text(r, cut, self.0.eval(&Cx::of(u)).bytes(), align(AL), self.1.eval(&Cx::of(u)).0)
    }
}

/// A [`Line`](crate::line::Line) of application state, aligned `AL` in its box.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct TextOf<const AL: u8, C>(pub LineRef, pub C);
// SAFETY: `repr(C)` over `Words` operands.
unsafe impl<const AL: u8, C: Words> Words for TextOf<AL, C> {}
impl<const AL: u8, C: Expr<Out = Color>> Arch for TextOf<AL, C> {
    const READS: u64 = C::READS;
    const NAME: &'static str = "text-of";
    const STAGE: StageId = later(&[StageId::Turn, C::STAGE]);
    #[inline(always)]
    fn changed(&self, old: &U, new: &U) -> bool {
        self.1.eval(&Cx::of(old)) != self.1.eval(&Cx::of(new))
    }
    #[inline(always)]
    fn paint<B: Backend>(b: &mut B, col: ColRef<'_, Self>, u: &U) {
        b.draws(col, u)
    }
}
impl<const AL: u8, C: Expr<Out = Color>> Draws for TextOf<AL, C> {
    #[inline(always)]
    fn draw<P: Prims>(&self, r: Rect, cut: Rect, u: &U, p: &mut P) {
        let c = self.1.eval(&Cx::of(u)).0;
        self.0.with(|s| {
            if let Some(s) = s {
                p.text(r, cut, s, align(AL), c)
            }
        })
    }
}

/// A bar inside the record's box: `w` pixels wide from `x` pixels right of the box's left edge, the box's
/// height. A caret or a selection: where it is belongs to the record, so moving it is an operand write whose
/// damage is the box.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Bar<C>(pub i32, pub i32, pub C);
// SAFETY: `repr(C)` over `Words` operands.
unsafe impl<C: Words> Words for Bar<C> {}
impl<C: Expr<Out = Color>> Arch for Bar<C> {
    const READS: u64 = C::READS;
    const NAME: &'static str = "bar";
    const STAGE: StageId = later(&[StageId::Turn, C::STAGE]);
    #[inline(always)]
    fn changed(&self, old: &U, new: &U) -> bool {
        self.2.eval(&Cx::of(old)) != self.2.eval(&Cx::of(new))
    }
    #[inline(always)]
    fn paint<B: Backend>(b: &mut B, col: ColRef<'_, Self>, u: &U) {
        b.draws(col, u)
    }
}
impl<C: Expr<Out = Color>> Draws for Bar<C> {
    #[inline(always)]
    fn draw<P: Prims>(&self, r: Rect, cut: Rect, u: &U, p: &mut P) {
        let c = self.2.eval(&Cx::of(u));
        // a colour with no alpha is no bar (a caret in its off phase)
        if self.1 > 0 && c.0 >> 24 != 0 {
            p.solid(Rect { x0: r.x0 + self.0, x1: r.x0 + self.0 + self.1, ..r }.and(r).and(cut), c.0)
        }
    }
}

/// A rounded box of radius `R` turned by an expression (a full turn is 65,536): a spinner is this with the
/// turn a function of `Time`, so it costs no turn while it spins.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Spin<const R: i32, C, T>(pub C, pub T);
// SAFETY: `repr(C)` over `Words` operands.
unsafe impl<const R: i32, C: Words, T: Words> Words for Spin<R, C, T> {}
impl<const R: i32, C: Expr<Out = Color>, T: Expr<Out = u32>> Arch for Spin<R, C, T> {
    const READS: u64 = C::READS | T::READS;
    const NAME: &'static str = "spin";
    const STAGE: StageId = later(&[C::STAGE, T::STAGE]);
    const TIMED: bool = (C::READS | T::READS) >> U_TIME & 1 != 0;
    #[inline(always)]
    fn until(&self, u: &U) -> u32 {
        self.0.until(&Cx::of(u)).max(self.1.until(&Cx::of(u)))
    }
    /// Turned, the box stays inside the circle through its corners.
    #[inline(always)]
    fn sweep(&self, r: Rect, _: &U) -> Rect {
        reach(r)
    }
    #[inline(always)]
    fn changed(&self, old: &U, new: &U) -> bool {
        (self.0.eval(&Cx::of(old)) != self.0.eval(&Cx::of(new))) | (self.1.eval(&Cx::of(old)) != self.1.eval(&Cx::of(new)))
    }
    #[inline(always)]
    fn paint<B: Backend>(b: &mut B, col: ColRef<'_, Self>, u: &U) {
        b.draws(col, u)
    }
}
/// The box a turned box of rect `r` stays inside.
#[inline(always)]
pub fn reach(r: Rect) -> Rect {
    let (w, h) = ((r.x1 - r.x0) as f32, (r.y1 - r.y0) as f32);
    let d = ((w * w + h * h).sqrt() * 0.5).ceil() as i32 + 1;
    let (cx, cy) = ((r.x0 + r.x1) / 2, (r.y0 + r.y1) / 2);
    Rect { x0: cx - d, y0: cy - d, x1: cx + d + 1, y1: cy + d + 1 }
}
impl<const R: i32, C: Expr<Out = Color>, T: Expr<Out = u32>> Draws for Spin<R, C, T> {
    #[inline(always)]
    fn draw<P: Prims>(&self, r: Rect, cut: Rect, u: &U, p: &mut P) {
        p.rbox(r, cut, R, self.1.eval(&Cx::of(u)), self.0.eval(&Cx::of(u)).0)
    }
}

/// `Time` (milliseconds) as a turn of period `MS`: an endless motion.
#[derive(Clone, Copy)]
pub struct Turns<const MS: u32>;
// SAFETY: zero-sized.
unsafe impl<const MS: u32> Words for Turns<MS> {}
impl<const MS: u32> Expr for Turns<MS> {
    type Out = u32;
    type Stage = bough_expr::Frame;
    const READS: u64 = 1 << U_TIME;
    const CODE: &'static Code = &Code::Call("x_turns", &[&Code::U(U_TIME), &Code::Lit(MS)]);
    #[inline(always)]
    fn eval(&self, cx: &Cx) -> u32 {
        ((cx.u.w[U_TIME] % MS) as u64 * 65536 / MS as u64) as u32
    }
}
