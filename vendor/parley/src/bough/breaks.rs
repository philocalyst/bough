// Copyright 2026 the bough authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Graft 3b: exact UAX #14 (line) and UAX #29 (word) boundaries for covered Latin-1 text, by walking ICU4X's own
//! rule tables (`RuleBreakData`, the baked data parley's segmenters use) over a 256-entry property map.
//!
//! ICU4X's iterators are table walks: a `(left, right)` property pair gives Break / Keep / NoMatch, or a state that
//! looks ahead (`Index` / `Intermediate`, the context rules such as UAX #14 LB25's number sequences). For Latin-1
//! text their other branches cannot run: no Latin-1 character outside the C0/C1 controls has the CM or ZWJ line
//! property (LB9/LB8a), none is SA (complex scripts), parley's options are strictness Strict, word-break Normal and
//! no Japanese/Chinese tailoring (so neither `is_break_utf32_by_normal` nor the loose or keep-all branches apply),
//! and the word segmenter has no locale override. What remains is transcribed below, decision for decision, from
//! `line/v1.rs` and `rule_segmenter_v1.rs` (icu_segmenter 2.4). Gate: `tests/parley_twin.rs` against upstream
//! parley's segmenters (exhaustive pairs and short strings over every break class, corpora, random strings).

use std::prelude::rust_2024::*;

use icu_segmenter::provider::{Baked, BreakState, RuleBreakData};

/// One ICU rule machine over Latin-1.
pub(crate) struct Machine {
    /// Break property of each codepoint `0..=255`.
    pub(crate) prop: [u8; 256],
    /// `state[left * n + right]`, decoded once from the data's `ZeroVec`.
    state: Vec<BreakState>,
    n: usize,
    last_cp: u8,
    sot: u8,
    eot: u8,
}

impl Machine {
    fn new(data: &RuleBreakData<'_>) -> Self {
        let n = data.property_count as usize;
        // 256 rows (any `u8` left state) x n columns (every right property and sot/eot is < n), so a lookup is in
        // bounds without a check; rows past the data are Keep, as `get_break_state_from_table`'s fallback.
        let state = (0..256 * n)
            .map(|i| if i < n * n { data.break_state_table.get(i).unwrap_or(BreakState::Keep) } else { BreakState::Keep })
            .collect();
        let mut prop = [0u8; 256];
        for (cp, p) in prop.iter_mut().enumerate() {
            *p = data.property_table.get32(cp as u32);
        }
        Machine { prop, state, n, last_cp: data.last_codepoint_property, sot: data.sot_property, eot: data.eot_property }
    }

    #[inline(always)]
    fn st(&self, l: u8, r: u8) -> BreakState {
        debug_assert!((r as usize) < self.n);
        // SAFETY: `state` has 256 rows of `n` entries and `r < n` (a break property, sot or eot of the data).
        unsafe { *self.state.get_unchecked(l as usize * self.n + r as usize) }
    }

    /// UAX #14 over `cps` (one Latin-1 codepoint per character): calls `brk(i)` for every internal break position `i`
    /// (between characters `i - 1` and `i`, `0 < i < p.len()`), in order. `LineBreakIteratorV1::next` without the
    /// branches Latin-1 cannot reach.
    pub(crate) fn line(&self, cps: &[u8], mut brk: impl FnMut(usize)) {
        let n = cps.len();
        // SAFETY (both closures): callers index below `n = cps.len()`.
        let p = |i: usize| self.prop[unsafe { *cps.get_unchecked(i) } as usize];
        let mut i = 0;
        'a: loop {
            if i + 1 >= n {
                return;
            }
            let cur = i + 1;
            match self.st(p(i), p(cur)) {
                BreakState::Break | BreakState::NoMatch => {
                    brk(cur);
                    i = cur;
                }
                BreakState::Keep => i = cur,
                BreakState::Index(ix) | BreakState::Intermediate(ix) => {
                    let mut index = ix;
                    let mut prev = cur;
                    let mut pos = cur;
                    loop {
                        pos += 1;
                        let prev_is_cp = index <= self.last_cp;
                        if pos >= n {
                            if self.st(index, self.eot) == BreakState::NoMatch {
                                brk(prev);
                                i = prev;
                                continue 'a;
                            }
                            return;
                        }
                        match self.st(index, p(pos)) {
                            BreakState::Keep => {
                                i = pos;
                                continue 'a;
                            }
                            BreakState::NoMatch => {
                                brk(prev);
                                i = prev;
                                continue 'a;
                            }
                            BreakState::Break => {
                                brk(pos);
                                i = pos;
                                continue 'a;
                            }
                            BreakState::Intermediate(x) => {
                                index = x;
                                prev = pos;
                            }
                            BreakState::Index(x) => {
                                index = x;
                                if prev_is_cp {
                                    prev = pos;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// UAX #29 over `cps`: calls `brk(i)` for every boundary `i < p.len()` the word iterator reports (0 included
    /// when the start-of-text rule breaks), in order. `RuleBreakIterator::next` without the complex branches.
    pub(crate) fn word(&self, cps: &[u8], mut brk: impl FnMut(usize)) {
        let n = cps.len();
        // SAFETY (both closures): callers index below `n = cps.len()`.
        let p = |i: usize| self.prop[unsafe { *cps.get_unchecked(i) } as usize];
        if n == 0 {
            return;
        }
        if matches!(self.st(self.sot, p(0)), BreakState::Break | BreakState::NoMatch) {
            brk(0);
        }
        let mut i = 0;
        'a: loop {
            if i + 1 >= n {
                return;
            }
            let cur = i + 1;
            match self.st(p(i), p(cur)) {
                BreakState::Keep => i = cur,
                BreakState::Break | BreakState::NoMatch => {
                    brk(cur);
                    i = cur;
                }
                BreakState::Index(ix) | BreakState::Intermediate(ix) => {
                    let mut index = ix;
                    let mut prev = cur;
                    let mut pos = cur;
                    loop {
                        pos += 1;
                        if pos >= n {
                            if self.st(index, self.eot) == BreakState::NoMatch {
                                brk(prev);
                                i = prev;
                                continue 'a;
                            }
                            return;
                        }
                        let prev_is_cp = index <= self.last_cp;
                        match self.st(index, p(pos)) {
                            BreakState::Keep => {
                                i = pos;
                                continue 'a;
                            }
                            BreakState::NoMatch => {
                                brk(prev);
                                i = prev;
                                continue 'a;
                            }
                            BreakState::Break => {
                                brk(pos);
                                i = pos;
                                continue 'a;
                            }
                            BreakState::Intermediate(x) => {
                                index = x;
                                prev = pos;
                            }
                            BreakState::Index(x) => {
                                index = x;
                                if prev_is_cp {
                                    prev = pos;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

pub(crate) struct Machines {
    pub(crate) line: Machine,
    pub(crate) word: Machine,
}

/// The two machines, built once from the baked data.
pub(crate) fn machines() -> &'static Machines {
    static M: std::sync::OnceLock<Machines> = std::sync::OnceLock::new();
    M.get_or_init(|| Machines {
        line: Machine::new(Baked::SINGLETON_SEGMENTER_BREAK_LINE_V1),
        word: Machine::new(Baked::SINGLETON_SEGMENTER_BREAK_WORD_V1),
    })
}

/// Boundaries of a covered Latin-1 text, one per character, as parley's `analyze_text` assigns them: 2 = line
/// (an internal line break before the character), 1 = word (a word boundary at the character), 0 = none.
/// `cps` is scratch for the per-character codepoints.
pub(crate) fn boundaries(text: &[u8], out: &mut Vec<u8>, cps: &mut Vec<u8>) {
    decode(text, cps);
    boundaries_cps(cps, out);
}

/// The Latin-1 codepoints of a covered text, one per character.
#[inline]
pub(crate) fn decode(text: &[u8], cps: &mut Vec<u8>) {
    cps.clear();
    cps.reserve(text.len());
    let mut k = 0;
    while k < text.len() {
        let (cp, len) = match text[k] {
            c if c < 0x80 => (c, 1),
            0xC2 => (text[k + 1], 2),
            _ => (text[k + 1] + 0x40, 2),
        };
        cps.push(cp);
        k += len;
    }
}

/// `boundaries` over already decoded codepoints.
pub(crate) fn boundaries_cps(cps: &[u8], out: &mut Vec<u8>) {
    let m = machines();
    out.clear();
    out.resize(cps.len(), 0);
    m.word.word(cps, |i| out[i] = 1);
    m.line.line(cps, |i| out[i] = 2);
}
