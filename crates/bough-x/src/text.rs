//! Text as records (text lane; `docs/r8/TEXT-SEAM.md` revision 2).
//!
//! * [`Line`]: one line of shaped text. Its operands are a [`Run`] (two words: a range of the shared glyph
//!   lanes and the face it was shaped in) and a colour expression. The glyphs are Mount stage (shaped once,
//!   when the row is described; the memo in [`bough_face::Glyphs`] gives the same run for the same text, so a
//!   re-description compares equal), the colour is whatever stage its expression is. The record's rect is the
//!   line's box: the pen starts at its left edge and the baseline is centred in its height. Nothing is placed
//!   in pixels until the backend paints it ([`bough_face::Face::spans`]).
//! * [`Digits`]: a number drawn with `D` decimals and at least `P` digits, on the face's digit cell. Like
//!   [`crate::kinds::Num`] its value is an expression of any stage (a record constant, a word of the block),
//!   and its glyphs are arithmetic on the value while shading: no run, no lanes.
//!
//! What a backend needs to paint either is the application's table of faces and, for lines, the lanes:
//! [`TextCx`].
use crate::expr::{Code, Color, Cx, Expr, StageId, U, U_TIME, Words, later};
use crate::kinds::NumArch;
use crate::reflect::Val;
use crate::store::{Arch, Backend, ColRef, Op};
pub use bough_face::{Face, Glyphs, Run};
use core::mem::offset_of;

// SAFETY: `Run` is `repr(C)` over two `u32`s: whole words, no padding, every bit pattern valid.
unsafe impl Words for RunW {}
/// A [`Run`] as record words.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct RunW(pub Run);

pub trait LineArch: Arch {
    fn run(&self) -> Run;
    fn color(&self, u: &U) -> Color;
}

/// One line of shaped text in a colour.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Line<C>(pub RunW, pub C);
// SAFETY: `repr(C)` over `Words` operands: whole words at alignment 4, so no padding.
unsafe impl<C: Words> Words for Line<C> {}
impl<C> Line<C> {
    #[inline(always)]
    pub fn new(run: Run, color: C) -> Line<C> {
        Line(RunW(run), color)
    }
}
impl<C: Expr<Out = Color>> Arch for Line<C> {
    const OPS: &'static [Op] = &[
        Op { name: "run", code: &Code::Rec, ty: core::any::type_name::<Run>, stage: StageId::Mount, reads: 0, word: 0, words: 2 },
        Op { name: "color", code: C::CODE, ty: core::any::type_name::<C>, stage: C::STAGE, reads: C::READS, word: (offset_of!(Self, 1) / 4) as u16, words: (size_of::<C>() / 4) as u16 },
    ];
    const READS: u64 = C::READS;
    const NAME: &'static str = "line";
    const STAGE: StageId = C::STAGE;
    const TIMED: bool = C::READS >> U_TIME & 1 != 0;
    #[inline(always)]
    fn until(&self, u: &U) -> u32 {
        self.1.until(&Cx::of(u))
    }
    #[inline(always)]
    fn changed(&self, old: &U, new: &U) -> bool {
        #[cfg(feature = "mutant_line")]
        return (self.1.eval(&Cx::of(old)) != self.1.eval(&Cx::of(new))) & false;
        #[cfg(not(feature = "mutant_line"))]
        return self.1.eval(&Cx::of(old)) != self.1.eval(&Cx::of(new));
    }
    #[inline(always)]
    fn paint<B: Backend>(b: &mut B, col: ColRef<'_, Self>, u: &U) {
        b.line(col, u)
    }
    #[inline]
    fn val(&self, op: usize, u: &U) -> Val {
        match op {
            0 => Val::Pair(self.0.0.g0 as i32, self.0.0.n_face as i32),
            1 => Val::Color(self.1.eval(&Cx::of(u)).0),
            _ => Val::None,
        }
    }
}
impl<C: Expr<Out = Color>> LineArch for Line<C> {
    #[inline(always)]
    fn run(&self) -> Run {
        self.0.0
    }
    #[inline(always)]
    fn color(&self, u: &U) -> Color {
        self.1.eval(&Cx::of(u))
    }
}

/// A number with `D` decimals and at least `P` digits (zeros on the left), tabular: `Digits::<0, 5, _, _>` is
/// `{:05}`. The value is an integer scaled by `10^D`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Digits<const D: u8, const P: u8, V, C>(pub V, pub C);
// SAFETY: `repr(C)` over `Words` operands.
unsafe impl<const D: u8, const P: u8, V: Words, C: Words> Words for Digits<D, P, V, C> {}
impl<const D: u8, const P: u8, V: Expr<Out = i32>, C: Expr<Out = Color>> Arch for Digits<D, P, V, C> {
    const OPS: &'static [Op] = &[
        Op { name: "value", code: V::CODE, ty: core::any::type_name::<V>, stage: V::STAGE, reads: V::READS, word: 0, words: (size_of::<V>() / 4) as u16 },
        Op { name: "color", code: C::CODE, ty: core::any::type_name::<C>, stage: C::STAGE, reads: C::READS, word: (offset_of!(Self, 1) / 4) as u16, words: (size_of::<C>() / 4) as u16 },
    ];
    const READS: u64 = V::READS | C::READS;
    const NAME: &'static str = "digits";
    const STAGE: StageId = later(&[V::STAGE, C::STAGE]);
    const TIMED: bool = (V::READS | C::READS) >> U_TIME & 1 != 0;
    #[inline(always)]
    fn until(&self, u: &U) -> u32 {
        self.0.until(&Cx::of(u)).max(self.1.until(&Cx::of(u)))
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
impl<const D: u8, const P: u8, V: Expr<Out = i32>, C: Expr<Out = Color>> NumArch for Digits<D, P, V, C> {
    const DECIMALS: u8 = D;
    const PAD: u8 = P;
    #[inline(always)]
    fn value(&self, u: &U) -> i32 {
        self.0.eval(&Cx::of(u))
    }
    #[inline(always)]
    fn color(&self, u: &U) -> Color {
        self.1.eval(&Cx::of(u))
    }
}

/// What a backend reads to paint text: the application's faces (a run names one by index; numbers use face
/// 0) and the lanes its lines index.
#[derive(Clone, Copy)]
pub struct TextCx<'a> {
    pub faces: &'a [Face],
    pub glyphs: &'a Glyphs,
}

impl TextCx<'_> {
    /// The coverage of a line record whose box is `r` (already in screen coordinates), cut to `clip`.
    #[inline(always)]
    pub fn line(&self, run: Run, r: crate::store::Rect, clip: crate::store::Rect, f: impl FnMut(i32, i32, &[u8])) {
        let face = &self.faces[run.face()];
        let (slot, x) = self.glyphs.of(run);
        face.spans(slot, x, 0, (r.x0, r.y0 + face.baseline(r.y1 - r.y0)), [clip.x0, clip.y0, clip.x1, clip.y1], f);
    }
    /// The coverage of a number record whose box is `r`: its left edge at the box's left edge.
    #[inline(always)]
    pub fn num(&self, v: i32, d: u8, pad: u8, r: crate::store::Rect, clip: crate::store::Rect, f: impl FnMut(i32, i32, &[u8])) {
        let face = &self.faces[0];
        let Some(cell) = face.cell() else { return };
        bough_face::num::Num::new(v, d as u32, pad as u32).spans(face, &cell, (r.x0, r.y0 + face.baseline(r.y1 - r.y0)), false, [clip.x0, clip.y0, clip.x1, clip.y1], f);
    }
}

/// Keep the glyph memo bounded: when it holds more than `budget` bytes, forget every line and describe what
/// is exposed again. Call it after a settle. The exposed records are the only holders of runs, so after the
/// re-description every run on screen is of the new generation and everything else is gone. The cost is one
/// re-description and one repaint of every text record's rect (their run words changed, though their pixels
/// did not). Returns whether it collected.
pub fn collect<V: crate::sink::View, B: Backend>(world: &mut crate::world::World<B>, v: &V, glyphs: &core::cell::RefCell<Glyphs>, budget: usize) -> bool {
    if !glyphs.borrow().over(budget) {
        return false;
    }
    glyphs.borrow_mut().clear();
    world.redescribe(v);
    true
}

/// A line of `text` in face `id`, always something: the face's exact line when it covers the text, otherwise
/// its stand-in ([`Face::shape_lossy`]: uncovered code points as `?`, unkerned where a pair needs the full
/// shaper), with `false`. What an application shows while the full shaper's line is not there.
pub fn line_or_stand_in(glyphs: &mut Glyphs, id: u16, face: &Face, text: &str) -> (Run, i32, bool) {
    if let Some((r, w)) = glyphs.line(id, face, text) {
        return (r, w, true);
    }
    // a different key from the exact line's, so the exact one can replace it when it lands
    let h = bough_face::key(id as u32 | 1 << 31, text.as_bytes());
    let (r, w) = glyphs
        .line_with(h, id, text.as_bytes(), |slot, x| {
            Some(
                face.shape_lossy(text.as_bytes(), |t, pen, _| {
                    slot.push(t as u16);
                    x.push(pen);
                })
                .0,
            )
        })
        .expect("a stand-in is never refused");
    (r, w, false)
}
