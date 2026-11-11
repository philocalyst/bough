//! Find-all over the document: every match of a query (plain text, ASCII case folded unless case counts), as
//! zgx's `state.text:find` / `:count` give them (successive, non-overlapping), capped at [`LIMIT`] for the count.
//!
//! The searches are kept (zgx keeps them too: a letter typed and taken back is answered from them). The
//! structural step past that: a query that extends a kept one matches only where the kept one occurs, so a
//! new letter reads the kept occurrences, not the text (the matches of `q + c` are the occurrences of `q`
//! followed by `c`). Only a query with no kept prefix reads the whole text: 16 bytes a step (`std::simd`),
//! anchored on the query's rarest letter. `Search::refine = false` turns the refinement off (every new query
//! reads the whole text, as zgx's does), for the comparison in the port's report (`FULLSCAN=1`).
use super::doc::Document;
use std::simd::cmp::SimdPartialEq;
use std::simd::Simd;

/// `search.LIMIT`: at most this many matches are counted ("10000+ matches" past it).
pub const LIMIT: usize = 10_000;
/// Searches kept (most recently used first).
const KEEP: usize = 32;

struct Kept {
    /// The query, folded.
    query: Vec<u8>,
    /// Every occurrence (overlapping), ascending: what a longer query refines.
    occ: Vec<u32>,
    /// The matches (successive, non-overlapping), ascending.
    hits: Vec<u32>,
}

pub struct Search {
    kept: Vec<Kept>,
    version: u64,
    kept_case: bool,
    /// Case counts (zgx's "Match Case").
    pub case: bool,
    /// Refine a kept prefix's occurrences (else every new query reads the whole text).
    pub refine: bool,
    /// Bytes of text read by whole-text scans (stats).
    pub scanned: u64,
}

impl Default for Search {
    fn default() -> Self {
        Search { kept: Vec::new(), version: u64::MAX, kept_case: false, case: false, refine: true, scanned: 0 }
    }
}

#[inline]
fn fold(b: u8, case: bool) -> u8 {
    if !case && b.is_ascii_uppercase() { b | 0x20 } else { b }
}

/// How rare a byte is in source text (higher: rarer): letters by English frequency, then the rest.
fn rarity(b: u8) -> u8 {
    const ORDER: &[u8] = b"etaoinsrhldcumfpgwybvkxjqz";
    match b {
        b'a'..=b'z' => ORDER.iter().position(|&c| c == b).unwrap_or(0) as u8,
        b' ' => 0,
        b'0'..=b'9' => 12,
        _ => 20,
    }
}

impl Search {
    /// The matches of `query` (all of them, ascending; count with [`Search::count`]).
    pub fn find(&mut self, doc: &Document, query: &[u8]) -> &[u32] {
        if doc.version != self.version || self.case != self.kept_case {
            self.kept.clear();
            self.version = doc.version;
            self.kept_case = self.case;
        }
        if query.is_empty() {
            return &[];
        }
        let case = self.case;
        let q: Vec<u8> = query.iter().map(|&b| fold(b, case)).collect();
        if let Some(i) = self.kept.iter().position(|k| k.query == q) {
            let k = self.kept.remove(i);
            self.kept.insert(0, k);
            return &self.kept[0].hits;
        }
        let base = if self.refine { self.kept.iter().filter(|k| q.starts_with(&k.query)).max_by_key(|k| k.query.len()) } else { None };
        let occ = match base {
            Some(k) => {
                let from = k.query.len();
                let n = doc.len();
                k.occ.iter().copied().filter(|&p| verify(doc, n, p as usize, &q, from, usize::MAX, case)).collect()
            }
            None => {
                self.scanned += doc.len() as u64;
                scan(doc, &q, case)
            }
        };
        let hits = if overlaps(&q) {
            let mut hits = Vec::with_capacity(occ.len());
            let mut end = 0usize;
            for &p in &occ {
                if p as usize >= end {
                    hits.push(p);
                    end = p as usize + q.len();
                }
            }
            hits
        } else {
            occ.clone()
        };
        self.kept.insert(0, Kept { query: q, occ, hits });
        self.kept.truncate(KEEP);
        &self.kept[0].hits
    }

    /// zgx's `count`: the matches (at most [`LIMIT`]) and whether there were more.
    pub fn count(hits: &[u32]) -> (usize, bool) {
        (hits.len().min(LIMIT), hits.len() > LIMIT)
    }
}

/// Can `q` overlap itself (a proper prefix equal to a suffix)? Then successive matches skip some occurrences.
fn overlaps(q: &[u8]) -> bool {
    (1..q.len()).any(|k| q[..k] == q[q.len() - k..])
}

/// Does `q` occur at `p` (bytes `from..` compared; `skip` is a position already known to match)?
#[inline]
fn verify(doc: &Document, n: usize, p: usize, q: &[u8], from: usize, skip: usize, case: bool) -> bool {
    if p + q.len() > n {
        return false;
    }
    (from..q.len()).all(|j| j == skip || fold(doc.byte(p + j), case) == q[j])
}

/// Every occurrence of `q` (folded) in the text: the anchor byte (the query's rarest) found 16 bytes a step
/// in each half of the gap buffer, each candidate verified (logically, so a match across the gap is found).
fn scan(doc: &Document, q: &[u8], case: bool) -> Vec<u32> {
    let j = (0..q.len()).max_by_key(|&j| (rarity(q[j]), usize::MAX - j)).unwrap_or(0);
    let anchor = q[j];
    let letter = !case && anchor.is_ascii_lowercase();
    let n = doc.len();
    let (a, b) = doc.halves();
    let mut out = Vec::new();
    for (half, base) in [(a, 0usize), (b, a.len())] {
        let check = |i: usize, out: &mut Vec<u32>| {
            let c = base + i;
            if c >= j && verify(doc, n, c - j, q, 0, j, case) {
                out.push((c - j) as u32);
            }
        };
        let want = Simd::<u8, 16>::splat(anchor);
        let fold_bit = Simd::<u8, 16>::splat(if letter { 0x20 } else { 0 });
        let mut chunks = half.chunks_exact(16);
        let mut at = 0usize;
        for ch in &mut chunks {
            let v = Simd::<u8, 16>::from_slice(ch) | fold_bit;
            let mut m = v.simd_eq(want).to_bitmask();
            while m != 0 {
                let k = m.trailing_zeros() as usize;
                check(at + k, &mut out);
                m &= m - 1;
            }
            at += 16;
        }
        for (k, &x) in chunks.remainder().iter().enumerate() {
            if fold(x, case) == anchor {
                check(at + k, &mut out);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn naive(t: &[u8], q: &[u8], case: bool) -> Vec<u32> {
        let f = |b: u8| fold(b, case);
        let mut out = Vec::new();
        let mut i = 0;
        while i + q.len() <= t.len() {
            if (0..q.len()).all(|k| f(t[i + k]) == f(q[k])) {
                out.push(i as u32);
                i += q.len();
            } else {
                i += 1;
            }
        }
        out
    }

    #[test]
    fn matches_equal_a_naive_search_through_edits() {
        let text = crate::code::source(300);
        let mut doc = Document::new(text.into_bytes());
        let mut s = Search::default();
        let queries: [&[u8]; 9] = [b"Overflo", b"Overflow", b"overflow(", b"o", b"oo", b"aa", b"aaa", b"comp", b"Error::"];
        let mut seed = 7u64;
        for round in 0..60 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let p = (seed >> 33) as usize % (doc.len() + 1);
            doc.insert(p, [&b"aa"[..], b"Overflow", b"x\n", b"OO"][round % 4]);
            if round % 7 == 3 {
                doc.delete(p / 2, p / 2 + 5);
            }
            for case in [false, true] {
                s.case = case;
                for refine in [true, false] {
                    s.refine = refine;
                    for q in queries {
                        let got = s.find(&doc, q).to_vec();
                        assert_eq!(got, naive(&doc.text(), q, case), "round {round} q {:?}", std::str::from_utf8(q));
                    }
                }
                // a letter at a time, each refined from the last
                s.refine = true;
                for k in 1..=9 {
                    let q = &b"Overflow("[..k];
                    assert_eq!(s.find(&doc, q).to_vec(), naive(&doc.text(), q, case));
                }
            }
        }
    }
}
