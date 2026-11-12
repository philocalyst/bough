//! The sheet of `comparisons/bough-workloads/src/sheet.rs` (r7/bench) in bough-x: a toolbar, a header of ten
//! hoverable cells, `rows` rows of (gutter, row number, ten cells), a selection outline, a footer of ten totals.
//! Device pixels at scale 2 (the workload's window: 1280 x 800 points).
//!
//! State is plain data. The Frame-stage part is words of the block (theme, hovered column, selected cell, scroll
//! offset, the ten totals); the cells are a relation of rows. Handlers are ordinary methods on `&mut Sheet`.
#![allow(dead_code)]
use bough_x::prelude::*;

pub const COLS: usize = 10;
pub const W: i32 = 2560;
pub const H: i32 = 1600;
pub const CELL_W: i32 = 220;
pub const CELL_H: i32 = 44;
pub const GUTTER: i32 = 112;
pub const TOOLBAR: i32 = 80;
pub const HEADER: i32 = 48;
pub const NO_HOVER: u32 = 0xff;
pub const BODY: Rect = Rect { x0: 0, y0: TOOLBAR + HEADER, x1: W, y1: H - CELL_H };

lens!(pub Theme: u32 = 0);
lens!(pub Hover: u32 = 1);
lens!(pub Sel: u32 = 2);
lens!(pub Scroll: f32 = 3);
lens!(pub Sum[COLS]: f32 = 4);
pub const NWORDS: usize = 4 + COLS;

pub struct Sheet {
    pub u: U,
    pub rows: Rel<[f32; COLS]>,
}

fn fill((dark, hot, k): (u32, u32, u32)) -> Color {
    let (col, even) = (k & 0xff, k >> 8 == 0);
    Color(match (dark != 0, hot == col, even) {
        (false, true, _) => 0xdbe8ff,
        (false, false, true) => 0xffffff,
        (false, false, false) => 0xf6f7f9,
        (true, true, _) => 0x26406b,
        (true, false, true) => 0x15171c,
        (true, false, false) => 0x1b1e24,
    })
}
fn page(dark: u32) -> Color {
    Color(if dark != 0 { 0x15171c } else { 0xffffff })
}
fn ink(dark: u32) -> Color {
    Color(if dark != 0 { 0xe6e9ef } else { 0x1d2129 })
}
fn chrome(dark: u32) -> Color {
    Color(if dark != 0 { 0x23262d } else { 0xeceef2 })
}
fn head((dark, hot, col): (u32, u32, u32)) -> Color {
    if hot == col { Color(0x9db8f0) } else { chrome(dark) }
}
fn sel_at(i: u32) -> (i32, i32) {
    ((i % COLS as u32) as i32 * CELL_W, (i / COLS as u32) as i32 * CELL_H)
}

/// The gui-bench spec's data, near enough: deterministic, two decimals.
pub fn value(r: usize, c: usize) -> f32 {
    (((r * 31 + c * 17) * 7919) % 100_000) as f32 / 100.0
}

const NAMES: [&str; COLS] = ["A", "B", "C", "D", "E", "F", "G", "H", "I", "J"];

impl Sheet {
    pub fn new(rows: usize) -> Sheet {
        let items: Vec<[f32; COLS]> = (0..rows).map(|r| std::array::from_fn(|c| value(r, c))).collect();
        let mut u = U::default();
        u.set::<Hover>(NO_HOVER);
        for c in 0..COLS {
            u.w[Sum::BASE + c] = items.iter().map(|row| row[c]).sum::<f32>().to_bits();
        }
        Sheet { u, rows: Rel::new(items) }
    }
    pub fn hover_header(&mut self, col: u32, on: bool) {
        if on {
            self.u.set::<Hover>(col)
        } else if self.u.get::<Hover>() == col {
            self.u.set::<Hover>(NO_HOVER)
        }
    }
    pub fn toggle_theme(&mut self) {
        let t = self.u.get::<Theme>() ^ 1;
        self.u.set::<Theme>(t);
    }
    pub fn edit(&mut self, r: usize, c: usize, v: f32) {
        let cell = &mut self.rows.edit(r)[c];
        let old = core::mem::replace(cell, v);
        let s = f32::from_bits(self.u.w[Sum::BASE + c]) + v - old;
        self.u.w[Sum::BASE + c] = s.to_bits();
    }
    pub fn select(&mut self, i: u32) {
        self.u.set::<Sel>(i);
    }
    pub fn scroll_by(&mut self, dy: f32) {
        let max = (self.rows.len() as i32 * CELL_H - (BODY.y1 - BODY.y0)).max(0) as f32;
        let y = (self.u.get::<Scroll>() + dy).clamp(0.0, max);
        self.u.set::<Scroll>(y);
    }
    pub fn insert_row(&mut self, at: usize, row: [f32; COLS]) {
        for c in 0..COLS {
            self.u.w[Sum::BASE + c] = (f32::from_bits(self.u.w[Sum::BASE + c]) + row[c]).to_bits();
        }
        self.rows.insert(at, row);
    }
    pub fn remove_row(&mut self, at: usize) {
        let row = self.rows.remove(at);
        for c in 0..COLS {
            self.u.w[Sum::BASE + c] = (f32::from_bits(self.u.w[Sum::BASE + c]) - row[c]).to_bits();
        }
        self.scroll_by(0.0);
    }
}

impl View for Sheet {
    fn block(&self) -> &U {
        &self.u
    }
    fn view<S: Sink<Self>>(&self, s: &mut S) {
        let theme = un::<Theme>();
        s.emit(Rect::new(0, 0, W, H), Fill(theme.map(page)));
        // toolbar
        s.emit(Rect::new(0, 0, W, TOOLBAR), Fill(theme.map(chrome)));
        s.emit(Rect::new(24, 20, 200, 40), Label(Txt::new("Sheet 1"), theme.map(ink)));
        s.emit(Rect::new(256, 20, 200, 40), Label(Txt::new("Σ totals"), Kc::<0x7a808c>));
        // header: hovering a cell highlights its column
        s.emit(Rect::new(0, TOOLBAR, GUTTER, HEADER), Fill(theme.map(chrome)));
        for c in 0..COLS {
            let r = Rect::new(GUTTER + c as i32 * CELL_W, TOOLBAR, CELL_W, HEADER);
            s.emit_hover(r, Fill((theme, un::<Hover>(), rec(c as u32)).map(head)), |v: &mut Sheet, a, on| v.hover_header(a.0.0.2.0, on));
            s.emit(r, Label(Txt::new(NAMES[c]), theme.map(ink)));
        }
        // body: the rows scroll under the header; the selection outline rides above them
        s.scope(Space { clip: BODY, base: (0, BODY.y0), scroll: Scroll::SLOT as u8 }, |s| {
            s.list(&List { n: self.rows.len() as u32, row_h: CELL_H, x: 0, y: 0, edits: &self.rows.journal }, |r| {
                let vals = self.rows.get(r);
                (
                    At(Rect::new(0, 0, GUTTER, CELL_H), Fill(theme.map(chrome))),
                    At(Rect::new(0, 0, GUTTER, CELL_H), Num::<0, _, _>(rec((r + 1) as f32), Kc::<0x7a808c>)),
                    std::array::from_fn::<_, COLS, _>(|c| {
                        let at = Rect::new(GUTTER + c as i32 * CELL_W, 0, CELL_W, CELL_H);
                        (
                            At(at, Fill((theme, un::<Hover>(), rec(c as u32 | ((r as u32 & 1) << 8))).map(fill))),
                            At(at, Num::<2, _, _>(rec(vals[c]), theme.map(ink))),
                        )
                    }),
                )
            });
            s.emit(Rect::new(GUTTER, 0, CELL_W, CELL_H), Outline::<4, _, _>(Kc::<0x2f6bff>, un::<Sel>().map(sel_at)));
        });
        // footer: the totals are words of the block, formatted while shading
        s.emit(Rect::new(0, H - CELL_H, W, CELL_H), Fill(theme.map(chrome)));
        s.emit(Rect::new(0, H - CELL_H, GUTTER, CELL_H), Label(Txt::new("Σ"), theme.map(ink)));
        for c in 0..COLS {
            s.emit(Rect::new(GUTTER + c as i32 * CELL_W, H - CELL_H, CELL_W, CELL_H), Num::<2, _, _>(un_at::<Sum>(c), theme.map(ink)));
        }
    }
}
