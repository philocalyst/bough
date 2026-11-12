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
/// The head's cell is held by a pending async cohort (`reader.rs`): only `latest` wires run until the
/// cohort commits, which re-queues it.
pub const HELD: u32 = 1 << 30;

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
    /// The value, read by the runtime (a wire's patch): not a description's plain read (`grain.rs`).
    #[inline(always)]
    pub(crate) fn peek(&self) -> &T {
        &self.value
    }
    /// Overwrite the value without queueing its wires: for a value the runtime itself just wrote
    /// (a measured list's anchored scroll offset), mirrored back into the owner.
    #[inline]
    pub fn set_silent(&mut self, v: T) {
        self.value = v;
    }
}

impl<T: PartialEq> Signal<T> {
    /// Equality cut-off, then mark. Returns whether the value changed. A signal whose one binding is a direct
    /// wire (`direct.rs`) is applied here: the destination's column takes the value, no settle runs.
    #[inline]
    pub fn set(&mut self, v: T) -> bool {
        if self.value == v {
            return false;
        }
        let h = self.head.0.get();
        // SAFETY: a head is tagged `DIRECT` only for a `Signal<Color>` (`direct::store`'s contract).
        if h & (crate::direct::DIRECT | HELD) == crate::direct::DIRECT && unsafe { crate::direct::store(h, &self.head.0, &v) } {
            self.value = v;
            return true;
        }
        *self.deref_mut() = v;
        true
    }
}

impl<T> Deref for Signal<T> {
    type Target = T;
    /// A plain read: while an owner describes itself it subscribes the owner to this signal (`grain.rs`).
    #[inline(always)]
    fn deref(&self) -> &T {
        crate::grain::read(&self.head.0, crate::grain::Dep::Value, crate::grain::wake_root);
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
    /// Runtimes alive on this thread: with one, an address that is not its root owner's can be dropped.
    pub live: Cell<u32>,
    pub at: [Cell<usize>; LOG_CAP],
}

thread_local! {
    pub(crate) static LOG: WriteLog = const {
        WriteLog { n: Cell::new(0), epoch: Cell::new(0), live: Cell::new(0), at: [const { Cell::new(0) }; LOG_CAP] }
    };
}

#[cold]
#[inline(never)]
fn log(addr: usize) {
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

/// Queue a head's wires: nothing for a head without wires or one already queued (two instructions and a
/// predicted branch on the path every later write of the turn takes).
#[inline(always)]
pub(crate) fn queue(head: &Cell<u32>) {
    let h = head.get();
    if h != 0 && h & QUEUED == 0 {
        head.set(h | QUEUED);
        log(head as *const _ as usize);
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

