//! The text half of zgx-script's code editor (`examples/code_editor`, measured by `code_editor_bench`) on
//! bough-text: a document of 100,000 lines in a **gap buffer** with a line index, the lines in view as a **ring
//! of lines** (`LineSet`: a scroll by k lines shapes k lines, a keystroke shapes one), Rust highlighting per
//! line as **colour windows** over the line's shaped block (a span is a 32-byte run header onto the same slots:
//! no per-glyph colour, no re-shaping), line numbers as a second ring, and soft wrap as one `Para` per line in
//! view. Damage goes to a tile plane as the façade's would.
//!
//! The gap buffer folds every offset after the caret into one constant: line starts are stored in buffer
//! coordinates, so typing at the gap moves no byte and no line start (only the gap's own line changes).
use bough_text::atlas::LineBounds;
use bough_text::lines::{LineSet, LineStyle};
use bough_text::para::Para;
use bough_text::pool::{GlyphPool, RunHeader};
use bough_text::Shaper;

/// A gap buffer with a line index (line starts in buffer coordinates).
pub struct Document {
    buf: Vec<u8>,
    g0: usize,
    g1: usize,
    /// Buffer index of each line's first byte.
    starts: Vec<u32>,
}

impl Document {
    pub fn new(text: String) -> Document {
        let mut buf = text.into_bytes();
        let n = buf.len();
        let gap = 4096;
        buf.resize(n + gap, 0);
        let mut spans = Vec::new();
        bough_text::lines::split_lines(&buf[..n], &mut spans);
        let starts = spans.iter().map(|s| s.0).collect();
        Document { buf, g0: n, g1: n + gap, starts }
    }

    pub fn lines(&self) -> usize {
        self.starts.len()
    }

    pub fn len(&self) -> usize {
        self.buf.len() - (self.g1 - self.g0)
    }

    /// Bytes of line `i` (into `scratch` when the gap splits it).
    pub fn line<'a>(&'a self, i: usize, scratch: &'a mut Vec<u8>) -> &'a [u8] {
        let a = self.starts[i] as usize;
        let e = if i + 1 < self.starts.len() { self.starts[i + 1] as usize - 1 } else { self.buf.len() };
        if a <= self.g0 && self.g0 <= e && self.g1 > self.g0 {
            scratch.clear();
            scratch.extend_from_slice(&self.buf[a..self.g0]);
            scratch.extend_from_slice(&self.buf[self.g1..e]);
            return scratch;
        }
        let e = if self.g0 == e { e } else { e };
        &self.buf[a..e]
    }

    /// Line holding logical byte `p`, and the buffer index of `p`.
    fn locate(&self, p: usize) -> (usize, usize) {
        let b = if p < self.g0 { p } else { p + (self.g1 - self.g0) };
        (self.starts.partition_point(|&s| s as usize <= b) - 1, b)
    }

    /// Move the gap to logical position `p` (a memmove of the distance, once per caret jump).
    pub fn move_gap(&mut self, p: usize) {
        let gap = self.g1 - self.g0;
        if p < self.g0 {
            let k = self.g0 - p;
            self.buf.copy_within(p..self.g0, self.g1 - k);
            // line starts in the moved range shift right by the gap
            for s in &mut self.starts {
                if (*s as usize) > p && (*s as usize) <= self.g0 {
                    *s += gap as u32;
                }
            }
            self.g0 = p;
            self.g1 -= k;
        } else if p > self.g0 {
            let k = p - self.g0;
            self.buf.copy_within(self.g1..self.g1 + k, self.g0);
            for s in &mut self.starts {
                if (*s as usize) >= self.g1 && (*s as usize) <= self.g1 + k {
                    *s -= gap as u32;
                }
            }
            self.g0 = p;
            self.g1 += k;
        }
    }

    /// Type `s` (no newline) at logical position `p`: the gap moves there (no-op when it is already there) and
    /// the bytes go into it; returns the line typed into.
    pub fn insert(&mut self, p: usize, s: &[u8]) -> usize {
        self.move_gap(p);
        if self.g1 - self.g0 < s.len() {
            let more = 4096.max(s.len());
            self.buf.splice(self.g1..self.g1, std::iter::repeat_n(0u8, more));
            for st in &mut self.starts {
                if *st as usize >= self.g1 {
                    *st += more as u32;
                }
            }
            self.g1 += more;
        }
        self.buf[self.g0..self.g0 + s.len()].copy_from_slice(s);
        self.g0 += s.len();
        self.locate(p).0
    }
}

pub const KEYWORD: u32 = 0xff5eb1ff;
pub const STRING: u32 = 0xff7ad67a;
pub const NUMBER: u32 = 0xffe5b25e;
pub const COMMENT: u32 = 0xff6b7686;
pub const TYPE: u32 = 0xff56c8d8;
pub const MACRO: u32 = 0xffe06c75;
pub const TEXT: u32 = 0xffe5edf7;

const KEYWORDS: &[&[u8]] = &[b"fn", b"let", b"pub", b"if", b"return", b"as", b"mut", b"for", b"in", b"while", b"match", b"struct", b"impl", b"use", b"mod", b"const", b"else", b"self", b"Self"];

/// Rust highlighting of one line given the state the line before left (0 nothing open, 1 a block comment, 2 a
/// string): spans `(from, to, colour)` into `out`; returns the state this line leaves.
pub fn highlight(line: &[u8], mut state: u8, out: &mut Vec<(u32, u32, u32)>) -> u8 {
    out.clear();
    let n = line.len();
    let mut i = 0;
    let push = |out: &mut Vec<(u32, u32, u32)>, a: usize, b: usize, c: u32| {
        if b > a {
            out.push((a as u32, b as u32, c));
        }
    };
    let mut plain = 0;
    while i < n {
        if state == 1 {
            let a = i;
            while i < n && !(line[i] == b'*' && i + 1 < n && line[i + 1] == b'/') {
                i += 1;
            }
            if i < n {
                i += 2;
                state = 0;
            }
            push(out, a, i, COMMENT);
            plain = i;
            continue;
        }
        if state == 2 {
            let a = i;
            while i < n && line[i] != b'"' {
                i += if line[i] == b'\\' { 2 } else { 1 };
            }
            i = (i + 1).min(n);
            state = if i <= n && line.get(i - 1) == Some(&b'"') { 0 } else { 2 };
            push(out, a, i, STRING);
            plain = i;
            continue;
        }
        let b = line[i];
        let (a, colour, end, st) = if b == b'/' && i + 1 < n && line[i + 1] == b'/' {
            (i, COMMENT, n, 0)
        } else if b == b'/' && i + 1 < n && line[i + 1] == b'*' {
            plain = plain.min(i);
            push(out, plain, i, TEXT);
            state = 1;
            i += 2;
            let a = i - 2;
            while i < n && !(line[i] == b'*' && i + 1 < n && line[i + 1] == b'/') {
                i += 1;
            }
            if i < n {
                i += 2;
                state = 0;
            }
            push(out, a, i, COMMENT);
            plain = i;
            continue;
        } else if b == b'"' {
            let mut j = i + 1;
            while j < n && line[j] != b'"' {
                j += if line[j] == b'\\' { 2 } else { 1 };
            }
            if j < n { (i, STRING, j + 1, 0) } else { (i, STRING, n, 2) }
        } else if b.is_ascii_digit() {
            let mut j = i;
            while j < n && (line[j].is_ascii_alphanumeric() || line[j] == b'_') {
                j += 1;
            }
            (i, NUMBER, j, 0)
        } else if b.is_ascii_alphabetic() || b == b'_' {
            let mut j = i;
            while j < n && (line[j].is_ascii_alphanumeric() || line[j] == b'_') {
                j += 1;
            }
            let w = &line[i..j];
            let c = if j < n && line[j] == b'!' {
                MACRO
            } else if KEYWORDS.contains(&w) {
                KEYWORD
            } else if b.is_ascii_uppercase() {
                TYPE
            } else {
                i = j;
                continue;
            };
            (i, c, j + (c == MACRO) as usize, 0)
        } else {
            i += 1;
            continue;
        };
        push(out, plain, a, TEXT);
        push(out, a, end, colour);
        state = st;
        i = end;
        plain = end;
    }
    push(out, plain, n, TEXT);
    state
}

/// The editor's view: the visible lines, their numbers, their colour windows; soft wrap optional.
pub struct View {
    pub first: usize,
    pub rows: usize,
    pub lines: LineSet,
    pub numbers: LineSet,
    /// Highlight state each line leaves (`u8::MAX`: not yet seen): one byte a line.
    pub states: Vec<u8>,
    /// Colour windows: `rows * SPANS` run headers, slot `r` holds visible row `r`'s spans.
    pub spans_first: u32,
    pub span_count: Vec<u8>,
    pub wrap: bool,
    pub paras: Vec<Para>,
    text: String,
    nums: String,
    scratch: Vec<u8>,
    hl: Vec<(u32, u32, u32)>,
}

pub const SPANS: usize = 16;

impl View {
    pub fn new(pool: &mut GlyphPool, rows: usize, style: LineStyle, num_style: LineStyle, lines: usize) -> View {
        let (spans_first, _) = pool.alloc_run_range(rows * SPANS);
        View {
            first: 0,
            rows,
            lines: LineSet::new(style),
            numbers: LineSet::new(num_style),
            states: vec![u8::MAX; lines],
            spans_first,
            span_count: vec![0; rows],
            wrap: false,
            paras: (0..rows).map(|_| Para::default()).collect(),
            text: String::new(),
            nums: String::new(),
            scratch: Vec::new(),
            hl: Vec::new(),
        }
    }

    /// The state line `i` starts in (computing forward from the last line seen before it).
    fn state_before(&mut self, doc: &Document, i: usize) -> u8 {
        if i == 0 {
            return 0;
        }
        if self.states[i - 1] != u8::MAX {
            return self.states[i - 1];
        }
        let mut k = i - 1;
        while k > 0 && self.states[k - 1] == u8::MAX {
            k -= 1;
        }
        let mut st = if k == 0 { 0 } else { self.states[k - 1] };
        let mut hl = std::mem::take(&mut self.hl);
        let mut scratch = std::mem::take(&mut self.scratch);
        for j in k..i {
            st = highlight(doc.line(j, &mut scratch), st, &mut hl);
            self.states[j] = st;
        }
        self.hl = hl;
        self.scratch = scratch;
        st
    }

    /// Show lines `first..first + rows` (after a scroll or an edit of line `edited`): the visible text and the
    /// numbers go through their rings (shift / prefix / suffix matching, only new or changed lines shaped);
    /// lines shaped are re-highlighted; every visible row's colour windows are written.
    pub fn show(&mut self, pool: &mut GlyphPool, sh: &Shaper, doc: &Document, first: usize, edited: Option<usize>, b: &LineBounds, plane: &mut [u128]) -> (u32, u32) {
        self.first = first;
        let end = (first + self.rows).min(doc.lines());
        let mut text = std::mem::take(&mut self.text);
        let mut nums = std::mem::take(&mut self.nums);
        text.clear();
        nums.clear();
        let mut scratch = std::mem::take(&mut self.scratch);
        for i in first..end {
            if i > first {
                text.push('\n');
                nums.push('\n');
            }
            // SAFETY of from_utf8_unchecked is not needed: the sample is ASCII; keep the check
            text.push_str(std::str::from_utf8(doc.line(i, &mut scratch)).expect("utf8"));
            nums.push_str(itoa(i + 1, &mut [0; 12]));
        }
        self.scratch = scratch;
        if let Some(e) = edited {
            self.states[e] = u8::MAX;
        }
        let st = self.lines.set_text(pool, sh, &text, b, Some(plane)).expect("covered");
        let sn = self.numbers.set_text(pool, sh, &nums, b, Some(plane)).expect("covered");
        // highlight the rows (a kept row keeps its spans; here every row's windows are re-pointed, since rows
        // slide; the tokenizer runs on rows whose line is new or edited)
        let mut st0 = self.state_before(doc, first);
        let mut hl = std::mem::take(&mut self.hl);
        let mut scratch = std::mem::take(&mut self.scratch);
        for (r, i) in (first..end).enumerate() {
            let line = doc.line(i, &mut scratch);
            st0 = highlight(line, st0, &mut hl);
            self.states[i] = st0;
            let l = self.lines.lines[r];
            let h = self.lines.header(r, &l);
            let k = hl.len().min(SPANS);
            for (s, &(a, e, c)) in hl.iter().take(k).enumerate() {
                pool.runs[self.spans_first as usize + r * SPANS + s] = RunHeader { base: l.base + a, len: e - a, x0: 0, color: c, ..h };
            }
            for s in k..self.span_count[r] as usize {
                pool.runs[self.spans_first as usize + r * SPANS + s].len = 0;
            }
            self.span_count[r] = k as u8;
            // the line's own header draws nothing: its spans do
            pool.runs[(self.lines.first + r as u32) as usize].len = 0;
        }
        if self.wrap {
            // soft wrap: each visible line a paragraph (shape + wrap at the editor's width)
            for (r, i) in (first..end).enumerate() {
                let line = doc.line(i, &mut scratch);
                let p = &mut self.paras[r];
                p.release(pool);
                p.set(pool, &[sh], std::str::from_utf8(line).unwrap(), &[]).unwrap();
                std::hint::black_box(p.wrap(140_000));
            }
        }
        self.hl = hl;
        self.scratch = scratch;
        self.text = text;
        self.nums = nums;
        (st.shaped + sn.shaped, st.bytes + sn.bytes)
    }
}

fn itoa(mut n: usize, buf: &mut [u8; 12]) -> &str {
    let mut i = 12;
    loop {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    // SAFETY: ASCII digits.
    unsafe { std::str::from_utf8_unchecked(&buf[i..]) }
}

/// zgx's `source(lines)`: a Rust file as the editor's sample makes it.
pub fn source(lines: usize) -> String {
    let body = [
        "/// A function, its docs and its body.",
        "pub fn compute_N(input: &[u8], scale: f32) -> Result<Vec<u32>, Error> {",
        "    let name = \"item N\"; // a comment",
        "    let total: u32 = input.iter().map(|b| *b as u32 * N).sum();",
        "    /* a block comment",
        "       running on */",
        "    if total > 0xff { return Err(Error::Overflow(total)); }",
        "    Ok(vec![total; scale as usize])",
        "}",
        "",
    ];
    (0..lines).map(|i| body[i % body.len()].replace('N', &i.to_string())).collect::<Vec<_>>().join("\n")
}
