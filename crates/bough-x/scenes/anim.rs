//! An animation fixture for the core (not an API): a scrolling list of `rows` rows of 16 cells. In every row,
//! eight cells pulse for ever (an opaque Rust function of `Time`: no bound, so they are always in motion) and
//! eight glow with the pointer's distance (an opaque function of the pointer words). Above the list a banner
//! fades between two colours when toggled (a bounded transition of paint, nothing moves).
//!
//! What it is for: a frame must cost the same whether 64 or 4,096 records are animating, and rows in endless
//! motion must enter and leave exposure without a stale or missing sweep.
#![allow(dead_code)]
use bough_x::kinds::Tween;
use bough_x::prelude::*;

pub const CELLS: usize = 8;
pub const CELL_W: i32 = 64;
pub const ROW_H: i32 = 40;
pub const BANNER: i32 = 48;
pub const W: i32 = 2 * CELLS as i32 * CELL_W;

lens!(pub Scroll: i32 = 0);
lens!(pub FadeT0: u32 = 1);
lens!(pub FadeOn: u32 = 2);
pub const NWORDS: usize = 3;

pub struct Anim {
    pub u: U,
    pub rows: u32,
    pub h: i32,
}

fn pulse((t, phase): (u32, u32)) -> Color {
    let v = (t / 16 + phase * 37) & 0xff;
    Color(0xff00_0000 | v << 16 | (255 - v) << 8 | 0x40)
}
fn glow((x, y, at): (i32, i32, [i32; 2])) -> Color {
    let d = ((x - at[0]).abs() + (y - at[1]).abs()).min(255) as u32;
    Color(0xff00_0000 | (255 - d) << 8 | 0x20)
}
fn ends(on: u32) -> (Color, Color) {
    if on != 0 { (Color(0xff20_2020), Color(0xffe0_a020)) } else { (Color(0xffe0_a020), Color(0xff20_2020)) }
}
fn first(p: (Color, Color)) -> Color {
    p.0
}
fn second(p: (Color, Color)) -> Color {
    p.1
}

const SHAPE: [Rect; 2 * CELLS] = {
    let mut r = [Rect::new(0, 0, CELL_W, ROW_H); 2 * CELLS];
    let mut c = 0;
    while c < 2 * CELLS {
        r[c] = Rect::new(c as i32 * CELL_W, 2, CELL_W - 2, ROW_H - 4);
        c += 1;
    }
    r
};

impl Anim {
    pub fn new(rows: u32, h: i32) -> Anim {
        Anim { u: U::default(), rows, h }
    }
    pub fn body(&self) -> Rect {
        Rect { x0: 0, y0: BANNER, x1: W, y1: self.h }
    }
    pub fn scroll_by(&mut self, dy: i32) {
        let max = (self.rows as i32 * ROW_H - (self.h - BANNER)).max(0);
        let y = (self.u.get::<Scroll>() + dy).clamp(0, max);
        self.u.set::<Scroll>(y);
    }
    pub fn toggle(&mut self, now: u32) {
        let on = self.u.get::<FadeOn>() ^ 1;
        self.u.set::<FadeOn>(on);
        self.u.set::<FadeT0>(now);
    }
}

impl View for Anim {
    fn block(&self) -> &U {
        &self.u
    }
    fn view<S: Sink<Self>>(&self, s: &mut S) {
        s.row(|| ([Rect::new(0, 0, W, BANNER)], Fill(Tween::<200, _, _, _>(un::<FadeOn>().map(ends).map(first), un::<FadeOn>().map(ends).map(second), un::<FadeT0>()))));
        s.scope(Space { clip: self.body(), base: (0, BANNER), scroll: Scroll::SLOT as u8 }, |s| {
            s.list(&List { n: self.rows, row_h: ROW_H, x: 0, y: 0, shape: &SHAPE, edits: &FIXED }, |r| {
                (
                    std::array::from_fn::<_, CELLS, _>(|c| Fill((un::<Time>(), rec((r * CELLS + c) as u32)).map(pulse))),
                    std::array::from_fn::<_, CELLS, _>(|c| {
                        // the cell's centre on screen at scroll 0; a fixture, so the glow does not follow the scroll
                        let at = [(CELLS + c) as i32 * CELL_W + CELL_W / 2, BANNER + r as i32 * ROW_H + ROW_H / 2];
                        Fill((un::<PtrX>(), un::<PtrY>(), rec(at)).map(glow))
                    }),
                )
            });
        });
    }
}
