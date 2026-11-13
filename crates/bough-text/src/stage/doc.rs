//! A document as blocks of lines: an edit touches one block, whatever the document's size.
//!
//! A block holds up to [`CAP`] lines: their bytes in one buffer and their start offsets in one lane. Above the
//! blocks are two lanes of prefix sums: lines before each block within its group of 64 blocks, and lines
//! before each group. Finding line `n` is one binary search in each (a line of memory each for the first two
//! levels of a 300,000-line document), then an index into the block's offsets. An edit adds a constant to the
//! entries after it in each lane: straight vector adds over contiguous words, no tree and no pointer chain.
//!
//! Storage only: this is what a `TextState` over a large document holds. Shaping an edited line is the
//! caller's (the line's own text is re-shaped; the memo serves the lines nobody is editing).

/// Lines per block before it splits.
pub const CAP: usize = 64;
/// Blocks per group.
const GROUP: usize = 64;

#[derive(Default, Clone)]
struct Blk {
    bytes: Vec<u8>,
    /// Start of each line, and the end of the last: `lines + 1` entries.
    off: Vec<u32>,
}
impl Blk {
    fn lines(&self) -> usize {
        self.off.len() - 1
    }
}

#[derive(Default)]
pub struct Doc {
    blocks: Vec<Blk>,
    /// Lines before each block, within its group.
    in_group: Vec<u32>,
    /// Lines before each group.
    before: Vec<u32>,
    lines: usize,
}

/// Where a line is: its block and its index in the block.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct At {
    b: u32,
    k: u32,
}

impl Doc {
    pub fn from_lines<'a>(lines: impl Iterator<Item = &'a str>) -> Doc {
        let mut d = Doc::default();
        let fresh = || Blk { bytes: Vec::new(), off: vec![0] };
        let mut cur = fresh();
        // blocks start three quarters full, so the first edits do not split
        let fill = CAP * 3 / 4;
        for l in lines {
            cur.bytes.extend_from_slice(l.as_bytes());
            cur.off.push(cur.bytes.len() as u32);
            d.lines += 1;
            if cur.lines() == fill {
                d.blocks.push(core::mem::replace(&mut cur, fresh()));
            }
        }
        if cur.lines() > 0 || d.blocks.is_empty() {
            if cur.lines() == 0 {
                cur.off.push(0);
                d.lines += 1;
            }
            d.blocks.push(cur);
        }
        d.resum();
        d
    }
    /// Rebuild both prefix lanes (after blocks were inserted or removed: rare).
    #[cold]
    fn resum(&mut self) {
        self.in_group.clear();
        self.before.clear();
        let mut total = 0u32;
        for g in self.blocks.chunks(GROUP) {
            self.before.push(total);
            let mut acc = 0u32;
            for b in g {
                self.in_group.push(acc);
                acc += b.lines() as u32;
            }
            total += acc;
        }
    }
    pub fn len(&self) -> usize {
        self.lines
    }
    pub fn is_empty(&self) -> bool {
        self.lines == 0
    }

    /// Where line `n` is. A caret keeps this between keystrokes; it stays valid until a line is added or
    /// removed before it.
    #[inline]
    pub fn at(&self, n: usize) -> At {
        assert!(n < self.lines);
        let n = n as u32;
        let g = self.before.partition_point(|&v| v <= n) - 1;
        let r = n - self.before[g];
        let grp = &self.in_group[g * GROUP..((g + 1) * GROUP).min(self.in_group.len())];
        let i = grp.partition_point(|&v| v <= r) - 1;
        At { b: (g * GROUP + i) as u32, k: r - grp[i] }
    }

    #[inline]
    pub fn line(&self, n: usize) -> &str {
        self.line_at(self.at(n))
    }
    #[inline]
    pub fn line_at(&self, a: At) -> &str {
        let blk = &self.blocks[a.b as usize];
        let (s, e) = (blk.off[a.k as usize] as usize, blk.off[a.k as usize + 1] as usize);
        // SAFETY: lines are stored whole and were `str`s; `splice_at` and `split` refuse (always, not only in
        // debug builds) an offset inside a character, and `join` concatenates whole lines.
        unsafe { core::str::from_utf8_unchecked(&blk.bytes[s..e]) }
    }

    /// Replace bytes `col..col + del` of line `n` with `ins` (no newline in `ins`).
    pub fn splice(&mut self, n: usize, col: usize, del: usize, ins: &str) {
        let a = self.at(n);
        self.splice_at(a, col, del, ins)
    }
    pub fn splice_at(&mut self, a: At, col: usize, del: usize, ins: &str) {
        assert!(!ins.contains('\n'), "a newline is a split, not a splice");
        let blk = &mut self.blocks[a.b as usize];
        let k = a.k as usize;
        let (s, e) = (blk.off[k] as usize, blk.off[k + 1] as usize);
        assert!(col + del <= e - s, "splice beyond the line");
        // Both ends on character boundaries, checked always: `line_at` hands these bytes out as a `str`
        // without looking again. The line is valid UTF-8 now (the invariant), so a byte starts a character
        // exactly when it is not a continuation byte.
        let starts = |i: usize| i == e - s || (blk.bytes[s + i] as i8) >= -0x40;
        assert!(starts(col) && starts(col + del), "splice inside a character");
        let at = s + col;
        blk.bytes.splice(at..at + del, ins.bytes());
        let d = (ins.len() as u32).wrapping_sub(del as u32);
        for o in &mut blk.off[k + 1..] {
            *o = o.wrapping_add(d);
        }
    }

    /// One more (`+1`) or one fewer (`-1`) line in block `b`: the prefix lanes after it.
    #[inline]
    fn bump(&mut self, b: usize, d: u32) {
        let g = b / GROUP;
        let end = ((g + 1) * GROUP).min(self.in_group.len());
        for v in &mut self.in_group[b + 1..end] {
            *v = v.wrapping_add(d);
        }
        for v in &mut self.before[g + 1..] {
            *v = v.wrapping_add(d);
        }
        self.lines = (self.lines as u32).wrapping_add(d) as usize;
    }

    /// Break line `n` at byte `col`: the rest becomes line `n + 1`.
    pub fn split(&mut self, n: usize, col: usize) {
        let a = self.at(n);
        let blk = &mut self.blocks[a.b as usize];
        let k = a.k as usize;
        let len = (blk.off[k + 1] - blk.off[k]) as usize;
        assert!(col <= len, "split beyond the line");
        assert!(col == len || (blk.bytes[blk.off[k] as usize + col] as i8) >= -0x40, "split inside a character");
        let cut = blk.off[k] + col as u32;
        blk.off.insert(k + 1, cut);
        self.bump(a.b as usize, 1);
        if self.blocks[a.b as usize].lines() > CAP {
            self.split_block(a.b as usize);
        }
    }

    /// Join line `n + 1` onto line `n`.
    pub fn join(&mut self, n: usize) {
        assert!(n + 1 < self.lines);
        let a = self.at(n);
        let (b, k) = (a.b as usize, a.k as usize);
        if k + 1 == self.blocks[b].lines() {
            // the next line starts the next block: its bytes move here, onto the end of this line
            let nb = b + 1;
            let l = self.blocks[nb].off[1] as usize;
            let moved: Vec<u8> = self.blocks[nb].bytes.drain(..l).collect();
            self.blocks[nb].off.remove(0);
            for o in &mut self.blocks[nb].off {
                *o -= l as u32;
            }
            self.blocks[b].bytes.extend_from_slice(&moved);
            *self.blocks[b].off.last_mut().unwrap() += l as u32;
            if self.blocks[nb].lines() == 0 {
                self.blocks.remove(nb);
            }
            self.lines -= 1;
            self.resum();
            return;
        }
        self.blocks[b].off.remove(k + 1);
        self.bump(b, 1u32.wrapping_neg());
    }

    #[cold]
    fn split_block(&mut self, b: usize) {
        let blk = &mut self.blocks[b];
        let half = blk.lines() / 2;
        let cut = blk.off[half];
        let mut tail = Blk { bytes: blk.bytes.split_off(cut as usize), off: blk.off.split_off(half) };
        for o in &mut tail.off {
            *o -= cut;
        }
        blk.off.push(cut);
        self.blocks.insert(b + 1, tail);
        // block indices after `b` moved: both lanes are rebuilt (one pass over the blocks; once per 16 or
        // more newlines in a block)
        self.resum();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The independent review's probes (T01): an edit inside a UTF-8 scalar is refused, not stored.
    #[test]
    fn edits_inside_a_character_are_refused() {
        let refused = |f: &dyn Fn(&mut Doc)| {
            let mut d = Doc::from_lines(["\u{e9}a", "\u{4e2d}\u{6587}"].into_iter());
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&mut d))).is_err()
        };
        assert!(refused(&|d| d.split(0, 1)));
        assert!(refused(&|d| d.splice(0, 0, 1, "")));
        assert!(refused(&|d| d.splice(0, 1, 1, "x")));
        assert!(refused(&|d| d.splice(1, 3, 2, "")));
        assert!(refused(&|d| d.splice(0, 0, 0, "a\nb")));
        assert!(refused(&|d| d.split(1, 4)));
        // and on boundaries they are accepted, with the text still what it should be
        let mut d = Doc::from_lines(["\u{e9}a", "\u{4e2d}\u{6587}"].into_iter());
        d.splice(0, 0, 2, "\u{fc}");
        d.split(1, 3);
        d.splice(2, 3, 0, "!");
        assert_eq!((d.line(0), d.line(1), d.line(2)), ("\u{fc}a", "\u{4e2d}", "\u{6587}!"));
    }

    /// Random edits against a `Vec<String>`.
    #[test]
    fn matches_a_vec_of_strings() {
        let mut model: Vec<String> = (0..500).map(|i| format!("line {i} of the document")).collect();
        let mut d = Doc::from_lines(model.iter().map(|s| s.as_str()));
        let mut r = 0x9E37_79B9u64;
        let mut next = |n: usize| {
            r ^= r << 13;
            r ^= r >> 7;
            r ^= r << 17;
            (r % n as u64) as usize
        };
        for step in 0..20_000 {
            let n = next(model.len());
            match next(10) {
                0..=5 => {
                    let col = next(model[n].len() + 1);
                    let del = next(model[n].len() - col + 1).min(3);
                    d.splice(n, col, del, "xy");
                    model[n].replace_range(col..col + del, "xy");
                }
                6..=7 => {
                    let col = next(model[n].len() + 1);
                    d.split(n, col);
                    let rest = model[n].split_off(col);
                    model.insert(n + 1, rest);
                }
                _ if n + 1 < model.len() && model.len() > 50 => {
                    d.join(n);
                    let rest = model.remove(n + 1);
                    model[n].push_str(&rest);
                }
                _ => {}
            }
            if step % 997 == 0 || step == 19_999 {
                assert_eq!(d.len(), model.len());
                for (i, l) in model.iter().enumerate() {
                    assert_eq!(d.line(i), l, "line {i} at step {step}");
                }
            }
        }
    }
}
