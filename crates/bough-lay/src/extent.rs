//! Where the rows of a list are, without visiting them.
//!
//! [`Uniform`]: every row has one height; positions are a multiply and a divide. [`Heights`]: a blocked prefix
//! sum over per-row heights, exact for rows that were measured; a row never measured counts as the mean of the
//! measured ones. `row_at` reads two chained cache lines (the block sums, then one block).

pub trait Extent {
    fn len(&self) -> u32;
    /// The top of `row` (`top(len)` is the total).
    fn top(&self, row: u32) -> i32;
    /// The row that covers `y` (clamped to the rows).
    fn row_at(&self, y: i32) -> u32;
    #[inline(always)]
    fn total(&self) -> i32 {
        self.top(self.len())
    }
    #[inline(always)]
    fn height(&self, row: u32) -> i32 {
        self.top(row + 1) - self.top(row)
    }
    /// A pixel offset as (row, offset inside the row): the anchor a scroll position is kept as.
    #[inline(always)]
    fn anchor(&self, y: i32) -> (u32, i32) {
        let r = self.row_at(y);
        (r, y - self.top(r))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Uniform {
    pub n: u32,
    pub h: i32,
}
impl Extent for Uniform {
    #[inline(always)]
    fn len(&self) -> u32 {
        self.n
    }
    #[inline(always)]
    fn top(&self, row: u32) -> i32 {
        row as i32 * self.h
    }
    #[inline(always)]
    fn row_at(&self, y: i32) -> u32 {
        (y.max(0) / self.h.max(1)).min(self.n.saturating_sub(1) as i32) as u32
    }
}

const B: usize = 64;
/// A height that was never measured.
const UNKNOWN: i32 = -1;

/// Per-row heights with their validity interval over the list's width.
pub struct Heights {
    /// Rows, in blocks of 64: `h[b][i]` is the height of row `b * 64 + i`, or `UNKNOWN`.
    h: Vec<[i32; B]>,
    /// The interval of list widths each measured height holds over.
    lo: Vec<[f32; B]>,
    hi: Vec<[f32; B]>,
    /// Per block: the top of its first row (a prefix sum with estimates for unknown rows), and how many of its
    /// rows are unknown, and the sum of its known heights.
    top: Vec<i32>,
    known_sum: Vec<i32>,
    unknown: Vec<u32>,
    /// Per block: the interval over which every measured row of it holds (`max lo`, `min hi`).
    blo: Vec<f32>,
    bhi: Vec<f32>,
    n: u32,
    /// What an unknown row counts as.
    est: i32,
    msum: i64,
    mcount: u32,
    /// Blocks from here on have stale `top`.
    stale: usize,
}

impl Heights {
    pub fn new(n: u32, est: i32) -> Heights {
        let nb = (n as usize).div_ceil(B).max(1);
        let mut e = Heights { h: vec![[UNKNOWN; B]; nb], lo: vec![[f32::NEG_INFINITY; B]; nb], hi: vec![[f32::INFINITY; B]; nb], top: vec![0; nb + 1], known_sum: vec![0; nb], unknown: vec![0; nb], blo: vec![f32::NEG_INFINITY; nb], bhi: vec![f32::INFINITY; nb], n, est: est.max(1), msum: 0, mcount: 0, stale: 0 };
        for b in 0..nb {
            e.unknown[b] = e.rows_in(b);
        }
        e.fix();
        e
    }
    #[inline(always)]
    fn rows_in(&self, b: usize) -> u32 {
        (self.n as usize - (b * B).min(self.n as usize)).min(B) as u32
    }
    /// How many rows were measured, and whether every row was: then the extent is exact.
    pub fn measured(&self) -> (u32, bool) {
        (self.mcount, self.mcount == self.n)
    }
    pub fn is_measured(&self, row: u32) -> bool {
        self.h[row as usize / B][row as usize % B] != UNKNOWN
    }
    /// Record the measured height of `row` and the interval of widths it holds over.
    #[inline]
    pub fn set(&mut self, row: u32, h: i32, lo: f32, hi: f32) {
        let (b, i) = (row as usize / B, row as usize % B);
        let old = self.h[b][i];
        if old == UNKNOWN {
            self.unknown[b] -= 1;
            self.mcount += 1;
            self.msum += h as i64;
            self.known_sum[b] += h;
        } else {
            self.msum += (h - old) as i64;
            self.known_sum[b] += h - old;
        }
        self.h[b][i] = h;
        self.lo[b][i] = lo;
        self.hi[b][i] = hi;
        self.blo[b] = self.blo[b].max(lo);
        self.bhi[b] = self.bhi[b].min(hi);
        if old != h {
            self.stale = self.stale.min(b + 1);
        }
    }
    /// Re-derive the estimate from the measured rows and bring the block tops up to date. Called once per
    /// pass, after the `set`s; `top` and `row_at` are exact for the measured rows after it.
    pub fn fix(&mut self) {
        let est = if self.mcount == 0 { self.est } else { ((self.msum + self.mcount as i64 / 2) / self.mcount as i64).max(1) as i32 };
        let from = if est != self.est { 0 } else { self.stale.min(self.h.len()) };
        self.est = est;
        let mut t = if from == 0 { 0 } else { self.top[from - 1] + self.known_sum[from - 1] + self.unknown[from - 1] as i32 * est };
        for b in from..self.h.len() {
            self.top[b] = t;
            t += self.known_sum[b] + self.unknown[b] as i32 * est;
        }
        let nb = self.h.len();
        self.top[nb] = t;
        self.stale = usize::MAX;
    }
    #[inline(always)]
    fn at(&self, b: usize, i: usize) -> i32 {
        let h = self.h[b][i];
        if h == UNKNOWN { self.est } else { h }
    }
    /// The measured rows whose interval does not hold at `width`, block by block: one compare per block of 64
    /// rows whose rows all hold.
    pub fn invalid(&mut self, width: f32, mut f: impl FnMut(u32)) {
        for b in 0..self.h.len() {
            if width >= self.blo[b] && width <= self.bhi[b] {
                continue;
            }
            let (mut blo, mut bhi) = (f32::NEG_INFINITY, f32::INFINITY);
            for i in 0..self.rows_in(b) as usize {
                if self.h[b][i] == UNKNOWN {
                    continue;
                }
                if width < self.lo[b][i] || width > self.hi[b][i] {
                    f((b * B + i) as u32);
                } else {
                    blo = blo.max(self.lo[b][i]);
                    bhi = bhi.min(self.hi[b][i]);
                }
            }
            // rows reported are re-measured by the caller (`set` narrows the block's interval again)
            self.blo[b] = blo;
            self.bhi[b] = bhi;
        }
    }
    /// A row was inserted before `row` (its height unknown until measured).
    pub fn insert(&mut self, row: u32) {
        self.shift(row, true);
    }
    pub fn remove(&mut self, row: u32) {
        self.shift(row, false);
    }
    /// Rows from `row` on move by one. Cost: the rows after `row`, as plain memory moves of three lanes; an
    /// append or a removal at the end touches one block.
    fn shift(&mut self, row: u32, ins: bool) {
        let n = self.n as usize;
        let mut flat = |v: &mut Vec<[i32; B]>, fill: i32| {
            let s = v.as_flattened_mut();
            if ins {
                s.copy_within(row as usize..n, row as usize + 1);
                s[row as usize] = fill;
            } else {
                s.copy_within(row as usize + 1..n, row as usize);
                s[n - 1] = fill;
            }
        };
        if ins && n % B == 0 && n / B == self.h.len() {
            self.h.push([UNKNOWN; B]);
            self.lo.push([f32::NEG_INFINITY; B]);
            self.hi.push([f32::INFINITY; B]);
            self.top.push(0);
            self.known_sum.push(0);
            self.unknown.push(0);
            self.blo.push(f32::NEG_INFINITY);
            self.bhi.push(f32::INFINITY);
        }
        let old = self.h[row as usize / B][row as usize % B];
        if !ins && old != UNKNOWN {
            self.mcount -= 1;
            self.msum -= old as i64;
        }
        flat(&mut self.h, UNKNOWN);
        let mut flatf = |v: &mut Vec<[f32; B]>, fill: f32| {
            let s = v.as_flattened_mut();
            if ins {
                s.copy_within(row as usize..n, row as usize + 1);
                s[row as usize] = fill;
            } else {
                s.copy_within(row as usize + 1..n, row as usize);
                s[n - 1] = fill;
            }
        };
        flatf(&mut self.lo, f32::NEG_INFINITY);
        flatf(&mut self.hi, f32::INFINITY);
        self.n = if ins { self.n + 1 } else { self.n - 1 };
        for b in row as usize / B..self.h.len() {
            let (mut ks, mut un, mut blo, mut bhi) = (0, 0, f32::NEG_INFINITY, f32::INFINITY);
            for i in 0..self.rows_in(b) as usize {
                if self.h[b][i] == UNKNOWN {
                    un += 1;
                } else {
                    ks += self.h[b][i];
                    blo = blo.max(self.lo[b][i]);
                    bhi = bhi.min(self.hi[b][i]);
                }
            }
            (self.known_sum[b], self.unknown[b], self.blo[b], self.bhi[b]) = (ks, un, blo, bhi);
        }
        self.stale = self.stale.min(row as usize / B + 1);
        self.fix();
    }
}

impl Extent for Heights {
    #[inline(always)]
    fn len(&self) -> u32 {
        self.n
    }
    #[inline]
    fn top(&self, row: u32) -> i32 {
        debug_assert!(self.stale == usize::MAX, "fix() after set()");
        let (b, i) = (row as usize / B, row as usize % B);
        if b >= self.h.len() {
            return self.top[self.h.len()];
        }
        let mut t = self.top[b];
        for k in 0..i {
            t += self.at(b, k);
        }
        t
    }
    #[inline]
    fn row_at(&self, y: i32) -> u32 {
        debug_assert!(self.stale == usize::MAX, "fix() after set()");
        // the block: the last whose top is at or before y (a binary search over one array)
        let nb = self.h.len();
        let b = self.top[..nb].partition_point(|&t| t <= y).saturating_sub(1);
        let mut t = self.top[b];
        let rows = self.rows_in(b) as usize;
        for i in 0..rows {
            let h = self.at(b, i);
            if y < t + h {
                return (b * B + i) as u32;
            }
            t += h;
        }
        ((b * B + rows) as u32).saturating_sub(1).min(self.n.saturating_sub(1))
    }
}
