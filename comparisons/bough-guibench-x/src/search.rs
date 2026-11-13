//! `typing` and `search`: a header with a focused text field above the rows; with `filter`, the rows shown are
//! those whose label holds the field's text.
//!
//! The field's text is a [`Line`] of plain state; the field is a relation of one row, so a keystroke
//! re-describes that row and nothing else. The filter is the spec's: every keystroke scans every label with
//! `spec::matches` (which lower-cases both sides each time), exactly as the other frameworks' apps do. Nothing
//! is precomputed and no result is kept between keystrokes: an incremental filter is the core's to provide as a
//! relation operator (ROUND8 Amendment 1, item 7), and until it does this cell measures the plain scan.
use crate::spec::{self, Row};
use crate::{Geo, rgb};
use bough_glyph::Image;
use bough_span::kinds::{Bar, LEFT, RIGHT, Str, Text, TextOf};
use bough_span::line::Line;
use bough_x::prelude::*;
use bough_x::sink::Edit;

lens!(pub Scroll: i32 = 0);
lens!(pub Len: u32 = 1);
pub const NWORDS: usize = 2;

const SELECTION: u32 = 0xffb4d5fe;
const CARET: u32 = 0xff1f2328;
const FIELD_BG: u32 = 0xffffff;
/// Space between the field's edge and its text, in logical pixels.
const FIELD_PAD: i32 = 6;

pub struct Search {
    pub u: U,
    pub rows: Vec<Row>,
    /// The rows shown, as indices into `rows`.
    pub shown: Rel<u32>,
    pub query: Line,
    /// The field's one row.
    field: Rel<()>,
    pub selected: bool,
    pub filter: bool,
    pub geo: Geo,
    shape: [Rect; 4],
    header: [Rect; 3],
    field_shape: [Rect; 5],
}

fn status(len: u32) -> Str<8> {
    let mut s = Str::<8>::num(len);
    s.push(b" rows");
    s
}
fn image(k: i32) -> &'static Image {
    if k >= 2 { &bough_glyph::DEJAVU_SANS_28 } else { &bough_glyph::DEJAVU_SANS_14 }
}

impl Search {
    pub fn new(count: usize, filter: bool) -> Search {
        let geo = Geo::new(spec::WINDOW.0 as i32, spec::WINDOW.1 as i32, 1);
        let rows = spec::Ids::default().rows(count);
        let mut s = Search {
            u: U::default(),
            shown: Rel::new((0..count as u32).collect()),
            rows,
            query: Line::new(""),
            field: Rel::new(vec![()]),
            selected: false,
            filter,
            geo,
            shape: geo.row_shape(),
            header: geo.header_shape(),
            field_shape: [Rect::default(); 5],
        };
        s.layout(geo);
        s
    }
    pub fn layout(&mut self, geo: Geo) {
        let y = self.u.get::<Scroll>() as i64 * geo.k as i64 / self.geo.k as i64;
        self.geo = geo;
        self.shape = geo.row_shape();
        self.header = geo.header_shape();
        let k = geo.k;
        let (fw, fh) = ((spec::INPUT_WIDTH as i32 * k).min(geo.w - 2 * geo.pad()).max(k), spec::INPUT_HEIGHT as i32 * k);
        let f = Rect::new(0, 0, fw, fh);
        let inner = Rect { x0: k, y0: k, x1: fw - k, y1: fh - k };
        let text = Rect { x0: FIELD_PAD * k, y0: k, x1: fw - FIELD_PAD * k, y1: fh - k };
        let bar = Rect { y0: 4 * k, y1: fh - 4 * k, ..text };
        // border, background, selection, text, caret
        self.field_shape = [f, inner, bar, text, bar];
        self.u.set::<Scroll>(y as i32);
        self.words();
        self.field.journal.push(Edit::Update(0));
    }
    /// Where the field is in the window.
    pub fn field_at(&self) -> (i32, i32) {
        let g = self.geo;
        (g.w - g.pad() - self.field_shape[0].x1, (g.header_h() - g.k - self.field_shape[0].y1) / 2)
    }
    fn words(&mut self) {
        self.u.set::<Len>(self.rows.len() as u32);
        let y = self.u.get::<Scroll>().clamp(0, self.geo.max_scroll(self.shown.len(), self.geo.header_h()));
        self.u.set::<Scroll>(y);
    }
    /// The query changed: bring the rows shown up to date.
    fn refilter(&mut self) {
        self.field.journal.push(Edit::Update(0));
        if !self.filter {
            return;
        }
        let result: Vec<u32> = self.query.with(|q| self.rows.iter().enumerate().filter(|(_, r)| spec::matches(&r.label, q)).map(|(i, _)| i as u32).collect());
        if result != *self.shown.items() {
            *self.shown.items_mut() = result;
            self.shown.journal.push(Edit::Shift(0));
        }
        self.words();
    }
    pub fn type_text(&mut self, t: &str) {
        if self.selected {
            self.selected = false;
            self.query.edit(String::clear);
        }
        self.query.edit(|s| s.push_str(t));
        self.refilter();
    }
    pub fn key(&mut self, e: &Ev) {
        match e.key {
            Some(Key::Backspace) => {
                if self.selected {
                    self.selected = false;
                    self.query.edit(String::clear);
                } else {
                    self.query.edit(|s| {
                        s.pop();
                    });
                }
                self.refilter();
            }
            Some(Key::Char('a')) if e.mods.ctrl || e.mods.cmd => {
                self.selected = !self.query.is_empty();
                self.field.journal.push(Edit::Update(0));
            }
            Some(Key::Left | Key::Right | Key::Home | Key::End) if self.selected => {
                self.selected = false;
                self.field.journal.push(Edit::Update(0));
            }
            _ => {}
        }
    }
    pub fn scroll_by(&mut self, dy: i32) {
        let y = (self.u.get::<Scroll>() + dy).clamp(0, self.geo.max_scroll(self.shown.len(), self.geo.header_h()));
        self.u.set::<Scroll>(y);
    }
}

impl View for Search {
    fn block(&self) -> &U {
        &self.u
    }
    fn view<S: Sink<Self>>(&self, s: &mut S) {
        let g = self.geo;
        let top = g.header_h();
        s.row(|| {
            let h = self.header;
            (
                [Rect::new(0, 0, g.w, g.h), h[0], h[1], h[2]],
                (
                    Fill(Kc::<{ rgb(spec::BACKGROUND) }>),
                    Fill(Kc::<{ rgb(spec::HEADER_BG) }>),
                    Fill(Kc::<{ rgb(spec::HEADER_BORDER) }>),
                    Text::<LEFT, _, _>(un::<Len>().map(status), Kc::<{ rgb(spec::LABEL_COLOR) }>),
                ),
            )
        });
        // the field: one row of a relation the handlers note an edit in
        let (fx, fy) = self.field_at();
        s.list(&List { n: 1, row_h: self.field_shape[0].y1, x: fx, y: fy, shape: &self.field_shape, edits: &self.field.journal }, |_| {
            let img = image(g.k);
            let adv = self.query.with(|q| img.snap(img.advance(q.as_bytes())));
            let width = self.field_shape[3].x1 - self.field_shape[3].x0;
            (
                Fill(Kc::<{ rgb(spec::HEADER_BORDER) }>).on_text(|v: &mut Search, t: &str| v.type_text(t)).on_key(|v: &mut Search, e: &Ev| v.key(e)),
                Fill(Kc::<{ rgb(FIELD_BG) }>),
                Bar(0, if self.selected { adv.min(width) } else { 0 }, Kc::<SELECTION>),
                TextOf::<LEFT, _>(self.query.handle(), Kc::<{ rgb(spec::LABEL_COLOR) }>),
                Bar(if self.selected { 0 } else { adv.min(width - g.k) }, if self.selected { 0 } else { g.k }, Kc::<CARET>),
            )
        });
        s.scope(Space { clip: g.body(top), base: (0, top), scroll: Scroll::SLOT as u8 }, |s| {
            s.list(&List { n: self.shown.len() as u32, row_h: g.row_h(), x: 0, y: 0, shape: &self.shape, edits: &self.shown.journal }, |r| {
                let row = &self.rows[*self.shown.get(r) as usize];
                (
                    Fill(Kc::<{ rgb(spec::ROW_EVEN) }>),
                    Fill(Kc::<{ rgb(spec::ROW_BORDER) }>),
                    Text::<LEFT, _, _>(rec(Str::<8>::new(&row.label)), Kc::<{ rgb(spec::LABEL_COLOR) }>),
                    Text::<RIGHT, _, _>(rec(Str::<1>::num(row.value)), Kc::<{ rgb(spec::VALUE_COLOR) }>),
                )
            });
        });
    }
}

impl crate::scene::Model for Search {
    const NWORDS: usize = NWORDS;
    fn layout(&mut self, geo: Geo) {
        Search::layout(self, geo)
    }
    fn wheel(&mut self, _x: i32, _y: i32, dy: i32) {
        self.scroll_by(dy)
    }
    fn focus_at(&self) -> Option<(i32, i32)> {
        let (x, y) = self.field_at();
        Some((x + self.field_shape[0].x1 / 2, y + self.field_shape[0].y1 / 2))
    }
}
