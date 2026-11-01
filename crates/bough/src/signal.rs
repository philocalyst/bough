//! `Signal<T>`: a value plus a 4-byte wire head.
//!
//! `head` = index of the first wire of this signal in the runtime's wire arena (0 = unbound) with
//! bit 31 = QUEUED. `#[repr(C)]` with the head first so the runtime can reach the head of any
//! `Signal<_>` at `owner + offset` without knowing `T`.
use std::cell::Cell;
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
    #[inline(always)]
    pub fn head(&self) -> &Cell<u32> {
        &self.head
    }
    /// A projection operand: `f` is evaluated against the signal's value on every update.
    pub fn map<R, F: Fn(&T) -> R + Copy + 'static>(&self, f: F) -> Map<'_, T, F> {
        Map { sig: self, f }
    }
    pub fn into_inner(self) -> T {
        self.value
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

/// `sig.map(f)`: an operand with the recipe inline (`f` must be `Copy`, at most 16 bytes of captures).
pub struct Map<'a, T, F> {
    pub(crate) sig: &'a Signal<T>,
    pub(crate) f: F,
}
