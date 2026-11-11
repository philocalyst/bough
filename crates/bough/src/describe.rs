//! Re-description: after an arbitrary handler the owner's `view` runs again and
//! the new description is walked **in lockstep with the retained records** (`Element::update`).
//!
//! A description emits its records in paint preorder, the order the Bough stores them, so the diff is a
//! linear merge, not a keyed reconciliation: the walk carries the Bough position of the next record (`pos`)
//! and the layout child index inside the current container (`kid`); each element reads its node id from
//! the owner's **tape** (recorded at mount, one `u32` per element, in the same preorder), compares its new
//! operands with the node's retained state and patches only what differs, through the same monomorphic
//! paths the wires use. A description's *type* fixes its shape, so inside a fixed shape nothing is matched
//! at all. Shape changes exist only where the type allows them, at **dynamic parts** (`Option<E>`,
//! `Vec<E>`, `each(..)`): those realign positionally (the contract's `Vec<T>` rung: items keep their
//! position, not an identity; `List<T>` is the keyed form) — item `k` updates item `k`, surplus items are
//! spliced out as one preorder run, new items are mounted and spliced in as one run.
use crate::elem::{Element, Mounter};
use crate::rt::*;
use bough_core::RecordInit;

/// One item of a dynamic part: its record and layout-child counts, the resources it owns, its own tape.
pub struct DynItem {
    pub key: u64,
    pub nrec: u32,
    pub nkid: u32,
    pub scope: Scope,
    pub tape: Vec<u32>,
}

#[derive(Default)]
pub struct Dyns {
    pub(crate) parts: Vec<Vec<DynItem>>,
    free: Vec<u32>,
}

impl Dyns {
    /// Dynamic parts (`each`, conditionals) as memory-ledger lines: one item per dynamic child, each with
    /// its scope and its re-description tape.
    pub fn ledger(&self, l: &mut bough_mem::Ledger) {
        use bough_mem::Scales::Content;
        let items: usize = self.parts.iter().map(|p| p.len()).sum();
        let blocks: usize = self.parts.iter().map(|p| p.capacity() * size_of::<DynItem>()).sum::<usize>() + self.parts.capacity() * 24 + self.free.capacity() * 4;
        l.many("describe", format!("dynamic items ({items})"), Content, blocks, blocks);
        let scopes: usize = self.parts.iter().flatten().map(|d| d.scope.heap_bytes()).sum();
        l.many("describe", "dynamic item scopes", Content, scopes, scopes);
        let tapes: usize = self.parts.iter().flatten().map(|d| d.tape.capacity() * 4).sum();
        l.many("describe", "dynamic item tapes", Content, tapes, tapes);
    }
    fn alloc(&mut self) -> u32 {
        match self.free.pop() {
            Some(i) => i,
            None => {
                self.parts.push(Vec::new());
                (self.parts.len() - 1) as u32
            }
        }
    }
}

impl Rt {
    /// Release a dynamic part and everything its items own (called from `release_scope`).
    pub(crate) fn release_dyn(&mut self, d: u32) {
        let items = std::mem::take(&mut self.dyns.parts[d as usize]);
        for it in items {
            self.release_scope(it.scope);
        }
        self.dyns.free.push(d);
    }

    fn queue_container(&mut self, lid: u32) {
        if lid == NONE {
            return;
        }
        let root = self.lay.root_of(lid);
        if !self.lay.info[root as usize].queued {
            self.lay.info[root as usize].queued = true;
            self.lay.pending.push(root);
        }
    }

    /// Release the layout subtree of `n` (a removed item's top-level box).
    pub(crate) fn remove_layout_subtree(&mut self, n: u32) {
        let mut stack = vec![n];
        let mut all = Vec::new();
        while let Some(x) = stack.pop() {
            all.push(x);
            self.lay.eng.children_into(&self.bough, x, &mut stack);
        }
        for &x in &all {
            if let Some(ctx) = self.lay.eng.ctx_mut(x) {
                ctx.release(&mut self.glyphs);
            }
            self.lay.info[x as usize].live = false;
        }
        self.lay.eng.remove_tree(&self.bough, n, &all);
    }
}

/// Mount the items of a dynamic part. A fixed owner mounts them inline (no tape, no per-item scope); a
/// re-describing one gives each item its own scope and tape and records the part on the owner's tape.
pub(crate) fn mount_dyn<V, E: Element<V>>(m: &mut Mounter<'_, V>, items: impl Iterator<Item = E>) {
    if !m.taping() {
        for e in items {
            e.mount(m);
        }
        return;
    }
    let d = m.rt.dyns.alloc();
    m.tape_push(d);
    m.scope.dyns.push(d);
    let mut out = Vec::new();
    for e in items {
        let key = e.dyn_key();
        let mut it = mount_item(m, e, None);
        it.key = key;
        out.push(it);
    }
    m.rt.dyns.parts[d as usize] = out;
}

/// Mount one item. `into`: a separate records buffer (an item added by a re-description, inserted at
/// `m.pos` afterwards); `None`: the mounter's own buffer (a first mount).
fn mount_item<V, E: Element<V>>(m: &mut Mounter<'_, V>, e: E, into: Option<&mut Vec<RecordInit>>) -> DynItem {
    let mut scope = Scope::default();
    let mut tape = Vec::new();
    let lid = m.rt.lay.cur;
    let k0 = if lid == NONE { 0 } else { m.rt.lay.eng.kid_count(lid) };
    let nrec = match into {
        Some(buf) => {
            let b0 = buf.len();
            let mut sub = m.sub(buf, &mut scope, &mut tape as *mut Vec<u32>);
            e.mount(&mut sub);
            buf.len() - b0
        }
        None => {
            let b0 = m.recs.len();
            let recs: *mut Vec<RecordInit> = m.recs;
            // SAFETY: `recs` is the mounter's own buffer, reborrowed for the sub-mount only.
            let mut sub = m.sub(unsafe { &mut *recs }, &mut scope, &mut tape as *mut Vec<u32>);
            e.mount(&mut sub);
            m.recs.len() - b0
        }
    };
    let nkid = if lid == NONE { 0 } else { m.rt.lay.eng.kid_count(lid) - k0 };
    DynItem { key: 0, nrec: nrec as u32, nkid: nkid as u32, scope, tape }
}

/// Re-describe a dynamic part: item `k` updates retained item `k`, new items are mounted as one run at the
/// end of the part, surplus items are removed as one run.
pub(crate) fn update_dyn<V, E: Element<V>>(m: &mut Mounter<'_, V>, items: impl Iterator<Item = E>) {
    let d = m.tape_next();
    let mut old = std::mem::take(&mut m.rt.dyns.parts[d as usize]);
    let lid = m.rt.lay.cur;
    let mut k = 0;
    let mut fresh: Vec<RecordInit> = Vec::new();
    let mut added: Vec<DynItem> = Vec::new();
    let (fresh_pos, fresh_kid) = (std::cell::Cell::new(usize::MAX), std::cell::Cell::new(0));
    for e in items {
        if k < old.len() && old[k].key != e.dyn_key() {
            // a keyed item that changed identity: replaced in place (its run out, the new one in)
            let key = e.dyn_key();
            let gone = std::mem::replace(&mut old[k], DynItem { key, nrec: 0, nkid: 0, scope: Scope::default(), tape: Vec::new() });
            if lid != NONE && gone.nkid > 0 {
                for i in 0..gone.nkid as usize {
                    let c = m.rt.lay.eng.kids(lid)[m.kid + i];
                    m.rt.remove_layout_subtree(c);
                }
                m.rt.lay.eng.splice_kids(&m.rt.bough, lid, m.kid, gone.nkid as usize, &[]);
            }
            m.rt.bough.remove_run(m.pos, gone.nrec as usize);
            m.rt.release_scope(gone.scope);
            let mut buf = Vec::new();
            let mut it = mount_item(m, e, Some(&mut buf));
            it.key = key;
            m.rt.bough.insert_run(m.pos, &buf);
            if lid != NONE && it.nkid > 0 {
                m.rt.lay.eng.place_tail_kids(lid, m.kid, it.nkid as usize);
                m.rt.lay.eng.invalidate(&m.rt.bough, lid);
                m.rt.queue_container(lid);
            }
            m.pos += it.nrec as usize;
            m.kid += it.nkid as usize;
            old[k] = it;
        } else if k < old.len() {
            let it = &mut old[k];
            let (p0, k0) = (m.pos, m.kid);
            let recs: *mut Vec<RecordInit> = m.recs;
            {
                // SAFETY: reborrow of the mounter's buffer for the sub-walk only.
                let mut sub = m.sub(unsafe { &mut *recs }, &mut it.scope, &mut it.tape as *mut Vec<u32>);
                e.update(&mut sub);
                let (p1, k1) = (sub.pos, sub.kid);
                drop(sub);
                m.pos = p1;
                m.kid = k1;
            }
            it.nrec = (m.pos - p0) as u32;
            it.nkid = (m.kid - k0) as u32;
        } else {
            if fresh_pos.get() == usize::MAX {
                fresh_pos.set(m.pos);
                fresh_kid.set(m.kid);
            }
            let key = e.dyn_key();
            let mut it = mount_item(m, e, Some(&mut fresh));
            it.key = key;
            added.push(it);
        }
        k += 1;
    }
    if !added.is_empty() {
        let at = fresh_pos.get();
        m.rt.bough.insert_run(at, &fresh);
        let nkid: usize = added.iter().map(|i| i.nkid as usize).sum();
        if lid != NONE {
            m.rt.lay.eng.place_tail_kids(lid, fresh_kid.get(), nkid);
            m.rt.lay.eng.invalidate(&m.rt.bough, lid);
            m.rt.queue_container(lid);
        }
        m.pos += fresh.len();
        m.kid += nkid;
        old.extend(added);
    } else if k < old.len() {
        let gone: Vec<DynItem> = old.drain(k..).collect();
        let nrec: usize = gone.iter().map(|i| i.nrec as usize).sum();
        let nkid: usize = gone.iter().map(|i| i.nkid as usize).sum();
        if lid != NONE && nkid > 0 {
            for i in 0..nkid {
                let c = m.rt.lay.eng.kids(lid)[m.kid + i];
                m.rt.remove_layout_subtree(c);
            }
            m.rt.lay.eng.splice_kids(&m.rt.bough, lid, m.kid, nkid, &[]);
            m.rt.queue_container(lid);
        }
        m.rt.bough.remove_run(m.pos, nrec);
        for it in gone {
            m.rt.release_scope(it.scope);
        }
    }
    m.rt.dyns.parts[d as usize] = old;
}

/// `Option<E>`: a dynamic part of zero or one item.
impl<V, E: Element<V>> Element<V> for Option<E> {
    fn mount(self, m: &mut Mounter<'_, V>) {
        mount_dyn(m, self.into_iter());
    }
    fn update(self, m: &mut Mounter<'_, V>) {
        update_dyn(m, self.into_iter());
    }
}

/// Re-describe the root owner `v` against its tape (`App::turn` and every handler, unless `Config::mount_once`).
///
/// # Safety
/// `tape` is the tape recorded when `v` was mounted at the start of the Bough, and `v` is that owner.
pub(crate) unsafe fn redescribe<V: crate::app::View>(rt: &mut Rt, v: &V, tape: &mut Vec<u32>) {
    let el = v.view();
    let mut recs: Vec<RecordInit> = Vec::new();
    let mut scope = Scope::default();
    let prev = rt.lay.cur;
    rt.lay.cur = NONE;
    {
        let mut m = Mounter { rt, owner: v as *const V, recs: &mut recs, scope: &mut scope, depth: 0, ox: 0.0, oy: 0.0, space: 0, clip: 0, pending_num: None, cls: 0, tape: tape as *mut Vec<u32>, ti: 0, pos: 0, kid: 0, keep: false };
        el.update(&mut m);
        debug_assert_eq!(m.ti, tape.len(), "re-description walked a different shape than the tape");
    }
    rt.lay.cur = prev;
    // a fixed shape adds nothing outside dynamic parts: the root scope stays empty
    debug_assert!(scope.nodes.is_empty() && scope.wires.is_empty());
}

/// Mount an element a re-description steps over (its content is state-driven: templates, scroll views,
/// text fields, tooltips): the tape records how many records, tape entries and layout children it made.
pub(crate) fn mount_opaque<V>(m: &mut Mounter<'_, V>, f: impl FnOnce(&mut Mounter<'_, V>)) {
    if !m.taping() {
        return f(m);
    }
    let at = m.ti_len();
    m.tape_push(0);
    m.tape_push(0);
    m.tape_push(0);
    let r0 = m.recs.len();
    let lid = m.rt.lay.cur;
    let k0 = if lid == NONE { 0 } else { m.rt.lay.eng.kid_count(lid) };
    f(m);
    let nkid = if lid == NONE { 0 } else { m.rt.lay.eng.kid_count(lid) - k0 };
    let t1 = m.ti_len();
    m.tape_set(at, (m.recs.len() - r0) as u32);
    m.tape_set(at + 1, (t1 - at - 3) as u32);
    m.tape_set(at + 2, nkid as u32);
}

/// Step over an element mounted by `mount_opaque`.
pub(crate) fn update_opaque<V>(m: &mut Mounter<'_, V>) {
    let nrec = m.tape_next() as usize;
    let ntape = m.tape_next() as usize;
    let nkid = m.tape_next() as usize;
    m.ti += ntape;
    m.pos += nrec;
    m.kid += nkid;
}
