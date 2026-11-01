//! List regions: a `List<T>` shown as the children of a flex container (`list`).
//!
//! A region is the list's consumer: one wire on the list's head (`patch_list`) replays the turn's delta log
//! (`list::Op`) on the region's mounted rows. Each row is an owner of its own (the item, at its stable slab
//! address) whose wires were installed by one monomorphic row function, so an edited item settles only its
//! own queued signals (`Rt::settle_cls` with the region's owner class).
//!
//! The rows' records are one contiguous preorder run right after the container's record, row after row,
//! and the rows' top-level layout nodes are the container's layout children in the same order, so every
//! delta is a run edit at a prefix-summed offset:
//!
//! | op | Bough | layout |
//! |---|---|---|
//! | inserts at `k` (consecutive ones batched) | mount rows into one buffer, one `insert_run` | kids spliced in, container invalidated |
//! | removes (forward and backward runs batched) | one `remove_run` | subtrees released, kids spliced out |
//! | move / permutation of a window | `read_run` + one `rewrite_run` of the window (handles follow) | the window's kids permuted |
//! | edit | none: the row's queued wires run | what the wires invalidate |
use crate::color::Color;
use crate::elem::{quad_instance, Element, Mounter};
use crate::layout::{Spec, Styled};
use crate::list::{LWired, List, Op};
use crate::rt::*;
use bough_core::{Handle, NRect, RecordInit};

pub struct LRow {
    pub slot: u32,
    pub nrec: u32,
    pub nkid: u32,
    pub scope: Scope,
}

pub type LRowMountFn = unsafe fn(&mut Rt, u32, *const u8, &mut Vec<RecordInit>) -> LRow;

pub struct LRegion {
    pub(crate) cls: u32,
    /// The container's record: the rows' run starts right after it.
    pub(crate) anchor: Handle,
    pub(crate) lid: u32,
    pub(crate) depth: u16,
    pub(crate) space: u16,
    pub(crate) clip: u16,
    pub rows: Vec<LRow>,
    pub(crate) mount: LRowMountFn,
    /// The row function `fn(&T, &mut Mounter<T>)`, type-erased (transmuted back by `mount`).
    pub(crate) row_fn: usize,
    pub(crate) row_size: usize,
}

/// The list regions of a runtime, and the lists to close at the end of the turn's settle.
#[derive(Default)]
pub struct Lists {
    pub regs: Vec<LRegion>,
    pub(crate) ends: Vec<(*const u8, unsafe fn(*const u8))>,
    ops: Vec<Op>,
    perm: Vec<u32>,
    stage: Vec<RecordInit>,
    stage2: Vec<RecordInit>,
    kids: Vec<u32>,
    map: Vec<u32>,
}

/// # Safety
/// `l` is a live `List<T>` that no other reference is borrowing mutably.
unsafe fn end_list<T>(l: *const u8) {
    // SAFETY: the function contract.
    unsafe { (*(l as *mut List<T>)).end_turn() };
}

/// # Safety
/// `row` points at a live `T` in its list's slab; `reg` was created for a `fn(&T, &mut Mounter<T>)`.
unsafe fn mount_lrow<T: 'static>(rt: &mut Rt, reg: u32, row: *const u8, recs: &mut Vec<RecordInit>) -> LRow {
    let (depth, space, clip, lid, cls, f) = {
        let r = &rt.lists.regs[reg as usize];
        (r.depth, r.space, r.clip, r.lid, r.cls, r.row_fn)
    };
    // SAFETY: the region was created by `LList::<T>::mount` from a `fn(&T, &mut Mounter<T>)`.
    let f: fn(&T, &mut Mounter<'_, T>) = unsafe { std::mem::transmute::<usize, fn(&T, &mut Mounter<'_, T>)>(f) };
    let before = recs.len();
    let kids_before = rt.lay.eng.kid_count(lid);
    let prev = rt.lay.cur;
    rt.lay.cur = lid;
    let mut scope = Scope::default();
    {
        let mut m = Mounter { rt, owner: row as *const T, recs, scope: &mut scope, depth, ox: 0.0, oy: 0.0, space, clip, pending_num: None, cls, tape: std::ptr::null_mut(), ti: 0, pos: 0, kid: 0 };
        // SAFETY: the function contract.
        f(unsafe { &*(row as *const T) }, &mut m);
    }
    rt.lay.cur = prev;
    let nkid = (rt.lay.eng.kid_count(lid) - kids_before) as u32;
    LRow { slot: 0, nrec: (recs.len() - before) as u32, nkid, scope }
}

fn view_row<T: crate::app::View>(v: &T, m: &mut Mounter<'_, T>) {
    v.view().mount(m)
}

impl Rt {
    fn lr_base(&self, reg: usize) -> usize {
        self.bough.pos_of(self.lists.regs[reg].anchor).expect("list container is in the Bough") + 1
    }
    fn lr_prefix(&self, reg: usize, k: usize) -> (usize, usize) {
        let rows = &self.lists.regs[reg].rows[..k];
        (rows.iter().map(|r| r.nrec as usize).sum(), rows.iter().map(|r| r.nkid as usize).sum())
    }
    fn lr_queue_layout(&mut self, lid: u32) {
        let root = self.lay.root_of(lid);
        if !self.lay.info[root as usize].queued {
            self.lay.info[root as usize].queued = true;
            self.lay.pending.push(root);
        }
    }
    /// Release one layout subtree (a removed row's top-level box and everything below it).
    fn lr_remove_layout(&mut self, n: u32) {
        let mut stack: Vec<u32> = Vec::new();
        let mut all = std::mem::take(&mut self.lists.kids);
        stack.clear();
        all.clear();
        stack.push(n);
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
        self.lists.kids = all;
    }
    /// Release a row's scope; its row-local wires (whose head lies in the row's own memory) are freed now:
    /// nothing else can reach them (the item's heads are cleared by `List::free_slot`, or here).
    fn lr_release_row(&mut self, row: LRow, row_ptr: usize, size: usize, live: bool, cls: u32) {
        let (lo, hi) = (row_ptr, row_ptr + size);
        if live {
            for &off in &self.classes[cls as usize] {
                // SAFETY: the item is live in its slab and `off` is an `offset_of!`-proven headed field of it.
                unsafe { (*((row_ptr + off as usize) as *const std::cell::Cell<u32>)).set(0) };
            }
        }
        let mut scope = row.scope;
        for &w in &scope.wires {
            let wr = &mut self.wires[w as usize];
            if wr.head >= lo && wr.head < hi {
                wr.dead = true;
                wr.head = 0;
                self.wire_free.push(w);
            } else {
                wr.dead = true;
            }
        }
        scope.wires.clear();
        self.release_scope(scope);
    }

    /// Mount the items in `slots` as rows `at..` of region `reg`.
    unsafe fn lr_insert<T: 'static>(&mut self, reg: usize, list: *const List<T>, at: usize, slots: &[u32]) {
        let base = self.lr_base(reg);
        let (rp, kp) = self.lr_prefix(reg, at);
        let mut recs = std::mem::take(&mut self.lists.stage);
        recs.clear();
        let (mount, lid) = (self.lists.regs[reg].mount, self.lists.regs[reg].lid);
        let mut new = Vec::with_capacity(slots.len());
        let mut nkid = 0;
        for &s in slots {
            // An item inserted and removed again in the same turn is not in the slab any more (its slot is
            // not reused before the turn ends, so a dead slot means exactly that): it stays a phantom row
            // with no records until its own remove is replayed.
            // SAFETY: shared read of the live list.
            if !unsafe { (*list).slot_live(s) } {
                new.push(LRow { slot: s, nrec: 0, nkid: 0, scope: Scope::default() });
                continue;
            }
            // SAFETY: a live slot holds an initialised item.
            let p = unsafe { (*list).slot_ptr(s) } as *const u8;
            let mut r = unsafe { mount(self, reg as u32, p, &mut recs) };
            r.slot = s;
            nkid += r.nkid as usize;
            new.push(r);
        }
        self.bough.insert_run(base + rp, &recs);
        self.lay.eng.place_tail_kids(lid, kp, nkid);
        self.lay.eng.invalidate(&self.bough, lid);
        self.lr_queue_layout(lid);
        let rows = &mut self.lists.regs[reg].rows;
        rows.splice(at..at, new);
        recs.clear();
        self.lists.stage = recs;
    }

    unsafe fn lr_remove<T: 'static>(&mut self, reg: usize, list: *const List<T>, at: usize, n: usize) {
        let base = self.lr_base(reg);
        let (rp, kp) = self.lr_prefix(reg, at);
        let (nrec, nkid): (usize, usize) = {
            let rows = &self.lists.regs[reg].rows[at..at + n];
            (rows.iter().map(|r| r.nrec as usize).sum(), rows.iter().map(|r| r.nkid as usize).sum())
        };
        let lid = self.lists.regs[reg].lid;
        for i in 0..nkid {
            let k = self.lay.eng.kids(lid)[kp + i];
            self.lr_remove_layout(k);
        }
        self.lay.eng.splice_kids(&self.bough, lid, kp, nkid, &[]);
        self.lr_queue_layout(lid);
        self.bough.remove_run(base + rp, nrec);
        let (cls, size) = (self.lists.regs[reg].cls, self.lists.regs[reg].row_size);
        let gone: Vec<LRow> = self.lists.regs[reg].rows.drain(at..at + n).collect();
        for r in gone {
            // SAFETY: slab addresses stay valid for the list's life (the item itself was moved out).
            let p = unsafe { (*list).slot_ptr(r.slot) } as usize;
            self.lr_release_row(r, p, size, false, cls);
        }
    }

    /// Reorder rows `lo..lo+new.len()`: `new[i]` is the old index (within the window) of the row now at
    /// `lo + i`. One `read_run` + `rewrite_run` of the window's records, one kid permutation.
    fn lr_reorder(&mut self, reg: usize, lo: usize, new: &[u32]) {
        let n = new.len();
        let base = self.lr_base(reg);
        let (rp, kp) = self.lr_prefix(reg, lo);
        let lid = self.lists.regs[reg].lid;
        let mut stage = std::mem::take(&mut self.lists.stage);
        let mut out = std::mem::take(&mut self.lists.stage2);
        let mut kids = std::mem::take(&mut self.lists.kids);
        stage.clear();
        out.clear();
        kids.clear();
        let (total, ktotal, roff, koff) = {
            let rows = &self.lists.regs[reg].rows[lo..lo + n];
            let mut roff = Vec::with_capacity(n + 1);
            let mut koff = Vec::with_capacity(n + 1);
            let (mut a, mut b) = (0usize, 0usize);
            for r in rows {
                roff.push(a);
                koff.push(b);
                a += r.nrec as usize;
                b += r.nkid as usize;
            }
            roff.push(a);
            koff.push(b);
            (a, b, roff, koff)
        };
        self.bough.read_run(base + rp, total, &mut stage);
        let old_kids = &self.lay.eng.kids(lid)[kp..kp + ktotal];
        for &j in new {
            let j = j as usize;
            out.extend_from_slice(&stage[roff[j]..roff[j + 1]]);
            kids.extend_from_slice(&old_kids[koff[j]..koff[j + 1]]);
        }
        self.bough.rewrite_run(base + rp, &out);
        self.lay.eng.permute_kids(&self.bough, lid, kp, &kids);
        self.lr_queue_layout(lid);
        {
            let rows = &mut self.lists.regs[reg].rows;
            let mut window: Vec<Option<LRow>> = rows.drain(lo..lo + n).map(Some).collect();
            let re: Vec<LRow> = new.iter().map(|&j| window[j as usize].take().unwrap()).collect();
            rows.splice(lo..lo, re);
        }
        self.lists.stage = stage;
        self.lists.stage2 = out;
        self.lists.kids = kids;
    }

    /// Replay list `l`'s delta log on region `reg` (see the module docs), then settle the edited rows.
    unsafe fn lr_replay<T: 'static>(&mut self, reg: usize, l: *const List<T>) {
        let mut ops = std::mem::take(&mut self.lists.ops);
        let mut perm = std::mem::take(&mut self.lists.perm);
        ops.clear();
        perm.clear();
        {
            // SAFETY: the list is live; only shared reads here.
            let (o, p) = unsafe { (*l).log() };
            ops.extend_from_slice(o);
            perm.extend_from_slice(p);
        }
        let mut i = 0;
        let mut slots: Vec<u32> = Vec::new();
        while i < ops.len() {
            match ops[i] {
                Op::Insert { at, .. } => {
                    slots.clear();
                    let mut next = at;
                    while let Some(&Op::Insert { at: a, slot }) = ops.get(i) {
                        if a != next {
                            break;
                        }
                        slots.push(slot);
                        next += 1;
                        i += 1;
                    }
                    unsafe { self.lr_insert(reg, l, at as usize, &slots) };
                }
                Op::Remove { at, .. } => {
                    let (mut lo, mut hi) = (at, at + 1);
                    i += 1;
                    while let Some(&Op::Remove { at: a, .. }) = ops.get(i) {
                        if a == lo && hi < u32::MAX {
                            hi += 1;
                        } else if a + 1 == lo {
                            lo -= 1;
                        } else {
                            break;
                        }
                        i += 1;
                    }
                    unsafe { self.lr_remove(reg, l, lo as usize, (hi - lo) as usize) };
                }
                Op::Move { from, to } => {
                    let (lo, hi) = (from.min(to) as usize, from.max(to) as usize + 1);
                    let mut new: Vec<u32> = (0..(hi - lo) as u32).collect();
                    let x = new.remove(from as usize - lo);
                    new.insert(to as usize - lo, x);
                    self.lr_reorder(reg, lo, &new);
                    i += 1;
                }
                Op::Permute { at, n, off } => {
                    let (at, n, off) = (at as usize, n as usize, off as usize);
                    let mut map = std::mem::take(&mut self.lists.map);
                    for (j, r) in self.lists.regs[reg].rows[at..at + n].iter().enumerate() {
                        let s = r.slot as usize;
                        if map.len() <= s {
                            map.resize(s + 1, u32::MAX);
                        }
                        map[s] = j as u32;
                    }
                    let new: Vec<u32> = perm[off..off + n].iter().map(|&s| map[s as usize]).collect();
                    self.lists.map = map;
                    self.lr_reorder(reg, at, &new);
                    i += 1;
                }
                Op::Edit { .. } => i += 1,
            }
        }
        // the rows now follow the list's order
        debug_assert!(self.lists.regs[reg].rows.iter().map(|r| r.slot).eq(unsafe { (*l).order_slots() }.iter().copied()), "list region out of step with its list");
        // edited rows: their queued wires (a removed item is not live and is skipped)
        let cls = self.lists.regs[reg].cls;
        for op in &ops {
            if let Op::Edit { slot } = *op {
                // SAFETY: a live slot is initialised; its wires were installed for class `cls`.
                if unsafe { (*l).slot_live(slot) } {
                    let p = unsafe { (*l).slot_ptr(slot) } as *const u8;
                    unsafe { self.settle_cls(p, cls) };
                }
            }
        }
        // the item type's wired fields, for `List::free_slot`
        {
            let c = &self.classes[cls as usize];
            // SAFETY: shared read of the live list; `wired` is a RefCell.
            let w = unsafe { &(*l).wired };
            if w.borrow().len() != c.len() {
                *w.borrow_mut() = c.clone();
            }
        }
        self.lists.ops = ops;
        self.lists.perm = perm;
    }

    /// End of the turn's settle: close every list whose log a region replayed.
    pub(crate) fn lists_end_turn(&mut self) {
        while let Some((p, f)) = self.lists.ends.pop() {
            // SAFETY: registered by `patch_list::<T>` for a live list field of the settled owner.
            unsafe { f(p) };
        }
    }
}

/// # Safety
/// Wire contract: `sig` is a `List<T>` field of the live owner (derived from a unique borrow), `w.node` its
/// region.
unsafe fn patch_list<T: 'static>(rt: &mut Rt, w: &Wire, sig: *const u8, _owner: *const u8) {
    let l = sig as *const List<T>;
    // SAFETY: the contract.
    let reg = unsafe { &*l };
    if !reg.registered.get() {
        reg.registered.set(true);
        rt.lists.ends.push((sig, end_list::<T>));
    }
    unsafe { rt.lr_replay::<T>(w.node as usize, l) };
}

// =============================================================================== element

/// A `List<T>` as the children of a flex container (a column unless `.horizontal()`): every item is one
/// row, mounted by its `View::view` (`list`) or by a row function (`list_with`).
pub struct LList<'a, T, V> {
    list: LWired<'a, T, V>,
    row: fn(&T, &mut Mounter<'_, T>),
    spec: Spec,
    bg: Option<Color>,
    radius: f32,
}

pub fn list<'a, T: crate::app::View, V>(l: LWired<'a, T, V>) -> LList<'a, T, V> {
    list_with(l, view_row::<T>)
}
pub fn list_with<'a, T, V>(l: LWired<'a, T, V>, row: fn(&T, &mut Mounter<'_, T>)) -> LList<'a, T, V> {
    LList { list: l, row, spec: Spec::new(true, true), bg: None, radius: 0.0 }
}

impl<T, V> Styled for LList<'_, T, V> {
    fn spec_mut(&mut self) -> &mut Spec {
        &mut self.spec
    }
}
impl<T, V> LList<'_, T, V> {
    /// Lay the rows out in a row instead of a column.
    pub fn horizontal(mut self) -> Self {
        self.spec.column = false;
        self
    }
    pub fn bg(mut self, c: Color) -> Self {
        self.bg = Some(c);
        self
    }
    pub fn rounded(mut self, r: f32) -> Self {
        self.radius = r;
        self
    }
}

impl<'a, T: 'static, V: 'static> Element<V> for LList<'a, T, V> {
    fn mount(self, m: &mut Mounter<'_, V>) {
        let s = m.rt.scale;
        let hd = m.rt.bough.new_handle();
        let lid = m.rt.lay.eng.add(hd, &self.spec, s, None);
        let parent = crate::layout::attach(m, lid);
        match self.bg {
            Some(bg) => {
                let node = m.rt.alloc_node();
                let (ins, _kind, margin) = quad_instance(bg, self.radius * s, None, s, m.space);
                let run = m.rt.alloc_run(1);
                m.rt.runs[run as usize].len = 1;
                let inst = m.rt.runs[run as usize].first;
                *m.rt.inst_mut(inst) = ins;
                {
                    let n = &mut m.rt.nodes[node as usize];
                    n.handle = hd;
                    n.run = run;
                    n.space = m.space;
                    n.clip = m.clip;
                    n.kind = NK_QUAD;
                    n.color = bg.0;
                    n.margin = margin;
                    n.lnode = lid;
                }
                m.scope.nodes.push(node);
                m.scope.runs.push(run);
                m.rt.lay.info[lid as usize].rtn = node;
                m.rt.lay.info[lid as usize].kind = 1;
                m.recs.push(RecordInit { depth: m.depth, ink: NRect::EMPTY, kind: REC_QUAD, payload: inst, handle: hd.idx, clip: m.clip });
            }
            None => m.recs.push(RecordInit { depth: m.depth, ink: NRect::EMPTY, kind: REC_BOX, payload: 0, handle: hd.idx, clip: m.clip }),
        }
        let cls = m.rt.new_class();
        let reg = m.rt.lists.regs.len();
        m.rt.lists.regs.push(LRegion {
            cls,
            anchor: hd,
            lid,
            depth: m.depth + 1,
            space: m.space,
            clip: m.clip,
            rows: Vec::new(),
            mount: mount_lrow::<T>,
            row_fn: self.row as usize,
            row_size: size_of::<T>(),
        });
        let l = self.list.list;
        // SAFETY: `self.list` is an `offset_of!`-proven `List<T>` field of `V`; `patch_list::<T>` reads one.
        unsafe { m.wire_cell(l.head(), self.list.off, patch_list::<T>, reg as u32, [0; 16]) };
        let n = l.len();
        let mut rows = Vec::with_capacity(n);
        for pos in 0..n {
            let sl = l.slot_at(pos);
            // SAFETY: the slot is live; the region was just created for `T`.
            let mut r = unsafe { mount_lrow::<T>(m.rt, reg as u32, l.slot_ptr(sl) as *const u8, m.recs) };
            r.slot = sl;
            rows.push(r);
        }
        m.rt.lists.regs[reg].rows = rows;
        *l.wired.borrow_mut() = m.rt.classes[cls as usize].clone();
        m.rt.lay.cur = parent;
        m.tape_push(reg as u32);
    }

    /// The rows are the list's (state, not description): a re-description restyles the container and
    /// steps over the rows' run.
    fn update(self, m: &mut Mounter<'_, V>) {
        let reg = m.tape_next() as usize;
        let lid = m.rt.lists.regs[reg].lid;
        crate::layout::restyle(m.rt, lid, &self.spec);
        if let (Some(bg), rtn) = (self.bg, m.rt.lay.info[lid as usize].rtn) {
            if rtn != NONE {
                <crate::elem::ApplyBg as crate::elem::Apply>::apply(m.rt, rtn, &bg);
            }
        }
        m.pos += 1 + m.rt.lists.regs[reg].rows.iter().map(|r| r.nrec as usize).sum::<usize>();
        m.kid += 1;
    }
}
