//! zgx's Code example (`crates/zgx-script/examples/code_editor`: `app.luau`, `state.luau`, `actions.luau`,
//! `components/{find_bar,status_bar}.luau`, `lib/{highlight,search}.luau`) as a bough view: the editor (line
//! numbers, Rust highlighting, the current line, find matches marked behind the text, the caret), the find
//! bar (query, count, Previous/Next, Match Case, the replacement, Replace, Replace All, close) and the status
//! bar (line and column, the last message, the language, Wrap).
//!
//! Representation:
//! * the text is a gap buffer with a folded line index ([`doc`]); highlighting is zgx's tokenizer
//!   ([`highlight`]) with one state byte a line, valid up to a watermark (an edit that changes what a line
//!   leaves open lowers the watermark; nothing is cleared line by line);
//! * the screen is a window of rows: each row is its line projected into seven class rows (one per colour)
//!   and kept while its line is on screen and unedited; the window is seven multi-line texts plus the
//!   numbers, each a bough-text ring of lines (a line that did not change keeps its shaped block), in one
//!   space whose offset is the scroll's fraction of a line;
//! * the caret, the current line and the find bar are spaces (a move is one space write); the find marks
//!   are one `lanes` run; find-all keeps its searches and refines them ([`search`]).
pub mod doc;
pub mod highlight;
pub mod search;

use crate::theme::*;
use bough::layout::prelude::*;
use bough::prelude::*;
use doc::Document;
use highlight::CLASSES;
use search::Search;

pub const WIDTH: f32 = 1100.0;
pub const HEIGHT: f32 = 760.0;
/// The editor's text: `font = "mono"`, `size = 13`; zgx's lines are 19 pt.
pub const SIZE: f32 = 13.0;
pub const LINE: f32 = 19.0;
/// The status bar (py 4, a 22 pt row, its top border) and the find bar (py 6, a control, its bottom border).
pub const STATUS_H: f32 = 31.0;
pub const FIND_H: f32 = 47.0;
/// Line numbers start here, right aligned in `DIGITS` columns; the text starts at `TEXT_X`.
pub const GUTTER_X: f32 = 12.0;
pub const DIGITS: usize = 6;
pub const CARET_W: f32 = 1.0;
/// Find marks shown at once (the rows in view never hold more for a query of one letter or more).
pub const MARKS: usize = 96;
/// The find box: its left edge and text inset in the bar (`px = 12`, the input's own 8).
const FIND_X: f32 = 12.0;
const FIND_PAD: f32 = 8.0;
const FIND_W: f32 = 220.0;
/// Where a hidden bar goes: off the window, above it.
const AWAY: f32 = -2.0 * HEIGHT;

/// zgx's harness `source(lines)`: a Rust file of `lines` lines, as the editor's sample makes them.
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

/// The colour of each class (zgx's theme tokens: `$accent`, `$success`, `$warning`, `$text.tertiary`,
/// `$info`, `$danger`; plain text `$text.primary`).
pub const CLASS_COLOR: [Color; CLASSES] = [TEXT_PRIMARY, ACCENT, SUCCESS, WARNING, TEXT_TERTIARY, INFO, DANGER];
pub const MARK: Color = alpha(WARNING, 0x48);

/// The texts on screen: one per class, and the line numbers.
#[derive(Default)]
pub struct Layers {
    pub plain: Signal<String>,
    pub keyword: Signal<String>,
    pub string: Signal<String>,
    pub number: Signal<String>,
    pub comment: Signal<String>,
    pub ty: Signal<String>,
    pub mac: Signal<String>,
    pub numbers: Signal<String>,
}
impl Layers {
    fn class(&mut self, c: usize) -> &mut Signal<String> {
        match c {
            0 => &mut self.plain,
            1 => &mut self.keyword,
            2 => &mut self.string,
            3 => &mut self.number,
            4 => &mut self.comment,
            5 => &mut self.ty,
            _ => &mut self.mac,
        }
    }
}

/// The find marks on screen (content-space points; rows from the top of the window of rows).
#[derive(Clone, Copy, PartialEq)]
pub struct Marks {
    pub n: u32,
    pub w: f32,
    pub x: [f32; MARKS],
    pub y: [f32; MARKS],
}
impl Default for Marks {
    fn default() -> Self {
        Marks { n: 0, w: 0.0, x: [0.0; MARKS], y: [0.0; MARKS] }
    }
}
fn mark_lane(m: &Marks, i: usize) -> Lane {
    if (i as u32) < m.n {
        Lane { x: m.x[i], y: m.y[i], w: m.w, h: LINE, color: MARK }
    } else {
        Lane { x: 0.0, y: 0.0, w: 0.0, h: 0.0, color: CLEAR }
    }
}

/// A row on screen: its line projected by class, and the highlight state it started in.
#[derive(Default)]
struct Row {
    line: usize,
    start: u8,
    end: u8,
    cls: [Vec<u8>; CLASSES],
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    Text,
    Rust,
}

pub struct Code {
    // what the screen shows (wired)
    pub layers: Layers,
    pub content: Signal<(f32, f32)>,
    pub caret_at: Signal<(f32, f32)>,
    pub line_at: Signal<(f32, f32)>,
    pub ed_clip: Signal<[f32; 4]>,
    pub ed_span: Signal<(f32, f32)>,
    pub marks: Signal<Marks>,
    pub find_at: Signal<(f32, f32)>,
    pub find_caret: Signal<(f32, f32)>,
    pub query: Signal<String>,
    pub count: Signal<String>,
    pub st_caret: Signal<String>,
    pub st_msg: Signal<String>,
    pub st_lang: Signal<String>,
    pub status_at: Signal<(f32, f32)>,
    // the model
    pub doc: Document,
    pub file: Option<String>,
    pub lang: Lang,
    /// The caret (a logical byte offset).
    pub caret: usize,
    /// Scrolled down this many points.
    pub scroll: f64,
    pub find_open: bool,
    /// The query's caret (bytes).
    qcaret: usize,
    pub search: Search,
    /// Highlight state each line leaves; valid for lines `< hl_valid`.
    states: Vec<u8>,
    hl_valid: usize,
    rows: Vec<Row>,
    first: usize,
    /// Rows whose line was edited since they were projected: lines `>= stale_from` (`usize::MAX`: none),
    /// or only `stale_line`.
    stale_line: usize,
    stale_from: usize,
    spans: Vec<(u32, u32, u8)>,
    scratch: Vec<u8>,
    buf: String,
    /// Mono font: advance and units per em; the window's device scale.
    adv: u32,
    upem: u32,
    scale: f32,
}

impl Code {
    pub fn new(mono: &[u8], scale: f32) -> Code {
        let (adv, upem) = crate::mono_metrics(mono);
        let mut c = Code {
            layers: Layers::default(),
            content: Signal::new((0.0, 0.0)),
            caret_at: Signal::new((0.0, 0.0)),
            line_at: Signal::new((0.0, 0.0)),
            ed_clip: Signal::new([0.0, 0.0, WIDTH, HEIGHT - STATUS_H]),
            ed_span: Signal::new((0.0, HEIGHT - STATUS_H)),
            marks: Signal::new(Marks::default()),
            find_at: Signal::new((0.0, AWAY)),
            find_caret: Signal::new((0.0, AWAY)),
            query: Signal::new(String::new()),
            count: Signal::new(String::new()),
            st_caret: Signal::new("Ln 1, Col 1".into()),
            st_msg: Signal::new(String::new()),
            st_lang: Signal::new("Plain Text".into()),
            status_at: Signal::new((0.0, HEIGHT - STATUS_H + 1.0)),
            doc: Document::new(Vec::new()),
            file: None,
            lang: Lang::Text,
            caret: 0,
            scroll: 0.0,
            find_open: false,
            qcaret: 0,
            search: Search::default(),
            states: vec![u8::MAX],
            hl_valid: 0,
            rows: Vec::new(),
            first: 0,
            stale_line: usize::MAX,
            stale_from: usize::MAX,
            spans: Vec::new(),
            scratch: Vec::new(),
            buf: String::new(),
            adv,
            upem,
            scale,
        };
        c.refresh();
        c
    }

    // ------------------------------------------------------------------ geometry

    fn ed_top(&self) -> f32 {
        if self.find_open { FIND_H } else { 0.0 }
    }
    fn ed_h(&self) -> f32 {
        HEIGHT - STATUS_H - self.ed_top()
    }
    /// The text's left edge: the gutter's digits, a gap.
    pub fn text_x(&self) -> f32 {
        let s = self.scale;
        let digits = self.pen(GUTTER_X, DIGITS);
        (digits + (16.0 * s).round()) / s
    }
    /// Device x of column `col` of a text whose box starts at `x` points (bough-text's pen rounding).
    fn pen(&self, x: f32, col: usize) -> f32 {
        let s = self.scale;
        let k = SIZE * s / self.upem as f32;
        bough_text::pool::pen_px((col as u32 * self.adv) as i32, k, x * s + 0.5) as f32
    }
    fn max_scroll(&self) -> f64 {
        (self.doc.lines() as f64 * LINE as f64 - self.ed_h() as f64).max(0.0)
    }

    // ------------------------------------------------------------------ highlighting

    /// The state line `i` starts in (computed forward from the watermark).
    fn state_before(&mut self, i: usize) -> u8 {
        if i == 0 || self.lang != Lang::Rust {
            return 0;
        }
        while self.hl_valid < i {
            let k = self.hl_valid;
            let st = if k == 0 { 0 } else { self.states[k - 1] };
            let line = self.doc.line(k, &mut self.scratch);
            self.states[k] = highlight::rust(line, st, &mut self.spans);
            self.hl_valid += 1;
        }
        self.states[i - 1]
    }

    /// Project line `i` (starting in state `st`) into `row`.
    fn project(&mut self, i: usize, st: u8, row: &mut Row) {
        let line = self.doc.line(i, &mut self.scratch);
        let end = if self.lang == Lang::Rust {
            highlight::rust(line, st, &mut self.spans)
        } else {
            self.spans.clear();
            0
        };
        highlight::project(line, &self.spans, &mut row.cls);
        row.line = i;
        row.start = st;
        row.end = end;
        if self.lang == Lang::Rust && i < self.states.len() {
            if self.hl_valid > i && self.states[i] != end {
                // what this line leaves open changed: the states after it are no longer known
                self.hl_valid = i + 1;
            }
            self.states[i] = end;
            if self.hl_valid == i {
                self.hl_valid = i + 1;
            }
        }
    }

    // ------------------------------------------------------------------ the screen

    /// Bring every wired value up to the model: the rows in view (projecting only lines that are new or were
    /// edited), the texts, the caret, the current line, the marks, the status.
    pub fn refresh(&mut self) {
        let top = self.ed_top();
        let h = self.ed_h();
        self.ed_clip.set([0.0, top, WIDTH, h]);
        self.ed_span.set((top, h));
        self.scroll = self.scroll.clamp(0.0, self.max_scroll());
        let lines = self.doc.lines();
        let first = ((self.scroll / LINE as f64).floor() as usize).min(lines.saturating_sub(1));
        let frac = (self.scroll - first as f64 * LINE as f64) as f32;
        let count = ((h / LINE).ceil() as usize + 1).min(lines - first);
        // the rows: kept by line while on screen and not edited
        let mut old = std::mem::take(&mut self.rows);
        let old_first = self.first;
        let mut rows: Vec<Row> = Vec::with_capacity(count);
        let mut st = self.state_before(first);
        for r in 0..count {
            let i = first + r;
            let stale = i == self.stale_line || i >= self.stale_from;
            let kept = (i >= old_first && i - old_first < old.len()).then(|| std::mem::take(&mut old[i - old_first]));
            let row = match kept {
                Some(row) if !stale && row.line == i && row.start == st => row,
                Some(mut row) => {
                    self.project(i, st, &mut row);
                    row
                }
                None => {
                    let mut row = Row::default();
                    self.project(i, st, &mut row);
                    row
                }
            };
            st = row.end;
            rows.push(row);
        }
        self.rows = rows;
        self.first = first;
        self.stale_line = usize::MAX;
        self.stale_from = usize::MAX;
        // the texts: one per class, a line per row
        let mut buf = std::mem::take(&mut self.buf);
        for c in 0..CLASSES {
            buf.clear();
            for (r, row) in self.rows.iter().enumerate() {
                if r > 0 {
                    buf.push('\n');
                }
                // SAFETY of the conversion: rows are byte copies of the (UTF-8) text with ASCII blanks
                // between whole chars; `from_utf8` checks it.
                buf.push_str(std::str::from_utf8(&row.cls[c]).unwrap_or(""));
            }
            let sig = self.layers.class(c);
            if **sig != buf {
                std::mem::swap(&mut **sig, &mut buf);
            }
        }
        buf.clear();
        for r in 0..count {
            if r > 0 {
                buf.push('\n');
            }
            let n = first + r + 1;
            let mut digits = [b' '; DIGITS];
            let mut k = n;
            for d in (0..DIGITS).rev() {
                digits[d] = b'0' + (k % 10) as u8;
                k /= 10;
                if k == 0 {
                    break;
                }
            }
            buf.push_str(std::str::from_utf8(&digits).unwrap_or(""));
        }
        if *self.layers.numbers != buf {
            std::mem::swap(&mut *self.layers.numbers, &mut buf);
        }
        self.buf = buf;
        self.content.set((0.0, top - frac));
        // the caret and its line
        let line = self.doc.line_of(self.caret);
        let col = self.column(line, self.caret);
        let y = top - frac + (line as f32 - first as f32) * LINE;
        let x = self.pen(self.text_x(), col) / self.scale;
        self.caret_at.set((x, y));
        self.line_at.set((0.0, y));
        let st = format!("Ln {}, Col {}", line + 1, col + 1);
        self.st_caret.set(st);
        self.mark();
    }

    /// Chars from line `line`'s start to `p`.
    fn column(&mut self, line: usize, p: usize) -> usize {
        let a = self.doc.line_start(line);
        let s = self.doc.slice(a, p, &mut self.scratch);
        s.iter().filter(|&&b| b & 0xc0 != 0x80).count()
    }

    /// The find marks in view, and the count.
    fn mark(&mut self) {
        let mut m = Marks::default();
        let count;
        if !self.find_open || self.query.is_empty() {
            count = String::new();
        } else {
            let (first, rows) = (self.first, self.rows.len());
            let (a, b) = if rows > 0 { (self.doc.line_start(first), self.doc.line_end(first + rows - 1)) } else { (1, 0) };
            let hits = self.search.find(&self.doc, self.query.as_bytes());
            let (n, more) = Search::count(hits);
            let from = hits.partition_point(|&p| (p as usize) < a);
            let vis: Vec<u32> = hits[from..].iter().copied().take_while(|&p| p as usize <= b).take(MARKS).collect();
            count = if n == 0 { "No results".to_owned() } else { format!("{n}{} matches", if more { "+" } else { "" }) };
            let chars = self.query.chars().count();
            let tx = self.text_x();
            let mut line = first;
            for p in vis {
                let p = p as usize;
                while line + 1 < first + rows && self.doc.line_start(line + 1) <= p {
                    line += 1;
                }
                let col = self.column(line, p);
                let k = m.n as usize;
                let x0 = self.pen(tx, col);
                m.x[k] = x0 / self.scale;
                m.y[k] = (line - first) as f32 * LINE;
                m.w = (self.pen(tx, col + chars) - x0) / self.scale;
                m.n += 1;
            }
        }
        self.marks.set(m);
        self.count.set(count);
    }

    fn reveal(&mut self) {
        let line = self.doc.line_of(self.caret) as f64;
        let (y, h, lh) = (line * LINE as f64, self.ed_h() as f64, LINE as f64);
        if y < self.scroll {
            self.scroll = y;
        } else if y + lh > self.scroll + h {
            self.scroll = y + lh - h;
        }
    }

    // ------------------------------------------------------------------ actions

    /// `actions.open_file`: the file read and shown, the caret at its start.
    pub fn open(&mut self, path: &std::path::Path) {
        match std::fs::read(path) {
            Ok(text) => {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                self.doc = Document::new(text);
                self.lang = if name.ends_with(".rs") { Lang::Rust } else { Lang::Text };
                self.states = vec![u8::MAX; self.doc.lines()];
                self.hl_valid = 0;
                self.rows.clear();
                self.caret = 0;
                self.scroll = 0.0;
                self.st_lang.set(if self.lang == Lang::Rust { "Rust".into() } else { "Plain Text".into() });
                self.st_msg.set(format!("Opened {name}"));
                self.file = Some(name);
            }
            Err(_) => {
                let name = path.display().to_string();
                self.st_msg.set(format!("Couldn't open {name}"));
            }
        }
        self.refresh();
    }

    /// The editor's `set_selection(at, at)`: the caret there, revealed.
    pub fn set_caret(&mut self, at: usize) {
        let mut at = at.min(self.doc.len());
        while at > 0 && at < self.doc.len() && self.doc.byte(at) & 0xc0 == 0x80 {
            at -= 1;
        }
        self.caret = at;
        self.reveal();
        self.refresh();
    }

    /// Text typed at the caret.
    pub fn type_text(&mut self, s: &str) {
        let line = self.doc.insert(self.caret, s.as_bytes());
        self.caret += s.len();
        if s.contains('\n') {
            self.grow_states(line);
            self.stale_from = self.stale_from.min(line);
        } else {
            self.stale_line = line;
        }
        self.reveal();
        self.refresh();
    }

    /// Lines were added or removed at `line`: the per-line states after it shift (unknown past it).
    fn grow_states(&mut self, line: usize) {
        self.states.resize(self.doc.lines(), u8::MAX);
        self.hl_valid = self.hl_valid.min(line);
    }

    fn delete(&mut self, a: usize, b: usize) {
        let line = self.doc.line_of(a);
        let joined = self.doc.slice(a, b, &mut self.scratch).contains(&b'\n');
        self.doc.delete(a, b);
        self.caret = a;
        if joined {
            self.grow_states(line);
            self.stale_from = self.stale_from.min(line);
        } else {
            self.stale_line = line;
        }
        self.reveal();
        self.refresh();
    }

    fn prev_char(&self, p: usize) -> usize {
        let mut q = p.saturating_sub(1);
        while q > 0 && self.doc.byte(q) & 0xc0 == 0x80 {
            q -= 1;
        }
        q
    }
    fn next_char(&self, p: usize) -> usize {
        let mut q = (p + 1).min(self.doc.len());
        while q < self.doc.len() && self.doc.byte(q) & 0xc0 == 0x80 {
            q += 1;
        }
        q
    }

    /// The editor's keys (it has focus).
    pub fn editor_key(&mut self, e: &Ev) {
        match e.key {
            Some(Key::Char(c)) if !e.mods.cmd && !e.mods.ctrl => {
                let mut b = [0u8; 4];
                self.type_text(c.encode_utf8(&mut b));
            }
            Some(Key::Enter) => self.type_text("\n"),
            Some(Key::Backspace) if self.caret > 0 => {
                let p = self.prev_char(self.caret);
                self.delete(p, self.caret);
            }
            Some(Key::Delete) if self.caret < self.doc.len() => {
                let n = self.next_char(self.caret);
                self.delete(self.caret, n);
            }
            Some(Key::Left) => self.set_caret(self.prev_char(self.caret)),
            Some(Key::Right) => self.set_caret(self.next_char(self.caret)),
            Some(Key::Home) => self.set_caret(self.doc.line_start(self.doc.line_of(self.caret))),
            Some(Key::End) => self.set_caret(self.doc.line_end(self.doc.line_of(self.caret))),
            Some(Key::Up) | Some(Key::Down) => {
                let line = self.doc.line_of(self.caret);
                let col = self.column(line, self.caret);
                let to = if e.key == Some(Key::Up) { line.saturating_sub(1) } else { (line + 1).min(self.doc.lines() - 1) };
                let (a, b) = (self.doc.line_start(to), self.doc.line_end(to));
                let mut p = a;
                for _ in 0..col {
                    if p >= b {
                        break;
                    }
                    p = self.next_char(p);
                }
                self.set_caret(p);
            }
            Some(Key::Escape) if self.find_open => self.close_find(),
            _ => {}
        }
    }

    /// A wheel over the editor: `dy` points down (zgx's `Scroll { delta_y }`).
    pub fn on_wheel(&mut self, _dx: f32, dy: f32) {
        self.scroll = (self.scroll + dy as f64).clamp(0.0, self.max_scroll());
        self.refresh();
    }

    /// `actions.find`: the find bar shown (its input takes focus: the caller's click).
    pub fn open_find(&mut self) {
        self.find_open = true;
        self.find_at.set((0.0, 0.0));
        self.place_find_caret();
        self.refresh();
    }
    /// `actions.close_find`.
    pub fn close_find(&mut self) {
        self.find_open = false;
        self.find_at.set((0.0, AWAY));
        self.find_caret.set((0.0, AWAY));
        self.refresh();
    }
    fn place_find_caret(&mut self) {
        let col = self.query[..self.qcaret].chars().count();
        let x = self.pen(FIND_X + FIND_PAD, col) / self.scale;
        self.find_caret.set((x, 6.0 + (crate::theme::CONTROL_H - LINE_SM) / 2.0));
    }

    /// The find box's keys (it has focus): the query edited, its matches found and marked.
    pub fn find_key(&mut self, e: &Ev) {
        let q = &mut *self.query;
        match e.key {
            Some(Key::Char(c)) if !e.mods.cmd && !e.mods.ctrl => {
                q.insert(self.qcaret, c);
                self.qcaret += c.len_utf8();
            }
            Some(Key::Backspace) if self.qcaret > 0 => {
                let p = q[..self.qcaret].char_indices().next_back().map_or(0, |(i, _)| i);
                q.replace_range(p..self.qcaret, "");
                self.qcaret = p;
            }
            Some(Key::Left) => self.qcaret = q[..self.qcaret].char_indices().next_back().map_or(0, |(i, _)| i),
            Some(Key::Right) => self.qcaret = q[self.qcaret..].chars().next().map_or(self.qcaret, |c| self.qcaret + c.len_utf8()),
            Some(Key::Escape) => return self.close_find(),
            _ => return,
        }
        self.place_find_caret();
        self.mark();
    }

    // ------------------------------------------------------------------ the description

    fn find_bar(&self) -> impl Element<Self> + '_ {
        row((
            row(label(sig!(self.query)).font(SIZE_SM, LINE_SM).color(TEXT_PRIMARY))
                .w(FIND_W)
                .h(CONTROL_H)
                .px(FIND_PAD)
                .items(Align::Center)
                .bg(INPUT_BG)
                .border(1.0, BORDER)
                .rounded(RADIUS_MD)
                .on_key(|c: &mut Code, e: &Ev| c.find_key(e)),
            label(sig!(self.count)).font(SIZE_MD, LINE_MD).color(TEXT_SECONDARY).min_w(90.0),
            button("↑"),
            button("↓"),
            checkbox("Match Case"),
            row(label("Replace").font(SIZE_SM, LINE_SM).color(TEXT_TERTIARY)).w(180.0).h(CONTROL_H).px(8.0).items(Align::Center).bg(INPUT_BG).border(1.0, BORDER).rounded(RADIUS_MD),
            (button("Replace"), button("Replace All"), block(0.0, 0.0).grow(1.0), button("×")),
        ))
        .w(WIDTH)
        .h(FIND_H - 1.0)
        .px(FIND_X)
        .gap(8.0)
        .items(Align::Center)
        .bg(SURFACE_RAISED)
    }

    fn status_bar(&self) -> impl Element<Self> + '_ {
        row((
            label(sig!(self.st_caret)).font(12.0, 16.0).color(TEXT_SECONDARY),
            label(sig!(self.st_msg)).font(12.0, 16.0).color(TEXT_TERTIARY).grow(1.0),
            label(sig!(self.st_lang)).font(12.0, 16.0).color(TEXT_SECONDARY),
            checkbox("Wrap"),
        ))
        .w(WIDTH)
        .h(STATUS_H - 1.0)
        .px(12.0)
        .gap(16.0)
        .items(Align::Center)
        .bg(SURFACE_RAISED)
    }
}

/// One text of the window of rows: a class's colour, or the line numbers.
fn layer<'a>(s: Wired<'a, String, Code>, x: f32, c: Color) -> impl Element<Code> + 'a {
    text(s).at(x, 0.0).size(SIZE).line_height(LINE).color(c)
}

/// A push button of `md`-less text (`ui.button { label }`).
pub fn button<V: 'static>(t: &'static str) -> impl Element<V> {
    row(label(t).font(SIZE_SM, LINE_SM).color(TEXT_PRIMARY)).h(CONTROL_H).px(10.0).items(Align::Center).bg(SURFACE_BASE).rounded(RADIUS_MD)
}
/// `ui.checkbox { label }` (unchecked).
pub fn checkbox<V: 'static>(t: &'static str) -> impl Element<V> {
    row((block(16.0, 16.0).bg(INPUT_BG).border(1.0, BORDER_STRONG).rounded(4.0), label(t).font(SIZE_SM, LINE_SM).color(TEXT_PRIMARY))).gap(6.0).items(Align::Center)
}

impl View for Code {
    const FIXED: bool = true;
    fn view(&self) -> impl Element<Self> + '_ {
        let t = layer;
        let tx = self.text_x();
        let editor = quad(0.0, 0.0, WIDTH, 0.0).bg(BACKGROUND).span(sig!(self.ed_span)).on_key(|c: &mut Code, e: &Ev| c.editor_key(e));
        let screen = clip_by(
            sig!(self.ed_clip),
            |r: &[f32; 4]| *r,
            (
                no_hit(offset(sig!(self.line_at), quad(0.0, 0.0, WIDTH, LINE).bg(HOVER))),
                no_hit(offset(
                    sig!(self.content),
                    (
                        lanes(MARKS, sig!(self.marks), mark_lane),
                        t(sig!(self.layers.numbers), GUTTER_X, TEXT_TERTIARY),
                        (
                            t(sig!(self.layers.plain), tx, CLASS_COLOR[0]),
                            t(sig!(self.layers.keyword), tx, CLASS_COLOR[1]),
                            t(sig!(self.layers.string), tx, CLASS_COLOR[2]),
                            t(sig!(self.layers.number), tx, CLASS_COLOR[3]),
                            t(sig!(self.layers.comment), tx, CLASS_COLOR[4]),
                            t(sig!(self.layers.ty), tx, CLASS_COLOR[5]),
                            t(sig!(self.layers.mac), tx, CLASS_COLOR[6]),
                        ),
                    ),
                )),
                no_hit(offset(sig!(self.caret_at), quad(0.0, 0.0, CARET_W, LINE).bg(ACCENT))),
            ),
        );
        let find = (
            offset(sig!(self.find_at), (self.find_bar(), quad(0.0, FIND_H - 1.0, WIDTH, 1.0).bg(BORDER))),
            no_hit(offset(sig!(self.find_caret), quad(0.0, 0.0, CARET_W, LINE_SM).bg(TEXT_PRIMARY))),
        );
        let status = (quad(0.0, HEIGHT - STATUS_H, WIDTH, 1.0).bg(BORDER), offset(sig!(self.status_at), self.status_bar()));
        (quad(0.0, 0.0, WIDTH, HEIGHT).bg(BACKGROUND), editor, screen, find, status)
    }
}

// ====================================================================== mounting and driving

/// The code editor mounted at zgx's window size (1100 x 760 pt) at scale 2.
pub fn config(font: Vec<u8>) -> Config {
    Config::new(WIDTH, HEIGHT, 2.0, font)
}

/// The editor mounted (`gpu`: drawn by the fused presenter; `ring`: surfaces, 0 the default).
pub fn mount(gpu: bool, ring: usize) -> App<Code> {
    let mono = crate::font_mono();
    let v = Code::new(&mono, 2.0);
    let mut c = config(mono);
    c.ring = ring;
    App::new(v, c, gpu)
}

/// Where a click focuses the editor (its middle), and the find box (its middle).
pub fn editor_point(c: &Code) -> (f32, f32) {
    (WIDTH / 2.0, c.ed_top() + c.ed_h() / 2.0)
}
pub const FIND_POINT: (f32, f32) = (FIND_X + FIND_W / 2.0, 6.0 + CONTROL_H / 2.0);

/// A click (press and release) at (x, y): focus goes to the focusable record under it.
pub fn click<V: View>(app: &mut App<V>, (x, y): (f32, f32)) {
    app.pointer_down(x, y, Button::Primary);
    app.pointer_up(x, y, Button::Primary);
}

/// zgx's `InputEvent::Text`: each char a key to the focused record's route.
pub fn type_keys<V: View>(app: &mut App<V>, s: &str) {
    for ch in s.chars() {
        app.key_down(Key::Char(ch), Mods::default());
    }
}

/// zgx's `Scroll { x, y, delta_y }` over the editor: hit-tested; the editor's wheel runs if it is under (x, y).
pub fn wheel(app: &mut App<Code>, editor: bough_core::Handle, (x, y): (f32, f32), dy: f32) -> bool {
    crate::harness::route_to(app, x, y, editor, |c| c.on_wheel(0.0, dy))
}

/// The editor's record (the interactive record under its middle).
pub fn editor_handle(app: &App<Code>) -> bough_core::Handle {
    let (x, y) = editor_point(&app.owner);
    crate::harness::hit_chain(app, x, y)[0]
}

impl Code {
    /// A new editor holding this one's state (its text, file, caret, scroll, find bar and query): the fresh
    /// mount the pixel gate compares a retained one with.
    pub fn fresh_like(&self, mono: &[u8]) -> Code {
        let mut c = Code::new(mono, self.scale);
        c.doc = Document::new(self.doc.text());
        c.lang = self.lang;
        c.file = self.file.clone();
        c.states = vec![u8::MAX; c.doc.lines()];
        c.rows.clear();
        c.st_lang.set((*self.st_lang).clone());
        c.st_msg.set((*self.st_msg).clone());
        c.caret = self.caret;
        c.scroll = self.scroll;
        c.find_open = self.find_open;
        if c.find_open {
            c.find_at.set((0.0, 0.0));
        }
        c.query.set((*self.query).clone());
        c.qcaret = self.qcaret;
        if c.find_open {
            c.place_find_caret();
        }
        c.search.case = self.search.case;
        c.refresh();
        c
    }
}
