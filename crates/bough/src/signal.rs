//! `Signal<T>`: a value plus a 4-byte wire head, and `Wired`: a signal operand whose location inside its
//! owner is proven by the compiler (`sig!`).
//!
//! `head` = index of the first wire of this signal in the runtime's wire arena (0 = unbound) with
//! bit 31 = QUEUED. `#[repr(C)]` with the head first so the runtime can reach the head of any
//! `Signal<_>` at `owner + offset` without knowing `T`.
//!
//! # Soundness of wires (API.md)
//! A wire reads `owner + offset` as a `Signal<T>` long after the description that created it is gone.
//! That is sound only if the offset names a *struct field path* of the owner type whose type is
//! `Signal<T>`: a struct field never changes location or type for the owner's life. `sig!(self.a.b)`
//! expands to `offset_of!(Self, a.b)` (which accepts struct field paths only: an enum variant's field is
//! rejected at compile time) plus a never-called accessor `|o: &Self| &o.a.b` that pins the field's type
//! to the operand's `Signal<T>`. The owner type travels in `Wired<'_, T, V>`, and only a
//! `Mounter<'_, V>` of the same `V` accepts it. The old runtime inference (`addr(field) - addr(owner)`
//! accepted when inside `size_of::<V>()`) survives only as a debug assertion.
use std::cell::Cell;
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};

pub const QUEUED: u32 = 1 << 31;
/// The head's cell is held: a question reading it is out (`ask.rs`, or a `reader.rs` cohort). Only its status
/// wires (`latest`, `pending`) and questions run until the publication releases it, which re-queues it; a
/// description reads its shown value (`keep`).
pub const HELD: u32 = 1 << 30;
/// The cell is a source of a holding question: its first write of a turn keeps a pre-image (`keep`).
pub const KEEP: u32 = 1 << 29;
/// A question about this cell is in the air (`pending(..)` is true).
pub const LOADING: u32 = 1 << 28;
const _: () = assert!(IDX == crate::rt::HEAD_IDX);

/// A reactive cell whose first field is its wire head: `Signal`, `List`, `Reader`, `Entities`. `sig!`
/// proves any of them at an `offset_of!` offset and hands back the cell's operand type `At`.
///
/// # Safety
/// The implementor is `#[repr(C)]` with its `Cell<u32>` head at offset 0 (the runtime reaches the head of
/// any wired cell at `owner + offset`).
pub unsafe trait Headed {
    /// The proven operand (`Wired` for a `Signal`).
    type At<'a, V>
    where
        Self: 'a;
    /// # Safety
    /// `off` is the `offset_of!` offset of this cell in an owner of type `V` (only `prove` calls it).
    #[doc(hidden)]
    unsafe fn at<'a, V>(&'a self, off: u32) -> Self::At<'a, V>;
}

/// `sig!`'s expansion for every headed cell type.
///
/// # Safety
/// `off` must be `offset_of!(V, path)` for the same struct field path as `_path` (the macro computes both
/// from one token sequence; nothing else may call this).
#[doc(hidden)]
#[inline(always)]
pub unsafe fn prove<'a, C: Headed, V>(c: &'a C, off: usize, _path: fn(&V) -> &C) -> C::At<'a, V> {
    assert!(off + size_of::<C>() <= size_of::<V>() && off <= u32::MAX as usize);
    // SAFETY: the caller's contract.
    unsafe { c.at(off as u32) }
}

// SAFETY: `#[repr(C)]`, the head first.
unsafe impl<T> Headed for Signal<T> {
    type At<'a, V>
        = Wired<'a, T, V>
    where
        T: 'a;
    #[inline(always)]
    unsafe fn at<'a, V>(&'a self, off: u32) -> Wired<'a, T, V> {
        Wired { sig: self, off, _v: PhantomData }
    }
}

/// An array of signals as a field: `sig!(self.counts)` proves the array, `.at(i)` one element (the array's
/// offset plus `i` elements: arrays are contiguous, and `i < N` is checked).
// SAFETY: element 0's head is at offset 0 (`Signal` is `repr(C)`, head first; an array has no padding before
// element 0). The runtime never wires the array itself, only an element through `at`.
unsafe impl<T, const N: usize> Headed for [Signal<T>; N] {
    type At<'a, V>
        = WiredArray<'a, T, V, N>
    where
        T: 'a;
    #[inline(always)]
    unsafe fn at<'a, V>(&'a self, off: u32) -> WiredArray<'a, T, V, N> {
        WiredArray { arr: self, off, _v: PhantomData }
    }
}
/// `sig!(self.array)` for a `[Signal<T>; N]` field.
pub struct WiredArray<'a, T, V, const N: usize> {
    arr: &'a [Signal<T>; N],
    off: u32,
    _v: PhantomData<fn(&V)>,
}
impl<'a, T, V, const N: usize> WiredArray<'a, T, V, N> {
    /// Element `i` as a signal operand.
    #[inline(always)]
    pub fn at(&self, i: usize) -> Wired<'a, T, V> {
        assert!(i < N, "signal array index {i} out of bounds ({N})");
        Wired { sig: &self.arr[i], off: self.off + (i * size_of::<Signal<T>>()) as u32, _v: PhantomData }
    }
}

/// A cell's wire head (index + flags): the first field of every reactive cell (`Signal`, `List`, `Reader`,
/// `Entities`). Its own type so reflection recognises a reactive cell by its first field (`reflect.rs`).
#[repr(transparent)]
#[doc(hidden)]
pub struct WireHead(pub(crate) Cell<u32>);
impl WireHead {
    pub(crate) const fn new() -> Self {
        WireHead(Cell::new(0))
    }
}
impl Deref for WireHead {
    type Target = Cell<u32>;
    #[inline(always)]
    fn deref(&self) -> &Cell<u32> {
        &self.0
    }
}

#[repr(C)]
pub struct Signal<T> {
    head: WireHead,
    value: T,
}

impl<T> Signal<T> {
    pub const fn new(value: T) -> Self {
        Signal { head: WireHead::new(), value }
    }
    /// The wire head. Crate-private: a head names a wire list monomorphised for this signal's `T`, so
    /// copying one head into another signal's would make the runtime run wires of the wrong type.
    #[inline(always)]
    pub(crate) fn head(&self) -> &Cell<u32> {
        &self.head.0
    }
    pub fn into_inner(self) -> T {
        self.value
    }
    /// Overwrite the value without queueing its wires: for a value the runtime itself just wrote
    /// (a measured list's anchored scroll offset), mirrored back into the owner.
    #[inline]
    pub fn set_silent(&mut self, v: T) {
        self.value = v;
    }
}

impl<T: 'static> Signal<T> {
    /// Is a question about this cell in the air, or is the cell held (what shows it waits for an answer)?
    #[inline]
    pub fn pending(&self) -> bool {
        self.head.get() & (HELD | LOADING) != 0
    }
    /// What was last written, even while the cell is held: inside a description a held cell reads the value
    /// on screen (`*cell`), and this reads intent. Everywhere else the two are the same.
    #[inline]
    pub fn latest(&self) -> &T {
        if self.head.get() & HELD != 0 {
            if let Some(p) = keep::intent(self as *const Self as usize) {
                // SAFETY: the kept value is a `T` (registered for this cell) and stays in the table until the
                // description that swapped it ends, which outlives this borrow of the cell.
                return unsafe { &*(p as *const T) };
            }
        }
        &self.value
    }
    /// Ask again every question about this cell (its `into` destination, or the cell a `then` question is
    /// `about`), with the same input and holding nothing: stale-while-revalidate, a retry. Takes effect when the
    /// handler's turn settles.
    pub fn refresh(&mut self) {
        let a = self as *const Self as usize;
        LOG.with(|l| {
            let n = l.refresh_n.get() as usize;
            if n < REFRESH_CAP {
                l.refresh_at[n].set(a);
            } else {
                REFRESH_SPILL.with(|s| s.borrow_mut().push(a));
            }
            l.refresh_n.set(n as u32 + 1);
            l.asked.set(true);
        });
    }
}

/// Refreshes held in the write log's thread-local (no destructor there: an access to it stays a plain load).
pub(crate) const REFRESH_CAP: usize = 8;

thread_local! {
    /// Refreshes past `REFRESH_CAP` in one turn.
    pub(crate) static REFRESH_SPILL: std::cell::RefCell<Vec<usize>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Take the refreshes asked since the last call (inside a `LOG.with` the settle already makes).
pub(crate) fn take_refreshes(l: &WriteLog) -> Vec<usize> {
    let n = l.refresh_n.replace(0) as usize;
    let mut v: Vec<usize> = l.refresh_at[..n.min(REFRESH_CAP)].iter().map(|c| c.get()).collect();
    if n > REFRESH_CAP {
        REFRESH_SPILL.with(|s| v.append(&mut s.borrow_mut()));
    }
    v
}

impl<T: PartialEq> Signal<T> {
    /// Equality cut-off, then mark. Returns whether the value changed.
    #[inline]
    pub fn set(&mut self, v: T) -> bool {
        if self.value == v {
            return false;
        }
        *self.deref_mut() = v;
        true
    }
}

impl<T> Deref for Signal<T> {
    type Target = T;
    #[inline(always)]
    fn deref(&self) -> &T {
        &self.value
    }
}

impl<T> DerefMut for Signal<T> {
    /// Queues the signal's wires if it has any (see [`queue`]).
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut T {
        queue(&self.head.0);
        &mut self.value
    }
}

/// Entries of a thread's write log.
pub(crate) const LOG_CAP: usize = 32;

/// The write log of a thread: the addresses of the heads queued since they were last taken, so that settling
/// costs what was written instead of a scan of every wired cell of the owner. QUEUED on the head stays the
/// truth; the log only says where to look. When it is full its `epoch` advances instead, and every runtime
/// that has not scanned since then scans its whole owner once (`Rt::settle`).
pub(crate) struct WriteLog {
    pub n: Cell<u32>,
    pub epoch: Cell<u32>,
    /// A refresh was asked since the runtime last looked (`Signal::refresh`), and the cells: read by the settle's
    /// own access to this thread-local, so a turn without one pays a load.
    pub asked: Cell<bool>,
    pub refresh_n: Cell<u32>,
    pub refresh_at: [Cell<usize>; REFRESH_CAP],
    /// A head with `KEEP` or `HELD` was logged since the runtime last looked: a pre-image may need dropping at the
    /// end of the turn.
    pub kept: Cell<bool>,
    /// Runtimes alive on this thread: with one, an address that is not its root owner's can be dropped.
    pub live: Cell<u32>,
    pub at: [Cell<usize>; LOG_CAP],
}

thread_local! {
    pub(crate) static LOG: WriteLog = const {
        WriteLog { n: Cell::new(0), epoch: Cell::new(0), asked: Cell::new(false), refresh_n: Cell::new(0), refresh_at: [const { Cell::new(0) }; REFRESH_CAP], kept: Cell::new(false), live: Cell::new(0), at: [const { Cell::new(0) }; LOG_CAP] }
    };
}

#[cold]
#[inline(never)]
fn log(addr: usize, h: u32) {
    if h & (KEEP | HELD) != 0 {
        keep::capture(addr);
        LOG.with(|l| l.kept.set(true));
    }
    LOG.with(|l| {
        let n = l.n.get() as usize;
        if n == LOG_CAP {
            l.epoch.set(l.epoch.get().wrapping_add(1));
            l.n.set(0);
        } else {
            l.at[n].set(addr);
            l.n.set(n as u32 + 1);
        }
    })
}

/// The index bits of a head (`rt::HEAD_IDX`): zero for a cell with no wires, whatever its flags.
const IDX: u32 = (1 << 28) - 1;

/// Queue a head's wires: nothing for a head without wires or one already queued (two instructions and a
/// predicted branch on the path every later write of the turn takes). "Without wires" is the index bits, not
/// the whole word: a cell with no wires yet may carry `KEEP` or `LOADING`.
#[inline(always)]
pub(crate) fn queue(head: &Cell<u32>) {
    let h = head.get();
    if h & IDX != 0 && h & QUEUED == 0 {
        head.set(h | QUEUED);
        log(head as *const _ as usize, h);
    }
}

/// Queue a head for the runtime's own reasons (a hold or a release, a status change): no pre-image is kept.
pub(crate) fn queue_quiet(head: &Cell<u32>) {
    let h = head.get();
    if h & IDX != 0 && h & QUEUED == 0 {
        head.set(h | QUEUED);
        log(head as *const _ as usize, 0);
    }
}

/// Pre-images and kept values of held cells (DESIGN.md of lane `async`, section 4).
///
/// A cell whose head has `KEEP` (a source of a holding question) or `HELD` keeps a clone of its value at its
/// first write of a turn, before the write changes it (the write log's cold path is that event). When a
/// question holds the cell, the pre-image becomes the hold's kept value: what the screen shows. A description
/// runs with every held cell's kept value swapped in (`swap_held`), and the release drops it. Pre-images of
/// cells that end the turn unheld are dropped (`end_turn`). One thread-local table, touched only by cells with
/// one of the two bits.
pub(crate) mod keep {
    use std::any::Any;
    use std::cell::{Cell, RefCell};
    use bough_async::addr::AddrMap as HashMap;

    /// Clone a cell's value; swap a cell's value with a kept one. Monomorphised per cell type.
    #[derive(Clone, Copy)]
    pub(crate) struct Fns {
        pub clone: unsafe fn(*const u8) -> Box<dyn Any>,
        pub swap: unsafe fn(*mut u8, &mut Box<dyn Any>),
        pub value: unsafe fn(&Box<dyn Any>) -> *const u8,
    }

    #[derive(Default)]
    pub(crate) struct Table {
        /// Cell -> its functions, how many questions asked for them, and the runtime they belong to.
        fns: HashMap<usize, (Fns, u32, u64)>,
        /// Cell -> its kept value (a pre-image of this turn, or a held cell's shown value).
        kept: HashMap<usize, Box<dyn Any>>,
        /// Cells whose kept value is only this turn's pre-image.
        turn: Vec<usize>,
    }

    thread_local! {
        static T: RefCell<Table> = RefCell::new(Table::default());
        /// A description is running with the kept values swapped in.
        static SWAPPED: Cell<bool> = const { Cell::new(false) };
    }

    /// # Safety
    /// `p` is a `Signal<T>`.
    pub(crate) unsafe fn clone_of<T: Clone + 'static>(p: *const u8) -> Box<dyn Any> {
        Box::new(unsafe { (*(p as *const crate::signal::Signal<T>)).value.clone() })
    }
    /// # Safety
    /// `p` is a `Signal<T>` and `k` holds a `T`.
    pub(crate) unsafe fn swap_of<T: 'static>(p: *mut u8, k: &mut Box<dyn Any>) {
        let s = unsafe { &mut *(p as *mut crate::signal::Signal<T>) };
        std::mem::swap(&mut s.value, k.downcast_mut::<T>().expect("kept value of the cell's type"));
    }
    /// # Safety
    /// `k` holds a `T`.
    pub(crate) unsafe fn value_of<T: 'static>(k: &Box<dyn Any>) -> *const u8 {
        k.downcast_ref::<T>().expect("kept value of the cell's type") as *const T as *const u8
    }
    pub(crate) fn fns<T: Clone + 'static>() -> Fns {
        Fns { clone: clone_of::<T>, swap: swap_of::<T>, value: value_of::<T> }
    }

    /// Register `cell` as a source of a holding question (its head gets `KEEP` from the caller).
    pub(crate) fn register(cell: usize, f: Fns, rt: u64) {
        T.with(|t| {
            let mut t = t.borrow_mut();
            let e = t.fns.entry(cell).or_insert((f, 0, rt));
            if e.2 != rt {
                // a cell of a runtime that is gone, at an address reused by this one
                *e = (f, 0, rt);
            }
            e.1 += 1;
        });
    }

    /// Runtime `rt` is dropped: its cells leave the table.
    pub(crate) fn forget_runtime(rt: u64) {
        let _ = T.try_with(|t| {
            let mut t = t.borrow_mut();
            let t = &mut *t;
            let gone: Vec<usize> = t.fns.iter().filter(|(_, e)| e.2 == rt).map(|(&c, _)| c).collect();
            for c in gone {
                t.fns.remove(&c);
                t.kept.remove(&c);
                t.turn.retain(|&x| x != c);
            }
        });
    }
    /// One question fewer; returns whether none is left (the caller clears `KEEP`).
    pub(crate) fn unregister(cell: usize) -> bool {
        T.with(|t| {
            let mut t = t.borrow_mut();
            let gone = match t.fns.get_mut(&cell) {
                Some(e) => {
                    e.1 -= 1;
                    e.1 == 0
                }
                None => true,
            };
            if gone {
                t.fns.remove(&cell);
            }
            gone
        })
    }

    /// The write log's cold path, for a head with `KEEP` or `HELD`: keep the value before this first write.
    pub(crate) fn capture(cell: usize) {
        T.with(|t| {
            let mut t = t.borrow_mut();
            if t.kept.contains_key(&cell) {
                return;
            }
            let Some(&(f, _, _)) = t.fns.get(&cell) else { return };
            // SAFETY: registered for this cell's type by the question that set its `KEEP` bit.
            let v = unsafe { (f.clone)(cell as *const u8) };
            t.kept.insert(cell, v);
            // SAFETY: a registered cell's head is live (its question unregisters before its scope goes).
            let held = unsafe { &*(cell as *const Cell<u32>) }.get() & super::HELD != 0;
            if !held {
                t.turn.push(cell);
            }
        })
    }

    /// `cell` is now held: its pre-image of this turn (if it was written) is the hold's kept value.
    pub(crate) fn held(cell: usize) {
        T.with(|t| t.borrow_mut().turn.retain(|&c| c != cell));
    }
    /// `cell` is released: the screen shows its intent again. Call while its head still has `HELD`: during a
    /// description (kept values swapped in) the intent is swapped back into the cell before the kept value goes.
    pub(crate) fn release(cell: usize) {
        T.with(|t| {
            let mut t = t.borrow_mut();
            let t = &mut *t;
            if let Some(mut k) = t.kept.remove(&cell) {
                // SAFETY: as in `capture`.
                let held = unsafe { &*(cell as *const Cell<u32>) }.get() & super::HELD != 0;
                if held && SWAPPED.with(|s| s.get()) {
                    if let Some(&(f, _, _)) = t.fns.get(&cell) {
                        // SAFETY: registered for this cell's type.
                        unsafe { (f.swap)(cell as *mut u8, &mut k) };
                    }
                }
            }
            t.turn.retain(|&c| c != cell);
        })
    }
    /// The turn is over: pre-images of cells that are not held go.
    pub(crate) fn end_turn() {
        T.with(|t| {
            let mut t = t.borrow_mut();
            let mut turn = std::mem::take(&mut t.turn);
            for &c in &turn {
                // SAFETY: as in `capture`.
                if unsafe { &*(c as *const Cell<u32>) }.get() & super::HELD == 0 {
                    t.kept.remove(&c);
                }
            }
            // keep the list's capacity for the next turn
            turn.clear();
            t.turn = turn;
        })
    }
    /// Is anything kept (a hold, or a pre-image of this turn)?
    pub(crate) fn any() -> bool {
        T.with(|t| !t.borrow().kept.is_empty())
    }

    /// Swap every held cell of runtime `rt` with its kept value (call twice: before and after a description).
    pub(crate) fn swap_held(on: bool, rt: u64) {
        T.with(|t| {
            let mut t = t.borrow_mut();
            let t = &mut *t;
            for (&c, k) in t.kept.iter_mut() {
                // SAFETY: as in `capture`.
                if unsafe { &*(c as *const Cell<u32>) }.get() & super::HELD == 0 {
                    continue;
                }
                if let Some(&(f, _, r)) = t.fns.get(&c) {
                    if r != rt {
                        continue;
                    }
                    // SAFETY: registered for this cell's type.
                    unsafe { (f.swap)(c as *mut u8, k) };
                }
            }
        });
        SWAPPED.with(|s| s.set(on));
    }

    /// While a description runs: the intent of held cell `cell` (swapped out into the table).
    pub(crate) fn intent(cell: usize) -> Option<*const u8> {
        if !SWAPPED.with(|s| s.get()) {
            return None;
        }
        T.with(|t| {
            let t = t.borrow();
            let k = t.kept.get(&cell)?;
            let &(f, _, _) = t.fns.get(&cell)?;
            // SAFETY: registered for this cell's type.
            Some(unsafe { (f.value)(k) })
        })
    }

    /// Kept values (tests, the ledger).
    pub(crate) fn len() -> usize {
        T.with(|t| t.borrow().kept.len())
    }
}

impl<T: Default> Default for Signal<T> {
    fn default() -> Self {
        Signal::new(T::default())
    }
}

/// A signal operand of owner type `V`, at a compiler-proven offset (build it with [`sig!`](crate::sig)).
pub struct Wired<'a, T, V> {
    pub(crate) sig: &'a Signal<T>,
    pub(crate) off: u32,
    _v: PhantomData<fn(&V)>,
}

impl<T, V> Clone for Wired<'_, T, V> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T, V> Copy for Wired<'_, T, V> {}

impl<'a, T, V> Wired<'a, T, V> {
    /// The current value (for the description's initial state).
    #[inline(always)]
    pub fn get(&self) -> &'a T {
        &self.sig.value
    }
    /// A projection operand: `f` (`Copy`, at most 16 bytes of captures) runs on every update.
    pub fn map<R, F: Fn(&T) -> R + Copy + 'static>(self, f: F) -> WMap<'a, T, V, F> {
        WMap { w: self, f }
    }
    /// A numeric text operand: `f` computes the number, `fmt` renders it straight into the label's
    /// 16-byte vector (no `String`, bough-text's `NumFmt`; exact against `format!`).
    pub fn num<F: Fn(&T) -> f32 + Copy + 'static>(self, f: F, fmt: bough_text::numeric::NumFmt) -> Num<'a, T, V, F> {
        Num { w: self, f, fmt }
    }
}

/// `sig!(...).map(f)`.
pub struct WMap<'a, T, V, F> {
    pub(crate) w: Wired<'a, T, V>,
    pub(crate) f: F,
}

/// `sig!(...).num(f, fmt)`.
pub struct Num<'a, T, V, F> {
    pub(crate) w: Wired<'a, T, V>,
    pub(crate) f: F,
    pub(crate) fmt: bough_text::numeric::NumFmt,
}

/// A wired signal operand: `sig!(self.field.path)` inside a `View` impl, or `sig!(v: Type => field.path)`
/// where the owner is a value `v: &Type` (a list's row function).
/// A struct field path (nested paths too):
///
/// ```
/// use bough::raw::*;
/// struct Inner { n: Signal<u32> }
/// struct V { i: Inner }
/// impl View for V {
///     fn view(&self) -> impl Element<Self> + '_ { text(sig!(self.i.n).map(|n| n.to_string())) }
/// }
/// ```
///
/// Only struct field paths are accepted. Through a `Box` (or any `Deref`) the field access would compile but
/// `offset_of!` does not follow pointers:
///
/// ```compile_fail
/// use bough::raw::*;
/// struct Inner { n: Signal<u32> }
/// struct V { b: Box<Inner> }
/// impl View for V {
///     fn view(&self) -> impl Element<Self> + '_ { text(sig!(self.b.n).map(|n| n.to_string())) }
/// }
/// ```
///
/// A union field cannot be named without the caller's own `unsafe`:
///
/// ```compile_fail
/// use bough::raw::*;
/// struct Inner { n: Signal<u32> }
/// union U { a: std::mem::ManuallyDrop<Inner>, b: u64 }
/// struct V { u: U }
/// impl View for V {
///     fn view(&self) -> impl Element<Self> + '_ { text(sig!(self.u.a.n).map(|n| n.to_string())) }
/// }
/// ```
///
/// The field access and the accessor closure are outside the `unsafe` block on purpose: a union field (whose
/// bytes another field may overwrite) or a packed field cannot be named without the caller's own `unsafe`,
/// and an enum variant's field cannot be named as `owner.field` at all, so `sig!` accepts exactly the struct
/// field paths whose location and type are fixed for the owner's life.
#[macro_export]
macro_rules! sig {
    ($o:ident : $t:ty => $($f:ident).+) => {{
        let r = &$o.$($f).+;
        let p: fn(&$t) -> &_ = |o: &$t| &o.$($f).+;
        // SAFETY: the offset and the accessor name the same struct field path of `$t`.
        unsafe { $crate::signal::prove::<_, $t>(r, ::core::mem::offset_of!($t, $($f).+), p) }
    }};
    ($s:tt . $($f:ident).+) => {{
        let r = &$s.$($f).+;
        let p: fn(&Self) -> &_ = |o: &Self| &o.$($f).+;
        // SAFETY: the offset and the accessor name the same struct field path of `Self`.
        unsafe { $crate::signal::prove::<_, Self>(r, ::core::mem::offset_of!(Self, $($f).+), p) }
    }};
}

