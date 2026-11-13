//! Elements as the façade builds them. The children of a box are in its type; its style values are plain data.
use crate::flex::{Align, Dim, Hk, Justify, Spec};
use crate::shape::{K_BOX, K_GONE, K_LINE, K_WRAP, Nd, Shape, ShapeKids, Sink, Wrap};
use crate::val::{Av, Cx, V};
use core::marker::PhantomData;

/// The paint records an element owns: none, or one.
pub trait Paint {
    const N: usize;
}
impl Paint for () {
    const N: usize = 0;
}
/// One record of any paint type.
#[derive(Clone, Copy)]
pub struct Rec1<A>(pub A);
impl<A> Paint for Rec1<A> {
    const N: usize = 1;
}

/// The height was set by the author / comes from content.
#[derive(Clone, Copy)]
pub struct HFixed;
#[derive(Clone, Copy)]
pub struct HAuto;
pub trait HState {
    const FIXED: bool;
}
impl HState for HFixed {
    const FIXED: bool = true;
}
impl HState for HAuto {
    const FIXED: bool = false;
}

/// A layout box: a flex container with children `C`, painted by `P`.
#[derive(Clone, Copy)]
pub struct LBox<P = (), C = (), H = HAuto> {
    pub spec: Spec,
    pub paint: P,
    pub kids: C,
    _h: PhantomData<H>,
}

#[inline(always)]
pub fn column<C>(kids: C) -> LBox<(), C> {
    LBox { spec: Spec::new(true), paint: (), kids, _h: PhantomData }
}
#[inline(always)]
pub fn row<C>(kids: C) -> LBox<(), C> {
    LBox { spec: Spec::new(false), paint: (), kids, _h: PhantomData }
}
#[inline(always)]
pub fn div() -> LBox<(), ()> {
    column(())
}
/// A leaf rectangle of `w` x `h` points.
#[inline(always)]
pub fn block(w: f32, h: f32) -> LBox<(), (), HFixed> {
    div().w(w).h(h)
}

/// A flex layout recipe; every method is `const`, so a recipe can be a `const` item.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Flex {
    column: bool,
    gap: f32,
    align: Option<Align>,
    justify: Option<Justify>,
}
impl Flex {
    pub const fn row() -> Flex {
        Flex { column: false, gap: 0.0, align: None, justify: None }
    }
    pub const fn column() -> Flex {
        Flex { column: true, ..Flex::row() }
    }
    pub const fn gap(self, v: f32) -> Flex {
        Flex { gap: v, ..self }
    }
    pub const fn align(self, a: Align) -> Flex {
        Flex { align: Some(a), ..self }
    }
    pub const fn justify(self, j: Justify) -> Flex {
        Flex { justify: Some(j), ..self }
    }
}

/// `.child(e)` appends: the children so far, then one more.
#[derive(Clone, Copy)]
pub struct Chain<C, E>(pub C, pub E);

macro_rules! style_methods {
    () => {
        #[inline(always)]
        pub fn w(mut self, v: f32) -> Self {
            self.spec.w = Dim::Pt(v);
            self
        }
        #[inline(always)]
        pub fn w_pct(mut self, v: f32) -> Self {
            self.spec.w = Dim::Pct(v);
            self
        }
        #[inline(always)]
        pub fn min_w(mut self, v: f32) -> Self {
            self.spec.min[0] = v;
            self
        }
        #[inline(always)]
        pub fn min_h(mut self, v: f32) -> Self {
            self.spec.min[1] = v;
            self
        }
        #[inline(always)]
        pub fn margin(mut self, l: f32, t: f32, r: f32, b: f32) -> Self {
            self.spec.margin = [l, t, r, b];
            self
        }
        #[inline(always)]
        pub fn grow(mut self, v: f32) -> Self {
            self.spec.grow = v;
            self
        }
        #[inline(always)]
        pub fn shrink(mut self, v: f32) -> Self {
            self.spec.shrink = v;
            self
        }
        #[inline(always)]
        pub fn self_align(mut self, a: Align) -> Self {
            self.spec.self_align = Some(a);
            self
        }
        /// Out of flow, at (x, y) points from the parent's box.
        #[inline(always)]
        pub fn abs(mut self, x: f32, y: f32) -> Self {
            self.spec.abs = Some((x, y));
            self
        }
    };
}

impl<P, C, H> LBox<P, C, H> {
    style_methods!();
    #[inline(always)]
    pub fn h(self, v: f32) -> LBox<P, C, HFixed> {
        let mut spec = self.spec;
        spec.h = Dim::Pt(v);
        LBox { spec, paint: self.paint, kids: self.kids, _h: PhantomData }
    }
    #[inline(always)]
    pub fn h_pct(mut self, v: f32) -> Self {
        self.spec.h = Dim::Pct(v);
        self
    }
    #[inline(always)]
    pub fn gap(mut self, v: f32) -> Self {
        self.spec.gap = v;
        self
    }
    #[inline(always)]
    pub fn pad(mut self, l: f32, t: f32, r: f32, b: f32) -> Self {
        self.spec.pad = [l, t, r, b];
        self
    }
    #[inline(always)]
    pub fn p(self, v: f32) -> Self {
        self.pad(v, v, v, v)
    }
    #[inline(always)]
    pub fn px(mut self, v: f32) -> Self {
        self.spec.pad[0] = v;
        self.spec.pad[2] = v;
        self
    }
    #[inline(always)]
    pub fn py(mut self, v: f32) -> Self {
        self.spec.pad[1] = v;
        self.spec.pad[3] = v;
        self
    }
    #[inline(always)]
    pub fn items(mut self, a: Align) -> Self {
        self.spec.items = Some(a);
        self
    }
    #[inline(always)]
    pub fn justify(mut self, j: Justify) -> Self {
        self.spec.justify = Some(j);
        self
    }
    #[inline(always)]
    pub fn center(self) -> Self {
        self.items(Align::Center).justify(Justify::Center)
    }
    /// Apply a layout recipe (`const BAR: Flex = Flex::row().align(Align::Center).gap(8.0)`).
    #[inline(always)]
    pub fn layout(mut self, f: Flex) -> Self {
        self.spec.column = f.column;
        self.spec.gap = f.gap;
        if f.align.is_some() {
            self.spec.items = f.align;
        }
        if f.justify.is_some() {
            self.spec.justify = f.justify;
        }
        self
    }
    /// Paint the box: `a` is its record.
    #[inline(always)]
    pub fn bg<A>(self, a: A) -> LBox<Rec1<A>, C, H> {
        LBox { spec: self.spec, paint: Rec1(a), kids: self.kids, _h: PhantomData }
    }
    #[inline(always)]
    pub fn child<E>(self, e: E) -> LBox<P, Chain<C, E>, H> {
        LBox { spec: self.spec, paint: self.paint, kids: Chain(self.kids, e), _h: PhantomData }
    }
}

impl<P: Paint, C: ShapeKids, H: HState> Shape for LBox<P, C, H> {
    const LEAVES: usize = P::N + C::LEAVES;
    const WRAPS: bool = C::WRAPS;
    const VARIES: bool = C::VARIES;
    const HK: Hk = if H::FIXED {
        Hk::Fixed
    } else if C::WRAPS {
        Hk::Wraps
    } else if C::VARIES {
        Hk::Content
    } else {
        Hk::Fixed
    };
    #[inline(always)]
    fn emit<'a, S: Sink<'a>>(&'a self, s: &mut S) {
        s.node(Nd::pack(&self.spec, K_BOX, P::N == 1, C::COUNT, 0.0, 0.0, [0; 2]));
        self.kids.emit_kids(s);
    }
}

// ---- measured leaves ----------------------------------------------------------------------------------------

/// What `Measure::block` answers: the widest line and the height at an inline size, and the half-open
/// interval of inline sizes over which that answer (and the line structure behind it) is the same.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Block {
    pub w: f32,
    pub h: f32,
    pub lo: f32,
    pub hi: f32,
}

/// What layout asks of measured content (text). Device pixels. This is the boundary with the text lane
/// (`docs/r8/LAYOUT-SEAM.md` section 5); the leaves in [`crate::tree::standin`] stand in for it.
pub trait Measure {
    /// The content can break: its block size depends on its inline size.
    const WRAPS: bool;
    /// Min-content and max-content inline size.
    fn inline(&self) -> (f32, f32);
    fn block(&self, w: f32) -> Block;
}

/// A measured leaf: text. One record.
#[derive(Clone, Copy)]
pub struct Leaf<M, P> {
    pub spec: Spec,
    pub m: M,
    pub paint: P,
}
/// Text in flow, measured by `m`, painted by the record `a`.
#[inline(always)]
pub fn text<M, A>(m: M, a: A) -> Leaf<M, Rec1<A>> {
    Leaf { spec: Spec::new(false), m, paint: Rec1(a) }
}
impl<M, P> Leaf<M, P> {
    style_methods!();
    #[inline(always)]
    pub fn with_spec(mut self, spec: Spec) -> Self {
        self.spec = spec;
        self
    }
}

/// The content size of a measured leaf under known content dimensions and available inline space.
#[inline(always)]
pub fn content<M: Measure>(m: &M, cx: &mut Cx, kw: Option<V>, kh: Option<V>, av: Av) -> [V; 2] {
    if !M::WRAPS {
        let (_, mx) = m.inline();
        let w = kw.unwrap_or(V::k(mx));
        let h = match kh {
            Some(h) => h,
            None => V::k(m.block(mx).h),
        };
        return [w, h];
    }
    // the width the lines are broken at
    let at = match kw {
        Some(w) => w,
        None => {
            let (mn, mx) = m.inline();
            match av {
                Av::Max => V::k(mx),
                Av::Min => V::k(mn),
                Av::Def(a) => {
                    if !cx.lt(a, V::k(mx)) {
                        V::k(mx)
                    } else {
                        cx.max(a, V::k(mn))
                    }
                }
            }
        }
    };
    let b = m.block(at.v);
    cx.range(at, b.lo, b.hi);
    [kw.unwrap_or(V::k(b.w)), kh.unwrap_or(V::k(b.h))]
}

/// The border-box size of a measured leaf.
#[inline(always)]
pub fn leaf_size<M: Measure>(spec: &Spec, m: &M, cx: &mut Cx, known: [Option<V>; 2], parent: [Option<V>; 2], avail: [Av; 2], content_only: bool) -> [V; 2] {
    let node = crate::flex::own(spec, cx, known, parent, content_only, [0.0; 2]);
    if let [Some(w), Some(h)] = node {
        return [w, h];
    }
    // (the available space is for the border box: the parent already took the margins off)
    content(m, cx, node[0], node[1], avail[0])
}

/// What a leaf's content is to layout: a line of known size, or content that wraps and is asked.
pub trait LeafM {
    const WRAPS: bool;
    fn emit<'a, S: Sink<'a>>(&'a self, spec: &Spec, painted: bool, s: &mut S);
}
impl LeafM for crate::tree::standin::Line {
    const WRAPS: bool = false;
    #[inline(always)]
    fn emit<'a, S: Sink<'a>>(&'a self, spec: &Spec, painted: bool, s: &mut S) {
        s.node(Nd::pack(spec, K_LINE, painted, 0, self.0, self.1, [0; 2]));
    }
}
impl<W: Wrap> LeafM for W {
    const WRAPS: bool = true;
    #[inline(always)]
    fn emit<'a, S: Sink<'a>>(&'a self, spec: &Spec, painted: bool, s: &mut S) {
        s.node(Nd::pack(spec, K_WRAP, painted, 0, 0.0, 0.0, self.key()));
        s.wrap(self);
    }
}
impl<M: LeafM, P: Paint> Shape for Leaf<M, P> {
    const LEAVES: usize = P::N;
    const WRAPS: bool = M::WRAPS;
    const VARIES: bool = false;
    const HK: Hk = if M::WRAPS { Hk::Wraps } else { Hk::Fixed };
    #[inline(always)]
    fn emit<'a, S: Sink<'a>>(&'a self, s: &mut S) {
        self.m.emit(&self.spec, P::N == 1, s);
    }
}

// ---- children -----------------------------------------------------------------------------------------------

impl ShapeKids for () {
    const LEAVES: usize = 0;
    const WRAPS: bool = false;
    const VARIES: bool = false;
    const COUNT: u32 = 0;
    #[inline(always)]
    fn emit_kids<'a, S: Sink<'a>>(&'a self, _: &mut S) {}
}
impl<L: Shape> ShapeKids for L {
    const LEAVES: usize = L::LEAVES;
    const WRAPS: bool = L::WRAPS;
    const VARIES: bool = L::VARIES;
    const COUNT: u32 = 1;
    #[inline(always)]
    fn emit_kids<'a, S: Sink<'a>>(&'a self, s: &mut S) {
        self.emit(s)
    }
}
/// A child that may be absent. Its records exist either way (a row's type fixes its records); absent, they
/// have empty rects and it takes no part in layout.
impl<L: Shape> ShapeKids for Option<L> {
    const LEAVES: usize = L::LEAVES;
    const WRAPS: bool = L::WRAPS;
    const VARIES: bool = true;
    const COUNT: u32 = 1;
    #[inline(always)]
    fn emit_kids<'a, S: Sink<'a>>(&'a self, s: &mut S) {
        match self {
            Some(l) => l.emit(s),
            None => s.node(Nd::pack(&Spec::new(false), K_GONE, false, L::LEAVES as u32, 0.0, 0.0, [0; 2])),
        }
    }
}
impl<K: ShapeKids, const N: usize> ShapeKids for [K; N] {
    const LEAVES: usize = K::LEAVES * N;
    const WRAPS: bool = K::WRAPS;
    const VARIES: bool = K::VARIES;
    const COUNT: u32 = K::COUNT * N as u32;
    #[inline(always)]
    fn emit_kids<'a, S: Sink<'a>>(&'a self, s: &mut S) {
        for k in self {
            k.emit_kids(s);
        }
    }
}
impl<C: ShapeKids, E: ShapeKids> ShapeKids for Chain<C, E> {
    const LEAVES: usize = C::LEAVES + E::LEAVES;
    const WRAPS: bool = C::WRAPS | E::WRAPS;
    const VARIES: bool = C::VARIES | E::VARIES;
    const COUNT: u32 = C::COUNT + E::COUNT;
    #[inline(always)]
    fn emit_kids<'a, S: Sink<'a>>(&'a self, s: &mut S) {
        self.0.emit_kids(s);
        self.1.emit_kids(s);
    }
}
macro_rules! kids_tuple {
    ($($t:ident $i:tt)+) => {
        impl<$($t: ShapeKids),+> ShapeKids for ($($t,)+) {
            const LEAVES: usize = 0 $(+ $t::LEAVES)+;
            const WRAPS: bool = false $(| $t::WRAPS)+;
            const VARIES: bool = false $(| $t::VARIES)+;
            const COUNT: u32 = 0 $(+ $t::COUNT)+;
            #[inline(always)]
            fn emit_kids<'a, Sv: Sink<'a>>(&'a self, s: &mut Sv) {
                $(self.$i.emit_kids(s);)+
            }
        }
    };
}
kids_tuple!(A 0);
kids_tuple!(A 0 B 1);
kids_tuple!(A 0 B 1 C 2);
kids_tuple!(A 0 B 1 C 2 D 3);
kids_tuple!(A 0 B 1 C 2 D 3 E 4);
kids_tuple!(A 0 B 1 C 2 D 3 E 4 F 5);
kids_tuple!(A 0 B 1 C 2 D 3 E 4 F 5 G 6);
kids_tuple!(A 0 B 1 C 2 D 3 E 4 F 5 G 6 H 7);
kids_tuple!(A 0 B 1 C 2 D 3 E 4 F 5 G 6 H 7 I 8);
kids_tuple!(A 0 B 1 C 2 D 3 E 4 F 5 G 6 H 7 I 8 J 9);
kids_tuple!(A 0 B 1 C 2 D 3 E 4 F 5 G 6 H 7 I 8 J 9 K 10);
kids_tuple!(A 0 B 1 C 2 D 3 E 4 F 5 G 6 H 7 I 8 J 9 K 10 L 11);
