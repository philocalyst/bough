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

#[repr(C)]
pub struct Signal<T> {
    head: Cell<u32>,
    value: T,
}

impl<T> Signal<T> {
    pub const fn new(value: T) -> Self {
        Signal { head: Cell::new(0), value }
    }
    /// The wire head. Crate-private: a head names a wire list monomorphised for this signal's `T`, so
    /// copying one head into another signal's would make the runtime run wires of the wrong type.
    #[inline(always)]
    pub(crate) fn head(&self) -> &Cell<u32> {
        &self.head
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
    /// Sets QUEUED (one OR) if the signal has wires.
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut T {
        let h = self.head.get();
        self.head.set(h | (((h != 0) as u32) << 31));
        &mut self.value
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
    /// `sig!`'s expansion. `off` is `offset_of!(V, path)`; `_path` is the field accessor `|o: &V| &o.path`
    /// (never called: its signature proves the field at `off` is a `Signal<T>`).
    ///
    /// # Safety
    /// `off` must be `offset_of!(V, path)` for the same struct field path as `_path` (the macro computes
    /// both from one token sequence; nothing else may call this).
    #[doc(hidden)]
    #[inline(always)]
    pub unsafe fn proven(sig: &'a Signal<T>, off: usize, _path: fn(&V) -> &Signal<T>) -> Self {
        assert!(off + size_of::<Signal<T>>() <= size_of::<V>() && off <= u32::MAX as usize);
        Wired { sig, off: off as u32, _v: PhantomData }
    }
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
/// use bough::prelude::*;
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
/// use bough::prelude::*;
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
/// use bough::prelude::*;
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
        unsafe { $crate::signal::Wired::<_, $t>::proven(r, ::core::mem::offset_of!($t, $($f).+), p) }
    }};
    ($s:tt . $($f:ident).+) => {{
        let r = &$s.$($f).+;
        let p: fn(&Self) -> &_ = |o: &Self| &o.$($f).+;
        // SAFETY: the offset and the accessor name the same struct field path of `Self`.
        unsafe { $crate::signal::Wired::<_, Self>::proven(r, ::core::mem::offset_of!(Self, $($f).+), p) }
    }};
}
