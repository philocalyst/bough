//! bough-mem: the storage every structure the GPU reads lives in.
//!
//! The rule bent: "each structure owns its heap allocations, and whatever the GPU must see is copied
//! (mirrored) into a GPU buffer". Here a [`Region`] is ONE block of memory -- on Apple silicon the
//! `storageModeShared` MTLBuffer itself, otherwise zeroed heap memory -- and every GPU-visible column
//! (bough-core's pages and directory columns, its clip table, bough-text's glyph slots, run headers,
//! spaces and strike entries, the façade's instances) is a [`Col`] carved out of it once, at a fixed
//! offset and a fixed capacity. A column never moves, so the GPU reads the CPU's structures in place:
//! there is no mirror pass, and the offsets the GPU needs are known when the region is laid out.
//!
//! "Widen to delete corrections": capacities are reserved up front. Untouched pages of a shared
//! MTLBuffer or of `calloc`ed memory are not resident, so a generous reservation costs only what is
//! written (measured: `bough-metal/examples/resprobe.rs buf`).
//!
//! A `Col` that is not carved from a region owns its own allocation and grows like a `Vec`
//! ([`Col::owned`]): that is what the crates' standalone tests and benches use. A carved column cannot
//! grow past its capacity; overflowing it is a configuration error and panics with the column's name.
//!
//! # Safety model
//! A carved `Col` holds a reference count on its region (`Rc`), so the memory outlives every column.
//! Columns carved from one region never overlap (the carve cursor only advances). A `Col<T>` is the
//! unique owner of its elements: all access goes through `&self`/`&mut self`.
#![allow(clippy::len_without_is_empty)]

use std::alloc::{Layout, alloc_zeroed, dealloc, realloc};
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};
use std::ptr::NonNull;
use std::rc::Rc;

/// Alignment of every carved column (a cache line; the GPU's buffer offsets need 4, NEON 16).
pub const COL_ALIGN: usize = 64;
/// Alignment of a heap region (one VM page on Apple silicon).
pub const PAGE: usize = 16384;

struct RegionInner {
    base: NonNull<u8>,
    len: usize,
    /// `Some` when the region allocated its memory itself (freed on drop).
    heap: Option<Layout>,
    /// Keeps external memory alive (e.g. the MTLBuffer whose contents the region is).
    _keep: Option<Box<dyn std::any::Any>>,
}

impl Drop for RegionInner {
    fn drop(&mut self) {
        if let Some(l) = self.heap {
            // SAFETY: allocated in `Region::heap` with exactly this layout.
            unsafe { dealloc(self.base.as_ptr(), l) };
        }
    }
}

/// One block of memory carved into fixed columns (see the crate docs).
pub struct Region {
    inner: Rc<RegionInner>,
    next: usize,
}

impl Region {
    /// `len` bytes of zeroed heap memory (lazily backed by the OS).
    pub fn heap(len: usize) -> Region {
        let len = len.next_multiple_of(PAGE).max(PAGE);
        let l = Layout::from_size_align(len, PAGE).expect("region layout");
        // SAFETY: non-zero size.
        let p = unsafe { alloc_zeroed(l) };
        let base = NonNull::new(p).expect("region allocation");
        Region { inner: Rc::new(RegionInner { base, len, heap: Some(l), _keep: None }), next: 0 }
    }

    /// The region is `len` bytes at `base`, kept alive by `keep` (dropped with the last column).
    ///
    /// # Safety
    /// `base` must be valid for reads and writes of `len` bytes, aligned to [`COL_ALIGN`], zeroed or
    /// otherwise initialised, and stay valid (and not be accessed except through this region's columns
    /// and [`Region::base`]) for as long as `keep` is alive.
    pub unsafe fn external(base: *mut u8, len: usize, keep: Box<dyn std::any::Any>) -> Region {
        assert!(base as usize % COL_ALIGN == 0, "region base must be {COL_ALIGN}-aligned");
        let base = NonNull::new(base).expect("region base");
        Region { inner: Rc::new(RegionInner { base, len, heap: None, _keep: Some(keep) }), next: 0 }
    }

    pub fn base(&self) -> *mut u8 {
        self.inner.base.as_ptr()
    }
    pub fn len(&self) -> usize {
        self.inner.len
    }
    /// Bytes carved so far.
    pub fn used(&self) -> usize {
        self.next
    }

    /// Reserve `bytes` raw bytes (aligned to `align`, at least [`COL_ALIGN`]); returns their offset.
    pub fn reserve(&mut self, bytes: usize, align: usize) -> usize {
        let a = align.max(COL_ALIGN);
        let off = self.next.next_multiple_of(a);
        assert!(off + bytes <= self.inner.len, "bough-mem: region of {} bytes is full (need {} more at {off})", self.inner.len, bytes);
        self.next = off + bytes;
        off
    }

    /// A column of capacity `cap` at the next free offset. Its memory is the region's (zeroed if the
    /// region was); the column starts empty.
    pub fn carve<T>(&mut self, name: &'static str, cap: usize) -> Col<T> {
        let bytes = cap.checked_mul(size_of::<T>()).expect("column size");
        let off = self.reserve(bytes, align_of::<T>());
        assert!(off <= u32::MAX as usize, "region offsets are u32");
        // SAFETY: off + bytes <= len (checked in `reserve`), base is valid for len bytes.
        let ptr = unsafe { NonNull::new_unchecked(self.inner.base.as_ptr().add(off) as *mut T) };
        Col { ptr, len: 0, cap, off: off as u32, name, region: Some(self.inner.clone()), _t: PhantomData }
    }
}

/// A fixed-capacity column carved from a [`Region`], or an owned growable one ([`Col::owned`]).
pub struct Col<T> {
    ptr: NonNull<T>,
    len: usize,
    cap: usize,
    /// Byte offset of element 0 in its region (`u32::MAX` when owned).
    off: u32,
    name: &'static str,
    /// `None` = owned (heap, growable).
    region: Option<Rc<RegionInner>>,
    _t: PhantomData<T>,
}

impl<T> Col<T> {
    /// An empty owned column (grows like a `Vec`; never seen by the GPU).
    pub fn owned(name: &'static str) -> Col<T> {
        Col { ptr: NonNull::dangling(), len: 0, cap: if size_of::<T>() == 0 { usize::MAX } else { 0 }, off: u32::MAX, name, region: None, _t: PhantomData }
    }
    pub fn owned_with_capacity(name: &'static str, cap: usize) -> Col<T> {
        let mut c = Col::owned(name);
        c.grow_to(cap);
        c
    }
    #[inline(always)]
    pub fn len(&self) -> usize {
        self.len
    }
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    #[inline(always)]
    pub fn capacity(&self) -> usize {
        self.cap
    }
    /// Byte offset of element 0 in the region (`None` for an owned column).
    pub fn region_offset(&self) -> Option<usize> {
        self.region.as_ref().map(|_| self.off as usize)
    }
    pub fn is_carved(&self) -> bool {
        self.region.is_some()
    }
    #[inline(always)]
    pub fn as_ptr(&self) -> *const T {
        self.ptr.as_ptr()
    }
    #[inline(always)]
    pub fn as_mut_ptr(&mut self) -> *mut T {
        self.ptr.as_ptr()
    }

    #[cold]
    #[inline(never)]
    fn grow_to(&mut self, want: usize) {
        if want <= self.cap {
            return;
        }
        assert!(self.region.is_none(), "bough-mem: column `{}` is full (capacity {}, need {want}); raise it in the region layout", self.name, self.cap);
        let new_cap = want.max(self.cap * 2).max(4);
        let nl = Layout::array::<T>(new_cap).expect("column layout");
        // SAFETY: owned columns allocate with Layout::array::<T>(cap); size > 0 (T is not zero-sized here
        // because a ZST column has cap = usize::MAX and never grows).
        let p = unsafe {
            if self.cap == 0 {
                alloc_zeroed(nl)
            } else {
                realloc(self.ptr.as_ptr() as *mut u8, Layout::array::<T>(self.cap).unwrap(), nl.size())
            }
        };
        self.ptr = NonNull::new(p as *mut T).expect("column allocation");
        self.cap = new_cap;
    }

    /// Append (grows an owned column; a full carved column panics).
    #[inline]
    pub fn push(&mut self, v: T) {
        if self.len == self.cap {
            self.grow_to(self.len + 1);
        }
        // SAFETY: len < cap after the growth check.
        unsafe { self.ptr.as_ptr().add(self.len).write(v) };
        self.len += 1;
    }

    /// Remove and drop the last element.
    pub fn pop(&mut self) -> Option<T> {
        if self.len == 0 {
            return None;
        }
        self.len -= 1;
        // SAFETY: element `len` was initialised and is now logically removed.
        Some(unsafe { self.ptr.as_ptr().add(self.len).read() })
    }

    /// Remove element `i`, shifting the tail down.
    pub fn remove(&mut self, i: usize) -> T {
        assert!(i < self.len, "column `{}`: remove index {i} out of range {}", self.name, self.len);
        // SAFETY: i < len; the tail [i+1, len) moves down by one (overlapping copy).
        unsafe {
            let p = self.ptr.as_ptr().add(i);
            let v = p.read();
            std::ptr::copy(p.add(1), p, self.len - i - 1);
            self.len -= 1;
            v
        }
    }

    /// Drop every element (keeps the capacity).
    pub fn clear(&mut self) {
        let n = self.len;
        self.len = 0;
        if std::mem::needs_drop::<T>() {
            for i in 0..n {
                // SAFETY: elements [0, n) were initialised.
                unsafe { self.ptr.as_ptr().add(i).drop_in_place() };
            }
        }
    }
}

impl<T: Copy> Col<T> {
    /// Insert `count` copies of `v` at `at` (the tail shifts up).
    pub fn insert_n(&mut self, at: usize, count: usize, v: T) {
        assert!(at <= self.len, "column `{}`: insert index {at} out of range {}", self.name, self.len);
        if self.len + count > self.cap {
            self.grow_to(self.len + count);
        }
        // SAFETY: len + count <= cap; the tail [at, len) moves up by `count` (overlapping copy).
        unsafe {
            let p = self.ptr.as_ptr().add(at);
            std::ptr::copy(p, p.add(count), self.len - at);
            for k in 0..count {
                p.add(k).write(v);
            }
        }
        self.len += count;
    }
    /// Resize to `n`, filling new elements with `v`.
    /// Grow to `n` elements, the new ones all-zero bytes: one `memset`, not an element loop (text round 3: a
    /// glyph block of 512 slots was ~1,500 instructions of element writes).
    ///
    /// # Safety
    /// The all-zero bit pattern must be a valid `T`.
    pub unsafe fn grow_zeroed(&mut self, n: usize) {
        if n <= self.len {
            return;
        }
        if n > self.cap {
            self.grow_to(n);
        }
        // SAFETY: len..n lies inside the capacity; the caller vouches for zero being a valid T.
        unsafe { std::ptr::write_bytes(self.ptr.as_ptr().add(self.len), 0, n - self.len) };
        self.len = n;
    }

    pub fn resize(&mut self, n: usize, v: T) {
        if n > self.len {
            if n > self.cap {
                self.grow_to(n);
            }
            for i in self.len..n {
                // SAFETY: i < n <= cap.
                unsafe { self.ptr.as_ptr().add(i).write(v) };
            }
        }
        self.len = n;
    }
    pub fn truncate(&mut self, n: usize) {
        self.len = self.len.min(n);
    }
    /// Become a copy of `src` (same contents; capacity grows if owned).
    pub fn clone_from_slice_of(&mut self, src: &[T]) {
        if src.len() > self.cap {
            self.grow_to(src.len());
        }
        // SAFETY: src.len() <= cap; the two never overlap (distinct columns).
        unsafe { std::ptr::copy_nonoverlapping(src.as_ptr(), self.ptr.as_ptr(), src.len()) };
        self.len = src.len();
    }
    pub fn extend_from_slice(&mut self, src: &[T]) {
        if self.len + src.len() > self.cap {
            self.grow_to(self.len + src.len());
        }
        // SAFETY: room checked; src is not inside this column's spare capacity (it is a live slice).
        unsafe { std::ptr::copy_nonoverlapping(src.as_ptr(), self.ptr.as_ptr().add(self.len), src.len()) };
        self.len += src.len();
    }
    /// Set the length to `n` without writing (the elements below `n` must already hold valid values:
    /// for carved columns over zeroed memory, `T`'s all-zero bit pattern).
    ///
    /// # Safety
    /// Elements `[len, n)` must be initialised values of `T`, and `n <= capacity`.
    pub unsafe fn set_len(&mut self, n: usize) {
        debug_assert!(n <= self.cap);
        self.len = n;
    }
}

impl<T> Drop for Col<T> {
    fn drop(&mut self) {
        self.clear();
        if self.region.is_none() && self.cap != 0 && size_of::<T>() != 0 {
            // SAFETY: owned columns allocate with Layout::array::<T>(cap).
            unsafe { dealloc(self.ptr.as_ptr() as *mut u8, Layout::array::<T>(self.cap).unwrap()) };
        }
    }
}

impl<T> Deref for Col<T> {
    type Target = [T];
    #[inline(always)]
    fn deref(&self) -> &[T] {
        // SAFETY: [0, len) initialised, ptr valid (dangling only when len == 0).
        unsafe { std::slice::from_raw_parts(self.ptr.as_ptr(), self.len) }
    }
}
impl<T> DerefMut for Col<T> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut [T] {
        // SAFETY: as in `deref`; &mut self is exclusive.
        unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len) }
    }
}

impl<T: Copy> Clone for Col<T> {
    /// Clones into an OWNED column (a replica never aliases the GPU's memory).
    fn clone(&self) -> Self {
        let mut c = Col::owned(self.name);
        c.extend_from_slice(self);
        c
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for Col<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl<T> Default for Col<T> {
    fn default() -> Self {
        Col::owned("col")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owned_grows_like_vec() {
        let mut c: Col<u32> = Col::owned("t");
        let mut v = Vec::new();
        for i in 0..1000u32 {
            c.push(i);
            v.push(i);
        }
        c.insert_n(10, 5, 7);
        v.splice(10..10, std::iter::repeat_n(7, 5));
        assert_eq!(c.remove(3), v.remove(3));
        c.resize(2000, 9);
        v.resize(2000, 9);
        assert_eq!(&*c, &v[..]);
        c.truncate(17);
        v.truncate(17);
        assert_eq!(c.pop(), v.pop());
        assert_eq!(&*c, &v[..]);
    }

    #[test]
    fn carved_columns_are_disjoint_and_fixed() {
        let mut r = Region::heap(1 << 16);
        let mut a: Col<u64> = r.carve("a", 100);
        let mut b: Col<u16> = r.carve("b", 100);
        let pa = a.as_ptr();
        for i in 0..100 {
            a.push(i);
            b.push(i as u16 * 3);
        }
        assert_eq!(a.as_ptr(), pa);
        assert_eq!(a.region_offset(), Some(0));
        assert_eq!(b.region_offset(), Some(832)); // 800 rounded up to 64
        assert!(a.iter().enumerate().all(|(i, &v)| v == i as u64));
        assert!(b.iter().enumerate().all(|(i, &v)| v == i as u16 * 3));
        drop(r); // the columns keep the memory alive
        assert_eq!(a[99], 99);
    }

    #[test]
    #[should_panic(expected = "is full")]
    fn carved_column_overflow_panics() {
        let mut r = Region::heap(1 << 14);
        let mut a: Col<u32> = r.carve("small", 4);
        for i in 0..5 {
            a.push(i);
        }
    }
}
