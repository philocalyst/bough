//! The prefix index of a list region: row pitches (device px) by list position, with prefix sums.
//!
//! # Representation: in-block prefix sums of 64-row blocks, a Fenwick tree over the blocks, rebuilt lazily
//! A virtual list asks three questions of its index: the offset of position `k` (`prefix`), the position at
//! offset `y` (`row_at`), and "row `k` was measured at `h`" (`set`). A list that can be spliced, moved and
//! permuted adds a fourth: the positions changed. A Fenwick tree over rows answers the first three in
//! O(log n) but has no splice (every entry after the splice point changes meaning). Three observations
//! change the domain:
//!
//! * **the heights belong to the items, not to the positions.** The region keeps the measured height *per
//!   slot* (`LRegion::hs`, the List's stable identity); the list's own order vector already says which slot is
//!   at which position after any splice, move or permutation. The positional index is a *derived* cache:
//!   after a turn's log is replayed it is rebuilt from the lowest position the log touched (`assign_from`, a
//!   gather `hs[order[p]]`), once per turn whatever the number of ops. Nothing is shifted op by op; an append
//!   rebuilds only the appended rows and O(log) tree entries.
//! * **the tree only needs to see blocks.** The Fenwick tree is over the sums of 64-row blocks (100,000 rows:
//!   1,563 entries, 12 KB, in L1): `log2(n / 64)` steps for every query and update.
//! * **inside a block, store what the queries read: the running sum.** Position `k` holds the sum of its
//!   block's heights up to and including `k` (`ip`), so a prefix is the Fenwick walk plus one load, a block's
//!   sum is its last entry (no separate column), a height is one subtraction, and `row_at` inside a block is a
//!   binary search. A point update adds the change to the rest of its block (at most 63 contiguous i32: a few
//!   vector adds); updates are rare (a row measured), queries are every frame.
//!
//! Heights are integral device px (row tops are integral, so a row's space is exact).
const B: usize = 64;
const SHIFT: u32 = 6;
const MASK: usize = B - 1;

#[derive(Clone)]
pub struct HeightIndex {
    est: i32,
    /// In-block inclusive prefix sums by position: `ip[k] = Σ h[block_start(k)..=k]`.
    ip: Vec<i32>,
    /// Fenwick tree over the block sums, 1-based (`fen[0]` unused).
    fen: Vec<i64>,
    total: i64,
    /// Highest power of two <= number of blocks (the `row_at` descent's first step).
    top: usize,
    /// Running block prefixes of a rebuild (reused).
    scratch: Vec<i64>,
}

impl HeightIndex {
    /// Heap bytes (memory ledger).
    pub fn heap_bytes(&self) -> usize {
        self.ip.capacity() * 4 + self.fen.capacity() * 8
    }
    /// `n` rows of `est` each.
    pub fn new(n: usize, est: i32) -> HeightIndex {
        let mut x = HeightIndex { est, ip: Vec::new(), fen: vec![0], total: 0, top: 0, scratch: Vec::new() };
        x.assign_from(0, n, |_| est);
        x
    }
    pub fn len(&self) -> usize {
        self.ip.len()
    }
    pub fn is_empty(&self) -> bool {
        self.ip.is_empty()
    }
    pub fn estimate(&self) -> i32 {
        self.est
    }
    #[inline]
    pub fn height(&self, k: usize) -> i32 {
        if k & MASK == 0 { self.ip[k] } else { self.ip[k] - self.ip[k - 1] }
    }
    pub fn total(&self) -> i64 {
        self.total
    }
    /// The heights by position.
    pub fn heights(&self) -> Vec<i32> {
        (0..self.ip.len()).map(|k| self.height(k)).collect()
    }
    #[inline]
    fn bsum(&self, b: usize) -> i64 {
        self.ip[((b + 1) << SHIFT).min(self.ip.len()) - 1] as i64
    }
    /// Sum of the blocks `0..b` (Fenwick entries `<= b` only).
    #[inline]
    fn blocks(&self, b: usize) -> i64 {
        let (mut i, mut s) = (b, 0i64);
        while i > 0 {
            s += self.fen[i];
            i &= i - 1;
        }
        s
    }

    /// Positions `from..n` become `f(from)..f(n - 1)` (the length becomes `n`); positions before `from` keep
    /// their heights. Rescans the suffix's blocks; the Fenwick entries that cover them are rebuilt.
    pub fn assign_from(&mut self, from: usize, n: usize, f: impl Fn(usize) -> i32) {
        let from = from.min(self.ip.len()).min(n);
        let old_nb = self.fen.len() - 1;
        self.ip.truncate(from);
        let mut k = from;
        while k < n {
            let end = ((k | MASK) + 1).min(n);
            let mut acc = if k & MASK != 0 { self.ip[k - 1] } else { 0 };
            self.ip.extend((k..end).map(|j| {
                acc += f(j);
                acc
            }));
            k = end;
        }
        self.rebuild((from >> SHIFT).min(old_nb));
    }

    /// Insert `cnt` rows of height `v` at `at`.
    pub fn insert(&mut self, at: usize, cnt: usize, v: i32) {
        let n = self.ip.len();
        let mut raw: Vec<i32> = (at..n).map(|k| self.height(k)).collect();
        raw.splice(0..0, std::iter::repeat_n(v, cnt));
        self.assign_from(at, n + cnt, |k| raw[k - at]);
    }
    /// Remove rows `at..at + cnt`.
    pub fn remove(&mut self, at: usize, cnt: usize) {
        let n = self.ip.len();
        let raw: Vec<i32> = (at + cnt..n).map(|k| self.height(k)).collect();
        self.assign_from(at, n - cnt, |k| raw[k - at]);
    }

    /// The Fenwick entries covering blocks `b0..` (entries `<= b0` cover only earlier blocks and stay): entry
    /// `i` is `P(i) - P(i - lowbit(i))`, `P` the block prefix, from a running sum over the suffix (and, for the
    /// at most `log` entries reaching below `b0`, a walk over the kept entries).
    fn rebuild(&mut self, b0: usize) {
        let nb = self.ip.len().div_ceil(B);
        let b0 = b0.min(nb);
        self.fen.truncate(b0 + 1);
        let mut acc = self.blocks(b0);
        let mut pre = std::mem::take(&mut self.scratch);
        pre.clear();
        pre.push(acc);
        for b in b0..nb {
            acc += self.bsum(b);
            pre.push(acc);
        }
        for i in b0 + 1..=nb {
            let lo = i - (i & i.wrapping_neg());
            let plo = if lo >= b0 { pre[lo - b0] } else { self.blocks(lo) };
            self.fen.push(pre[i - b0] - plo);
        }
        self.scratch = pre;
        self.total = acc;
        self.top = if nb == 0 { 0 } else { 1usize << (usize::BITS - 1 - nb.leading_zeros()) };
    }

    /// Sum of the heights of positions `0..k`.
    #[inline]
    pub fn prefix(&self, k: usize) -> i64 {
        let k = k.min(self.ip.len());
        let s = self.blocks(k >> SHIFT);
        if k & MASK != 0 { s + self.ip[k - 1] as i64 } else { s }
    }

    /// Set position `k`'s height; returns the change.
    #[inline]
    pub fn set(&mut self, k: usize, v: i32) -> i32 {
        let d = v - self.height(k);
        if d != 0 {
            let end = ((k | MASK) + 1).min(self.ip.len());
            for x in &mut self.ip[k..end] {
                *x += d;
            }
            self.total += d as i64;
            let nb = self.fen.len() - 1;
            let mut i = (k >> SHIFT) + 1;
            while i <= nb {
                self.fen[i] += d as i64;
                i += i & i.wrapping_neg();
            }
        }
        d
    }

    /// The position containing offset `y`: the last `k` with `prefix(k) <= y` (the last row for `y` at or
    /// past the end; 0 for an empty index).
    #[inline]
    pub fn row_at(&self, y: i64) -> usize {
        let n = self.ip.len();
        if n == 0 {
            return 0;
        }
        let nb = self.fen.len() - 1;
        let (mut pos, mut rem) = (0usize, y.max(0));
        let mut pw = self.top;
        while pw > 0 {
            let next = pos + pw;
            if next <= nb && self.fen[next] <= rem {
                pos = next;
                rem -= self.fen[next];
            }
            pw >>= 1;
        }
        if pos >= nb {
            return n - 1;
        }
        // block `pos` (whose sum exceeds `rem`): the first row whose running sum exceeds `rem`
        let a = pos << SHIFT;
        let blk = &self.ip[a..(a + B).min(n)];
        (a + blk.partition_point(|&x| x as i64 <= rem)).min(n - 1)
    }
}
