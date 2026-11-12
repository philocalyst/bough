//! Paints as types. A paint is a primitive whose operands are expressions, or a composition of paints
//! (`Over`, `Clip`, `Hit`). The value of a paint type is one record. Its type fixes, before the program
//! runs, everything the generic kernel discovered per command per tile: which primitives, in which order,
//! which operands move its window, whether its core replaces what is beneath, and what it costs.
//!
//! A paint describes itself by continuation: `paint` evaluates its operands and drives a `Sink` with the
//! staged primitives, in paint order, the kind of each call fixed by the type. The tile rasteriser, the
//! flat-row classifier, the window pass, the per-pixel specification, the oracle's stager and the pick are
//! six sinks; each is monomorphised per archetype, so none of them tests a kind.
use crate::expr::{Code, Cx, Expr, Ty, Val, Words};
use crate::ops::ops as x;
use bough_kernel::Cmd;
use wgsl_rs::std::{Vec4, Vec4i, Vec4u};

pub const EMPTY: Vec4i = Vec4 { x: 0, y: 0, z: 0, w: 0 };
/// No clip (Q8), the kernel stager's bounds.
pub const NO_CLIP: Vec4i = Vec4 { x: -4_096_000, y: -4_096_000, z: 4_096_000, w: 4_096_000 };
pub const NO_HIT: u32 = u32::MAX;

/// Per-pixel cost class of a paint (the plan's input; `plan.rs`).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Cost {
    /// Constant spans (solids, opaque interiors, glyph rows): a CPU core beats a GPU round trip.
    Flat = 0,
    /// A constant interior with a distance evaluated along the edges (rounded boxes, borders).
    Edge = 1,
    /// Arithmetic at every pixel (gradients).
    PerPixel = 2,
    /// A neighbourhood integrated at every pixel (blurred shadows).
    Wide = 3,
}
pub const fn worst(a: Cost, b: Cost) -> Cost {
    if a as u8 >= b as u8 { a } else { b }
}

/// A paint's structure as data.
#[derive(Debug)]
pub enum PCode {
    /// `p(mk(args.., clip), x, y)`; `glyph`: `p` also takes the atlas byte at `x_texel(mk(..), x, y, aw)`.
    Prim { mk: &'static str, p: &'static str, args: &'static [&'static Code], tys: &'static [Ty], glyph: bool, opaque: bool },
    /// `a` then `b`; `cut`: `b`'s core is opaque, so `a` is skipped inside it.
    Over { a: &'static PCode, b: &'static PCode, cut: bool },
    Clip { p: &'static PCode, k: &'static Code },
    Hit { p: &'static PCode, id: &'static Code },
}

/// Where a paint's staged primitives go. Every method has the kind in its name: the caller is a paint
/// type, so the callee is chosen when the archetype is compiled.
pub trait Sink {
    fn solid(&mut self, c: Cmd);
    fn rrect(&mut self, c: Cmd);
    fn border(&mut self, c: Cmd);
    fn shadow(&mut self, c: Cmd);
    fn grad(&mut self, c: Cmd);
    fn glyph(&mut self, c: Cmd);
    /// The pixels of `core` are about to be replaced by an opaque layer of the same record: what is painted
    /// until `show` may skip them. Returns what to hand back to `show`.
    #[inline(always)]
    fn hide(&mut self, _core: Vec4i) -> Vec4i {
        EMPTY
    }
    #[inline(always)]
    fn show(&mut self, _prev: Vec4i) {}
}

pub trait Paint: Words {
    const READS: u64;
    /// Block words that can move its window (rect, radius, blur and clip operands).
    const GEOM_READS: u64;
    /// Some layer's core is opaque: it covers what is beneath (the occlusion cut's condition, from types).
    const OPAQUE_CORE: bool;
    const COST: Cost;
    const LAYERS: usize;
    const HIT: bool = false;
    const CODE: &'static PCode;
    fn paint<Sk: Sink>(&self, cx: &Cx, clip: Vec4i, s: &mut Sk);
    /// Some operand that reads a word of `mask` has a different value under the two blocks.
    fn changed(&self, old: &Cx, new: &Cx, mask: u64) -> bool;
    /// The pixels this record covers opaquely (empty unless `OPAQUE_CORE`).
    fn cover(&self, cx: &Cx, clip: Vec4i) -> Vec4i;
    #[inline(always)]
    fn pick_id(&self, _cx: &Cx) -> u32 {
        NO_HIT
    }
}

/// A primitive from its stager leaf and its shade leaf:
/// `prim!(Name: sink_method = mk_leaf, p_leaf(field: Type: Out: moves_window, ..); opaque = ..; cost = ..)`.
macro_rules! prim {
    ($(#[$m:meta])* $name:ident : $sink:ident = $mk:ident, $p:ident ( $($f:ident : $T:ident : $ty:ty : $geom:literal),* ); opaque = $opq:expr; cost = $cost:expr; glyph = $gl:literal) => {
        $(#[$m])*
        #[repr(C)]
        #[derive(Clone, Copy)]
        pub struct $name<$($T),*> { $(pub $f: $T),* }
        // SAFETY: `repr(C)` over `Words` operands.
        unsafe impl<$($T: Words),*> Words for $name<$($T),*> {}
        impl<$($T: Expr<Out = $ty>),*> Paint for $name<$($T),*> {
            const READS: u64 = 0 $(| $T::READS)*;
            const GEOM_READS: u64 = 0 $(| if $geom { $T::READS } else { 0 })*;
            const OPAQUE_CORE: bool = $opq;
            const COST: Cost = $cost;
            const LAYERS: usize = 1;
            const CODE: &'static PCode = &PCode::Prim { mk: stringify!($mk), p: stringify!($p), args: &[$($T::CODE),*], tys: &[$(<$ty as Val>::TY),*], glyph: $gl, opaque: $opq };
            #[inline(always)]
            fn paint<Sk: Sink>(&self, cx: &Cx, clip: Vec4i, s: &mut Sk) {
                s.$sink(x::$mk($(self.$f.eval(cx)),*, clip))
            }
            #[inline(always)]
            fn changed(&self, old: &Cx, new: &Cx, mask: u64) -> bool {
                false $(|| ($T::READS & mask != 0 && self.$f.eval(old) != self.$f.eval(new)))*
            }
            #[inline(always)]
            fn cover(&self, cx: &Cx, clip: Vec4i) -> Vec4i {
                if Self::OPAQUE_CORE { x::$mk($(self.$f.eval(cx)),*, clip).core } else { EMPTY }
            }
        }
    };
}

prim!(/// A solid box (no antialiasing: pixels whose centres it holds).
    Solid: solid = mk_solid, p_solid(b: B: Vec4i: true, c: C: Vec4u: false); opaque = C::OPAQUE; cost = Cost::Flat; glyph = false);
prim!(/// A rounded box.
    RRect: rrect = mk_rrect, p_rrect(b: B: Vec4i: true, r: R: i32: true, c: C: Vec4u: false); opaque = C::OPAQUE; cost = Cost::Edge; glyph = false);
prim!(/// A rounded box of colour `fill` with a border of width `w` in colour `c` inside its edge.
    Border: border = mk_border, p_border(b: B: Vec4i: true, r: R: i32: true, w: W: i32: true, c: C: Vec4u: false, fill: F: Vec4u: false); opaque = F::OPAQUE; cost = Cost::Edge; glyph = false);
prim!(/// The shadow of a rounded box, blurred by a spline of width `blur` (Q8, two sigma).
    Shadow: shadow = mk_shadow, p_shadow(b: B: Vec4i: true, r: R: i32: true, blur: S: i32: true, c: C: Vec4u: false); opaque = false; cost = Cost::Wide; glyph = false);
prim!(/// A two-stop linear gradient in a rounded box.
    Grad: grad = mk_grad, p_grad(b: B: Vec4i: true, r: R: i32: true, dx: X: i32: false, dy: Y: i32: false, c: C: Vec4u: false, c2: D: Vec4u: false); opaque = C::OPAQUE && D::OPAQUE; cost = Cost::PerPixel; glyph = false);
prim!(/// One glyph: `b` is its texels' box, `texel` its first texel in the atlas.
    Glyph: glyph = mk_glyph, p_glyph(b: B: Vec4i: true, texel: T: u32: false, c: C: Vec4u: false); opaque = false; cost = Cost::Flat; glyph = true);

/// `A` beneath `B`, one record. Where `B`'s core is opaque, `A` is not evaluated: the cut between a record's
/// own layers is decided by the types, not found by a binner.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Over<A, B>(pub A, pub B);
// SAFETY: `repr(C)` over `Words` operands.
unsafe impl<A: Words, B: Words> Words for Over<A, B> {}
impl<A: Paint, B: Paint> Paint for Over<A, B> {
    const READS: u64 = A::READS | B::READS;
    const GEOM_READS: u64 = A::GEOM_READS | B::GEOM_READS;
    const OPAQUE_CORE: bool = A::OPAQUE_CORE || B::OPAQUE_CORE;
    const COST: Cost = worst(A::COST, B::COST);
    const LAYERS: usize = A::LAYERS + B::LAYERS;
    const HIT: bool = A::HIT || B::HIT;
    const CODE: &'static PCode = &PCode::Over { a: A::CODE, b: B::CODE, cut: B::OPAQUE_CORE };
    #[inline(always)]
    fn paint<Sk: Sink>(&self, cx: &Cx, clip: Vec4i, s: &mut Sk) {
        if B::OPAQUE_CORE {
            let prev = s.hide(self.1.cover(cx, clip));
            self.0.paint(cx, clip, s);
            s.show(prev);
        } else {
            self.0.paint(cx, clip, s);
        }
        self.1.paint(cx, clip, s);
    }
    #[inline(always)]
    fn changed(&self, old: &Cx, new: &Cx, mask: u64) -> bool {
        self.0.changed(old, new, mask) || self.1.changed(old, new, mask)
    }
    #[inline(always)]
    fn cover(&self, cx: &Cx, clip: Vec4i) -> Vec4i {
        // the upper layer's core if it has one; else the lower's is still a cover only where the upper layer
        // does not ink, which is not known here, so a translucent top makes the record no cover
        if B::OPAQUE_CORE { self.1.cover(cx, clip) } else { EMPTY }
    }
    #[inline(always)]
    fn pick_id(&self, cx: &Cx) -> u32 {
        if B::HIT { self.1.pick_id(cx) } else { self.0.pick_id(cx) }
    }
}

/// `P` cut by a rect expression (Q8).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Clip<P, K> {
    pub p: P,
    pub k: K,
}
// SAFETY: `repr(C)` over `Words` operands.
unsafe impl<P: Words, K: Words> Words for Clip<P, K> {}
impl<P: Paint, K: Expr<Out = Vec4i>> Paint for Clip<P, K> {
    const READS: u64 = P::READS | K::READS;
    const GEOM_READS: u64 = P::GEOM_READS | K::READS;
    const OPAQUE_CORE: bool = P::OPAQUE_CORE;
    const COST: Cost = P::COST;
    const LAYERS: usize = P::LAYERS;
    const HIT: bool = P::HIT;
    const CODE: &'static PCode = &PCode::Clip { p: P::CODE, k: K::CODE };
    #[inline(always)]
    fn paint<Sk: Sink>(&self, cx: &Cx, clip: Vec4i, s: &mut Sk) {
        self.p.paint(cx, x::x_and(clip, self.k.eval(cx)), s)
    }
    #[inline(always)]
    fn changed(&self, old: &Cx, new: &Cx, mask: u64) -> bool {
        (K::READS & mask != 0 && self.k.eval(old) != self.k.eval(new)) || self.p.changed(old, new, mask)
    }
    #[inline(always)]
    fn cover(&self, cx: &Cx, clip: Vec4i) -> Vec4i {
        self.p.cover(cx, x::x_and(clip, self.k.eval(cx)))
    }
    #[inline(always)]
    fn pick_id(&self, cx: &Cx) -> u32 {
        self.p.pick_id(cx)
    }
}

/// `P`, answering the pick with `id` wherever it inks.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Hit<P, I> {
    pub p: P,
    pub id: I,
}
// SAFETY: `repr(C)` over `Words` operands.
unsafe impl<P: Words, I: Words> Words for Hit<P, I> {}
impl<P: Paint, I: Expr<Out = u32>> Paint for Hit<P, I> {
    const READS: u64 = P::READS | I::READS;
    const GEOM_READS: u64 = P::GEOM_READS;
    const OPAQUE_CORE: bool = P::OPAQUE_CORE;
    const COST: Cost = P::COST;
    const LAYERS: usize = P::LAYERS;
    const HIT: bool = true;
    const CODE: &'static PCode = &PCode::Hit { p: P::CODE, id: I::CODE };
    #[inline(always)]
    fn paint<Sk: Sink>(&self, cx: &Cx, clip: Vec4i, s: &mut Sk) {
        self.p.paint(cx, clip, s)
    }
    #[inline(always)]
    fn changed(&self, old: &Cx, new: &Cx, mask: u64) -> bool {
        self.p.changed(old, new, mask)
    }
    #[inline(always)]
    fn cover(&self, cx: &Cx, clip: Vec4i) -> Vec4i {
        self.p.cover(cx, clip)
    }
    #[inline(always)]
    fn pick_id(&self, cx: &Cx) -> u32 {
        self.id.eval(cx)
    }
}

/// The union of the staged primitives' windows (the record's window lane).
pub struct WinSink(pub Vec4i);
impl WinSink {
    #[inline(always)]
    fn add(&mut self, c: Cmd) {
        self.0 = x::x_union(self.0, c.win);
    }
}
impl Sink for WinSink {
    #[inline(always)]
    fn solid(&mut self, c: Cmd) {
        self.add(c)
    }
    #[inline(always)]
    fn rrect(&mut self, c: Cmd) {
        self.add(c)
    }
    #[inline(always)]
    fn border(&mut self, c: Cmd) {
        self.add(c)
    }
    #[inline(always)]
    fn shadow(&mut self, c: Cmd) {
        self.add(c)
    }
    #[inline(always)]
    fn grad(&mut self, c: Cmd) {
        self.add(c)
    }
    #[inline(always)]
    fn glyph(&mut self, c: Cmd) {
        self.add(c)
    }
}

/// The specification of an archetype at one pixel: every layer through its guarded shade leaf, folded with
/// the kernel's blend. No core, no span, no hidden region. This is what the generated GPU function computes.
pub struct SpecSink<'a> {
    pub x: i32,
    pub y: i32,
    pub acc: Vec4u,
    pub atlas: &'a [u8],
    pub aw: u32,
    /// Some layer inked this pixel (the pick's test).
    pub ink: bool,
}
impl SpecSink<'_> {
    #[inline(always)]
    fn put(&mut self, sh: Vec4u) {
        self.ink |= sh.w != 0;
        self.acc = x::x_over(self.acc, sh);
    }
}
impl Sink for SpecSink<'_> {
    #[inline(always)]
    fn solid(&mut self, c: Cmd) {
        self.put(x::p_solid(c, self.x, self.y))
    }
    #[inline(always)]
    fn rrect(&mut self, c: Cmd) {
        self.put(x::p_rrect(c, self.x, self.y))
    }
    #[inline(always)]
    fn border(&mut self, c: Cmd) {
        self.put(x::p_border(c, self.x, self.y))
    }
    #[inline(always)]
    fn shadow(&mut self, c: Cmd) {
        self.put(x::p_shadow(c, self.x, self.y))
    }
    #[inline(always)]
    fn grad(&mut self, c: Cmd) {
        self.put(x::p_grad(c, self.x, self.y))
    }
    #[inline(always)]
    fn glyph(&mut self, c: Cmd) {
        let t = x::x_texel(c, self.x, self.y, self.aw);
        let cov = if t == bough_kernel::k::NO_TEXEL || self.atlas.is_empty() { 0 } else { self.atlas[(t as usize).min(self.atlas.len() - 1)] as u32 };
        self.put(x::p_glyph(c, self.x, self.y, cov))
    }
}

/// The staged primitives as the generic kernel's commands, in paint order (the oracle's input).
pub struct StageSink<'a>(pub &'a mut bough_kernel::stage::Scene);
impl StageSink<'_> {
    #[inline]
    fn add(&mut self, c: Cmd) {
        if c.win.x < c.win.z {
            self.0.push(c);
        }
    }
}
impl Sink for StageSink<'_> {
    fn solid(&mut self, c: Cmd) {
        self.add(c)
    }
    fn rrect(&mut self, c: Cmd) {
        self.add(c)
    }
    fn border(&mut self, c: Cmd) {
        self.add(c)
    }
    fn shadow(&mut self, c: Cmd) {
        self.add(c)
    }
    fn grad(&mut self, c: Cmd) {
        self.add(c)
    }
    fn glyph(&mut self, c: Cmd) {
        self.add(c)
    }
}
