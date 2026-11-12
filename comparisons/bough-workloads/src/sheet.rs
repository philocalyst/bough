//! A spreadsheet: 10 columns x 10,000 rows = 100,000 numeric cells (at `SCENE_SCALE` 1), a header row, a row
//! number gutter, a column-totals footer, a selection outline and a toolbar.
//!
//! The fan-out: hovering a column header highlights that column's 10,000 cells, and the theme recolours every
//! cell's fill and text. Both are one root signal (`look`: theme bit and hovered column) that each cell's fill
//! and text colour read through `sig!(self.look).map(..)`, i.e. 200,000 wires on one signal. Steady interaction:
//! a value edit (the cell and its column total change), a selection move, a wheel scroll.
//!
//! Wanted and not expressible today (results, not workarounds):
//! * a cell's colour from two signals (theme and hover) needs them packed into one `u32` by hand: there is no
//!   operand over two signals;
//! * the cells are a `[Signal<f32>; 100_000]` inside the owner, because a wired signal must sit at an
//!   `offset_of!` path (a `Vec` or `Box` of signals cannot be wired); `List<Row>` rows are their own owners and
//!   cannot read the root's `look`;
//! * hovering a column is meant to touch that column's cells only; with the hover packed into `look`, every
//!   cell's two wires run on a hover (the fan-out is all 200,000 wires, of which 20,000 change a pixel).
use crate::{scaled, Rng, Scene};
use bough::prelude::*;
use bough::raw::{App, Caps, Ev, NumFmt};

pub const COLS: usize = 10;
pub const MAX_ROWS: usize = 10_000;
pub const CELLS: usize = COLS * MAX_ROWS;

const CELL_W: f32 = 110.0;
const CELL_H: f32 = 22.0;
const GUTTER: f32 = 56.0;
const TOOLBAR: f32 = 40.0;
const HEADER: f32 = 24.0;
const NO_HOVER: u32 = 0xff;
const DARK: u32 = 0x100;

pub struct Sheet {
    rows: usize,
    /// Cell values, row-major (`r * COLS + c`); only the first `rows * COLS` are shown.
    cells: [Signal<f32>; CELLS],
    /// Column totals (a dependent value the handlers keep).
    sums: [Signal<f32>; COLS],
    /// Theme bit (`DARK`) | hovered column (`NO_HOVER` when none).
    look: Signal<u32>,
    /// Selected cell index.
    sel: Signal<u32>,
    scroll: Signal<f32>,
    rng: Rng,
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
fn ink(look: u32) -> Color {
    if look & DARK != 0 { rgb(0xe6e9ef) } else { rgb(0x1d2129) }
}
fn chrome(look: u32) -> Color {
    if look & DARK != 0 { rgb(0x23262d) } else { rgb(0xeceef2) }
}

const VALUE: NumFmt = NumFmt::new("", Some(2), "");

impl Sheet {
    fn hover_header(&mut self, col: u32, on: bool) {
        let look = *self.look;
        let next = if on { (look & !0xff) | col } else if look & 0xff == col { (look & !0xff) | NO_HOVER } else { look };
        self.look.set(next);
    }
    fn edit(&mut self, i: usize, v: f32) {
        let c = i % COLS;
        let old = *self.cells[i];
        self.cells[i].set(v);
        let s = *self.sums[c] + v - old;
        self.sums[c].set(s);
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
                row(each((0..COLS).map(move |c| cell(r, c)))),
            ))
        };
        let toolbar = row((text("Sheet 1").font(14.0, 20.0), text("Σ totals").font(12.0, 20.0).color(rgb(0x7a808c))))
            .gap(16.0)
            .px(12.0)
            .h(TOOLBAR)
            .items(Align::Center)
            .bg(sig!(self.look).map(|l: &u32| chrome(*l)));
        let header = row((div().w(GUTTER).h(HEADER).bg(sig!(self.look).map(|l: &u32| chrome(*l))), row(each((0..COLS).map(header_cell)))));
        let selection = block(CELL_W, CELL_H)
            .border(2.0, rgb(0x2f6bff))
            .abs(GUTTER, 0.0)
            .translate(sig!(self.sel).map(|i: &u32| (((i % COLS as u32) as f32) * CELL_W, ((i / COLS as u32) as f32) * CELL_H)));
        let body = column((selection, column(each((0..self.rows).map(data_row)))))
            .relative()
            .grow(1.0)
            .scroll(sig!(self.scroll));
        let footer = row((
            div().w(GUTTER).h(CELL_H).child(text("Σ").font(12.0, CELL_H)),
            row(each((0..COLS).map(move |c| div().w(CELL_W).h(CELL_H).px(6.0).child(text(sig!(self.sums).at(c).num(|v: &f32| *v, VALUE)).font(12.0, CELL_H))))),
        ))
        .bg(sig!(self.look).map(|l: &u32| chrome(*l)));
        column((toolbar, header, body, footer)).w_pct(1.0).h_pct(1.0).bg(sig!(self.look).map(|l: &u32| fill(*l, NO_HOVER, 0)))
    }
}

const COLUMN_NAMES: [&str; COLS] = ["A", "B", "C", "D", "E", "F", "G", "H", "I", "J"];

impl Scene for Sheet {
    const NAME: &'static str = "sheet";

    fn build(scale: f32) -> Sheet {
        let rows = scaled(MAX_ROWS, scale, 10).min(MAX_ROWS);
        let mut rng = Rng::new(0x5ee7);
        let cells: [Signal<f32>; CELLS] = std::array::from_fn(|_| Signal::new((rng.below(1_000_000) as f32) / 100.0));
        let mut sums = [0.0f32; COLS];
        for (i, c) in cells.iter().enumerate().take(rows * COLS) {
            sums[i % COLS] += **c;
        }
        Sheet { rows, cells, sums: sums.map(Signal::new), look: Signal::new(NO_HOVER), sel: Signal::new(0), scroll: Signal::new(0.0), rng }
    }

    fn caps(&self) -> Caps {
        let cells = self.rows * COLS;
        // a cell: a fill and up to 9 glyph instances ("99999.99"); a row: its gutter box and label
        let instances = (cells * 10 + self.rows * 6 + 4096).next_power_of_two();
        let records = cells * 2 + self.rows * 4 + 4096;
        Caps {
            instances: instances.min(Caps::RESERVE.instances),
            pages: (records / 32 + 1024).next_power_of_two().min(Caps::RESERVE.pages),
            // measured: ~35 glyph slots per cell (a numeric label's 16-byte slot and its shaping headroom)
            glyphs: (cells * 40 + self.rows * 16 + 65536).next_power_of_two().min(Caps::RESERVE.glyphs),
            runs: (cells + self.rows * 2 + 8192).next_power_of_two().min(Caps::RESERVE.runs),
            ..Caps::DEFAULT
        }
    }

    fn describe(&self) -> String {
        format!("{} rows x {COLS} columns = {} cells", self.rows, self.rows * COLS)
    }

    fn fanout(&self) -> usize {
        self.rows // one column's cells
    }

    fn steady(app: &mut App<Self>, k: usize) {
        match k % 3 {
            0 => app.turn(|s| {
                let i = s.rng.below(s.rows * COLS);
                let v = s.rng.below(1_000_000) as f32 / 100.0;
                s.edit(i, v);
            }),
            1 => app.turn(|s| {
                let n = (s.rows * COLS) as u32;
                let next = (*s.sel + COLS as u32 + 1) % n;
                s.sel.set(next);
            }),
            _ => {
                // a wheel detent over the body, alternating down and up
                let dy = if (k / 3) % 2 == 0 { 3.0 } else { -3.0 };
                app.wheel(GUTTER + 2.5 * CELL_W, TOOLBAR + HEADER + 200.0, 0.0, dy);
            }
        }
    }

    fn hover(app: &mut App<Self>, k: usize) {
        // onto column D's header, then up into the toolbar
        if k % 2 == 0 {
            app.pointer_move(GUTTER + 3.5 * CELL_W, TOOLBAR + HEADER / 2.0);
        } else {
            app.pointer_move(GUTTER + 3.5 * CELL_W, TOOLBAR / 2.0);
        }
    }

    fn theme(app: &mut App<Self>, _k: usize) {
        app.turn(|s| {
            let l = *s.look ^ DARK;
            s.look.set(l);
        });
    }

    fn animate(app: &mut App<Self>, k: usize) {
        // the sheet has no transitions of its own: a selection jump stands in (a paint-only move)
        app.turn(|s| {
            let n = (s.rows * COLS) as u32;
            s.sel.set((k as u32 * 37) % n);
        });
    }
}
