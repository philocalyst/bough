//! The façade's variable-length state: one arena of 32-bit words per window (`Spans`), carved from the
//! reserved block (`rt::StoreCaps`), so nothing an element owns is a heap object of its own (round 6, lane
//! `store`; bbtree: type-stable power-of-two storage that never goes back to the allocator).
//!
//! A span is `1 << class` words at a word offset. Freed spans go on a per-class list threaded through their
//! own first word (no side table); a class with no free span takes fresh words from the end of the arena,
//! which only grows. What lives here: a bar series' samples, a dynamic item's tape, a text's line windows
//! (widths and byte spans), the release lists of mounted subtrees.
use bough_mem::Col;

/// Offset of "no span".
pub const NO_SPAN: u32 = 0;

pub struct Spans {
    w: Col<u32>,
    /// Head of each class's free list (a word offset; 0: empty). Word 0 is never handed out.
    free: [u32; 32],
}

/// The class (log2 of the word count) that holds `n` words.
#[inline(always)]
pub fn class_of(n: usize) -> u8 {
    n.max(1).next_power_of_two().trailing_zeros() as u8
}

impl Spans {
    /// Over a column of `cap` words (zeroed, never moved).
    pub fn new(mut w: Col<u32>) -> Spans {
        w.push(0); // offset 0 is `NO_SPAN`
        Spans { w, free: [0; 32] }
    }

    /// A span of `1 << class` words; its contents are unspecified (a reused span holds old words).
    #[inline]
    pub fn alloc(&mut self, class: u8) -> u32 {
        let c = class as usize;
        let head = self.free[c];
        if head != 0 {
            self.free[c] = self.w[head as usize];
            return head;
        }
        let at = self.w.len();
        // SAFETY: all-zero is a valid `u32`; a full carved column panics with its name inside `grow_zeroed`.
        unsafe { self.w.grow_zeroed(at + (1usize << c)) };
        at as u32
    }

    /// Return the span of `1 << class` words at `off`.
    #[inline]
    pub fn free(&mut self, off: u32, class: u8) {
        if off == NO_SPAN {
            return;
        }
        debug_assert!(off as usize + (1usize << class) <= self.w.len());
        let c = class as usize;
        self.w[off as usize] = self.free[c];
        self.free[c] = off;
    }

    #[inline(always)]
    pub fn words(&self, off: u32, n: usize) -> &[u32] {
        &self.w[off as usize..off as usize + n]
    }
    #[inline(always)]
    pub fn words_mut(&mut self, off: u32, n: usize) -> &mut [u32] {
        &mut self.w[off as usize..off as usize + n]
    }
    /// The span as `f32`s (a bar series): the same bits, reinterpreted.
    #[inline(always)]
    pub fn floats(&self, off: u32, n: usize) -> &[f32] {
        let w = self.words(off, n);
        // SAFETY: `u32` and `f32` have the same size and alignment and every bit pattern is valid for both.
        unsafe { std::slice::from_raw_parts(w.as_ptr() as *const f32, n) }
    }
    #[inline(always)]
    pub fn floats_mut(&mut self, off: u32, n: usize) -> &mut [f32] {
        let w = self.words_mut(off, n);
        // SAFETY: as `floats`; the borrow is unique.
        unsafe { std::slice::from_raw_parts_mut(w.as_mut_ptr() as *mut f32, n) }
    }
    /// A copy on its own heap storage (test support: a cold copy of the layout engine).
    pub fn clone_owned(&self) -> Spans {
        let mut w = Col::owned("spans copy");
        w.extend_from_slice(&self.w);
        Spans { w, free: self.free }
    }
    /// Words handed out so far (the arena's high-water mark), for the ledger.
    /// The word column the spans are carved from (memory ledger).
    pub fn column(&self) -> &Col<u32> {
        &self.w
    }
    pub fn touched(&self) -> usize {
        self.w.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A span comes back from its class's list before fresh words are taken, classes do not mix, and
    /// live spans never overlap (a scalar model of the offsets handed out).
    #[test]
    fn spans_reuse_by_class_and_never_overlap() {
        let mut s = Spans::new(Col::owned("test spans"));
        let mut live: Vec<(u32, u8)> = Vec::new();
        let mut x = 0x9e37_79b9u32;
        for step in 0..4000 {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            if x % 3 != 0 || live.is_empty() {
                let class = (x >> 8) as u8 % 6;
                let off = s.alloc(class);
                assert!(off != NO_SPAN);
                let n = 1usize << class;
                for &(o, c) in &live {
                    let (a0, a1, b0, b1) = (off as usize, off as usize + n, o as usize, o as usize + (1usize << c));
                    assert!(a1 <= b0 || b1 <= a0, "step {step}: span {off}+{n} overlaps {o}+{}", 1usize << c);
                }
                s.words_mut(off, n).fill(step as u32);
                live.push((off, class));
            } else {
                let (off, class) = live.swap_remove(x as usize % live.len());
                let touched = s.touched();
                s.free(off, class);
                // the next span of that class is the one just freed, and takes no fresh words
                let again = s.alloc(class);
                assert_eq!(again, off);
                assert_eq!(s.touched(), touched);
                s.free(again, class);
            }
        }
    }

    #[test]
    fn floats_view_the_same_words() {
        let mut s = Spans::new(Col::owned("test spans"));
        let off = s.alloc(class_of(5));
        s.floats_mut(off, 5).copy_from_slice(&[1.0, -0.0, 2.5, f32::MAX, 0.0]);
        assert_eq!(s.words(off, 5)[1], (-0.0f32).to_bits());
        assert_eq!(s.floats(off, 5)[2], 2.5);
    }
}
