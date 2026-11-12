//! Rows with real text (text lane): the shape of gui-bench's `rows` and `table` scenarios. A header with a
//! count, then rows of (background, "Row ", a five-digit index, a label, a value). Device pixels, 14 px
//! DejaVu Sans from a prepared face image: no font file is opened and nothing is shaped but the labels of the
//! rows that are exposed.
//!
//! What is which stage, none of it written: "Row " and "rows" are constants (shaped once, found again by the
//! memo); a row's label is Mount stage (shaped when the row is described); the index, the value and the count
//! are numbers (no shaping: arithmetic on a record constant or a word of the block while painting); every
//! colour is an expression over the block.
#![allow(dead_code)]
use bough_x::prelude::*;
use bough_x::text::{Face, Glyphs};
use core::cell::RefCell;
use std::rc::Rc;

pub const W: i32 = 1280;
pub const H: i32 = 800;
pub const ROW_H: i32 = 32;
pub const HEADER: i32 = 40;
pub const PAD: i32 = 12;
pub const NO_HOVER: u32 = u32::MAX;
pub const BODY: Rect = Rect { x0: 0, y0: HEADER, x1: W, y1: H };

lens!(pub Hover: u32 = 0);
lens!(pub Scroll: i32 = 1);
lens!(pub Count: i32 = 2);
// the labels' ink alone follows this word: a change of it damages text and nothing else
lens!(pub Tint: u32 = 3);
pub const NWORDS: usize = 4;

pub static FACES: [Face; 1] = [Face::new(include_bytes!("../../bough-face/data/DejaVuSans-14.face"))];

pub struct Item {
    pub id: u32,
    pub label: String,
    /// Hundredths.
    pub value: i32,
}

pub struct Rows {
    pub u: U,
    pub items: Rel<Item>,
    pub glyphs: Rc<RefCell<Glyphs>>,
    shape: [Rect; 5],
    head: [Rect; 3],
}

const WORDS: [&str; 12] = ["quick", "amber", "river", "office", "fluffy", "violet", "stone", "bright", "copper", "field", "na\u{ef}ve", "caf\u{e9}"];
pub fn label(id: u32) -> String {
    format!("{} {} {}", WORDS[(id * 7 % 12) as usize], WORDS[(id * 13 % 12) as usize], WORDS[(id * 17 % 12) as usize])
}
fn bg((hot, k): (u32, u32)) -> Color {
    Color(if hot == k { 0xcfe3ff } else if k & 1 == 1 { 0xf6f8fa } else { 0xffffff })
}
fn ink((hot, k): (u32, u32)) -> Color {
    Color(if hot == k { 0x0b3d91 } else { 0x1f2328 })
}
fn label_ink((hot, k, tint): (u32, u32, u32)) -> Color {
    if tint != 0 { Color(0x8a2be2) } else { ink((hot, k)) }
}
fn head_ink(tint: u32) -> Color {
    Color(if tint != 0 { 0x8a2be2 } else { 0x6e7781 })
}

impl Rows {
    pub fn new(n: usize) -> Rows {
        let items: Vec<Item> = (0..n as u32).map(|id| Item { id, label: label(id), value: (id as i32 * 7919) % 100_000 }).collect();
        let mut u = U::default();
        u.set::<Hover>(NO_HOVER);
        u.set::<Count>(n as i32);
        let f = &FACES[0];
        // the boxes of a row: where each text starts is the advance of what precedes it
        let px = |s: &str| f.px(f.advance(s.as_bytes()).expect("covered"));
        let x1 = PAD + px("Row ");
        let x2 = x1 + px("00000") + 2 * PAD;
        let shape = [Rect::new(0, 0, W, ROW_H), Rect::new(PAD, 0, x1 - PAD, ROW_H), Rect::new(x1, 0, x2 - x1, ROW_H), Rect::new(x2, 0, 420, ROW_H), Rect::new(x2 + 440, 0, 160, ROW_H)];
        let head = [Rect::new(0, 0, W, HEADER), Rect::new(PAD, 0, 80, HEADER), Rect::new(PAD + 80, 0, px("rows \u{b7} sorted"), HEADER)];
        Rows { u, items: Rel::new(items), glyphs: Rc::new(RefCell::new(Glyphs::new())), shape, head }
    }
    /// A line of text in the one face. Text the face does not cover draws nothing here (it is the full
    /// shaper's, off the interaction path).
    fn line(&self, s: &str) -> bough_x::text::Run {
        self.glyphs.borrow_mut().line(0, &FACES[0], s).map_or(Default::default(), |l| l.0)
    }
    pub fn hover(&mut self, row: u32) {
        self.u.set::<Hover>(row);
    }
    pub fn toggle_tint(&mut self) {
        let t = self.u.get::<Tint>() ^ 1;
        self.u.set::<Tint>(t);
    }
    pub fn scroll_by(&mut self, dy: i32) {
        let max = (self.items.len() as i32 * ROW_H - (BODY.y1 - BODY.y0)).max(0);
        let y = (self.u.get::<Scroll>() + dy).clamp(0, max);
        self.u.set::<Scroll>(y);
    }
    pub fn toggle_suffix(&mut self, i: usize) {
        let l = &mut self.items.edit(i).label;
        match l.strip_suffix(" !!!") {
            Some(b) => *l = b.to_string(),
            None => l.push_str(" !!!"),
        }
    }
    pub fn set_value(&mut self, i: usize, v: i32) {
        self.items.edit(i).value = v;
    }
    pub fn remove(&mut self, i: usize) {
        self.items.remove(i);
        self.u.set::<Count>(self.items.len() as i32);
        self.scroll_by(0);
    }
    pub fn insert(&mut self, i: usize, id: u32) {
        self.items.insert(i, Item { id, label: label(id), value: id as i32 });
        self.u.set::<Count>(self.items.len() as i32);
    }
}

impl View for Rows {
    fn block(&self) -> &U {
        &self.u
    }
    fn view<S: Sink<Self>>(&self, s: &mut S) {
        s.row(|| (self.head, (Fill(Kc::<0xeef1f4>), Digits::<0, 0, _, _>(un::<Count>(), Kc::<0x1f2328>), Line::new(self.line("rows \u{b7} sorted"), un::<Tint>().map(head_ink)))));
        s.scope(Space { clip: BODY, base: (0, BODY.y0), scroll: Scroll::SLOT as u8 }, |s| {
            s.list(&List { n: self.items.len() as u32, row_h: ROW_H, x: 0, y: 0, shape: &self.shape, edits: &self.items.journal }, |r| {
                let it = self.items.get(r);
                let k = rec(r as u32);
                (
                    Fill((un::<Hover>(), k).map(bg)),
                    Line::new(self.line("Row "), (un::<Hover>(), k).map(ink)),
                    Digits::<0, 5, _, _>(rec(it.id as i32), (un::<Hover>(), k).map(ink)),
                    Line::new(self.line(&it.label), (un::<Hover>(), k, un::<Tint>()).map(label_ink)),
                    Digits::<2, 0, _, _>(rec(it.value), Kc::<0x6e7781>),
                )
            });
        });
    }
}
