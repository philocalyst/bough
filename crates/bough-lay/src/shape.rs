//! A row's layout as data, and the one function that lays it out.
//!
//! The first form of this crate monomorphised the flex algorithm per element type. Measured, that bought no
//! staging (the compiler did not fold a general algorithm into constants: 25,000 instructions for a sheet row
//! against 200 to copy kept rects) and cost minutes of compile time per row type. So the type carries only
//! what it is good at: turning the builder calls into a flat array of plain words ([`Nd`], one per element, in
//! depth-first order), by code that is a few stores per element.
//!
//! * The **shape** of a row is the hash of those words ([`hash`]). Where the author's values are literals the
//!   words are constants, and so is the hash, at compile time: rows of one call site then have one shape with
//!   no work per row. Where a value varies per row, the hash is computed from it.
//! * A list keeps the rects of the last shape it laid out ([`Memo`]): the same shape at a width inside the
//!   answer's validity interval is a copy.
//! * Otherwise the words are laid out by [`place`]: the flex algorithm of [`crate::flex`], instantiated once,
//!   for a cursor into the array. One body for every row type of a program.
use crate::el::{Block, Measure, leaf_size};
use crate::flex::{Align, Dim, Hk, Item, Justify, Kids, Lay, Out, Pass, Spec, place_kids, rect, solve};
use crate::tree::standin::Line;
use crate::tree::{Kind, Node};
use crate::val::{Av, Cx, V, Valid};
use bough_expr::U;
use core::mem::MaybeUninit;
use std::rc::Rc;

/// Words per element.
pub const NW: usize = 24;
/// One element's layout: its properties, its kind, how many children follow it.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Nd(pub [u32; NW]);

pub const K_BOX: u32 = 0;
pub const K_LINE: u32 = 1;
pub const K_WRAP: u32 = 2;
/// An absent optional child: `count` is the number of records it would have had.
pub const K_GONE: u32 = 3;

#[inline(always)]
const fn dimw(d: Dim) -> (u32, u32) {
    match d {
        Dim::Auto => (0, 0),
        Dim::Pt(v) => (1, v.to_bits()),
        Dim::Pct(v) => (2, v.to_bits()),
    }
}
#[inline(always)]
const fn undim(t: u32, v: u32) -> Dim {
    match t {
        1 => Dim::Pt(f32::from_bits(v)),
        2 => Dim::Pct(f32::from_bits(v)),
        _ => Dim::Auto,
    }
}
const ALIGNS: [Option<Align>; 5] = [None, Some(Align::Start), Some(Align::End), Some(Align::Center), Some(Align::Stretch)];
const JUSTS: [Option<Justify>; 7] = [None, Some(Justify::Start), Some(Justify::End), Some(Justify::Center), Some(Justify::SpaceBetween), Some(Justify::SpaceAround), Some(Justify::SpaceEvenly)];
#[inline(always)]
const fn alw(a: Option<Align>) -> u32 {
    match a {
        None => 0,
        Some(Align::Start) => 1,
        Some(Align::End) => 2,
        Some(Align::Center) => 3,
        Some(Align::Stretch) => 4,
    }
}
#[inline(always)]
const fn juw(j: Option<Justify>) -> u32 {
    match j {
        None => 0,
        Some(Justify::Start) => 1,
        Some(Justify::End) => 2,
        Some(Justify::Center) => 3,
        Some(Justify::SpaceBetween) => 4,
        Some(Justify::SpaceAround) => 5,
        Some(Justify::SpaceEvenly) => 6,
    }
}

impl Nd {
    /// `a`, `b`: a line's width and height in pixels. `key`: what identifies a wrapping leaf's content.
    #[inline(always)]
    pub const fn pack(sp: &Spec, kind: u32, painted: bool, count: u32, a: f32, b: f32, key: [u32; 2]) -> Nd {
        let (wt, wv) = dimw(sp.w);
        let (ht, hv) = dimw(sp.h);
        let (ax, ay, aon) = match sp.abs {
            Some((x, y)) => (x.to_bits(), y.to_bits(), 1),
            None => (0, 0, 0),
        };
        let flags = sp.column as u32 | alw(sp.items) << 1 | alw(sp.self_align) << 4 | juw(sp.justify) << 7 | aon << 10 | kind << 11 | (painted as u32) << 13 | wt << 14 | ht << 16;
        Nd([
            wv,
            hv,
            sp.min[0].to_bits(),
            sp.min[1].to_bits(),
            sp.pad[0].to_bits(),
            sp.pad[1].to_bits(),
            sp.pad[2].to_bits(),
            sp.pad[3].to_bits(),
            sp.margin[0].to_bits(),
            sp.margin[1].to_bits(),
            sp.margin[2].to_bits(),
            sp.margin[3].to_bits(),
            sp.gap.to_bits(),
            sp.grow.to_bits(),
            sp.shrink.to_bits(),
            ax,
            ay,
            flags,
            count,
            a.to_bits(),
            b.to_bits(),
            key[0],
            key[1],
            0,
        ])
    }
    pub fn spec(&self) -> Spec {
        let (w, f) = (&self.0, self.0[17]);
        let fl = f32::from_bits;
        Spec {
            w: undim(f >> 14 & 3, w[0]),
            h: undim(f >> 16 & 3, w[1]),
            min: [fl(w[2]), fl(w[3])],
            pad: [fl(w[4]), fl(w[5]), fl(w[6]), fl(w[7])],
            margin: [fl(w[8]), fl(w[9]), fl(w[10]), fl(w[11])],
            gap: fl(w[12]),
            grow: fl(w[13]),
            shrink: fl(w[14]),
            column: f & 1 != 0,
            items: ALIGNS[(f >> 1 & 7) as usize],
            self_align: ALIGNS[(f >> 4 & 7) as usize],
            justify: JUSTS[(f >> 7 & 7) as usize],
            abs: if f >> 10 & 1 != 0 { Some((fl(w[15]), fl(w[16]))) } else { None },
        }
    }
    #[inline(always)]
    pub fn kind(&self) -> u32 {
        self.0[17] >> 11 & 3
    }
    #[inline(always)]
    pub fn painted(&self) -> bool {
        self.0[17] >> 13 & 1 != 0
    }
    #[inline(always)]
    pub fn count(&self) -> u32 {
        self.0[18]
    }
}

/// Content that wraps: what layout asks of it (the text lane's leaf). Object-safe: one indirect call per
/// question per paragraph, none for anything else.
pub trait Wrap {
    fn inline(&self) -> (f32, f32);
    fn block(&self, w: f32) -> Block;
    /// What identifies the content for layout: equal keys measure equally.
    fn key(&self) -> [u32; 2];
    /// The content kept alive on its own: for the general solver (the oracle and the fallback). Cold.
    fn rc(&self) -> Rc<dyn Wrap>;
}

/// Where an element's words go.
pub trait Sink<'a> {
    fn node(&mut self, nd: Nd);
    #[inline(always)]
    fn wrap(&mut self, _w: &'a dyn Wrap) {}
}

/// An element as layout data.
pub trait Shape {
    /// Records, and so rects, this element emits (itself first, then its children in order).
    const LEAVES: usize;
    const WRAPS: bool;
    const VARIES: bool;
    /// How the block size of a row of this type comes about.
    const HK: Hk;
    fn emit<'a, S: Sink<'a>>(&'a self, s: &mut S);
}
pub trait ShapeKids {
    const LEAVES: usize;
    const WRAPS: bool;
    const VARIES: bool;
    /// Elements emitted at this level.
    const COUNT: u32;
    fn emit_kids<'a, S: Sink<'a>>(&'a self, s: &mut S);
}

struct Hasher(u64);
impl<'a> Sink<'a> for Hasher {
    #[inline(always)]
    fn node(&mut self, nd: Nd) {
        let mut h = self.0;
        let mut i = 0;
        while i < NW - 1 {
            h = (h.rotate_left(5) ^ nd.0[i] as u64).wrapping_mul(0x517c_c1b7_2722_0a95);
            i += 1;
        }
        self.0 = h;
    }
}
/// The shape of an element: a hash of its layout words. A constant where the element's values are literals.
#[inline(always)]
pub fn hash<T: Shape>(t: &T) -> u64 {
    let mut h = Hasher(T::LEAVES as u64 ^ 0x9E37_79B9_7F4A_7C15);
    t.emit(&mut h);
    h.0 | 1
}

/// The most elements in one row, and the most wrapping leaves.
pub const MAXN: usize = 96;
pub const MAXW: usize = 12;

/// A row's elements, decoded for the layout function.
pub struct Flat<'a> {
    n: usize,
    nw: usize,
    spec: [MaybeUninit<Spec>; MAXN],
    meta: [Meta; MAXN],
    wraps: [Option<&'a dyn Wrap>; MAXW],
}
#[derive(Clone, Copy)]
struct Meta {
    kind: u8,
    painted: bool,
    count: u16,
    /// The index after this element's subtree.
    end: u16,
    wrap: u16,
    a: f32,
    b: f32,
}
impl<'a> Sink<'a> for Flat<'a> {
    #[inline]
    fn node(&mut self, nd: Nd) {
        assert!(self.n < MAXN, "a row has at most 96 elements");
        self.spec[self.n] = MaybeUninit::new(nd.spec());
        self.meta[self.n] = Meta { kind: nd.kind() as u8, painted: nd.painted(), count: nd.count() as u16, end: 0, wrap: self.nw as u16, a: f32::from_bits(nd.0[19]), b: f32::from_bits(nd.0[20]) };
        self.n += 1;
    }
    #[inline]
    fn wrap(&mut self, w: &'a dyn Wrap) {
        assert!(self.nw < MAXW, "a row has at most 12 wrapping leaves");
        self.wraps[self.nw] = Some(w);
        self.nw += 1;
    }
}
impl<'a> Flat<'a> {
    pub fn of<T: Shape>(t: &'a T) -> Flat<'a> {
        let mut f = Flat { n: 0, nw: 0, spec: [const { MaybeUninit::uninit() }; MAXN], meta: [Meta { kind: 0, painted: false, count: 0, end: 0, wrap: 0, a: 0.0, b: 0.0 }; MAXN], wraps: [None; MAXW] };
        t.emit(&mut f);
        f.ends(0);
        f
    }
    fn ends(&mut self, i: usize) -> usize {
        let mut j = i + 1;
        if self.meta[i].kind == K_BOX as u8 {
            for _ in 0..self.meta[i].count {
                j = self.ends(j);
            }
        }
        self.meta[i].end = j as u16;
        j
    }
    #[inline(always)]
    pub fn root(&self) -> At<'_, 'a> {
        At { f: self, i: 0 }
    }
}

/// One element of a row: the cursor the flex algorithm is instantiated for.
#[derive(Clone, Copy)]
pub struct At<'f, 'a> {
    f: &'f Flat<'a>,
    i: usize,
}
struct Under<'f, 'a>(At<'f, 'a>);

/// The items of a line: up to [`crate::flex::CAP`], on the stack.
pub struct Items {
    n: usize,
    it: [MaybeUninit<Item>; crate::flex::CAP],
}
impl Kids for Under<'_, '_> {
    type St = Items;
    const LEAVES: usize = 0;
    const WRAPS: bool = true;
    const VARIES: bool = true;
    #[inline(always)]
    fn st(&self) -> Items {
        let n = self.0.f.meta[self.0.i].count as usize;
        assert!(n <= crate::flex::CAP, "a flex container has at most 32 children; a longer run is a list");
        let mut st = Items { n, it: [const { MaybeUninit::uninit() }; crate::flex::CAP] };
        for k in 0..n {
            st.it[k] = MaybeUninit::new(Item::NONE);
        }
        st
    }
    #[inline(always)]
    fn pass<P: Pass>(&self, st: &mut Items, p: &mut P) {
        let mut j = self.0.i + 1;
        for k in 0..st.n {
            // SAFETY: the first `n` items were written by `st`.
            p.kid(&At { f: self.0.f, i: j }, unsafe { st.it[k].assume_init_mut() });
            j = self.0.f.meta[j].end as usize;
        }
    }
    #[inline(always)]
    fn each<F: FnMut(&Item)>(st: &Items, f: &mut F) {
        for k in 0..st.n {
            // SAFETY: as `pass`.
            f(unsafe { st.it[k].assume_init_ref() })
        }
    }
    #[inline(always)]
    fn each_mut<F: FnMut(&mut Item)>(st: &mut Items, f: &mut F) {
        for k in 0..st.n {
            // SAFETY: as `pass`.
            f(unsafe { st.it[k].assume_init_mut() })
        }
    }
    #[inline(always)]
    fn empty(&self) -> bool {
        self.0.f.meta[self.0.i].count == 0
    }
    fn trees(&self, _: &mut Vec<Node>) {}
}

/// A wrapping leaf by reference, as measured content.
struct ByRef<'a>(&'a dyn Wrap);
impl Measure for ByRef<'_> {
    const WRAPS: bool = true;
    #[inline(always)]
    fn inline(&self) -> (f32, f32) {
        self.0.inline()
    }
    #[inline(always)]
    fn block(&self, w: f32) -> Block {
        self.0.block(w)
    }
}
/// The same, kept alive (for the solver's measure function).
#[derive(Clone)]
struct ByRc(Rc<dyn Wrap>);
impl Measure for ByRc {
    const WRAPS: bool = true;
    fn inline(&self) -> (f32, f32) {
        self.0.inline()
    }
    fn block(&self, w: f32) -> Block {
        self.0.block(w)
    }
}

impl Lay for At<'_, '_> {
    const LEAVES: usize = 0;
    const WRAPS: bool = true;
    #[inline(always)]
    fn spec(&self) -> &Spec {
        // SAFETY: `i` is under `n`: an element the sink wrote.
        unsafe { self.f.spec[self.i].assume_init_ref() }
    }
    #[inline(always)]
    fn absent(&self) -> bool {
        self.f.meta[self.i].kind == K_GONE as u8
    }
    #[inline(never)]
    fn size(&self, cx: &mut Cx, known: [Option<V>; 2], parent: [Option<V>; 2], avail: [Av; 2], content_only: bool) -> [V; 2] {
        let m = &self.f.meta[self.i];
        if m.kind == K_BOX as u8 {
            return solve(self.spec(), &Under(*self), cx, known, parent, avail, content_only, false).outer;
        }
        if m.kind == K_LINE as u8 {
            leaf_size(self.spec(), &Line(m.a, m.b), cx, known, parent, avail, content_only)
        } else {
            leaf_size(self.spec(), &ByRef(self.f.wraps[m.wrap as usize].expect("a wrapping leaf")), cx, known, parent, avail, content_only)
        }
    }
    #[inline(always)]
    fn reported(&self, cx: &mut Cx, size: [V; 2]) -> [V; 2] {
        if self.f.meta[self.i].kind == K_BOX as u8 { crate::flex::reported(self.spec(), cx, size) } else { size }
    }
    #[inline(never)]
    fn place<O: Out + ?Sized>(&self, cx: &mut Cx, size: [V; 2], org: [V; 2], out: &mut O, mut k: usize) -> usize {
        let m = &self.f.meta[self.i];
        if m.kind == K_GONE as u8 {
            for j in 0..m.count as usize {
                out.put(k + j, [0; 4]);
            }
            return k + m.count as usize;
        }
        if m.painted {
            out.put(k, rect(cx, size, org));
            k += 1;
        }
        if m.kind == K_BOX as u8 && m.count != 0 {
            let av = [Av::Def(size[0]), Av::Def(size[1])];
            let mut sol = solve(self.spec(), &Under(*self), cx, [Some(size[0]), Some(size[1])], [None; 2], av, false, true);
            k = place_kids(&Under(*self), cx, &mut sol, org, out, k);
        }
        k
    }
    fn tree(&self) -> Node {
        let m = &self.f.meta[self.i];
        let p = m.painted as usize;
        match m.kind as u32 {
            K_GONE => Node::gone(m.count as usize),
            K_LINE => Node { spec: *self.spec(), kind: Kind::Leaf(crate::tree::measurer(Line(m.a, m.b))), kids: vec![], leaves: p, total: p },
            K_WRAP => Node { spec: *self.spec(), kind: Kind::Leaf(crate::tree::measurer(ByRc(self.f.wraps[m.wrap as usize].expect("a wrapping leaf").rc()))), kids: vec![], leaves: p, total: p },
            _ => {
                let mut kids = Vec::new();
                let mut j = self.i + 1;
                for _ in 0..m.count {
                    kids.push(At { f: self.f, i: j }.tree());
                    j = self.f.meta[j].end as usize;
                }
                let total = p + kids.iter().map(|k| k.total).sum::<usize>();
                Node { spec: *self.spec(), kind: Kind::Flex, kids, leaves: p, total }
            }
        }
    }
}

/// Lay a row out: its words, then the one layout function. No memo. Returns the slot's height and the interval
/// of list widths over which every rect and the height are the same.
#[inline(never)]
pub fn place<T: Shape, O: Out + ?Sized>(t: &T, s: f32, u: &U, width: f32, out: &mut O) -> (i32, Valid) {
    let f = Flat::of(t);
    place_flat(&f, s, u, width, out)
}
/// The layout function: one body for every row type.
#[inline(never)]
pub fn place_flat<O: Out + ?Sized>(f: &Flat<'_>, s: f32, u: &U, width: f32, out: &mut O) -> (i32, Valid) {
    crate::place_row(&f.root(), s, u, width, out)
}
/// A row's height and its interval, with no rects.
#[inline(never)]
pub fn size<T: Shape>(t: &T, s: f32, u: &U, width: f32) -> (i32, Valid) {
    let f = Flat::of(t);
    size_flat(&f, s, u, width)
}
#[inline(never)]
pub fn size_flat(f: &Flat<'_>, s: f32, u: &U, width: f32) -> (i32, Valid) {
    crate::size_row(&f.root(), s, u, width)
}
/// The row as a style tree for the general solver.
pub fn tree<T: Shape>(t: &T) -> Node {
    Flat::of(t).root().tree()
}

/// The rects of the last shape a list laid out, with the widths they hold over. A row of the same shape at a
/// width inside that interval is a copy of them.
pub struct Memo {
    hash: u64,
    valid: Valid,
    h: i32,
    rects: Vec<[i32; 4]>,
    pub hits: u64,
    pub misses: u64,
}
impl Default for Memo {
    fn default() -> Self {
        Memo::new()
    }
}
impl Memo {
    pub const fn new() -> Memo {
        Memo { hash: 0, valid: Valid { lo: [1.0; 2], hi: [0.0; 2] }, h: 0, rects: Vec::new(), hits: 0, misses: 0 }
    }
    /// Whether a row of shape `hash` at `width` is the kept answer.
    #[inline(always)]
    pub fn holds(&self, hash: u64, width: f32) -> bool {
        (hash == self.hash) & (width >= self.valid.lo[0]) & (width <= self.valid.hi[0])
    }
    /// The kept answer: one rect per record, relative to the row, and the row's height.
    #[inline(always)]
    pub fn kept(&self) -> (&[[i32; 4]], i32) {
        (&self.rects, self.h)
    }
    /// The interval of widths the kept answer holds over.
    #[inline(always)]
    pub fn valid(&self) -> Valid {
        self.valid
    }
    /// Lay `t` out into the memo.
    #[inline(never)]
    pub fn lay<T: Shape>(&mut self, t: &T, hash: u64, s: f32, u: &U, width: f32) {
        self.rects.clear();
        self.rects.resize(T::LEAVES, [0; 4]);
        let (h, v) = place(t, s, u, width, &mut self.rects[..]);
        (self.hash, self.valid, self.h) = (hash, v, h);
        self.misses += 1;
    }
    /// The rects of row `t` at `at`, into `out`; returns the row's height. The whole of a row's layout on the
    /// path of a list: a compare of the shape, a compare of the width, a copy.
    #[inline(always)]
    pub fn row<T: Shape, O: Out + ?Sized>(&mut self, t: &T, s: f32, u: &U, width: f32, at: (i32, i32), out: &mut O) -> i32 {
        let hash = hash(t);
        if !self.holds(hash, width) {
            self.lay(t, hash, s, u, width);
        } else {
            self.hits += 1;
        }
        for (k, r) in self.rects.iter().enumerate() {
            out.put(k, [r[0] + at.0, r[1] + at.1, r[2] + at.0, r[3] + at.1]);
        }
        self.h
    }
}
