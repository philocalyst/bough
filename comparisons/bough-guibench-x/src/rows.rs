//! `rows` and `virtual`: `count` striped rows that scroll, the one under the pointer highlighted.
//!
//! The two scenarios are one view. A list is a window on a relation: only exposed rows exist, at any count, so
//! there is no virtual-list variant to write (ROUND7 Amendment 2).
use crate::{Geo, NONE, rgb, spec};
use bough_span::kinds::{LEFT, RIGHT, Str, Text};
use bough_x::prelude::*;

lens!(pub Scroll: i32 = 0);
lens!(pub Hot: u32 = 1);
pub const NWORDS: usize = 2;

pub struct Rows {
    pub u: U,
    pub count: usize,
    pub geo: Geo,
    shape: [Rect; 4],
}

fn row_fill((hot, i): (u32, u32)) -> Color {
    Color(rgb(if hot == i { spec::ROW_HOVER } else { spec::row_background(i as usize) }))
}
/// `Row 00042`, without a heap string.
pub fn row_label(i: usize) -> Str<3> {
    let mut b = *b"Row 00000";
    let mut v = i;
    for k in (4..9).rev() {
        b[k] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    if v != 0 {
        // six digits and more: as `format!("Row {i:05}")` writes them
        return Str::new(&spec::row_label(i));
    }
    // SAFETY-free: ASCII
    Str::new(core::str::from_utf8(&b).unwrap())
}

impl Rows {
    pub fn new(count: usize) -> Rows {
        let geo = Geo::new(spec::WINDOW.0 as i32, spec::WINDOW.1 as i32, 1);
        let mut u = U::default();
        u.set::<Hot>(NONE);
        Rows { u, count, geo, shape: geo.row_shape() }
    }
    pub fn layout(&mut self, geo: Geo) {
        // the scroll offset is in buffer pixels: keep the same logical position across a scale change
        let y = self.u.get::<Scroll>() as i64 * geo.k as i64 / self.geo.k as i64;
        self.geo = geo;
        self.shape = geo.row_shape();
        self.u.set::<Scroll>((y as i32).clamp(0, geo.max_scroll(self.count, 0)));
    }
    fn hover(&mut self, i: u32, on: bool) {
        if on {
            self.u.set::<Hot>(i)
        } else if self.u.get::<Hot>() == i {
            self.u.set::<Hot>(NONE)
        }
    }
    pub fn scroll_by(&mut self, dy: i32) {
        let y = (self.u.get::<Scroll>() + dy).clamp(0, self.geo.max_scroll(self.count, 0));
        self.u.set::<Scroll>(y);
    }
}

impl View for Rows {
    fn block(&self) -> &U {
        &self.u
    }
    fn view<S: Sink<Self>>(&self, s: &mut S) {
        let g = self.geo;
        s.row(|| ([Rect::new(0, 0, g.w, g.h)], Fill(Kc::<{ rgb(spec::BACKGROUND) }>)));
        s.scope(Space { clip: g.body(0), base: (0, 0), scroll: Scroll::SLOT as u8 }, |s| {
            s.list(&List { n: self.count as u32, row_h: g.row_h(), x: 0, y: 0, shape: &self.shape, edits: &FIXED }, |r| {
                let i = r as u32;
                (
                    Fill((un::<Hot>(), rec(i)).map(row_fill)).on_hover(move |v: &mut Rows, e: &Ev| v.hover(i, e.on)),
                    Fill(Kc::<{ rgb(spec::ROW_BORDER) }>),
                    Text::<LEFT, _, _>(rec(row_label(r)), Kc::<{ rgb(spec::LABEL_COLOR) }>),
                    Text::<RIGHT, _, _>(rec(Str::<1>::num(spec::value(r as u64))), Kc::<{ rgb(spec::VALUE_COLOR) }>),
                )
            });
        });
    }
}

impl crate::scene::Model for Rows {
    const NWORDS: usize = NWORDS;
    fn layout(&mut self, geo: Geo) {
        Rows::layout(self, geo)
    }
    fn wheel(&mut self, _x: i32, _y: i32, dy: i32) {
        self.scroll_by(dy)
    }
}
