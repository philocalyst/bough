//! Single-line flexbox as a monomorphised function of the element's type.
//!
//! The arithmetic follows CSS Flexible Box Layout as `vendor/cephal` implements it (operation order included,
//! so results agree bit for bit where both apply): flex base sizes, the main size of a content-sized
//! container, resolving flexible lengths, cross sizes, justification and alignment. What is not here is
//! everything a run-time engine needs because it does not know the tree: node ids, a style table, caches of
//! answers, a scheduler. A container's children are a tuple, so each pass over them is straight-line code.
//!
//! Two things keep the code small and shared across row types:
//! * the freeze loop of section 9.7 ([`resolve`]) is one function over a slice of [`Item`], outlined; a line
//!   with no free space to distribute, or with nothing that takes it, never calls it;
//! * the automatic minimum size of an item (the min-content size of its content) is unused unless the item
//!   shrinks, so it is only computed on the way into [`resolve`].
use crate::val::{Av, Cx, V, round};
use core::mem::MaybeUninit;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Dim {
    Auto,
    /// Points.
    Pt(f32),
    /// A fraction of the parent's inner size.
    Pct(f32),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Start,
    End,
    Center,
    Stretch,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Justify {
    Start,
    End,
    Center,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
}

/// A box's layout properties, in points. The façade's `Spec`, as plain values.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spec {
    pub w: Dim,
    pub h: Dim,
    pub min: [f32; 2],
    /// left, top, right, bottom
    pub pad: [f32; 4],
    pub margin: [f32; 4],
    pub gap: f32,
    pub grow: f32,
    pub shrink: f32,
    pub column: bool,
    pub items: Option<Align>,
    pub self_align: Option<Align>,
    pub justify: Option<Justify>,
    /// Out of flow, at this inset from the parent's box.
    pub abs: Option<(f32, f32)>,
}
impl Spec {
    pub const fn new(column: bool) -> Spec {
        Spec { w: Dim::Auto, h: Dim::Auto, min: [0.0; 2], pad: [0.0; 4], margin: [0.0; 4], gap: 0.0, grow: 0.0, shrink: 1.0, column, items: None, self_align: None, justify: None, abs: None }
    }
}

/// Where rects go: the lanes of a page, for the record at position `k` of the row.
pub trait Out {
    fn put(&mut self, k: usize, r: [i32; 4]);
}
impl<const N: usize> Out for [[i32; 4]; N] {
    #[inline(always)]
    fn put(&mut self, k: usize, r: [i32; 4]) {
        self[k] = r;
    }
}
impl Out for [[i32; 4]] {
    #[inline(always)]
    fn put(&mut self, k: usize, r: [i32; 4]) {
        self[k] = r;
    }
}

/// How a row's block size comes about: what an extent may assume without calling the row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hk {
    /// From values the author wrote (a height on the row's root, or sizes of content that neither wraps nor
    /// comes and goes): the same for every row unless the author varies the values.
    Fixed,
    /// From content that cannot wrap but is not always there: independent of the width, not of the row.
    Content,
    /// From content that wraps: a function of the available width.
    Wraps,
}
impl Hk {
    pub const fn join(self, o: Hk) -> Hk {
        match (self, o) {
            (Hk::Wraps, _) | (_, Hk::Wraps) => Hk::Wraps,
            (Hk::Fixed, Hk::Fixed) => Hk::Fixed,
            _ => Hk::Content,
        }
    }
}

/// The layout of an element: a pure function, monomorphised per element type.
pub trait Lay {
    /// Records, and so rects, this element emits (itself first, then its children in order).
    const LEAVES: usize;
    /// Whether content under this element can wrap (its size depends on the width it is given).
    const WRAPS: bool;
    /// The records under this element are not always the same ones in flow (an optional child).
    const VARIES: bool = false;
    /// How the block size of a row of this type comes about.
    const HK: Hk = if Self::WRAPS {
        Hk::Wraps
    } else if Self::VARIES {
        Hk::Content
    } else {
        Hk::Fixed
    };
    fn spec(&self) -> &Spec;
    /// The border-box size under `known` dimensions. `parent` is the parent's inner size (what a percentage
    /// resolves against). With `content`, the element's own size properties are ignored: the caller wants
    /// the size of its content.
    fn size(&self, cx: &mut Cx, known: [Option<V>; 2], parent: [Option<V>; 2], avail: [Av; 2], content: bool) -> [V; 2];
    /// Emit every rect: the element is `size` at `org` (unrounded, relative to the row's origin). `k` is the
    /// position of its first record; returns the position after its last (`k + LEAVES`).
    fn place<O: Out + ?Sized>(&self, cx: &mut Cx, size: [V; 2], org: [V; 2], out: &mut O, k: usize) -> usize;
    /// The size the element reports for itself when laid out at `size`: a container is never narrower than its
    /// padding across its own cross axis, whatever its parent gave it.
    #[inline(always)]
    fn reported(&self, _cx: &mut Cx, size: [V; 2]) -> [V; 2] {
        size
    }
    /// The element is not there (an optional child that is `None`): it takes no part in layout; `place` gives
    /// its records empty rects.
    #[inline(always)]
    fn absent(&self) -> bool {
        false
    }
    /// The element as a style tree: for the general solver (the oracle and the fallback). Cold.
    fn tree(&self) -> crate::tree::Node;
}

/// The children of a container. Their flex items during a run are a value of `St`: a tuple shaped like the
/// children, so each child's item is a local of its own and a pass over the children is straight-line code.
pub trait Kids {
    type St;
    const LEAVES: usize;
    const WRAPS: bool;
    const VARIES: bool;
    fn st(&self) -> Self::St;
    /// One call of `p` per child, in order, with that child's item.
    fn pass<P: Pass>(&self, st: &mut Self::St, p: &mut P);
    /// Every item, in order (what does not need the children's types: sums, offsets).
    fn each<F: FnMut(&Item)>(st: &Self::St, f: &mut F);
    fn each_mut<F: FnMut(&mut Item)>(st: &mut Self::St, f: &mut F);
    /// No children at all: the box is a leaf to the solver.
    #[inline(always)]
    fn empty(&self) -> bool {
        false
    }
    fn trees(&self, out: &mut Vec<crate::tree::Node>);
}
/// A pass over children.
pub trait Pass {
    fn kid<L: Lay>(&mut self, l: &L, it: &mut Item);
    /// A child that is absent (an `Option` that is `None`): it takes no part in layout but its records exist.
    #[inline(always)]
    fn gone<L: Lay>(&mut self, _it: &mut Item) {}
}

/// One flex item during a run.
#[derive(Clone, Copy)]
pub struct Item {
    /// In flow: present and not positioned out of it.
    pub flow: bool,
    pub size: [Option<V>; 2],
    pub min: [f32; 2],
    /// left, top, right, bottom, in pixels
    pub margin: [f32; 4],
    /// The item's own padding, summed per axis.
    pub pb: [f32; 2],
    pub grow: f32,
    pub shrink: f32,
    pub align: Align,
    pub abs: Option<(f32, f32)>,
    /// The cross space for the item's border box: the container's, less the item's margins.
    pub cav: Av,
    /// The cross size the item's content is measured under: its own (within its minimum), or the line's when
    /// it stretches.
    pub kcross: Option<V>,
    pub basis: V,
    pub hyp: V,
    pub target: [V; 2],
    pub off: [V; 2],
    /// The automatic minimum main size; only valid after the min pass.
    pub auto_min: f32,
    frozen: bool,
    violation: f32,
}
impl Item {
    pub const NONE: Item = Item { flow: false, size: [None; 2], min: [0.0; 2], margin: [0.0; 4], pb: [0.0; 2], grow: 0.0, shrink: 0.0, align: Align::Stretch, abs: None, cav: Av::Max, kcross: None, basis: V::k(0.0), hyp: V::k(0.0), target: [V::k(0.0); 2], off: [V::k(0.0); 2], auto_min: 0.0, frozen: false, violation: 0.0 };
}

/// The most children in flow a container may have on the freeze loop's path.
pub const CAP: usize = 32;

#[inline(always)]
fn dim(d: Dim, parent: Option<V>, s: f32) -> Option<V> {
    match d {
        Dim::Auto => None,
        Dim::Pt(v) => Some(V::k(v * s)),
        Dim::Pct(p) => parent.map(|v| v.mulk(p)),
    }
}
#[inline(always)]
fn opt_min(m: f32, s: f32) -> Option<f32> {
    if m > 0.0 { Some(m * s) } else { None }
}
#[inline(always)]
fn vmaxk(cx: &mut Cx, a: V, k: f32) -> V {
    cx.max(a, V::k(k))
}

/// The element's own border-box size from what the parent fixed and (unless `content`) its own properties.
#[inline(always)]
pub fn own(sp: &Spec, cx: &mut Cx, known: [Option<V>; 2], parent: [Option<V>; 2], content: bool, pb: [f32; 2]) -> [Option<V>; 2] {
    let s = cx.s;
    let one = |cx: &mut Cx, a: usize, d: Dim| -> Option<V> {
        let mut o = known[a];
        if o.is_none() {
            if !content {
                o = dim(d, parent[a], s);
            }
            if let (Some(v), Some(m)) = (o, opt_min(sp.min[a], s)) {
                o = Some(vmaxk(cx, v, m));
            }
        }
        match o {
            Some(v) if pb[a] > 0.0 => Some(vmaxk(cx, v, pb[a])),
            o => o,
        }
    };
    [one(cx, 0, sp.w), one(cx, 1, sp.h)]
}

struct Gather<'c, 'a> {
    cx: &'c mut Cx<'a>,
    m: usize,
    inner: [Option<V>; 2],
    cross_av: Av,
    items: Align,
}
impl Pass for Gather<'_, '_> {
    #[inline(always)]
    fn kid<L: Lay>(&mut self, l: &L, it: &mut Item) {
        let (cx, m) = (&mut *self.cx, self.m);
        let (c, s, ks) = (1 - m, cx.s, l.spec());
        let size = [dim(ks.w, self.inner[0], s), dim(ks.h, self.inner[1], s)];
        let margin = [ks.margin[0] * s, ks.margin[1] * s, ks.margin[2] * s, ks.margin[3] * s];
        let pb = [(ks.pad[0] + ks.pad[2]) * s, (ks.pad[1] + ks.pad[3]) * s];
        let align = ks.self_align.unwrap_or(self.items);
        let cav = match self.cross_av {
            Av::Def(v) => {
                let v = vmaxk(cx, v.subk(margin[c] + margin[c + 2]), 0.0);
                Av::Def(if ks.min[c] > 0.0 { vmaxk(cx, v, ks.min[c] * s) } else { v })
            }
            o => o,
        };
        *it = Item { flow: ks.abs.is_none() && !l.absent(), size, min: [ks.min[0] * s, ks.min[1] * s], margin, pb, grow: ks.grow, shrink: ks.shrink, align, abs: ks.abs, cav, ..Item::NONE };
    }
}

/// A column whose own width is unknown: the width its stretched items get is the widest outer width any of its
/// items wants under the available space (the container's fit-content width).
struct Widest<'c, 'a> {
    cx: &'c mut Cx<'a>,
    inner: [Option<V>; 2],
    widest: V,
}
impl Pass for Widest<'_, '_> {
    #[inline(always)]
    fn kid<L: Lay>(&mut self, l: &L, it: &mut Item) {
        if !it.flow {
            return;
        }
        let w = match it.size[0] {
            Some(w) => w,
            None => l.size(self.cx, [None, it.size[1]], [None, self.inner[1]], [it.cav, Av::Max], true)[0],
        };
        let w = if it.min[0] > 0.0 { vmaxk(self.cx, w, it.min[0]) } else { w };
        let w = if it.pb[0] > 0.0 { vmaxk(self.cx, w, it.pb[0]) } else { w };
        self.widest = self.cx.max(self.widest, w.addk(it.margin[0] + it.margin[2]));
    }
}

/// The flex base size and hypothetical main size of every in-flow item.
struct Basis<'c, 'a> {
    cx: &'c mut Cx<'a>,
    m: usize,
    inner: [Option<V>; 2],
    main_min: bool,
    /// The line's cross size where the container's own is unknown (a column's fit-content width).
    stretch: Option<V>,
}
impl Pass for Basis<'_, '_> {
    #[inline(always)]
    fn kid<L: Lay>(&mut self, l: &L, it: &mut Item) {
        let (cx, m) = (&mut *self.cx, self.m);
        let c = 1 - m;
        if !it.flow {
            return;
        }
        it.kcross = match it.size[c] {
            Some(v) if it.min[c] > 0.0 => Some(vmaxk(cx, v, it.min[c])),
            Some(v) => Some(v),
            None if it.align == Align::Stretch => match self.stretch {
                Some(line) => {
                    let v = vmaxk(cx, line.subk(it.margin[c] + it.margin[c + 2]), 0.0);
                    Some(if it.min[c] > 0.0 { vmaxk(cx, v, it.min[c]) } else { v })
                }
                None => it.cav.def(),
            },
            None => None,
        };
        let basis = match it.size[m] {
            Some(b) => b,
            None => {
                let kd = known_for_content(it, m);
                let mut av = [Av::Max; 2];
                av[m] = if self.main_min { Av::Min } else { Av::Max };
                av[c] = it.cav;
                l.size(cx, kd, self.inner, av, true)[m]
            }
        };
        it.basis = if it.pb[m] > 0.0 { vmaxk(cx, basis, it.pb[m]) } else { basis };
        it.hyp = if it.min[m] > 0.0 { vmaxk(cx, it.basis, it.min[m]) } else { it.basis };
        it.target[m] = it.hyp;
    }
}

/// The dimensions a child is measured under for its content sizes: its own cross size, or the line's when it
/// stretches.
#[inline(always)]
fn known_for_content(it: &Item, m: usize) -> [Option<V>; 2] {
    let mut kd = [None; 2];
    kd[1 - m] = it.kcross;
    kd
}

/// The automatic minimum main size of every in-flow item: min-content of its content, capped by its own size.
struct Mins<'c, 'a> {
    cx: &'c mut Cx<'a>,
    m: usize,
    inner: [Option<V>; 2],
}
impl Pass for Mins<'_, '_> {
    #[inline(always)]
    fn kid<L: Lay>(&mut self, l: &L, it: &mut Item) {
        let m = self.m;
        if !it.flow {
            return;
        }
        it.auto_min = if it.min[m] > 0.0 {
            it.min[m]
        } else {
            let kd = known_for_content(it, m);
            let mut av = [Av::Min; 2];
            av[1 - m] = it.cav;
            let mc = l.size(self.cx, kd, self.inner, av, true)[m];
            // a decision on the measured size: which of the two is the minimum
            let mc = match it.size[m] {
                Some(sz) => self.cx.min(mc, sz),
                None => mc,
            };
            mc.v.max(it.pb[m])
        };
    }
}

/// The cross size of every in-flow item that does not take the line's.
struct Cross<'c, 'a> {
    cx: &'c mut Cx<'a>,
    m: usize,
    inner: [Option<V>; 2],
    /// The container's outer main size.
    main: V,
    /// The line's cross size is known: stretched items need no measure.
    line: bool,
    stretch: Option<V>,
}
impl Pass for Cross<'_, '_> {
    #[inline(always)]
    fn kid<L: Lay>(&mut self, l: &L, it: &mut Item) {
        let (m, c) = (self.m, 1 - self.m);
        if !it.flow {
            return;
        }
        let own = match it.size[c] {
            Some(v) => Some(if it.min[c] > 0.0 { vmaxk(self.cx, v, it.min[c]) } else { v }),
            None => None,
        };
        let own = match own {
            Some(v) if it.pb[c] > 0.0 => Some(vmaxk(self.cx, v, it.pb[c])),
            o => o,
        };
        it.target[c] = match own {
            Some(v) => v,
            None if self.line && it.align == Align::Stretch => V::k(0.0),
            None if self.stretch.is_some() && it.align == Align::Stretch => {
                let v = self.stretch.unwrap_or(V::k(0.0)).subk(it.margin[c] + it.margin[c + 2]);
                let v = if it.min[c] > 0.0 { vmaxk(self.cx, v, it.min[c]) } else { v };
                vmaxk(self.cx, v, it.pb[c])
            }
            None => {
                let mut kd = [None; 2];
                kd[m] = Some(it.target[m]);
                let mut av = [Av::Max; 2];
                av[m] = Av::Def(vmaxk(self.cx, self.main.subk(it.margin[m] + it.margin[m + 2]), 0.0));
                av[c] = it.cav;
                let v = l.size(self.cx, kd, self.inner, av, true)[c];
                let v = if it.min[c] > 0.0 { vmaxk(self.cx, v, it.min[c]) } else { v };
                if it.pb[c] > 0.0 { vmaxk(self.cx, v, it.pb[c]) } else { v }
            }
        };
    }
}

struct Place<'c, 'a, 'o, O: ?Sized> {
    cx: &'c mut Cx<'a>,
    out: &'o mut O,
    k: usize,
    org: [V; 2],
    /// The container's outer size (for children out of flow).
    outer: [V; 2],
    m: usize,
    /// Where the next item in flow starts on the main axis, from the container's origin.
    run: V,
}
impl<O: Out + ?Sized> Pass for Place<'_, '_, '_, O> {
    #[inline(always)]
    fn kid<L: Lay>(&mut self, l: &L, it: &mut Item) {
        let k = self.k;
        if l.absent() {
            self.k = l.place(self.cx, [V::k(0.0); 2], [V::k(0.0); 2], self.out, k);
            return;
        }
        self.k = match it.abs {
            None => {
                // (the solver advances by the size the item reports, and gives it that size)
                let m = self.m;
                let size = l.reported(self.cx, it.target);
                let mut off = it.off;
                off[m] = self.run.add(it.off[m]).addk(it.margin[m]);
                self.run = self.run.add(it.off[m].addk(it.margin[m] + it.margin[m + 2]).add(size[m]));
                l.place(self.cx, size, [self.org[0].add(off[0]), self.org[1].add(off[1])], self.out, k)
            }
            Some((x, y)) => {
                // out of flow: sized by its own properties against the container's box, placed at its inset
                let s = self.cx.s;
                let at = [x * s + it.margin[0], y * s + it.margin[1]];
                let av = [Av::Def(self.outer[0].subk(at[0])), Av::Def(self.outer[1].subk(at[1]))];
                let size = l.size(self.cx, [None; 2], [Some(self.outer[0]), Some(self.outer[1])], av, false);
                let size = l.reported(self.cx, size);
                l.place(self.cx, size, [self.org[0].addk(at[0]), self.org[1].addk(at[1])], self.out, k)
            }
        };
    }
    #[inline(always)]
    fn gone<L: Lay>(&mut self, _: &mut Item) {
        for j in 0..L::LEAVES {
            self.out.put(self.k + j, [0; 4]);
        }
        self.k += L::LEAVES;
    }
}

/// The min-content width of a row container: the same container under min-content space.
#[inline(never)]
fn min_content_width<K: Kids>(sp: &Spec, kids: &K, cx: &mut Cx, known: [Option<V>; 2], parent: [Option<V>; 2], avail: [Av; 2], content: bool) -> V {
    solve(sp, kids, cx, [None, known[1]], parent, [Av::Min, avail[1]], content, false).outer[0]
}

/// The freeze loop for a container's items: they are copied to a slice for it and back. Off the path of a
/// line that has nothing to distribute.
#[inline(never)]
fn flex<K: Kids>(st: &mut K::St, m: usize, inner: V, gaps: f32, growing: bool) {
    let mut buf = [const { MaybeUninit::<Item>::uninit() }; CAP];
    let mut n = 0;
    K::each(st, &mut |it: &Item| {
        assert!(n < CAP, "a flex container has at most 32 children; a longer run is a list");
        buf[n] = MaybeUninit::new(*it);
        n += 1;
    });
    // SAFETY: the first `n` entries were written just above.
    let items = unsafe { core::slice::from_raw_parts_mut(buf.as_mut_ptr() as *mut Item, n) };
    resolve(items, m, inner, gaps, growing);
    let mut i = 0;
    K::each_mut(st, &mut |it: &mut Item| {
        *it = items[i];
        i += 1;
    });
}

/// What a solved container hands back: its size and its items.
pub struct Solved<St> {
    pub outer: [V; 2],
    pub inner: [Option<V>; 2],
    /// Where the first item starts on the main axis.
    pub start: V,
    pub m: usize,
    pub st: St,
}

/// One flex container: sizes its items and, with `full`, their offsets.
#[inline(always)]
pub fn solve<K: Kids>(sp: &Spec, kids: &K, cx: &mut Cx, known: [Option<V>; 2], parent: [Option<V>; 2], avail: [Av; 2], content: bool, full: bool) -> Solved<K::St> {
    let s = cx.s;
    let (m, c) = (sp.column as usize, 1 - sp.column as usize);
    let pad = [sp.pad[0] * s, sp.pad[1] * s, sp.pad[2] * s, sp.pad[3] * s];
    let pb = [pad[0] + pad[2], pad[1] + pad[3]];
    let gap = sp.gap * s;
    let leaf = kids.empty();
    let mut outer = own(sp, cx, known, parent, content, pb);
    let mut inner = [outer[0].map(|v| v.subk(pb[0])), outer[1].map(|v| v.subk(pb[1]))];
    let av = [
        match inner[0] {
            Some(v) => Av::Def(v),
            None => avail[0].subk(pb[0]),
        },
        match inner[1] {
            Some(v) => Av::Def(v),
            None => avail[1].subk(pb[1]),
        },
    ];
    let cross_av = av[c];
    let items_align = sp.items.unwrap_or(Align::Stretch);
    let mut st = kids.st();
    kids.pass(&mut st, &mut Gather { cx, m, inner, cross_av, items: items_align });
    // A column of unknown width with a stretched item: the width that item will get.
    let stretch = if m == 1 && inner[0].is_none() {
        let mut any = false;
        K::each(&st, &mut |i: &Item| any |= i.flow && i.size[0].is_none() && i.align == Align::Stretch);
        if any {
            let mut w = Widest { cx, inner, widest: V::k(0.0) };
            kids.pass(&mut st, &mut w);
            let widest = w.widest;
            let widest = match opt_min(sp.min[0], s) {
                Some(mn) => vmaxk(cx, widest, mn - pb[0]),
                None => widest,
            };
            Some(vmaxk(cx, widest, 0.0))
        } else {
            None
        }
    } else {
        None
    };
    kids.pass(&mut st, &mut Basis { cx, m, inner, main_min: av[m] == Av::Min, stretch });
    let mut flow = 0usize;
    K::each(&st, &mut |i: &Item| flow += i.flow as usize);
    let gaps = if flow == 0 { 0.0 } else { gap * (flow - 1) as f32 };

    // The main size of a content-sized container.
    if inner[m].is_none() {
        let mut sum = V::k(-0.0);
        let definite = av[m].def();
        K::each(&st, &mut |it: &Item| {
            if !it.flow {
                return;
            }
            let msum = it.margin[m] + it.margin[m + 2];
            sum = sum.add(match definite {
                Some(_) => it.hyp.addk(msum),
                // under min- or max-content space an item contributes its content size within its limits,
                // as the solver computes it (a flex fraction of the difference to its basis)
                None => {
                    let contribution = match it.size[m] {
                        Some(pref) if m == 0 && it.grow != 0.0 => {
                            let v = vmaxk(cx, pref, it.pb[m]).addk(msum);
                            if it.min[m] > 0.0 { vmaxk(cx, v, it.min[m]) } else { v }
                        }
                        _ => it.hyp.addk(msum),
                    };
                    let diff = contribution.v - it.basis.v;
                    let part = if diff > 0.0 {
                        let g = it.grow.max(1.0);
                        g * (diff / g)
                    } else if diff < 0.0 {
                        let sc = it.shrink.max(1.0) * (it.basis.v - it.pb[m]);
                        if sc == 0.0 { 0.0 } else { sc * (diff / sc) }
                    } else {
                        0.0
                    };
                    V { v: it.basis.v + part, d: contribution.d | it.basis.d }
                }
            });
        });
        let content_main = vmaxk(cx, sum.addk(gaps), 0.0);
        let mut o = content_main.addk(pb[m]);
        // A row of unknown width in definite space shrinks to fit: no wider than the space, unless its
        // min-content width is.
        if let (Some(space), true) = (definite, m == 0) {
            if cx.lt(space, content_main) {
                let mc = min_content_width(sp, kids, cx, known, parent, avail, content);
                let fit = cx.max(space.addk(pb[m]), mc);
                let fit = match opt_min(sp.min[m], s) {
                    Some(mn) if !(content && leaf) => vmaxk(cx, fit, mn),
                    _ => fit,
                };
                let fit = vmaxk(cx, fit, pb[m]);
                o = cx.min(o, fit);
            }
        }
        if let Some(mn) = opt_min(sp.min[m], s) {
            // (the solver clamps a container's content size by its minimum, a childless box's not)
            if !(content && leaf) {
                o = vmaxk(cx, o, mn);
            }
        }
        outer[m] = Some(o);
        inner[m] = Some(o.subk(pb[m]));
    }
    let im = inner[m].unwrap_or(V::k(0.0));
    {
        // the gaps plus every item's hypothetical outer main size, summed as the solver sums them
        let mut sum = V::k(-0.0);
        let mut grow = 0.0f32;
        K::each(&st, &mut |it: &Item| {
            if it.flow {
                sum = sum.add(it.hyp.addk(it.margin[m] + it.margin[m + 2]));
                grow += it.grow;
            }
        });
        let used = V::k(gaps).add(sum);
        let growing = cx.lt(used, im);
        let shrinking = !growing && cx.lt(im, used);
        if growing {
            if grow > 0.0 {
                if grow < 1.0 {
                    // flex factors that sum to less than one take only part of the free space, less the
                    // gaps: a target can come out under its basis, so the content minimums apply
                    kids.pass(&mut st, &mut Mins { cx, m, inner });
                } else {
                    // style minimums only: an item that grows cannot violate its content minimum
                    K::each_mut(&mut st, &mut |it: &mut Item| it.auto_min = it.min[m].max(it.pb[m]));
                }
                flex::<K>(&mut st, m, im, gaps, true);
            }
        } else if shrinking {
            kids.pass(&mut st, &mut Mins { cx, m, inner });
            flex::<K>(&mut st, m, im, gaps, false);
        }
    }

    // Cross sizes. A size-only question about the main axis of a container whose cross size is known stops here.
    if !full && outer[c].is_some() {
        return Solved { outer: [outer[0].unwrap_or(V::k(0.0)), outer[1].unwrap_or(V::k(0.0))], inner, start: V::k(pad[m]), m, st };
    }
    let main_outer = outer[m].unwrap_or(V::k(0.0));
    kids.pass(&mut st, &mut Cross { cx, m, inner, main: main_outer, line: inner[c].is_some(), stretch });
    let line = match inner[c] {
        Some(v) => v,
        None => {
            let mut mx = V::k(0.0);
            K::each(&st, &mut |it: &Item| {
                if it.flow {
                    mx = cx.max(mx, it.target[c].addk(it.margin[c] + it.margin[c + 2]));
                }
            });
            mx
        }
    };
    if outer[c].is_none() {
        let mut o = line.addk(pb[c]);
        if let Some(mn) = opt_min(sp.min[c], s) {
            if !(content && leaf) {
                o = vmaxk(cx, o, mn);
            }
        }
        outer[c] = Some(o);
        inner[c] = Some(o.subk(pb[c]));
    }
    let outer = [outer[0].unwrap_or(V::k(0.0)), outer[1].unwrap_or(V::k(0.0))];
    if !full {
        return Solved { outer, inner, start: V::k(pad[m]), m, st };
    }

    // Stretch, justify, align: the offsets of every item from the container's origin.
    let (first, between): (V, V) = match sp.justify {
        None | Some(Justify::Start) => (V::k(0.0), V::k(gap)),
        Some(j) => {
            let mut sum = V::k(-0.0);
            K::each(&st, &mut |it: &Item| {
                if it.flow {
                    sum = sum.add(it.target[m].addk(it.margin[m] + it.margin[m + 2]));
                }
            });
            let free = im.sub(V::k(gaps).add(sum));
            // free space decides the fallback: a decision
            let pos = cx.lt(V::k(0.0), free);
            let j = match j {
                // a distributed alignment with nothing to distribute falls back: to the start when there is
                // no free space, to its single-item form otherwise
                Justify::SpaceBetween | Justify::SpaceAround | Justify::SpaceEvenly if !pos => Justify::Start,
                Justify::SpaceBetween if flow == 1 => Justify::Start,
                Justify::SpaceAround | Justify::SpaceEvenly if flow == 1 => Justify::Center,
                j => j,
            };
            let n = flow as f32;
            match j {
                Justify::Start => (V::k(0.0), V::k(gap)),
                Justify::End => (free, V::k(gap)),
                Justify::Center => (free.mulk(0.5), V::k(gap)),
                Justify::SpaceBetween => (V::k(0.0), V { v: gap + free.v / (n - 1.0), d: free.blur().d }),
                Justify::SpaceAround => (V { v: (free.v / n) / 2.0, d: free.blur().d }, V { v: gap + free.v / n, d: free.blur().d }),
                Justify::SpaceEvenly => (V { v: free.v / (n + 1.0), d: free.blur().d }, V { v: gap + free.v / (n + 1.0), d: free.blur().d }),
            }
        }
    };
    let mut seen = false;
    K::each_mut(&mut st, &mut |it: &mut Item| {
        if !it.flow {
            return;
        }
        // cross
        if it.align == Align::Stretch && it.size[c].is_none() {
            let v = line.subk(it.margin[c] + it.margin[c + 2]);
            let v = if it.min[c] > 0.0 { vmaxk(cx, v, it.min[c]) } else { v };
            // (never negative: a line narrower than the item's margins leaves it empty)
            it.target[c] = vmaxk(cx, v, 0.0);
        }
        let oc = match it.align {
            Align::Start | Align::Stretch => V::k(0.0),
            Align::End => line.sub(it.target[c].addk(it.margin[c] + it.margin[c + 2])),
            Align::Center => line.sub(it.target[c].addk(it.margin[c] + it.margin[c + 2])).mulk(0.5),
        };
        it.off[c] = V::k(pad[c]).add(oc).addk(it.margin[c]);
        // main
        let om = if seen { between } else { first };
        seen = true;
        #[cfg(feature = "mutant_gap")]
        let om = if om.v == gap && gap != 0.0 { V::k(0.0) } else { om };
        // (the item's offset before it on the main axis; `Place` accumulates the positions)
        it.off[m] = om;
    });
    Solved { outer, inner, start: V::k(pad[m]), m, st }
}

/// CSS Flexible Box Layout 9.7, resolving flexible lengths: one body for every container.
#[inline(never)]
pub fn resolve(items: &mut [Item], m: usize, inner: V, gaps: f32, growing: bool) {
    let shrinking = !growing;
    let im = inner.v;
    let msum = |it: &Item| it.margin[m] + it.margin[m + 2];
    struct G {
        used: f32,
        unfrozen: bool,
        grow: f32,
        shrink: f32,
        scaled: f32,
    }
    let gather = |items: &[Item]| {
        let mut g = G { used: -0.0, unfrozen: false, grow: 0.0, shrink: 0.0, scaled: -0.0 };
        for it in items.iter().filter(|i| i.flow) {
            if it.frozen {
                g.used += it.target[m].v + msum(it);
            } else {
                g.used += it.basis.v + msum(it);
                g.unfrozen = true;
                g.grow += it.grow;
                g.shrink += it.shrink;
                g.scaled += (it.basis.v - it.pb[m]) * it.shrink;
            }
        }
        g
    };
    let mut flexed = 0u32;
    for it in items.iter_mut().filter(|i| i.flow) {
        it.target[m] = it.hyp;
        it.frozen = (it.grow == 0.0 && it.shrink == 0.0) || (growing && it.basis.v > it.hyp.v) || (shrinking && it.basis.v < it.hyp.v);
    }
    let mut g = gather(items);
    let initial_free = im - (gaps + g.used);
    while g.unfrozen {
        let used = gaps + g.used;
        let free = if growing && g.grow < 1.0 {
            (initial_free * g.grow - gaps).min(im - used)
        } else if shrinking && g.shrink < 1.0 {
            (initial_free * g.shrink - gaps).max(im - used)
        } else {
            im - used
        };
        #[derive(Clone, Copy)]
        enum D {
            Nothing,
            Grow(f32),
            Shrink(f32),
        }
        let d = if !free.is_normal() {
            D::Nothing
        } else if growing && g.grow > 0.0 {
            D::Grow(g.grow)
        } else if shrinking && g.shrink > 0.0 && g.scaled > 0.0 {
            D::Shrink(g.scaled)
        } else {
            D::Nothing
        };
        let mut total = 0.0f32;
        for it in items.iter_mut().filter(|i| i.flow && !i.frozen) {
            match d {
                D::Nothing => {}
                D::Grow(sum) => {
                    it.target[m].v = it.basis.v + free * (it.grow / sum);
                    flexed += (it.grow > 0.0) as u32;
                }
                D::Shrink(sum) => {
                    it.target[m].v = it.basis.v + free * ((it.basis.v - it.pb[m]) * it.shrink / sum);
                    flexed += 2;
                }
            }
            let clamped = it.target[m].v.max(it.auto_min).max(0.0);
            it.violation = clamped - it.target[m].v;
            it.target[m].v = clamped;
            total += it.violation;
        }
        for it in items.iter_mut().filter(|i| i.flow && !i.frozen) {
            it.frozen = if total > 0.0 {
                it.violation > 0.0
            } else if total < 0.0 {
                it.violation < 0.0
            } else {
                true
            };
        }
        g = gather(items);
    }
    // How the flexed sizes depend on the available space. One item that takes all of the free space follows
    // the container's size exactly; anything else is some other function of it.
    let sole = growing && flexed == 1;
    for it in items.iter_mut().filter(|i| i.flow) {
        let moved = it.target[m].v != it.hyp.v || (growing && it.grow > 0.0) || (shrinking && it.shrink > 0.0);
        if moved {
            let grows_all = sole && it.grow >= 1.0 && it.violation == 0.0;
            it.target[m].d = if grows_all { it.basis.d | inner.d } else { it.basis.blur().d | inner.blur().d };
        }
    }
}

/// The rect of a box of `size` at `org`: four rounded absolute edges.
#[inline(always)]
pub fn rect(cx: &mut Cx, size: [V; 2], org: [V; 2]) -> [i32; 4] {
    cx.emitted(size[0].d | size[1].d | org[0].d | org[1].d);
    [round(org[0].v), round(org[1].v), round(org[0].v + size[0].v), round(org[1].v + size[1].v)]
}

/// Place the children of a solved container.
#[inline(always)]
pub fn place_kids<K: Kids, O: Out + ?Sized>(kids: &K, cx: &mut Cx, sol: &mut Solved<K::St>, org: [V; 2], out: &mut O, k: usize) -> usize {
    let mut p = Place { cx, out, k, org, outer: sol.outer, m: sol.m, run: sol.start };
    kids.pass(&mut sol.st, &mut p);
    p.k
}

/// A container laid out at `size` reports at least its padding across its cross axis.
#[inline(always)]
pub fn reported(sp: &Spec, cx: &mut Cx, mut size: [V; 2]) -> [V; 2] {
    let c = 1 - sp.column as usize;
    let pb = (sp.pad[c] + sp.pad[c + 2]) * cx.s;
    if pb > 0.0 {
        size[c] = cx.max(size[c], V::k(pb));
    }
    size
}

/// One element is one child.
impl<L: Lay> Kids for L {
    type St = Item;
    const LEAVES: usize = L::LEAVES;
    const WRAPS: bool = L::WRAPS;
    const VARIES: bool = L::VARIES;
    #[inline(always)]
    fn st(&self) -> Item {
        Item::NONE
    }
    #[inline(always)]
    fn pass<P: Pass>(&self, st: &mut Item, p: &mut P) {
        p.kid(self, st)
    }
    #[inline(always)]
    fn each<F: FnMut(&Item)>(st: &Item, f: &mut F) {
        f(st)
    }
    #[inline(always)]
    fn each_mut<F: FnMut(&mut Item)>(st: &mut Item, f: &mut F) {
        f(st)
    }
    fn trees(&self, out: &mut Vec<crate::tree::Node>) {
        out.push(self.tree())
    }
}
