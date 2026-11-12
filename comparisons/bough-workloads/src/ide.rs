//! An IDE: 12 file tabs, a file tree of 2,000 entries, an editor of 3,000 syntax-coloured lines with a gutter,
//! a caret and a current-line band, a minimap (one thin block per line and a viewport box), a terminal of 400
//! lines and a status bar, in a 1280 x 800 pt window (at `SCENE_SCALE` 1).
//!
//! Layout (points): tab bar 36, file tree 240 wide, editor (fills the rest), minimap 110 wide, terminal 200,
//! status bar 22; the middle row grows, so the resize sweeps relayout everything.
//!
//! The fan-out: every identifier span of the editor (~4,300 of the ~14,500 token spans) reads the `look`
//! signal (theme bit | hovered symbol) through `sig!(self.look).map(..)`: pointing at an occurrence of `value`
//! (a route on each identifier span sets the hovered symbol) recolours all of its >= 2,000 occurrences, and
//! the other identifier spans re-evaluate and keep their colour. The theme recolours every token span (identifier
//! spans through `look`, the rest through `theme`), the 2,000 tree rows, the 3,000 minimap blocks, the tabs, the
//! terminal lines and the bars (~25,000 wires on `theme`/`look` in all).
//!
//! Phases:
//! * steady: one turn per step types a character at the cursor (the cursor line's last token is a
//!   `Signal<String>`, the caret and the current-line band follow the `cursor` signal through translate
//!   operands, the status bar shows line and column through `NumFmt`); a new line every 40 characters moves the
//!   cursor to the next line; every 6th step a terminal line is appended and the diagnostics count updates;
//!   every 10th step the editor scrolls by a wheel detent (a trackpad-style 54 pt, `wheel_points`) and the
//!   minimap viewport box follows it through a translate transition. One frame-clock tick ends every step (the
//!   host ticks every frame; without it a transition would never advance during steady use).
//! * hover: the pointer onto the first token of a visible line that starts with `value`, then off it (onto the
//!   status bar).
//! * theme: dark / light.
//! * animate: a tab switch. The editor is covered by a box in its own background colour whose opacity
//!   transitions: the cover fades in (the content fades out), the tab changes under it (active tab, saved
//!   scroll position, cursor line, tree selection, the visible lines' last tokens), the cover fades out. In the
//!   same call a folder of 30 tree entries expands or collapses (height transition on its container, with the
//!   rows' colours fading) and the tree selection and the minimap viewport box move (translate transitions).
//!
//! Wanted and not expressible today (results, not workarounds):
//! * group opacity: `.opacity(op)` multiplies the alpha of the box's own quad only, not its descendants, so the
//!   editor content (16,500 text spans) cannot cross-fade by fading a parent. The cross-fade is a cover box
//!   that fades over the content (fade through the background colour, not a true cross-dissolve of two
//!   contents), and it needs the fade-out to run first: `animate` ticks the frame clock itself between the two
//!   turns, because a transition always starts from the value shown now and the cover has to be opaque before
//!   the tab changes;
//! * clipping a flow box's painted children: `clip_overflow()` only stops layout propagation, nothing clips the
//!   paint, so the collapsing folder (height 0) would still draw its rows over the entries below. The scene fades
//!   the rows' text and icon colours (a Color transition per row) while the container's height transitions;
//! * a conditional element: `each` needs one element type, and identifier spans carry a hover route that other
//!   spans do not. The scene defines its own `Either` element over `Mounter` (L / R forwarded to the chosen
//!   branch), which also refuses sealing (default) so MODE=redescribe re-describes every span;
//! * a text background / highlight operand: the hovered symbol is shown by colour only; a background per span
//!   would need a painted box around each of the ~4,300 identifier spans;
//! * a colour from two signals (theme and hover; theme and selected tab; theme and folder state) needs them
//!   packed into one `u32` by hand (`look`, `chrome`, `fold_look`), and the non-identifier spans read a second
//!   signal (`theme`) the handler keeps in step: there is no operand over two signals and no derived signal;
//! * the minimap's scroll offset and the viewport box are dependent values the handlers keep (`mini_scroll` is
//!   set after every editor scroll the scene makes): nothing says "scroll this view in proportion to that one",
//!   and a scroll the runtime makes itself (a glide in `tick`) would leave the minimap behind;
//! * monospace metrics: the caret's x is `column * 6.6 pt`, an estimate, because a flow row of labels exposes no
//!   glyph positions to read the caret from;
//! * inserting a line: the editor's lines are fixed structure (3,000 rows mounted once), so "start a new line"
//!   moves the cursor to the next existing line and overwrites its last token; a real insert would shift 3,000
//!   rows (a `List`, whose rows cannot read the root's theme and hover signals);
//! * appending a terminal line: the terminal is a fixed array of 528 label signals (400 shown + 128 spare, filled
//!   as lines arrive, the scroll offset kept at the bottom); with MODE=once a mounted description cannot grow;
//! * distinct text per tab: all tabs share the editor's mounted lines (a switch rewrites the visible lines' last
//!   token); 3,000 lines of different text per tab would be 16,500 text signals;
//! * a transition that ignores some writes: the expanding folder's rows read `fold_look` (theme bit | open bit)
//!   through a Color transition, so a theme switch also starts a 0.2 s colour transition on those 60 operands
//!   (they stay mid-fade through the `theme` phase, which never ticks the frame clock); an operand cannot say
//!   "ease this part of the value, snap that part";
//! * a label's box excludes its leading and trailing spaces (a token `"let "` measures as `"let"`): the scene
//!   trims every token and turns its edge spaces into margins (3.8 pt each);
//! * everything is mounted, nothing virtualised: 3,000 editor rows, 2,000 tree rows and 3,000 minimap blocks are
//!   live records (`virtual_list` / `list` rows cannot read the root's signals).
use crate::{scaled, Rng, Scene};
use bough::prelude::*;
use bough::raw::{no_hit, App, Caps, Ev, Mounter, NumFmt};

// ---------------------------------------------------------------------------------------------- geometry
pub const MAX_LINES: usize = 3000;
pub const MAX_TREE: usize = 2000;
pub const MAX_TERM: usize = 400;
const TERM_SPARE: usize = 128;
const TERM_SLOTS: usize = MAX_TERM + TERM_SPARE;
const TABS: usize = 12;

const TAB_H: f32 = 36.0;
const TREE_W: f32 = 240.0;
const MINI_W: f32 = 110.0;
const TERM_H: f32 = 200.0;
const TERM_HEAD: f32 = 24.0;
const STATUS_H: f32 = 22.0;
const MID_H: f32 = 800.0 - TAB_H - TERM_H - STATUS_H;
const LINE_H: f32 = 18.0;
const GUTTER_W: f32 = 44.0;
const TREE_ROW: f32 = 20.0;
const TERM_LINE: f32 = 16.0;
const CHAR_W: f32 = 6.6;
const SPACE_W: f32 = 3.8;
const INDENT: f32 = 14.0;
const FOLD_AT: usize = 12;
const FOLD_N: usize = 30;
const HOT_EVERY: usize = 16;
const HOT_AT: usize = 3;
const WHEEL_PT: f32 = 54.0;

// ---------------------------------------------------------------------------------------------- tokens
const KW: u32 = 0;
const ID: u32 = 1;
const STR: u32 = 2;
const NUM: u32 = 3;
const CMT: u32 = 4;
const PUN: u32 = 5;

const VOCAB: [&str; 24] = [
    "value", "state", "index", "buffer", "result", "config", "item", "node", "count", "self", "len", "name", "path", "data", "text", "range", "cursor", "key", "next", "size", "offset", "event", "frame", "slot",
];
const NUMS: [&str; 10] = ["0", "1", "2", "4", "8", "16", "42", "100", "1024", "0.5"];
const STRS: [&str; 6] = ["\"ok\"", "\"main\"", "\"id\"", "\"/tmp\"", "\"error\"", "\"title\""];
const CMT_A: [&str; 4] = ["// compute", "// TODO:", "// note:", "// fixme:"];
const CMT_B: [&str; 4] = [" the next", " flush the", " cached", " reset"];
const CMT_C: [&str; 4] = [" value", " buffer", " state", " index"];
const ALT_TAIL: [&str; 5] = [";", ");", ",", " {", " // ok"];
const TYPE_CHARS: &[u8] = b"etaoinshrdlu_(){};=. valuestate";

#[derive(Clone, Copy)]
struct Tok {
    class: u8,
    /// Vocabulary index of an identifier.
    sym: u8,
    text: &'static str,
}

struct Line {
    toks: Vec<Tok>,
    indent: f32,
    /// Characters before the last token (the typed tail), indent included, as columns.
    prefix: u32,
    /// Width of this line's minimap block (points).
    mini_w: f32,
    /// The class that owns most of the line's characters (the minimap block's colour).
    dominant: u8,
}

struct Entry {
    name: String,
    depth: u8,
    folder: bool,
    icon: u32,
}

fn tok(class: u32, text: &'static str) -> Tok {
    Tok { class: class as u8, sym: 0, text }
}
fn ident(rng: &mut Rng) -> Tok {
    let s = if rng.below(100) < 50 { 0 } else { 1 + rng.below(VOCAB.len() - 1) };
    Tok { class: ID as u8, sym: s as u8, text: VOCAB[s] }
}

fn gen_line(rng: &mut Rng, i: usize) -> Line {
    let mut t: Vec<Tok> = Vec::with_capacity(8);
    let mut indent = rng.below(4) as f32 * INDENT;
    if i % HOT_EVERY == HOT_AT {
        // the hover phase's target: a line that starts with `value` at a known indent
        indent = INDENT;
        t.push(Tok { class: ID as u8, sym: 0, text: VOCAB[0] });
        t.push(tok(PUN, " += "));
        t.push(tok(NUM, NUMS[rng.below(NUMS.len())]));
        t.push(tok(PUN, ";"));
    } else {
        match rng.below(13) {
            0 => {
                t.extend([tok(KW, "let "), ident(rng), tok(PUN, " = "), ident(rng), tok(PUN, "("), tok(NUM, NUMS[rng.below(NUMS.len())]), tok(PUN, ");")]);
            }
            1 => t.extend([tok(KW, "if "), ident(rng), tok(PUN, " > "), tok(NUM, NUMS[rng.below(NUMS.len())]), tok(PUN, " {")]),
            2 => t.extend([tok(KW, "fn "), ident(rng), tok(PUN, "("), ident(rng), tok(PUN, ": "), ident(rng), tok(PUN, ") {")]),
            3 => t.extend([tok(CMT, CMT_A[rng.below(4)]), tok(CMT, CMT_B[rng.below(4)]), tok(CMT, CMT_C[rng.below(4)])]),
            4 => t.extend([tok(KW, "return "), ident(rng), tok(PUN, ";")]),
            5 => t.extend([ident(rng), tok(PUN, "."), ident(rng), tok(PUN, "("), tok(STR, STRS[rng.below(STRS.len())]), tok(PUN, ");")]),
            6 => t.extend([tok(KW, "match "), ident(rng), tok(PUN, " {")]),
            7 => t.extend([ident(rng), tok(PUN, " => "), tok(STR, STRS[rng.below(STRS.len())]), tok(PUN, ",")]),
            8 => t.extend([tok(KW, "use "), ident(rng), tok(PUN, "::"), ident(rng), tok(PUN, ";")]),
            9 => t.extend([tok(KW, "for "), ident(rng), tok(KW, " in "), tok(NUM, NUMS[rng.below(NUMS.len())]), tok(PUN, ".."), ident(rng), tok(PUN, " {")]),
            10 => t.extend([tok(KW, "while "), ident(rng), tok(PUN, " < "), ident(rng), tok(PUN, " {")]),
            11 => t.extend([ident(rng), tok(PUN, " += "), tok(NUM, NUMS[rng.below(NUMS.len())]), tok(PUN, ";")]),
            _ => t.extend([tok(KW, "let mut "), ident(rng), tok(PUN, " = "), tok(STR, STRS[rng.below(STRS.len())]), tok(PUN, ";")]),
        }
    }
    debug_assert!((3..=8).contains(&t.len()));
    let mut by_class = [0usize; 6];
    for k in &t {
        by_class[k.class as usize] += k.text.len();
    }
    let dominant = (0..6).max_by_key(|&c| (by_class[c], 6 - c)).unwrap() as u8;
    let chars: usize = t.iter().map(|k| k.text.len()).sum();
    let last = t.last().unwrap().text.len();
    let prefix = (indent / CHAR_W).round() as u32 + (chars - last) as u32;
    Line { mini_w: (chars as f32 * 1.5 + indent * 0.3).clamp(4.0, MINI_W - 6.0), toks: t, indent, prefix, dominant }
}

fn gen_tree(rng: &mut Rng, n: usize) -> Vec<Entry> {
    const DIRS: [&str; 10] = ["src", "core", "render", "layout", "text", "input", "tests", "assets", "util", "net"];
    const STEMS: [&str; 12] = ["mod", "lib", "main", "view", "state", "paint", "scene", "theme", "list", "route", "wire", "frame"];
    const EXTS: [(&str, u32); 5] = [("rs", 0xde7a3a), ("toml", 0x9a9a9a), ("md", 0x4fa3d9), ("json", 0xd9c34f), ("wgsl", 0x8a63d2)];
    let mut v = Vec::with_capacity(n);
    let mut depth = 0u8;
    while v.len() < n {
        let r = rng.below(100);
        if r < 20 && depth < 6 {
            v.push(Entry { name: format!("{}_{}", DIRS[rng.below(DIRS.len())], v.len()), depth, folder: true, icon: 0x6b9bd2 });
            depth += 1;
        } else if r < 32 && depth > 0 {
            depth -= 1;
        } else {
            let (ext, icon) = EXTS[rng.below(EXTS.len())];
            v.push(Entry { name: format!("{}_{}.{}", STEMS[rng.below(STEMS.len())], v.len(), ext), depth, folder: false, icon });
        }
    }
    v
}

const TAB_NAMES: [&str; TABS] = ["main.rs", "lib.rs", "view.rs", "paint.rs", "state.rs", "theme.rs", "Cargo.toml", "list.rs", "input.rs", "scene.rs", "layout.rs", "README.md"];

/// A terminal line (kind, text) from its slot number: build output, warnings, test results.
fn term_line(slot: usize) -> (u8, String) {
    let h = (slot as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 33;
    match h % 8 {
        0 => (1, format!("warning: unused variable `value` in src/{}.rs:{}", STEM_T[(h as usize / 8) % 4], 10 + slot % 200)),
        1 => (2, format!("error[E0308]: mismatched types at src/{}.rs:{}", STEM_T[(h as usize / 8) % 4], 3 + slot % 300)),
        2 | 3 => (3, format!("test {}::case_{} ... ok", STEM_T[(h as usize / 8) % 4], slot)),
        _ => (0, format!("   Compiling bough-{} v0.{}.{} ({}/crates/{})", STEM_T[(h as usize / 8) % 4], slot % 9, slot % 17, "/work", slot % 31)),
    }
}
const STEM_T: [&str; 4] = ["layout", "paint", "input", "text"];

// ---------------------------------------------------------------------------------------------- colours
fn pick(t: u32, dark: u32, light: u32) -> Color {
    rgb(if t & 1 == 1 { dark } else { light })
}
fn tok_hex(dark: bool, class: u32) -> u32 {
    match (class, dark) {
        (KW, true) => 0xcf8e6d,
        (KW, false) => 0x0033b3,
        (ID, true) => 0xbcbec4,
        (ID, false) => 0x1b1b1b,
        (STR, true) => 0x6aab73,
        (STR, false) => 0x067d17,
        (NUM, true) => 0x2aacb8,
        (NUM, false) => 0x1750eb,
        (CMT, true) => 0x7a7e85,
        (CMT, false) => 0x8c8c8c,
        (_, true) => 0xa9b7c6,
        (_, false) => 0x4a4a4a,
    }
}
/// A token's colour: `look` = theme bit | (hovered symbol + 1) << 1; `sym` = the span's symbol + 1 (0: not an
/// identifier).
fn tok_color(look: u32, class: u32, sym: u32) -> Color {
    let dark = look & 1 == 1;
    if class == ID && sym != 0 && look >> 1 == sym {
        return rgb(if dark { 0xffc66d } else { 0xc2410c });
    }
    rgb(tok_hex(dark, class))
}
fn mini_color(t: u32, class: u32) -> Color {
    let c = tok_hex(t & 1 == 1, class);
    Color::rgba((c >> 16) as u8, (c >> 8) as u8, c as u8, 0xc0)
}
fn tree_text(t: u32, folder: u32) -> Color {
    match (t & 1 == 1, folder) {
        (true, 1) => rgb(0xdfe1e5),
        (true, _) => rgb(0xbcbec4),
        (false, 1) => rgb(0x1b1b1b),
        (false, _) => rgb(0x383838),
    }
}
fn tree_bg(t: u32, i: usize) -> Color {
    match (t & 1 == 1, i % 2 == 0) {
        (true, true) => rgb(0x2b2d30),
        (true, false) => rgb(0x2e3033),
        (false, true) => rgb(0xf7f8fa),
        (false, false) => rgb(0xf1f2f5),
    }
}
/// Folder-body rows: the text and icon fade with the folder's open bit (`fold_look` bit 1).
fn fold_text(l: u32) -> Color {
    let c = tree_text(l, 0);
    if l >> 1 & 1 == 1 { c } else { Color::rgba(c.0 as u8, (c.0 >> 8) as u8, (c.0 >> 16) as u8, 0) }
}
fn fold_icon(l: u32, hex: u32) -> Color {
    let a = if l >> 1 & 1 == 1 { 255 } else { 0 };
    Color::rgba((hex >> 16) as u8, (hex >> 8) as u8, hex as u8, a)
}
fn tab_bg(c: u32, i: usize) -> Color {
    let active = (c >> 1) as usize == i;
    match (c & 1 == 1, active) {
        (true, true) => rgb(0x1e1f22),
        (true, false) => rgb(0x2b2d30),
        (false, true) => rgb(0xffffff),
        (false, false) => rgb(0xe9eaee),
    }
}
fn tab_fg(c: u32, i: usize) -> Color {
    let active = (c >> 1) as usize == i;
    match (c & 1 == 1, active) {
        (true, true) => rgb(0xffffff),
        (true, false) => rgb(0x9da0a8),
        (false, true) => rgb(0x000000),
        (false, false) => rgb(0x6c707a),
    }
}
fn term_color(t: u32, kind: u32) -> Color {
    let dark = t & 1 == 1;
    rgb(match (kind, dark) {
        (1, true) => 0xe5c07b,
        (1, false) => 0x9a6700,
        (2, true) => 0xf47067,
        (2, false) => 0xc62828,
        (3, true) => 0x6aab73,
        (3, false) => 0x1a7f37,
        (_, true) => 0xb0b3ba,
        (_, false) => 0x333333,
    })
}
const HOVER_TINT: Color = Color::rgba(90, 150, 255, 70);

// ---------------------------------------------------------------------------------------------- conditional element
/// One of two element types (the API has no conditional element; `each` needs one type).
enum Either<A, B> {
    L(A),
    R(B),
}
impl<V, A: Element<V>, B: Element<V>> Element<V> for Either<A, B> {
    fn mount(self, m: &mut Mounter<'_, V>) {
        match self {
            Either::L(a) => a.mount(m),
            Either::R(b) => b.mount(m),
        }
    }
    fn update(self, m: &mut Mounter<'_, V>) {
        match self {
            Either::L(a) => a.update(m),
            Either::R(b) => b.update(m),
        }
    }
}

// ---------------------------------------------------------------------------------------------- the model
pub struct Ide {
    n_lines: usize,
    n_tree: usize,
    n_term: usize,
    lines: Vec<Line>,
    tree: Vec<Entry>,
    term_kind: Vec<u8>,
    ident_spans: usize,
    value_count: usize,
    /// Text size counts for `caps`: characters, labels (text nodes), painted quads, records.
    chars: usize,
    labels: usize,
    quads: usize,

    /// Theme bit.
    theme: Signal<u32>,
    /// Theme bit | (hovered symbol + 1) << 1: what identifier spans read.
    look: Signal<u32>,
    /// Theme bit | active tab << 1: the tab bar.
    chrome: Signal<u32>,
    /// Theme bit | folder open bit << 1: the rows of the expanding folder.
    fold_look: Signal<u32>,
    fold_h: Signal<f32>,
    /// Selected tree entry.
    sel: Signal<u32>,
    /// Cursor (line, column).
    cursor: Signal<(u32, u32)>,
    scroll: Signal<f32>,
    mini_scroll: Signal<f32>,
    tree_scroll: Signal<f32>,
    term_scroll: Signal<f32>,
    /// Opacity of the cover over the editor (the cross-fade).
    fade: Signal<f32>,
    diag: Signal<u32>,
    /// Each line's last token (the typed text lands here).
    tails: [Signal<String>; MAX_LINES],
    term: [Signal<String>; TERM_SLOTS],

    rng: Rng,
    typed: usize,
    cur_line: usize,
    cur_col: usize,
    term_n: usize,
    tab: usize,
    fold_open: bool,
}

impl Ide {
    fn set_hover(&mut self, sym: u32, on: bool) {
        let cur = *self.look >> 1;
        let want = sym + 1;
        let next = if on { want } else if cur == want { 0 } else { cur };
        let l = (*self.look & 1) | next << 1;
        self.look.set(l);
    }
    /// gui-bench's keys (`guibench_spec::ide`): `t` toggles the theme; `e` toggles `EDIT_SUFFIX` on the last token of
    /// line `EDIT_LINE`.
    fn key(&mut self, e: &Ev) {
        use crate::guibench_spec::ide as gs;
        match e.key {
            Some(Key::Char('t')) => {
                let t = *self.theme ^ 1;
                self.set_theme(t);
            }
            Some(Key::Char('e')) if gs::EDIT_LINE < self.n_lines => {
                let l = gs::EDIT_LINE;
                let cur = (*self.tails[l]).clone();
                let next = match cur.strip_suffix(gs::EDIT_SUFFIX) {
                    Some(base) => base.to_string(),
                    None => cur + gs::EDIT_SUFFIX,
                };
                self.tails[l].set(next);
            }
            _ => {}
        }
    }
    fn set_theme(&mut self, t: u32) {
        let (l, c, f) = (*self.look, *self.chrome, *self.fold_look);
        self.theme.set(t);
        self.look.set(l & !1 | t);
        self.chrome.set(c & !1 | t);
        self.fold_look.set(f & !1 | t);
    }
    fn type_char(&mut self) {
        let c = TYPE_CHARS[self.rng.below(TYPE_CHARS.len())] as char;
        let l = self.cur_line;
        self.tails[l].push(c);
        self.typed += 1;
        self.cur_col += 1;
        if self.typed % 40 == 0 {
            let nl = (l + 1) % self.n_lines;
            self.cur_line = nl;
            self.tails[nl].set(String::new());
            self.cur_col = self.lines[nl].prefix as usize;
        }
        self.cursor.set((self.cur_line as u32, self.cur_col as u32));
    }
    fn term_append(&mut self) {
        let n = self.term_n;
        let slot = if n < TERM_SLOTS { n } else { TERM_SLOTS - 1 };
        self.term[slot].set(term_line(slot).1);
        self.term_n = (n + 1).min(TERM_SLOTS);
        self.term_scroll.set(((self.term_n.min(TERM_SLOTS - TERM_SPARE) as f32) * TERM_LINE - (TERM_H - TERM_HEAD)).max(0.0));
        self.diag.set(2 + (n as u32 * 7) % 11);
    }
    fn sync_mini(&mut self) {
        let max_e = (self.n_lines as f32 * LINE_H - MID_H).max(1.0);
        let max_m = (self.n_lines as f32 - MID_H).max(0.0);
        self.mini_scroll.set((*self.scroll / max_e).clamp(0.0, 1.0) * max_m);
    }
    fn switch_tab(&mut self, k: usize) {
        let next = (self.tab + 1) % TABS;
        self.tab = next;
        let c = *self.chrome;
        self.chrome.set((c & 1) | (next as u32) << 1);
        let span = self.n_lines.saturating_sub(40).max(1);
        let first = (next * 997 + k * 131) % span;
        self.scroll.set(first as f32 * LINE_H);
        self.sync_mini();
        // the visible lines' last token differs per tab
        for l in first..(first + 32).min(self.n_lines) {
            self.tails[l].set(ALT_TAIL[(l + next) % ALT_TAIL.len()].to_string());
        }
        self.cur_line = (first + 3).min(self.n_lines - 1);
        self.cur_col = self.lines[self.cur_line].prefix as usize + self.tails[self.cur_line].len();
        self.cursor.set((self.cur_line as u32, self.cur_col as u32));
        self.sel.set(((next * 7 + 3) % FOLD_AT) as u32);
        self.fold_open = !self.fold_open;
        let t = *self.theme;
        self.fold_look.set(t | (self.fold_open as u32) << 1);
        self.fold_h.set(if self.fold_open { FOLD_N as f32 * TREE_ROW } else { 0.0 });
    }
}

const LN: NumFmt = NumFmt::new("Ln ", None, "");
const COL: NumFmt = NumFmt::new("Col ", None, "");
const DIAG: NumFmt = NumFmt::new("", None, " issues");

impl View for Ide {
    fn view(&self) -> impl Element<Self> + '_ {
        // ---- tabs
        let tab = move |i: usize| {
            row((text(TAB_NAMES[i]).font(12.0, 16.0).color(sig!(self.chrome).map(move |c: &u32| tab_fg(*c, i))),))
                .items(Align::Center)
                .px(14.0)
                .h(TAB_H)
                .bg(rgb(0x2b2d30))
                .paint_with(sig!(self.chrome), move |c: &u32| (Color(0), tab_bg(*c, i)))
                .hover_bg(HOVER_TINT)
        };
        let tabbar = row(each((0..TABS).map(tab))).h(TAB_H).bg(sig!(self.theme).map(|t: &u32| pick(*t, 0x2b2d30, 0xe9eaee)));

        // ---- file tree
        let tree_row = move |i: usize| {
            let e = &self.tree[i];
            let folder = e.folder as u32;
            row((
                block(10.0, 10.0).bg(rgb(e.icon)).rounded(2.0),
                text(e.name.as_str()).font(12.0, 16.0).color(sig!(self.theme).map(move |t: &u32| tree_text(*t, folder))),
            ))
            .items(Align::Center)
            .gap(6.0)
            .pad(8.0 + e.depth as f32 * 12.0, 0.0, 4.0, 0.0)
            .h(TREE_ROW)
            .shrink(0.0)
            .bg(rgb(0xf7f8fa))
            .paint_with(sig!(self.theme), move |t: &u32| (Color(0), tree_bg(*t, i)))
            .hover_bg(HOVER_TINT)
        };
        let fold_end = (FOLD_AT + FOLD_N).min(self.n_tree);
        let fold_row = move |i: usize| {
            let e = &self.tree[i];
            let icon = e.icon;
            row((
                block(10.0, 10.0).bg(sig!(self.fold_look).map(move |l: &u32| fold_icon(*l, icon)).transition(Curve::out(0.2))).rounded(2.0),
                text(e.name.as_str()).font(12.0, 16.0).color(sig!(self.fold_look).map(|l: &u32| fold_text(*l)).transition(Curve::out(0.2))),
            ))
            .items(Align::Center)
            .gap(6.0)
            .pad(8.0 + e.depth as f32 * 12.0, 0.0, 4.0, 0.0)
            .h(TREE_ROW)
            .shrink(0.0)
        };
        let sel_band = no_hit(
            block(TREE_W, TREE_ROW)
                .bg(Color::rgba(70, 130, 255, 56))
                .abs(0.0, 0.0)
                .translate(sig!(self.sel).map(|s: &u32| (0.0, *s as f32 * TREE_ROW)).transition(Curve::out(0.15))),
        );
        let tree = column((
            each((0..FOLD_AT.min(self.n_tree)).map(tree_row)),
            column(each((FOLD_AT.min(self.n_tree)..fold_end).map(fold_row))).h_op(sig!(self.fold_h).transition(Curve::out(0.25))),
            each((fold_end..self.n_tree).map(tree_row)),
            sel_band,
        ))
        .relative()
        .w(TREE_W)
        .bg(sig!(self.theme).map(|t: &u32| pick(*t, 0x2b2d30, 0xf7f8fa)))
        .scroll(sig!(self.tree_scroll))
        .line(TREE_ROW);

        // ---- editor
        let span = move |t: &Tok, indent: f32| {
            let is_id = t.class as u32 == ID;
            let w = if is_id { sig!(self.look) } else { sig!(self.theme) };
            let (class, sym) = (t.class as u32, if is_id { t.sym as u32 + 1 } else { 0 });
            // a label's box excludes its edge spaces: they become margins
            let (lead, trail) = (t.text.len() - t.text.trim_start().len(), t.text.len() - t.text.trim_end().len());
            let p = text(t.text.trim())
                .font(12.0, LINE_H)
                .margin(indent + lead as f32 * SPACE_W, 0.0, trail as f32 * SPACE_W, 0.0)
                .color(w.map(move |l: &u32| tok_color(*l, class, sym)));
            if is_id {
                Either::R(p.on_hover(move |s: &mut Ide, e: &Ev| s.set_hover(sym - 1, e.on)))
            } else {
                Either::L(p)
            }
        };
        let line = move |i: usize| {
            let ln = &self.lines[i];
            let n = ln.toks.len();
            let lc = ln.toks[n - 1].class as u32;
            let lead = ln.toks[n - 1].text.len() - ln.toks[n - 1].text.trim_start().len();
            row((
                text(i as u32 + 1).font(11.0, LINE_H).w(GUTTER_W).color(sig!(self.theme).map(|t: &u32| pick(*t, 0x6f737a, 0xa0a0a0))),
                each(ln.toks[..n - 1].iter().enumerate().map(move |(j, t)| span(t, if j == 0 { ln.indent } else { 0.0 }))),
                text(sig!(self.tails).at(i)).font(12.0, LINE_H).margin(lead as f32 * SPACE_W, 0.0, 0.0, 0.0).color(sig!(self.theme).map(move |t: &u32| tok_color(*t, lc, 0))),
            ))
            .h(LINE_H)
        };
        let cur_band = no_hit(
            block(1600.0, LINE_H)
                .bg(sig!(self.theme).map(|t: &u32| if *t & 1 == 1 { Color::rgba(255, 255, 255, 14) } else { Color::rgba(0, 0, 0, 12) }))
                .abs(0.0, 0.0)
                .translate(sig!(self.cursor).map(|c: &(u32, u32)| (0.0, c.0 as f32 * LINE_H))),
        );
        let caret = no_hit(
            block(2.0, LINE_H - 2.0)
                .bg(sig!(self.theme).map(|t: &u32| pick(*t, 0xffffff, 0x000000)))
                .abs(GUTTER_W, 1.0)
                .translate(sig!(self.cursor).map(|c: &(u32, u32)| (c.1 as f32 * CHAR_W, c.0 as f32 * LINE_H))),
        );
        let editor = column((column(each((0..self.n_lines).map(line))), cur_band, caret))
            .relative()
            .grow(1.0)
            .bg(sig!(self.theme).map(|t: &u32| pick(*t, 0x1e1f22, 0xffffff)))
            .scroll(sig!(self.scroll))
            .line(LINE_H);
        // the cross-fade cover (the editor's background colour, opacity transitions)
        let cover = no_hit(
            block(10.0, 10.0)
                .w_pct(1.0)
                .h_pct(1.0)
                .bg(sig!(self.theme).map(|t: &u32| pick(*t, 0x1e1f22, 0xffffff)))
                .abs(0.0, 0.0)
                .opacity(sig!(self.fade).transition(Curve::out(0.12))),
        );
        let editor_wrap = column((editor, cover)).relative().grow(1.0);

        // ---- minimap
        let mini_block = move |i: usize| {
            let ln = &self.lines[i];
            let class = ln.dominant as u32;
            block(ln.mini_w, 1.0).bg(sig!(self.theme).map(move |t: &u32| mini_color(*t, class)))
        };
        let viewport = no_hit(
            block(MINI_W, MID_H / LINE_H)
                .bg(sig!(self.theme).map(|t: &u32| if *t & 1 == 1 { Color::rgba(255, 255, 255, 40) } else { Color::rgba(0, 0, 0, 36) }))
                .abs(0.0, 0.0)
                .translate(sig!(self.scroll).map(|s: &f32| (0.0, *s / LINE_H)).transition(Curve::out(0.12))),
        );
        let mini = column((column(each((0..self.n_lines).map(mini_block))), viewport))
            .relative()
            .w(MINI_W)
            .bg(sig!(self.theme).map(|t: &u32| pick(*t, 0x25272a, 0xf4f5f7)))
            .scroll(sig!(self.mini_scroll));

        // ---- terminal
        let term_row = move |i: usize| {
            let kind = self.term_kind[i] as u32;
            text(sig!(self.term).at(i)).font(11.0, TERM_LINE).color(sig!(self.theme).map(move |t: &u32| term_color(*t, kind)))
        };
        let terminal = column((
            row((
                text("TERMINAL").font(11.0, 16.0).color(sig!(self.theme).map(|t: &u32| pick(*t, 0xdfe1e5, 0x1b1b1b))),
                text("cargo test --workspace").font(11.0, 16.0).color(sig!(self.theme).map(|t: &u32| pick(*t, 0x7a7e85, 0x8c8c8c))),
            ))
            .gap(14.0)
            .px(10.0)
            .items(Align::Center)
            .h(TERM_HEAD),
            column(each((0..TERM_SLOTS).map(term_row))).px(10.0).grow(1.0).bg(sig!(self.theme).map(|t: &u32| pick(*t, 0x1e1f22, 0xffffff))).scroll(sig!(self.term_scroll)).line(TERM_LINE),
        ))
        .h(TERM_H)
        .bg(sig!(self.theme).map(|t: &u32| pick(*t, 0x2b2d30, 0xe9eaee)));

        // ---- status bar
        let dim = |t: &u32| pick(*t, 0xb0b3ba, 0x444444);
        let status = row((
            text("main").font(11.0, 16.0).color(sig!(self.theme).map(dim)),
            text(sig!(self.cursor).num(|c: &(u32, u32)| (c.0 + 1) as f32, LN)).font(11.0, 16.0).color(sig!(self.theme).map(dim)),
            text(sig!(self.cursor).num(|c: &(u32, u32)| (c.1 + 1) as f32, COL)).font(11.0, 16.0).color(sig!(self.theme).map(dim)),
            text(sig!(self.diag).num(|d: &u32| *d as f32, DIAG)).font(11.0, 16.0).color(sig!(self.theme).map(dim)),
        ))
        .gap(18.0)
        .px(10.0)
        .items(Align::Center)
        .h(STATUS_H)
        .bg(sig!(self.theme).map(|t: &u32| pick(*t, 0x2b2d30, 0xe0e2e8)));

        column((tabbar, row((tree, editor_wrap, mini)).grow(1.0), terminal, status))
            .w_pct(1.0)
            .h_pct(1.0)
            .bg(sig!(self.theme).map(|t: &u32| pick(*t, 0x1e1f22, 0xffffff)))
            // the gui-bench keys (a window host focuses the root): `t` the theme, `e` the spec's edit
            .on_key(|s: &mut Ide, e: &Ev| s.key(e))
    }
}

impl Scene for Ide {
    const NAME: &'static str = "ide";

    fn build(scale: f32) -> Ide {
        let n_lines = scaled(MAX_LINES, scale, 48).min(MAX_LINES);
        let n_tree = scaled(MAX_TREE, scale, 100).min(MAX_TREE);
        let n_term = scaled(MAX_TERM, scale, 24).min(MAX_TERM);
        let mut rng = Rng::new(0x1de);
        let lines: Vec<Line> = (0..n_lines).map(|i| gen_line(&mut rng, i)).collect();
        let tree = gen_tree(&mut rng, n_tree);
        let term_kind: Vec<u8> = (0..TERM_SLOTS).map(|s| term_line(s).0).collect();
        let (mut ident_spans, mut value_count, mut chars, mut labels) = (0usize, 0usize, 0usize, 0usize);
        for l in &lines {
            for t in &l.toks {
                chars += t.text.len();
                labels += 1;
                if t.class as u32 == ID {
                    ident_spans += 1;
                    value_count += (t.sym == 0) as usize;
                }
            }
            chars += 4; // line number
            labels += 1;
        }
        chars += tree.iter().map(|e| e.name.len()).sum::<usize>();
        labels += n_tree;
        chars += (0..n_term).map(|s| term_line(s).1.len()).sum::<usize>();
        labels += TERM_SLOTS;
        chars += TERM_SPARE * 52 + TABS * 10 + 200;
        labels += TABS + 8;
        let quads = n_lines + n_tree * 2 + TABS + 16;
        let tails: [Signal<String>; MAX_LINES] = std::array::from_fn(|i| Signal::new(lines.get(i).map(|l| l.toks.last().unwrap().text.to_string()).unwrap_or_default()));
        let term: [Signal<String>; TERM_SLOTS] = std::array::from_fn(|s| Signal::new(if s < n_term { term_line(s).1 } else { String::new() }));
        let cur_line = 5.min(n_lines - 1);
        let cur_col = lines[cur_line].prefix as usize + lines[cur_line].toks.last().unwrap().text.len();
        Ide {
            n_lines,
            n_tree,
            n_term,
            lines,
            tree,
            term_kind,
            ident_spans,
            value_count,
            chars,
            labels,
            quads,
            theme: Signal::new(0),
            look: Signal::new(0),
            chrome: Signal::new(0),
            fold_look: Signal::new(0),
            fold_h: Signal::new(0.0),
            sel: Signal::new(4),
            cursor: Signal::new((cur_line as u32, cur_col as u32)),
            scroll: Signal::new(0.0),
            mini_scroll: Signal::new(0.0),
            tree_scroll: Signal::new(0.0),
            term_scroll: Signal::new(((n_term as f32) * TERM_LINE - (TERM_H - TERM_HEAD)).max(0.0)),
            fade: Signal::new(0.0),
            diag: Signal::new(3),
            tails,
            term,
            rng,
            typed: 0,
            cur_line,
            cur_col,
            term_n: n_term,
            tab: 0,
            fold_open: false,
        }
    }

    fn caps(&self) -> Caps {
        let instances = (self.chars + self.quads + 16_384).next_power_of_two();
        let records = self.labels + self.n_lines * 2 + self.quads + 4096;
        Caps {
            instances: instances.min(Caps::RESERVE.instances),
            pages: (records / 32 + 1024).next_power_of_two().min(Caps::RESERVE.pages),
            // a label's glyph slots are allocated in blocks of 16 and double when it grows: ~20 K labels need ~32 slots each
            glyphs: (self.chars + self.labels * 32 + 65_536).next_power_of_two().min(Caps::RESERVE.glyphs),
            runs: (self.labels + 8192).next_power_of_two().min(Caps::RESERVE.runs),
            ..Caps::DEFAULT
        }
    }

    fn describe(&self) -> String {
        format!(
            "{} tabs, {} tree entries, {} editor lines ({} token spans, {} identifier spans, {} occurrences of `value`), minimap {} blocks, terminal {}+{} lines; ~{} glyphs, ~{} text nodes",
            TABS,
            self.n_tree,
            self.n_lines,
            self.lines.iter().map(|l| l.toks.len()).sum::<usize>(),
            self.ident_spans,
            self.value_count,
            self.n_lines,
            self.n_term,
            TERM_SPARE,
            self.chars,
            self.labels
        )
    }

    fn fanout(&self) -> usize {
        self.ident_spans
    }

    fn steady(app: &mut App<Self>, k: usize) {
        app.turn(|s| {
            s.type_char();
            if k % 6 == 5 {
                s.term_append();
            }
        });
        if k % 10 == 9 {
            let dy = if (k / 10) % 2 == 0 { WHEEL_PT } else { -WHEEL_PT };
            app.wheel_points(TREE_W + 300.0, TAB_H + 200.0, dy);
            app.turn(|s| s.sync_mini());
        }
        app.tick(1.0 / 60.0);
    }

    fn hover(app: &mut App<Self>, k: usize) {
        if k % 2 == 0 {
            // the first `value`-led line fully below the editor's top edge
            let scroll = *app.owner.scroll;
            let top = (scroll / LINE_H).ceil() as usize + 1;
            let mut i = top - top % HOT_EVERY + HOT_AT;
            if i < top {
                i += HOT_EVERY;
            }
            if i >= app.owner.n_lines {
                i = HOT_AT;
            }
            let x = TREE_W + GUTTER_W + INDENT + 8.0;
            let y = TAB_H + i as f32 * LINE_H - scroll + LINE_H / 2.0;
            app.pointer_move(x, y);
        } else {
            app.pointer_move(640.0, 800.0 - STATUS_H / 2.0);
        }
    }

    fn theme(app: &mut App<Self>, k: usize) {
        app.turn(|s| s.set_theme((k % 2 == 0) as u32));
    }

    fn animate(app: &mut App<Self>, k: usize) {
        // fade the editor out, change the tab under the cover, fade it back in
        app.turn(|s| {
            s.fade.set(1.0);
        });
        for _ in 0..8 {
            app.tick(1.0 / 60.0);
        }
        app.turn(|s| {
            s.switch_tab(k);
            s.fade.set(0.0);
        });
    }
}
