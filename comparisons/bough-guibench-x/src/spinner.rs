//! `spinner`: a header with a turning square above static rows. The turn is an expression over `Time`: no turn
//! of the state runs while it spins, and it is drawn once per frame callback, so it follows the output's refresh.
use crate::spec::{self, Row};
use crate::Geo;
use bough_span::kinds::{LEFT, RIGHT, Spin, Str, Text, Turns};
use bough_x::prelude::*;

lens!(pub Scroll: i32 = 0);
lens!(pub Len: u32 = 1);
pub const NWORDS: usize = 2;

pub struct Spinner {
    pub u: U,
    pub rows: Vec<Row>,
    pub geo: Geo,
    shape: [Rect; 4],
    header: [Rect; 3],
}
fn status(len: u32) -> Str<8> {
    let mut s = Str::<8>::num(len);
    s.push(b" rows");
    s
}
impl Spinner {
    pub fn new(count: usize) -> Spinner {
        let geo = Geo::new(spec::WINDOW.0 as i32, spec::WINDOW.1 as i32, 1);
        let mut u = U::default();
        u.set::<Len>(count as u32);
        Spinner { u, rows: spec::Ids::default().rows(count), geo, shape: geo.row_shape(), header: geo.header_shape() }
    }
    fn square(&self) -> Rect {
        let (g, s) = (self.geo, spec::SPINNER_SIZE as i32 * self.geo.k);
        Rect::new(g.w - g.pad() - s, (g.header_h() - g.k - s) / 2, s, s)
    }
    pub fn scroll_by(&mut self, dy: i32) {
        let y = (self.u.get::<Scroll>() + dy).clamp(0, self.geo.max_scroll(self.rows.len(), self.geo.header_h()));
        self.u.set::<Scroll>(y);
    }
}
impl View for Spinner {
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
                    Fill(Kc::<{ spec::BACKGROUND }>),
                    Fill(Kc::<{ spec::HEADER_BG }>),
                    Fill(Kc::<{ spec::HEADER_BORDER }>),
                    Text::<LEFT, _, _>(un::<Len>().map(status), Kc::<{ spec::LABEL_COLOR }>),
                ),
            )
        });
        // the square is a row of its own: its column is the only one a frame of the animation damages
        if g.k == 1 {
            s.row(|| ([self.square()], Spin::<{ spec::SPINNER_RADIUS as i32 }, _, _>(Kc::<{ spec::SPINNER_COLOR }>, Turns::<{ spec::SPINNER_PERIOD_MS as u32 }>)));
        } else {
            s.row(|| ([self.square()], Spin::<{ 2 * spec::SPINNER_RADIUS as i32 }, _, _>(Kc::<{ spec::SPINNER_COLOR }>, Turns::<{ spec::SPINNER_PERIOD_MS as u32 }>)));
        }
        s.scope(Space { clip: g.body(top), base: (0, top), scroll: Scroll::SLOT as u8 }, |s| {
            s.list(&List { n: self.rows.len() as u32, row_h: g.row_h(), x: 0, y: 0, shape: &self.shape, edits: &FIXED }, |r| {
                let row = &self.rows[r];
                (
                    Fill(Kc::<{ spec::ROW_EVEN }>),
                    Fill(Kc::<{ spec::ROW_BORDER }>),
                    Text::<LEFT, _, _>(rec(Str::<8>::new(&row.label)), Kc::<{ spec::LABEL_COLOR }>),
                    Text::<RIGHT, _, _>(rec(Str::<1>::num(row.value)), Kc::<{ spec::VALUE_COLOR }>),
                )
            });
        });
    }
}
impl crate::scene::Model for Spinner {
    const NWORDS: usize = NWORDS;
    fn layout(&mut self, geo: Geo) {
        let y = self.u.get::<Scroll>() as i64 * geo.k as i64 / self.geo.k as i64;
        self.geo = geo;
        self.shape = geo.row_shape();
        self.header = geo.header_shape();
        self.u.set::<Scroll>((y as i32).clamp(0, geo.max_scroll(self.rows.len(), geo.header_h())));
    }
    fn wheel(&mut self, _x: i32, _y: i32, dy: i32) {
        self.scroll_by(dy)
    }
}
