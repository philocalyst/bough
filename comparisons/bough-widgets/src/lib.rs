//! zgx's `widget_bench` and `callback_mount_bench` (crates/zgx/examples), ported onto bough.
//!
//! zgx's `button()` / `text_input()` are themed widgets (`WidgetStyles` in `compose.rs`, tokens of
//! `themes/zgx-dark.json`); bough has layout boxes with a hover background (`hover_bg`) and an absolutely
//! positioned `text_field`. This crate reproduces zgx's default look with them:
//!
//! * `button()`: a row, `px 12 py 8 gap 6`, items centred, `button.bg` `#232e40`, radius 6, hover `#2f4058`
//!   (`button.hover` = `over(#232e40, #3b527080)`), its text 16 pt on a 22 pt line (`font.size.md` /
//!   `font.line.md`), colour `#e5edf7`. `.bg(#2a3a50)` overrides the rest colour, the hover colour is the same.
//! * `text_input()`: 160 x 36, `input.bg` `#232e40`, radius 6, 1 px border `#2f3d52`, text 16 pt, padding 8.
//!   Its first argument ("Name") is the editor's *accessible name*, never drawn (zgx draws a placeholder only
//!   after `.placeholder(..)`), so nothing is drawn for it here either.
//!
//! `text_field` is a quad world element (`rect(x, y, w, h)` in its space's coordinates); it cannot be a
//! layout leaf. A row is therefore a layout group (buttons + a 160 x 36 `block` reserving the field's slot)
//! and the field a sibling quad at the slot's position inside the row's own space; the slot's x is the
//! sum of the two buttons' laid-out widths, taken from a throwaway layout of the label texts (`Probe`).
//! Rows are `List<Row>` items: each owns its `Signal<String>` (zgx: 500 signals) at a stable address, which
//! is how `sig!` can name a per-row field.
use bough::layout::prelude::*;
use bough::prelude::*;

pub fn font() -> Vec<u8> {
    zgx_workload::font_file().unwrap_or_else(|| zgx_workload::FONT_BYTES.to_vec())
}

pub const fn rgbc(v: u32) -> Color {
    Color::rgba((v >> 16) as u8, (v >> 8) as u8, v as u8, 255)
}

pub const BUTTON_BG: Color = rgbc(0x232e40);
pub const STYLED_BG: Color = rgbc(0x2a3a50);
/// `over(#232e40, #3b527080)`.
pub const BUTTON_HOVER: Color = rgbc(0x2f4058);
pub const INPUT_BG: Color = rgbc(0x232e40);
pub const BORDER: Color = rgbc(0x2f3d52);
pub const TEXT: Color = rgbc(0xe5edf7);
pub const FIELD_W: f32 = 160.0;
pub const FIELD_H: f32 = 36.0;
pub const ROW_GAP: f32 = 8.0;

/// A config whose columns fit `rows` rows of about `recs` records / `glyphs` glyphs / `spaces` spaces.
pub fn config(w: f32, h: f32, rows: usize, recs: usize, glyphs: usize) -> Config {
    let mut cfg = Config::new(w, h, 1.0, font());
    let d = Caps::DEFAULT;
    let n = rows.max(1);
    cfg.caps = Caps {
        instances: d.instances.max((n * recs * 2).next_power_of_two()),
        pages: d.pages.max(n * recs * 2 / 32 + 64),
        glyphs: d.glyphs.max((n * glyphs * 2).next_power_of_two()),
        runs: d.runs.max(n * recs * 2),
        spaces: d.spaces.max(n * 2 + 64),
        ..d
    };
    cfg
}

/// zgx's `button().child(text(..))` with `bg` as its rest colour.
pub fn button<V: 'static>(bg: Color, s: impl TextSrc<V>) -> impl Element<V> {
    row(label(s).font(16.0, 22.0).color(TEXT)).px(12.0).py(8.0).gap(6.0).items(Align::Center).bg(bg).rounded(6.0).hover_bg(BUTTON_HOVER)
}

// ====================================================================================== widget_bench

/// One row of widget_bench: `Button {i}`, `Styled` and a text input.
pub struct Row {
    pub i: usize,
    /// The input's x in the row's space (the buttons' widths and gaps).
    pub x: f32,
    pub value: Signal<String>,
}

impl View for Row {
    const FIXED: bool = true;
    fn view(&self) -> impl Element<Self> + '_ {
        (
            row((button(BUTTON_BG, format!("Button {}", self.i)), button(STYLED_BG, "Styled"), block(FIELD_W, FIELD_H))).gap(ROW_GAP),
            text_field(sig!(self.value)).rect(self.x, 0.0, FIELD_W, FIELD_H).font(16.0).colors(INPUT_BG, BORDER, TEXT).rounded(6.0),
        )
    }
}

pub struct Widgets {
    pub rows: List<Row>,
}

impl Widgets {
    /// `n` rows. `xs` is the input x for each digit count (`field_xs`).
    pub fn new(n: usize, xs: &[f32]) -> Widgets {
        Widgets { rows: List::from_iter_in((0..n).map(|i| Row { i, x: xs[digits(i) - 1], value: Signal::new(String::new()) })) }
    }
}

impl View for Widgets {
    const FIXED: bool = true;
    fn view(&self) -> impl Element<Self> + '_ {
        list(sig!(self.rows))
    }
}

pub fn digits(mut i: usize) -> usize {
    let mut d = 1;
    while i >= 10 {
        i /= 10;
        d += 1;
    }
    d
}

/// Laid-out widths of one-line labels (a throwaway layout: the same shaper and sizes the rows use).
struct Probe {
    texts: Vec<String>,
}
impl View for Probe {
    const FIXED: bool = true;
    fn view(&self) -> impl Element<Self> + '_ {
        column(each(self.texts.iter().map(|t| label(t.clone()).font(16.0, 22.0))))
    }
}

/// The input's x for rows of 1, 2, .. `digits(n - 1)` digits: `px 12 + text + px 12`, gap 8, the same for
/// the `Styled` button, gap 8. (Digits of one font are equally wide, so one label per digit count.)
pub fn field_xs(n: usize) -> Vec<f32> {
    let classes = digits(n.saturating_sub(1).max(1));
    let mut texts: Vec<String> = (0..classes).map(|d| format!("Button {}", if d == 0 { 0 } else { 10usize.pow(d as u32) })).collect();
    texts.push("Styled".to_string());
    let mut a = App::new(Probe { texts }, Config::new(1200.0, 800.0, 1.0, font()), false);
    a.frame();
    let root = a.rt.lay.first_root();
    let w = |k: usize| a.rt.lay_child_rect(root, k).2;
    let styled = w(classes);
    (0..classes).map(|d| (12.0 + w(d) + 12.0) + ROW_GAP + (12.0 + styled + 12.0) + ROW_GAP).collect()
}

/// Rows are 38 pt tall (button: 8 + 22 + 8).
pub const ROW_H: f32 = 38.0;

// ============================================================================ callback_mount_bench

/// One row of callback_mount_bench: a row (gap 8) of a 160 pt input, its height `28 + width * 0`.
pub struct CRow {
    pub value: Signal<String>,
}

pub const CALLBACK_H: f32 = 28.0;

impl View for CRow {
    const FIXED: bool = true;
    fn view(&self) -> impl Element<Self> + '_ {
        (
            row(block(FIELD_W, FIELD_H)).gap(ROW_GAP).h(CALLBACK_H),
            text_field(sig!(self.value)).rect(0.0, 0.0, FIELD_W, FIELD_H).font(16.0).colors(INPUT_BG, BORDER, TEXT).rounded(6.0),
        )
    }
}

pub struct CallbackRows {
    pub rows: List<CRow>,
}

impl CallbackRows {
    pub fn new(n: usize) -> CallbackRows {
        CallbackRows { rows: List::from_iter_in((0..n).map(|_| CRow { value: Signal::new(String::new()) })) }
    }
}

impl View for CallbackRows {
    const FIXED: bool = true;
    fn view(&self) -> impl Element<Self> + '_ {
        list(sig!(self.rows))
    }
}

/// zgx's hover sweep: the pointer at x = 20, y = `4 + (step % 20) * 36`.
pub fn hover_y(step: usize) -> f32 {
    4.0 + (step % 20) as f32 * 36.0
}
