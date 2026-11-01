//! Page start offsets (prefix counts of directory page lengths).
//!
//! `start[i]` = number of records before directory page `i`; `start[n]` = total. Three
//! implementations, chosen at compile time and compared in RESULTS.md:
//!
//! * default, `BlockedPrefix`: two-level eager counts (block starts + block-local starts); a length
//!   change touches at most 32 + n/32 entries.
//! * feature `flat-prefix`, `FlatPrefix` eager: one array; a single-page length change adds the delta
//!   to the whole suffix with NEON; structural changes rebuild from the changed index immediately.
//! * feature `lazy-prefix`, `FlatPrefix` lazy: every change only lowers a watermark; the next lookup
//!   rebuilds `watermark..n`.

use core::cell::{Cell, UnsafeCell};

pub const LAZY: bool = cfg!(feature = "lazy-prefix");

#[cfg(any(feature = "flat-prefix", feature = "lazy-prefix"))]
pub type Prefix = FlatPrefix;
#[cfg(not(any(feature = "flat-prefix", feature = "lazy-prefix")))]
pub type Prefix = BlockedPrefix;
pub const NAME: &str = if cfg!(feature = "lazy-prefix") {
    "flat, lazy watermark"
} else if cfg!(feature = "flat-prefix") {
    "flat, eager NEON suffix add"
} else {
    "blocked (2-level), eager"
};


pub struct FlatPrefix {
    // Interior mutability so `&self` queries can lazily rebuild. Single-threaded by construction
    // (UnsafeCell makes Bough !Sync); no reference into the Vec escapes any method.
    start: UnsafeCell<Vec<u32>>,
    /// `start[0..=valid]` is correct.
    valid: Cell<usize>,
}

impl Default for FlatPrefix {
    fn default() -> Self {
        Self::new()
    }
}

impl FlatPrefix {
    pub fn new() -> Self {
        FlatPrefix { start: UnsafeCell::new(vec![0]), valid: Cell::new(0) }
    }

    #[inline(always)]
    fn v(&self) -> &mut Vec<u32> {
        // SAFETY: single-threaded; callers never hold the returned reference across another call.
        unsafe { &mut *self.start.get() }
    }

    /// Directory gained a page at the end of the array (length bookkeeping only).
    pub fn grow(&mut self) {
        self.v().push(0);
    }
    pub fn shrink(&mut self) {
        self.v().pop();
        let n = self.v().len() - 1;
        if self.valid.get() > n {
            self.valid.set(n);
        }
    }

    /// Lengths of pages `>= d` (or their number) changed arbitrarily.
    pub fn invalidate_from(&mut self, d: usize, lens: &[u32]) {
        if self.valid.get() > d {
            self.valid.set(d);
        }
        if !LAZY {
            self.ensure(lens);
        }
    }

    /// Page `d`'s length changed by `delta`; nothing else did.
    #[inline]
    pub fn delta(&mut self, d: usize, delta: i32, lens: &[u32]) {
        if LAZY {
            if self.valid.get() > d {
                self.valid.set(d);
            }
        } else {
            let v = self.v();
            add_suffix(&mut v[d + 1..], delta);
            debug_assert_eq!(self.valid.get(), lens.len());
        }
    }

    #[inline(always)]
    pub fn ensure(&self, lens: &[u32]) {
        if self.valid.get() < lens.len() {
            self.rebuild(lens);
        }
    }

    #[cold]
    #[inline(never)]
    fn rebuild(&self, lens: &[u32]) {
        let n = lens.len();
        let valid = self.valid.get();
        let v = self.v();
        assert!(v.len() == n + 1);
        let mut acc = v[valid];
        for i in valid..n {
            // SAFETY: i < n = lens.len() and i + 1 <= n < v.len() (asserted above).
            unsafe {
                acc += *lens.get_unchecked(i);
                *v.get_unchecked_mut(i + 1) = acc;
            }
        }
        self.valid.set(n);
    }

    /// Start offset of directory page `d` (`d <= n`).
    #[inline(always)]
    pub fn get(&self, d: usize, lens: &[u32]) -> usize {
        self.ensure(lens);
        self.v()[d] as usize
    }

    /// Directory page containing global position `pos` (`pos < total`) and that page's start.
    /// Branchless binary search over the start offsets (no bounds checks: `n >= 1`, `v[0] == 0`).
    #[inline(always)]
    pub fn locate(&self, pos: usize, lens: &[u32]) -> (usize, usize) {
        self.ensure(lens);
        let n = lens.len();
        let v = self.v();
        debug_assert!(n >= 1 && v.len() > n && v[0] == 0);
        let (mut lo, mut size) = (0usize, n);
        while size > 1 {
            let half = size / 2;
            let mid = lo + half;
            // SAFETY: mid < n <= v.len() - 1.
            let m = unsafe { *v.get_unchecked(mid) } as usize;
            lo = if m <= pos { mid } else { lo };
            size -= half;
        }
        (lo, unsafe { *v.get_unchecked(lo) } as usize)
    }

    pub fn heap_bytes(&self) -> usize {
        self.v().capacity() * 4
    }
    pub fn snapshot(&self, lens: &[u32]) -> Vec<u32> {
        self.ensure(lens);
        self.v().clone()
    }
}

#[inline(always)]
fn add_suffix(s: &mut [u32], delta: i32) {
    #[cfg(target_arch = "aarch64")]
    unsafe {
        use core::arch::aarch64::*;
        let dv = vdupq_n_u32(delta as u32);
        let n = s.len();
        let p = s.as_mut_ptr();
        let mut i = 0;
        while i + 4 <= n {
            vst1q_u32(p.add(i), vaddq_u32(vld1q_u32(p.add(i)), dv));
            i += 4;
        }
        while i < n {
            *p.add(i) = (*p.add(i)).wrapping_add(delta as u32);
            i += 1;
        }
    }
    #[cfg(not(target_arch = "aarch64"))]
    for x in s {
        *x = x.wrapping_add(delta as u32);
    }
}


/// Two-level prefix counts: per-block start offsets plus start offsets local to the block.
/// A length change touches at most `BLOCK` local entries and `n / BLOCK` block entries, instead of
/// the whole suffix; lookups are two short binary searches. Eager only.
pub struct BlockedPrefix {
    local: Vec<u32>,
    blk: Vec<u32>,
    n: usize,
}

const BLOCK: usize = 32;

impl Default for BlockedPrefix {
    fn default() -> Self {
        Self::new()
    }
}

impl BlockedPrefix {
    pub fn new() -> Self {
        BlockedPrefix { local: vec![0], blk: vec![0], n: 0 }
    }
    pub fn grow(&mut self) {
        self.n += 1;
        self.local.resize(self.n + 1, 0);
        self.blk.resize(self.n / BLOCK + 1, 0);
    }
    pub fn shrink(&mut self) {
        self.n -= 1;
        self.local.truncate(self.n + 1);
        self.blk.truncate(self.n / BLOCK + 1);
    }
    pub fn invalidate_from(&mut self, _d: usize, lens: &[u32]) {
        debug_assert_eq!(lens.len(), self.n);
        let mut acc = 0u32;
        for i in 0..=self.n {
            if i % BLOCK == 0 {
                self.blk[i / BLOCK] = acc;
            }
            self.local[i] = acc - self.blk[i / BLOCK];
            if i < self.n {
                acc += lens[i];
            }
        }
    }
    #[inline]
    pub fn delta(&mut self, d: usize, delta: i32, _lens: &[u32]) {
        let i0 = d + 1;
        let b0 = i0 / BLOCK;
        if i0 % BLOCK == 0 {
            add_suffix(&mut self.blk[b0..], delta);
        } else {
            let end = ((b0 + 1) * BLOCK).min(self.n + 1);
            add_suffix(&mut self.local[i0..end], delta);
            add_suffix(&mut self.blk[b0 + 1..], delta);
        }
    }
    #[inline(always)]
    pub fn ensure(&self, _lens: &[u32]) {}
    #[inline(always)]
    pub fn get(&self, d: usize, _lens: &[u32]) -> usize {
        unsafe { (*self.blk.get_unchecked(d / BLOCK) + *self.local.get_unchecked(d)) as usize }
    }
    #[inline(always)]
    pub fn locate(&self, pos: usize, _lens: &[u32]) -> (usize, usize) {
        let nb = self.blk.len();
        let (mut lo, mut size) = (0usize, nb);
        while size > 1 {
            let half = size / 2;
            let mid = lo + half;
            let m = unsafe { *self.blk.get_unchecked(mid) } as usize;
            lo = if m <= pos { mid } else { lo };
            size -= half;
        }
        let b = lo;
        let base = unsafe { *self.blk.get_unchecked(b) } as usize;
        let first = b * BLOCK;
        let last = (first + BLOCK).min(self.n); // exclusive; block holds pages first..last
        let rel = pos - base;
        let (mut lo, mut size) = (first, last - first);
        while size > 1 {
            let half = size / 2;
            let mid = lo + half;
            let m = unsafe { *self.local.get_unchecked(mid) } as usize;
            lo = if m <= rel { mid } else { lo };
            size -= half;
        }
        (lo, base + unsafe { *self.local.get_unchecked(lo) } as usize)
    }
    pub fn heap_bytes(&self) -> usize {
        (self.local.capacity() + self.blk.capacity()) * 4
    }
    pub fn snapshot(&self, lens: &[u32]) -> Vec<u32> {
        (0..=lens.len()).map(|d| self.get(d, lens) as u32).collect()
    }
}
