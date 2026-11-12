//! A text that grows (a streamed reply, a log line being written): an append costs the appended glyphs.
//!
//! The state is the shaped text in font units and its line starts at the current width; there is no break
//! table (`brk::block_from` reads the pens and the bytes). `set` is given the whole new text, as a plain
//! `String` field would give it: when the old text is a prefix (found by comparing) the rest is appended. An
//! append re-shapes from the start of the last word (pens before it cannot change: every pair step up to
//! there is already summed) and re-breaks from the start of the last line (greedy breaking runs left to
//! right, so earlier lines cannot change).
use super::brk;
use crate::shape::{GlyphRun, Shaper};

#[derive(Default)]
pub struct Flow {
    pub text: Vec<u8>,
    pub gid: Vec<u16>,
    pub x: Vec<i32>,
    pub width: i32,
    /// First glyph of each line at `w`.
    pub lines: Vec<u32>,
    pub w: i32,
    scratch: GlyphRun,
    /// Bytes shaped by the last append (what it cost).
    pub shaped: usize,
    /// Mutant for the gates: restart shaping at the appended bytes instead of the last word (the pair step
    /// across the join is lost). Never set outside a test.
    pub mutant_join: bool,
}

/// What `set` did.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Set {
    Same,
    Appended,
    Rebuilt,
    /// The table shaper refused the text: the caller queues it for the full shaper.
    Refused,
}

impl Flow {
    pub fn new(w: i32) -> Flow {
        Flow { w, ..Flow::default() }
    }

    /// The text is now `new`. ASCII without hard breaks in this form.
    pub fn set(&mut self, sh: &Shaper, new: &str) -> Set {
        let (old, nb) = (self.text.len(), new.as_bytes());
        if nb.len() >= old && nb[..old] == self.text[..] {
            if nb.len() == old {
                return Set::Same;
            }
            return if self.append(sh, &new[old..]) { Set::Appended } else { Set::Refused };
        }
        *self = Flow { w: self.w, scratch: core::mem::take(&mut self.scratch), mutant_join: self.mutant_join, ..Flow::default() };
        if self.append(sh, new) { Set::Rebuilt } else { Set::Refused }
    }

    /// Append `tail`. False: refused (the state is unchanged).
    pub fn append(&mut self, sh: &Shaper, tail: &str) -> bool {
        if !tail.is_ascii() || tail.contains('\n') {
            return false;
        }
        // restart at the last word: after the last space
        let from = self.text.len();
        let g = if self.mutant_join { from } else { self.text.iter().rposition(|&c| c == b' ').map_or(0, |i| i + 1) };
        let pen = if g < from { self.x[g] } else { self.width };
        self.text.extend_from_slice(tail.as_bytes());
        // SAFETY: `text` is ASCII (checked on every append), and `g` is a byte index into it.
        let part = unsafe { core::str::from_utf8_unchecked(&self.text[g..]) };
        let mut run = core::mem::take(&mut self.scratch);
        if !sh.shape_into(part, 1.0, &mut run) || run.len() != part.len() {
            self.text.truncate(from);
            self.scratch = run;
            return false;
        }
        self.shaped = part.len();
        self.gid.truncate(g);
        self.x.truncate(g);
        self.gid.extend_from_slice(&run.gid);
        self.x.extend(run.x.iter().map(|v| v + pen));
        self.width = pen + run.width;
        self.scratch = run;
        self.rebreak_tail();
        true
    }

    /// Re-break from the start of the last line.
    fn rebreak_tail(&mut self) {
        let keep = self.lines.len().saturating_sub(1);
        let s0 = self.lines.get(keep).copied().unwrap_or(0) as usize;
        self.lines.truncate(keep);
        let x0 = if keep == 0 { 0 } else { self.x[s0] };
        let lines = &mut self.lines;
        brk::block_from(&self.text, &self.x, self.width, self.w, s0, x0, |g| lines.push(g));
    }

    /// The width changed: every line is broken again (no shaping).
    pub fn set_width(&mut self, w: i32) {
        self.w = w;
        self.lines.clear();
        self.rebreak_tail();
    }
}
