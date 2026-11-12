//! `table` and `ticker`: a header with the row count, and a keyed table the function keys change; a click
//! selects a row. With a ticker, a clock also updates rows and the header.
//!
//! `fine`: the rows are a relation whose edits are its own journal, so an operation re-describes the rows it
//! names. `coarse`: the same state with no note taken; after a handler the exposed view is described again and
//! compared with the columns (re-describing mode). Either way only exposed rows exist.
use crate::cli::Ticker;
use crate::spec::{self, Ids, Op, Row};
use crate::{Geo, NONE};
use bough_span::kinds::{LEFT, RIGHT, Str, Text};
use bough_x::prelude::*;
use bough_x::sink::Edit;
use core::time::Duration;

lens!(pub Scroll: i32 = 0);
lens!(pub Sel: u32 = 1);
lens!(pub Len: u32 = 2);
lens!(pub Tick: u32 = 3);
lens!(pub Ticking: u32 = 4);
pub const NWORDS: usize = 5;

pub struct Table {
    pub u: U,
    pub rows: Rel<Row>,
    pub ids: Ids,
    pub count: usize,
    pub ticker: Option<Ticker>,
    pub ticks: u64,
    /// Edits take no note (the coarse variant).
    pub opaque: bool,
    pub geo: Geo,
    shape: [Rect; 4],
    header: [Rect; 3],
}

fn row_fill((sel, id): (u32, u32)) -> Color {
    Color(if sel == id { spec::SELECTED } else { spec::ROW_EVEN })
}
/// `1000 rows`, or `1000 rows · tick 7` when a ticker runs.
pub fn status((len, tick, ticking): (u32, u32, u32)) -> Str<8> {
    let mut s = Str::<8>::num(len);
    s.push(b" rows");
    if ticking != 0 {
        s.push(" \u{b7} tick ".as_bytes());
        s.push(Str::<3>::num(tick).bytes());
    }
    s
}

impl Table {
    pub fn new(count: usize, ticker: Option<Ticker>) -> Table {
        let geo = Geo::new(spec::WINDOW.0 as i32, spec::WINDOW.1 as i32, 1);
        let mut ids = Ids::default();
        let mut t = Table { u: U::default(), rows: Rel::new(ids.rows(count)), ids, count, ticker, ticks: 0, opaque: false, geo, shape: geo.row_shape(), header: geo.header_shape() };
        t.u.set::<Sel>(NONE);
        t.u.set::<Ticking>(ticker.is_some() as u32);
        t.words();
        t
    }
    pub fn layout(&mut self, geo: Geo) {
        let y = self.u.get::<Scroll>() as i64 * geo.k as i64 / self.geo.k as i64;
        self.geo = geo;
        self.shape = geo.row_shape();
        self.header = geo.header_shape();
        self.u.set::<Scroll>(y as i32);
        self.words();
    }
    /// The words of the block that follow the rows: the count, and a scroll offset still inside the list.
    fn words(&mut self) {
        self.u.set::<Len>(self.rows.len() as u32);
        self.u.set::<Tick>(self.ticks as u32);
        let y = self.u.get::<Scroll>().clamp(0, self.geo.max_scroll(self.rows.len(), self.geo.header_h()));
        self.u.set::<Scroll>(y);
    }
    pub fn select(&mut self, id: u32) {
        let s = if self.u.get::<Sel>() == id { NONE } else { id };
        self.u.set::<Sel>(s);
    }
    pub fn selected(&self) -> Option<u32> {
        Some(self.u.get::<Sel>()).filter(|&s| s != NONE)
    }
    fn note(&mut self, e: Edit) {
        if !self.opaque {
            self.rows.journal.push(e);
        }
    }
    /// A table operation (gui-bench's `Op::apply`, with a note of which rows it changed).
    pub fn apply(&mut self, op: Op) {
        let len = self.rows.len();
        match op {
            Op::Update10 => {
                for i in (0..len).step_by(10) {
                    spec::TableRow::toggle_suffix(&mut self.rows.items_mut()[i]);
                    self.note(Edit::Update(i as u32));
                }
            }
            Op::Swap => {
                if len > 3 {
                    self.rows.items_mut().swap(1, len - 2);
                    self.note(Edit::Update(1));
                    self.note(Edit::Update(len as u32 - 2));
                }
            }
            Op::Remove => {
                if len > spec::EDIT_AT {
                    self.rows.items_mut().remove(spec::EDIT_AT);
                    self.note(Edit::Shift(spec::EDIT_AT as u32));
                }
            }
            Op::Insert => {
                let at = spec::EDIT_AT.min(len);
                let row = self.ids.row();
                self.rows.items_mut().insert(at, row);
                self.note(Edit::Shift(at as u32));
            }
            Op::Replace => {
                *self.rows.items_mut() = self.ids.rows(self.count);
                self.note(Edit::Shift(0));
            }
            Op::Append => {
                let more = self.ids.rows(spec::APPEND);
                self.rows.items_mut().extend(more);
                self.note(Edit::Shift(len as u32));
            }
            Op::Truncate => {
                self.rows.items_mut().truncate(self.count);
                self.note(Edit::Shift(self.count.min(len) as u32));
            }
            Op::Clear => {
                if len == 0 {
                    *self.rows.items_mut() = self.ids.rows(self.count);
                } else {
                    self.rows.items_mut().clear();
                }
                self.note(Edit::Shift(0));
            }
        }
        self.words();
    }
    fn key(&mut self, e: &Ev) {
        if let Some(Key::Char(c)) = e.key
            && (0xf704..=0xf70b).contains(&(c as u32))
            && let Some(op) = Op::from_f_key((c as u32 - 0xf703) as u8)
        {
            self.apply(op);
        }
    }
    pub fn scroll_by(&mut self, dy: i32) {
        let y = (self.u.get::<Scroll>() + dy).clamp(0, self.geo.max_scroll(self.rows.len(), self.geo.header_h()));
        self.u.set::<Scroll>(y);
    }
    /// The ticker's clock: the tick count and `updates` rows spread over the list.
    pub fn clock(&mut self) {
        let Some(t) = self.ticker else { return };
        self.ticks += 1;
        let len = self.rows.len();
        for i in spec::ticker_positions(len, t.updates) {
            let r = &mut self.rows.items_mut()[i];
            r.value = spec::ticker_value(r.id, self.ticks);
            self.note(Edit::Update(i as u32));
        }
        self.words();
    }
}

impl View for Table {
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
                    // the window: the function keys are heard here
                    Fill(Kc::<{ spec::BACKGROUND }>).on_key(|v: &mut Table, e: &Ev| v.key(e)),
                    Fill(Kc::<{ spec::HEADER_BG }>),
                    Fill(Kc::<{ spec::HEADER_BORDER }>),
                    Text::<LEFT, _, _>((un::<Len>(), un::<Tick>(), un::<Ticking>()).map(status), Kc::<{ spec::LABEL_COLOR }>),
                ),
            )
        });
        s.scope(Space { clip: g.body(top), base: (0, top), scroll: Scroll::SLOT as u8 }, |s| {
            s.list(&List { n: self.rows.len() as u32, row_h: g.row_h(), x: 0, y: 0, shape: &self.shape, edits: &self.rows.journal }, |r| {
                let row = self.rows.get(r);
                let id = row.id as u32;
                (
                    Fill((un::<Sel>(), rec(id)).map(row_fill)).on_click(move |v: &mut Table| v.select(id)),
                    Fill(Kc::<{ spec::ROW_BORDER }>),
                    Text::<LEFT, _, _>(rec(Str::<8>::new(&row.label)), Kc::<{ spec::LABEL_COLOR }>),
                    Text::<RIGHT, _, _>(rec(Str::<1>::num(row.value)), Kc::<{ spec::VALUE_COLOR }>),
                )
            });
        });
    }
}

impl crate::scene::Model for Table {
    const NWORDS: usize = NWORDS;
    fn layout(&mut self, geo: Geo) {
        Table::layout(self, geo)
    }
    fn wheel(&mut self, _x: i32, _y: i32, dy: i32) {
        self.scroll_by(dy)
    }
    fn period(&self) -> Option<Duration> {
        self.ticker.map(|t| Duration::from_millis(t.interval_ms.max(1)))
    }
    fn clock(&mut self) {
        Table::clock(self)
    }
}

/// The coarse variant: the same table with opaque state, described again after every handler.
pub struct Coarse(pub Table);
impl Coarse {
    pub fn new(count: usize, ticker: Option<Ticker>) -> Coarse {
        let mut t = Table::new(count, ticker);
        t.opaque = true;
        Coarse(t)
    }
}
impl View for Coarse {
    fn block(&self) -> &U {
        &self.0.u
    }
    fn view<S: Sink<Self>>(&self, s: &mut S) {
        let t = &self.0;
        let g = t.geo;
        let top = g.header_h();
        s.row(|| {
            let h = t.header;
            (
                [Rect::new(0, 0, g.w, g.h), h[0], h[1], h[2]],
                (
                    Fill(Kc::<{ spec::BACKGROUND }>).on_key(|v: &mut Coarse, e: &Ev| v.0.key(e)),
                    Fill(Kc::<{ spec::HEADER_BG }>),
                    Fill(Kc::<{ spec::HEADER_BORDER }>),
                    Text::<LEFT, _, _>((un::<Len>(), un::<Tick>(), un::<Ticking>()).map(status), Kc::<{ spec::LABEL_COLOR }>),
                ),
            )
        });
        s.scope(Space { clip: g.body(top), base: (0, top), scroll: Scroll::SLOT as u8 }, |s| {
            s.list(&List { n: t.rows.len() as u32, row_h: g.row_h(), x: 0, y: 0, shape: &t.shape, edits: &FIXED }, |r| {
                let row = t.rows.get(r);
                let id = row.id as u32;
                (
                    Fill((un::<Sel>(), rec(id)).map(row_fill)).on_click(move |v: &mut Coarse| v.0.select(id)),
                    Fill(Kc::<{ spec::ROW_BORDER }>),
                    Text::<LEFT, _, _>(rec(Str::<8>::new(&row.label)), Kc::<{ spec::LABEL_COLOR }>),
                    Text::<RIGHT, _, _>(rec(Str::<1>::num(row.value)), Kc::<{ spec::VALUE_COLOR }>),
                )
            });
        });
    }
}
impl crate::scene::Model for Coarse {
    const NWORDS: usize = NWORDS;
    const REDESCRIBE: bool = true;
    fn layout(&mut self, geo: Geo) {
        self.0.layout(geo)
    }
    fn wheel(&mut self, _x: i32, _y: i32, dy: i32) {
        self.0.scroll_by(dy)
    }
    fn period(&self) -> Option<Duration> {
        self.0.ticker.map(|t| Duration::from_millis(t.interval_ms.max(1)))
    }
    fn clock(&mut self) {
        self.0.clock()
    }
}
