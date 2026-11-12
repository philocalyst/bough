//! `View::changed`: the one effect site. After a turn's settle the root owner is shown what crossed its
//! boundaries this turn: the delta logs of its lists, the entities edited, the readers whose cohort
//! committed. It may write state in return (another wave settles, bounded). Its own signals are not on
//! the view: a value the owner wrote itself is handled where it was written (the contract's
//! boundary-only self-delta).
use crate::entity::EWired;
use crate::list::{LWired, Op};
use crate::reader::RWired;
use std::marker::PhantomData;

/// The publications of one turn (flat buffers, reused).
#[derive(Default)]
pub struct Pubs {
    /// Owners other than the root whose boundary cells published (owner, class): their `changed` runs too.
    pub(crate) owners: Vec<(usize, u32)>,
    lists: Vec<(usize, u32, u32)>,
    ops: Vec<Op>,
    ents: Vec<(usize, u32, u32)>,
    idx: Vec<u32>,
    /// Set by the default `View::changed` (`Changed::unhandled`): the owner has no effect site, so showing
    /// it the publications was not a handler and nothing of it needs describing again.
    pub(crate) unhandled: std::cell::Cell<bool>,
}

impl Pubs {
    pub(crate) fn clear(&mut self) {
        self.owners.clear();
        self.lists.clear();
        self.ops.clear();
        self.ents.clear();
        self.idx.clear();
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.lists.is_empty() && self.ents.is_empty()
    }
    pub(crate) fn owner(&mut self, owner: usize, cls: u32) {
        if cls != 0 && !self.owners.contains(&(owner, cls)) {
            self.owners.push((owner, cls));
        }
    }
    pub(crate) fn list(&mut self, addr: usize, ops: &[Op]) {
        if self.lists.iter().any(|l| l.0 == addr) || ops.is_empty() {
            return;
        }
        self.lists.push((addr, self.ops.len() as u32, ops.len() as u32));
        self.ops.extend_from_slice(ops);
    }
    pub(crate) fn ents(&mut self, addr: usize, idx: &[u32]) {
        if self.ents.iter().any(|l| l.0 == addr) || idx.is_empty() {
            return;
        }
        self.ents.push((addr, self.idx.len() as u32, idx.len() as u32));
        self.idx.extend_from_slice(idx);
    }
}

/// What crossed the owner's boundaries this turn (see the module docs).
pub struct Changed<'a, V> {
    pub(crate) pubs: &'a Pubs,
    pub(crate) readers: &'a [usize],
    pub(crate) _v: PhantomData<fn(&V)>,
}

impl<V> Changed<'_, V> {
    /// The default `View::changed` calls this and nothing else: the owner did not handle the publications.
    #[doc(hidden)]
    pub fn unhandled(&self) {
        self.pubs.unhandled.set(true);
    }
    pub fn any(&self) -> bool {
        !self.pubs.is_empty() || !self.readers.is_empty()
    }
    /// The list's delta log this turn (splices, moves, permutations, edits), if it changed.
    pub fn list<T>(&self, l: LWired<'_, T, V>) -> Option<&[Op]> {
        let a = l.list as *const _ as usize;
        let &(_, s, n) = self.pubs.lists.iter().find(|x| x.0 == a)?;
        Some(&self.pubs.ops[s as usize..(s + n) as usize])
    }
    /// The slab indices of the entities edited this turn.
    pub fn entities<T>(&self, e: EWired<'_, T, V>) -> Option<&[u32]> {
        let a = e.ents as *const _ as usize;
        let &(_, s, n) = self.pubs.ents.iter().find(|x| x.0 == a)?;
        Some(&self.pubs.idx[s as usize..(s + n) as usize])
    }
    /// Did this reader's cohort commit (a new value is shown) this turn?
    pub fn reader<T>(&self, r: RWired<'_, T, V>) -> bool {
        self.readers.contains(&(r.r as *const _ as usize))
    }
}

/// `T::changed` behind a type-erased owner pointer (registered per owner class by list regions and entity
/// views whose rows are `View`s).
///
/// # Safety
/// `owner` points at a live `T` that nothing else borrows.
pub(crate) unsafe fn changed_shim<T: crate::app::View>(owner: *mut u8, pubs: &Pubs, readers: &[usize]) {
    let d = Changed { pubs, readers, _v: PhantomData };
    // SAFETY: the function contract.
    unsafe { (*(owner as *mut T)).changed(d) };
}
