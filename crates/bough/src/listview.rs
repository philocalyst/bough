//! List regions: a `List<T>` shown as a stack of rows (`list`) — all of them, or the window a viewport
//! shows (`.scroll(sig!(..))` / `.viewport()`): ONE mechanism for keyed lists and virtual lists.
//!
//! A region is the list's consumer: one wire on the list's head (`patch_list`) replays the turn's delta log
//! (`list::Op`) on the region's window. Each row is an owner of its own (the item, at its stable slab
//! address) whose wires were installed by one monomorphic row function, so an edited item settles only its
//! own queued signals (`Rt::settle_cls` with the region's owner class).
//!
//! # Representation
//! * **Rows are independent groups stacked by a prefix sum.** A list's column has no layout of its own to
//!   do: row `k` sits at `y_k = Σ_{j<k} (h_j + gap)`. The container is a *sized leaf* of its parent's layout,
//!   each row's top box is a **layout group of its own** in a **space of its own** (instances are row-local;
//!   the space adds `origin + (0, y_k - scroll)`). An edit lays out one row; a height change, a splice or a
//!   scroll moves rows by space writes plus `scroll_run`s (bough-core page offsets, O(pages)).
//! * **The prefix index is per position, derived from heights per slot** (`mlist::HeightIndex`, see there).
//!   A measured height belongs to the item (`hs[slot]`); an unmounted row counts its last measured height or
//!   the estimate. After a turn's log the index is rebuilt once from the lowest position the log touched.
//! * **The window** is the contiguous run of positions `lo..lo + rows.len()` whose rows are mounted (holes:
//!   positions whose row is not mounted yet). Without a viewport the window is the whole list (the container
//!   is then as tall as the stack). With one, it is the rows meeting the viewport ± `overscan`. The log
//!   replay is bookkeeping only (the window follows the splices; moved / permuted rows keep their records
//!   and are re-placed; rows that leave the window are unmounted; positions that enter it become holes); the
//!   placement (`lists_place`, between a layout compute and the write-back) mounts the holes, measures what
//!   was laid out, and places.
//! * **Anchoring** (the measured list's semantics): the first visible row and the offset inside it are fixed
//!   while measurement runs, so a row measured above the viewport moves the scroll offset by the difference,
//!   not the content; a jump converges inside its frame (mount around the target, measure, re-derive the
//!   offset from the anchor, repeat until the range is stable). The driver mirrors the settled offset back
//!   (`list_scroll_pt`, `Signal::set_silent`). Data changes (splices, moves) keep the pixel offset.
//! * **Moves animate** (`.animate_moves()`, FLIP): a list change places rows at their new target but each
//!   moved row's displayed offset `cur` holds it where it was; `App::tick` eases every `cur` to zero with one
//!   spring evaluation per frame and, per moving row, a space write plus a `scroll_run` of its records (no
//!   layout, no instance rewrite). A row whose whole path stays outside its clip snaps to its target at once.
//! * The rows' records are one contiguous preorder run right after the container's record.
use crate::color::Color;
use crate::elem::{quad_instance, Element, Mounter};
use crate::layout::{Dim, Spec, Styled};
use crate::list::{LWired, List, Op};
use crate::mlist::HeightIndex;
use crate::rt::*;
use crate::signal::{Signal, Wired};
use bough_core::{Handle, NRect, RecordInit};

/// A window row: 48 bytes (its scope lives in the regions' scope slab, so window splices move small rows).
pub struct LRow {
    pub slot: u32,
    pub nrec: u32,
    /// Index of its scope in `Lists::scopes`.
    pub scope: u32,
    pub space: u16,
    /// Mounted (false: a hole, a window position whose row is not mounted yet).
    pub mounted: bool,
    /// Placed: its space holds its position and its records' ink follows it.
    pub placed: bool,
    /// The row's layout group (its top box), or `NONE`.
    pub root: u32,
    /// Target content y (device px from the content top) it was last placed at.
    pub y: i64,
    /// Displayed minus target content y (device px): a move animating; 0 at rest.
    pub cur: i32,
    /// The animation's start offset (device px).
    pub d0: f32,
    /// Displacement its records' ink has not taken yet (device px): a moving row outside its clip is moved by
    /// its space alone; its ink catches up when it is about to be seen, or when the motion ends.
    pub pend: i32,
}

impl LRow {
    fn hole() -> LRow {
        LRow { slot: NONE, nrec: 0, scope: NONE, space: 0, mounted: false, placed: false, root: NONE, y: 0, cur: 0, d0: 0.0, pend: 0 }
    }
}

/// Mount the row of `slot` (a list's slot id, or an index) of region `reg` from its source.
pub type LRowMountFn = unsafe fn(&mut Rt, u32, *const u8, u32, &mut Vec<RecordInit>) -> LRow;

#[derive(Default, Clone, Copy, Debug)]
pub struct LStats {
    pub passes: u64,
    pub rows_mounted: u64,
    pub rows_unmounted: u64,
    pub height_changes: u64,
}

/// A damped spring solved in closed form (anime.js `createSpring`, zgx `anime::ease::Spring`); its settling
/// time (within 1e-3 of rest for 20 samples at 120 Hz) is the length of a move.
#[derive(Clone, Copy, Debug)]
pub struct Spring {
    zeta: f64,
    w0: f64,
    wd: f64,
    b: f64,
    /// Seconds until it settles.
    pub settle: f64,
    /// Bounds of `1 - solve(t)` over `0..settle` (a moving row's path is `d0 * [e_lo, e_hi]`).
    e_lo: f64,
    e_hi: f64,
}

impl Spring {
    pub fn new(mass: f64, stiffness: f64, damping: f64, velocity: f64) -> Spring {
        let mass = mass.clamp(1e-3, 1e4);
        let stiffness = stiffness.clamp(1e-3, 1e5);
        let damping = damping.clamp(1e-3, 1e4);
        let w0 = (stiffness / mass).sqrt();
        let zeta = damping / (2. * (stiffness * mass).sqrt());
        let wd = if zeta < 1. { w0 * (1. - zeta * zeta).sqrt() } else { 0. };
        let b = if zeta < 1. { (zeta * w0 - velocity) / wd } else { -velocity + w0 };
        let mut s = Spring { zeta, w0, wd, b, settle: 0., e_lo: 0., e_hi: 1. };
        // settled once it stays within a thousandth of rest for a sixth of a second, sampled at 120 Hz
        let (step, hold) = (1. / 120., 20);
        let (mut time, mut still) = (0f64, 0);
        while still < hold && time < 60. {
            time += step;
            if (1. - s.solve(time)).abs() < 1e-3 {
                still += 1;
            } else {
                still = 0;
            }
        }
        s.settle = (time - step * hold as f64).max(step);
        let (mut lo, mut hi) = (0f64, 1f64);
        for i in 0..=2048 {
            let e = 1. - s.solve(s.settle * i as f64 / 2048.);
            lo = lo.min(e);
            hi = hi.max(e);
        }
        s.e_lo = lo - 2e-3;
        s.e_hi = hi + 2e-3;
        s
    }
    /// From perceived `bounce` (-1..1, 0 = critically damped) and duration in seconds (anime.js `spring()`).
    pub fn perceptual(bounce: f64, duration: f64) -> Spring {
        let duration = duration.max(1e-3);
        let stiffness = (2. * std::f64::consts::PI / duration).powi(2);
        let ratio = (1. - bounce.clamp(-1., 0.99)).max(0.01);
        Spring::new(1., stiffness, 2. * ratio * stiffness.sqrt(), 0.)
    }
    /// zgx's default for layout moves: `spring { bounce = 0.1, duration = 0.4 }`.
    pub fn moves() -> Spring {
        Spring::perceptual(0.1, 0.4)
    }
    /// Progress at `time` seconds (0 → 1).
    pub fn solve(&self, time: f64) -> f64 {
        let t = if self.zeta < 1. {
            (-time * self.zeta * self.w0).exp() * ((self.wd * time).cos() + self.b * (self.wd * time).sin())
        } else {
            (1. + self.b * time) * (-time * self.w0).exp()
        };
        1. - t
    }
    /// The remaining fraction of a move at `time` (1 → 0; exactly 0 once settled).
    #[inline]
    pub fn remaining(&self, time: f64) -> f64 {
        if time >= self.settle { 0. } else { 1. - self.solve(time) }
    }
}

/// A region's moves in flight: one clock and one spring; per moving row its window index, its record offset
/// in the window's run and its start offset.
pub struct Motion {
    pub spring: Spring,
    pub t: f64,
    pub rows: Vec<(u32, u32, f32)>,
}

pub struct LRegion {
    pub(crate) cls: u32,
    /// The container's record: the rows' run starts right after it.
    pub(crate) anchor: Handle,
    pub(crate) lid: u32,
    pub(crate) depth: u16,
    /// The rows' clip (a virtual list's own, cut to its box; else the parent's) and the parent's.
    pub(crate) clip: u16,
    pub(crate) pclip: u16,
    /// A virtual list's viewport rect (absolute device px): its rows' clip.
    pub vpr: NRect,
    /// The container's spec: the user's, padding and gap stripped (applied by the region), the height the
    /// stack's (`auto_h`) or the user's.
    pub(crate) spec: Spec,
    pub(crate) auto_h: bool,
    pub(crate) gap: i32,
    pub(crate) pad: [f32; 4],
    /// Content origin (absolute device px, unrounded) and width; NaN until laid out.
    pub(crate) origin: [f32; 2],
    pub(crate) width: f32,
    /// The content height the container is styled with.
    pub(crate) total: i64,
    pub(crate) dirty: bool,
    /// A row group of this region was laid out since the last measure.
    pub(crate) remeasure: bool,
    /// The list's log changed positions this turn (moves animate).
    pub(crate) moved: bool,
    /// Its scope was released: never placed again.
    pub(crate) dead: bool,
    /// Pitches (height + gap, device px) by position, and per slot (the measured height belongs to the item).
    pub idx: HeightIndex,
    pub hs: Vec<i32>,
    /// What an unmeasured row counts as (pitch, device px).
    pub est: i32,
    /// The window: positions `lo..lo + rows.len()`.
    pub lo: usize,
    pub rows: Vec<LRow>,
    /// Virtual: a viewport `vh` px tall (the container's height) scrolled by `s` (settled) / `req` (asked),
    /// device px.
    pub virt: bool,
    pub vh: f32,
    pub s: i64,
    pub req: i64,
    /// Anchoring's correction of the asked offset in the last window pass.
    da: i64,
    /// Window index from which rows' targets changed since the last placement (`usize::MAX`: none). The rows
    /// before it only shift with the content origin (a scroll, the container moving): one constant for all
    /// of them, applied as one `scroll_run` over their records.
    stale: usize,
    /// The screen position of the content's top (x, y - scroll) at the last placement. (A row mounted since
    /// is not placed yet: it is placed at its target wherever it is.)
    pub(crate) placed_at: [i64; 2],
    pub overscan: usize,
    /// The row of an enclosing region this region was mounted in (region, slot), or `(NONE, NONE)`: its
    /// rows' records are part of that row's run, so the row's count follows them (`lr_count`).
    pub(crate) parent: (u32, u32),
    /// The space the container is placed in (moving it re-windows the region: `Rt::carry_space`).
    pub(crate) cspace: u16,
    /// What the region last windowed against: without a viewport of its own, the visible rect (its clip ∩
    /// the window, absolute device px). A change is an exposure (or a hiding): the window follows.
    pub(crate) vis: NRect,
    /// No estimate was given: the first measured rows set it.
    pub(crate) est_auto: bool,
    pub motion: Option<Motion>,
    pub(crate) animate: Option<Spring>,
    pub stats: LStats,
    /// The row source: the `List<T>` field, or for an index source the owner (a stable address either way:
    /// a field of the region's owner, or the owner, which outlives the region).
    pub(crate) list: *const u8,
    pub(crate) mount: LRowMountFn,
    pub(crate) slot_at: unsafe fn(*const u8, usize) -> u32,
    pub(crate) item: unsafe fn(*const u8, u32) -> *const u8,
    pub(crate) set_wired: unsafe fn(*const u8, &[u32]),
    /// The row function `fn(&T, &mut Mounter<T>)`, type-erased (transmuted back by `mount`).
    pub(crate) row_fn: usize,
    pub(crate) row_size: usize,
}

impl LRegion {
    /// The stack's height (device px, no padding).
    #[inline]
    fn content(&self) -> i64 {
        if self.idx.is_empty() { 0 } else { self.idx.total() - self.gap as i64 }
    }
}

/// An unmaterialized run of a list region's rows (`Rt::stubs`).
#[derive(Clone, Debug, PartialEq)]
pub struct Stub {
    pub region: u32,
    pub rows: std::ops::Range<usize>,
    /// Its extent (absolute device px).
    pub rect: NRect,
}

/// The list regions of a runtime, and the lists to close at the end of the turn's settle.
#[derive(Default)]
pub struct Lists {
    /// Private: a region's index names its row functions (monomorphised for its source); a reordered or
    /// edited region table would run them on the wrong source. Read through `regs()`.
    pub(crate) regs: Vec<LRegion>,
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
    /// The mounted rows' scopes (a slab: `LRow::scope` indexes it).
    scopes: Vec<Scope>,
    scope_free: Vec<u32>,
    /// The rows being mounted (region, slot), innermost last: a region mounted inside one is nested in it.
    pub(crate) mounting: Vec<(u32, u32)>,
}

impl Lists {
    /// List regions as memory-ledger lines.
    pub fn ledger(&self, l: &mut bough_mem::Ledger) {
        use bough_mem::Scales::Content;
        let regs: usize = self.regs.capacity() * size_of::<LRegion>() + self.regs.iter().map(|r| r.hs.capacity() * 4 + r.rows.capacity() * size_of::<LRow>() + r.idx.heap_bytes()).sum::<usize>();
        l.many("lists", format!("list regions ({})", self.regs.len()), Content, regs, regs);
        let scopes: usize = self.scopes.capacity() * size_of::<Scope>() + self.scopes.iter().map(Scope::heap_bytes).sum::<usize>() + self.scope_free.capacity() * 4;
        l.many("lists", "list row scopes", Content, scopes, scopes);
        let misc = self.ends.capacity() * 16 + (self.root_reg.capacity() + self.root_h.capacity() + self.perm.capacity() + self.map.capacity()) * 4 + (self.stage.capacity() + self.stage2.capacity()) * size_of::<RecordInit>() + self.ops.capacity() * size_of::<Op>() + self.mounting.capacity() * 8;
        l.many("lists", "list tables and scratch", Content, misc, misc);
    }
    /// The regions, read-only.
    pub fn regs(&self) -> &[LRegion] {
        &self.regs
    }
    fn put_scope(&mut self, s: Scope) -> u32 {
        match self.scope_free.pop() {
            Some(i) => {
                self.scopes[i as usize] = s;
                i
            }
            None => {
                self.scopes.push(s);
                (self.scopes.len() - 1) as u32
            }
        }
    }
    fn take_scope(&mut self, i: u32) -> Scope {
        self.scope_free.push(i);
        std::mem::take(&mut self.scopes[i as usize])
    }
    /// Is a move animating in some region (`App::tick` advances it)?
    pub fn animating(&self) -> bool {
        self.regs.iter().any(|r| !r.dead && r.motion.as_ref().is_some_and(|m| !m.rows.is_empty()))
    }
}

/// # Safety
/// `l` is a live `List<T>` that no other reference is borrowing mutably.
unsafe fn end_list<T>(l: *const u8) {
    // SAFETY: the function contract.
    unsafe { (*(l as *mut List<T>)).end_turn() };
}

/// # Safety
/// `l` points at a live `List<T>` with more than `p` items.
unsafe fn list_slot_at<T>(l: *const u8, p: usize) -> u32 {
    // SAFETY: the function contract.
    unsafe { (&(*(l as *const List<T>)).order_slots())[p] }
}
/// An index source: the row at position `p` is row `p`.
unsafe fn index_slot_at(_: *const u8, p: usize) -> u32 {
    p as u32
}
/// An index source's rows are owned by the source's owner itself.
///
/// # Safety
/// None needed: returns `o` unread.
unsafe fn index_item(o: *const u8, _: u32) -> *const u8 {
    o
}
unsafe fn index_set_wired(_: *const u8, _: &[u32]) {}

/// # Safety
/// `l` points at a live `List<T>` and `s` is one of its slot ids (a removed item's address is computed,
/// not read).
unsafe fn list_item<T>(l: *const u8, s: u32) -> *const u8 {
    // SAFETY: the function contract.
    unsafe { (*(l as *const List<T>)).slot_ptr(s) as *const u8 }
}

/// # Safety
/// `l` points at a live `List<T>`; `c` holds `offset_of!`-proven headed fields of `T`.
unsafe fn list_set_wired<T>(l: *const u8, c: &[u32]) {
    // SAFETY: the function contract; `wired` is a RefCell (shared access).
    let w = unsafe { &(*(l as *const List<T>)).wired };
    if w.borrow().len() != c.len() {
        *w.borrow_mut() = c.to_vec();
    }
}

/// # Safety
/// `l` is a live `List<T>` and `slot` a live slot of it; `reg` was created for a `fn(&T, &mut Mounter<T>)`.
unsafe fn mount_lrow<T: 'static>(rt: &mut Rt, reg: u32, l: *const u8, slot: u32, recs: &mut Vec<RecordInit>) -> LRow {
    // SAFETY: the function contract.
    let row = unsafe { list_item::<T>(l, slot) };
    let (cls, f) = {
        let r = &rt.lists.regs[reg as usize];
        (r.cls, r.row_fn)
    };
    // SAFETY: the region was created by `LList::mount` from a `fn(&T, &mut Mounter<T>)`.
    let f: fn(&T, &mut Mounter<'_, T>) = unsafe { std::mem::transmute::<usize, fn(&T, &mut Mounter<'_, T>)>(f) };
    // SAFETY: a live slot holds an initialised `T`, the row's owner (class `cls`).
    unsafe { mount_row(rt, reg, row as *const T, cls, recs, |m| f(&*(row as *const T), m)) }
}

/// # Safety
/// `o` is the live owner `V` of an index source; `reg` was created for a `fn(&V, usize, &mut Mounter<V>)`.
unsafe fn mount_irow<V: 'static>(rt: &mut Rt, reg: u32, o: *const u8, i: u32, recs: &mut Vec<RecordInit>) -> LRow {
    let f = rt.lists.regs[reg as usize].row_fn;
    // SAFETY: the region was created by `LList::mount` from a `fn(&V, usize, &mut Mounter<V>)`.
    let f: fn(&V, usize, &mut Mounter<'_, V>) = unsafe { std::mem::transmute::<usize, fn(&V, usize, &mut Mounter<'_, V>)>(f) };
    // the rows' operands are the owner's own fields: its class (0, the root's) settles them
    // SAFETY: the function contract.
    unsafe { mount_row(rt, reg, o as *const V, 0, recs, |m| f(&*(o as *const V), i as usize, m)) }
}

/// Mount one row of region `reg`: its own space and layout group, owner `owner` of class `cls`.
///
/// # Safety
/// `owner` is the live owner `body` mounts for, of class `cls`.
unsafe fn mount_row<O>(rt: &mut Rt, reg: u32, owner: *const O, cls: u32, recs: &mut Vec<RecordInit>, body: impl FnOnce(&mut Mounter<'_, O>)) -> LRow {
    let (depth, clip, width, cspace) = {
        let r = &rt.lists.regs[reg as usize];
        (r.depth, r.clip, r.width, rt.lay.info[r.lid as usize].space)
    };
    // nested in the container's space: a row moves with whatever moves the list
    let space = rt.alloc_space_in(cspace);
    rt.set_space(space, [0.0, 0.0]);
    let before = recs.len();
    let (prev, avail) = (rt.lay.cur, rt.lay.mount_avail);
    rt.lay.cur = NONE;
    rt.lay.mount_avail = (width, f32::NAN);
    let mut scope = Scope::default();
    {
        let mut m = Mounter { rt, owner, recs, scope: &mut scope, depth, ox: 0.0, oy: 0.0, space, clip, pending_num: None, cls, tape: std::ptr::null_mut(), ti: 0, pos: 0, kid: 0, keep: false };
        body(&mut m);
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
    } else {
        // nothing will be laid out: its height (0) is measured as it is
        rt.lists.regs[reg as usize].remeasure = true;
    }
    rt.lists.regs[reg as usize].stats.rows_mounted += 1;
    let scope = rt.lists.put_scope(scope);
    LRow { slot: 0, nrec: (recs.len() - before) as u32, scope, space, mounted: true, placed: false, root, y: 0, cur: 0, d0: 0.0, pend: 0 }
}

fn view_row<T: crate::app::View>(v: &T, m: &mut Mounter<'_, T>) {
    // a row is an owner: its `changed` runs when one of its boundary cells publishes
    if m.rt.class_changed[m.cls as usize].is_none() {
        m.rt.class_changed[m.cls as usize] = Some(crate::changed::changed_shim::<T>);
    }
    v.view().mount(m);
    // the row's `Action` fields (action.rs)
    crate::action::mount_actions::<T>(m);
}

impl Rt {
    fn lr_base(&self, reg: usize) -> usize {
        self.bough.pos_of(self.lists.regs[reg].anchor).expect("list container is in the Bough") + 1
    }
    /// Region `reg` inserted (`d > 0`) or removed records: the rows of the enclosing regions it is nested in
    /// count them (their runs contain it).
    fn lr_count(&mut self, reg: usize, d: i64) {
        let mut p = self.lists.regs[reg].parent;
        while p.0 != NONE {
            let r = &mut self.lists.regs[p.0 as usize];
            if r.dead {
                return;
            }
            match r.rows.iter_mut().find(|x| x.mounted && x.slot == p.1) {
                Some(row) => row.nrec = (row.nrec as i64 + d) as u32,
                None => return,
            }
            p = r.parent;
        }
    }
    fn lr_prefix(&self, reg: usize, k: usize) -> usize {
        self.lists.regs[reg].rows[..k].iter().map(|r| r.nrec as usize).sum()
    }
    /// Release a row: its space, its resources (its layout group is a root of its scope). Row-local wires
    /// (whose head lies in the row's own memory) are freed now: nothing else can reach them (the item's
    /// heads are cleared by `List::free_slot`, or here when the item stays: `live`).
    fn lr_release_row(&mut self, row: LRow, row_ptr: usize, size: usize, live: bool, cls: u32) {
        if !row.mounted {
            return;
        }
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
        self.free_space(row.space);
        let mut scope = self.lists.take_scope(row.scope);
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
            if !row.mounted {
                continue;
            }
            if row.root != NONE {
                self.lists.root_reg[row.root as usize] = 0;
            }
            self.free_space(row.space);
            let scope = self.lists.take_scope(row.scope);
            self.release_scope(scope);
        }
        let r = &mut self.lists.regs[reg];
        r.dirty = false;
        r.dead = true;
        r.motion = None;
        let cs = r.cspace;
        self.space_reg(cs, reg as u32, false);
    }

    // ------------------------------------------------------------------ the window under the log

    /// Unmount window rows `k..k + n` (records removed, rows released; `live`: the items stay in the list).
    fn lw_unmount(&mut self, reg: usize, list: *const u8, k: usize, n: usize, live: bool) {
        if n == 0 {
            return;
        }
        let nrec: usize = self.lists.regs[reg].rows[k..k + n].iter().map(|r| r.nrec as usize).sum();
        if nrec > 0 {
            let at = self.lr_base(reg) + self.lr_prefix(reg, k);
            self.bough.remove_run(at, nrec);
            self.lr_count(reg, -(nrec as i64));
        }
        let (cls, size, item) = {
            let r = &self.lists.regs[reg];
            (r.cls, r.row_size, r.item)
        };
        let gone: Vec<LRow> = self.lists.regs[reg].rows.drain(k..k + n).collect();
        for row in gone {
            if row.mounted {
                self.lists.regs[reg].stats.rows_unmounted += 1;
                // SAFETY: slab addresses stay valid for the list's life (a removed item's is computed, not read).
                let p = unsafe { item(list, row.slot) } as usize;
                self.lr_release_row(row, p, size, live, cls);
            }
        }
    }

    /// `n` items were inserted at `at`: before the window it shifts; inside it they are holes.
    fn lw_insert(&mut self, reg: usize, at: usize, n: usize) {
        let r = &mut self.lists.regs[reg];
        let hi = r.lo + r.rows.len();
        if at <= r.lo {
            r.lo += n;
        } else if at < hi {
            let k = at - r.lo;
            r.rows.splice(k..k, (0..n).map(|_| LRow::hole()));
        }
    }

    /// Items `at..at + n` were removed.
    fn lw_remove(&mut self, reg: usize, list: *const u8, at: usize, n: usize) {
        let (lo, hi) = {
            let r = &self.lists.regs[reg];
            (r.lo, r.lo + r.rows.len())
        };
        let (a, b) = (at.max(lo), (at + n).min(hi));
        if a < b {
            self.lw_unmount(reg, list, a - lo, b - a, false);
        }
        self.lists.regs[reg].lo -= n.min(lo.saturating_sub(at));
    }

    /// The item at `from` moved to `to`.
    fn lw_move(&mut self, reg: usize, list: *const u8, from: usize, to: usize) {
        let (lo, hi) = {
            let r = &self.lists.regs[reg];
            (r.lo, r.lo + r.rows.len())
        };
        if from.max(to) < lo || from.min(to) >= hi {
            return;
        }
        let src: Vec<u32> = (lo..hi)
            .map(|p| {
                let q = if p == to {
                    from
                } else if from < to && p >= from && p < to {
                    p + 1
                } else if to < from && p > to && p <= from {
                    p - 1
                } else {
                    p
                };
                if q >= lo && q < hi { (q - lo) as u32 } else { NONE }
            })
            .collect();
        self.lw_remap(reg, list, &src);
    }

    /// Positions `at..at + slots.len()` now hold `slots` (a permutation of what they held).
    fn lw_permute(&mut self, reg: usize, list: *const u8, at: usize, slots: &[u32]) {
        let (lo, hi) = {
            let r = &self.lists.regs[reg];
            (r.lo, r.lo + r.rows.len())
        };
        let n = slots.len();
        if at + n <= lo || at >= hi {
            return;
        }
        let mut map = std::mem::take(&mut self.lists.map);
        for (k, row) in self.lists.regs[reg].rows.iter().enumerate() {
            if row.mounted {
                let s = row.slot as usize;
                if map.len() <= s {
                    map.resize(s + 1, NONE);
                }
                map[s] = k as u32;
            }
        }
        let src: Vec<u32> = (lo..hi)
            .map(|p| if p >= at && p < at + n { map.get(slots[p - at] as usize).copied().unwrap_or(NONE) } else { (p - lo) as u32 })
            .collect();
        for row in &self.lists.regs[reg].rows {
            if row.mounted {
                map[row.slot as usize] = NONE;
            }
        }
        self.lists.map = map;
        self.lw_remap(reg, list, &src);
    }

    /// The window's positions now hold `src[k]`: the old window row `src[k]`, or a hole (`NONE`). Rows not
    /// kept are unmounted (their items stay); the kept rows' records are rewritten in the new order (one
    /// `read_run` + `rewrite_run`, handles follow) and re-placed by `lists_place`.
    fn lw_remap(&mut self, reg: usize, list: *const u8, src: &[u32]) {
        let rows = std::mem::take(&mut self.lists.regs[reg].rows);
        let len = rows.len();
        let base = self.lr_base(reg);
        let mut used = vec![false; len];
        for &s in src {
            if s != NONE {
                used[s as usize] = true;
            }
        }
        // the records of rows not kept leave (back to front: earlier offsets stay valid)
        let mut roff = Vec::with_capacity(len);
        let mut acc = 0usize;
        for r in &rows {
            roff.push(acc);
            acc += r.nrec as usize;
        }
        for k in (0..len).rev() {
            if !used[k] && rows[k].nrec > 0 {
                self.bough.remove_run(base + roff[k], rows[k].nrec as usize);
                self.lr_count(reg, -(rows[k].nrec as i64));
            }
        }
        // the kept rows' records, contiguous in their old order: rewrite them in the new one
        let mut koff = vec![0usize; len];
        let mut kept = 0usize;
        for k in 0..len {
            koff[k] = kept;
            if used[k] {
                kept += rows[k].nrec as usize;
            }
        }
        let mut last = None;
        let reordered = src.iter().filter(|&&s| s != NONE).any(|&s| {
            let back = last.is_some_and(|l| s <= l);
            last = Some(s);
            back
        });
        if reordered && kept > 0 {
            let mut stage = std::mem::take(&mut self.lists.stage);
            let mut out = std::mem::take(&mut self.lists.stage2);
            stage.clear();
            out.clear();
            // exact rects travel with the records (a reordered row may lie outside the 16-bit domain)
            let (mut wide, mut wide_out) = (Vec::with_capacity(kept), Vec::with_capacity(kept));
            self.bough.read_run_wide(base, kept, &mut stage, &mut wide);
            for &s in src {
                if s != NONE {
                    let (a, n) = (koff[s as usize], rows[s as usize].nrec as usize);
                    out.extend_from_slice(&stage[a..a + n]);
                    wide_out.extend_from_slice(&wide[a..a + n]);
                }
            }
            self.bough.rewrite_run_wide(base, &out, &wide_out);
            self.lists.stage = stage;
            self.lists.stage2 = out;
        }
        let mut old: Vec<Option<LRow>> = rows.into_iter().map(Some).collect();
        let new: Vec<LRow> = src.iter().map(|&s| if s == NONE { LRow::hole() } else { old[s as usize].take().unwrap() }).collect();
        self.lists.regs[reg].rows = new;
        let (cls, size, item) = {
            let r = &self.lists.regs[reg];
            (r.cls, r.row_size, r.item)
        };
        for row in old.into_iter().flatten() {
            if row.mounted {
                self.lists.regs[reg].stats.rows_unmounted += 1;
                // SAFETY: the item is live (moved within the list); slab addresses are stable.
                let p = unsafe { item(list, row.slot) } as usize;
                self.lr_release_row(row, p, size, true, cls);
            }
        }
    }

    /// Replay list `l`'s delta log on region `reg`'s window (see the module docs), rebuild the index from the
    /// lowest position the log touched, then settle the edited rows.
    ///
    /// # Safety
    /// `l` points at the live `List<T>` registered for region `reg` (the wire contract of `patch_list`).
    unsafe fn lr_replay<T: 'static>(&mut self, reg: usize, l: *const List<T>) {
        let lp = l as *const u8;
        self.lists.regs[reg].list = lp;
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
        let mut low = usize::MAX;
        let mut i = 0;
        while i < ops.len() {
            match ops[i] {
                Op::Insert { at, .. } => {
                    let mut next = at;
                    let est = self.lists.regs[reg].est;
                    while let Some(&Op::Insert { at: a, slot }) = ops.get(i) {
                        if a != next {
                            break;
                        }
                        // a new item (a slot is reused only in a later turn): unmeasured
                        let hs = &mut self.lists.regs[reg].hs;
                        if hs.len() <= slot as usize {
                            hs.resize(slot as usize + 1, est);
                        }
                        hs[slot as usize] = est;
                        next += 1;
                        i += 1;
                    }
                    low = low.min(at as usize);
                    self.lw_insert(reg, at as usize, (next - at) as usize);
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
                    low = low.min(lo as usize);
                    self.lw_remove(reg, lp, lo as usize, (hi - lo) as usize);
                }
                Op::Move { from, to } => {
                    low = low.min(from.min(to) as usize);
                    self.lw_move(reg, lp, from as usize, to as usize);
                    i += 1;
                }
                Op::Permute { at, n, off } => {
                    low = low.min(at as usize);
                    self.lw_permute(reg, lp, at as usize, &perm[off as usize..(off + n) as usize]);
                    i += 1;
                }
                Op::Edit { .. } => i += 1,
            }
        }
        // SAFETY: shared read of the live list.
        let order = unsafe { (*l).order_slots() };
        if low != usize::MAX {
            let r = &mut self.lists.regs[reg];
            let hs = &r.hs;
            r.idx.assign_from(low, order.len(), |p| hs[order[p] as usize]);
            r.moved = true;
            r.dirty = true;
            r.stale = 0;
        }
        // the window now follows the list's order
        debug_assert!(
            {
                let r = &self.lists.regs[reg];
                r.lo + r.rows.len() <= order.len() && r.rows.iter().enumerate().all(|(k, x)| !x.mounted || x.slot == order[r.lo + k])
            },
            "list region out of step with its list"
        );
        // edited rows: their queued wires (an item removed or not mounted has none queued)
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

    /// A group was just laid out: if it is a list row, its region measures again (and re-places if the
    /// row's height changed).
    #[inline]
    pub(crate) fn lists_root_computed(&mut self, root: u32) {
        let Some(&r) = self.lists.root_reg.get(root as usize) else { return };
        if r != 0 {
            let changed = self.lists.root_h[root as usize] != self.lay.root_height(root);
            let reg = &mut self.lists.regs[r as usize - 1];
            reg.remeasure = true;
            reg.dirty |= changed;
        }
    }

    // ------------------------------------------------------------------ placement

    /// Place every region whose rows or container changed (after a compute pass, before the write-back;
    /// see `lay_flush`): container origin and size from its computed layout, the window from the scroll
    /// offset (anchored), its rows mounted, laid out and measured, `y` by prefix sum, spaces written and
    /// records translated for rows that moved, the container restyled when the stack's height changed.
    /// Returns whether layout is queued (the caller computes it and places again).
    pub(crate) fn lists_place(&mut self) -> bool {
        for _ in 0..8 {
            let mut again = false;
            let mut reg = 0;
            // regions mounted by rows placed now are placed in the same pass
            while reg < self.lists.regs.len() {
                again |= self.lr_place_region(reg);
                reg += 1;
            }
            if !self.lay.pending.is_empty() {
                return true;
            }
            // a container restyled and already laid out (inside a window pass), or a region dirtied by
            // another's rows: one more pass
            if !again && !self.lists.regs.iter().any(|r| r.dirty && !r.dead) {
                return false;
            }
        }
        false
    }

    fn lr_place_region(&mut self, reg: usize) -> bool {
        let lid = self.lists.regs[reg].lid;
        if self.lists.regs[reg].dead || !self.lay.info[lid as usize].live {
            return false;
        }
        // the container's group-local position: the sum of the rects up its parent chain
        let (mut ax, mut ay, mut p) = (0.0f32, 0.0f32, lid);
        while p != NONE {
            let (x, y, _, _) = self.lay.eng.rect(p);
            ax += x;
            ay += y;
            p = self.lay.info[p as usize].parent;
        }
        let (_, _, aw, ah) = self.lay.eng.rect(lid);
        let so = self.space(self.lay.info[lid as usize].space);
        let (origin, width, vh, virt) = {
            let r = &self.lists.regs[reg];
            ([so[0] + ax + r.pad[0], so[1] + ay + r.pad[1]], aw - r.pad[0] - r.pad[2], if r.virt { ah } else { r.vh }, r.virt)
        };
        let (moved, resized, mut vres) = {
            let r = &self.lists.regs[reg];
            (origin != r.origin, width != r.width, vh != r.vh)
        };
        if !virt {
            // no viewport of its own: what is visible of it is its clip ∩ the window (an exposure when it changes)
            let vis = self.lr_visible(reg);
            if vis != self.lists.regs[reg].vis {
                self.lists.regs[reg].vis = vis;
                vres = true;
            }
        }
        if !(self.lists.regs[reg].dirty || moved || resized || vres) {
            return false;
        }
        if virt && (moved || resized || vres) {
            // the viewport: the container's box, cut by the parent's clip
            let (x0, y0) = (so[0] + ax, so[1] + ay);
            let b = NRect::new(x0.floor() as i32, y0.floor() as i32, (x0 + aw).ceil() as i32, (y0 + ah).ceil() as i32);
            let (clip, pclip) = (self.lists.regs[reg].clip, self.lists.regs[reg].pclip);
            let rect = b.intersect(self.bough.clip_rect(pclip)).canon();
            if rect != self.bough.clip_rect(clip) {
                self.bough.set_clip_rect(clip, rect);
            }
            self.lists.regs[reg].vpr = rect;
        }
        {
            let r = &mut self.lists.regs[reg];
            r.origin = origin;
            r.vh = vh;
        }
        if resized {
            let r = &mut self.lists.regs[reg];
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
        }
        self.lr_window(reg);
        self.lr_place(reg);
        let r = &mut self.lists.regs[reg];
        r.dirty = false;
        // the stack's height is the container's
        if r.auto_h {
            let total = r.content();
            if total != r.total {
                let mut spec = r.spec;
                spec.h = Dim::Px(total as f32 + r.pad[1] + r.pad[3]);
                r.total = total;
                r.spec = spec;
                crate::layout::restyle(self, lid, &spec);
                return true;
            }
        }
        false
    }

    /// Settle region `reg`'s window: from the asked offset (anchored: the first visible row and the offset
    /// inside it hold while rows are measured), mount the rows in range, lay them out, measure them, and
    /// repeat until the range is stable (1-3 passes).
    fn lr_window(&mut self, reg: usize) {
        let (virt, pad_t, pad_b, ov) = {
            let r = &self.lists.regs[reg];
            (r.virt, r.pad[1].round() as i64, r.pad[3].round() as i64, r.overscan)
        };
        let s_req = if virt { self.lists.regs[reg].req } else { 0 };
        let mut s = s_req;
        let mut anchor: Option<(usize, i64)> = None;
        for _pass in 0..32 {
            let r = &mut self.lists.regs[reg];
            r.stats.passes += 1;
            let n = r.idx.len();
            let range = if r.est_auto && r.stats.rows_mounted == 0 {
                // no estimate yet: a few rows are measured first
                0..n.min(8)
            } else if virt {
                let vh = r.vh.round() as i64;
                s = s.clamp(0, (r.content() + pad_t + pad_b - vh).max(0));
                let c = s - pad_t;
                let idx = &r.idx;
                anchor.get_or_insert_with(|| {
                    let f = idx.row_at(c);
                    (f, c - idx.prefix(f))
                });
                if n == 0 {
                    0..0
                } else {
                    let first = idx.row_at(c.max(0));
                    let last = idx.row_at((c + vh - 1).max(0));
                    first.saturating_sub(ov)..(last + 1 + ov).min(n)
                }
            } else {
                // the rows meeting the visible rect (± `overscan` rows); none if it is empty
                let top = r.origin[1].round() as i64;
                let [_, vy0, _, vy1] = r.vis.to_xyxy();
                let content = r.content();
                let (a, b) = (vy0 as i64 - top, vy1 as i64 - top);
                if n == 0 || r.vis.is_empty() || b <= 0 || a >= content {
                    0..0
                } else {
                    let idx = &r.idx;
                    let first = idx.row_at(a.max(0));
                    let last = idx.row_at((b - 1).clamp(0, content - 1));
                    first.saturating_sub(ov)..(last + 1 + ov).min(n)
                }
            };
            self.lr_set_range(reg, range.start, range.end);
            if !self.lay.pending.is_empty() {
                self.lay_compute_pending();
            }
            if !self.lr_measure(reg) {
                break;
            }
            if let Some((f, w)) = anchor {
                s = self.lists.regs[reg].idx.prefix(f) + w + pad_t;
            }
        }
        let r = &mut self.lists.regs[reg];
        r.da = s - s_req;
        r.s = s;
        r.req = s;
    }

    /// The window becomes positions `a..b`: rows outside it are unmounted, positions that enter it are holes,
    /// then every hole is mounted.
    fn lr_set_range(&mut self, reg: usize, a: usize, b: usize) {
        let (lo, len, list) = {
            let r = &self.lists.regs[reg];
            (r.lo, r.rows.len(), r.list)
        };
        let hi = lo + len;
        if len == 0 || b <= lo || a >= hi {
            self.lw_unmount(reg, list, 0, len, true);
            let r = &mut self.lists.regs[reg];
            r.lo = a;
            r.stale = 0;
            r.rows.extend((a..b).map(|_| LRow::hole()));
        } else {
            let back = hi.saturating_sub(b);
            self.lw_unmount(reg, list, len - back, back, true);
            let front = a.saturating_sub(lo);
            self.lw_unmount(reg, list, 0, front, true);
            let r = &mut self.lists.regs[reg];
            r.lo += front;
            // window indices shift (the rows' targets do not)
            if r.stale != usize::MAX {
                r.stale = r.stale.saturating_sub(front);
            }
            if a < r.lo {
                let k = r.lo - a;
                r.rows.splice(0..0, (0..k).map(|_| LRow::hole()));
                r.lo = a;
                if r.stale != usize::MAX {
                    r.stale += k;
                }
            }
            let end = r.lo + r.rows.len();
            if end < b {
                r.rows.extend((end..b).map(|_| LRow::hole()));
            }
        }
        if self.lists.regs[reg].rows.iter().any(|r| !r.mounted) {
            self.lr_fill(reg);
        }
    }

    /// Mount every hole of the window (one `insert_run` per run of holes).
    fn lr_fill(&mut self, reg: usize) {
        let (list, mount, slot_at, set_wired, lo, cls) = {
            let r = &self.lists.regs[reg];
            (r.list, r.mount, r.slot_at, r.set_wired, r.lo, r.cls)
        };
        let mut pos = self.lr_base(reg);
        let mut k = 0;
        let len = self.lists.regs[reg].rows.len();
        let mut recs = std::mem::take(&mut self.lists.stage);
        while k < len {
            if self.lists.regs[reg].rows[k].mounted {
                pos += self.lists.regs[reg].rows[k].nrec as usize;
                k += 1;
                continue;
            }
            recs.clear();
            while k < len && !self.lists.regs[reg].rows[k].mounted {
                // SAFETY: `list` is the region's live source (see `LRegion::list`) and the window lies in it; a
                // slot in a list's order is live and initialised; `mount` was monomorphised for the source.
                let slot = unsafe { slot_at(list, lo + k) };
                self.lists.mounting.push((reg as u32, slot));
                let mut row = unsafe { mount(self, reg as u32, list, slot, &mut recs) };
                self.lists.mounting.pop();
                row.slot = slot;
                self.lists.regs[reg].rows[k] = row;
                k += 1;
            }
            self.bough.insert_run(pos, &recs);
            self.lr_count(reg, recs.len() as i64);
            pos += recs.len();
        }
        recs.clear();
        self.lists.stage = recs;
        // the item type's wired fields, for `List::free_slot`
        // SAFETY: as above; the class holds `offset_of!`-proven headed fields of `T`.
        unsafe { set_wired(list, &self.classes[cls as usize]) };
    }

    /// Take the laid-out heights of the window's rows into the index (per slot and per position); returns
    /// whether any changed.
    fn lr_measure(&mut self, reg: usize) -> bool {
        if !std::mem::take(&mut self.lists.regs[reg].remeasure) {
            return false;
        }
        let mut changed = false;
        let len = self.lists.regs[reg].rows.len();
        for k in 0..len {
            let (root, slot, mounted) = {
                let x = &self.lists.regs[reg].rows[k];
                (x.root, x.slot, x.mounted)
            };
            if !mounted {
                continue;
            }
            let h = if root == NONE {
                0
            } else {
                let hf = self.lay.root_height(root);
                // the height the region now accounts for: a later layout of the row re-measures only if it differs
                self.lists.root_h[root as usize] = hf;
                hf.round() as i32
            };
            let r = &mut self.lists.regs[reg];
            let v = h + r.gap;
            let p = r.lo + k;
            if r.idx.height(p) != v {
                r.idx.set(p, v);
                if let Some(h) = r.hs.get_mut(slot as usize) {
                    *h = v;
                }
                r.stale = r.stale.min(k + 1);
                r.stats.height_changes += 1;
                changed = true;
            }
        }
        let r = &mut self.lists.regs[reg];
        if r.est_auto && len > 0 {
            // the first measured rows are the estimate of every unmeasured one
            r.est_auto = false;
            let (lo, n, old) = (r.lo, r.idx.len(), r.est);
            let win: Vec<i32> = (lo..lo + len).map(|p| r.idx.height(p)).collect();
            let est = (win.iter().map(|&h| h as i64).sum::<i64>() / len as i64) as i32;
            if est != old {
                r.est = est;
                for h in r.hs.iter_mut() {
                    if *h == old {
                        *h = est;
                    }
                }
                r.idx.assign_from(0, n, |p| if p >= lo && p < lo + len { win[p - lo] } else { est });
                r.stale = 0;
                changed = true;
            }
        }
        changed
    }

    /// What is visible of region `reg`'s rows: their clip ∩ the window (absolute device px).
    fn lr_visible(&self, reg: usize) -> NRect {
        let win = NRect::new(0, 0, self.win_w as i32, self.win_h as i32);
        self.bough.clip_rect(self.lists.regs[reg].clip).intersect(win).canon()
    }

    /// Place region `reg`'s window: row `k` at `origin + (0, prefix(lo + k) + cur - s)`; a row whose space
    /// moves gets its space written, and the records of consecutive rows moving by the same amount one
    /// `scroll_run` (O(pages), damage cut to the rows' clip). With moves animating, a list change holds each
    /// placed row where it is shown (`cur`) and the motion restarts from there.
    fn lr_place(&mut self, reg: usize) {
        if self.lists.regs[reg].motion.is_some() {
            self.lr_catch_up(reg);
        }
        let base = self.lr_base(reg);
        let (origin, s, lo, virt, hold, da, gap) = {
            let r = &self.lists.regs[reg];
            (r.origin, r.s, r.lo, r.virt, r.animate.is_some() && r.moved, r.da, r.gap as i64)
        };
        let clip = if virt { self.lists.regs[reg].vpr } else { self.bough.clip_rect(self.lists.regs[reg].clip) };
        let ox = origin[0].round() as i32;
        let oy = origin[1].round() as i64;
        let n = self.lists.regs[reg].rows.len();
        // the rows before `stale` keep their targets: they shift with the content's top, all by one constant
        let k0 = self.lists.regs[reg].stale.min(n);
        let at = [ox as i64, oy - s];
        let mut pos = base;
        if k0 > 0 {
            let pa = self.lists.regs[reg].placed_at;
            let (dx, dy) = ((at[0] - pa[0]) as i32, (at[1] - pa[1]) as i32);
            let fresh = self.lists.regs[reg].rows[..k0].iter().any(|r| !r.placed);
            let mut y = if fresh { self.lists.regs[reg].idx.prefix(lo) } else { 0 };
            let mut nrec = 0usize;
            for k in 0..k0 {
                let r = &mut self.lists.regs[reg].rows[k];
                nrec += r.nrec as usize;
                let space = r.space;
                if r.placed {
                    self.shift_space(space, dx, dy);
                } else {
                    // mounted since the last placement (its ink is written after this, from its space)
                    r.placed = true;
                    r.y = y;
                    self.set_space(space, [ox as f32, (oy + y - s) as f32]);
                }
                if fresh {
                    y += self.lists.regs[reg].idx.height(lo + k) as i64;
                }
            }
            if dx == 0 && dy != 0 && self.lists.regs[reg].motion.is_none() {
                // a scroll: the window's rows move as one run by an integral dy, so the presenter copies
                // their pixels from the previous frame and repaints the exposed strip (`Rt::scroll_copy`)
                self.scroll_copy(base, nrec, dy, clip);
            } else if dx != 0 || dy != 0 {
                self.bough.scroll_run(base, nrec, dx, dy, clip);
            }
            pos += nrec;
        }
        {
            let r = &mut self.lists.regs[reg];
            r.stale = usize::MAX;
            r.placed_at = at;
        }
        if k0 == n {
            // nothing else moved: moves in flight keep their entries (a window shrunk at its end drops some)
            let r = &mut self.lists.regs[reg];
            if let Some(m) = r.motion.as_mut() {
                m.rows.retain(|e| (e.0 as usize) < n);
            }
            r.moved = false;
            r.da = 0;
            return;
        }
        let (cy0, cy1) = {
            let [_, y0, _, y1] = clip.to_xyxy();
            (y0 as i64, y1 as i64)
        };
        let (e_lo, e_hi) = self.lists.regs[reg].animate.map_or((0., 1.), |sp| (sp.e_lo, sp.e_hi));
        let mut y = self.lists.regs[reg].idx.prefix(lo + k0);
        let mut anim: Vec<(u32, u32, f32)> = Vec::new();
        if self.lists.regs[reg].animate.is_some() {
            // moves in flight among the shifted rows
            let mut p = base;
            for (k, row) in self.lists.regs[reg].rows[..k0].iter().enumerate() {
                if row.cur != 0 {
                    anim.push((k as u32, (p - base) as u32, row.d0));
                }
                p += row.nrec as usize;
            }
        }
        // pending run of placed rows moving by the same delta: (first record, count, dx, dy)
        let mut run: Option<(usize, usize, i32, i32)> = None;
        for k in k0..n {
            let pitch = self.lists.regs[reg].idx.height(lo + k) as i64;
            let (space, nrec, placed, ty, cur0, mounted) = {
                let r = &self.lists.regs[reg].rows[k];
                (r.space, r.nrec as usize, r.placed, r.y, r.cur, r.mounted)
            };
            if !mounted {
                y += pitch;
                continue;
            }
            let target = y;
            let mut cur = cur0 as i64;
            if hold && placed {
                // a list change: the row stays where it is shown (the user's scrolling excepted)
                cur = ty + cur - target + da;
                if cur != 0 {
                    // a path that never meets the clip snaps at once
                    let (a, b) = ((cur as f64 * e_lo).floor() as i64, (cur as f64 * e_hi).ceil() as i64);
                    let top = oy + target - s + a.min(b);
                    let bot = oy + target - s + a.max(b) + (pitch - gap);
                    if bot <= cy0 - 2 || top >= cy1 + 2 {
                        cur = 0;
                    }
                }
            }
            let new = [ox, (oy + target + cur - s) as i32];
            let old = self.glyphs.spaces[space as usize];
            let mut flush = true;
            if !placed || old != new {
                self.set_space(space, [new[0] as f32, new[1] as f32]);
                if placed {
                    let (dx, dy) = (new[0] - old[0], new[1] - old[1]);
                    match &mut run {
                        Some((a, c, rx, ry)) if *rx == dx && *ry == dy && *a + *c == pos => {
                            *c += nrec;
                            flush = false;
                        }
                        _ => {
                            if let Some((a, c, rx, ry)) = run.take() {
                                self.bough.scroll_run(a, c, rx, ry, clip);
                            }
                            run = Some((pos, nrec, dx, dy));
                            flush = false;
                        }
                    }
                }
            }
            if flush {
                if let Some((a, c, rx, ry)) = run.take() {
                    self.bough.scroll_run(a, c, rx, ry, clip);
                }
            }
            let r = &mut self.lists.regs[reg].rows[k];
            r.placed = true;
            r.y = target;
            r.cur = cur as i32;
            if cur != 0 {
                if hold {
                    r.d0 = cur as f32;
                }
                anim.push((k as u32, (pos - base) as u32, r.d0));
            }
            y += pitch;
            pos += nrec;
        }
        if let Some((a, c, rx, ry)) = run.take() {
            self.bough.scroll_run(a, c, rx, ry, clip);
        }
        let r = &mut self.lists.regs[reg];
        if let Some(sp) = r.animate {
            let m = r.motion.get_or_insert(Motion { spring: sp, t: 0., rows: Vec::new() });
            if hold {
                m.t = 0.;
            }
            m.rows = anim;
        }
        r.moved = false;
        r.da = 0;
    }

    /// Every row's records take the displacement their ink skipped while out of sight (before a placement:
    /// a scroll or a list change can bring them into view).
    fn lr_catch_up(&mut self, reg: usize) {
        let clip = if self.lists.regs[reg].virt { self.lists.regs[reg].vpr } else { self.bough.clip_rect(self.lists.regs[reg].clip) };
        let mut pos = self.lr_base(reg);
        for k in 0..self.lists.regs[reg].rows.len() {
            let row = &mut self.lists.regs[reg].rows[k];
            let (d, nrec) = (std::mem::take(&mut row.pend), row.nrec as usize);
            if d != 0 {
                self.bough.scroll_run(pos, nrec, 0, d, clip);
            }
            pos += nrec;
        }
    }

    /// Advance every region's moves by `dt` seconds: one spring evaluation per region; per moving row a
    /// space write and its records translated (no layout, no instance rewrite). Returns whether any moved.
    pub fn lists_tick(&mut self, dt: f32) -> bool {
        let mut any = false;
        for reg in 0..self.lists.regs.len() {
            let r = &mut self.lists.regs[reg];
            let Some(m) = r.motion.as_mut() else { continue };
            if r.dead || m.rows.is_empty() {
                continue;
            }
            any = true;
            m.t += dt as f64;
            let e = m.spring.remaining(m.t);
            let done = m.t >= m.spring.settle;
            let rows = std::mem::take(&mut m.rows);
            let (virt, top, lo, gap) = (r.virt, r.placed_at[1], r.lo, r.gap);
            let clip = if virt { r.vpr } else { self.bough.clip_rect(r.clip) };
            let [_, cy0, _, cy1] = clip.to_xyxy();
            let base = self.lr_base(reg);
            let mut run: Option<(usize, usize, i32)> = None;
            for &(k, rel, d0) in &rows {
                let h = self.lists.regs[reg].idx.height(lo + k as usize) - gap;
                let row = &mut self.lists.regs[reg].rows[k as usize];
                let new = (d0 as f64 * e).round() as i32;
                let d = new - row.cur;
                if d == 0 && !(done && row.pend != 0) {
                    continue;
                }
                row.cur = new;
                row.pend += d;
                let space = row.space;
                self.shift_space(space, 0, d);
                let row = &mut self.lists.regs[reg].rows[k as usize];
                // shown at `y` (its ink at `y - pend`): out of the clip both ways, only the space moves
                let y = (top + row.y) as i32 + new;
                // (2 px of margin: anti-aliased edges reach past the box)
                let seen = |y: i32| y < cy1 + 2 && y + h > cy0 - 2;
                if !(done || seen(y) || seen(y - row.pend)) {
                    continue;
                }
                let (d, nrec) = (std::mem::take(&mut row.pend), row.nrec as usize);
                let at = base + rel as usize;
                match &mut run {
                    Some((a, c, rd)) if *rd == d && *a + *c == at => *c += nrec,
                    _ => {
                        if let Some((a, c, rd)) = run.take() {
                            self.bough.scroll_run(a, c, 0, rd, clip);
                        }
                        run = Some((at, nrec, d));
                    }
                }
            }
            if let Some((a, c, rd)) = run.take() {
                self.bough.scroll_run(a, c, 0, rd, clip);
            }
            if let Some(m) = self.lists.regs[reg].motion.as_mut() {
                m.rows = if done { Vec::new() } else { rows };
            }
        }
        any
    }

    // ------------------------------------------------------------------ queries (points)
    /// The settled scroll offset of region `reg` (it may differ from the one asked: anchoring, clamping).
    /// The unmaterialized runs of every list region: typed placeholders (region, rows, extent). A run is
    /// sized by the extent index (measured heights, the estimate for rows never mounted); it is materialized
    /// when the visible rect meets it.
    pub fn stubs(&self) -> Vec<Stub> {
        let mut out = Vec::new();
        for (reg, r) in self.lists.regs.iter().enumerate() {
            if r.dead || r.origin[0].is_nan() {
                continue;
            }
            let n = r.idx.len();
            let hi = r.lo + r.rows.len();
            let top = r.origin[1].round() as i64 - r.s;
            let x0 = r.origin[0].round() as i32;
            let x1 = (r.origin[0] + r.width).round() as i32;
            for (a, b) in [(0, r.lo.min(n)), (hi.min(n), n)] {
                if a < b {
                    let (y0, y1) = (top + r.idx.prefix(a), top + r.idx.prefix(b) - r.gap as i64);
                    out.push(Stub { region: reg as u32, rows: a..b, rect: NRect::new(x0, y0.clamp(i32::MIN as i64, i32::MAX as i64) as i32, x1, y1.clamp(i32::MIN as i64, i32::MAX as i64) as i32) });
                }
            }
        }
        out
    }
    /// The placeholder under a point (device px), with the row the extent index puts there: hit testing's
    /// answer where nothing is materialized.
    pub fn stub_at(&self, x: i32, y: i32) -> Option<(Stub, usize)> {
        self.stubs().into_iter().find(|s| { let [x0, y0, x1, y1] = s.rect.to_xyxy(); x >= x0 && x < x1 && y >= y0 && y < y1 }).map(|s| {
            let r = &self.lists.regs[s.region as usize];
            let top = r.origin[1].round() as i64 - r.s;
            let row = r.idx.row_at(y as i64 - top).clamp(s.rows.start, s.rows.end - 1);
            (s, row)
        })
    }
    pub fn list_scroll_pt(&self, reg: usize) -> f32 {
        self.lists.regs[reg].s as f32 / self.scale
    }
    /// The content's height (measured rows, estimates for the others, padding included).
    pub fn list_content_height_pt(&self, reg: usize) -> f32 {
        let r = &self.lists.regs[reg];
        (r.content() as f32 + r.pad[1] + r.pad[3]) / self.scale
    }
    /// The scroll offset that shows position `pos` at the top.
    pub fn list_row_offset_pt(&self, reg: usize, pos: usize) -> f32 {
        let r = &self.lists.regs[reg];
        (r.idx.prefix(pos) as f32 + r.pad[1]) / self.scale
    }
    pub fn list_row_height_pt(&self, reg: usize, pos: usize) -> f32 {
        let r = &self.lists.regs[reg];
        (r.idx.height(pos) - r.gap) as f32 / self.scale
    }
    /// The positions whose rows are mounted.
    pub fn list_window(&self, reg: usize) -> std::ops::Range<usize> {
        let r = &self.lists.regs[reg];
        r.lo..r.lo + r.rows.len()
    }
    /// Give region `reg` these pitches (height + gap, device px) by position, as if measured (tests: a fresh
    /// mount made to show the heights another region measured over its history); placed at the next flush.
    pub fn list_set_heights(&mut self, reg: usize, h: &[i32]) {
        let r = &mut self.lists.regs[reg];
        assert_eq!(r.idx.len(), h.len(), "one height per position");
        if !r.hs.is_empty() {
            for (p, &v) in h.iter().enumerate() {
                // SAFETY: the region's live source (see `LRegion::list`), read only; `p` is a position of it.
                let s = unsafe { (r.slot_at)(r.list, p) } as usize;
                if r.hs.len() <= s {
                    r.hs.resize(s + 1, r.est);
                }
                r.hs[s] = v;
            }
        }
        r.idx.assign_from(0, h.len(), |p| h[p]);
        r.dirty = true;
        r.stale = 0;
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
    rt.pubs.owner(_owner as usize, w.cls);
    // SAFETY: the contract: `l` is the live `List<T>` field of the owner and `w.node` its region.
    unsafe { rt.lr_replay::<T>(w.node as usize, l) };
}

/// # Safety
/// Wire contract: `sig` is a `Signal<f32>` field of the live owner, `w.node` the region it scrolls.
unsafe fn patch_scroll(rt: &mut Rt, w: &Wire, sig: *const u8, _owner: *const u8) {
    // SAFETY: the contract.
    let v = **unsafe { &*(sig as *const Signal<f32>) };
    let px = (v * rt.scale).round() as i64;
    let r = &mut rt.lists.regs[w.node as usize];
    if px != r.req && !r.dead {
        r.req = px;
        r.dirty = true;
    }
}

// =============================================================================== element

/// Where a region's rows come from (sealed): a `List<T>` field (keyed items at stable addresses, a delta log
/// replayed by the region) or an index range (row `i` a function of `i` and the owner; no log, no per-item
/// storage: a 100,000-row index source costs the region its 4-byte-per-row prefix index alone).
pub trait RowSource<V>: Sized + sealed::Sealed {
    #[doc(hidden)]
    fn parts(&self) -> SrcParts;
    /// Wire the source to region `reg` (a list: its log's patch).
    #[doc(hidden)]
    fn wire(&self, m: &mut Mounter<'_, V>, reg: u32);
}
mod sealed {
    pub trait Sealed {}
}
#[doc(hidden)]
pub struct SrcParts {
    list: *const u8,
    n: usize,
    slots: usize,
    mount: LRowMountFn,
    slot_at: unsafe fn(*const u8, usize) -> u32,
    item: unsafe fn(*const u8, u32) -> *const u8,
    set_wired: unsafe fn(*const u8, &[u32]),
    row_fn: usize,
    row_size: usize,
}

/// A `List<T>` field and its row function.
pub struct ListRows<'a, T, V> {
    list: LWired<'a, T, V>,
    row: fn(&T, &mut Mounter<'_, T>),
}
impl<T, V> sealed::Sealed for ListRows<'_, T, V> {}
impl<T: 'static, V: 'static> RowSource<V> for ListRows<'_, T, V> {
    fn parts(&self) -> SrcParts {
        let l = self.list.list;
        SrcParts {
            list: l as *const List<T> as *const u8,
            n: l.len(),
            slots: l.order_slots().iter().copied().max().map_or(0, |x| x as usize + 1),
            mount: mount_lrow::<T>,
            slot_at: list_slot_at::<T>,
            item: list_item::<T>,
            set_wired: list_set_wired::<T>,
            row_fn: self.row as usize,
            row_size: size_of::<T>(),
        }
    }
    fn wire(&self, m: &mut Mounter<'_, V>, reg: u32) {
        // SAFETY: `self.list` is an `offset_of!`-proven `List<T>` field of `V`; `patch_list::<T>` reads one.
        unsafe { m.wire_cell(self.list.list.head(), self.list.off, patch_list::<T>, reg, [0; 16]) };
    }
}

/// `n` rows, row `i` mounted by `row(owner, i)`.
pub struct IndexRows<V> {
    n: usize,
    row: fn(&V, usize, &mut Mounter<'_, V>),
}
impl<V> sealed::Sealed for IndexRows<V> {}
impl<V: 'static> RowSource<V> for IndexRows<V> {
    fn parts(&self) -> SrcParts {
        SrcParts { list: std::ptr::null(), n: self.n, slots: 0, mount: mount_irow::<V>, slot_at: index_slot_at, item: index_item, set_wired: index_set_wired, row_fn: self.row as usize, row_size: 0 }
    }
    fn wire(&self, _: &mut Mounter<'_, V>, _: u32) {}
}

/// A list region (see the module docs): every item of its source is one row, laid out at the container's
/// content width; all of them, or (`viewport` / `scroll`) the ones its box shows.
pub struct LList<'a, S, V> {
    src: S,
    spec: Spec,
    bg: Option<Color>,
    radius: f32,
    virt: bool,
    scroll: Option<Wired<'a, f32, V>>,
    estimate: f32,
    overscan: usize,
    animate: Option<Spring>,
}

/// A `List<T>` of views: item `T` is mounted by its `View::view`.
pub fn list<'a, T: crate::app::View, V>(l: LWired<'a, T, V>) -> LList<'a, ListRows<'a, T, V>, V> {
    list_with(l, view_row::<T>)
}
/// A `List<T>` whose rows are mounted by `row`.
pub fn list_with<'a, T, V>(l: LWired<'a, T, V>, row: fn(&T, &mut Mounter<'_, T>)) -> LList<'a, ListRows<'a, T, V>, V> {
    LList::new(ListRows { list: l, row })
}
/// `n` rows, row `i` mounted by `row(owner, i)` (an index source: no list, no per-row storage).
pub fn index_list<'a, V>(n: usize, row: fn(&V, usize, &mut Mounter<'_, V>)) -> LList<'a, IndexRows<V>, V> {
    LList::new(IndexRows { n, row })
}

impl<S, V> Styled for LList<'_, S, V> {
    fn spec_mut(&mut self) -> &mut Spec {
        &mut self.spec
    }
}
impl<'a, S, V> LList<'a, S, V> {
    fn new(src: S) -> Self {
        LList { src, spec: Spec::new(false, true), bg: None, radius: 0.0, virt: false, scroll: None, estimate: 0.0, overscan: 2, animate: None }
    }
    pub fn bg(mut self, c: Color) -> Self {
        self.bg = Some(c);
        self
    }
    pub fn rounded(mut self, r: f32) -> Self {
        self.radius = r;
        self
    }
    /// Virtual: the container's box is a viewport (give it a height) that shows the rows meeting it, at
    /// scroll offset 0. A row not mounted counts its last measured height, or the estimate.
    pub fn viewport(mut self) -> Self {
        self.virt = true;
        if self.estimate == 0.0 {
            self.estimate = 24.0;
        }
        self
    }
    /// Virtual, scrolled by `offset` (points). The list may move the offset itself while it anchors
    /// (`Rt::list_scroll_pt`; mirror it back with `Signal::set_silent`).
    pub fn scroll(mut self, offset: Wired<'a, f32, V>) -> Self {
        self.scroll = Some(offset);
        self.viewport()
    }
    /// What an unmeasured row counts as (points).
    pub fn estimate(mut self, pt: f32) -> Self {
        self.estimate = pt;
        self
    }
    /// Rows mounted beyond each edge of the viewport (default 2).
    pub fn overscan(mut self, rows: usize) -> Self {
        self.overscan = rows;
        self
    }
    /// Rows a list change moves slide from where they were (zgx's `animate = true`: the spring
    /// `bounce 0.1, duration 0.4`); `App::tick` advances them.
    pub fn animate_moves(self) -> Self {
        self.animate_with(Spring::moves())
    }
    pub fn animate_with(mut self, s: Spring) -> Self {
        self.animate = Some(s);
        self
    }
}

impl<'a, S: RowSource<V>, V: 'static> Element<V> for LList<'a, S, V> {
    fn mount(self, m: &mut Mounter<'_, V>) {
        let s = m.rt.scale;
        let hd = m.rt.bough.new_handle();
        // the container is a sized leaf: the region applies its padding and gap, and its height is the
        // stack's (without a viewport)
        let mut spec = self.spec;
        let pad = spec.pad.map(|p| p * s);
        let gap = (spec.gap * s).round() as i32;
        spec.pad = [0.0; 4];
        spec.gap = 0.0;
        spec.container = false;
        let auto_h = spec.h == Dim::Auto && !self.virt;
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
        // a definite width is known now
        let width = self.spec.w.fixed_px(s).map_or(f32::NAN, |w| w - pad[0] - pad[2]);
        // a virtual list's rows are cut to its box (the clip's rect is set when the box is placed)
        let clip = if self.virt { m.rt.bough.new_clip(NRect::EMPTY) } else { m.clip };
        let cls = m.rt.new_class();
        let reg = m.rt.lists.regs.len();
        let src = self.src.parts();
        let est = (self.estimate * s).round() as i32 + gap;
        let req = self.scroll.map_or(0, |w| (*w.get() * s).round() as i64);
        m.rt.lists.regs.push(LRegion {
            cls,
            anchor: hd,
            lid,
            depth: m.depth + 1,
            clip,
            pclip: m.clip,
            vpr: NRect::EMPTY,
            spec,
            auto_h,
            gap,
            pad,
            origin: [f32::NAN; 2],
            width,
            total: 0,
            dirty: true,
            remeasure: false,
            moved: false,
            dead: false,
            idx: HeightIndex::new(src.n, est),
            hs: vec![est; src.slots],
            est,
            lo: 0,
            rows: Vec::new(),
            virt: self.virt,
            vh: if self.virt { f32::NAN } else { f32::INFINITY },
            s: req,
            req,
            da: 0,
            stale: 0,
            placed_at: [0; 2],
            overscan: self.overscan,
            parent: m.rt.lists.mounting.last().copied().unwrap_or((NONE, NONE)),
            cspace: m.space,
            vis: NRect::EMPTY,
            est_auto: self.estimate <= 0.0,
            motion: None,
            animate: self.animate,
            stats: LStats::default(),
            // an index source's rows belong to the owner itself
            list: if src.list.is_null() { m.owner as *const u8 } else { src.list },
            mount: src.mount,
            slot_at: src.slot_at,
            item: src.item,
            set_wired: src.set_wired,
            row_fn: src.row_fn,
            row_size: src.row_size,
        });
        m.rt.space_reg(m.space, reg as u32, true);
        self.src.wire(m, reg as u32);
        if let Some(sc) = self.scroll {
            // SAFETY: `sc` is a `Wired<f32, V>` (an `offset_of!`-proven `Signal<f32>` field of `V`);
            // `patch_scroll` reads a `Signal<f32>`.
            unsafe { m.wire_cell(sc.sig.head(), sc.off, patch_scroll, reg as u32, [0; 16]) };
        }
        m.scope.lregs.push(reg as u32);
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
