//! The spreadsheet as it is written for the façade (`comparisons/bough-workloads/src/sheet.rs`, branch
//! `r7/bench`), on this core. The differences from that file, all of them:
//!
//! 1. the import line (`bough_x::el::prelude` for `bough::prelude`);
//! 2. a run of children *inside a row* is an array (`std::array::from_fn(|c| cell(r, c))`) where the façade
//!    writes `each((0..COLS).map(..))`: the count of a row's records is in its type until rows can hold runs;
//! 3. the benchmark harness's `Scene` impl is not here (the drivers are `examples/sheet_el_x.rs` and
//!    `tests/sheet_el.rs`).
//!
//! State, handlers, `sig!`, the closures (capturing ones included), the builder chains, `each` for the rows,
//! the scrolled body, the selection's transition: as written there.
#![allow(dead_code)]
use bough_x::el::prelude::*;

pub const COLS: usize = 10;
pub const MAX_ROWS: usize = 10_000;
pub const CELLS: usize = COLS * MAX_ROWS;

pub const CELL_W: f32 = 110.0;
pub const CELL_H: f32 = 22.0;
pub const GUTTER: f32 = 56.0;
pub const TOOLBAR: f32 = 40.0;
pub const HEADER: f32 = 24.0;
const NO_HOVER: u32 = 0xff;
const DARK: u32 = 0x100;

pub struct Sheet {
    pub rows: usize,
    /// Cell values, row-major (`r * COLS + c`); only the first `rows * COLS` are shown.
    pub cells: [Signal<f32>; CELLS],
    /// Column totals (a dependent value the handlers keep).
    pub sums: [Signal<f32>; COLS],
    /// Theme bit (`DARK`) | hovered column (`NO_HOVER` when none).
    pub look: Signal<u32>,
    /// Selected cell index.
    pub sel: Signal<u32>,
    pub scroll: Signal<f32>,
}

fn fill(look: u32, col: u32, row: u32) -> Color {
    let dark = look & DARK != 0;
    let hot = look & 0xff == col;
    match (dark, hot, row % 2 == 0) {
        (false, true, _) => rgb(0xdbe8ff),
        (false, false, true) => rgb(0xffffff),
        (false, false, false) => rgb(0xf6f7f9),
        (true, true, _) => rgb(0x26406b),
        (true, false, true) => rgb(0x15171c),
        (true, false, false) => rgb(0x1b1e24),
    }
}
/// The page right of the cells (a hovered column never reaches it).
fn page(look: u32) -> Color {
    if look & DARK != 0 { rgb(0x15171c) } else { rgb(0xffffff) }
}
fn ink(look: u32) -> Color {
    if look & DARK != 0 { rgb(0xe6e9ef) } else { rgb(0x1d2129) }
}
fn chrome(look: u32) -> Color {
    if look & DARK != 0 { rgb(0x23262d) } else { rgb(0xeceef2) }
}

const VALUE: NumFmt = NumFmt::new("", Some(2), "");

/// The gui-bench spec's data, near enough: deterministic, two decimals.
pub fn value(r: usize, c: usize) -> f32 {
    (((r * 31 + c * 17) * 7919) % 100_000) as f32 / 100.0
}

impl Sheet {
    /// A sheet of `rows` rows (1 to `MAX_ROWS`). Boxed: the cells are 400 KB of the owner.
    pub fn with_rows(rows: usize) -> Box<Sheet> {
        let rows = rows.clamp(1, MAX_ROWS);
        let mut s: Box<Sheet> = {
            // SAFETY: every field is a `Signal` of a plain number (or a `usize`): zero bytes are a valid `Sheet`.
            unsafe { Box::<Sheet>::new_zeroed().assume_init() }
        };
        s.rows = rows;
        for i in 0..CELLS {
            s.cells[i] = Signal::new(value(i / COLS, i % COLS));
        }
        let mut sums = [0.0f32; COLS];
        for (i, c) in s.cells.iter().enumerate().take(rows * COLS) {
            sums[i % COLS] += **c;
        }
        s.sums = sums.map(Signal::new);
        s.look = Signal::new(NO_HOVER);
        s
    }

    pub fn hover_header(&mut self, col: u32, on: bool) {
        let look = *self.look;
        let next = if on { (look & !0xff) | col } else if look & 0xff == col { (look & !0xff) | NO_HOVER } else { look };
        self.look.set(next);
    }
    pub fn toggle_theme(&mut self) {
        let l = *self.look ^ DARK;
        self.look.set(l);
    }
    fn key(&mut self, e: &Ev) {
        if let Some(Key::Char('t')) = e.key {
            self.toggle_theme()
        }
    }
    pub fn edit(&mut self, i: usize, v: f32) {
        let c = i % COLS;
        let old = *self.cells[i];
        self.cells[i].set(v);
        let s = *self.sums[c] + v - old;
        self.sums[c].set(s);
    }
    pub fn select(&mut self, i: u32) {
        self.sel.set(i);
    }
    pub fn scroll_by(&mut self, dy: f32, viewport: f32) {
        let max = (self.rows as f32 * CELL_H - viewport).max(0.0);
        let y = (*self.scroll + dy).clamp(0.0, max);
        self.scroll.set(y);
    }
}

impl View for Sheet {
    fn view(&self) -> impl Element<Self> + '_ {
        let header_cell = move |c: usize| {
            let col = c as u32;
            div()
                .w(CELL_W)
                .h(HEADER)
                .px(6.0)
                .bg(sig!(self.look).map(move |l: &u32| if l & 0xff == col { rgb(0x9db8f0) } else { chrome(*l) }))
                .child(text(COLUMN_NAMES[c]).font(12.0, HEADER).color(sig!(self.look).map(|l: &u32| ink(*l))))
                .on_hover(move |s: &mut Sheet, e: &Ev| s.hover_header(col, e.on))
        };
        let cell = move |r: usize, c: usize| {
            let (row, col) = (r as u32, c as u32);
            div()
                .w(CELL_W)
                .h(CELL_H)
                .px(6.0)
                .bg(sig!(self.look).map(move |l: &u32| fill(*l, col, row)))
                .child(text(sig!(self.cells).at(r * COLS + c).num(|v: &f32| *v, VALUE)).font(12.0, CELL_H).color(sig!(self.look).map(|l: &u32| ink(*l))))
        };
        let data_row = move |r: usize| {
            row((
                div().w(GUTTER).h(CELL_H).px(6.0).bg(sig!(self.look).map(|l: &u32| chrome(*l))).child(text(format!("{}", r + 1)).font(11.0, CELL_H).color(rgb(0x7a808c))),
                row(std::array::from_fn::<_, COLS, _>(move |c| cell(r, c))),
            ))
        };
        let toolbar = row((text("Sheet 1").font(14.0, 20.0), text("Σ totals").font(12.0, 20.0).color(rgb(0x7a808c))))
            .gap(16.0)
            .px(12.0)
            .h(TOOLBAR)
            .items(Align::Center)
            .bg(sig!(self.look).map(|l: &u32| chrome(*l)));
        let header = row((div().w(GUTTER).h(HEADER).bg(sig!(self.look).map(|l: &u32| chrome(*l))), row(std::array::from_fn::<_, COLS, _>(header_cell))));
        let selection = block(CELL_W, CELL_H)
            .border(2.0, rgb(0x2f6bff))
            .abs(GUTTER, 0.0)
            .translate(sig!(self.sel).map(|i: &u32| (((i % COLS as u32) as f32) * CELL_W, ((i / COLS as u32) as f32) * CELL_H)).transition(Curve::out(0.18)));
        // the outline after the rows, so it draws over its cell
        let body = column((column(each((0..self.rows).map(data_row))), selection))
            .relative()
            .grow(1.0)
            .scroll(sig!(self.scroll));
        let footer = row((
            div().w(GUTTER).h(CELL_H).child(text("Σ").font(12.0, CELL_H)),
            row(std::array::from_fn::<_, COLS, _>(move |c| div().w(CELL_W).h(CELL_H).px(6.0).child(text(sig!(self.sums).at(c).num(|v: &f32| *v, VALUE)).font(12.0, CELL_H)))),
        ))
        .bg(sig!(self.look).map(|l: &u32| chrome(*l)));
        column((toolbar, header, body, footer))
            .w_pct(1.0)
            .h_pct(1.0)
            .bg(sig!(self.look).map(|l: &u32| page(*l)))
            // the gui-bench keys (a window host focuses the root): `t` the theme
            .on_key(|s: &mut Sheet, e: &Ev| s.key(e))
    }
}

const COLUMN_NAMES: [&str; COLS] = ["A", "B", "C", "D", "E", "F", "G", "H", "I", "J"];
