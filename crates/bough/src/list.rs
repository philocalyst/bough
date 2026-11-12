//! `List<T>`: keyed rows with durable [`Slot`] identity, and the per-turn delta log its regions replay.
//!
//! # Representation
//! * **Rows never move in memory.** Items live in a slab of 64-item chunks (allocated when the slab grows,
//!   never moved or freed while the list lives), so a row is an *owner* at a stable address: the wires a
//!   row's description installs (`sig!(self.label)` with `Self = T`) read `row + offset_of!(T, label)`
//!   for the row's whole life, whatever happens to its position.
//! * **Order is a vector of slot ids** (`order[pos] = slot`). A splice, a move or a permutation edits
//!   that vector; no item is copied.
//! * **Identity is a generational slot** (`Slot<T>`: index + generation, `Copy`): taking an item bumps the
//!   generation, so a stale slot answers `None`, never another item.
//! * **The delta log.** Each op appends what a region needs to replay it on its own mounted rows:
//!   `Insert { at, slot }`, `Remove { at }`, `Move { from, to }`, `Permute { at, n }` (the new slot
//!   order of positions `at..at+n`, kept in a side vector) and `Edit { slot }` (once per slot per turn:
//!   a bit in the slot's meta word). The log is kept only while the list is displayed (its head has
//!   wires) and is cleared when the turn's settle is over, after every region showing it has replayed
//!   it. A freed slot is reused only after that (`pending_free`), so a log entry never names two items.
//!
//! The list's own head (first field, `#[repr(C)]`) is a wire head like a `Signal`'s: every op sets
//! `QUEUED` when the list is displayed, and the root's settle scan runs the region's wire.
use crate::signal::WireHead;
use crate::signal::Headed;
use std::cell::Cell;
use std::marker::PhantomData;
use std::mem::MaybeUninit;

const CHUNK: usize = 64;
const NONE: u32 = u32::MAX;
/// Meta word of a slot: generation in the low 30 bits, LIVE and EDITED above.
const LIVE: u32 = 1 << 31;
const EDITED: u32 = 1 << 30;
const GEN: u32 = EDITED - 1;

/// Durable identity of one item of a `List<T>`: valid until the item is taken out.
pub struct Slot<T> {
    pub(crate) idx: u32,
    pub(crate) generation: u32,
    _t: PhantomData<fn() -> T>,
}
impl<T> Clone for Slot<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for Slot<T> {}
impl<T> PartialEq for Slot<T> {
    fn eq(&self, o: &Self) -> bool {
        self.idx == o.idx && self.generation == o.generation
    }
}
impl<T> Eq for Slot<T> {}
impl<T> std::fmt::Debug for Slot<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Slot({}#{})", self.idx, self.generation)
    }
}
impl<T> Slot<T> {
    pub fn index(&self) -> u32 {
        self.idx
    }
    /// A slot from its parts (a route's owner path, `rt::OwnerRef`).
    pub(crate) fn raw(idx: u32, generation: u32) -> Slot<T> {
        Slot { idx, generation, _t: PhantomData }
    }
}

/// One replayable step of a turn's changes (positions are those of the list at the time of the op).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Insert { at: u32, slot: u32 },
    Remove { at: u32, slot: u32 },
    /// The item at `from` ends at `to` in the resulting sequence.
    Move { from: u32, to: u32 },
    /// Positions `at..at+n` now hold `perm[off..off+n]` (slot ids), a permutation of what they held.
    Permute { at: u32, n: u32, off: u32 },
    Edit { slot: u32 },
    /// Item flags changed this turn (`set_flag`, `set_flag_range`, `clear_flag`): logged once per turn; a
    /// region compares its mounted rows' flags with their items'.
    Flags,
}

/// A slab chunk: 64 item slots allocated once and never moved, held by its raw pointer (from
/// `Box::into_raw`), so a slot's address carries the allocation's own provenance: writing an item through
/// it (`List::edit`, a wire's owner) never goes through a shared borrow of the list.
pub(crate) struct Chunk<T>(std::ptr::NonNull<[MaybeUninit<T>; CHUNK]>);
impl<T> Chunk<T> {
    pub(crate) fn new() -> Chunk<T> {
        // SAFETY: an array of `MaybeUninit` needs no initialisation.
        let b: Box<[MaybeUninit<T>; CHUNK]> = Box::new(unsafe { MaybeUninit::<[MaybeUninit<T>; CHUNK]>::uninit().assume_init() });
        Chunk(std::ptr::NonNull::from(Box::leak(b)))
    }
    /// Slot `i < CHUNK` (possibly uninitialised: the slab's metadata says).
    #[inline(always)]
    pub(crate) fn slot(&self, i: usize) -> *mut T {
        debug_assert!(i < CHUNK);
        // SAFETY: in bounds of the chunk's allocation.
        unsafe { (self.0.as_ptr() as *mut MaybeUninit<T>).add(i) as *mut T }
    }
}
impl<T> Drop for Chunk<T> {
    fn drop(&mut self) {
        // SAFETY: allocated by `Box` in `new`, freed once (the slots are `MaybeUninit`: the slab dropped its
        // live items before).
        drop(unsafe { Box::from_raw(self.0.as_ptr()) });
    }
}

#[repr(C)]
pub struct List<T> {
    head: crate::signal::WireHead,
    chunks: Vec<Chunk<T>>,
    meta: Vec<u32>,
    order: Vec<u32>,
    free: Vec<u32>,
    pending_free: Vec<u32>,
    log: Vec<Op>,
    perm: Vec<u32>,
    /// Offsets of the row fields that carry wires (a row region's owner class), so an item taken out
    /// leaves with clean heads. Written by the regions as rows mount.
    pub(crate) wired: std::cell::RefCell<Vec<u32>>,
    /// Some region's patch has registered this list for the end-of-settle cleanup this turn.
    pub(crate) registered: Cell<bool>,
    /// The plain bytes of the items edited this turn as they were before their first edit (`grain.rs`: a row
    /// whose plain bytes did not change does not describe itself again), and where each slot's start in it
    /// (`u32::MAX`: unknown, the row describes itself again). Stale after `end_turn`, cleared by the next edit.
    snap: Vec<u8>,
    snap_at: Vec<(u32, u32)>,
    snap_stale: bool,
    /// Per slot: the item's state flags (`flags.rs`: selected, checked, expanded, search hits, ...), shown by
    /// its row and inherited by the row's subtree. Data of the item, kept by slot (a move does not touch it).
    fl: Vec<u16>,
    /// `Op::Flags` was logged this turn.
    fl_noted: bool,
}

// SAFETY: `#[repr(C)]` with the head first.
unsafe impl<T> Headed for List<T> {
    type At<'a, V>
        = LWired<'a, T, V>
    where
        T: 'a;
    unsafe fn at<'a, V>(&'a self, off: u32) -> LWired<'a, T, V> {
        LWired { list: self, off, _v: PhantomData }
    }
}

/// A list operand of owner type `V` at a compiler-proven offset (`sig!(self.items)`).
pub struct LWired<'a, T, V> {
    pub(crate) list: &'a List<T>,
    pub(crate) off: u32,
    _v: PhantomData<fn(&V)>,
}
impl<T, V> Clone for LWired<'_, T, V> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T, V> Copy for LWired<'_, T, V> {}
impl<'a, T, V> LWired<'a, T, V> {
    pub fn get(&self) -> &'a List<T> {
        self.list
    }
}

impl<T> Default for List<T> {
    fn default() -> Self {
        List::new()
    }
}

impl<T> Drop for List<T> {
    fn drop(&mut self) {
        for &s in &self.order {
            // SAFETY: every slot in `order` is live and initialised.
            unsafe { self.slot_ptr(s).drop_in_place() };
        }
    }
}

impl<T> List<T> {
    pub fn new() -> List<T> {
        List { head: WireHead::new(), chunks: Vec::new(), meta: Vec::new(), order: Vec::new(), free: Vec::new(), pending_free: Vec::new(), log: Vec::new(), perm: Vec::new(), wired: Default::default(), registered: Cell::new(false), snap: Vec::new(), snap_at: Vec::new(), snap_stale: false, fl: Vec::new(), fl_noted: false }
    }
    pub fn with_capacity(n: usize) -> List<T> {
        let mut l = List::new();
        l.order.reserve(n);
        l.meta.reserve(n);
        l.chunks.reserve(n.div_ceil(CHUNK));
        l
    }
    pub fn from_iter_in(it: impl IntoIterator<Item = T>) -> List<T> {
        let it = it.into_iter();
        // reserved once (100,000 items: no doubling growth of the order and meta columns)
        let mut l = List::with_capacity(it.size_hint().0);
        for v in it {
            l.push(v);
        }
        l
    }

    #[inline(always)]
    pub(crate) fn head(&self) -> &Cell<u32> {
        &self.head
    }
    #[inline(always)]
    fn displayed(&self) -> bool {
        self.head.get() != 0
    }
    #[inline(always)]
    fn note(&mut self, op: Op) {
        if self.displayed() {
            self.log.push(op);
            crate::signal::queue(&self.head);
        }
    }

    #[inline(always)]
    pub(crate) fn slot_ptr(&self, s: u32) -> *mut T {
        self.chunks[s as usize / CHUNK].slot(s as usize % CHUNK)
    }
    fn alloc_slot(&mut self, v: T) -> u32 {
        let s = match self.free.pop() {
            Some(s) => s,
            None => {
                let s = self.meta.len() as u32;
                if s as usize % CHUNK == 0 {
                    self.chunks.push(Chunk::new());
                }
                self.meta.push(0);
                self.fl.push(0);
                s
            }
        };
        self.fl[s as usize] = 0;
        // SAFETY: a free slot holds no value.
        unsafe { self.slot_ptr(s).write(v) };
        self.meta[s as usize] = (self.meta[s as usize] & GEN) | LIVE;
        s
    }
    fn slot_of(&self, s: u32) -> Slot<T> {
        Slot { idx: s, generation: self.meta[s as usize] & GEN, _t: PhantomData }
    }
    /// Move the item out of slot `s`; the slot's heads are cleared first (see `wired`).
    fn free_slot(&mut self, s: u32) -> T {
        let p = self.slot_ptr(s);
        for &off in self.wired.borrow().iter() {
            // SAFETY: `wired` holds `offset_of!`-proven headed fields of `T` (written by the row region).
            unsafe { (*((p as *const u8).add(off as usize) as *const Cell<u32>)).set(0) };
        }
        let m = &mut self.meta[s as usize];
        *m = ((*m & GEN) + 1) & GEN;
        if self.displayed() {
            self.pending_free.push(s);
        } else {
            self.free.push(s);
        }
        // SAFETY: the slot was live; it is marked free above and never read again until rewritten.
        unsafe { p.read() }
    }
    #[inline]
    fn live(&self, s: Slot<T>) -> bool {
        self.meta.get(s.idx as usize).is_some_and(|&m| m & LIVE != 0 && m & GEN == s.generation)
    }

    // ------------------------------------------------------------------ reads
    // A read while an owner describes itself subscribes the owner (`grain.rs`): to the list's shape (`len`,
    // `slot`, `position`, `slots`: inserts, removals, moves) or to its items as well (`get`, `get_slot`, `iter`).
    #[inline(always)]
    fn read_shape(&self) {
        crate::grain::read(&self.head.0, crate::grain::Dep::Shape, crate::listview::wake_list::<T>);
    }
    #[inline(always)]
    fn read_items(&self) {
        crate::grain::read(&self.head.0, crate::grain::Dep::Items, crate::listview::wake_list::<T>);
    }
    pub fn len(&self) -> usize {
        self.read_shape();
        self.order.len()
    }
    /// `len` read by the runtime (not a description's dependency).
    #[inline(always)]
    pub(crate) fn len_raw(&self) -> usize {
        self.order.len()
    }
    pub fn is_empty(&self) -> bool {
        self.read_shape();
        self.order.is_empty()
    }
    pub fn get(&self, pos: usize) -> Option<&T> {
        self.read_items();
        let &s = self.order.get(pos)?;
        // SAFETY: slots in `order` are live.
        Some(unsafe { &*self.slot_ptr(s) })
    }
    pub fn slot(&self, pos: usize) -> Option<Slot<T>> {
        self.read_shape();
        self.order.get(pos).map(|&s| self.slot_of(s))
    }
    pub fn get_slot(&self, s: Slot<T>) -> Option<&T> {
        self.read_items();
        // SAFETY: a live slot is initialised.
        self.live(s).then(|| unsafe { &*self.slot_ptr(s.idx) })
    }
    /// The position of `s` (a scan of the order vector), or `None` if it was taken out.
    pub fn position(&self, s: Slot<T>) -> Option<usize> {
        self.read_shape();
        if !self.live(s) {
            return None;
        }
        self.order.iter().position(|&x| x == s.idx)
    }
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &T> + '_ {
        self.read_items();
        // SAFETY: slots in `order` are live.
        self.order.iter().map(move |&s| unsafe { &*self.slot_ptr(s) })
    }
    pub fn slots(&self) -> impl ExactSizeIterator<Item = Slot<T>> + '_ {
        self.read_shape();
        self.order.iter().map(move |&s| self.slot_of(s))
    }

    // ------------------------------------------------------------------ item flags
    /// Is flag `f` set on the item at slot `s`? (A plain read: an owner describing itself subscribes to the
    /// list's items.)
    pub fn has_flag(&self, s: Slot<T>, f: crate::flags::Flag) -> bool {
        self.read_items();
        self.live(s) && self.fl[s.idx as usize] & f.bit() != 0
    }
    /// The item's flag word.
    pub(crate) fn flags_raw(&self, s: u32) -> u16 {
        self.fl[s as usize]
    }
    #[inline]
    fn note_flags(&mut self) {
        if !self.fl_noted && self.displayed() {
            self.fl_noted = true;
            self.note(Op::Flags);
        }
    }
    /// Set or clear flag `f` on the item at slot `s`: its row (if mounted) shows it at the turn's settle,
    /// with no description and no signal; a row mounted later shows it from its mount.
    pub fn set_flag(&mut self, s: Slot<T>, f: crate::flags::Flag, on: bool) {
        if !self.live(s) {
            return;
        }
        let w = &mut self.fl[s.idx as usize];
        let n = if on { *w | f.bit() } else { *w & !f.bit() };
        if n != *w {
            *w = n;
            self.note_flags();
        }
    }
    /// `set_flag` on the item at `pos`.
    pub fn set_flag_at(&mut self, pos: usize, f: crate::flags::Flag, on: bool) {
        let s = self.order[pos];
        self.set_flag(self.slot_of(s), f, on);
    }
    /// Set or clear flag `f` on the items at `range` (a shift-click selection, a "select all"): one store per
    /// item, and only the rows mounted now are written.
    pub fn set_flag_range(&mut self, range: std::ops::Range<usize>, f: crate::flags::Flag, on: bool) {
        let b = f.bit();
        let mut any = false;
        for &s in &self.order[range] {
            let w = &mut self.fl[s as usize];
            let n = if on { *w | b } else { *w & !b };
            any |= n != *w;
            *w = n;
        }
        if any {
            self.note_flags();
        }
    }
    /// Clear flag `f` on every item.
    pub fn clear_flag(&mut self, f: crate::flags::Flag) {
        let n = self.order.len();
        self.set_flag_range(0..n, f, false);
    }

    // ------------------------------------------------------------------ ops
    pub fn push(&mut self, v: T) -> Slot<T> {
        let at = self.order.len();
        self.insert(at, v)
    }
    pub fn insert(&mut self, at: usize, v: T) -> Slot<T> {
        assert!(at <= self.order.len(), "insert position out of range");
        let s = self.alloc_slot(v);
        self.order.insert(at, s);
        self.note(Op::Insert { at: at as u32, slot: s });
        self.slot_of(s)
    }
    pub fn extend(&mut self, it: impl IntoIterator<Item = T>) {
        for v in it {
            self.push(v);
        }
    }
    pub fn remove(&mut self, at: usize) -> T {
        let s = self.order.remove(at);
        self.note(Op::Remove { at: at as u32, slot: s });
        self.free_slot(s)
    }
    pub fn pop(&mut self) -> Option<T> {
        (!self.order.is_empty()).then(|| self.remove(self.order.len() - 1))
    }
    /// Take the item out (its slot dies: a later lookup answers `None`).
    pub fn take(&mut self, s: Slot<T>) -> Option<T> {
        let at = self.position(s)?;
        Some(self.remove(at))
    }
    pub fn clear(&mut self) {
        while !self.order.is_empty() {
            self.remove(self.order.len() - 1);
        }
    }
    pub fn truncate(&mut self, n: usize) {
        while self.order.len() > n {
            self.remove(self.order.len() - 1);
        }
    }
    /// Keep the items `f` accepts (in order).
    pub fn retain(&mut self, mut f: impl FnMut(&T) -> bool) {
        let mut i = 0;
        while i < self.order.len() {
            // SAFETY: slots in `order` are live.
            if f(unsafe { &*self.slot_ptr(self.order[i]) }) {
                i += 1;
            } else {
                drop(self.remove(i));
            }
        }
    }
    /// The item at `from` moves so that it ends at `to`.
    pub fn move_to(&mut self, from: usize, to: usize) {
        let n = self.order.len();
        assert!(from < n && to < n, "move out of range");
        if from == to {
            return;
        }
        let s = self.order.remove(from);
        self.order.insert(to, s);
        self.note(Op::Move { from: from as u32, to: to as u32 });
    }
    /// Move `s` so it sits right before `other` (`None`: at the end).
    pub fn move_before(&mut self, s: Slot<T>, other: Option<Slot<T>>) {
        let Some(from) = self.position(s) else { return };
        let to = match other.and_then(|o| self.position(o)) {
            Some(p) if p > from => p - 1,
            Some(p) => p,
            None => self.order.len() - 1,
        };
        self.move_to(from, to);
    }
    pub fn swap(&mut self, a: usize, b: usize) {
        if a == b {
            return;
        }
        self.permute_with(a.min(b), a.max(b) + 1, |o| o.swap(0, o.len() - 1));
    }
    pub fn reverse(&mut self) {
        let n = self.order.len();
        self.permute_with(0, n, |o| o.reverse());
    }
    pub fn sort_by(&mut self, mut f: impl FnMut(&T, &T) -> std::cmp::Ordering) {
        let n = self.order.len();
        let chunks = &self.chunks;
        let p = |s: u32| -> *const T { chunks[s as usize / CHUNK].slot(s as usize % CHUNK) };
        let mut o = self.order.clone();
        // SAFETY: slots in `order` are live.
        o.sort_by(|&a, &b| f(unsafe { &*p(a) }, unsafe { &*p(b) }));
        self.permute_with(0, n, |x| x.copy_from_slice(&o));
    }
    pub fn sort_by_key<K: Ord>(&mut self, mut f: impl FnMut(&T) -> K) {
        self.sort_by(|a, b| f(a).cmp(&f(b)));
    }
    fn permute_with(&mut self, lo: usize, hi: usize, f: impl FnOnce(&mut [u32])) {
        if hi <= lo + 1 {
            return;
        }
        f(&mut self.order[lo..hi]);
        if self.displayed() {
            let off = self.perm.len() as u32;
            self.perm.extend_from_slice(&self.order[lo..hi]);
            self.note(Op::Permute { at: lo as u32, n: (hi - lo) as u32, off });
        }
    }
    /// Edit the item at `pos`: its row's queued signals run (and a re-describing row re-describes) at the
    /// turn's settle.
    pub fn edit(&mut self, pos: usize) -> &mut T {
        let s = self.order[pos];
        self.mark_edit(s);
        // SAFETY: live slot; `&mut self` makes this the only borrow.
        unsafe { &mut *self.slot_ptr(s) }
    }
    pub fn edit_slot(&mut self, s: Slot<T>) -> Option<&mut T> {
        if !self.live(s) {
            return None;
        }
        self.mark_edit(s.idx);
        // SAFETY: as in `edit`.
        Some(unsafe { &mut *self.slot_ptr(s.idx) })
    }
    #[inline]
    /// The item at `pos` (reflection).
    pub(crate) fn item_ptr(&self, pos: usize) -> *mut T {
        self.slot_ptr(self.order[pos])
    }
    /// The item at `pos` was written in place (reflection): log it as an edit (its row describes itself
    /// again: the bytes before the write are gone).
    pub(crate) fn mark_edit_pos(&mut self, pos: usize) {
        let s = self.order[pos];
        self.mark_edit(s);
        if let Some(x) = self.snap_at.iter_mut().rev().find(|x| x.0 == s) {
            x.1 = u32::MAX;
        }
    }
    fn mark_edit(&mut self, s: u32) {
        let m = self.meta[s as usize];
        if m & EDITED == 0 && self.displayed() {
            self.meta[s as usize] = m | EDITED;
            self.note(Op::Edit { slot: s });
            // the item's plain bytes before its first edit this turn
            if std::mem::take(&mut self.snap_stale) {
                self.snap.clear();
                self.snap_at.clear();
            }
            let plan = crate::grain::plan::<T>();
            let at = if plan.exact {
                let at = self.snap.len() as u32;
                // SAFETY: a live slot holds an initialised `T`, borrowed by nothing (`&mut self`).
                unsafe { plan.snapshot(self.slot_ptr(s) as *const u8, &mut self.snap) };
                at
            } else {
                u32::MAX
            };
            self.snap_at.push((s, at));
        }
    }
    /// Are slot `s`'s plain bytes what they were before its first edit this turn (`grain.rs`)? `false` when
    /// that is not known.
    pub(crate) fn unchanged(&self, s: u32) -> bool {
        let Some(&(_, at)) = self.snap_at.iter().rev().find(|x| x.0 == s) else { return false };
        if at == u32::MAX || self.meta[s as usize] & LIVE == 0 {
            return false;
        }
        let plan = crate::grain::plan::<T>();
        // SAFETY: a live slot holds an initialised `T`; `at` starts a snapshot of `T`'s plan.
        unsafe { plan.same(self.slot_ptr(s) as *const u8, &self.snap[at as usize..at as usize + plan.bytes as usize]) }
    }
    /// Replace the item at `pos` (identity kept: the same slot, its row re-mounted).
    pub fn set(&mut self, pos: usize, v: T) -> T {
        let old = self.remove(pos);
        let s = self.alloc_slot(v);
        self.order.insert(pos, s);
        self.note(Op::Insert { at: pos as u32, slot: s });
        old
    }

    // ------------------------------------------------------------------ runtime side
    /// This turn's ops, and the permutation side vector.
    pub(crate) fn log(&self) -> (&[Op], &[u32]) {
        (&self.log, &self.perm)
    }
    pub(crate) fn order_slots(&self) -> &[u32] {
        &self.order
    }
    /// End of the turn's settle: drop the log, clear the edit bits, release the slots freed this turn.
    pub(crate) fn end_turn(&mut self) {
        for op in &self.log {
            if let Op::Edit { slot } = *op {
                self.meta[slot as usize] &= !EDITED;
            }
        }
        self.log.clear();
        self.perm.clear();
        self.free.append(&mut self.pending_free);
        self.registered.set(false);
        self.snap_stale = true;
        self.fl_noted = false;
    }
    #[inline]
    pub(crate) fn slot_live(&self, s: u32) -> bool {
        self.meta[s as usize] & LIVE != 0
    }
    /// The generation slot `s` has now.
    #[inline]
    pub(crate) fn slot_gen(&self, s: u32) -> u32 {
        self.meta[s as usize] & GEN
    }
}

/// Positional operations of a `Vec` oracle, for the differential tests (`tests/list.rs`).
#[doc(hidden)]
pub fn replay<X: Copy>(rows: &mut Vec<X>, op: Op, perm: &[u32], resolve: impl Fn(u32) -> X) {
    match op {
        Op::Insert { at, slot } => rows.insert(at as usize, resolve(slot)),
        Op::Remove { at, .. } => {
            rows.remove(at as usize);
        }
        Op::Move { from, to } => {
            let x = rows.remove(from as usize);
            rows.insert(to as usize, x);
        }
        Op::Permute { at, n, off } => {
            for i in 0..n as usize {
                rows[at as usize + i] = resolve(perm[off as usize + i]);
            }
        }
        Op::Edit { .. } | Op::Flags => {}
    }
}

#[allow(dead_code)]
const _: () = assert!(NONE == u32::MAX);

/// A list's current delta log (tests: replaying it on the previous order gives the current one).
#[doc(hidden)]
pub fn log_of<T>(l: &List<T>) -> (&[Op], &[u32]) {
    l.log()
}
