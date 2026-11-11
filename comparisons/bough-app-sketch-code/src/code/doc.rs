//! The code editor's text: a gap buffer with a line index.
//!
//! The representation folds the gap into the index: a line start `s` (a logical byte offset) is stored as
//! `s` while it is at or before the gap and as `s + gap` after it, so typing at the gap moves no byte and
//! touches no line start (the value already is where the next logical position will be). Moving the gap
//! rewrites only the starts between its old and new place (two binary searches bound them), never the
//! whole index.
pub struct Document {
    buf: Vec<u8>,
    g0: usize,
    g1: usize,
    /// Folded line starts (see the module docs); `starts[0] == 0`.
    starts: Vec<u32>,
    /// Bumped by every edit (searches kept are for one version).
    pub version: u64,
}

/// Gap bytes added when the gap runs out.
const GROW: usize = 64 * 1024;

impl Document {
    pub fn new(text: Vec<u8>) -> Document {
        let n = text.len();
        let mut buf = text;
        let mut starts = Vec::with_capacity(n / 32 + 1);
        starts.push(0u32);
        for (i, &b) in buf.iter().enumerate() {
            if b == b'\n' {
                starts.push(i as u32 + 1);
            }
        }
        buf.resize(n + GROW, 0);
        // the gap at the end: every start is at or before it
        Document { buf, g0: n, g1: n + GROW, starts, version: 0 }
    }

    #[inline]
    fn gap(&self) -> usize {
        self.g1 - self.g0
    }
    /// Logical offset of a stored start.
    #[inline]
    fn logical(&self, b: u32) -> usize {
        let b = b as usize;
        if b <= self.g0 { b } else { b - self.gap() }
    }

    pub fn len(&self) -> usize {
        self.buf.len() - self.gap()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn lines(&self) -> usize {
        self.starts.len()
    }
    /// Logical start of line `i`.
    #[inline]
    pub fn line_start(&self, i: usize) -> usize {
        self.logical(self.starts[i])
    }
    /// Logical end of line `i` (its `\n`, or the end of the text).
    #[inline]
    pub fn line_end(&self, i: usize) -> usize {
        if i + 1 < self.starts.len() { self.line_start(i + 1) - 1 } else { self.len() }
    }
    /// The line holding logical offset `p`.
    pub fn line_of(&self, p: usize) -> usize {
        // stored starts are increasing in logical order too (the fold preserves order)
        let g0 = self.g0;
        let gap = self.gap();
        self.starts.partition_point(|&b| {
            let b = b as usize;
            (if b <= g0 { b } else { b - gap }) <= p
        }) - 1
    }
    #[inline]
    pub fn byte(&self, p: usize) -> u8 {
        self.buf[if p < self.g0 { p } else { p + self.gap() }]
    }
    /// The bytes before and after the gap: the text, in two pieces.
    pub fn halves(&self) -> (&[u8], &[u8]) {
        (&self.buf[..self.g0], &self.buf[self.g1..])
    }
    /// Logical bytes `a..b`, borrowed when the gap is not inside them, else copied into `scratch`.
    pub fn slice<'a>(&'a self, a: usize, b: usize, scratch: &'a mut Vec<u8>) -> &'a [u8] {
        if b <= self.g0 {
            &self.buf[a..b]
        } else if a >= self.g0 {
            let g = self.gap();
            &self.buf[a + g..b + g]
        } else {
            scratch.clear();
            scratch.extend_from_slice(&self.buf[a..self.g0]);
            scratch.extend_from_slice(&self.buf[self.g1..b + self.gap()]);
            scratch
        }
    }
    /// Line `i`'s bytes (no `\n`).
    pub fn line<'a>(&'a self, i: usize, scratch: &'a mut Vec<u8>) -> &'a [u8] {
        self.slice(self.line_start(i), self.line_end(i), scratch)
    }

    /// Move the gap to logical offset `p`: the bytes between move, and the starts between are refolded.
    pub fn move_gap(&mut self, p: usize) {
        let gap = self.gap();
        if p < self.g0 {
            // starts in (p, g0] now lie after the gap: + gap
            let (lo, hi) = (self.starts.partition_point(|&b| b as usize <= p), self.starts.partition_point(|&b| b as usize <= self.g0));
            for s in &mut self.starts[lo..hi] {
                *s += gap as u32;
            }
            let k = self.g0 - p;
            self.buf.copy_within(p..self.g0, self.g1 - k);
            self.g0 = p;
            self.g1 -= k;
        } else if p > self.g0 {
            // starts stored in (g1, p + gap] now lie before it: - gap
            let (lo, hi) = (self.starts.partition_point(|&b| b as usize <= self.g1), self.starts.partition_point(|&b| b as usize <= p + gap));
            for s in &mut self.starts[lo..hi] {
                *s -= gap as u32;
            }
            let k = p - self.g0;
            self.buf.copy_within(self.g1..self.g1 + k, self.g0);
            self.g0 = p;
            self.g1 += k;
        }
    }

    /// Insert `s` at logical offset `p`; returns the line it starts on.
    pub fn insert(&mut self, p: usize, s: &[u8]) -> usize {
        self.move_gap(p);
        if self.gap() < s.len() {
            let more = GROW.max(s.len());
            let at = self.starts.partition_point(|&b| b as usize <= self.g0);
            for b in &mut self.starts[at..] {
                *b += more as u32;
            }
            self.buf.splice(self.g1..self.g1, std::iter::repeat_n(0u8, more));
            self.g1 += more;
        }
        let line = self.line_of(p);
        self.buf[self.g0..self.g0 + s.len()].copy_from_slice(s);
        // a newline starts a line right after it (at or before the new gap end: stored as is)
        let mut at = line + 1;
        for (j, &b) in s.iter().enumerate() {
            if b == b'\n' {
                self.starts.insert(at, (p + j + 1) as u32);
                at += 1;
            }
        }
        self.g0 += s.len();
        self.version += 1;
        line
    }

    /// Delete logical bytes `a..b`.
    pub fn delete(&mut self, a: usize, b: usize) {
        if a >= b {
            return;
        }
        self.move_gap(b);
        // starts in (a, b] (stored as is: before the gap) go: their lines join the one before
        let (lo, hi) = (self.starts.partition_point(|&s| s as usize <= a), self.starts.partition_point(|&s| s as usize <= b));
        self.starts.drain(lo..hi);
        self.g0 = a;
        self.version += 1;
    }

    /// The whole text (tests).
    pub fn text(&self) -> Vec<u8> {
        let (x, y) = self.halves();
        [x, y].concat()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oracle_lines(t: &[u8]) -> Vec<usize> {
        let mut v = vec![0];
        v.extend(t.iter().enumerate().filter(|(_, b)| **b == b'\n').map(|(i, _)| i + 1));
        v
    }

    #[test]
    fn edits_match_a_vec_oracle() {
        let mut seed = 0x9e37_79b9u64;
        let mut rnd = |n: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % n as u64) as usize
        };
        let mut oracle: Vec<u8> = b"fn a() {\n    let x = 1;\n}\n\nlast".to_vec();
        let mut d = Document::new(oracle.clone());
        for step in 0..4000 {
            let p = rnd(oracle.len() + 1);
            if rnd(3) == 0 && !oracle.is_empty() {
                let b = (p + rnd(6)).min(oracle.len());
                oracle.drain(p..b);
                d.delete(p, b);
            } else {
                let s: &[u8] = [&b"x"[..], b"\n", b"ab\ncd", b"yy"][rnd(4)];
                oracle.splice(p..p, s.iter().copied());
                d.insert(p, s);
            }
            assert_eq!(d.text(), oracle, "step {step}");
            let starts = oracle_lines(&oracle);
            assert_eq!(d.lines(), starts.len(), "step {step}");
            let mut scratch = Vec::new();
            for (i, &s) in starts.iter().enumerate() {
                assert_eq!(d.line_start(i), s, "step {step} line {i}");
                let e = starts.get(i + 1).map_or(oracle.len(), |n| n - 1);
                assert_eq!(d.line(i, &mut scratch), &oracle[s..e], "step {step} line {i}");
            }
            for q in [0, p.min(oracle.len()), oracle.len()] {
                let want = starts.partition_point(|&s| s <= q) - 1;
                assert_eq!(d.line_of(q), want);
            }
        }
    }
}
