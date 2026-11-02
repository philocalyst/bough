//! Pair values by table instead of by probe (round 3, input agent: the cold first frame).
//!
//! `Builder::pair_value` answers one pair at a time: it walks the lookup list, and for each pair
//! subtable binary-searches the first glyph's coverage, its pair set or both class definitions. The
//! build asks it for every pair of covered codepoints under two lookup lists, ~72 K probes, which were
//! ~110 M instructions of a ~170 M cold start. The representation change: evaluate each subtable once
//! per glyph (coverage index, first class, second class, the pair set) and combine per pair over
//! small arrays, so a pair costs a few loads instead of a lookup walk. Same answers, same order of
//! precedence: within a lookup the first subtable that applies decides; across lookups the
//! adjustments add; an unsupported value record makes the pair a trigger (`None`).
use std::vec;
use super::*;

pub(super) struct PairTable {
    n: usize,
    v: Vec<Option<i32>>,
}

impl PairTable {
    /// Pair values for every (first, second) in `gids` x `gids` under `lookups`, exactly as
    /// `Builder::pair_value_g` would return them.
    pub(super) fn new(face: &Face, lookups: &[u16], gids: &[u16]) -> PairTable {
        let n = gids.len();
        let Some(t) = face.tables().gpos else {
            return PairTable { n, v: vec![None; n * n] };
        };
        let mut acc: Vec<Option<i32>> = vec![Some(0); n * n];
        let mut decided = vec![false; n * n];
        let apply = |acc: &mut Vec<Option<i32>>, k: usize, v1: &ValueRecord, v2: &ValueRecord| {
            let r = if !value_is_zero(face, v2) { None } else { value_is_plain_xadv(face, v1) };
            acc[k] = match (acc[k], r) {
                (Some(s), Some(x)) => Some(s + x),
                _ => None,
            };
        };
        let mut c2 = vec![0u16; n];
        for &li in lookups {
            let l = t.lookups.get(li).unwrap();
            if !matches!(l.subtables.get::<PositioningSubtable>(0), Some(PositioningSubtable::Pair(_))) {
                continue;
            }
            if flags_conservative(l.flags) {
                continue;
            }
            decided.fill(false);
            for si in 0..l.subtables.len() {
                let Some(PositioningSubtable::Pair(p)) = l.subtables.get::<PositioningSubtable>(si) else {
                    continue;
                };
                match p {
                    PairAdjustment::Format1 { coverage, sets } => {
                        for ia in 0..n {
                            let Some(ci) = coverage.get(GlyphId(gids[ia])) else { continue };
                            let Some(set) = sets.get(ci) else { continue };
                            for ib in 0..n {
                                let k = ia * n + ib;
                                if decided[k] {
                                    continue;
                                }
                                if let Some((v1, v2)) = set.get(GlyphId(gids[ib])) {
                                    decided[k] = true;
                                    apply(&mut acc, k, &v1, &v2);
                                }
                            }
                        }
                    }
                    PairAdjustment::Format2 { coverage, classes, matrix } => {
                        for (ib, c) in c2.iter_mut().enumerate() {
                            *c = classes.1.get(GlyphId(gids[ib]));
                        }
                        for ia in 0..n {
                            let ga = GlyphId(gids[ia]);
                            if !coverage.contains(ga) {
                                continue;
                            }
                            let c1 = classes.0.get(ga);
                            for ib in 0..n {
                                let k = ia * n + ib;
                                if decided[k] {
                                    continue;
                                }
                                if let Some((v1, v2)) = matrix.get((c1, c2[ib])) {
                                    decided[k] = true;
                                    apply(&mut acc, k, &v1, &v2);
                                }
                            }
                        }
                    }
                }
            }
        }
        PairTable { n, v: acc }
    }

    #[inline]
    pub(super) fn get(&self, ia: usize, ib: usize) -> Option<i32> {
        self.v[ia * self.n + ib]
    }
}
