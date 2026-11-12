//! Direct wires (round 6 experiment, the lead's "direct wires"): a `sig!` wire whose whole effect is one store
//! into a column and a damage rect is applied **by the write itself**: `Signal::set` stores the new value into
//! the destination's instance column and logs the record for damage; no settle runs, no wire is walked, no
//! patch function is called.
//!
//! **Typed lowering.** Whether a binding can be direct is decided at compile time by the property's applier
//! (`Apply::DIRECT`): a box's background colour with no paint operand beside it (`LBox<_, _, ()>`, a raw quad)
//! is a plain colour-word store. The binding is lowered as an ordinary wire (so dispatch, release and the
//! `DerefMut` path stay what they are) and, when the signal's chain is exactly that wire, the head is tagged
//! [`DIRECT`] and the wire's entry here records the destination. The entry is armed at the next frame, once
//! the element has finished mounting: a node under variants (`flags.rs`) or with an opacity stays on the
//! ordinary path. A second binding of the signal (`Rt::add_wire`) clears the tag.
//!
//! **What the write does**: one bounds-checked load of the entry, a compare of the entry's head with the
//! signal's (a stale tag on a reused wire index falls back), one store into the instance, and, on the first
//! write since the last frame, a push of the entry for damage. The frame turns each logged entry into the
//! paint write the wire would have made (the record's visible ink, the paint log's (old, new) states for
//! content addressing) and brings the node's base colour up to date.
//!
//! **Batches** of writes to one cell between two frames cost one store each and one damage computation. A
//! **derived value** is a cell written by a recompute wire through `set`: its own bindings can be direct, its
//! sources' bindings to it cannot (a recompute is code, not a store).
use crate::rt::*;
use bough_metal::instance::Instance;
use std::cell::Cell;

/// The head tag: the signal's wire chain is one direct wire, whose index the head holds.
pub(crate) const DIRECT: u32 = 1 << 29;

#[derive(Clone, Copy)]
pub(crate) struct Entry {
    /// The head the entry serves (0: none): a write through any other head falls back.
    head: usize,
    node: u32,
    inst: u32,
    bordered: bool,
    armed: bool,
    dirty: bool,
    /// The colour the instance showed before the first write since the last frame.
    old: u32,
}
impl Entry {
    const NONE: Entry = Entry { head: 0, node: NONE, inst: NONE, bordered: false, armed: false, dirty: false, old: 0 };
}

/// The direct entries of one runtime, by wire index; boxed (its address is the thread's `TABLE`).
pub(crate) struct Table {
    entries: Vec<Entry>,
    dirty: Vec<u32>,
    unarmed: Vec<u32>,
    /// The region's instance column (it never moves).
    inst: *mut Instance,
}
impl Default for Table {
    fn default() -> Self {
        Table { entries: Vec::new(), dirty: Vec::new(), unarmed: Vec::new(), inst: std::ptr::null_mut() }
    }
}

impl Drop for Table {
    /// The thread's pointer never outlives the table it names.
    fn drop(&mut self) {
        let p = self as *mut Table;
        let _ = TABLE.try_with(|t| {
            if t.get() == p {
                t.set(std::ptr::null_mut());
            }
        });
    }
}

thread_local! {
    /// The table of the runtime last made or run on this thread (a write of another runtime's signal finds its
    /// head not in it and falls back).
    static TABLE: Cell<*mut Table> = const { Cell::new(std::ptr::null_mut()) };
}

impl Table {
    /// This runtime runs on this thread now. Only a runtime with direct entries needs the thread's pointer: a
    /// write consults it only through a head this runtime tagged (an untagged write never reads it), so a
    /// runtime without entries skips the thread-local store on every turn.
    #[inline(always)]
    pub(crate) fn enter(&mut self) {
        if !self.entries.is_empty() {
            self.enter_now();
        }
    }
    #[inline(never)]
    fn enter_now(&mut self) {
        let p = self as *mut Table;
        TABLE.with(|t| t.set(p));
    }
    /// Wire `w` (just linked into `head`) binds a colour store to node `node`: tag the head if the chain is
    /// that wire alone.
    pub(crate) fn bind(&mut self, wires: &[Wire], w: u32, head: &Cell<u32>, node: u32) {
        let h = head.get();
        if cfg!(feature = "mutant_direct_off") || h & HEAD_IDX != w || wires[w as usize].next != 0 {
            return;
        }
        let i = w as usize;
        if self.entries.len() <= i {
            self.entries.resize(i + 1, Entry::NONE);
        }
        self.entries[i] = Entry { head: head as *const _ as usize, node, ..Entry::NONE };
        self.unarmed.push(w);
        head.set(h | DIRECT);
        // a runtime's first entry: it may not have entered at the start of this turn (`enter`)
        self.enter_now();
    }
    /// Wire `w` is gone: its entry serves no head.
    #[inline]
    pub(crate) fn kill(&mut self, w: u32) {
        if let Some(e) = self.entries.get_mut(w as usize) {
            *e = Entry::NONE;
        }
    }
}

/// A write of `v` to the signal whose head (tagged [`DIRECT`]) is `head`: stored into its destination now.
/// Returns false when the write must take the ordinary path (no armed entry for this head).
///
/// # Safety
/// `T` is the value type of the signal whose head is `head`; a head is tagged only for a `Signal<Color>`
/// (`Apply::DIRECT` appliers take `Color`), so `T` is a 4-byte colour word.
#[inline(always)]
pub(crate) unsafe fn store<T>(h: u32, head: &Cell<u32>, v: &T) -> bool {
    if size_of::<T>() != 4 {
        return false;
    }
    let t = TABLE.with(|t| t.get());
    if t.is_null() {
        return false;
    }
    // SAFETY: the table outlives every head tagged for it (`Rt` owns it; a dropped runtime's heads are its
    // owner's, dropped with it); single-threaded access (the thread's own table).
    let t = unsafe { &mut *t };
    let Some(e) = t.entries.get_mut((h & HEAD_IDX) as usize) else { return false };
    if e.head != head as *const _ as usize || !e.armed {
        return false;
    }
    // SAFETY: the contract (a colour word); `e.inst` is the armed destination's instance in the region.
    let c = unsafe { *(v as *const T as *const u32) };
    let p = unsafe { &mut *t.inst.add(e.inst as usize) };
    let slot = if e.bordered { &mut p.color2 } else { &mut p.color };
    if !e.dirty {
        e.dirty = true;
        e.old = *slot;
        t.dirty.push(h & HEAD_IDX);
    }
    if !cfg!(feature = "mutant_direct_store") {
        *slot = c;
    }
    true
}

impl Rt {
    /// Arm the entries bound since the last call (their elements have finished mounting: the end of a mount, of
    /// a turn, a frame).
    #[inline(always)]
    pub(crate) fn direct_arm(&mut self) {
        if !self.input.direct.unarmed.is_empty() {
            self.direct_arm_new();
        }
    }
    #[cold]
    #[inline(never)]
    fn direct_arm_new(&mut self) {
        let t = &mut *self.input.direct;
        if t.inst.is_null() {
            t.inst = &mut self.inst[0] as *mut Instance;
        }
        for w in std::mem::take(&mut t.unarmed) {
            let e = t.entries[w as usize];
            if e.head == 0 {
                continue;
            }
            let hot = self.hot[e.node as usize];
            let kind = hot.kind;
            // plain colour stores only: no paint modifier (`HF_MOD`: under variants, an opacity, a paint
            // offset), a quad or a border box (whose instance is at its own id: the instance is the state)
            if hot.flags & HF_MOD != 0 || kind & NK_HOVERED != 0 || !(kind == NK_QUAD || kind == NK_BORDER) {
                continue;
            }
            let inst = self.ibase + e.node; // `inst_of`: the instance is at the element's id
            let e = &mut t.entries[w as usize];
            (e.inst, e.bordered, e.armed) = (inst, kind == NK_BORDER, true);
        }
    }

    /// Before a frame: arm what is new, and turn the direct writes since the last frame into the paint writes
    /// the wires would have made.
    pub(crate) fn direct_flush(&mut self) {
        self.direct_arm();
        if self.input.direct.dirty.is_empty() {
            return;
        }
        let dirty = std::mem::take(&mut self.input.direct.dirty);
        for &w in &dirty {
            let e = &mut self.input.direct.entries[w as usize];
            e.dirty = false;
            if !e.armed {
                continue;
            }
            let (node, inst, bordered, old) = (e.node, e.inst, e.bordered, e.old);
            let p = &self.inst[inst as usize];
            let now = if bordered { p.color2 } else { p.color };
            // the instance is the element's state (nothing mirrors its colour): only the damage is left
            let h = self.handle(node);
            if now != old && !cfg!(feature = "mutant_direct_damage") {
                self.paint_write(h, old as u64, now as u64);
            }
        }
        let mut dirty = dirty;
        dirty.clear();
        self.input.direct.dirty = dirty;
    }
}
