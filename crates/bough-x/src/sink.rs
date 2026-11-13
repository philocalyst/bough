//! Description by continuation (ROUND8 section 5) and lists as relations (section 3).
//!
//! `view` returns nothing. It drives a [`Sink`]: the same code mounts (`Write`, fresh), re-describes by comparing
//! against the columns (`Write`, not fresh), brings lists up to date after a scroll or an edit while skipping
//! everything fixed (`Patch`), and paints directly as an oracle (`soft::Immediate`).
//!
//! What is described is always a [`Row`]: a plain value (a record, a tuple or an array of rows) whose *type* is
//! the shape. The fixed part of a view is one row; a list's row is a pure function from an index to a row. A
//! row carries operands only. Where its records are is the layout's answer, given once per shape as a slice of
//! rects, so rows of a list that share a shape share one layout.
use crate::expr::U;
use crate::store::{Arch, Backend, Hov, Pass, Rect, Site, Space, Store, Tiles, desc};
use core::any::TypeId;
use core::marker::PhantomData;
use core::sync::atomic::{AtomicU32, Ordering};

pub trait View: Sized {
    /// The Frame-stage state: plain words the handlers write.
    fn block(&self) -> &U;
    /// The block for a pass. A view that declares its block hands it over; a view whose Frame state is its own
    /// fields (the element layer) has them gathered into `out` by the table of `slots` the world learned from
    /// its columns.
    #[inline(always)]
    fn gathered<'a>(&'a self, _slots: &[crate::expr::Slot], _out: &'a mut U) -> &'a U {
        self.block()
    }
    /// The same words, for the core to write the ones a space reads as its scroll offset (a wheel over a space
    /// scrolls it with no handler). A view that does not give them out scrolls only through its own handlers.
    fn block_mut(&mut self) -> Option<&mut U> {
        None
    }
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

/// What a pass lays elements out against.
#[derive(Clone, Copy)]
pub struct Env<'a> {
    /// Device pixels per point.
    pub scale: f32,
    /// The window, in device pixels.
    pub size: [f32; 2],
    pub u: &'a U,
}

/// Where a list of elements is: its origin in the enclosing space and its width, in device pixels.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Geo {
    pub x: i32,
    pub y: i32,
    pub w: i32,
}

pub trait Sink<V>: Sized + for<'r> ScopeOf<'r, V> {
    /// Whether this sink takes the fixed rows at all. A pass after a turn does not: an element then skips its
    /// layout and its records and only reaches its lists.
    const FIXED: bool = true;
    /// What elements are laid out against. A sink that only takes rows with given rects need not answer.
    fn env(&self) -> Env<'_> {
        unimplemented!("this sink does not lay elements out")
    }
    /// The rows of a list of elements, one under another: `f(r)` is row `r`, an element that is its records
    /// and its own layout. `geo` is where the list is; a sink that keeps what it was told (`FIXED == false`)
    /// is given `None`. Returns the height of the list in device pixels.
    #[track_caller]
    fn rows<R: Row<V> + bough_lay::Lay, F: FnMut(usize) -> R>(&mut self, _n: u32, _geo: Option<Geo>, _f: F) -> i32 {
        unimplemented!("this sink does not take lists of elements")
    }
    /// A row of the fixed shape: its layout (one rect per record, in order) and its operands. The closure is
    /// only called by a sink that wants the row; after a turn that changed no opaque state, none does.
    ///
    /// Every method is `#[track_caller]`: the reflecting sink records where in the view a row, a scope or a list
    /// is described; every other sink ignores the location, and after inlining it is not there.
    #[track_caller]
    fn row<R: Row<V>, L: AsRef<[Rect]>, F: FnOnce() -> (L, R)>(&mut self, f: F);
    /// A child with its own coordinate space (a clip, a scroll offset read from the block).
    #[track_caller]
    fn scope<F: for<'r> FnOnce(&mut <Self as ScopeOf<'r, V>>::Scope)>(&mut self, space: Space, f: F);
    /// The rows of a relation, one under another. Only exposed rows are asked for.
    #[track_caller]
    fn list<R: Row<V>, F: FnMut(usize) -> R>(&mut self, l: &List<'_>, f: F);
}

// ---- relations ---------------------------------------------------------------------------------------------

/// Bumped by every edit of every relation. A settle that finds it where it left it, and the block unchanged,
/// has nothing to do and does not drive the view.
pub static REL_EPOCH: AtomicU32 = AtomicU32::new(0);

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Edit {
    Update(u32),
    /// Rows from here on are other rows than before (for a list: as an insert or a remove does).
    Shift(u32),
    /// A row was inserted at this index: the rows from here on moved down by one.
    Insert(u32),
    /// The row at this index was removed: the rows after it moved up by one.
    Remove(u32),
}

/// The last edits of a relation: its derivative. A list compares its own `seen` with `ver` and replays what it
/// missed; nothing has to be cleared, and any number of lists may read one relation.
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
        REL_EPOCH.fetch_add(1, Ordering::Relaxed);
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
                Edit::Shift(at) | Edit::Insert(at) | Edit::Remove(at) => from = from.min(at),
            }
        }
        (from, upd, n)
    }
    /// The inserts and removes since version `seen`, oldest first, into `out` (what a reference to a row by its
    /// index is remapped by). `false` when more happened than the journal keeps: the rows' identities are lost.
    pub fn moves_since(&self, seen: u32, out: &mut Vec<Edit>) -> bool {
        out.clear();
        let pending = self.ver.wrapping_sub(seen);
        if pending > 8 {
            return false;
        }
        for k in (0..pending).rev() {
            match self.log[self.ver.wrapping_sub(1 + k) as usize & 7] {
                Edit::Update(_) => {}
                Edit::Shift(_) => return false,
                e => out.push(e),
            }
        }
        true
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
        self.journal.push(Edit::Insert(i as u32));
        self.items.insert(i, v);
    }
    pub fn remove(&mut self, i: usize) -> T {
        self.journal.push(Edit::Remove(i as u32));
        self.items.remove(i)
    }
}

/// A list of `n` rows of one height and one layout. `x`, `y` are in the enclosing space; `shape` is the layout
/// of one row, relative to the row.
pub struct List<'a> {
    pub n: u32,
    pub row_h: i32,
    pub x: i32,
    pub y: i32,
    pub shape: &'a [Rect],
    pub edits: &'a Journal,
}

// ---- rows --------------------------------------------------------------------------------------------------

pub trait Leaf<V> {
    fn leaf<A: Arch>(&mut self, a: &A, key: TypeId, hov: Option<Hov>);
    /// A place in a row that is not a record: a child with its own space (a scrolled container), whose rect
    /// the row's layout gives. It takes one position of the row so layout and records count alike.
    #[inline(always)]
    fn hole(&mut self) {}
}

/// `TypeId` of a type that may borrow: the identity of the type with its lifetimes erased. A row may hold
/// closures that borrow the view; which columns it is written to depends on its type and never on a lifetime.
#[inline(always)]
pub fn tid<T: ?Sized>() -> TypeId {
    trait Key {
        fn key(&self) -> TypeId
        where
            Self: 'static;
    }
    impl<T: ?Sized> Key for PhantomData<T> {
        #[inline(always)]
        fn key(&self) -> TypeId
        where
            Self: 'static,
        {
            TypeId::of::<T>()
        }
    }
    let p = PhantomData::<T>;
    // SAFETY: only the vtable's `key` is called, which reads nothing of the value; extending the trait
    // object's lifetime bound cannot let a borrow escape because no data of `T` exists in a `PhantomData`.
    unsafe { core::mem::transmute::<&dyn Key, &(dyn Key + 'static)>(&p).key() }
}

/// The shape of a row: a record, a tuple of rows, or an array of rows. Its type fixes the sequence of
/// archetypes, so the columns of a store are resolved once, when the row is first described.
pub trait Row<V> {
    const LEAVES: u32;
    fn visit<L: Leaf<V>>(&self, l: &mut L);
    /// Write the row's operands into slot `slot`.
    ///
    /// SAFETY: `sites` points at `LEAVES` sites discovered from a value of this type, whose pointers are
    /// current, and `slot` is under the store's capacity.
    unsafe fn write(&self, sites: *const Site, slot: u32);
    /// Compare with what slot `slot` holds; store and damage what differs.
    ///
    /// SAFETY: as [`Row::write`].
    unsafe fn patch(&self, sites: *const Site, slot: u32, mk: &Mk, cx: &mut Cx<'_>);
}

/// Where a store's records are on screen during a pass.
#[derive(Clone, Copy)]
pub struct Mk {
    pub org: (i32, i32),
    pub clip: Rect,
}

impl<V, A: Arch> Row<V> for A {
    const LEAVES: u32 = 1;
    #[inline(always)]
    fn visit<L: Leaf<V>>(&self, l: &mut L) {
        l.leaf(self, TypeId::of::<A>(), None)
    }
    #[inline(always)]
    unsafe fn write(&self, sites: *const Site, slot: u32) {
        // SAFETY: the caller's contract: the site's column is of archetype `A` and holds `slot`.
        unsafe { *(*sites).rec::<A>(slot) = *self }
    }
    #[inline(always)]
    unsafe fn patch(&self, sites: *const Site, slot: u32, mk: &Mk, cx: &mut Cx<'_>) {
        // SAFETY: as `write`.
        unsafe {
            let p = (*sites).rec::<A>(slot);
            if A::STATE {
                if !(*p).eq_ops(self) {
                    // other operands: its transitions go on from what the record shows now
                    let mut a = *self;
                    a.carry(&*p, cx.u, cx.u);
                    replace(&*sites, slot, p, a, mk, cx);
                }
            } else if !(*p).same(self) {
                replace(&*sites, slot, p, *self, mk, cx);
            }
        }
    }
}

/// A record whose operands differ from what the column holds: damage what it showed, store it, damage what it
/// shows (the same place unless its paint moves it).
///
/// SAFETY: `p` is the record of `site` at `slot`.
#[inline(never)]
unsafe fn replace<A: Arch>(site: &Site, slot: u32, p: *mut A, a: A, mk: &Mk, cx: &mut Cx<'_>) {
    // SAFETY: the caller's contract.
    unsafe {
        let r = site.rect(slot).shift(mk.org);
        if A::MOVES {
            cx.tiles.mark(r.shift((*p).offset(cx.u)).and(mk.clip));
            cx.tiles.mark(r.shift(a.offset(cx.u)).and(mk.clip));
        } else {
            cx.tiles.mark(r.and(mk.clip));
        }
        *p = a;
        cx.changed += 1;
    }
}

impl<V, X: Row<V>, const N: usize> Row<V> for [X; N] {
    const LEAVES: u32 = X::LEAVES * N as u32;
    #[inline(always)]
    fn visit<L: Leaf<V>>(&self, l: &mut L) {
        for x in self {
            x.visit(l);
        }
    }
    #[inline(always)]
    unsafe fn write(&self, sites: *const Site, slot: u32) {
        for (k, x) in self.iter().enumerate() {
            // SAFETY: element `k` owns the `X::LEAVES` sites from `k * X::LEAVES`, part of the caller's range.
            unsafe { x.write(sites.add(k * X::LEAVES as usize), slot) }
        }
    }
    #[inline(always)]
    unsafe fn patch(&self, sites: *const Site, slot: u32, mk: &Mk, cx: &mut Cx<'_>) {
        for (k, x) in self.iter().enumerate() {
            // SAFETY: as `write`.
            unsafe { x.patch(sites.add(k * X::LEAVES as usize), slot, mk, cx) }
        }
    }
}

macro_rules! row_tuple {
    ($($t:ident)+) => {
        impl<V, $($t: Row<V>),+> Row<V> for ($($t,)+) {
            const LEAVES: u32 = 0 $(+ $t::LEAVES)+;
            #[inline(always)]
            #[allow(non_snake_case)]
            fn visit<Lf: Leaf<V>>(&self, l: &mut Lf) {
                let ($($t,)+) = self;
                $($t.visit(l);)+
            }
            #[inline(always)]
            #[allow(non_snake_case, unused_assignments)]
            unsafe fn write(&self, sites: *const Site, slot: u32) {
                let ($($t,)+) = self;
                let mut p = sites;
                // SAFETY: each element owns the next `LEAVES` sites of the caller's range.
                $(unsafe { $t.write(p, slot); p = p.add($t::LEAVES as usize); })+
            }
            #[inline(always)]
            #[allow(non_snake_case, unused_assignments)]
            unsafe fn patch(&self, sites: *const Site, slot: u32, mk: &Mk, cx: &mut Cx<'_>) {
                let ($($t,)+) = self;
                let mut p = sites;
                // SAFETY: as `write`.
                $(unsafe { $t.patch(p, slot, mk, cx); p = p.add($t::LEAVES as usize); })+
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
row_tuple!(A B C D E F G H I);
row_tuple!(A B C D E F G H I J);
row_tuple!(A B C D E F G H I J K);
row_tuple!(A B C D E F G H I J K L);

// ---- the sinks ---------------------------------------------------------------------------------------------

/// What a pass carries: the damage, the block, whether every exposed row is to be compared, and (out) the
/// block words that move some list's exposure.
pub struct Cx<'c> {
    /// (out) A fixed record moved so that painting column by column no longer paints in the order described:
    /// the stores are to be made again.
    pub regroup: bool,
    pub tiles: &'c mut Tiles,
    pub u: &'c U,
    pub all: bool,
    pub lists: u64,
    /// Device pixels per point (what elements are laid out with).
    pub scale: f32,
    /// Records this pass stored again with other words or another rect, or lists it moved (an
    /// over-approximation): what a reader of the stores (reflection, accessibility) compares to know whether
    /// anything it read may have changed.
    pub changed: u32,
}

struct Discover<'a, B> {
    st: &'a mut Store,
    rects: &'a [Rect],
    k: usize,
    _b: PhantomData<B>,
}
impl<V, B: Backend> Leaf<V> for Discover<'_, B> {
    fn leaf<A: Arch>(&mut self, _: &A, key: TypeId, hov: Option<Hov>) {
        self.st.discover(desc::<A, B>(), self.rects[self.k], key, TypeId::of::<A>(), hov);
        self.k += 1;
    }
    fn hole(&mut self) {
        self.st.hole();
        self.k += 1;
    }
}

/// The sites of row `k` of the store's shape, discovering them the first time. One check per row value.
#[inline(always)]
fn row_sites<V, R: Row<V>, B: Backend>(st: &mut Store, k: usize, rects: &[Rect], row: &R) -> u32 {
    assert!(rects.len() == R::LEAVES as usize, "a row's layout has one rect per record");
    if k == st.rows.len() {
        return row_new::<V, R, B>(st, rects, row);
    }
    let (ty, p0) = st.rows[k];
    assert!(ty == tid::<R>(), "the shape of a view changed between passes; a part that comes and goes is a list");
    p0
}
#[cold]
#[inline(never)]
fn row_new<V, R: Row<V>, B: Backend>(st: &mut Store, rects: &[Rect], row: &R) -> u32 {
    let p0 = st.sites.len() as u32;
    row.visit(&mut Discover::<B> { st, rects, k: 0, _b: PhantomData });
    st.rows.push((tid::<R>(), p0));
    p0
}

/// Mount and re-description: every row described is written (fresh) or compared with its columns.
pub struct Write<'w, 'c, V, B> {
    pub(crate) st: &'w mut Store,
    pub(crate) cx: &'w mut Cx<'c>,
    pub(crate) rowc: u32,
    pub(crate) sub: u32,
    pub(crate) fresh: bool,
    pub(crate) mk: Mk,
    pub(crate) _p: PhantomData<fn(&mut V, B)>,
}

impl<'w, 'c, V, B: Backend> Write<'w, 'c, V, B> {
    pub(crate) fn new(st: &'w mut Store, cx: &'w mut Cx<'c>, fresh: bool) -> Self {
        let mk = Mk { org: st.space.origin(cx.u), clip: st.space.clip };
        Write { st, cx, rowc: 0, sub: 0, fresh, mk, _p: PhantomData }
    }
}

impl<'r, 'w, 'c, V, B: Backend> ScopeOf<'r, V> for Write<'w, 'c, V, B> {
    type Scope = Write<'r, 'c, V, B>;
}
impl<'w, 'c, V, B: Backend> Sink<V> for Write<'w, 'c, V, B> {
    #[inline]
    fn row<R: Row<V>, L: AsRef<[Rect]>, F: FnOnce() -> (L, R)>(&mut self, f: F) {
        let (rects, row) = f();
        let rects = rects.as_ref();
        let k = self.rowc as usize;
        self.rowc += 1;
        let p0 = row_sites::<V, R, B>(self.st, k, rects, &row) as usize;
        let sites = &self.st.sites[p0..p0 + rects.len()];
        if self.fresh {
            for (s, r) in sites.iter().zip(rects) {
                if s.col == u32::MAX {
                    continue;
                }
                // SAFETY: slot 0 of a site of this store (capacity 1).
                unsafe { s.set_rect(0, *r) }
            }
            // SAFETY: the sites were discovered from a row of type `R` (`row_sites`).
            unsafe { row.write(sites.as_ptr(), 0) }
        } else {
            let mut moved = false;
            for (s, r) in sites.iter().zip(rects) {
                if s.col == u32::MAX {
                    continue;
                }
                // SAFETY: as above.
                unsafe {
                    let old = s.rect(0);
                    if old != *r {
                        // A fixed record somewhere else: rare (a resize in re-describing mode). Its paint may
                        // move it further, which this untyped path cannot see, so the whole space is damaged.
                        self.cx.tiles.mark(self.mk.clip);
                        self.cx.changed += 1;
                        s.set_rect(0, *r);
                        moved = true;
                    }
                }
            }
            // SAFETY: as above.
            unsafe { row.patch(sites.as_ptr(), 0, &self.mk, self.cx) }
            if moved && !self.st.order_holds() {
                self.cx.regroup = true;
            }
        }
    }
    fn scope<F: for<'r> FnOnce(&mut <Self as ScopeOf<'r, V>>::Scope)>(&mut self, space: Space, f: F) {
        let q = self.sub as usize;
        self.sub += 1;
        let mut fresh = self.fresh;
        if q == self.st.subs.len() {
            self.st.subs.push(Store::new(1, space));
            crate::store::SHAPE.fetch_add(1, Ordering::Relaxed);
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
        let st = &self.st.subs[q];
        if st.timed {
            st.resweep(self.cx.u, self.cx.tiles);
        }
    }
    #[inline(always)]
    fn list<R: Row<V>, F: FnMut(usize) -> R>(&mut self, l: &List<'_>, f: F) {
        let q = self.sub as usize;
        self.sub += 1;
        list_sync::<V, R, F, B>(self.st, q, self.cx, l, f, core::panic::Location::caller());
    }
    #[inline(always)]
    fn env(&self) -> Env<'_> {
        Env { scale: self.cx.scale, size: [self.cx.tiles.w as f32, self.cx.tiles.h as f32], u: self.cx.u }
    }
    #[inline(always)]
    fn rows<R: Row<V> + bough_lay::Lay, F: FnMut(usize) -> R>(&mut self, n: u32, geo: Option<Geo>, f: F) -> i32 {
        let q = self.sub as usize;
        self.sub += 1;
        rows_sync::<V, R, F, B>(self.st, q, self.cx, n, geo, f, core::panic::Location::caller())
    }
}

/// After a turn: the fixed shape cannot have changed (its operands are expressions over the block), so its
/// rows are not even built; only lists are brought up to date.
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
    const FIXED: bool = false;
    #[inline(always)]
    fn env(&self) -> Env<'_> {
        Env { scale: self.cx.scale, size: [self.cx.tiles.w as f32, self.cx.tiles.h as f32], u: self.cx.u }
    }
    #[inline(always)]
    fn rows<R: Row<V> + bough_lay::Lay, F: FnMut(usize) -> R>(&mut self, n: u32, _: Option<Geo>, f: F) -> i32 {
        let q = self.sub as usize;
        self.sub += 1;
        rows_sync::<V, R, F, B>(self.st, q, self.cx, n, None, f, core::panic::Location::caller())
    }
    #[inline(always)]
    fn row<R: Row<V>, L: AsRef<[Rect]>, F: FnOnce() -> (L, R)>(&mut self, _: F) {}
    #[inline(always)]
    fn scope<F: for<'r> FnOnce(&mut <Self as ScopeOf<'r, V>>::Scope)>(&mut self, _: Space, f: F) {
        let q = self.sub as usize;
        self.sub += 1;
        let mut sub = Patch::<V, B> { st: &mut self.st.subs[q], cx: self.cx, sub: 0, _p: PhantomData };
        f(&mut sub);
    }
    #[inline(always)]
    fn list<R: Row<V>, F: FnMut(usize) -> R>(&mut self, l: &List<'_>, f: F) {
        let q = self.sub as usize;
        self.sub += 1;
        list_sync::<V, R, F, B>(self.st, q, self.cx, l, f, core::panic::Location::caller());
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
///
/// What is generic here is only what must be: asking the row function for a row, and writing or comparing its
/// records. Which rows, their layout, their damage and the window are decided by [`list_plan`], one body for
/// every list in a program.
#[inline]
///
/// A list's rows are known to be the rows its columns were made for by where they are described (the list's call
/// site) and their shape (leaves and size), not by their Rust type: a closure inside `view` (a handler, a mapped
/// function) is a different type in each sink's instantiation of `view`, the same source and the same layout.
pub fn list_sync<V, R: Row<V>, F: FnMut(usize) -> R, B: Backend>(parent: &mut Store, q: usize, cx: &mut Cx<'_>, l: &List<'_>, mut f: F, at: &'static core::panic::Location<'static>) {
    let space = parent.space;
    cx.lists |= space.reads();
    if q == parent.subs.len() {
        list_new(parent, l);
    }
    let st = &mut parent.subs[q];
    let org = space.origin(cx.u);
    let (lo, hi) = exposed(l, org, space.clip);
    if (lo == st.lo) & (hi == st.hi) & (st.seen == l.edits.ver) & !cx.all {
        return;
    }
    if lo < hi && !st.rows.is_empty() {
        // Admission, once per pass: the rows about to be written are of the record types the columns were made
        // for, leaf by leaf, and the window fits. If not, the list's columns are made again for these rows.
        let mut same = Same { st: &*st, k: 0, ok: hi - lo <= st.cap };
        f(lo as usize).visit(&mut same);
        if !(same.ok && same.k == st.sites.len()) {
            list_reset(st, cx, hi - lo);
        }
    }
    if st.rows.is_empty() && lo < hi {
        row_new::<V, R, B>(st, l.shape, &f(lo as usize));
        st.src = Some((at, R::LEAVES, size_of::<R>() as u32));
    }
    let mk = list_plan(st, cx, l, (lo, hi), org, space, R::LEAVES as usize, (at, R::LEAVES, size_of::<R>() as u32));
    let (sites, mask) = (st.sites.as_ptr(), st.cap - 1);
    let plan = core::mem::take(&mut st.plan);
    for &e in &plan {
        let r = e & 0x3fff_ffff;
        let row = f(r as usize);
        // SAFETY: the sites were discovered from a row of type `R` (`list_plan` checked the type); `r & mask` is
        // a slot of the list.
        unsafe {
            if e >> 31 != 0 {
                row.write(sites, r & mask);
            } else {
                row.patch(sites, r & mask, &mk, cx);
            }
        }
    }
    st.plan = plan;
    if st.timed {
        // rows came, went or were rewritten: which records are in motion, and where, is to be said again
        st.resweep(cx.u, cx.tiles);
    }
}

/// A row's rects as its layout gives them, into the lanes of slot `slot`, shifted to where the row is.
struct Lanes {
    sites: *const Site,
    slot: u32,
    at: (i32, i32),
}
impl bough_lay::Out for Lanes {
    #[inline(always)]
    fn put(&mut self, k: usize, r: [i32; 4]) {
        // SAFETY: `k` is under the row's leaves (the layout and the row count alike, checked when the list's
        // columns were made); the site is a record's and holds `slot`.
        unsafe { (*self.sites.add(k)).set_rect(self.slot, Rect { x0: r[0] + self.at.0, y0: r[1] + self.at.1, x1: r[2] + self.at.0, y1: r[3] + self.at.1 }) }
    }
}

/// A list of elements. As [`list_sync`], with each row's rects from the row's own layout: a row that enters is
/// laid out into its lanes. Rows are of one height (the first row's); a list whose rows differ in height
/// needs an extent, which the layout lane's `Extent` will give.
#[inline]
pub fn rows_sync<V, R: Row<V> + bough_lay::Lay, F: FnMut(usize) -> R, B: Backend>(parent: &mut Store, q: usize, cx: &mut Cx<'_>, n: u32, geo: Option<Geo>, mut f: F, at: &'static core::panic::Location<'static>) -> i32 {
    const { assert!(<R as Row<V>>::LEAVES as usize == <R as bough_lay::Lay>::LEAVES, "a row's layout has one rect per record") };
    let space = parent.space;
    cx.lists |= space.reads();
    if q == parent.subs.len() {
        // the first pass must say where the list is
        let g = geo.expect("a list of elements is first described by a pass that lays out");
        rows_new::<V, R, F, B>(parent, cx, n, g, &mut f, at);
    }
    let st = &mut parent.subs[q];
    if st.src.is_none() && n > 0 {
        // a list that was empty when first described: its columns are made now
        let g = geo.unwrap_or(Geo { x: st.lay.0, y: st.lay.1, w: st.lay.2 });
        rows_shape(st, cx, n, g, &mut f);
        let shape = st.row0.clone();
        row_new::<V, R, B>(st, &shape, &f(0));
        st.src = Some((at, <R as Row<V>>::LEAVES, size_of::<R>() as u32));
        st.tmpl = st.row0.clone();
        st.tmpl_at = st.row0.as_ptr() as usize;
    }
    if let Some(g) = geo {
        if (g.x, g.y, g.w) != st.lay {
            // somewhere else, or another width: every exposed row is laid out again
            rows_shape(st, cx, n, g, &mut f);
        }
    }
    let row_h = st.geo.2;
    if n == 0 || row_h == 0 {
        if st.hi > st.lo {
            cx.tiles.mark(space.clip);
            st.row_live.fill(0);
            (st.lo, st.hi) = (0, 0);
        }
        return 0;
    }
    let (gx, gy, gw) = st.lay;
    let org = space.origin(cx.u);
    let row0 = core::mem::take(&mut st.row0);
    let l = List { n, row_h, x: gx, y: gy, shape: &row0, edits: &FIXED };
    let (lo, hi) = exposed(&l, org, space.clip);
    if (lo == st.lo) & (hi == st.hi) & !cx.all {
        st.row0 = row0;
        return n as i32 * row_h;
    }
    let src = (at, <R as Row<V>>::LEAVES, size_of::<R>() as u32);
    if hi - lo > st.cap {
        // the window outgrew the ring: its columns again, larger
        list_reset(st, cx, hi - lo);
        st.lay = (gx, gy, gw);
        st.geo = (gx, gy, row_h);
        row_new::<V, R, B>(st, &row0, &f(lo as usize));
        st.src = Some(src);
        st.tmpl = row0.clone();
    }
    let mk = list_plan(st, cx, &l, (lo, hi), org, space, <R as Row<V>>::LEAVES as usize, src);
    st.row0 = row0;
    let (sites, mask) = (st.sites.as_ptr(), st.cap - 1);
    let plan = core::mem::take(&mut st.plan);
    for &e in &plan {
        let r = e & 0x3fff_ffff;
        let row = f(r as usize);
        // SAFETY: the sites were discovered from a row of type `R` (`list_plan` checked its source and shape);
        // `r & mask` is a slot of the list.
        unsafe {
            if e >> 31 != 0 {
                if e >> 30 & 1 != 0 {
                    // a row that entered: its own layout, into its lanes
                    let mut out = Lanes { sites, slot: r & mask, at: (gx, gy + r as i32 * row_h) };
                    bough_lay::place_row(&row, cx.scale, cx.u, gw as f32, &mut out);
                }
                row.write(sites, r & mask);
            } else {
                row.patch(sites, r & mask, &mk, cx);
            }
        }
    }
    st.plan = plan;
    if st.timed {
        st.resweep(cx.u, cx.tiles);
    }
    n as i32 * row_h
}

/// The first description of a list of elements: lay its first row out, make its columns from that.
#[cold]
#[inline(never)]
fn rows_new<V, R: Row<V> + bough_lay::Lay, F: FnMut(usize) -> R, B: Backend>(parent: &mut Store, cx: &mut Cx<'_>, n: u32, g: Geo, f: &mut F, at: &'static core::panic::Location<'static>) {
    let space = parent.space;
    let mut rects = vec![[0i32; 4]; <R as bough_lay::Lay>::LEAVES];
    let mut row_h = 0;
    let first = if n > 0 { Some(f(0)) } else { None };
    if let Some(row) = &first {
        row_h = bough_lay::place_row(row, cx.scale, cx.u, g.w as f32, &mut rects[..]).0;
    }
    let rows = if row_h > 0 { (space.clip.y1 - space.clip.y0) / row_h + 2 } else { 64 };
    let mut st = Store::new((rows as u32).next_power_of_two().max(64), space);
    st.row0 = rects.iter().map(|r| Rect { x0: r[0], y0: r[1], x1: r[2], y1: r[3] }).collect();
    st.lay = (g.x, g.y, g.w);
    st.geo = (g.x, g.y, row_h);
    if let Some(row) = &first {
        let shape = st.row0.clone();
        row_new::<V, R, B>(&mut st, &shape, row);
        st.src = Some((at, <R as Row<V>>::LEAVES, size_of::<R>() as u32));
    }
    // `list_plan` takes the layout as new the first time: nothing is exposed yet
    st.tmpl = st.row0.clone();
    st.tmpl_at = st.row0.as_ptr() as usize;
    parent.order.push(Pass::Sub(parent.subs.len() as u32));
    parent.subs.push(st);
    crate::store::SHAPE.fetch_add(1, Ordering::Relaxed);
}

/// A list of elements is somewhere else or of another width: its first row's layout again, and every exposed
/// row with it.
#[cold]
#[inline(never)]
fn rows_shape<V, R: Row<V> + bough_lay::Lay, F: FnMut(usize) -> R>(st: &mut Store, cx: &mut Cx<'_>, n: u32, g: Geo, f: &mut F) {
    let mut rects = vec![[0i32; 4]; <R as bough_lay::Lay>::LEAVES];
    let row_h = if n > 0 { bough_lay::place_row(&f(0), cx.scale, cx.u, g.w as f32, &mut rects[..]).0 } else { 0 };
    st.row0 = rects.iter().map(|r| Rect { x0: r[0], y0: r[1], x1: r[2], y1: r[3] }).collect();
    st.lay = (g.x, g.y, g.w);
    // force `list_plan` to see another layout: the window is emptied and every exposed row comes in again
    st.tmpl.clear();
    st.geo = (g.x, g.y, row_h);
    if st.hi > st.lo {
        cx.tiles.mark(st.space.clip);
    }
    st.row_live.fill(0);
    (st.lo, st.hi) = (0, 0);
}

/// Do the leaves of a row match the columns of a list, type by type?
struct Same<'a> {
    st: &'a Store,
    k: usize,
    ok: bool,
}
impl<V> Leaf<V> for Same<'_> {
    #[inline(always)]
    fn leaf<A: Arch>(&mut self, _: &A, _: TypeId, hov: Option<Hov>) {
        let fits = self.st.sites.get(self.k).is_some_and(|s| {
            let c = &self.st.cols[s.col as usize];
            // The record type, and with it the record's layout. A handler is the same handler if its captures
            // have the layout the column's lane was made for (the same closure of a generic `view` is another
            // type in each sink's instantiation; the element layer's views are not generic and need no such
            // allowance).
            c.arch == TypeId::of::<A>()
                && match (c.hover, hov) {
                    (None, None) => true,
                    (Some(a), Some(b)) => a.kinds == b.kinds && a.caps == b.caps && a.caps_align == b.caps_align,
                    _ => false,
                }
        });
        self.ok &= fits;
        self.k += 1;
    }
    fn hole(&mut self) {
        self.k += 1;
    }
}

/// A list whose rows are of other record types than its columns, or whose window outgrew them: its columns
/// are dropped and made again by the rows now described. Cold.
#[cold]
#[inline(never)]
fn list_reset(st: &mut Store, cx: &mut Cx<'_>, window: u32) {
    cx.tiles.mark(st.space.clip);
    let cap = st.cap.max((window + 2).next_power_of_two()).max(64);
    let (space, seen) = (st.space, st.seen);
    *st = Store::new(cap, space);
    st.seen = seen;
    crate::store::SHAPE.fetch_add(1, Ordering::Relaxed);
}

#[cold]
#[inline(never)]
fn list_new(parent: &mut Store, l: &List<'_>) {
    let space = parent.space;
    let rows = (space.clip.y1 - space.clip.y0) / l.row_h + 2;
    let mut st = Store::new((rows as u32).next_power_of_two().max(64), space);
    st.seen = l.edits.ver;
    parent.order.push(Pass::Sub(parent.subs.len() as u32));
    parent.subs.push(st);
    crate::store::SHAPE.fetch_add(1, Ordering::Relaxed);
}

/// Decide what a list pass does, and do everything that does not need the row's type: replay the relation's
/// journal, move the window, lay out rows that entered (their rects are the shape's, shifted), damage rows that
/// entered, left or are other rows than before, and leave in `st.plan` the rows whose operands the caller must
/// write (top bit) or compare.
#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn list_plan(st: &mut Store, cx: &mut Cx<'_>, l: &List<'_>, (lo, hi): (u32, u32), org: (i32, i32), space: Space, leaves: usize, src: crate::store::RowSrc) -> Mk {
    let (from, upd, nupd) = l.edits.since(st.seen);
    // inserts and removes since the last pass, for whoever holds a row by its index (focus, a press)
    st.moved_ok = l.edits.moves_since(st.seen, &mut st.moved);
    st.moved_from = st.seen;
    #[cfg(feature = "mutant_journal")]
    let (from, nupd) = (u32::MAX, 0 * nupd);
    st.seen = l.edits.ver;
    st.space = space;
    st.n = l.n;
    st.plan.clear();
    let mk = Mk { org, clip: space.clip };
    assert!(hi - lo <= st.cap, "the window outgrew the list's capacity");
    assert!(l.shape.len() == leaves, "a row's layout has one rect per record");
    // The one check per batch: this list's rows are still the rows its columns were made for (same call site,
    // same shape).
    match st.src {
        Some(s) => assert!(s.1 == src.1 && s.2 == src.2 && (core::ptr::eq(s.0, src.0) || *s.0 == *src.0), "a list's rows changed shape"),
        None => assert!(lo == hi),
    }
    let geo = (l.x, l.y, l.row_h);
    let (mut olo, mut ohi) = (st.lo, st.hi);
    let row_rect = |r: u32| Rect { x0: space.clip.x0, y0: org.1 + l.y + r as i32 * l.row_h, x1: space.clip.x1, y1: org.1 + l.y + (r as i32 + 1) * l.row_h }.and(space.clip);
    // The layout is the same if it is the same slice (the usual case: one layout per shape, kept by whoever
    // computed it); only a different slice is compared by content.
    // The layout is compared by content: a slice kept by its owner may have been written in place.
    st.tmpl_at = l.shape.as_ptr() as usize;
    if st.geo != geo || st.tmpl != l.shape {
        // the layout of the rows changed: every exposed row is laid out again
        st.geo = geo;
        st.tmpl.clear();
        st.tmpl.extend_from_slice(l.shape);
        if ohi > olo {
            cx.tiles.mark(space.clip);
        }
        cx.changed += 1;
        (olo, ohi) = (0, 0);
    }
    // rows entered, left or became other rows: what a reader of the stores sees moved (rows only compared
    // count where `replace` stores one)
    cx.changed += ((lo != olo) | (hi != ohi) | (from != u32::MAX)) as u32;
    if (lo == olo) & (hi == ohi) & (from == u32::MAX) & !cx.all {
        // only updates of single rows: those and nothing else
        for &r in &upd[..nupd] {
            if (r >= lo) & (r < hi) {
                st.plan.push(r);
            }
        }
        return mk;
    }
    // rows that left: what they showed is gone (a no-op when they left by scrolling out of the clip)
    for r in olo..ohi {
        if (r < lo) | (r >= hi) {
            cx.tiles.mark(row_rect(r));
        }
    }
    let mask = st.cap - 1;
    for r in lo..hi {
        let fresh = (r < olo) | (r >= ohi);
        if fresh | (r >= from) {
            // A row that is new here, or another row than was here: its records are written without being
            // compared, and the row is damaged as a whole.
            if fresh {
                let at = (l.x, l.y + r as i32 * l.row_h);
                for (s, t) in st.sites[..leaves].iter().zip(l.shape) {
                    // SAFETY: a site of this list; the slot is under `cap`.
                    unsafe { s.set_rect(r & mask, t.shift(at)) }
                }
            }
            cx.tiles.mark(row_rect(r));
            st.plan.push(r | 1 << 31 | (fresh as u32) << 30);
        } else if cx.all | upd[..nupd].contains(&r) {
            st.plan.push(r);
        }
    }
    st.lo = lo;
    st.hi = hi;
    st.row_live.fill(0);
    for r in lo..hi {
        let s = r & mask;
        st.row_live[s as usize >> 6] |= 1 << (s & 63);
    }
    mk
}
