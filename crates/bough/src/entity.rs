//! `Entities<T>` / `Entity<T>`: independently owned state behind generational handles.
//!
//! The slab is carved up front (`with_capacity`: whole 64-entity chunks, allocated once and never moved),
//! so creating an entity allocates nothing and an entity is an owner at a stable address: views of it
//! (`entity_view`) wire its own signals exactly as a list row does. A handle is (index, generation): a
//! removed entity's handle answers `None` from then on, and its slot is reused only after the turn that
//! removed it has settled.
//!
//! Edits are logged once per entity per turn (a bit in the slot's meta word); the field's head (first
//! field, `#[repr(C)]`) is queued, and its wire settles exactly the edited entities that are displayed.
use crate::signal::WireHead;
use crate::elem::{Element, Mounter};
use crate::rt::*;
use crate::signal::Headed;
use bough_core::RecordInit;
use std::cell::{Cell, RefCell};
use std::marker::PhantomData;

const CHUNK: usize = 64;
const LIVE: u32 = 1 << 31;
const EDITED: u32 = 1 << 30;
const GEN: u32 = EDITED - 1;

pub struct Entity<T> {
    idx: u32,
    generation: u32,
    _t: PhantomData<fn() -> T>,
}
impl<T> Clone for Entity<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for Entity<T> {}
impl<T> PartialEq for Entity<T> {
    fn eq(&self, o: &Self) -> bool {
        self.idx == o.idx && self.generation == o.generation
    }
}
impl<T> Eq for Entity<T> {}
impl<T> std::fmt::Debug for Entity<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Entity({}#{})", self.idx, self.generation)
    }
}

#[repr(C)]
pub struct Entities<T> {
    head: crate::signal::WireHead,
    chunks: Vec<crate::list::Chunk<T>>,
    meta: Vec<u32>,
    free: Vec<u32>,
    pending_free: Vec<u32>,
    edits: Vec<u32>,
    /// Offsets of `T`'s wired fields (cleared from a removed entity before it leaves).
    pub(crate) wired: RefCell<Vec<u32>>,
    pub(crate) registered: Cell<bool>,
}

// SAFETY: `#[repr(C)]`, the head first.
unsafe impl<T> Headed for Entities<T> {
    type At<'a, V>
        = EWired<'a, T, V>
    where
        T: 'a;
    unsafe fn at<'a, V>(&'a self, off: u32) -> EWired<'a, T, V> {
        EWired { ents: self, off, _v: PhantomData }
    }
}

/// An entities operand of owner `V` (`sig!(self.docs)`).
pub struct EWired<'a, T, V> {
    pub(crate) ents: &'a Entities<T>,
    pub(crate) off: u32,
    _v: PhantomData<fn(&V)>,
}
impl<T, V> Clone for EWired<'_, T, V> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T, V> Copy for EWired<'_, T, V> {}

impl<T> Drop for Entities<T> {
    fn drop(&mut self) {
        for (i, &m) in self.meta.iter().enumerate() {
            if m & LIVE != 0 {
                // SAFETY: a live slot is initialised.
                unsafe { self.ptr(i as u32).drop_in_place() };
            }
        }
    }
}

impl<T> Entities<T> {
    /// A slab of `cap` entities, carved now (growing past it allocates whole chunks).
    pub fn with_capacity(cap: usize) -> Entities<T> {
        let mut e = Entities { head: WireHead::new(), chunks: Vec::new(), meta: Vec::new(), free: Vec::new(), pending_free: Vec::new(), edits: Vec::new(), wired: Default::default(), registered: Cell::new(false) };
        e.carve(cap.div_ceil(CHUNK).max(1));
        e
    }
    fn carve(&mut self, chunks: usize) {
        for _ in 0..chunks {
            let base = self.meta.len() as u32;
            self.chunks.push(crate::list::Chunk::new());
            self.meta.extend(std::iter::repeat_n(0, CHUNK));
            self.free.extend((base..base + CHUNK as u32).rev());
        }
    }
    #[inline(always)]
    pub(crate) fn ptr(&self, i: u32) -> *mut T {
        self.chunks[i as usize / CHUNK].slot(i as usize % CHUNK)
    }
    #[inline(always)]
    pub(crate) fn head(&self) -> &Cell<u32> {
        &self.head
    }
    fn live(&self, e: Entity<T>) -> bool {
        self.meta.get(e.idx as usize).is_some_and(|&m| m & LIVE != 0 && m & GEN == e.generation)
    }
    pub fn insert(&mut self, v: T) -> Entity<T> {
        if self.free.is_empty() {
            self.carve(1);
        }
        let i = self.free.pop().unwrap();
        // SAFETY: a free slot holds no value.
        unsafe { self.ptr(i).write(v) };
        let m = &mut self.meta[i as usize];
        *m = (*m & GEN) | LIVE;
        Entity { idx: i, generation: *m & GEN, _t: PhantomData }
    }
    /// Take the entity out (its handle dies). A view still showing it keeps its last pixels: its wires
    /// are cut here.
    pub fn remove(&mut self, e: Entity<T>) -> Option<T> {
        if !self.live(e) {
            return None;
        }
        let p = self.ptr(e.idx);
        for &off in self.wired.borrow().iter() {
            // SAFETY: `wired` holds `offset_of!`-proven headed fields of `T`.
            unsafe { (*((p as *const u8).add(off as usize) as *const Cell<u32>)).set(0) };
        }
        let m = &mut self.meta[e.idx as usize];
        *m = ((*m & GEN) + 1) & GEN;
        if self.head.get() != 0 {
            self.pending_free.push(e.idx);
        } else {
            self.free.push(e.idx);
        }
        // SAFETY: the slot was live; it is marked free above.
        Some(unsafe { p.read() })
    }
    /// A plain read while an owner describes itself: inserts and removals do not queue the head, so the
    /// owner describes itself again after every turn while it reads one plainly (`grain.rs`).
    #[inline(always)]
    fn read(&self) {
        crate::grain::read(&self.head.0, crate::grain::Dep::Always, crate::grain::wake_root);
    }
    /// A scoped, generation-checked read.
    pub fn get(&self, e: Entity<T>) -> Option<&T> {
        self.read();
        self.get_raw(e)
    }
    /// `get` for the runtime (an entity view's own mount: not its owner's plain read).
    #[inline(always)]
    pub(crate) fn get_raw(&self, e: Entity<T>) -> Option<&T> {
        // SAFETY: a live slot is initialised.
        self.live(e).then(|| unsafe { &*self.ptr(e.idx) })
    }
    pub fn inspect<R>(&self, e: Entity<T>, f: impl FnOnce(&T) -> R) -> Option<R> {
        self.get(e).map(f)
    }
    /// Edit an entity: its views' queued wires run at the turn's settle.
    pub fn edit(&mut self, e: Entity<T>) -> Option<&mut T> {
        if !self.live(e) {
            return None;
        }
        let m = self.meta[e.idx as usize];
        let h = self.head.get();
        if m & EDITED == 0 && h != 0 {
            self.meta[e.idx as usize] = m | EDITED;
            self.edits.push(e.idx);
            crate::signal::queue(&self.head);
        }
        // SAFETY: live; `&mut self` is the only borrow.
        Some(unsafe { &mut *self.ptr(e.idx) })
    }
    pub fn len(&self) -> usize {
        self.read();
        self.meta.iter().filter(|&&m| m & LIVE != 0).count()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// The entities edited this turn (`changed`).
    pub fn edited(&self) -> impl Iterator<Item = Entity<T>> + '_ {
        self.read();
        self.edits.iter().map(move |&i| Entity { idx: i, generation: self.meta[i as usize] & GEN, _t: PhantomData })
    }
    pub(crate) fn edits_raw(&self) -> &[u32] {
        &self.edits
    }
    pub(crate) fn end_turn(&mut self) {
        for &i in &self.edits {
            self.meta[i as usize] &= !EDITED;
        }
        self.edits.clear();
        self.free.append(&mut self.pending_free);
        self.registered.set(false);
    }
    pub(crate) fn is_live_idx(&self, i: u32) -> bool {
        self.meta[i as usize] & LIVE != 0
    }
}

// =============================================================================== views

pub struct EView {
    pub(crate) cls: u32,
    pub(crate) idx: u32,
    /// The owner whose description mounted the view (the entities are its field at `off`): its path and
    /// type; and `Entities::edit`, type-erased (`rt::OwnerRef`, `Rt::owner_mut`).
    pub(crate) own: OwnerRef,
    pub(crate) own_ty: std::any::TypeId,
    pub(crate) off: u32,
    pub(crate) edit: unsafe fn(*mut u8, u32, u32) -> *mut u8,
}

/// # Safety
/// `e` points at a live `Entities<T>` reached through a unique borrow of its owner.
unsafe fn ents_edit<T>(e: *mut u8, idx: u32, generation: u32) -> *mut u8 {
    // SAFETY: the function contract.
    let e = unsafe { &mut *(e as *mut Entities<T>) };
    match e.edit(Entity { idx, generation, _t: PhantomData }) {
        Some(x) => x as *mut T as *mut u8,
        None => std::ptr::null_mut(),
    }
}

/// # Safety
/// `p` is a live `Entities<T>`.
unsafe fn end_ents<T>(p: *const u8) {
    unsafe { (*(p as *mut Entities<T>)).end_turn() };
}

/// # Safety
/// Wire contract: `sig` is an `Entities<T>` field of the live owner; `w.node` an `EView` index.
unsafe fn patch_entities<T: 'static>(rt: &mut Rt, w: &Wire, sig: *const u8, owner: *const u8) {
    let e = unsafe { &*(sig as *const Entities<T>) };
    if !e.registered.get() {
        e.registered.set(true);
        rt.lists.ends.push((sig, end_ents::<T>));
        rt.pubs.ents(sig as usize, e.edits_raw());
        rt.pubs.owner(owner as usize, w.cls);
    }
    let (cls, idx) = {
        let v = &rt.eviews[w.node as usize];
        (v.cls, v.idx)
    };
    if e.edits_raw().contains(&idx) && e.is_live_idx(idx) {
        // SAFETY: a live entity at its slab address; its wires were installed for class `cls`.
        unsafe { rt.settle_cls(e.ptr(idx) as *const u8, cls) };
    }
    let c = &rt.classes[cls as usize];
    if e.wired.borrow().len() < c.len() {
        *e.wired.borrow_mut() = c.clone();
    }
}

/// A view of one entity: `T::view` mounted with the entity as its owner.
pub struct EntityView<'a, T, V> {
    ents: EWired<'a, T, V>,
    e: Entity<T>,
}
pub fn entity_view<'a, T: crate::app::View, V>(ents: EWired<'a, T, V>, e: Entity<T>) -> EntityView<'a, T, V> {
    EntityView { ents, e }
}

impl<'a, T: crate::app::View, V: 'static> Element<V> for EntityView<'a, T, V> {
    fn mount(self, m: &mut Mounter<'_, V>) {
        if m.taping() {
            // in a re-describing owner: a keyed dynamic part of one item, replaced when the entity changes
            return crate::describe::mount_dyn(m, std::iter::once(Keyed(self)));
        }
        self.mount_in(m);
    }
    fn update(self, m: &mut Mounter<'_, V>) {
        crate::describe::update_dyn(m, std::iter::once(Keyed(self)));
    }
}

/// The entity view as a dynamic item keyed by its entity (`Element::dyn_key`).
struct Keyed<'a, T, V>(EntityView<'a, T, V>);
impl<'a, T: crate::app::View, V: 'static> Element<V> for Keyed<'a, T, V> {
    /// The tape: the counts of an opaque element (records, tape entries, layout children: what a
    /// re-description steps over when the entity is gone), then the view, then the entity's own entries.
    fn mount(self, m: &mut Mounter<'_, V>) {
        crate::describe::mount_opaque(m, |m| self.0.mount_in(m));
    }
    /// The same entity: its view describes itself again in lockstep, as the entity's own owner (its plain
    /// fields, its routes' captures). A removed entity's view keeps its last pixels: stepped over.
    fn update(self, m: &mut Mounter<'_, V>) {
        let at = m.ti;
        let Some(p) = self.0.ents.ents.get_raw(self.0.e) else { return crate::describe::update_opaque(m) };
        let p = p as *const T;
        m.ti += 3;
        let ev = m.tape_next();
        let cls = m.rt.eviews[ev as usize].cls;
        let own = OwnerRef::entity(ev, self.0.e.idx, self.0.e.generation);
        let (pos0, kid0) = (m.pos, m.kid);
        let recs: *mut Vec<RecordInit> = m.recs;
        let scope: *mut Scope = m.scope;
        let (ti, pos, kid) = {
            // SAFETY: reborrows of the mounter's own buffer and scope for the sub-walk, as in `mount_in`.
            let mut sub = Mounter { rt: &mut *m.rt, owner: p, recs: unsafe { &mut *recs }, scope: unsafe { &mut *scope }, depth: m.depth, ox: m.ox, oy: m.oy, space: m.space, clip: m.clip, pending_num: None, cls, tape: m.tape, ti: m.ti, pos: m.pos, kid: m.kid, keep: false, own };
            // SAFETY: `p` is the live entity (checked above; its slab never moves).
            unsafe { &*p }.view().update(&mut sub);
            (sub.ti, sub.pos, sub.kid)
        };
        (m.ti, m.pos, m.kid) = (ti, pos, kid);
        // a dynamic part of the entity's view may have grown or shrunk
        m.tape_set(at, (m.pos - pos0) as u32);
        m.tape_set(at + 1, (m.ti - at - 3) as u32);
        m.tape_set(at + 2, (m.kid - kid0) as u32);
    }
    fn dyn_key(&self) -> u64 {
        (self.0.e.idx as u64) << 32 | self.0.e.generation as u64 | 1 << 63
    }
}

impl<'a, T: crate::app::View, V: 'static> EntityView<'a, T, V> {
    fn mount_in(self, m: &mut Mounter<'_, V>) {
        let ents = self.ents.ents;
        let p = ents.get_raw(self.e).expect("entity_view of a removed entity") as *const T;
        let cls = m.rt.new_class();
        m.rt.class_changed[cls as usize] = Some(crate::changed::changed_shim::<T>);
        let ev = m.rt.eviews.len() as u32;
        m.rt.eviews.push(EView { cls, idx: self.e.idx, own: m.own, own_ty: std::any::TypeId::of::<V>(), off: self.ents.off, edit: ents_edit::<T> });
        let own = OwnerRef::entity(ev, self.e.idx, self.e.generation);
        m.tape_push(ev);
        // SAFETY: `self.ents` is an `offset_of!`-proven `Entities<T>` field of `V`; `patch_entities::<T>` reads one.
        unsafe { m.wire_cell(ents.head(), self.ents.off, patch_entities::<T>, ev, [0; 16]) };
        let recs: *mut Vec<RecordInit> = m.recs;
        let scope: *mut Scope = m.scope;
        {
            // the entity is the owner of what its view mounts (its own class and path); resources go to this
            // scope, and in a re-describing owner its entries to this tape
            // SAFETY: reborrows of the mounter's own buffer and scope for the sub-mount.
            let mut sub = Mounter { rt: &mut *m.rt, owner: p, recs: unsafe { &mut *recs }, scope: unsafe { &mut *scope }, depth: m.depth, ox: m.ox, oy: m.oy, space: m.space, clip: m.clip, pending_num: None, cls, tape: m.tape, ti: 0, pos: 0, kid: 0, keep: false, own };
            // SAFETY: `p` is the live entity (its slab never moves).
            unsafe { &*p }.view().mount(&mut sub);
        }
        *ents.wired.borrow_mut() = m.rt.classes[cls as usize].clone();
    }
}
