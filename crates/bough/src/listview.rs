//! List regions: a `List<T>` shown as a stack of rows (`list`).
//!
//! A region is the list's consumer: one wire on the list's head (`patch_list`) replays the turn's delta log
//! (`list::Op`) on the region's mounted rows. Each row is an owner of its own (the item, at its stable slab
//! address) whose wires were installed by one monomorphic row function, so an edited item settles only its
//! own queued signals (`Rt::settle_cls` with the region's owner class).
//!
//! # Representation: rows are independent groups stacked by a prefix sum
//! A list's column has no layout of its own to do: row `k` sits at `y_k = Σ_{j<k} (h_j + gap)` and every
//! row is laid out at the content width. So the container is a *sized leaf* in its parent's layout (its
//! height is the stack's), each row's top box is a **layout group of its own** in a **space of its own**
//! (instances are row-local; the space adds `origin + (0, y_k)`), and the region places the rows itself:
//!
//! * an edit re-lays out that one row's group (cephal over the row's few nodes, never its siblings); if
//!   its height is unchanged nothing else moves;
//! * a height change, an insert or a remove shifts the rows after it: one space write per row and one
//!   `translate_run` per run of rows moving by the same amount (bough-core page offsets, O(pages)); the
//!   container's height is restyled when the total changes;
//! * a move or a permutation rewrites the window's records once (`read_run` + `rewrite_run`, handles
//!   follow) and re-places the window;
//! * the rows' records are one contiguous preorder run right after the container's record.
use crate::color::Color;
use crate::elem::{quad_instance, Element, Mounter};
use crate::layout::{Dim, Spec, Styled};
use crate::list::{LWired, List, Op};
use crate::rt::*;
use bough_core::{Handle, NRect, RecordInit};

pub struct LRow {
    pub slot: u32,
    pub nrec: u32,
    pub scope: Scope,
    pub space: u16,
    /// The row's layout group (its top box), or `NONE`.
    pub root: u32,
    /// Placed: its space holds `origin + (0, y)` and its records' ink follows it.
    pub placed: bool,
    pub y: f32,
    pub h: f32,
}

pub type LRowMountFn = unsafe fn(&mut Rt, u32, *const u8, &mut Vec<RecordInit>) -> LRow;

pub struct LRegion {
    pub(crate) cls: u32,
    /// The container's record: the rows' run starts right after it.
    pub(crate) anchor: Handle,
    pub(crate) lid: u32,
    pub(crate) depth: u16,
    pub(crate) clip: u16,
    /// The container's spec: the user's, padding and gap stripped (applied by the region), the height the
    /// stack's (`auto_h`) or the user's.
    pub(crate) spec: Spec,
    pub(crate) auto_h: bool,
    pub(crate) gap: f32,
    pub(crate) pad: [f32; 4],
    /// Content origin (in the container's space, device px) and width; NaN until laid out.
    pub(crate) origin: [f32; 2],
    pub(crate) width: f32,
    /// The content height the container is styled with.
    pub(crate) total: f32,
    pub(crate) dirty: bool,
    /// Its scope was released: never placed again.
    pub(crate) dead: bool,
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
    /// Per layout node: region + 1 if it is a list row's group root (its height is watched), and the
    /// height it was placed with.
    pub(crate) root_reg: Vec<u32>,
    pub(crate) root_h: Vec<f32>,
    ops: Vec<Op>,
    perm: Vec<u32>,
    stage: Vec<RecordInit>,
    stage2: Vec<RecordInit>,
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
    let (depth, clip, cls, f, width) = {
        let r = &rt.lists.regs[reg as usize];
        (r.depth, r.clip, r.cls, r.row_fn, r.width)
    };
    // SAFETY: the region was created by `LList::<T>::mount` from a `fn(&T, &mut Mounter<T>)`.
    let f: fn(&T, &mut Mounter<'_, T>) = unsafe { std::mem::transmute::<usize, fn(&T, &mut Mounter<'_, T>)>(f) };
    let space = rt.alloc_space();
    rt.set_space(space, [0.0, 0.0]);
    let before = recs.len();
    let (prev, avail) = (rt.lay.cur, rt.lay.mount_avail);
    rt.lay.cur = NONE;
    rt.lay.mount_avail = (width, f32::NAN);
    let mut scope = Scope::default();
    {
        let mut m = Mounter { rt, owner: row as *const T, recs, scope: &mut scope, depth, ox: 0.0, oy: 0.0, space, clip, pending_num: None, cls, tape: std::ptr::null_mut(), ti: 0, pos: 0, kid: 0, keep: false };
        // SAFETY: the function contract.
        f(unsafe { &*(row as *const T) }, &mut m);
    }
    rt.lay.cur = prev;
    rt.lay.mount_avail = avail;
    let root = scope.lroots.first().copied().unwrap_or(NONE);
    if root != NONE {
        let rr = &mut rt.lists.root_reg;
        if rr.len() <= root as usize {
            rr.resize(root as usize + 1, 0);
            rt.lists.root_h.resize(root as usize + 1, f32::NAN);
        }
        rr[root as usize] = reg + 1;
        rt.lists.root_h[root as usize] = f32::NAN;
    }
    LRow { slot: 0, nrec: (recs.len() - before) as u32, scope, space, root, placed: false, y: 0.0, h: 0.0 }
}

fn view_row<T: crate::app::View>(v: &T, m: &mut Mounter<'_, T>) {
    v.view().mount(m)
}

impl Rt {
    fn lr_base(&self, reg: usize) -> usize {
        self.bough.pos_of(self.lists.regs[reg].anchor).expect("list container is in the Bough") + 1
    }
    fn lr_prefix(&self, reg: usize, k: usize) -> usize {
        self.lists.regs[reg].rows[..k].iter().map(|r| r.nrec as usize).sum()
    }
    /// Release a row: its space, its resources (its layout group is a root of its scope). Row-local wires
    /// (whose head lies in the row's own memory) are freed now: nothing else can reach them (the item's
    /// heads are cleared by `List::free_slot`, or here when the item stays).
    fn lr_release_row(&mut self, row: LRow, row_ptr: usize, size: usize, live: bool, cls: u32) {
        let (lo, hi) = (row_ptr, row_ptr + size);
        if live {
            for &off in &self.classes[cls as usize] {
                // SAFETY: the item is live in its slab and `off` is an `offset_of!`-proven headed field of it.
                unsafe { (*((row_ptr + off as usize) as *const std::cell::Cell<u32>)).set(0) };
            }
        }
        if row.root != NONE {
            self.lists.root_reg[row.root as usize] = 0;
        }
        if row.nrec > 0 {
            self.free_space(row.space);
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

    /// The scope that mounted region `reg` is released: so are its rows (their items stay in the list;
    /// their heads keep indices of dead wires, which `add_wire`/`settle` recognise and drop).
    pub(crate) fn lr_release_region(&mut self, reg: usize) {
        let rows = std::mem::take(&mut self.lists.regs[reg].rows);
        for row in rows {
            if row.root != NONE {
                self.lists.root_reg[row.root as usize] = 0;
            }
            if row.nrec > 0 {
                self.free_space(row.space);
            }
            self.release_scope(row.scope);
        }
        self.lists.regs[reg].dirty = false;
        self.lists.regs[reg].dead = true;
    }

    /// Mount the items in `slots` as rows `at..` of region `reg`.
    unsafe fn lr_insert<T: 'static>(&mut self, reg: usize, list: *const List<T>, at: usize, slots: &[u32]) {
        let base = self.lr_base(reg);
        let rp = self.lr_prefix(reg, at);
        let mut recs = std::mem::take(&mut self.lists.stage);
        recs.clear();
        let mount = self.lists.regs[reg].mount;
        let mut new = Vec::with_capacity(slots.len());
        for &s in slots {
            // An item inserted and removed again in the same turn is not in the slab any more (its slot is
            // not reused before the turn ends, so a dead slot means exactly that): it stays a phantom row
            // with no records until its own remove is replayed.
            // SAFETY: shared read of the live list.
            if !unsafe { (*list).slot_live(s) } {
                new.push(LRow { slot: s, nrec: 0, scope: Scope::default(), space: 0, root: NONE, placed: true, y: 0.0, h: 0.0 });
                continue;
            }
            // SAFETY: a live slot holds an initialised item.
            let p = unsafe { (*list).slot_ptr(s) } as *const u8;
            let mut r = unsafe { mount(self, reg as u32, p, &mut recs) };
            r.slot = s;
            new.push(r);
        }
        self.bough.insert_run(base + rp, &recs);
        let r = &mut self.lists.regs[reg];
        r.rows.splice(at..at, new);
        r.dirty = true;
        recs.clear();
        self.lists.stage = recs;
    }

    unsafe fn lr_remove<T: 'static>(&mut self, reg: usize, list: *const List<T>, at: usize, n: usize) {
        let base = self.lr_base(reg);
        let rp = self.lr_prefix(reg, at);
        let nrec: usize = self.lists.regs[reg].rows[at..at + n].iter().map(|r| r.nrec as usize).sum();
        self.bough.remove_run(base + rp, nrec);
        let (cls, size) = (self.lists.regs[reg].cls, self.lists.regs[reg].row_size);
        let gone: Vec<LRow> = self.lists.regs[reg].rows.drain(at..at + n).collect();
        self.lists.regs[reg].dirty = true;
        for r in gone {
            // SAFETY: slab addresses stay valid for the list's life (the item itself was moved out).
            let p = unsafe { (*list).slot_ptr(r.slot) } as usize;
            self.lr_release_row(r, p, size, false, cls);
        }
    }

    /// Reorder rows `lo..lo+new.len()`: `new[i]` is the old index (within the window) of the row now at
    /// `lo + i`. One `read_run` + `rewrite_run` of the window's records; the rows are re-placed (their
    /// spaces) by `lists_place`.
    fn lr_reorder(&mut self, reg: usize, lo: usize, new: &[u32]) {
        let n = new.len();
        let base = self.lr_base(reg);
        let rp = self.lr_prefix(reg, lo);
        let mut stage = std::mem::take(&mut self.lists.stage);
        let mut out = std::mem::take(&mut self.lists.stage2);
        stage.clear();
        out.clear();
        let (total, roff) = {
            let rows = &self.lists.regs[reg].rows[lo..lo + n];
            let mut roff = Vec::with_capacity(n + 1);
            let mut a = 0usize;
            for r in rows {
                roff.push(a);
                a += r.nrec as usize;
            }
            roff.push(a);
            (a, roff)
        };
        self.bough.read_run(base + rp, total, &mut stage);
        for &j in new {
            out.extend_from_slice(&stage[roff[j as usize]..roff[j as usize + 1]]);
        }
        self.bough.rewrite_run(base + rp, &out);
        {
            let r = &mut self.lists.regs[reg];
            let mut window: Vec<Option<LRow>> = r.rows.drain(lo..lo + n).map(Some).collect();
            let re: Vec<LRow> = new.iter().map(|&j| window[j as usize].take().unwrap()).collect();
            r.rows.splice(lo..lo, re);
            r.dirty = true;
        }
        self.lists.stage = stage;
        self.lists.stage2 = out;
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

    /// A group was just laid out: if it is a list row, its region re-places (a row whose height did not
    /// change costs the region one compare).
    #[inline]
    pub(crate) fn lists_root_computed(&mut self, root: u32) {
        let Some(&r) = self.lists.root_reg.get(root as usize) else { return };
        if r != 0 && self.lists.root_h[root as usize] != self.lay.root_height(root) {
            self.lists.regs[r as usize - 1].dirty = true;
        }
    }

    /// Place every region whose rows or container changed (after a compute pass, before the write-back;
    /// see `lay_flush`): container origin and width from its computed layout, row heights from their
    /// groups, `y` by prefix sum, spaces written and records translated for rows that moved, the
    /// container restyled when the stack's height changed. Returns whether it queued more layout.
    pub(crate) fn lists_place(&mut self) -> bool {
        let mut again = false;
        for reg in 0..self.lists.regs.len() {
            let lid = self.lists.regs[reg].lid;
            if self.lists.regs[reg].dead || !self.lay.info[lid as usize].live {
                continue;
            }
            // the container's group-local position: the sum of the rects up its parent chain
            let (mut ax, mut ay, mut p) = (0.0f32, 0.0f32, lid);
            while p != NONE {
                let (x, y, _, _) = self.lay.eng.rect(p);
                ax += x;
                ay += y;
                p = self.lay.info[p as usize].parent;
            }
            let aw = self.lay.eng.rect(lid).2;
            let space = self.lay.info[lid as usize].space;
            let so = self.space(space);
            let (origin, width) = {
                let r = &self.lists.regs[reg];
                ([so[0] + ax + r.pad[0], so[1] + ay + r.pad[1]], aw - r.pad[0] - r.pad[2])
            };
            let r = &mut self.lists.regs[reg];
            let moved = origin != r.origin;
            let resized = width != r.width;
            if !(r.dirty || moved || resized) {
                continue;
            }
            r.origin = origin;
            if resized {
                r.width = width;
                for row in &r.rows {
                    if row.root != NONE {
                        let li = &mut self.lay.info[row.root as usize];
                        li.avail_w = width;
                        if !li.queued {
                            li.queued = true;
                            self.lay.pending.push(row.root);
                        }
                    }
                }
                r.dirty = true;
                again = true;
                continue; // placed once the rows are laid out at the new width
            }
            r.dirty = false;
            unsafe { self.lr_place(reg) };
            // the stack's height is the container's
            let r = &self.lists.regs[reg];
            if r.auto_h {
                let n = r.rows.iter().filter(|x| x.root != NONE).count();
                let total = r.rows.iter().map(|x| x.h).sum::<f32>() + r.gap * n.saturating_sub(1) as f32;
                if total != r.total {
                    let mut spec = r.spec;
                    spec.h = Dim::Px(total + r.pad[1] + r.pad[3]);
                    let r = &mut self.lists.regs[reg];
                    r.total = total;
                    r.spec = spec;
                    crate::layout::restyle(self, lid, &spec);
                    again = true;
                }
            }
        }
        again
    }

    /// Place region `reg`'s rows: heights from their groups, `y` by prefix sum; a row whose space moves gets
    /// its space written, and the records of consecutive placed rows moving by the same amount one
    /// `translate_run` (O(pages)).
    unsafe fn lr_place(&mut self, reg: usize) {
        let base = self.lr_base(reg);
        let (origin, gap) = (self.lists.regs[reg].origin, self.lists.regs[reg].gap);
        let mut y = 0.0f32;
        let mut pos = base;
        // pending run of placed rows moving by the same delta: (first record, count, dx, dy)
        let mut run: Option<(usize, usize, i32, i32)> = None;
        let n = self.lists.regs[reg].rows.len();
        for k in 0..n {
            let (root, space, nrec, placed) = {
                let r = &self.lists.regs[reg].rows[k];
                (r.root, r.space, r.nrec as usize, r.placed)
            };
            if root == NONE {
                pos += nrec;
                continue;
            }
            let h = self.lay.root_height(root);
            let new = [origin[0].round(), (origin[1] + y).round()];
            let old = self.space(space);
            let mut flush = true;
            if !placed || old != new {
                self.set_space(space, new);
                if placed {
                    let (dx, dy) = ((new[0] - old[0]) as i32, (new[1] - old[1]) as i32);
                    match &mut run {
                        Some((_, c, rx, ry)) if *rx == dx && *ry == dy => {
                            *c += nrec;
                            flush = false;
                        }
                        _ => {
                            if let Some((a, c, rx, ry)) = run.take() {
                                self.bough.translate_run(a, c, rx, ry);
                            }
                            run = Some((pos, nrec, dx, dy));
                            flush = false;
                        }
                    }
                }
            }
            if flush {
                if let Some((a, c, rx, ry)) = run.take() {
                    self.bough.translate_run(a, c, rx, ry);
                }
            }
            self.lists.root_h[root as usize] = h;
            let r = &mut self.lists.regs[reg].rows[k];
            r.placed = true;
            r.y = y;
            r.h = h;
            y += h + gap;
            pos += nrec;
        }
        if let Some((a, c, rx, ry)) = run.take() {
            self.bough.translate_run(a, c, rx, ry);
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
        rt.pubs.list(sig as usize, reg.log().0);
    }
    unsafe { rt.lr_replay::<T>(w.node as usize, l) };
}

// =============================================================================== element

/// A `List<T>` as a stack of rows (see the module docs): every item is one row, mounted by its
/// `View::view` (`list`) or by a row function (`list_with`), laid out at the container's content width.
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
    LList { list: l, row, spec: Spec::new(false, true), bg: None, radius: 0.0 }
}

impl<T, V> Styled for LList<'_, T, V> {
    fn spec_mut(&mut self) -> &mut Spec {
        &mut self.spec
    }
}
impl<T, V> LList<'_, T, V> {
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
        // the container is a sized leaf: the region applies its padding and gap, and its height is the stack's
        let mut spec = self.spec;
        let pad = spec.pad.map(|p| p * s);
        let gap = spec.gap * s;
        spec.pad = [0.0; 4];
        spec.gap = 0.0;
        spec.container = false;
        let auto_h = spec.h == Dim::Auto;
        if auto_h {
            spec.h = Dim::Px(pad[1] + pad[3]);
        }
        let lid = m.rt.lay.eng.add(hd, &spec, s, None);
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
        // a definite width is known now: the rows are laid out at it in the first pass
        let width = match self.spec.w {
            Dim::Pt(w) => w * s - pad[0] - pad[2],
            Dim::Px(w) => w - pad[0] - pad[2],
            _ => f32::NAN,
        };
        let cls = m.rt.new_class();
        let reg = m.rt.lists.regs.len();
        m.rt.lists.regs.push(LRegion {
            cls,
            anchor: hd,
            lid,
            depth: m.depth + 1,
            clip: m.clip,
            spec,
            auto_h,
            gap,
            pad,
            origin: [f32::NAN; 2],
            width,
            total: 0.0,
            dirty: true,
            dead: false,
            rows: Vec::new(),
            mount: mount_lrow::<T>,
            row_fn: self.row as usize,
            row_size: size_of::<T>(),
        });
        let l = self.list.list;
        // SAFETY: `self.list` is an `offset_of!`-proven `List<T>` field of `V`; `patch_list::<T>` reads one.
        unsafe { m.wire_cell(l.head(), self.list.off, patch_list::<T>, reg as u32, [0; 16]) };
        m.scope.lregs.push(reg as u32);
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

    /// The rows are the list's (state, not description): a re-description steps over the rows' run.
    fn update(self, m: &mut Mounter<'_, V>) {
        let reg = m.tape_next() as usize;
        let lid = m.rt.lists.regs[reg].lid;
        if let (Some(bg), rtn) = (self.bg, m.rt.lay.info[lid as usize].rtn) {
            if rtn != NONE {
                <crate::elem::ApplyBg as crate::elem::Apply>::apply(m.rt, rtn, &bg);
            }
        }
        m.pos += 1 + m.rt.lists.regs[reg].rows.iter().map(|r| r.nrec as usize).sum::<usize>();
        m.kid += 1;
    }
}
