//! Round 3: multi-line text as a **ring of lines**.
//!
//! Round 2 shaped a multi-line text into one block and drew each line as a window onto it, so any change
//! re-shaped the whole block (heavy's log pane: five 84-byte lines per tick for one new line), and a line with
//! a ligature or a Latin-1 character sent the whole text to rustybuzz glyph instances.
//!
//! Here every line is shaped **alone** into its own pool block (byte slots, `slots::any_slots`, so ligatures and
//! Latin-1 are ordinary lines), and the text's run headers `[first, first + cap)` draw visual line `i` from
//! header `i`. A new value is matched against the old one *line by line* before anything is shaped:
//!
//! * the common prefix of lines and the common suffix of lines keep their blocks (no glyph is touched);
//! * a **shift** (the first new line equals old line `k`: a log or stream that scrolled `k` lines off the top)
//!   keeps every surviving line's block and re-points its header one slot up: the window header slides;
//! * only the lines in between are shaped, each alone, into recycled blocks (the pool's size-class free lists);
//!   the blocks of lines that left are released.
//!
//! So the cost of a log tick is one line shaped plus `O(visible lines)` header writes and byte compares,
//! independent of the history, and an edit in the middle of a document shapes only the changed line. Comparing
//! lines is ~0.1 instruction per byte (16 bytes per NEON compare), shaping ~8, so proving a line unchanged is
//! always cheaper than re-shaping it. Damage is per line and O(1): the old and new extents of each line whose
//! (block, position) changed, through the strike's conservative `LineBounds`.
//!
//! [`LineSet::push_line`] is the same ring driven by structure instead of a diff (a log that knows it appends).

use crate::atlas::LineBounds;
use crate::pool::{GlyphPool, Marks, RunHeader};
use crate::shape::Shaper;
use crate::slots::{any_slots, Refusal};

/// One visual line: its bytes in the text and its own shaped block.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LineRec {
    /// Byte offset of the line in the current text, and its length (no `\n`).
    pub start: u32,
    pub len: u32,
    /// The line's pool block (`u32::MAX` for an empty line, which owns none) and its capacity.
    pub base: u32,
    pub bcap: u32,
    /// Pen after the last glyph (font units): the line's advance width.
    pub width: i32,
}

/// How the run headers are drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LineStyle {
    /// Pen origin of line 0 (`oy`: its baseline), space-relative px.
    pub ox: f32,
    pub oy: f32,
    /// Line advance, px (baselines are `round(oy + i * lh)`).
    pub lh: f32,
    pub scale: f32,
    pub color: u32,
    pub space: u16,
    pub strike: u16,
}

/// What a `set_text` did (for counters and tests).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SetStats {
    /// Lines whose block was kept (prefix, shift, suffix).
    pub kept: u32,
    /// Lines shaped.
    pub shaped: u32,
    /// Bytes shaped.
    pub bytes: u32,
    /// The header range moved (grew): the caller must re-point the record's payload.
    pub moved: bool,
    /// A shaped line holds a token beyond ASCII (the caller readies its strike, a cold path).
    pub exotic: bool,
}

pub struct LineSet {
    pub lines: Vec<LineRec>,
    text: Vec<u8>,
    /// Run headers: visual line `i` is header `first + i`.
    pub first: u32,
    pub cap: u32,
    pub style: LineStyle,
    /// Maximum lines kept by `push_line` (0: unbounded).
    pub max_lines: usize,
    // scratch, reused (no allocation in steady state)
    spans: Vec<(u32, u32)>,
    shaped: Vec<LineRec>,
    table: Vec<LineRec>,
    used: Vec<bool>,
}

/// Where new line `i` comes from: `Some(old line)` (its block is kept) or `None` (it is shaped).
#[derive(Clone, Copy)]
struct Match {
    p: usize,
    q: usize,
    shift: usize,
    s: usize,
    nn: usize,
    on: usize,
}
impl Match {
    #[inline]
    fn src(&self, i: usize) -> Option<usize> {
        if i < self.p {
            Some(i)
        } else if i < self.q {
            Some(i + self.shift)
        } else if i >= self.nn - self.s {
            Some(self.on - (self.nn - i))
        } else {
            None
        }
    }
    /// New line `i` keeps both its block and its header (same old line at the same index).
    #[inline]
    fn stays(&self, i: usize) -> bool {
        i < self.nn && self.src(i) == Some(i)
    }
}

/// Do `a` and `b` hold the same bytes? NEON compares 16 bytes per step.
#[inline]
fn same(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    #[cfg(target_arch = "aarch64")]
    unsafe {
        use core::arch::aarch64::*;
        let n = a.len();
        let (pa, pb) = (a.as_ptr(), b.as_ptr());
        let mut i = 0;
        // SAFETY: every load reads 16 bytes inside both slices.
        while i + 16 <= n {
            let d = veorq_u8(vld1q_u8(pa.add(i)), vld1q_u8(pb.add(i)));
            if vmaxvq_u8(d) != 0 {
                return false;
            }
            i += 16;
        }
        if n >= 16 {
            let d = veorq_u8(vld1q_u8(pa.add(n - 16)), vld1q_u8(pb.add(n - 16)));
            return vmaxvq_u8(d) == 0;
        }
        return a[i..] == b[i..];
    }
    #[allow(unreachable_code)]
    {
        a == b
    }
}

/// Byte spans of the lines of `t` (split at `\n`), into `out`. NEON finds the newlines 16 bytes at a time.
pub fn split_lines(t: &[u8], out: &mut Vec<(u32, u32)>) {
    out.clear();
    let n = t.len();
    let mut a = 0usize;
    let mut i = 0usize;
    #[cfg(target_arch = "aarch64")]
    unsafe {
        use core::arch::aarch64::*;
        let nl = vdupq_n_u8(b'\n');
        // SAFETY: each load reads 16 bytes inside `t`.
        while i + 16 <= n {
            let eq = vceqq_u8(vld1q_u8(t.as_ptr().add(i)), nl);
            let mut m = vget_lane_u64::<0>(vreinterpret_u64_u8(vshrn_n_u16::<4>(vreinterpretq_u16_u8(eq)))) & 0x8888_8888_8888_8888;
            while m != 0 {
                let p = i + (m.trailing_zeros() >> 2) as usize;
                m &= m - 1;
                out.push((a as u32, (p - a) as u32));
                a = p + 1;
            }
            i += 16;
        }
    }
    while i < n {
        if t[i] == b'\n' {
            out.push((a as u32, (i - a) as u32));
            a = i + 1;
        }
        i += 1;
    }
    out.push((a as u32, (n - a) as u32));
}

impl LineSet {
    /// An empty set (no lines, no headers yet).
    pub fn new(style: LineStyle) -> LineSet {
        LineSet { lines: Vec::new(), text: Vec::new(), first: 0, cap: 0, style, max_lines: 0, spans: Vec::new(), shaped: Vec::new(), table: Vec::new(), used: Vec::new() }
    }

    pub fn text(&self) -> &[u8] {
        &self.text
    }

    pub fn line_bytes(&self, i: usize) -> &[u8] {
        let l = self.lines[i];
        &self.text[l.start as usize..(l.start + l.len) as usize]
    }

    /// Release the blocks and the header range.
    pub fn free(&mut self, pool: &mut GlyphPool) {
        for l in self.lines.drain(..) {
            if l.base != u32::MAX {
                pool.free_block(l.base, l.bcap);
            }
        }
        if self.cap > 0 {
            pool.free_run_range(self.first, self.cap);
            self.cap = 0;
        }
        self.text.clear();
    }

    /// Shape `line` alone into a fresh block.
    fn shape_line(pool: &mut GlyphPool, sh: &Shaper, line: &[u8], start: u32, exotic: &mut bool) -> Result<LineRec, Refusal> {
        let n = line.len();
        if n == 0 {
            return Ok(LineRec { start, len: 0, base: u32::MAX, bcap: 0, width: 0 });
        }
        let cap = n.max(16).next_power_of_two();
        let base = pool.alloc_block(cap, u32::MAX);
        // SAFETY: the block holds `cap >= n.next_multiple_of(16)` slots; `line` is UTF-8 (a line of a `&str`
        // split at an ASCII byte).
        let r = unsafe { any_slots(sh, line, 0, pool.x.as_mut_ptr().add(base as usize), pool.ent.as_mut_ptr().add(base as usize)) };
        match r {
            Ok((w, x)) => {
                *exotic |= x;
                Ok(LineRec { start, len: n as u32, base, bcap: cap as u32, width: w })
            }
            Err(e) => {
                pool.free_block(base, cap as u32);
                Err(e)
            }
        }
    }

    /// The run header that draws line `l` at visual index `i`.
    #[inline]
    pub fn header(&self, i: usize, l: &LineRec) -> RunHeader {
        let s = &self.style;
        RunHeader {
            origin: [s.ox, (s.oy + i as f32 * s.lh).round()],
            scale: s.scale,
            color: s.color,
            space: s.space,
            strike: s.strike,
            base: if l.base == u32::MAX { 0 } else { l.base },
            len: l.len,
            x0: 0,
        }
    }

    /// Grow the header range to hold `n` lines (old extents marked first: the range moves).
    fn reserve<P: Marks + ?Sized>(&mut self, pool: &mut GlyphPool, n: usize, b: &LineBounds, damage: &mut Option<&mut P>) -> bool {
        if n as u32 <= self.cap {
            return false;
        }
        if self.cap > 0 {
            if let Some(d) = damage.as_deref_mut() {
                for (i, l) in self.lines.iter().enumerate() {
                    pool.mark_window(self.first + i as u32, l.width, b, d);
                }
            }
            pool.free_run_range(self.first, self.cap);
        }
        let (f, c) = pool.alloc_run_range(n);
        self.first = f;
        self.cap = c;
        true
    }

    /// Set the text. Lines equal to an old line at the same index, or at the index of a detected shift, keep
    /// their blocks; the others are shaped alone. With `damage`, the old and new extents of every line whose
    /// header changed are marked (O(1) each). On `Err` (a line is not covered) nothing changed.
    pub fn set_text<P: Marks + ?Sized>(&mut self, pool: &mut GlyphPool, sh: &Shaper, new: &str, b: &LineBounds, mut damage: Option<&mut P>) -> Result<SetStats, Refusal> {
        let nb = new.as_bytes();
        let mut spans = std::mem::take(&mut self.spans);
        split_lines(nb, &mut spans);
        let (nn, on) = (spans.len(), self.lines.len());
        let m = {
            let line_new = |i: usize| &nb[spans[i].0 as usize..(spans[i].0 + spans[i].1) as usize];
            let line_old = |j: usize| &self.text[self.lines[j].start as usize..(self.lines[j].start + self.lines[j].len) as usize];
            // 1. common prefix
            let mut p = 0;
            while p < nn && p < on && same(line_new(p), line_old(p)) {
                p += 1;
            }
            // 2. a shift: new line p is old line p + k (k lines scrolled off the top)
            let (mut q, mut shift) = (p, 0usize);
            if p < nn && p + 1 < on {
                for k in 1..=(on - p - 1).min(8) {
                    if same(line_new(p), line_old(p + k)) {
                        shift = k;
                        break;
                    }
                }
                if shift > 0 {
                    while q < nn && q + shift < on && same(line_new(q), line_old(q + shift)) {
                        q += 1;
                    }
                }
            }
            // 3. common suffix of what is left on both sides
            let mut s = 0;
            while nn - s > q && on - s > q + shift && same(line_new(nn - 1 - s), line_old(on - 1 - s)) {
                s += 1;
            }
            Match { p, q, shift, s, nn, on }
        };
        // 4. shape the lines in between first: on a refusal nothing has changed
        let mut shaped = std::mem::take(&mut self.shaped);
        shaped.clear();
        let mut st = SetStats::default();
        for i in m.q..nn - m.s {
            let (a, len) = spans[i];
            match Self::shape_line(pool, sh, &nb[a as usize..(a + len) as usize], a, &mut st.exotic) {
                Ok(l) => {
                    st.shaped += 1;
                    st.bytes += l.len;
                    shaped.push(l);
                }
                Err(e) => {
                    for l in shaped.drain(..) {
                        if l.base != u32::MAX {
                            pool.free_block(l.base, l.bcap);
                        }
                    }
                    self.shaped = shaped;
                    self.spans = spans;
                    return Err(e);
                }
            }
        }
        // 5. the new line table
        let mut table = std::mem::take(&mut self.table);
        let mut used = std::mem::take(&mut self.used);
        table.clear();
        used.clear();
        used.resize(on, false);
        let mut fi = 0;
        for i in 0..nn {
            match m.src(i) {
                Some(j) => {
                    used[j] = true;
                    table.push(LineRec { start: spans[i].0, ..self.lines[j] });
                    st.kept += 1;
                }
                None => {
                    table.push(shaped[fi]);
                    fi += 1;
                }
            }
        }
        // 6. headers. Old extents of the lines whose header changes (headers still describe them).
        st.moved = self.reserve(pool, nn, b, &mut damage);
        if !st.moved {
            if let Some(d) = damage.as_deref_mut() {
                for i in 0..on {
                    if !m.stays(i) {
                        pool.mark_window(self.first + i as u32, self.lines[i].width, b, d);
                    }
                }
            }
        }
        for j in 0..on {
            let l = self.lines[j];
            if !used[j] && l.base != u32::MAX {
                pool.free_block(l.base, l.bcap);
            }
        }
        for i in 0..nn {
            if st.moved || !m.stays(i) {
                let h = self.header(i, &table[i]);
                pool.runs[(self.first + i as u32) as usize] = h;
                if let Some(d) = damage.as_deref_mut() {
                    pool.mark_window(self.first + i as u32, table[i].width, b, d);
                }
            }
        }
        for i in nn..self.cap as usize {
            pool.runs[(self.first + i as u32) as usize].len = 0;
        }
        std::mem::swap(&mut self.lines, &mut table);
        self.text.clear();
        self.text.extend_from_slice(nb);
        self.table = table;
        self.used = used;
        self.shaped = shaped;
        self.spans = spans;
        Ok(st)
    }

    /// The stream form: append one line (no `\n` in it), shaped alone; when `max_lines` is set and exceeded the
    /// first line leaves (its block is recycled) and every header slides one slot up. Equal to `set_text` of
    /// the resulting text (tested).
    pub fn push_line<P: Marks + ?Sized>(&mut self, pool: &mut GlyphPool, sh: &Shaper, line: &str, b: &LineBounds, mut damage: Option<&mut P>) -> Result<SetStats, Refusal> {
        let lb = line.as_bytes();
        debug_assert!(!lb.contains(&b'\n'));
        let mut exotic = false;
        let rec = Self::shape_line(pool, sh, lb, 0, &mut exotic)?;
        let mut st = SetStats { shaped: 1, bytes: rec.len, exotic, ..Default::default() };
        let on = self.lines.len();
        let evict = self.max_lines > 0 && on >= self.max_lines;
        let n = if evict { on } else { on + 1 };
        st.moved = self.reserve(pool, n, b, &mut damage);
        // old extents of every header that changes: all of them on an eviction (each slides up), else none
        if evict && !st.moved {
            if let Some(d) = damage.as_deref_mut() {
                for (i, l) in self.lines.iter().enumerate() {
                    pool.mark_window(self.first + i as u32, l.width, b, d);
                }
            }
        }
        if evict {
            let gone = self.lines.remove(0);
            if gone.base != u32::MAX {
                pool.free_block(gone.base, gone.bcap);
            }
            let cut = ((gone.len + 1) as usize).min(self.text.len());
            self.text.drain(..cut);
            for l in &mut self.lines {
                l.start -= cut as u32;
            }
        }
        if !self.lines.is_empty() {
            self.text.push(b'\n');
        }
        let start = self.text.len() as u32;
        self.text.extend_from_slice(lb);
        self.lines.push(LineRec { start, ..rec });
        st.kept = n as u32 - 1;
        let from = if evict || st.moved { 0 } else { n - 1 };
        for i in from..n {
            let h = self.header(i, &self.lines[i]);
            pool.runs[(self.first + i as u32) as usize] = h;
            if let Some(d) = damage.as_deref_mut() {
                pool.mark_window(self.first + i as u32, self.lines[i].width, b, d);
            }
        }
        Ok(st)
    }

    /// Re-place every header (a new style: origin, line height, colour; no glyph is touched). Marks old and new
    /// extents.
    pub fn restyle<P: Marks + ?Sized>(&mut self, pool: &mut GlyphPool, style: LineStyle, b: &LineBounds, mut damage: Option<&mut P>) {
        if let Some(d) = damage.as_deref_mut() {
            for (i, l) in self.lines.iter().enumerate() {
                pool.mark_window(self.first + i as u32, l.width, b, d);
            }
        }
        self.style = style;
        for i in 0..self.lines.len() {
            let h = self.header(i, &self.lines[i]);
            pool.runs[(self.first + i as u32) as usize] = h;
            if let Some(d) = damage.as_deref_mut() {
                pool.mark_window(self.first + i as u32, self.lines[i].width, b, d);
            }
        }
    }
}
