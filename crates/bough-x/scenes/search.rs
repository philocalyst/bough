//! A search field over a list (text lane): the shape of gui-bench's `typing` and `search` scenarios. A header
//! with a one-line input and a count; under it the rows whose label holds the query.
//!
//! The input is plain state and plain handlers: `query` is a `String` (in a relation of one row, so that a
//! keystroke names what it changed), `caret` a byte index. What a keystroke does to text: the field's line is
//! shaped again (one line, the memo's miss), the caret's pixel position is one word of the block
//! (`CaretX`: a caret move alone shapes nothing and re-describes nothing), and rows that enter the result
//! find their labels in the memo, because they were shaped the first time they were exposed.
//!
//! The filter here is a plain scan of the items per keystroke: the incremental filter is the expr lane's, and
//! what this scene is for is the text under it.
#![allow(dead_code)]
use bough_x::prelude::*;
use bough_x::sink::Edit;
use bough_x::text::{Face, Glyphs};
use core::cell::RefCell;
use std::rc::Rc;

pub const W: i32 = 1280;
pub const H: i32 = 800;
pub const ROW_H: i32 = 32;
pub const HEADER: i32 = 40;
pub const PAD: i32 = 12;
pub const FIELD: Rect = Rect { x0: 868, y0: 6, x1: 1268, y1: 34 };
pub const BODY: Rect = Rect { x0: 0, y0: HEADER, x1: W, y1: H };

lens!(pub Scroll: i32 = 0);
lens!(pub Count: i32 = 1);
lens!(pub CaretX: i32 = 2);
pub const NWORDS: usize = 3;

pub static FACES: [Face; 1] = [Face::new(include_bytes!("../../bough-face/data/DejaVuSans-14.face"))];

pub struct Item {
    pub label: String,
    pub value: i32,
}

pub struct Search {
    pub u: U,
    pub query: Rel<String>,
    pub caret: usize,
    pub all: Vec<Item>,
    /// The items the query matches, in order.
    pub hits: Rel<u32>,
    pub glyphs: Rc<RefCell<Glyphs>>,
    /// Whether a keystroke also filters (off: the `typing` scenario).
    pub filter: bool,
}

const ADJ: [&str; 8] = ["quick", "amber", "bright", "calm", "eager", "fluffy", "grand", "humble"];
const COL: [&str; 6] = ["red", "green", "blue", "violet", "copper", "ivory"];
const NOUN: [&str; 8] = ["river", "office", "stone", "field", "harbour", "meadow", "lantern", "orchard"];
pub fn label(id: u32) -> String {
    format!("{} {} {}", ADJ[(id * 7 % 8) as usize], COL[(id * 13 % 6) as usize], NOUN[(id * 17 % 8) as usize])
}

const ROW: [Rect; 3] = [Rect::new(0, 0, W, ROW_H), Rect::new(PAD, 0, 600, ROW_H), Rect::new(640, 0, 160, ROW_H)];
const FIELD_ROW: [Rect; 2] = [Rect::new(0, 0, 400, 28), Rect::new(6, 0, 388, 28)];
fn bg(k: u32) -> Color {
    Color(if k & 1 == 1 { 0xf6f8fa } else { 0xffffff })
}
fn caret_at(x: i32) -> (i32, i32) {
    (x, 0)
}

impl Search {
    pub fn new(n: usize) -> Search {
        let all: Vec<Item> = (0..n as u32).map(|id| Item { label: label(id), value: (id as i32 * 7919) % 100_000 }).collect();
        let mut u = U::default();
        u.set::<Count>(n as i32);
        Search { u, query: Rel::new(vec![String::new()]), caret: 0, hits: Rel::new((0..n as u32).collect()), all, glyphs: Rc::new(RefCell::new(Glyphs::new())), filter: true }
    }
    fn line(&self, s: &str) -> bough_x::text::Run {
        self.glyphs.borrow_mut().line(0, &FACES[0], s).map_or(Default::default(), |l| l.0)
    }
    /// The caret's pixel position: the advance of the text before it.
    fn place_caret(&mut self) {
        let f = &FACES[0];
        let x = f.advance(self.query.get(0)[..self.caret].as_bytes()).map_or(0, |a| f.px(a));
        self.u.set::<CaretX>(x);
    }
    fn refilter(&mut self) {
        if !self.filter {
            return;
        }
        let q = self.query.get(0).to_ascii_lowercase();
        let hits: Vec<u32> = (0..self.all.len() as u32).filter(|&i| q.is_empty() || self.all[i as usize].label.contains(&q)).collect();
        self.u.set::<Count>(hits.len() as i32);
        *self.hits.items_mut() = hits;
        self.hits.journal.push(Edit::Shift(0));
        self.u.set::<Scroll>(0);
    }
    pub fn type_text(&mut self, t: &str) {
        let at = self.caret;
        self.query.edit(0).insert_str(at, t);
        self.caret += t.len();
        self.place_caret();
        self.refilter();
    }
    pub fn key(&mut self, e: &Ev) {
        let q = self.query.get(0);
        match e.key {
            Some(Key::Backspace) if self.caret > 0 => {
                let prev = q[..self.caret].char_indices().next_back().map_or(0, |c| c.0);
                self.query.edit(0).replace_range(prev..self.caret, "");
                self.caret = prev;
                self.place_caret();
                self.refilter();
            }
            // a caret move is a word of the block: nothing is shaped, nothing is re-described
            Some(Key::Left) if self.caret > 0 => {
                self.caret = q[..self.caret].char_indices().next_back().map_or(0, |c| c.0);
                self.place_caret();
            }
            Some(Key::Right) if self.caret < q.len() => {
                self.caret += q[self.caret..].chars().next().map_or(0, |c| c.len_utf8());
                self.place_caret();
            }
            Some(Key::Home) => {
                self.caret = 0;
                self.place_caret();
            }
            Some(Key::End) => {
                self.caret = q.len();
                self.place_caret();
            }
            _ => {}
        }
    }
}

impl View for Search {
    fn block(&self) -> &U {
        &self.u
    }
    fn view<S: Sink<Self>>(&self, s: &mut S) {
        s.row(|| {
            (
                [Rect::new(0, 0, W, H), Rect::new(0, 0, W, HEADER), Rect::new(PAD, 0, 80, HEADER), Rect::new(PAD + 80, 0, 60, HEADER), Rect::new(FIELD.x0 + 6, FIELD.y0 + 4, 2, 20)],
                (
                    Fill(Kc::<0xffffff>),
                    Fill(Kc::<0xeef1f4>),
                    Digits::<0, 0, _, _>(un::<Count>(), Kc::<0x1f2328>),
                    Line::new(self.line("rows"), Kc::<0x6e7781>),
                    // the caret: where it is is a word of the block
                    Outline::<1, _, _>(Kc::<0x1f2328>, un::<CaretX>().map(caret_at)),
                ),
            )
        });
        // the field: its text is a relation of one row, so a keystroke re-describes that row and nothing else
        s.list(&List { n: 1, row_h: 28, x: FIELD.x0, y: FIELD.y0, shape: &FIELD_ROW, edits: &self.query.journal }, |_| {
            (
                Fill(Kc::<0xfafbfc>).on_text(|s: &mut Search, t: &str| s.type_text(t)).on_key(|s: &mut Search, e: &Ev| s.key(e)),
                Line::new(self.line(self.query.get(0)), Kc::<0x1f2328>),
            )
        });
        s.scope(Space { clip: BODY, base: (0, BODY.y0), scroll: Scroll::SLOT as u8 }, |s| {
            s.list(&List { n: self.hits.len() as u32, row_h: ROW_H, x: 0, y: 0, shape: &ROW, edits: &self.hits.journal }, |r| {
                let it = &self.all[*self.hits.get(r) as usize];
                (Fill(rec(r as u32).map(bg)), Line::new(self.line(&it.label), Kc::<0x1f2328>), Digits::<2, 0, _, _>(rec(it.value), Kc::<0x6e7781>))
            });
        });
    }
}
