//! bough-core: the Bough, one integrated display tree (see DESIGN.md sections 1, 2, 9).
//!
//! Records live in paint preorder inside 64-slot SoA pages drawn from a type-stable pool; each page
//! keeps 8 group summaries, a flat directory keeps SoA page summaries, and a chunk level summarises 64
//! pages: every spatial query is a chunk -> page -> group -> record SIMD filter. Damage is the damage
//! lab's 32 px tile plane (see RESULTS.md for the measured reformulations).
#![allow(clippy::too_many_arguments, clippy::needless_range_loop)]

#[macro_use]
mod macros {
    #[cfg(feature = "stats")]
    macro_rules! stat { ($i:expr, $v:expr) => { crate::stats::add($i, $v as u64) }; }
    #[cfg(not(feature = "stats"))]
    macro_rules! stat { ($i:expr, $v:expr) => {}; }
    /// `attr!(slot, expr)`: with feature `attr`, charge the instructions `expr` retires to `attr::C[slot]`
    /// (round 6 attribution of a structural edit); without it, just `expr`.
    #[cfg(feature = "attr")]
    macro_rules! attr { ($i:expr, $e:expr) => {{ let a = crate::attr::now(); let r = $e; crate::attr::add($i, a); r }}; }
    #[cfg(not(feature = "attr"))]
    macro_rules! attr { ($i:expr, $e:expr) => { $e }; }
}
pub mod arena;
pub mod bough;
pub mod damage;
pub mod dirty;
pub mod emit;
pub mod nrect;
pub mod oracle;
pub mod page;
pub mod prefix;
pub mod rng;
pub mod shapes;
pub mod state;

/// Cull work counters (feature `stats`): pages, groups, group none/all/partial, refined records.
#[cfg(feature = "stats")]
pub mod stats {
    use core::sync::atomic::{AtomicU64, Ordering};
    pub static C: [AtomicU64; 8] = [const { AtomicU64::new(0) }; 8];
    pub const NAMES: [&str; 8] = ["pages", "groups", "g_none", "g_all", "g_partial", "refined", "rows_g", "rows_r"];
    #[inline(always)]
    pub fn add(i: usize, v: u64) {
        C[i].fetch_add(v, Ordering::Relaxed);
    }
    pub fn take() -> [u64; 8] {
        core::array::from_fn(|i| C[i].swap(0, Ordering::Relaxed))
    }
}

/// Instruction attribution of the directory's maintenance (feature `attr`; analysis only): each slot
/// accumulates the retired instructions of one piece of a structural edit, the counter pair's own cost
/// (calibrated once) subtracted.
#[cfg(feature = "attr")]
pub mod attr {
    use core::sync::atomic::{AtomicU64, Ordering};
    pub const NAMES: [&str; 10] =
        ["pool.alloc", "d_id shift", "d_ptr shift", "d_ink shift", "d_min_depth shift", "d_len shift", "prefix grow/shrink", "reindex_pages", "prefix.invalidate_from", "fix_chunks"];
    pub static C: [AtomicU64; 10] = [const { AtomicU64::new(0) }; 10];
    pub static CALLS: [AtomicU64; 10] = [const { AtomicU64::new(0) }; 10];
    static OV: AtomicU64 = AtomicU64::new(u64::MAX);
    #[repr(C)]
    struct Rusage {
        uuid: [u8; 16],
        f: [u64; 29],
        instructions: u64,
        rest: [u64; 5],
    }
    unsafe extern "C" {
        fn proc_pid_rusage(pid: i32, flavor: i32, buffer: *mut Rusage) -> i32;
        fn getpid() -> i32;
    }
    #[inline(never)]
    pub fn now() -> u64 {
        let mut ri = Rusage { uuid: [0; 16], f: [0; 29], instructions: 0, rest: [0; 5] };
        // SAFETY: a correctly sized rusage_info_v4 (flavor 4).
        unsafe { proc_pid_rusage(getpid(), 4, &mut ri) };
        ri.instructions
    }
    fn overhead() -> u64 {
        let o = OV.load(Ordering::Relaxed);
        if o != u64::MAX {
            return o;
        }
        let best = (0..50).map(|_| { let a = now(); now() - a }).min().unwrap();
        OV.store(best, Ordering::Relaxed);
        best
    }
    #[inline(never)]
    pub fn add(i: usize, since: u64) {
        let d = now() - since;
        C[i].fetch_add(d.saturating_sub(overhead()), Ordering::Relaxed);
        CALLS[i].fetch_add(1, Ordering::Relaxed);
    }
    /// (instructions, calls) per slot since the last `take`.
    pub fn take() -> [(u64, u64); 10] {
        overhead();
        core::array::from_fn(|i| (C[i].swap(0, Ordering::Relaxed), CALLS[i].swap(0, Ordering::Relaxed)))
    }
}

pub use bough::{Bough, Handle, RecordInit};
pub use bough::hit::{KF_FOCUS, KF_INTERACT, KF_MASK, KF_NOHIT};
pub use damage::Plane;
pub use nrect::{NRect, WRect};

/// Non-inlined entry points so the hot loops can be read in `--emit asm` output.
#[doc(hidden)]
pub mod probe {
    use crate::{Bough, NRect, Plane, bough::Handle};
    use crate::nrect::{Thresh, lt_mask, union_all};
    #[inline(never)]
    pub fn prefix_invalidate_from(p: &mut crate::prefix::BlockedPrefix, d: usize, lens: &[u32]) {
        p.invalidate_from(d, lens)
    }
    #[inline(never)]
    pub fn lt_mask_64(p: &[NRect; 64], t: Thresh) -> u64 {
        unsafe { lt_mask(p.as_ptr(), 64, t) }
    }
    #[inline(never)]
    pub fn union_all_64(p: &[NRect; 64]) -> NRect {
        union_all(p)
    }
    #[inline(never)]
    pub fn subtree_end(b: &Bough, pos: usize) -> usize {
        b.subtree_end(pos)
    }
    #[inline(never)]
    pub fn parent(b: &Bough, pos: usize) -> Option<usize> {
        b.parent(pos)
    }
    #[inline(never)]
    pub fn hit(b: &Bough, x: i32, y: i32) -> Option<usize> {
        b.hit(x, y)
    }
    #[inline(never)]
    pub fn cull(b: &Bough, r: NRect, out: &mut Vec<u32>) {
        b.cull(r, None, out)
    }
    #[inline(never)]
    pub fn cull_tiles(b: &Bough, p: &Plane, bb: NRect, out: &mut Vec<u32>) {
        b.cull_tiles(p, bb, out)
    }
    #[inline(never)]
    pub fn lt8(p: &[NRect; 8], t: Thresh) -> u32 {
        unsafe { crate::nrect::lt8(p.as_ptr(), t) }
    }
    #[inline(never)]
    pub fn compact8(src: &[u32; 8], m: u32, out: &mut [u32; 8]) -> usize {
        unsafe { crate::emit::compact8(src.as_ptr(), m, out.as_mut_ptr()).offset_from(out.as_ptr()) as usize }
    }
    #[inline(never)]
    pub fn touch_handle(b: &mut Bough, h: Handle) -> bool {
        b.touch_handle(h)
    }
    #[inline(never)]
    pub fn translate_run(b: &mut Bough, s: usize, n: usize, dx: i32, dy: i32) {
        b.translate_run(s, n, dx, dy)
    }
    #[inline(never)]
    pub fn next_epoch(b: &mut Bough) {
        b.next_epoch()
    }
    #[inline(never)]
    pub fn damage_rect(b: &mut Bough, r: NRect) {
        b.damage_rect(r)
    }
    #[inline(never)]
    pub fn plane_add(p: &mut Plane, r: NRect, w: NRect) {
        p.add(r, w)
    }
    #[inline(never)]
    pub fn repair_into(b: &Bough, age: u32, p: &mut Plane) -> NRect {
        b.repair_into(age, p)
    }
}
