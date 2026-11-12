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
lens!(pub Scroll: i32 = 3);
// the column totals, in hundredths
lens!(pub Sum[COLS]: i32 = 4);
// the selection outline's glide: where it started from and when
lens!(pub SelFromX: i32 = 14);
lens!(pub SelFromY: i32 = 15);
lens!(pub SelT0: u32 = 16);
pub const NWORDS: usize = 7 + COLS;
/// The glide's length in ticks of `Time` (milliseconds).
pub const SEL_MS: u32 = 180;

pub struct Sheet {
    pub u: U,
    pub rows: Rel<[f32; COLS]>,
}

/// A cell's value as the integer the column holds: hundredths.
fn cents(v: f32) -> i32 {
    (v as f64 * 100.0).round() as i32
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
/// Where the outline is: a glide from where it was to the selected cell. No turn runs while it moves.
fn sel_glide() -> impl bough_x::kinds::Motion {
    Glide::<SEL_MS, _, _, _>((un::<SelFromX>(), un::<SelFromY>()), un::<Sel>().map(sel_at), un::<SelT0>())
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
            u.set_at::<Sum>(c, items.iter().map(|row| cents(row[c])).sum());
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
        let old = core::mem::replace(&mut self.rows.edit(r)[c], v);
        self.total(c, cents(v) - cents(old));
    }
    /// The same edit with no note taken (for re-describing mode): only the total follows.
    pub fn edit_opaque(&mut self, r: usize, c: usize, v: f32) {
        let old = core::mem::replace(&mut self.rows.items_mut()[r][c], v);
        self.total(c, cents(v) - cents(old));
    }
    fn total(&mut self, c: usize, d: i32) {
        let s = self.u.at::<Sum>(c) + d;
        self.u.set_at::<Sum>(c, s);
    }
    /// Select a cell at time `now`: the outline glides there from wherever it is.
    pub fn select(&mut self, i: u32, now: u32) {
        let mut u = self.u;
        u.w[U_TIME] = now;
        let (x, y) = sel_glide().eval(&Cx::of(&u));
        self.u.set::<SelFromX>(x);
        self.u.set::<SelFromY>(y);
        self.u.set::<SelT0>(now);
        self.u.set::<Sel>(i);
    }
    pub fn scroll_by(&mut self, dy: i32) {
        let max = (self.rows.len() as i32 * CELL_H - (BODY.y1 - BODY.y0)).max(0);
        let y = (self.u.get::<Scroll>() + dy).clamp(0, max);
        self.u.set::<Scroll>(y);
    }
    pub fn insert_row(&mut self, at: usize, row: [f32; COLS], opaque: bool) {
        for c in 0..COLS {
            self.total(c, cents(row[c]));
        }
        if opaque { self.rows.items_mut().insert(at, row) } else { self.rows.insert(at, row) }
    }
    pub fn remove_row(&mut self, at: usize, opaque: bool) {
        let row = if opaque { self.rows.items_mut().remove(at) } else { self.rows.remove(at) };
        for c in 0..COLS {
            self.total(c, -cents(row[c]));
        }
        self.scroll_by(0);
    }
}

/// The layout of one row: the gutter fill and its number, then a fill and a number per cell. Phase 1 has no
/// layout engine: these are the fixed boxes of the workload, placed by arithmetic, once.
const ROW_SHAPE: [Rect; 2 + 2 * COLS] = {
    let mut r = [Rect::new(0, 0, GUTTER, CELL_H); 2 + 2 * COLS];
    let mut c = 0;
    while c < COLS {
        r[2 + 2 * c] = Rect::new(GUTTER + c as i32 * CELL_W, 0, CELL_W, CELL_H);
        r[3 + 2 * c] = r[2 + 2 * c];
        c += 1;
    }
    r
};
const fn header(c: usize) -> Rect {
    Rect::new(GUTTER + c as i32 * CELL_W, TOOLBAR, CELL_W, HEADER)
}
const fn total(c: usize) -> Rect {
    Rect::new(GUTTER + c as i32 * CELL_W, H - CELL_H, CELL_W, CELL_H)
}

impl View for Sheet {
    fn block(&self) -> &U {
        &self.u
    }
    fn view<S: Sink<Self>>(&self, s: &mut S) {
        let theme = un::<Theme>();
        // the page, the toolbar, the header: hovering a header cell highlights its column
        s.row(|| {
            let mut rects = [Rect::new(0, 0, W, H); 5 + 2 * COLS];
            rects[1] = Rect::new(0, 0, W, TOOLBAR);
            rects[2] = Rect::new(24, 20, 200, 40);
            rects[3] = Rect::new(256, 20, 200, 40);
            rects[4] = Rect::new(0, TOOLBAR, GUTTER, HEADER);
            for c in 0..COLS {
                rects[5 + 2 * c] = header(c);
                rects[6 + 2 * c] = header(c);
            }
            let row = (
                Fill(theme.map(page)),
                Fill(theme.map(chrome)),
                Label(Txt::new("Sheet 1"), theme.map(ink)),
                Label(Txt::new("Σ totals"), Kc::<0x7a808c>),
                Fill(theme.map(chrome)),
                std::array::from_fn::<_, COLS, _>(|c| {
                    (
                        hover(Fill((theme, un::<Hover>(), rec(c as u32)).map(head)), |v: &mut Sheet, a, on| v.hover_header(a.0.0.2.0, on)),
                        Label(Txt::new(NAMES[c]), theme.map(ink)),
                    )
                }),
            );
            (rects, row)
        });
        // body: the rows scroll under the header; the selection outline rides above them
        s.scope(Space { clip: BODY, base: (0, BODY.y0), scroll: Scroll::SLOT as u8 }, |s| {
            s.list(&List { n: self.rows.len() as u32, row_h: CELL_H, x: 0, y: 0, shape: &ROW_SHAPE, edits: &self.rows.journal }, |r| {
                let vals = self.rows.get(r);
                (
                    Fill(theme.map(chrome)),
                    Num::<0, _, _>(rec(r as i32 + 1), Kc::<0x7a808c>),
                    std::array::from_fn::<_, COLS, _>(|c| {
                        (Fill((theme, un::<Hover>(), rec(c as u32 | ((r as u32 & 1) << 8))).map(fill)), Num::<2, _, _>(rec(cents(vals[c])), theme.map(ink)))
                    }),
                )
            });
            s.row(|| ([Rect::new(GUTTER, 0, CELL_W, CELL_H)], Outline::<4, _, _>(Kc::<0x2f6bff>, sel_glide())));
        });
        // footer: the totals are words of the block, formatted while shading
        s.row(|| {
            let mut rects = [Rect::new(0, H - CELL_H, W, CELL_H); 2 + COLS];
            rects[1] = Rect::new(0, H - CELL_H, GUTTER, CELL_H);
            for c in 0..COLS {
                rects[2 + c] = total(c);
            }
            (rects, (Fill(theme.map(chrome)), Label(Txt::new("Σ"), theme.map(ink)), std::array::from_fn::<_, COLS, _>(|c| Num::<2, _, _>(un_at::<Sum>(c), theme.map(ink)))))
        });
    }
}
