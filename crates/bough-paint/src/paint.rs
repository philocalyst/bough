//! Paints as types. A paint is a primitive whose operands are expressions, or a composition of paints
//! (`Over`, `Clip`, `Hit`). The value of a paint type is one record. Its type fixes, before the program
//! runs, everything the generic kernel discovered per command per tile: which primitives, in which order,
//! which operands move its window, whether its core replaces what is beneath, and what it costs.
//!
//! A paint describes itself by continuation: `paint` evaluates its operands and drives a `Sink` with the
//! staged primitives, in paint order, the kind of each call fixed by the type. The tile rasteriser, the
//! flat-row classifier, the window pass, the per-pixel specification, the oracle's stager and the pick are
//! six sinks; each is monomorphised per archetype, so none of them tests a kind.
use crate::expr::{Alpha, Code, Cx, Expr, Ty, Val, Words};
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
    Prim {
        mk: &'static str,
        p: &'static str,
        args: &'static [&'static Code],
        tys: &'static [Ty],
        glyph: bool,
        /// Its core covers when the colour operands `full` names (indices into `args`) all have full alpha.
        opaque: bool,
        full: &'static [usize],
        /// Its last operand is a table handle (`Shadow`): the generated function reads the corner table.
        tab: bool,
    },
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
    /// Called once when the record is mounted or rewritten, under a zero block: a layer whose coverage is a
    /// pure function of record constants and the pixel's offset asks `m` for the table of its key and keeps
    /// the handle in its own record word. `m(radius, blur)` returns a handle.
    #[inline(always)]
    fn memo(&mut self, _cx: &Cx, _m: &mut dyn FnMut(i32, i32) -> u32) {}
}

/// A primitive from its stager leaf and its shade leaf:
/// `prim!(Name: sink_method = mk_leaf, p_leaf(field: Type: Out: moves_window, ..); full = [colour operands
/// whose full alpha makes the core a cover, with their positions]; cover = ..; cost = ..)`.
macro_rules! prim {
    ($(#[$m:meta])* $name:ident : $sink:ident = $mk:ident, $p:ident ( $($f:ident : $T:ident : $ty:ty : $geom:literal),* ); full = [$($ff:ident : $FT:ident = $fi:literal),*]; cover = $cov:literal; cost = $cost:expr; glyph = $gl:literal) => {
        $(#[$m])*
        #[repr(C)]
        #[derive(Clone, Copy)]
        pub struct $name<$($T),*> { $(pub $f: $T),* }
        // SAFETY: `repr(C)` over `Words` operands.
        unsafe impl<$($T: Words),*> Words for $name<$($T),*> {}
        impl<$($T: Expr<Out = $ty>),*> Paint for $name<$($T),*>
        where
            $($FT: Alpha,)*
        {
            const READS: u64 = 0 $(| $T::READS)*;
            const GEOM_READS: u64 = 0 $(| if $geom { $T::READS } else { 0 })*;
            const OPAQUE_CORE: bool = $cov $(&& $FT::MAY)*;
            const COST: Cost = $cost;
            const LAYERS: usize = 1;
            const CODE: &'static PCode = &PCode::Prim { mk: stringify!($mk), p: stringify!($p), args: &[$($T::CODE),*], tys: &[$(<$ty as Val>::TY),*], glyph: $gl, opaque: Self::OPAQUE_CORE, full: &[$($fi),*], tab: false };
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
                if Self::OPAQUE_CORE $(&& self.$ff.full(cx))* { x::$mk($(self.$f.eval(cx)),*, clip).core } else { EMPTY }
            }
        }
    };
}

prim!(/// A solid box (no antialiasing: pixels whose centres it holds).
    Solid: solid = mk_solid, p_solid(b: B: Vec4i: true, c: C: Vec4u: false); full = [c: C = 1]; cover = true; cost = Cost::Flat; glyph = false);
prim!(/// A rounded box.
    RRect: rrect = mk_rrect, p_rrect(b: B: Vec4i: true, r: R: i32: true, c: C: Vec4u: false); full = [c: C = 2]; cover = true; cost = Cost::Edge; glyph = false);
prim!(/// A rounded box of colour `fill` with a border of width `w` in colour `c` inside its edge.
    Border: border = mk_border, p_border(b: B: Vec4i: true, r: R: i32: true, w: W: i32: true, c: C: Vec4u: false, fill: F: Vec4u: false); full = [fill: F = 4]; cover = true; cost = Cost::Edge; glyph = false);
/// A table handle operand of a primitive that can memoise part of its coverage (`Shadow`'s corners).
pub trait TabOp: Expr<Out = u32> {
    /// The record owns a handle word.
    const ON: bool;
    fn put(&mut self, _handle: u32) {}
}
/// A handle word in the record: filled in when the record is mounted (`Paint::memo`).
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct Tab(pub u32);
impl Tab {
    pub const NONE: Tab = Tab(x::T_MISS);
}
// SAFETY: one word.
unsafe impl Words for Tab {}
impl Expr for Tab {
    type Out = u32;
    type Stage = crate::expr::Mount;
    const READS: u64 = 0;
    const CODE: &'static Code = &Code::Rec;
    #[inline(always)]
    fn eval(&self, _: &Cx) -> u32 {
        self.0
    }
}
impl TabOp for Tab {
    const ON: bool = true;
    #[inline(always)]
    fn put(&mut self, h: u32) {
        self.0 = h;
    }
}
/// No table: the closed form always (no record word).
pub type NoTab = crate::expr::K<{ x::T_MISS }>;
impl TabOp for NoTab {
    const ON: bool = false;
}

/// The shadow of a rounded box, blurred by a spline of width `blur` (Q8, two sigma). `tab`: `Tab` to let the
/// corner corrections be read from a table shared by every shadow of the same radius and blur, `NoTab`
/// (as a value: `K`) for the closed form only.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Shadow<B, R, S, C, T = NoTab> {
    pub b: B,
    pub r: R,
    pub blur: S,
    pub c: C,
    pub tab: T,
}
// SAFETY: `repr(C)` over `Words` operands.
unsafe impl<B: Words, R: Words, S: Words, C: Words, T: Words> Words for Shadow<B, R, S, C, T> {}
impl<B: Expr<Out = Vec4i>, R: Expr<Out = i32>, S: Expr<Out = i32>, C: Expr<Out = Vec4u>, T: TabOp> Paint for Shadow<B, R, S, C, T> {
    const READS: u64 = B::READS | R::READS | S::READS | C::READS;
    const GEOM_READS: u64 = B::READS | R::READS | S::READS;
    const OPAQUE_CORE: bool = false;
    const COST: Cost = Cost::Wide;
    const LAYERS: usize = 1;
    const CODE: &'static PCode = &PCode::Prim {
        mk: "mk_shadow",
        p: "p_shadow",
        args: &[B::CODE, R::CODE, S::CODE, C::CODE, T::CODE],
        tys: &[Ty::V4i, Ty::I32, Ty::I32, Ty::V4u, Ty::U32],
        glyph: false,
        opaque: false,
        full: &[],
        tab: T::ON,
    };
    #[inline(always)]
    fn paint<Sk: Sink>(&self, cx: &Cx, clip: Vec4i, s: &mut Sk) {
        s.shadow(x::mk_shadow(self.b.eval(cx), self.r.eval(cx), self.blur.eval(cx), self.c.eval(cx), self.tab.eval(cx), clip))
    }
    #[inline(always)]
    fn changed(&self, old: &Cx, new: &Cx, mask: u64) -> bool {
        (B::READS & mask != 0 && self.b.eval(old) != self.b.eval(new))
            || (R::READS & mask != 0 && self.r.eval(old) != self.r.eval(new))
            || (S::READS & mask != 0 && self.blur.eval(old) != self.blur.eval(new))
            || (C::READS & mask != 0 && self.c.eval(old) != self.c.eval(new))
    }
    #[inline(always)]
    fn cover(&self, _: &Cx, _: Vec4i) -> Vec4i {
        EMPTY
    }
    fn memo(&mut self, cx: &Cx, m: &mut dyn FnMut(i32, i32) -> u32) {
        // radius and blur fixed for the record's life: its corners are one table's. (A box whose size is
        // animated can still change the clamped radius; the key in the table's header is checked while
        // shading, so that only costs the closed form.)
        if T::ON && R::READS == 0 && S::READS == 0 {
            let z = Vec4 { x: 0, y: 0, z: 0, w: 0 };
            let c = x::mk_shadow(self.b.eval(cx), self.r.eval(cx), self.blur.eval(cx), z, x::T_MISS, NO_CLIP);
            if x::t_regime(c.rad.x, c.rad.y) {
                self.tab.put(m(c.rad.x, c.rad.y));
            }
        }
    }
}
prim!(/// A two-stop linear gradient in a rounded box.
    Grad: grad = mk_grad, p_grad(b: B: Vec4i: true, r: R: i32: true, dx: X: i32: false, dy: Y: i32: false, c: C: Vec4u: false, c2: D: Vec4u: false); full = [c: C = 4, c2: D = 5]; cover = true; cost = Cost::PerPixel; glyph = false);
prim!(/// One glyph: `b` is its texels' box, `texel` its first texel in the atlas.
    Glyph: glyph = mk_glyph, p_glyph(b: B: Vec4i: true, texel: T: u32: false, c: C: Vec4u: false); full = []; cover = false; cost = Cost::Flat; glyph = true);

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
    #[inline(always)]
    fn memo(&mut self, cx: &Cx, m: &mut dyn FnMut(i32, i32) -> u32) {
        self.0.memo(cx, m);
        self.1.memo(cx, m);
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
    #[inline(always)]
    fn memo(&mut self, cx: &Cx, m: &mut dyn FnMut(i32, i32) -> u32) {
        self.p.memo(cx, m);
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
    #[inline(always)]
    fn memo(&mut self, cx: &Cx, m: &mut dyn FnMut(i32, i32) -> u32) {
        self.p.memo(cx, m);
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
    /// The corner tables.
    pub tabs: &'a [u32],
    /// Some layer inked this pixel (the pick's test).
    pub ink: bool,
}
/// The four corner taps of a shadow at one pixel, read from `tabs`.
#[inline(always)]
pub fn corner_taps(c: Cmd, x: i32, y: i32, ok: bool, tabs: &[u32]) -> [u32; 4] {
    let base = c.par.z as usize;
    let t = x::x_ctaps(c, x, y, ok);
    let mut v = [t.x, t.y, t.z, t.w];
    for v in v.iter_mut() {
        *v = if *v == x::T_ZERO {
            0
        } else if *v == x::T_MISS {
            x::T_MISS
        } else {
            x::t_half(tabs[base + 2 + (*v >> 1) as usize], *v)
        };
    }
    v
}
/// The table named by a shadow's handle is filled for its key.
#[inline(always)]
pub fn tab_ok(c: Cmd, tabs: &[u32]) -> bool {
    let base = c.par.z as usize;
    base.saturating_add(1) < tabs.len() && x::x_tabok(c, tabs[base], tabs[base + 1])
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
        let v = corner_taps(c, self.x, self.y, tab_ok(c, self.tabs), self.tabs);
        self.put(x::p_shadowm(c, self.x, self.y, v[0], v[1], v[2], v[3]))
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
