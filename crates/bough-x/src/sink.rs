//! Description by continuation (ROUND8 section 5) and lists as relations (section 3).
//!
//! `view` returns nothing. It drives a [`Sink`]: the same code mounts (`Write`, fresh), re-describes by comparing
//! against the columns (`Write`, not fresh), brings lists up to date after a scroll or an edit while skipping
//! everything fixed (`Patch`), and paints directly as an oracle (`soft::Immediate`). A list's row is a pure
//! function from an index to a [`Row`]: a plain tuple of records whose type is the row's shape, so a row is
//! written with no cursor, no lookup by type and no check.
use crate::expr::U;
use crate::store::{Arch, Backend, HoverFn, Pass, Rect, Space, Store, Tiles};
use core::any::TypeId;
use core::marker::PhantomData;

pub trait View: Sized {
    /// The Frame-stage state: plain words the handlers write.
    fn block(&self) -> &U;
    fn view<S: Sink<Self>>(&self, s: &mut S);
}

/// The sink a scope's body is given, for a borrow of `'r`.
///
/// This is a lifetime-generic associated type spelled as a supertrait, because the direct spelling
/// (`type Scope<'r>: Sink<V> where Self: 'r`) cannot be named under `for<'r>`: the higher-ranked bound ranges
/// over every `'r`, `'static` included, and `Self: 'r` then fails for any sink that borrows. The defaulted
/// `Implied = &'r Self` parameter carries `Self: 'r` as an implied bound, so `for<'r> ScopeOf<'r, V>` ranges
/// only over the lifetimes for which the sink is alive.
pub trait ScopeOf<'r, V, Implied = &'r Self> {
    type Scope: Sink<V>;
}

pub trait Sink<V>: Sized + for<'r> ScopeOf<'r, V> {
    /// One record of the fixed shape.
    fn emit<A: Arch>(&mut self, r: Rect, a: A);
    /// One record that hears the pointer enter and leave. `f` captures nothing; it is given the record.
    fn emit_hover<A: Arch, F: Fn(&mut V, &A, bool) + Copy + 'static>(&mut self, r: Rect, a: A, f: F);
    /// A child with its own coordinate space (a clip, a scroll offset read from the block).
    fn scope<F: for<'r> FnOnce(&mut <Self as ScopeOf<'r, V>>::Scope)>(&mut self, space: Space, f: F);
    /// The rows of a relation, laid out one under another. Only exposed rows are asked for.
    fn list<R: Row, F: FnMut(usize) -> R>(&mut self, l: &List<'_>, f: F);
}

// ---- relations ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Edit {
    Update(u32),
    /// Rows from here on are other rows than before (an insert or a remove).
    Shift(u32),
}

/// The last edits of a relation. A list compares its own `seen` with `ver` and replays what it missed; nothing
/// has to be cleared, and any number of lists may read one relation.
pub struct Journal {
    pub ver: u32,
    pub log: [Edit; 8],
}
impl Journal {
    pub const fn new() -> Journal {
        Journal { ver: 0, log: [Edit::Update(0); 8] }
    }
    #[inline(always)]
    pub fn push(&mut self, e: Edit) {
        self.log[self.ver as usize & 7] = e;
        self.ver = self.ver.wrapping_add(1);
    }
    /// The rows that changed since version `seen`: every row from `.0` on, and the `.2` rows listed in `.1`.
    #[inline]
    pub fn since(&self, seen: u32) -> (u32, [u32; 8], usize) {
        let pending = self.ver.wrapping_sub(seen);
        let (mut from, mut upd, mut n) = (u32::MAX, [0u32; 8], 0);
        if pending > 8 {
            return (0, upd, 0);
        }
        for k in 0..pending {
            match self.log[self.ver.wrapping_sub(1 + k) as usize & 7] {
                Edit::Update(at) => {
                    upd[n] = at;
                    n += 1;
                }
                Edit::Shift(at) => from = from.min(at),
            }
        }
        (from, upd, n)
    }
}
impl Default for Journal {
    fn default() -> Self {
        Journal::new()
    }
}
/// For a list over something that never changes.
pub static FIXED: Journal = Journal::new();

/// A collection whose changes are its own derivative: an edit is a plain call that also notes which row.
pub struct Rel<T> {
    items: Vec<T>,
    pub journal: Journal,
}
impl<T> Rel<T> {
    pub fn new(items: Vec<T>) -> Rel<T> {
        Rel { items, journal: Journal::new() }
    }
    #[inline(always)]
    pub fn len(&self) -> usize {
        self.items.len()
    }
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    #[inline(always)]
    pub fn get(&self, i: usize) -> &T {
        &self.items[i]
    }
    pub fn items(&self) -> &[T] {
        &self.items
    }
    /// The items with no note taken: for state kept opaque, where the view is re-described and compared.
    pub fn items_mut(&mut self) -> &mut Vec<T> {
        &mut self.items
    }
    #[inline(always)]
    pub fn edit(&mut self, i: usize) -> &mut T {
        self.journal.push(Edit::Update(i as u32));
        &mut self.items[i]
    }
    pub fn insert(&mut self, i: usize, v: T) {
        self.journal.push(Edit::Shift(i as u32));
        self.items.insert(i, v);
    }
    pub fn remove(&mut self, i: usize) -> T {
        self.journal.push(Edit::Shift(i as u32));
        self.items.remove(i)
    }
}

/// A list of `n` rows of one height. Coordinates are in the enclosing space.
pub struct List<'a> {
    pub n: u32,
    pub row_h: i32,
    pub x: i32,
    pub y: i32,
    pub edits: &'a Journal,
}

// ---- rows --------------------------------------------------------------------------------------------------

/// One record of a row: where it is (relative to the row) and what it is.
#[derive(Clone, Copy)]
pub struct At<A>(pub Rect, pub A);

pub trait Leaf {
    fn leaf<A: Arch>(&mut self, r: Rect, a: &A);
}

/// The shape of a row: a record, a tuple of rows, or an array of rows. Its type fixes the sequence of
/// archetypes, so the columns of a list are resolved once, when the list is first described.
pub trait Row: Copy + 'static {
    const LEAVES: u32;
    fn visit<L: Leaf>(&self, l: &mut L);
    /// Write a fresh row into slot `slot`.
    ///
    /// SAFETY: sites `p0..p0 + LEAVES` of `st` were discovered from a value of this type, and `slot < st.cap`.
    unsafe fn write(&self, st: &mut Store, p0: u32, slot: u32, org: (i32, i32));
    /// Compare with what slot `slot` holds; write and damage what differs.
    ///
    /// SAFETY: as [`Row::write`].
    unsafe fn patch(&self, st: &mut Store, p0: u32, slot: u32, org: (i32, i32), mk: &Mk, cx: &mut Cx<'_>);
}

/// Where a store's records are on screen during a pass.
#[derive(Clone, Copy)]
pub struct Mk {
    pub org: (i32, i32),
    pub clip: Rect,
}

impl<A: Arch> Row for At<A> {
    const LEAVES: u32 = 1;
    #[inline(always)]
    fn visit<L: Leaf>(&self, l: &mut L) {
        l.leaf(self.0, &self.1)
    }
    #[inline(always)]
    unsafe fn write(&self, st: &mut Store, p0: u32, slot: u32, org: (i32, i32)) {
        // SAFETY: the caller's contract: site `p0` exists, its column is of archetype `A` with room for `cap`
        // slots at `base`.
        unsafe {
            let site = *st.sites.get_unchecked(p0 as usize);
            let col = st.cols.get_unchecked_mut(site.col as usize);
            let rec = site.base + slot;
            let pg = &mut *col.page::<A>(rec >> 6);
            let i = (rec & 63) as usize;
            pg.set_rect(i, self.0.shift(org));
            pg.rec[i] = self.1;
            col.dirty |= 1 << ((rec >> 6) & 63);
        }
    }
    #[inline(always)]
    unsafe fn patch(&self, st: &mut Store, p0: u32, slot: u32, org: (i32, i32), mk: &Mk, cx: &mut Cx<'_>) {
        // SAFETY: as `write`.
        unsafe {
            let site = *st.sites.get_unchecked(p0 as usize);
            let col = st.cols.get_unchecked_mut(site.col as usize);
            let rec = site.base + slot;
            let pg = &mut *col.page::<A>(rec >> 6);
            let i = (rec & 63) as usize;
            let r = self.0.shift(org);
            if (pg.rect(i) != r) | (pg.rec[i] != self.1) {
                replace(pg, i, r, self.1, mk, cx);
                col.dirty |= 1 << ((rec >> 6) & 63);
            }
        }
    }
}

/// A record that differs from what the column holds: damage where it was, store it, damage where it is.
#[inline(never)]
fn replace<A: Arch>(pg: &mut crate::store::Page<A>, i: usize, r: Rect, a: A, mk: &Mk, cx: &mut Cx<'_>) {
    let old = pg.rect(i).shift(mk.org).shift(if A::MOVES { pg.rec[i].offset(cx.u) } else { (0, 0) });
    cx.tiles.mark(old.and(mk.clip));
    pg.set_rect(i, r);
    pg.rec[i] = a;
    let new = r.shift(mk.org).shift(if A::MOVES { a.offset(cx.u) } else { (0, 0) });
    cx.tiles.mark(new.and(mk.clip));
}

impl<X: Row, const N: usize> Row for [X; N] {
    const LEAVES: u32 = X::LEAVES * N as u32;
    #[inline(always)]
    fn visit<L: Leaf>(&self, l: &mut L) {
        for x in self {
            x.visit(l);
        }
    }
    #[inline(always)]
    unsafe fn write(&self, st: &mut Store, p0: u32, slot: u32, org: (i32, i32)) {
        for (k, x) in self.iter().enumerate() {
            // SAFETY: element `k` owns sites `p0 + k * X::LEAVES ..`, part of the caller's range.
            unsafe { x.write(st, p0 + k as u32 * X::LEAVES, slot, org) }
        }
    }
    #[inline(always)]
    unsafe fn patch(&self, st: &mut Store, p0: u32, slot: u32, org: (i32, i32), mk: &Mk, cx: &mut Cx<'_>) {
        for (k, x) in self.iter().enumerate() {
            // SAFETY: as `write`.
            unsafe { x.patch(st, p0 + k as u32 * X::LEAVES, slot, org, mk, cx) }
        }
    }
}

macro_rules! row_tuple {
    ($($t:ident)+) => {
        impl<$($t: Row),+> Row for ($($t,)+) {
            const LEAVES: u32 = 0 $(+ $t::LEAVES)+;
            #[inline(always)]
            #[allow(non_snake_case)]
            fn visit<L: Leaf>(&self, l: &mut L) {
                let ($($t,)+) = self;
                $($t.visit(l);)+
            }
            #[inline(always)]
            #[allow(non_snake_case, unused_assignments)]
            unsafe fn write(&self, st: &mut Store, p0: u32, slot: u32, org: (i32, i32)) {
                let ($($t,)+) = self;
                let mut p = p0;
                // SAFETY: each element owns the next `LEAVES` sites of the caller's range.
                $(unsafe { $t.write(st, p, slot, org) } p += $t::LEAVES;)+
            }
            #[inline(always)]
            #[allow(non_snake_case, unused_assignments)]
            unsafe fn patch(&self, st: &mut Store, p0: u32, slot: u32, org: (i32, i32), mk: &Mk, cx: &mut Cx<'_>) {
                let ($($t,)+) = self;
                let mut p = p0;
                // SAFETY: as `write`.
                $(unsafe { $t.patch(st, p, slot, org, mk, cx) } p += $t::LEAVES;)+
            }
        }
    };
}
row_tuple!(A);
row_tuple!(A B);
row_tuple!(A B C);
row_tuple!(A B C D);
row_tuple!(A B C D E);
row_tuple!(A B C D E F);
row_tuple!(A B C D E F G);
row_tuple!(A B C D E F G H);

// ---- the sinks ---------------------------------------------------------------------------------------------

/// What a pass carries: the damage, the block, and whether every exposed row is to be compared.
pub struct Cx<'c> {
    pub tiles: &'c mut Tiles,
    pub u: &'c U,
    pub all: bool,
}

/// Mount and re-description: every record described is compared with its column (or written, when fresh).
pub struct Write<'w, 'c, V, B> {
    pub(crate) st: &'w mut Store,
    pub(crate) cx: &'w mut Cx<'c>,
    pub(crate) pos: u32,
    pub(crate) sub: u32,
    pub(crate) fresh: bool,
    pub(crate) mk: Mk,
    pub(crate) _p: PhantomData<fn(&mut V, B)>,
}

impl<'w, 'c, V, B: Backend> Write<'w, 'c, V, B> {
    pub(crate) fn new(st: &'w mut Store, cx: &'w mut Cx<'c>, fresh: bool) -> Self {
        let mk = Mk { org: st.space.origin(cx.u), clip: st.space.clip };
        Write { st, cx, pos: 0, sub: 0, fresh, mk, _p: PhantomData }
    }
    #[inline(always)]
    fn put<A: Arch>(&mut self, r: Rect, a: A, key: TypeId, hover: Option<HoverFn>) {
        let p = self.pos as usize;
        self.pos += 1;
        if p == self.st.sites.len() {
            self.st.discover::<A, B>(r, key, hover);
        }
        let site = self.st.sites[p];
        let col = &mut self.st.cols[site.col as usize];
        // The one check per record of a fixed shape: this position still describes the archetype it first did.
        assert!(col.key == key, "the shape of a view changed between passes; a part that comes and goes is a list");
        // SAFETY: the column's key is `A`'s (checked above) and the site's record is inside its pages (`discover`).
        let pg = unsafe { &mut *col.page::<A>(site.base >> 6) };
        let i = (site.base & 63) as usize;
        if self.fresh {
            pg.set_rect(i, r);
            pg.rec[i] = a;
        } else if (pg.rect(i) != r) | (pg.rec[i] != a) {
            replace(pg, i, r, a, &self.mk, self.cx);
        } else {
            return;
        }
        col.dirty |= 1 << ((site.base >> 6) & 63);
    }
}

unsafe fn hover_tramp<V, A, F: Fn(&mut V, &A, bool) + Copy + 'static>(v: *mut (), a: *const u8, on: bool) {
    const { assert!(size_of::<F>() == 0, "a handler captures nothing: it is given the record") };
    // SAFETY: `F` is zero-sized and `Copy`, and `emit_hover` was handed a value of it. The caller passes the
    // `V` the view was described for and a record of the column this handler was registered on.
    unsafe {
        let f: F = core::mem::zeroed();
        f(&mut *(v as *mut V), &*(a as *const A), on)
    }
}

impl<'r, 'w, 'c, V, B: Backend> ScopeOf<'r, V> for Write<'w, 'c, V, B> {
    type Scope = Write<'r, 'c, V, B>;
}
impl<'w, 'c, V, B: Backend> Sink<V> for Write<'w, 'c, V, B> {

    #[inline(always)]
    fn emit<A: Arch>(&mut self, r: Rect, a: A) {
        self.put(r, a, TypeId::of::<A>(), None)
    }
    #[inline(always)]
    fn emit_hover<A: Arch, F: Fn(&mut V, &A, bool) + Copy + 'static>(&mut self, r: Rect, a: A, f: F) {
        let _ = f;
        self.put(r, a, TypeId::of::<(A, F)>(), Some(hover_tramp::<V, A, F>))
    }
    fn scope<F: for<'r> FnOnce(&mut <Self as ScopeOf<'r, V>>::Scope)>(&mut self, space: Space, f: F) {
        let q = self.sub as usize;
        self.sub += 1;
        let mut fresh = self.fresh;
        if q == self.st.subs.len() {
            self.st.subs.push(Store::new(1, space));
            self.st.order.push(Pass::Sub(q as u32));
            fresh = true;
        }
        let st = &mut self.st.subs[q];
        if st.space != space {
            if !fresh {
                self.cx.tiles.mark(st.space.clip);
                self.cx.tiles.mark(space.clip);
            }
            st.space = space;
        }
        st.row_live[0] = 1;
        let mut sub = Write::<V, B>::new(st, self.cx, fresh);
        f(&mut sub);
    }
    #[inline(always)]
    fn list<R: Row, F: FnMut(usize) -> R>(&mut self, l: &List<'_>, f: F) {
        let q = self.sub as usize;
        self.sub += 1;
        list_sync::<R, F, B>(self.st, q, self.cx, l, f);
    }
}

/// After a turn: the fixed shape cannot have changed (its operands are expressions over the block), so its
/// records are not described at all; only lists are brought up to date.
pub struct Patch<'w, 'c, V, B> {
    pub(crate) st: &'w mut Store,
    pub(crate) cx: &'w mut Cx<'c>,
    pub(crate) sub: u32,
    pub(crate) _p: PhantomData<fn(&mut V, B)>,
}

impl<'r, 'w, 'c, V, B: Backend> ScopeOf<'r, V> for Patch<'w, 'c, V, B> {
    type Scope = Patch<'r, 'c, V, B>;
}
impl<'w, 'c, V, B: Backend> Sink<V> for Patch<'w, 'c, V, B> {

    #[inline(always)]
    fn emit<A: Arch>(&mut self, _: Rect, _: A) {}
    #[inline(always)]
    fn emit_hover<A: Arch, F: Fn(&mut V, &A, bool) + Copy + 'static>(&mut self, _: Rect, _: A, _: F) {}
    #[inline(always)]
    fn scope<F: for<'r> FnOnce(&mut <Self as ScopeOf<'r, V>>::Scope)>(&mut self, _: Space, f: F) {
        let q = self.sub as usize;
        self.sub += 1;
        let mut sub = Patch::<V, B> { st: &mut self.st.subs[q], cx: self.cx, sub: 0, _p: PhantomData };
        f(&mut sub);
    }
    #[inline(always)]
    fn list<R: Row, F: FnMut(usize) -> R>(&mut self, l: &List<'_>, f: F) {
        let q = self.sub as usize;
        self.sub += 1;
        list_sync::<R, F, B>(self.st, q, self.cx, l, f);
    }
}

struct Discover<'a, B>(&'a mut Store, PhantomData<B>);
impl<B: Backend> Leaf for Discover<'_, B> {
    fn leaf<A: Arch>(&mut self, r: Rect, _: &A) {
        self.0.discover::<A, B>(r, TypeId::of::<A>(), None);
    }
}

/// The exposed rows of a list of `n` rows under a space at `org` with clip `clip`.
#[inline(always)]
pub fn exposed(l: &List<'_>, org: (i32, i32), clip: Rect) -> (u32, u32) {
    let top = clip.y0 - org.1 - l.y;
    let n = l.n as i32;
    let lo = top.div_euclid(l.row_h).clamp(0, n);
    let hi = (top + (clip.y1 - clip.y0) + l.row_h - 1).div_euclid(l.row_h).clamp(lo, n);
    (lo as u32, hi as u32)
}

/// Bring list `q` of `parent` up to date: rows that entered exposure are written, rows the relation's journal
/// names are compared, rows that left are released. Nothing else is touched, and nothing outside the window
/// exists.
#[inline]
pub fn list_sync<R: Row, F: FnMut(usize) -> R, B: Backend>(parent: &mut Store, q: usize, cx: &mut Cx<'_>, l: &List<'_>, mut f: F) {
    let space = parent.space;
    if q == parent.subs.len() {
        let rows = (space.clip.y1 - space.clip.y0) / l.row_h + 2;
        let mut st = Store::new((rows as u32).next_power_of_two().max(64), space);
        st.seen = l.edits.ver;
        parent.subs.push(st);
        parent.order.push(Pass::Sub(q as u32));
    }
    let st = &mut parent.subs[q];
    let org = space.origin(cx.u);
    let (lo, hi) = exposed(l, org, space.clip);
    let (from, upd, nupd) = l.edits.since(st.seen);
    #[cfg(feature = "mutant_journal")]
    let (from, nupd) = (u32::MAX, 0 * nupd);
    let (olo, ohi) = (st.lo, st.hi);
    if (lo == olo) & (hi == ohi) & (st.seen == l.edits.ver) & !cx.all {
        return;
    }
    list_rows::<R, F, B>(st, cx, l, &mut f, (lo, hi), (from, upd, nupd), org, space);
    let _ = (olo, ohi);
}

#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn list_rows<R: Row, F: FnMut(usize) -> R, B: Backend>(st: &mut Store, cx: &mut Cx<'_>, l: &List<'_>, f: &mut F, (lo, hi): (u32, u32), (from, upd, nupd): (u32, [u32; 8], usize), org: (i32, i32), space: Space) {
    st.seen = l.edits.ver;
    st.space = space;
    let (olo, ohi) = (st.lo, st.hi);
    assert!(hi - lo <= st.cap, "the window outgrew the list's capacity");
    let mk = Mk { org, clip: space.clip };
    let row_rect = |r: u32| Rect { x0: space.clip.x0, y0: org.1 + l.y + r as i32 * l.row_h, x1: space.clip.x1, y1: org.1 + l.y + (r as i32 + 1) * l.row_h }.and(space.clip);
    // rows that left: what they showed is gone (a no-op when they left by scrolling out of the clip)
    for r in olo..ohi {
        if r < lo || r >= hi {
            cx.tiles.mark(row_rect(r));
        }
    }
    let mask = st.cap - 1;
    // The one check per batch: this list's rows are still of the type its columns were made for.
    let mut known = match st.row_ty {
        Some(t) => {
            assert!(t == TypeId::of::<R>(), "a list's row type changed");
            true
        }
        None => false,
    };
    for r in lo..hi {
        let fresh = r < olo || r >= ohi;
        if !(fresh | cx.all | (r >= from) | upd[..nupd].contains(&r)) {
            continue;
        }
        let row = f(r as usize);
        let at = (l.x, l.y + r as i32 * l.row_h);
        if !known {
            row.visit(&mut Discover::<B>(st, PhantomData));
            st.row_ty = Some(TypeId::of::<R>());
            known = true;
        }
        if fresh {
            // SAFETY: the sites were discovered from a row of type `R` (checked above); the slot is under `cap`.
            unsafe { row.write(st, 0, r & mask, at) };
            cx.tiles.mark(row_rect(r));
        } else {
            // SAFETY: as above.
            unsafe { row.patch(st, 0, r & mask, at, &mk, cx) };
        }
    }
    st.lo = lo;
    st.hi = hi;
    st.row_live.fill(0);
    for r in lo..hi {
        let s = r & mask;
        st.row_live[s as usize >> 6] |= 1 << (s & 63);
    }
}
