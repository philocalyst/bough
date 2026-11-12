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
//! "Widen to delete corrections": capacities are reserved up front, as **address space**. A heap region
//! is an anonymous `mmap`: an untouched page is neither resident nor charged. (Round 6, lane memory,
//! `bough-metal/examples/vmprobe.rs`: a storageModeShared MTLBuffer is wired and charged *whole* once it
//! joins a residency set, so a region that is one GPU buffer costs its capacity; and `alloc_zeroed` with
//! page alignment zero-fills every page. The fused presenter therefore wraps each GPU-read column's *used
//! prefix* in its own no-copy buffer and grows that view, never the column: see bough-metal `fused.rs`.)
//! GPU-read columns are carved on page boundaries ([`Region::carve_paged`]) so each can have its own view.
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

pub mod count;
pub mod pmu;

use std::alloc::{Layout, alloc_zeroed, dealloc, realloc};
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};
use std::ptr::NonNull;
use std::rc::Rc;

/// Alignment of every carved column (a cache line; the GPU's buffer offsets need 4, NEON 16).
pub const COL_ALIGN: usize = 64;
/// Alignment of a heap region (one VM page on Apple silicon).
pub const PAGE: usize = 16384;

/// Who owns a region's memory.
enum Mem {
    /// An anonymous private mapping of `len` bytes (unmapped on drop).
    #[cfg(unix)]
    Map,
    /// The global allocator (non-unix targets), with this layout.
    #[cfg_attr(unix, allow(dead_code))]
    Alloc(Layout),
    /// Someone else's (`Region::external`), kept alive by `_keep`.
    External,
}

struct RegionInner {
    base: NonNull<u8>,
    len: usize,
    mem: Mem,
    /// Keeps external memory alive (e.g. the MTLBuffer whose contents the region is).
    _keep: Option<Box<dyn std::any::Any>>,
}

#[cfg(unix)]
unsafe extern "C" {
    fn mmap(addr: *mut u8, len: usize, prot: i32, flags: i32, fd: i32, off: i64) -> *mut u8;
    fn munmap(addr: *mut u8, len: usize) -> i32;
}

impl Drop for RegionInner {
    fn drop(&mut self) {
        match self.mem {
            // SAFETY: mapped in `Region::heap` with exactly this length; no column outlives the region.
            #[cfg(unix)]
            Mem::Map => unsafe {
                munmap(self.base.as_ptr(), self.len);
            },
            // SAFETY: allocated in `Region::heap` with exactly this layout.
            Mem::Alloc(l) => unsafe { dealloc(self.base.as_ptr(), l) },
            Mem::External => {}
        }
    }
}

/// One block of memory carved into fixed columns (see the crate docs).
pub struct Region {
    inner: Rc<RegionInner>,
    next: usize,
}

impl Region {
    /// `len` bytes of zeroed memory, backed lazily: an anonymous private `mmap` (a page costs nothing until
    /// it is written), so a region can reserve its columns' largest capacities as address space.
    /// (Before round 6 it was `alloc_zeroed` with a 16 KiB alignment, which is `posix_memalign` + `bzero` of the
    /// whole block: lane `store` measured 39 M instructions for a 50 MB region at every `App::new`.)
    pub fn heap(len: usize) -> Region {
        let len = len.next_multiple_of(PAGE).max(PAGE);
        #[cfg(unix)]
        {
            #[cfg(target_os = "macos")]
            const MAP_ANON: i32 = 0x1000;
            #[cfg(not(target_os = "macos"))]
            const MAP_ANON: i32 = 0x20;
            const PROT_RW: i32 = 0x1 | 0x2;
            const MAP_PRIVATE: i32 = 0x2;
            // SAFETY: a fresh anonymous private mapping; the kernel picks the (page-aligned) address.
            let p = unsafe { mmap(std::ptr::null_mut(), len, PROT_RW, MAP_ANON | MAP_PRIVATE, -1, 0) };
            assert!(p as isize != -1 && !p.is_null(), "bough-mem: mmap of a {len}-byte region failed");
            let base = NonNull::new(p).expect("region mapping");
            Region { inner: Rc::new(RegionInner { base, len, mem: Mem::Map, _keep: None }), next: 0 }
        }
        #[cfg(not(unix))]
        {
            let l = Layout::from_size_align(len, PAGE).expect("region layout");
            // SAFETY: non-zero size.
            let p = unsafe { alloc_zeroed(l) };
            let base = NonNull::new(p).expect("region allocation");
            Region { inner: Rc::new(RegionInner { base, len, mem: Mem::Alloc(l), _keep: None }), next: 0 }
        }
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
        Region { inner: Rc::new(RegionInner { base, len, mem: Mem::External, _keep: Some(keep) }), next: 0 }
    }

    pub fn base(&self) -> *mut u8 {
        self.inner.base.as_ptr()
    }
    /// A handle that keeps the region's memory alive (for a view of it made elsewhere, e.g. a GPU buffer
    /// wrapping it without a copy).
    pub fn keeper(&self) -> Box<dyn std::any::Any> {
        Box::new(self.inner.clone())
    }
    pub fn len(&self) -> usize {
        self.inner.len
    }
    /// Bytes carved so far.
    pub fn used(&self) -> usize {
        self.next
    }
    /// Resident and dirty bytes of the whole region now (diagnostics, [`residency`]).
    pub fn residency(&self) -> (usize, usize) {
        residency(self.inner.base.as_ptr(), self.inner.len)
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
        self.carve_aligned(name, cap, align_of::<T>())
    }

    /// [`Region::carve`] starting on a VM page boundary, so that a view of the column alone (a GPU buffer
    /// over its used prefix) can be made.
    pub fn carve_paged<T>(&mut self, name: &'static str, cap: usize) -> Col<T> {
        self.carve_aligned(name, cap, vm_page().max(align_of::<T>()))
    }

    fn carve_aligned<T>(&mut self, name: &'static str, cap: usize, align: usize) -> Col<T> {
        let bytes = cap.checked_mul(size_of::<T>()).expect("column size");
        let off = self.reserve(bytes, align);
        // SAFETY: off + bytes <= len (checked in `reserve`), base is valid for len bytes.
        let ptr = unsafe { NonNull::new_unchecked(self.inner.base.as_ptr().add(off) as *mut T) };
        Col { ptr, len: 0, cap, off, name, region: Some(self.inner.clone()), _t: PhantomData }
    }
}

/// A fixed-capacity column carved from a [`Region`], or an owned growable one ([`Col::owned`]).
pub struct Col<T> {
    ptr: NonNull<T>,
    len: usize,
    cap: usize,
    /// Byte offset of element 0 in its region (`usize::MAX` when owned). A region the GPU addresses keeps
    /// its offsets below 2^32 (its user checks, bough `rt::gpu_off`); a CPU-only region (the façade's store
    /// block) may be larger.
    off: usize,
    name: &'static str,
    /// `None` = owned (heap, growable).
    region: Option<Rc<RegionInner>>,
    _t: PhantomData<T>,
}

impl<T> Col<T> {
    /// An empty owned column (grows like a `Vec`; never seen by the GPU).
    pub fn owned(name: &'static str) -> Col<T> {
        Col { ptr: NonNull::dangling(), len: 0, cap: if size_of::<T>() == 0 { usize::MAX } else { 0 }, off: usize::MAX, name, region: None, _t: PhantomData }
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
        self.region.as_ref().map(|_| self.off)
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

impl<T> Col<T> {
    /// This column's line in a memory ledger (diagnostics; never on a frame path): elements in use, the
    /// capacity's bytes, and how many of the pages under the capacity are resident and dirty now.
    pub fn mem(&self) -> ColMem {
        let elem = size_of::<T>();
        let reserved = if elem == 0 { 0 } else { self.cap * elem };
        let (resident, dirty) = if reserved == 0 { (0, 0) } else { residency(self.ptr.as_ptr() as *const u8, reserved) };
        ColMem { name: self.name, elem, len: self.len, cap: self.cap, carved: self.region.is_some(), resident, dirty }
    }
}

/// One column's memory (see [`Col::mem`]). `resident` / `dirty` count whole VM pages overlapping the
/// capacity, so neighbours sharing a page are counted by both (the ledger reports the region's pages once).
#[derive(Clone, Copy, Debug)]
pub struct ColMem {
    pub name: &'static str,
    pub elem: usize,
    pub len: usize,
    pub cap: usize,
    pub carved: bool,
    pub resident: usize,
    pub dirty: usize,
}

impl ColMem {
    pub fn used_bytes(&self) -> usize {
        self.len * self.elem
    }
    pub fn cap_bytes(&self) -> usize {
        self.cap * self.elem
    }
}

/// The VM page size (16 KB on Apple silicon).
pub fn vm_page() -> usize {
    #[cfg(unix)]
    {
        unsafe extern "C" {
            fn getpagesize() -> i32;
        }
        // SAFETY: no arguments, no preconditions.
        (unsafe { getpagesize() }) as usize
    }
    #[cfg(not(unix))]
    {
        4096
    }
}

/// Resident and dirty (modified) bytes of the VM pages overlapping `[ptr, ptr + len)`, by `mincore`
/// (diagnostics: one system call and one byte per page). Pages are whole: a range sharing a page with
/// something else is charged that page. `(0, 0)` where `mincore` is unavailable or fails.
pub fn residency(ptr: *const u8, len: usize) -> (usize, usize) {
    #[cfg(unix)]
    {
        if len == 0 {
            return (0, 0);
        }
        unsafe extern "C" {
            fn mincore(addr: *const u8, len: usize, vec: *mut u8) -> i32;
        }
        const INCORE: u8 = 0x1;
        // MINCORE_MODIFIED and MINCORE_MODIFIED_OTHER (xnu `sys/mman.h`)
        const MODIFIED: u8 = 0x4 | 0x10;
        let pg = vm_page();
        let start = ptr as usize & !(pg - 1);
        let end = (ptr as usize + len).next_multiple_of(pg);
        let n = (end - start) / pg;
        let mut v = vec![0u8; n];
        // SAFETY: `start` is page-aligned, `v` holds one byte per page of [start, end); mincore only reads
        // the page tables (an unmapped page makes it fail, which we report as zero).
        if unsafe { mincore(start as *const u8, end - start, v.as_mut_ptr()) } != 0 {
            return (0, 0);
        }
        let res = v.iter().filter(|&&b| b & INCORE != 0).count() * pg;
        let dirty = v.iter().filter(|&&b| b & INCORE != 0 && b & MODIFIED != 0).count() * pg;
        (res, dirty)
    }
    #[cfg(not(unix))]
    {
        let _ = (ptr, len);
        (0, 0)
    }
}

/// A memory ledger: one line per holder (diagnostics: built on demand, never on a frame path).
#[derive(Clone, Debug, Default)]
pub struct Ledger {
    pub lines: Vec<Line>,
}

/// What a holder's size follows: the mounted content (rows, records, glyphs), the window (surfaces, tile
/// rows), or neither (process constants: fonts, tables, pipelines).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scales {
    Content,
    Window,
    Fixed,
}

/// Where a holder's bytes live.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Space {
    /// A region column (inside the shared GPU buffer when windowed).
    Region,
    /// The Rust heap.
    Heap,
    /// A GPU object (texture, buffer, IOSurface) outside the region.
    Gpu,
}

#[derive(Clone, Debug)]
pub struct Line {
    pub group: &'static str,
    pub name: String,
    pub space: Space,
    pub scales: Scales,
    /// Bytes holding live elements.
    pub used: usize,
    /// Bytes reserved (capacity).
    pub reserved: usize,
    /// Resident and dirty bytes when measured (`None`: not measurable here; then the reservation is
    /// what it costs, e.g. small heap blocks that are fully written).
    pub resident: Option<usize>,
    pub dirty: Option<usize>,
}

/// Heap allocations at least this large are measured with `mincore` (malloc serves them from whole
/// pages); smaller ones are charged their capacity.
const MEASURE_HEAP: usize = 64 << 10;

impl Ledger {
    pub fn push(&mut self, group: &'static str, name: impl Into<String>, space: Space, scales: Scales, used: usize, reserved: usize, resident: Option<usize>, dirty: Option<usize>) {
        self.lines.push(Line { group, name: name.into(), space, scales, used, reserved, resident, dirty });
    }
    /// A column (region-carved or owned).
    pub fn col<T>(&mut self, group: &'static str, c: &Col<T>, scales: Scales) {
        let m = c.mem();
        let space = if m.carved { Space::Region } else { Space::Heap };
        self.push(group, m.name, space, scales, m.used_bytes(), m.cap_bytes(), Some(m.resident), Some(m.dirty));
    }
    /// A heap vector (its own block only; nested heap blocks are the caller's to add).
    pub fn vec<T>(&mut self, group: &'static str, name: impl Into<String>, v: &[T], cap: usize, scales: Scales) {
        let e = size_of::<T>();
        self.heap(group, name, scales, v.len() * e, cap * e, v.as_ptr() as *const u8);
    }
    /// A heap block of `reserved` bytes at `ptr` (`used` of them live).
    pub fn heap(&mut self, group: &'static str, name: impl Into<String>, scales: Scales, used: usize, reserved: usize, ptr: *const u8) {
        let (resident, dirty) = if reserved >= MEASURE_HEAP {
            let (r, d) = residency(ptr, reserved);
            (Some(r.min(reserved)), Some(d.min(reserved)))
        } else {
            (None, None)
        };
        self.push(group, name, Space::Heap, scales, used, reserved, resident, dirty);
    }
    /// Many small heap blocks summed (nested vectors, strings, boxes): `n` blocks, `used` / `reserved` bytes.
    pub fn many(&mut self, group: &'static str, name: impl Into<String>, scales: Scales, used: usize, reserved: usize) {
        self.push(group, name, Space::Heap, scales, used, reserved, None, None);
    }
    /// Lines whose resident bytes are known, else their reservation.
    pub fn cost(l: &Line) -> usize {
        l.resident.unwrap_or(l.reserved)
    }
    pub fn total(&self, space: Space) -> usize {
        self.lines.iter().filter(|l| l.space == space).map(Ledger::cost).sum()
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

    /// `Region::heap` is a mapping: zero when first read, writable at both ends, column-aligned, and far
    /// larger than what is touched without anything being cleared up front (a 4 GiB region is made and
    /// dropped here; an allocator's zeroed block of that size would be written whole).
    #[test]
    fn heap_region_is_zero_aligned_and_lazy() {
        let mut r = Region::heap(4 << 30);
        assert_eq!(r.base() as usize % COL_ALIGN, 0);
        assert!(r.len() >= 4 << 30);
        let mut c: Col<u64> = r.carve("probe", (4usize << 30) / 8 - 4096);
        // SAFETY: mapped memory reads as zero and all-zero is a valid u64.
        unsafe { c.set_len(c.capacity()) };
        let last = c.len() - 1;
        assert_eq!((c[0], c[last / 2], c[last]), (0, 0, 0));
        c[0] = 7;
        c[last] = 9;
        assert_eq!((c[0], c[last]), (7, 9));
    }

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

    /// A heap region is address space: reserving 256 MiB makes nothing resident, and writing one element of
    /// a column far into it makes one page resident (the old `alloc_zeroed` with page alignment zero-filled every
    /// page: this test fails with it).
    #[cfg(unix)]
    #[test]
    fn heap_region_is_lazy() {
        let mut r = Region::heap(256 << 20);
        assert_eq!(r.residency().0, 0, "a fresh region has no resident pages");
        let _a: Col<u64> = r.carve_paged("a", 1 << 20);
        let mut b: Col<u64> = r.carve_paged("b", 1 << 20);
        assert_eq!(b.region_offset().unwrap() % vm_page(), 0, "carve_paged starts on a page boundary");
        // SAFETY: zero is a valid u64 and the region is zeroed.
        unsafe { b.set_len(1 << 19) };
        b[1 << 18] = 7;
        assert_eq!(r.residency().0, vm_page(), "one written element, one resident page");
        assert_eq!(b.mem().resident, vm_page());
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
