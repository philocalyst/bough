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
    /// Set by the author on the row's root: the same for every row unless the author varies it.
    Fixed,
    /// From content that cannot wrap: independent of the available width.
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
    /// How the block size of a row of this type comes about.
    const HK: Hk = if Self::WRAPS { Hk::Wraps } else { Hk::Content };
    fn spec(&self) -> &Spec;
    /// The border-box size under `known` dimensions. `parent` is the parent's inner size (what a percentage
    /// resolves against). With `content`, the element's own size properties are ignored: the caller wants
    /// the size of its content.
    fn size(&self, cx: &mut Cx, known: [Option<V>; 2], parent: [Option<V>; 2], avail: [Av; 2], content: bool) -> [V; 2];
    /// Emit every rect: the element is `size` at `org` (unrounded, relative to the row's origin). `k` is the
    /// position of its first record; returns the position after its last (`k + LEAVES`).
    fn place<O: Out + ?Sized>(&self, cx: &mut Cx, size: [V; 2], org: [V; 2], out: &mut O, k: usize) -> usize;
    /// The element as a style tree: for the general solver (the oracle and the fallback). Cold.
    fn tree(&self) -> crate::tree::Node;
}

/// The children of a container.
pub trait Kids {
    const LEAVES: usize;
    const WRAPS: bool;
    fn visit<F: Visit>(&self, f: &mut F);
    fn trees(&self, out: &mut Vec<crate::tree::Node>);
}
/// A pass over children: one call per child that is present, in order.
pub trait Visit {
    fn kid<L: Lay>(&mut self, l: &L);
    /// A child that is absent (an `Option` that is `None`): it takes no part in layout but its records exist.
    #[inline(always)]
    fn gone<L: Lay>(&mut self) {}
}

/// One flex item during a run.
#[derive(Clone, Copy)]
pub struct Item {
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
    pub basis: V,
    pub hyp: V,
    pub target: [V; 2],
    pub off: [V; 2],
    /// The automatic minimum main size; only valid after the min pass.
    pub auto_min: f32,
    frozen: bool,
    violation: f32,
}

pub const CAP: usize = 24;
/// The items of one line, on the stack.
pub struct Buf {
    n: usize,
    it: [MaybeUninit<Item>; CAP],
}
impl Buf {
    #[inline(always)]
    pub fn new() -> Buf {
        Buf { n: 0, it: [const { MaybeUninit::uninit() }; CAP] }
    }
    #[inline(always)]
    fn push(&mut self, i: Item) {
        assert!(self.n < CAP, "a flex container has at most 24 children in flow; a longer run is a list");
        self.it[self.n] = MaybeUninit::new(i);
        self.n += 1;
    }
    #[inline(always)]
    pub fn items(&mut self) -> &mut [Item] {
        // SAFETY: the first `n` entries were written by `push`.
        unsafe { core::slice::from_raw_parts_mut(self.it.as_mut_ptr() as *mut Item, self.n) }
    }
}
impl Default for Buf {
    fn default() -> Self {
        Buf::new()
    }
}

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
    let mut out = known;
    for a in 0..2 {
        if out[a].is_none() && !content {
            let d = if a == 0 { sp.w } else { sp.h };
            out[a] = dim(d, parent[a], s);
            if let (Some(v), Some(m)) = (out[a], opt_min(sp.min[a], s)) {
                out[a] = Some(vmaxk(cx, v, m));
            }
        }
        if let Some(v) = out[a] {
            if pb[a] > 0.0 {
                out[a] = Some(vmaxk(cx, v, pb[a]));
            }
        }
    }
    out
}

struct Gather<'c, 'a, 'b> {
    cx: &'c mut Cx<'a>,
    buf: &'b mut Buf,
    m: usize,
    inner: [Option<V>; 2],
    cross_av: Av,
    main_min: bool,
    items: Align,
}
impl Visit for Gather<'_, '_, '_> {
    #[inline(always)]
    fn kid<L: Lay>(&mut self, l: &L) {
        let (cx, m) = (&mut *self.cx, self.m);
        let (c, s, ks) = (1 - m, cx.s, l.spec());
        let size = [dim(ks.w, self.inner[0], s), dim(ks.h, self.inner[1], s)];
        let margin = [ks.margin[0] * s, ks.margin[1] * s, ks.margin[2] * s, ks.margin[3] * s];
        let pb = [(ks.pad[0] + ks.pad[2]) * s, (ks.pad[1] + ks.pad[3]) * s];
        let align = ks.self_align.unwrap_or(self.items);
        let mut it = Item { size, min: [ks.min[0] * s, ks.min[1] * s], margin, pb, grow: ks.grow, shrink: ks.shrink, align, abs: ks.abs, basis: V::k(0.0), hyp: V::k(0.0), target: [V::k(0.0); 2], off: [V::k(0.0); 2], auto_min: 0.0, frozen: false, violation: 0.0 };
        if ks.abs.is_none() {
            let basis = match size[m] {
                Some(b) => b,
                None => {
                    let kd = known_for_content(&it, m, self.cross_av);
                    let mut av = [Av::Max; 2];
                    av[m] = if self.main_min { Av::Min } else { Av::Max };
                    av[c] = self.cross_av;
                    l.size(cx, kd, self.inner, av, true)[m]
                }
            };
            it.basis = if pb[m] > 0.0 { vmaxk(cx, basis, pb[m]) } else { basis };
            it.hyp = if it.min[m] > 0.0 { vmaxk(cx, it.basis, it.min[m]) } else { it.basis };
            it.target[m] = it.hyp;
        }
        self.buf.push(it);
    }
}

/// The dimensions a child is measured under for its content sizes: its own cross size, or the line's when it
/// stretches.
#[inline(always)]
fn known_for_content(it: &Item, m: usize, cross_av: Av) -> [Option<V>; 2] {
    let c = 1 - m;
    let mut kd = it.size;
    kd[m] = None;
    if it.align == Align::Stretch && kd[c].is_none() {
        if let Av::Def(a) = cross_av {
            kd[c] = Some(a.subk(it.margin[c] + it.margin[c + 2]));
        }
    }
    kd
}

/// The automatic minimum main size of every in-flow item: min-content of its content, capped by its own size.
struct Mins<'c, 'a, 'b> {
    cx: &'c mut Cx<'a>,
    items: &'b mut [Item],
    i: usize,
    m: usize,
    inner: [Option<V>; 2],
    cross_av: Av,
}
impl Visit for Mins<'_, '_, '_> {
    #[inline(always)]
    fn kid<L: Lay>(&mut self, l: &L) {
        let (m, it) = (self.m, &mut self.items[self.i]);
        self.i += 1;
        if it.abs.is_some() {
            return;
        }
        it.auto_min = if it.min[m] > 0.0 {
            it.min[m]
        } else {
            let kd = known_for_content(it, m, self.cross_av);
            let mut av = [Av::Min; 2];
            av[1 - m] = self.cross_av;
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
struct Cross<'c, 'a, 'b> {
    cx: &'c mut Cx<'a>,
    items: &'b mut [Item],
    i: usize,
    m: usize,
    inner: [Option<V>; 2],
    /// The container's main size and the cross space available to children.
    av: [Av; 2],
    /// The line's cross size is known: stretched items need no measure.
    line: bool,
}
impl Visit for Cross<'_, '_, '_> {
    #[inline(always)]
    fn kid<L: Lay>(&mut self, l: &L) {
        let (m, c) = (self.m, 1 - self.m);
        let it = &mut self.items[self.i];
        self.i += 1;
        if it.abs.is_some() {
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
            None => {
                let mut kd = [None; 2];
                kd[m] = Some(it.target[m]);
                let v = l.size(self.cx, kd, self.inner, self.av, true)[c];
                let v = if it.min[c] > 0.0 { vmaxk(self.cx, v, it.min[c]) } else { v };
                if it.pb[c] > 0.0 { vmaxk(self.cx, v, it.pb[c]) } else { v }
            }
        };
    }
}

/// A min-content row: each item that sizes to content contributes its min-content size.
struct MinRow<'c, 'a, 'b> {
    cx: &'c mut Cx<'a>,
    items: &'b mut [Item],
    i: usize,
    inner: [Option<V>; 2],
    cross_av: Av,
}
impl Visit for MinRow<'_, '_, '_> {
    #[inline(always)]
    fn kid<L: Lay>(&mut self, l: &L) {
        let it = &mut self.items[self.i];
        self.i += 1;
        if it.abs.is_some() || it.size[0].is_some() {
            return;
        }
        let mc = l.size(self.cx, [None; 2], self.inner, [Av::Min, self.cross_av], false)[0];
        let mc = if it.min[0] > 0.0 { vmaxk(self.cx, mc, it.min[0]) } else { mc };
        it.hyp = mc;
        it.target[0] = mc;
    }
}

struct Place<'c, 'a, 'b, 'o, O: ?Sized> {
    cx: &'c mut Cx<'a>,
    items: &'b [Item],
    i: usize,
    out: &'o mut O,
    k: usize,
    org: [V; 2],
    /// The container's inner size and outer size (for children out of flow).
    inner: [Option<V>; 2],
    outer: [V; 2],
}
impl<O: Out + ?Sized> Visit for Place<'_, '_, '_, '_, O> {
    #[inline(always)]
    fn kid<L: Lay>(&mut self, l: &L) {
        let it = &self.items[self.i];
        self.i += 1;
        let k = self.k;
        self.k = match it.abs {
            None => l.place(self.cx, it.target, [self.org[0].add(it.off[0]), self.org[1].add(it.off[1])], self.out, k),
            Some((x, y)) => {
                // out of flow: sized by its own properties against the container's box, placed at its inset
                let s = self.cx.s;
                let at = [x * s + it.margin[0], y * s + it.margin[1]];
                let av = [Av::Def(self.outer[0].subk(at[0])), Av::Def(self.outer[1].subk(at[1]))];
                let size = l.size(self.cx, [None; 2], [Some(self.outer[0]), Some(self.outer[1])], av, false);
                l.place(self.cx, size, [self.org[0].addk(at[0]), self.org[1].addk(at[1])], self.out, k)
            }
        };
    }
    #[inline(always)]
    fn gone<L: Lay>(&mut self) {
        for j in 0..L::LEAVES {
            self.out.put(self.k + j, [0; 4]);
        }
        self.k += L::LEAVES;
    }
}

/// What a solved container hands back.
pub struct Solved {
    pub outer: [V; 2],
    pub inner: [Option<V>; 2],
}

/// One flex container: sizes its items and, with `full`, their offsets. `buf` holds the items afterwards.
#[inline(always)]
pub fn solve<K: Kids>(sp: &Spec, kids: &K, cx: &mut Cx, known: [Option<V>; 2], parent: [Option<V>; 2], avail: [Av; 2], content: bool, buf: &mut Buf, full: bool) -> Solved {
    let s = cx.s;
    let (m, c) = (sp.column as usize, 1 - sp.column as usize);
    let pad = [sp.pad[0] * s, sp.pad[1] * s, sp.pad[2] * s, sp.pad[3] * s];
    let pb = [pad[0] + pad[2], pad[1] + pad[3]];
    let gap = sp.gap * s;
    let mar = [(sp.margin[0] + sp.margin[2]) * s, (sp.margin[1] + sp.margin[3]) * s];
    let mut outer = own(sp, cx, known, parent, content, pb);
    let mut inner = [outer[0].map(|v| v.subk(pb[0])), outer[1].map(|v| v.subk(pb[1]))];
    let av = [
        match inner[0] {
            Some(v) => Av::Def(v),
            None => avail[0].subk(mar[0]).subk(pb[0]),
        },
        match inner[1] {
            Some(v) => Av::Def(v),
            None => avail[1].subk(mar[1]).subk(pb[1]),
        },
    ];
    let cross_av = av[c];
    let items_align = sp.items.unwrap_or(Align::Stretch);
    kids.visit(&mut Gather { cx, buf, m, inner, cross_av, main_min: av[m] == Av::Min, items: items_align });
    let items = buf.items();
    let flow = items.iter().filter(|i| i.abs.is_none()).count();
    let gaps = if flow == 0 { 0.0 } else { gap * (flow - 1) as f32 };

    // The main size of a content-sized container.
    if inner[m].is_none() {
        if av[m] == Av::Min && m == 0 {
            kids.visit(&mut MinRow { cx, items, i: 0, inner, cross_av });
        }
        let used = sum_outer(items, m, gaps);
        let mut o = used.addk(pb[m]);
        if let Some(mn) = opt_min(sp.min[m], s) {
            if !content {
                o = vmaxk(cx, o, mn);
            }
        }
        outer[m] = Some(o);
        inner[m] = Some(o.subk(pb[m]));
        // (targets are the hypothetical sizes: the container was sized to hold them)
    } else {
        let im = inner[m].unwrap_or(V::k(0.0));
        let used = sum_outer(items, m, gaps);
        let growing = cx.lt(used, im);
        let shrinking = !growing && cx.lt(im, used);
        if growing {
            let grow: f32 = items.iter().filter(|i| i.abs.is_none()).map(|i| i.grow).sum();
            if grow > 0.0 {
                // style minimums only: an item that grows cannot violate its content minimum
                for it in items.iter_mut() {
                    it.auto_min = it.min[m].max(it.pb[m]);
                }
                resolve(items, m, im, gaps, true);
            }
        } else if shrinking {
            kids.visit(&mut Mins { cx, items, i: 0, m, inner, cross_av });
            resolve(items, m, im, gaps, false);
        }
    }
    let im = inner[m].unwrap_or(V::k(0.0));

    // Cross sizes. A size-only question about the main axis of a container whose cross size is known stops here.
    if !full && outer[c].is_some() {
        return Solved { outer: [outer[0].unwrap_or(V::k(0.0)), outer[1].unwrap_or(V::k(0.0))], inner };
    }
    let mut cav = [Av::Max; 2];
    cav[m] = Av::Def(im);
    cav[c] = cross_av;
    kids.visit(&mut Cross { cx, items, i: 0, m, inner, av: cav, line: inner[c].is_some() });
    let line = match inner[c] {
        Some(v) => v,
        None => {
            let mut mx = V::k(0.0);
            for it in items.iter().filter(|i| i.abs.is_none()) {
                mx = cx.max(mx, it.target[c].addk(it.margin[c] + it.margin[c + 2]));
            }
            mx
        }
    };
    if outer[c].is_none() {
        let mut o = line.addk(pb[c]);
        if let Some(mn) = opt_min(sp.min[c], s) {
            if !content {
                o = vmaxk(cx, o, mn);
            }
        }
        outer[c] = Some(o);
        inner[c] = Some(o.subk(pb[c]));
    }
    let outer = [outer[0].unwrap_or(V::k(0.0)), outer[1].unwrap_or(V::k(0.0))];
    if !full {
        return Solved { outer, inner };
    }

    // Stretch, justify, align: the offsets of every item from the container's origin.
    let used = {
        let mut sum = V::k(-0.0);
        for it in items.iter().filter(|i| i.abs.is_none()) {
            sum = sum.add(it.target[m].addk(it.margin[m] + it.margin[m + 2]));
        }
        V::k(gaps).add(sum)
    };
    let free = im.sub(used);
    let just = match sp.justify {
        None | Some(Justify::Start) => Justify::Start,
        Some(j) => {
            // free space decides the fallback: a decision
            let pos = cx.lt(V::k(0.0), free);
            match j {
                Justify::SpaceBetween if !pos || flow == 1 => Justify::Start,
                Justify::SpaceAround | Justify::SpaceEvenly if !pos || flow == 1 => Justify::Center,
                j => j,
            }
        }
    };
    let n = flow as f32;
    let (first, between): (V, V) = match just {
        Justify::Start => (V::k(0.0), V::k(gap)),
        Justify::End => (free, V::k(gap)),
        Justify::Center => (free.mulk(0.5), V::k(gap)),
        Justify::SpaceBetween => (V::k(0.0), V { v: gap + free.v / (n - 1.0), d: free.blur().d }),
        Justify::SpaceAround => (V { v: (free.v / n) / 2.0, d: free.blur().d }, V { v: gap + free.v / n, d: free.blur().d }),
        Justify::SpaceEvenly => (V { v: free.v / (n + 1.0), d: free.blur().d }, V { v: gap + free.v / (n + 1.0), d: free.blur().d }),
    };
    let mut run = V::k(pad[m]);
    let mut seen = false;
    for it in items.iter_mut().filter(|i| i.abs.is_none()) {
        // cross
        if it.align == Align::Stretch && it.size[c].is_none() {
            let v = line.subk(it.margin[c] + it.margin[c + 2]);
            let v = if it.min[c] > 0.0 { vmaxk(cx, v, it.min[c]) } else { v };
            it.target[c] = if it.pb[c] > 0.0 { vmaxk(cx, v, it.pb[c]) } else { v };
        }
        let free_c = line.sub(it.target[c].addk(it.margin[c] + it.margin[c + 2]));
        let oc = match it.align {
            Align::Start | Align::Stretch => V::k(0.0),
            Align::End => free_c,
            Align::Center => free_c.mulk(0.5),
        };
        it.off[c] = oc.addk(pad[c] + it.margin[c]);
        // main
        let om = if seen { between } else { first };
        seen = true;
        #[cfg(feature = "mutant_gap")]
        let om = if om.v == gap && gap != 0.0 { V::k(0.0) } else { om };
        it.off[m] = run.add(om).addk(it.margin[m]);
        run = run.add(om).addk(it.margin[m] + it.margin[m + 2]).add(it.target[m]);
    }
    Solved { outer, inner }
}

/// The gaps plus every in-flow item's hypothetical outer main size, summed as cephal sums them.
#[inline(always)]
fn sum_outer(items: &[Item], m: usize, gaps: f32) -> V {
    let mut sum = V::k(-0.0);
    for it in items.iter().filter(|i| i.abs.is_none()) {
        sum = sum.add(it.hyp.addk(it.margin[m] + it.margin[m + 2]));
    }
    V::k(gaps).add(sum)
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
        for it in items.iter().filter(|i| i.abs.is_none()) {
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
    for it in items.iter_mut().filter(|i| i.abs.is_none()) {
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
        for it in items.iter_mut().filter(|i| i.abs.is_none() && !i.frozen) {
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
        for it in items.iter_mut().filter(|i| i.abs.is_none() && !i.frozen) {
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
    for it in items.iter_mut().filter(|i| i.abs.is_none()) {
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
pub fn place_kids<K: Kids, O: Out + ?Sized>(kids: &K, cx: &mut Cx, buf: &mut Buf, sol: &Solved, org: [V; 2], out: &mut O, k: usize) -> usize {
    let mut p = Place { cx, items: buf.items(), i: 0, out, k, org, inner: sol.inner, outer: sol.outer };
    kids.visit(&mut p);
    p.k
}
