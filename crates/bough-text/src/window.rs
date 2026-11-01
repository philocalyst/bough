//! Invention A of `RESULTS.md`: a run is a *window* onto shaped slots.
//!
//! `RunHeader { base, len, x0 }` draws slots `base..base+len` with pens `x[k] - x0`. The pen of a
//! glyph inside a window depends only on the pairs inside the window, so a window over a longer
//! shaped text is exactly the shaping of the window's own bytes (the window's last glyph keeps
//! the kern with the byte after it, which only changes the window's *width*, never a pen). So:
//!
//! * a wrapped paragraph is shaped once; each line is one 32-byte header (no per-glyph rebase);
//! * a stream keeps its shaped tail; appending shapes only the new bytes; the visible lines
//!   are re-pointed windows, whatever the line breaking does to the text;
//! * a list of retained lines that scrolls moves headers (or spaces), not glyphs.
//!
//! Damage stays O(1) per line: the line's old and new pen extents through the strike's
//! conservative [`LineBounds`].

use crate::atlas::LineBounds;
use crate::pool::{mark, pen_px, GlyphPool, Marks, RunHeader};
use crate::shape::Shaper;

/// A text shaped into a pool block with no header of its own (ASCII through the dense table).
#[derive(Debug, Default, Clone, Copy)]
pub struct Shaped {
    pub base: u32,
    pub cap: u32,
    pub len: u32,
    /// Pen after the last glyph.
    pub width: i32,
    /// A token beyond ASCII was written (see `slots::any_slots`).
    pub exotic: bool,
}

impl GlyphPool {
    /// A run header with no glyphs yet.
    pub fn alloc_run(&mut self, origin: [f32; 2], scale: f32, color: u32, space: u16, strike: u16) -> u32 {
        self.new_run(RunHeader { origin, scale, color, space, strike, base: 0, len: 0, x0: 0 })
    }

    /// Shape UTF-8 `text` into a fresh block, one slot per byte (`slots::any_slots`: ASCII, Latin-1 and the
    /// font's ligatures, phantoms for the other bytes of a cluster). `false` if a codepoint is not covered
    /// (beyond Latin-1, a control character): the block is released then.
    pub fn shape_block(&mut self, sh: &Shaper, text: &[u8], out: &mut Shaped) -> bool {
        let n = text.len();
        let cap = n.max(16).next_power_of_two();
        let base = self.alloc_block(cap, u32::MAX);
        // SAFETY: the block holds `cap >= n.next_multiple_of(16)` slots; callers pass UTF-8 (a `&str`'s bytes).
        let r = unsafe { crate::slots::any_slots(sh, text, 0, self.x.as_mut_ptr().add(base as usize), self.ent.as_mut_ptr().add(base as usize)) };
        match r {
            Ok((w, exotic)) => {
                *out = Shaped { base, cap: cap as u32, len: n as u32, width: w, exotic };
                true
            }
            Err(_) => {
                self.free_block(base, cap as u32);
                false
            }
        }
    }

    pub fn free_shaped(&mut self, s: Shaped) {
        self.free_block(s.base, s.cap);
    }

    /// Point `run` at slots `[start, end)` of a shaped text, drawn from `origin`.
    #[inline]
    pub fn set_window(&mut self, run: u32, base: u32, start: u32, end: u32, origin: [f32; 2]) {
        let x0 = if end > start { self.x[(base + start) as usize] } else { 0 };
        let r = &mut self.runs[run as usize];
        r.base = base + start;
        r.len = end - start;
        r.x0 = x0;
        r.origin = origin;
    }

    /// Mark a line's tiles over the window pens `[0, w]` (font units) of `run`, O(1).
    #[inline]
    pub fn mark_window<P: Marks + ?Sized>(&self, run: u32, w: i32, b: &LineBounds, plane: &mut P) {
        let h = &self.runs[run as usize];
        let sp = self.spaces[h.space as usize];
        let oxh = h.origin[0] + 0.5;
        let oy = h.origin[1] as i32 + sp[1];
        mark(plane, pen_px(0, h.scale, oxh) + sp[0] + b.left, oy + b.top, pen_px(w, h.scale, oxh) + sp[0] + b.over, oy + b.bottom);
    }
}

pub use crate::slots::hybrid as slots;

/// Greedy word wrap over shaped slots (`text[i]` is slot `i`): appends the slot index where
/// each line starts. Same rule as `wrap::wrap_greedy`: break after spaces, a line's width is
/// its last visible glyph's right pen, a word wider than the line stays alone.
pub fn wrap_slots(x: &[i32], width: i32, text: &[u8], max_units: i32, starts: &mut Vec<u32>) -> usize {
    starts.clear();
    let n = text.len();
    if n == 0 {
        return 0;
    }
    starts.push(0);
    let mut line_x0 = x[0];
    let mut i = 0usize;
    let mut has_word = false;
    while i < n {
        let ws = i;
        while i < n && text[i] != b' ' {
            i += 1;
        }
        let we = i;
        while i < n && text[i] == b' ' {
            i += 1;
        }
        if we > ws {
            let end_x = if we < n { x[we] } else { width };
            if has_word && end_x - line_x0 > max_units {
                starts.push(ws as u32);
                line_x0 = x[ws];
            }
            has_word = true;
        }
    }
    starts.len()
}

/// A retained stream of ASCII text (a log pane, a token stream): its tail is kept shaped in
/// one block; [`Stream::append`] shapes only the new bytes; visible lines are windows.
pub struct Stream {
    pub base: u32,
    pub cap: u32,
    /// Slots `[start, start + len)` of the block hold the retained tail.
    start: usize,
    len: usize,
    /// The retained bytes, same indexing as the slots.
    bytes: Vec<u8>,
    /// Bytes kept (at least what can be visible).
    retain: usize,
    /// Total bytes ever appended (the stream offset of slot `start + len`).
    pub total: u64,
}

impl Stream {
    pub fn new(pool: &mut GlyphPool, retain: usize) -> Self {
        let cap = (retain * 2).next_power_of_two().max(64);
        let base = pool.alloc_block(cap, u32::MAX);
        Stream { base, cap: cap as u32, start: 0, len: 0, bytes: vec![0; cap], retain, total: 0 }
    }

    /// Append ASCII bytes, shaping only them (and re-reading the step of the previous last
    /// glyph, whose pen does not move). `false` if a trigger appears (the caller must re-shape
    /// with the token path); the stream is unchanged then.
    pub fn append(&mut self, pool: &mut GlyphPool, sh: &Shaper, chunk: &[u8]) -> bool {
        let k = chunk.len();
        if k == 0 {
            return true;
        }
        if !chunk.is_ascii() {
            return false;
        }
        let pad = k.next_multiple_of(16);
        if self.start + self.len + pad > self.cap as usize {
            // Compact: move the retained tail to the front (amortised O(1) per byte). Pens
            // keep their absolute values (wrapping i32; windows subtract x0).
            let (b, s, l) = (self.base as usize, self.start, self.len);
            pool.x.copy_within(b + s..b + s + l, b);
            pool.ent.copy_within(b + s..b + s + l, b);
            self.bytes.copy_within(s..s + l, 0);
            self.start = 0;
            assert!(self.len + pad <= self.cap as usize, "chunk larger than the stream block");
        }
        let at = self.start + self.len;
        let b = self.base as usize;
        let x_first = if self.len == 0 {
            0
        } else {
            let last = self.bytes[at - 1] as usize;
            let st = sh.dense()[last << 7 | chunk[0] as usize];
            if st & 0x8000 != 0 {
                return false;
            }
            pool.x[b + at - 1].wrapping_add(st as i32)
        };
        // SAFETY: ASCII; room for `pad` slots checked above.
        let (_, acc) = unsafe { slots(sh, chunk, 0, x_first, pool.x.as_mut_ptr().add(b + at), pool.ent.as_mut_ptr().add(b + at)) };
        if acc & 0x8000 != 0 {
            return false;
        }
        self.bytes[at..at + k].copy_from_slice(chunk);
        self.len += k;
        self.total += k as u64;
        if self.len > self.retain {
            let drop = self.len - self.retain;
            self.start += drop;
            self.len -= drop;
        }
        true
    }

    /// The pen after stream byte `off` (stream offset), i.e. where the next glyph would be.
    /// The last retained byte's pen-after uses the end-of-run step.
    fn pen_after(&self, pool: &GlyphPool, sh: &Shaper, off: u64) -> i32 {
        let i = self.slot_of(off);
        let b = self.base as usize;
        if i + 1 < self.start + self.len {
            pool.x[b + i + 1]
        } else {
            pool.x[b + i].wrapping_add(sh.dense()[(self.bytes[i] as usize) << 7] as i32)
        }
    }

    fn slot_of(&self, off: u64) -> usize {
        let first = self.total - self.len as u64;
        assert!(off >= first && off < self.total, "offset {off} not retained");
        self.start + (off - first) as usize
    }

    /// Point `run` at stream bytes `[from, to)` (stream offsets) drawn from `origin`. Returns
    /// the window's width (font units).
    pub fn window(&self, pool: &mut GlyphPool, sh: &Shaper, run: u32, from: u64, to: u64, origin: [f32; 2]) -> i32 {
        if to <= from {
            pool.set_window(run, self.base, 0, 0, origin);
            return 0;
        }
        let a = self.slot_of(from) as u32;
        pool.set_window(run, self.base, a, a + (to - from) as u32, origin);
        self.pen_after(pool, sh, to - 1).wrapping_sub(pool.x[(self.base + a) as usize])
    }

    pub fn bytes(&self, from: u64, to: u64) -> &[u8] {
        let a = self.slot_of(from);
        &self.bytes[a..a + (to - from) as usize]
    }
}
