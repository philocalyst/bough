//! A counting global allocator: the debug allocator hook of the memory ledger (AUDIT R4).
//!
//! A binary that wants it opts in (`#[global_allocator] static A: bough_mem::count::Counting =
//! bough_mem::count::Counting;`); nothing else pays for it. It forwards to the system allocator and keeps
//! four relaxed counters: allocations, frees, live bytes and the peak of live bytes. [`allocs`] read before
//! and after a stretch of frames is the steady-state assertion ("a frame allocates nothing"); [`live`] is
//! the Rust heap the process holds, which the ledger splits by holder.
//!
//! Only Rust allocations pass through a `#[global_allocator]`: memory the system frameworks (AppKit,
//! Core Animation, Metal) take from malloc directly is not counted here (the ledger reads it from the
//! malloc zones).
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

pub struct Counting;

static ALLOCS: AtomicUsize = AtomicUsize::new(0);
static FREES: AtomicUsize = AtomicUsize::new(0);
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

#[inline(always)]
fn add(n: usize) {
    ALLOCS.fetch_add(1, Relaxed);
    let l = LIVE.fetch_add(n, Relaxed) + n;
    PEAK.fetch_max(l, Relaxed);
}
#[inline(always)]
fn sub(n: usize) {
    FREES.fetch_add(1, Relaxed);
    LIVE.fetch_sub(n, Relaxed);
}

// SAFETY: every call forwards to `System` with the caller's layout unchanged; the counters are plain atomics
// and never allocate.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        // SAFETY: forwarded contract.
        let p = unsafe { System.alloc(l) };
        if !p.is_null() {
            add(l.size());
        }
        p
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        // SAFETY: forwarded contract.
        let p = unsafe { System.alloc_zeroed(l) };
        if !p.is_null() {
            add(l.size());
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        sub(l.size());
        // SAFETY: forwarded contract.
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, new: usize) -> *mut u8 {
        // SAFETY: forwarded contract.
        let q = unsafe { System.realloc(p, l, new) };
        if !q.is_null() {
            sub(l.size());
            add(new);
        }
        q
    }
}

/// Allocations so far (a realloc counts as one allocation and one free).
pub fn allocs() -> usize {
    ALLOCS.load(Relaxed)
}
pub fn frees() -> usize {
    FREES.load(Relaxed)
}
/// Live bytes of Rust heap (requested sizes; malloc's rounding is not included).
pub fn live() -> usize {
    LIVE.load(Relaxed)
}
pub fn peak() -> usize {
    PEAK.load(Relaxed)
}
/// Is the counting allocator installed? (Something allocated through it.)
pub fn installed() -> bool {
    allocs() > 0
}
