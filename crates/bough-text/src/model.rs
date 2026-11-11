//! Round 3: an editor's text model fed **external values** (a binding that hands the editor a whole new `String`
//! per update, zgx `TextEditor::set_text`), with bounded undo history.
//!
//! The new value is taken over, never copied: the model keeps the caller's `String`. What changed is found by one
//! pass from each end, 64 bytes per step (four NEON compares folded with `orr` before one `umaxv`), and the undo
//! entry holds only the replaced and the inserted bytes. So an update costs the comparison (which no model can
//! skip: equal bytes have to be read to be known equal) and an entry of the edit's size; history is bounded by
//! entries and bytes, oldest first.

use std::collections::VecDeque;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Edit {
    at: usize,
    removed: Box<str>,
    inserted: Box<str>,
}

impl Edit {
    fn bytes(&self) -> usize {
        self.removed.len() + self.inserted.len()
    }
}

/// Anchor and focus (byte offsets).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    pub anchor: usize,
    pub focus: usize,
}

pub struct TextModel {
    text: String,
    undo: VecDeque<Edit>,
    redo: Vec<Edit>,
    sel: Selection,
    max_entries: usize,
    max_bytes: usize,
    bytes: usize,
}

impl TextModel {
    pub fn new(text: String) -> TextModel {
        let n = text.len();
        TextModel { text, undo: VecDeque::new(), redo: Vec::new(), sel: Selection { anchor: n, focus: n }, max_entries: 100, max_bytes: 4 << 20, bytes: 0 }
    }

    pub fn set_history_limits(&mut self, entries: usize, bytes: usize) {
        self.max_entries = entries;
        self.max_bytes = bytes;
        self.trim();
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn selection(&self) -> Selection {
        self.sel
    }

    fn trim(&mut self) {
        while self.undo.len() > self.max_entries || (self.bytes > self.max_bytes && !self.undo.is_empty()) {
            let e = self.undo.pop_front().unwrap();
            self.bytes -= e.bytes();
        }
    }

    /// A new external value. `false` if it equals the current text (nothing recorded). The caret goes to the end
    /// of the inserted bytes.
    pub fn set_text(&mut self, new: String) -> bool {
        let (a, b) = (self.text.as_bytes(), new.as_bytes());
        let p = common_prefix(a, b);
        if p == a.len() && p == b.len() {
            return false;
        }
        let s = common_suffix(&a[p..], &b[p..]);
        // keep the edit on character boundaries
        let mut p = p;
        while !self.text.is_char_boundary(p) || !new.is_char_boundary(p) {
            p -= 1;
        }
        let (mut ea, mut eb) = (a.len() - s, b.len() - s);
        while !self.text.is_char_boundary(ea) || !new.is_char_boundary(eb) {
            ea += 1;
            eb += 1;
        }
        let e = Edit { at: p, removed: self.text[p..ea].into(), inserted: new[p..eb].into() };
        self.bytes += e.bytes();
        self.undo.push_back(e);
        self.redo.clear();
        self.text = new;
        self.sel = Selection { anchor: eb, focus: eb };
        self.trim();
        true
    }

    /// The writer's own edit (text round 3b, "the value that was there all along"): the writer knows what changed,
    /// so the delta is recorded as it is applied and nothing is compared. Same history, same caret rule as
    /// `set_text` of the resulting value.
    pub fn splice(&mut self, range: std::ops::Range<usize>, insert: &str) -> bool {
        if range.is_empty() && insert.is_empty() {
            return false;
        }
        let e = Edit { at: range.start, removed: self.text[range.clone()].into(), inserted: insert.into() };
        if *e.removed == *e.inserted {
            return false;
        }
        self.text.replace_range(range.clone(), insert);
        let c = range.start + insert.len();
        self.bytes += e.bytes();
        self.undo.push_back(e);
        self.redo.clear();
        self.sel = Selection { anchor: c, focus: c };
        self.trim();
        true
    }

    /// Append `s` (a stream's token): `splice(len..len, s)`.
    pub fn push_str(&mut self, s: &str) -> bool {
        let n = self.text.len();
        self.splice(n..n, s)
    }

    pub fn undo(&mut self) -> bool {
        let Some(e) = self.undo.pop_back() else { return false };
        self.bytes -= e.bytes();
        self.text.replace_range(e.at..e.at + e.inserted.len(), &e.removed);
        let c = e.at + e.removed.len();
        self.sel = Selection { anchor: c, focus: c };
        self.redo.push(e);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(e) = self.redo.pop() else { return false };
        self.text.replace_range(e.at..e.at + e.removed.len(), &e.inserted);
        let c = e.at + e.inserted.len();
        self.sel = Selection { anchor: c, focus: c };
        self.bytes += e.bytes();
        self.undo.push_back(e);
        true
    }
}

/// Length of the common prefix of `a` and `b`, 64 bytes per step.
pub fn common_prefix(a: &[u8], b: &[u8]) -> usize {
    let n = a.len().min(b.len());
    let mut i = 0;
    #[cfg(target_arch = "aarch64")]
    unsafe {
        use core::arch::aarch64::*;
        let (pa, pb) = (a.as_ptr(), b.as_ptr());
        // SAFETY: every load reads 64 bytes inside both slices (`i + 64 <= n`).
        while i + 64 <= n {
            let x = vld1q_u8_x4(pa.add(i));
            let y = vld1q_u8_x4(pb.add(i));
            let d = vorrq_u8(vorrq_u8(veorq_u8(x.0, y.0), veorq_u8(x.1, y.1)), vorrq_u8(veorq_u8(x.2, y.2), veorq_u8(x.3, y.3)));
            if vmaxvq_u8(d) != 0 {
                break;
            }
            i += 64;
        }
    }
    while i + 8 <= n {
        let x = u64::from_le_bytes(a[i..i + 8].try_into().unwrap()) ^ u64::from_le_bytes(b[i..i + 8].try_into().unwrap());
        if x != 0 {
            return i + (x.trailing_zeros() / 8) as usize;
        }
        i += 8;
    }
    while i < n && a[i] == b[i] {
        i += 1;
    }
    i
}

/// Length of the common suffix of `a` and `b`, 8 bytes per step.
pub fn common_suffix(a: &[u8], b: &[u8]) -> usize {
    let n = a.len().min(b.len());
    let (la, lb) = (a.len(), b.len());
    let mut k = 0;
    while k + 8 <= n {
        let x = u64::from_le_bytes(a[la - k - 8..la - k].try_into().unwrap()) ^ u64::from_le_bytes(b[lb - k - 8..lb - k].try_into().unwrap());
        if x != 0 {
            return k + (x.leading_zeros() / 8) as usize;
        }
        k += 8;
    }
    while k < n && a[la - 1 - k] == b[lb - 1 - k] {
        k += 1;
    }
    k
}
