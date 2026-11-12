//! `Action<I, O>`: a command a handler runs (`self.save.run(x)`), whose every run completes (unless
//! cancelled) and lands, in the order of the runs, through the handler named by `.then(..)`.
//!
//! An action field needs no registration: when its owner mounts, the owner type's compile-time layout
//! (`type_info`, as `reflect.rs` reads it) gives the offset of every `Action` among its struct fields, nested
//! structs included, with a vtable to reach it. So the offset is proven by the compiler, not inferred, and the
//! landing's view type is checked against the struct holding the action, once, at mount. Each action gets a
//! wire on its head: `run` queues the head, and the settle that follows starts the run.
//!
//! What is in flight is state: `inflight()` lists the inputs of the runs not yet landed, in order. Optimistic UI
//! is reading it where the value is shown (`docs/lanes/async/programs/p03_optimistic.rs`); a failed run leaves
//! the list in the same turn its landing runs, so its guess disappears with no rollback code.
use crate::elem::Mounter;
use crate::rt::*;
use crate::signal::WireHead;
use bough_async::{AId, Engine, Landing, Out, Work};
use std::any::TypeId;
use std::async_iter::AsyncIterator;
use std::collections::VecDeque;
use std::future::Future;
use std::ptr::DynMetadata;

#[repr(C)]
pub struct Action<I, O> {
    head: WireHead,
    f: Box<dyn Fn(I) -> Work>,
    land: Option<(TypeId, Box<dyn Fn(*mut u8, &I, O)>)>,
    queued: Vec<I>,
    inflight: VecDeque<I>,
    cancel: bool,
    /// The operations that need `I: Clone` and `O: 'static`, made where those bounds are known: the runtime
    /// reaches an action through a vtable found at compile time, and that lookup does not see impls with
    /// lifetime bounds, so `ActionCell` is implemented for every `Action<I, O>` and calls these.
    start_fn: fn(&mut Action<I, O>, &mut Engine, AId),
    land_fn: unsafe fn(&mut Action<I, O>, *mut u8, Out),
    /// `shows`: how a run in flight looks on a cell of the struct holding the action.
    overlay: Option<Overlay<I>>,
}

/// An action's overlay on one cell: the cell's offset in the struct holding the action, its type (checked at
/// mount), and the typed operations, made where the types are known.
struct Overlay<I> {
    off: u32,
    owner: TypeId,
    /// A copy of the cell's value, every in-flight input's overlay applied to it in order.
    make: Box<dyn Fn(*const u8, &mut dyn Iterator<Item = &I>) -> Box<dyn std::any::Any>>,
    swap: unsafe fn(*mut u8, &mut Box<dyn std::any::Any>),
}

fn start_runs<I: Clone + 'static, O: 'static>(a: &mut Action<I, O>, e: &mut Engine, aid: AId) {
    if std::mem::take(&mut a.cancel) {
        e.cancel(aid);
    }
    for i in std::mem::take(&mut a.queued) {
        a.inflight.push_back(i.clone());
        e.run(aid, (a.f)(i));
    }
}

/// # Safety
/// `parent` is the live struct holding the action, of the landing's type.
unsafe fn land_one<I: 'static, O: 'static>(a: &mut Action<I, O>, parent: *mut u8, out: Out) {
    let o = *out.downcast::<O>().expect("an answer of the action's type");
    if let (Some((_, h)), Some(i)) = (a.land.as_ref(), a.inflight.front()) {
        // the run landing is the oldest in flight (runs land in order)
        h(parent, i, o);
    }
}

impl<I: Clone + 'static, O: 'static> Action<I, O> {
    /// An action whose run is the future `f(input)`.
    pub fn new<F, Fut>(f: F) -> Action<I, O>
    where
        F: Fn(I) -> Fut + 'static,
        Fut: Future<Output = O> + 'static,
    {
        Action { head: WireHead::new(), f: Box::new(move |i| Work::once(f(i))), land: None, queued: Vec::new(), inflight: VecDeque::new(), cancel: false, start_fn: start_runs::<I, O>, land_fn: land_one::<I, O>, overlay: None }
    }
    /// An action whose run is the stream `f(input)`: every item lands on its own, in order.
    pub fn stream<F, St>(f: F) -> Action<I, O>
    where
        F: Fn(I) -> St + 'static,
        St: AsyncIterator<Item = O> + 'static,
    {
        Action { head: WireHead::new(), f: Box::new(move |i| Work::many(f(i))), land: None, queued: Vec::new(), inflight: VecDeque::new(), cancel: false, start_fn: start_runs::<I, O>, land_fn: land_one::<I, O>, overlay: None }
    }
    /// Answers land through `h` on the struct that holds this action (checked at mount).
    pub fn then<V: 'static>(mut self, h: fn(&mut V, O)) -> Action<I, O> {
        // the pointer handed to the closure is the struct holding the action, of type `V` (checked at mount)
        self.land = Some((TypeId::of::<V>(), Box::new(move |p: *mut u8, _: &I, o: O| h(unsafe { &mut *(p as *mut V) }, o))));
        self
    }
    /// `then`, with the input of the run the answer belongs to.
    pub fn then_with<V: 'static>(mut self, h: fn(&mut V, &I, O)) -> Action<I, O> {
        // as `then`
        self.land = Some((TypeId::of::<V>(), Box::new(move |p: *mut u8, i: &I, o: O| h(unsafe { &mut *(p as *mut V) }, i, o))));
        self
    }

    /// How a run in flight looks on `cell` (a field of the struct holding the action): while runs are in flight,
    /// every description reads the cell with `f(&mut value, input)` applied for each, in the order sent. A run that
    /// lands or fails stops being applied in the turn its landing runs. Handlers, landings and wires read the cell
    /// as it is (see `applied` for a handler that must act on what is shown).
    pub fn shows<V: 'static, T: Clone + 'static>(mut self, cell: crate::answer::Field<V, crate::signal::Signal<T>>, f: fn(&mut T, &I)) -> Action<I, O> {
        let make = move |p: *const u8, ins: &mut dyn Iterator<Item = &I>| -> Box<dyn std::any::Any> {
            // SAFETY: `p` is the cell (a `Signal<T>`) at the proven offset (the caller, `ActionCell::overlay`).
            let mut v: T = unsafe { (**(p as *const crate::signal::Signal<T>)).clone() };
            for i in ins {
                f(&mut v, i);
            }
            Box::new(v)
        };
        self.overlay = Some(Overlay { off: cell.offset(), owner: TypeId::of::<V>(), make: Box::new(make), swap: crate::signal::keep::swap_of::<T> });
        self
    }
    /// The cell as the screen shows it: its value with this action's in-flight runs applied (a copy).
    pub fn applied<T: Clone + 'static>(&self, cell: &crate::signal::Signal<T>) -> T {
        let mut v = (**cell).clone();
        if let Some(o) = self.overlay.as_ref() {
            let b = (o.make)(cell as *const _ as *const u8, &mut self.inflight.iter());
            if let Ok(b) = b.downcast::<T>() {
                v = *b;
            }
        }
        v
    }

    /// Run with `input`: it starts when this turn settles.
    pub fn run(&mut self, input: I) {
        self.queued.push(input);
        crate::signal::queue(&self.head.0);
    }
    /// Drop every run in flight (a stream stops where it is); their inputs leave `inflight` this turn.
    pub fn cancel(&mut self) {
        self.queued.clear();
        self.cancel = true;
        crate::signal::queue(&self.head.0);
    }
    /// The inputs of the runs not yet landed (queued ones included), oldest first.
    pub fn inflight(&self) -> impl DoubleEndedIterator<Item = &I> + '_ {
        self.inflight.iter().chain(self.queued.iter())
    }
    /// The newest run's input, if any is in flight.
    pub fn last(&self) -> Option<&I> {
        self.queued.last().or(self.inflight.back())
    }
    pub fn running(&self) -> bool {
        !self.inflight.is_empty() || !self.queued.is_empty()
    }
}

/// What the runtime needs of any `Action<I, O>`, reached through the vtable the owner's layout gives.
pub trait ActionCell {
    fn landing_type(&self) -> Option<TypeId>;
    /// Start the queued runs (and a cancel), at the settle after `run`.
    fn start(&mut self, e: &mut Engine, aid: AId);
    /// # Safety
    /// `parent` is the live struct holding this action, of the landing's type.
    unsafe fn land_out(&mut self, parent: *mut u8, out: Out);
    fn finished(&mut self);
    /// The overlay's cell offset and owner type, if any (checked at mount).
    fn overlay_of(&self) -> Option<(u32, TypeId)>;
    /// Swap the overlaid value in (`kept` empty: the truth goes into `kept`) or back (`kept` full).
    ///
    /// # Safety
    /// `parent` is the live struct holding the action.
    unsafe fn overlay_swap(&mut self, parent: *mut u8, kept: &mut Option<Box<dyn std::any::Any>>);
}

impl<I, O> ActionCell for Action<I, O> {
    fn landing_type(&self) -> Option<TypeId> {
        self.land.as_ref().map(|l| l.0)
    }
    fn start(&mut self, e: &mut Engine, aid: AId) {
        (self.start_fn)(self, e, aid)
    }
    unsafe fn land_out(&mut self, parent: *mut u8, out: Out) {
        // SAFETY: the caller's contract.
        unsafe { (self.land_fn)(self, parent, out) }
    }
    fn finished(&mut self) {
        self.inflight.pop_front();
    }
    fn overlay_of(&self) -> Option<(u32, TypeId)> {
        self.overlay.as_ref().map(|o| (o.off, o.owner))
    }
    unsafe fn overlay_swap(&mut self, parent: *mut u8, kept: &mut Option<Box<dyn std::any::Any>>) {
        let Some(o) = self.overlay.as_ref() else { return };
        // SAFETY: the caller's contract; `o.off` is the cell's proven offset in the parent's type (checked at mount).
        let cell = unsafe { parent.add(o.off as usize) };
        match kept.take() {
            None => {
                if self.inflight.is_empty() && self.queued.is_empty() {
                    return;
                }
                let mut v = (o.make)(cell, &mut self.inflight.iter().chain(self.queued.iter()));
                // SAFETY: the cell is a `Signal<T>` and `v` a `T` (both from `shows`).
                unsafe { (o.swap)(cell, &mut v) };
                *kept = Some(v);
            }
            Some(mut truth) => {
                // SAFETY: as above.
                unsafe { (o.swap)(cell, &mut truth) };
            }
        }
    }
}

/// An action as the runtime keeps it: where it is, its engine id.
pub(crate) struct ActSlot {
    pub cell: *mut dyn ActionCell,
    pub aid: AId,
    /// The struct holding the action (its overlay's cell is a field of it).
    pub parent: *mut u8,
}

struct ALand {
    cell: *mut dyn ActionCell,
    parent: *mut u8,
    owner: usize,
    cls: u32,
}

impl Landing for ALand {
    unsafe fn land(&self, out: Out) {
        touched(self.owner, self.cls);
        // SAFETY: the engine lands only while the action is mounted (`remove_action` at scope release); the
        // action and its parent live in the owner, which outlives its scope.
        unsafe { (*self.cell).land_out(self.parent, out) };
        // the action's head: `inflight` changed
        // SAFETY: the action's head is its first field (`#[repr(C)]`).
        crate::signal::queue_quiet(unsafe { &*(self.cell as *mut u8 as *const std::cell::Cell<u32>) });
    }
    unsafe fn finished(&self) {
        touched(self.owner, self.cls);
        // SAFETY: as `land`.
        unsafe { (*self.cell).finished() };
    }
}

fn touched(owner: usize, cls: u32) {
    crate::ask::touch_owner(owner, cls);
}

/// # Safety
/// The wire contract: `node` indexes `Rt::asks.acts`.
unsafe fn patch_action(rt: &mut Rt, w: &Wire, _sig: *const u8, _owner: *const u8) {
    let slot = &rt.asks.acts[w.node as usize];
    let (cell, aid) = (slot.cell, slot.aid);
    // SAFETY: the action is mounted (its wire is live) at its proven offset in the live owner.
    unsafe { (*cell).start(&mut rt.asks.engine, aid) };
    rt.apply_async();
}

// =============================================================================== the layout scan

#[derive(Clone, Copy)]
pub(crate) struct ActField {
    pub off: u32,
    pub parent_off: u32,
    pub parent: TypeId,
    pub vt: Option<DynMetadata<dyn ActionCell>>,
}

pub(crate) const MAX_ACTIONS: usize = 32;

#[derive(Clone, Copy)]
pub(crate) struct ActScan {
    pub n: usize,
    pub f: [ActField; MAX_ACTIONS],
}

/// The `Action` fields of `T` (struct fields, nested structs included; not inside enums, arrays, boxes or
/// cells), built at compile time.
pub(crate) fn actions_of<T>() -> &'static ActScan {
    const { &scan(std::mem::type_info::of::<T>()) }
}

#[rustc_comptime]
fn scan(t: TypeId) -> ActScan {
    let blank = ActField { off: 0, parent_off: 0, parent: t, vt: None };
    let mut s = ActScan { n: 0, f: [blank; MAX_ACTIONS] };
    walk(&mut s, t, 0, 0);
    s
}

#[rustc_comptime]
fn walk(s: &mut ActScan, t: TypeId, base: usize, depth: u8) {
    if depth > 6 || t.trait_info_of::<dyn crate::reflect::IsTuple>().is_some() || t.trait_info_of::<dyn crate::reflect::Opaque>().is_some() {
        return;
    }
    if !matches!(t.info().kind, std::mem::type_info::TypeKind::Struct) {
        return;
    }
    let n = t.fields(0);
    let mut k = 0;
    while k < n {
        let f = t.field(0, k);
        let ft = f.type_id();
        let off = base + f.offset();
        match ft.trait_info_of::<dyn ActionCell>() {
            Some(v) => {
                if s.n < MAX_ACTIONS {
                    s.f[s.n] = ActField { off: off as u32, parent_off: base as u32, parent: t, vt: Some(v.get_vtable()) };
                    s.n += 1;
                }
            }
            None => {
                // a reactive cell (`Signal`, `List`, ...) holds no action of its owner
                let cell = matches!(ft.info().kind, std::mem::type_info::TypeKind::Struct) && ft.fields(0) >= 1 && ft.field(0, 0).type_id() == std::mem::type_info::of::<WireHead>();
                if !cell {
                    walk(s, ft, off, depth + 1);
                }
            }
        }
        k += 1;
    }
}

/// How many `Action` fields the compile-time scan finds in `T` (tests).
#[doc(hidden)]
pub fn action_count<T>() -> usize {
    actions_of::<T>().n
}

/// Wire every `Action` field of the owner being mounted (`App::new` for the root, a list row's mount for a row).
pub(crate) fn mount_actions<V: 'static>(m: &mut Mounter<'_, V>) {
    let s = actions_of::<V>();
    for f in &s.f[..s.n] {
        let vt = f.vt.expect("an action's vtable");
        let owner = m.owner as *mut u8;
        // SAFETY: `f.off` is a struct field offset of `V` from the compiler's layout, of a type implementing
        // `ActionCell` with this vtable; the owner is live and does not move while mounted.
        let at = unsafe { owner.add(f.off as usize) };
        let cell: *mut dyn ActionCell = std::ptr::from_raw_parts_mut(at as *mut (), vt);
        // SAFETY: as above.
        if let Some(t) = unsafe { (*cell).landing_type() } {
            assert!(t == f.parent, "an Action's `then` names a handler of another type than the struct holding the action");
        }
        if let Some((_, t)) = unsafe { (*cell).overlay_of() } {
            assert!(t == f.parent, "an Action's `shows` names a cell of another type than the struct holding the action");
        }
        let parent = unsafe { owner.add(f.parent_off as usize) };
        m.rt.async_on = true;
        let aid = m.rt.asks.engine.add_action(Box::new(ALand { cell, parent, owner: owner as usize, cls: m.cls }));
        let node = m.rt.asks.acts.len() as u32;
        m.rt.asks.acts.push(ActSlot { cell, aid, parent });
        // SAFETY: the action's head is its first field (`#[repr(C)]`), at a compiler-given offset of `V`;
        // `patch_action` reaches the action through the slot, not through `sig`.
        let head = unsafe { &*(at as *const std::cell::Cell<u32>) };
        unsafe { m.wire_cell(head, f.off, patch_action, node, [0; 16]) };
        m.scope.acts.push(node);
    }
}

impl Rt {
    /// An action's scope was released: its runs are dropped and nothing of it lands again.
    pub(crate) fn revoke_action(&mut self, node: u32) {
        let aid = self.asks.acts[node as usize].aid;
        self.asks.engine.remove_action(aid);
    }
}

