//! Record-space damage (round 7): what changed this epoch, as bit-planes over records, one plane per class
//! of change. A write sets one bit; nothing is projected to the screen at the write. The screen-space tile
//! bitmap (`damage.rs`) is derived from the planes once, before a frame reads it, and every other consumer of
//! "what changed" (instance upload, layout, accessibility, held worlds) can read the same planes.
//!
//! The planes are indexed by **handle index**: a handle names its record for the record's whole life, wherever
//! structural edits move it, so a bit never has to be carried by an insert, a split or a merge. A plane is a
//! two-level bitset: `bits[w]` holds 64 records, `sum[w / 64]` says which words are non-zero. An idle epoch is
//! one zero summary word per 4,096 records; a pass over the planes visits only set bits (`drain`), and its
//! unit of work is a word of 64 records, so the same loop can be given a sub-range of words per core and the
//! results merged by OR.
pub const PAINT: usize = 0;
/// Classes reserved by the design (geometry with its old rect, text, media, structure); only `PAINT` has a
/// writer and a projection so far.
pub const NCLASS: usize = 1;

#[derive(Default)]
pub struct Dirty {
    bits: [Vec<u64>; NCLASS],
    sum: [Vec<u64>; NCLASS],
    /// A bit per class: some record of the class is marked (the idle test is this one byte).
    pending: u8,
}

impl Dirty {
    /// Make room for record indices below `n` (called where handles are made, never at a write).
    #[inline]
    pub fn reserve(&mut self, n: usize) {
        let words = n.div_ceil(64);
        if self.bits[0].len() < words {
            // grow by doubling: one summary word per 64 plane words
            let words = words.next_power_of_two().max(64);
            for c in 0..NCLASS {
                self.bits[c].resize(words, 0);
                self.sum[c].resize(words.div_ceil(64), 0);
            }
        }
    }
    /// Record `idx` changed in class `c`. `idx` is below the reserved count.
    #[inline(always)]
    pub fn mark(&mut self, c: usize, idx: u32) {
        let w = (idx >> 6) as usize;
        let old = self.bits[c][w];
        self.bits[c][w] = old | 1u64 << (idx & 63);
        if old == 0 {
            self.sum[c][w >> 6] |= 1u64 << (w & 63);
            self.pending |= 1 << c;
        }
    }
    #[inline(always)]
    pub fn any(&self, c: usize) -> bool {
        self.pending & (1 << c) != 0
    }
    /// Visit and clear every non-zero word of class `c`: `f(first record index of the word, mask)`.
    #[inline(always)]
    pub fn drain(&mut self, c: usize, mut f: impl FnMut(u32, u64)) {
        self.pending &= !(1 << c);
        for si in 0..self.sum[c].len() {
            let mut s = core::mem::take(&mut self.sum[c][si]);
            while s != 0 {
                let w = si * 64 + s.trailing_zeros() as usize;
                s &= s - 1;
                let m = core::mem::take(&mut self.bits[c][w]);
                f((w as u32) << 6, m);
            }
        }
    }
    pub fn heap_bytes(&self) -> usize {
        (0..NCLASS).map(|c| (self.bits[c].capacity() + self.sum[c].capacity()) * 8).sum()
    }
}
