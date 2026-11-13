//! What a global allocator choice can buy at start-up (threads lane, prediction A2): the mount cell of `hot_x`
//! under the system allocator (`BUMP=0`, the default) and under a bump allocator that never frees (`BUMP=1`:
//! the cheapest allocator there can be: an add and a compare). Same binary, so the two differ in nothing else.
//!
//!   BUMP=0 target/dist/examples/bump_x; BUMP=1 target/dist/examples/bump_x
#[path = "../scenes/cells.rs"]
mod cells;
use bough_mem::pmu;
use bough_x::prelude::*;
use bough_x::soft::Soft;
use cells::sheet::*;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

static BUMP: AtomicBool = AtomicBool::new(false);
static TOP: AtomicUsize = AtomicUsize::new(0);
static END: AtomicUsize = AtomicUsize::new(0);
struct Either;
// SAFETY: forwards to the system allocator, or hands out disjoint, aligned pieces of one mapping it never
// takes back (a piece freed while bumping is leaked; a piece from the system allocator is never freed while
// bumping because the switch is thrown once, before anything measured is allocated, and never back).
unsafe impl GlobalAlloc for Either {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        if !BUMP.load(Ordering::Relaxed) {
            return unsafe { System.alloc(l) };
        }
        let at = (TOP.load(Ordering::Relaxed) + l.align() - 1) & !(l.align() - 1);
        if at + l.size() > END.load(Ordering::Relaxed) {
            return core::ptr::null_mut();
        }
        TOP.store(at + l.size(), Ordering::Relaxed);
        at as *mut u8
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        let (a, lo) = (p as usize, END.load(Ordering::Relaxed).wrapping_sub(1 << 30));
        if !(BUMP.load(Ordering::Relaxed) && a >= lo && a < lo + (1 << 30)) {
            unsafe { System.dealloc(p, l) }
        }
    }
}
#[global_allocator]
static A: Either = Either;

fn main() {
    let bump = std::env::var("BUMP").is_ok_and(|v| v == "1");
    let rows: usize = std::env::var("ROWS").ok().and_then(|v| v.parse().ok()).unwrap_or(10_000);
    let sheet = Sheet::new(rows);
    let mut out = Vec::with_capacity(256);
    if bump {
        // one gigabyte of address space, touched as it is used; pre-touch the first megabyte so that the bump
        // allocator's own page faults are not what is measured
        let l = Layout::from_size_align(1 << 30, 4096).unwrap();
        // SAFETY: a non-zero layout; the result is checked.
        let p = unsafe { System.alloc(l) };
        assert!(!p.is_null());
        // SAFETY: inside the allocation.
        unsafe { core::ptr::write_bytes(p, 0, 1 << 20) };
        TOP.store(p as usize, Ordering::Relaxed);
        END.store(p as usize + (1 << 30), Ordering::Relaxed);
        BUMP.store(true, Ordering::Relaxed);
    }
    for _ in 0..200 {
        let a = pmu::sample();
        let mut w = World::<Soft>::new(W, H, NWORDS);
        w.mount(&sheet);
        let d = pmu::sample().since(&a);
        std::hint::black_box(&w);
        out.push((d.v[0], d.v[1]));
        // dropped outside the bracket
        drop(w);
    }
    out.sort();
    println!("bump={}\tworld_new+mount\tinstructions_p50\t{}\tcycles_p50\t{}", bump as u8, out[100].0, out[100].1);
}
