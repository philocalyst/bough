//! The row shapes of the façade's heavy scenes (`comparisons/bough-workloads` on r7/bench: sheet, ide, chat),
//! written with the façade's builder calls. Text is the stand-in leaves until the text lane's exist; paint
//! records are a word each (their layout is what is under test here).
#![allow(dead_code)]
use bough_lay::tree::standin::{Line, Para};
use bough_lay::*;

pub const COLS: usize = 10;
const CELL_W: f32 = 110.0;
const CELL_H: f32 = 22.0;
const GUTTER: f32 = 56.0;

/// A number in a box of fixed size: the text fills the box and aligns inside it while shading, so its
/// measured size never enters the layout.
#[inline(always)]
pub fn num(line_h: f32) -> Leaf<Line, Rec1<u32>> {
    text(Line(0.0, line_h), 0u32).grow(1.0)
}

/// sheet.rs `data_row`: a gutter with the row number, then ten cells.
#[inline(always)]
pub fn sheet_row(r: usize) -> impl Lay + Copy {
    row((
        div().w(GUTTER).h(CELL_H).px(6.0).bg(r as u32).child(num(2.0 * CELL_H)),
        row(core::array::from_fn::<_, COLS, _>(|c| div().w(CELL_W).h(CELL_H).px(6.0).bg(c as u32).child(num(2.0 * CELL_H)))),
    ))
}

pub const MAX_TOK: usize = 8;
const LINE_H: f32 = 18.0;
const GUTTER_W: f32 = 44.0;

/// ide.rs `line`: the line number, up to eight token spans with margins for their edge spaces, the typed
/// tail. `toks[i]` is (width, left margin, right margin) in pixels and points.
#[inline(always)]
pub fn ide_line(n: usize, toks: &[(f32, f32, f32); MAX_TOK], tail: f32) -> impl Lay + Copy {
    row((
        text(Line(0.0, 2.0 * LINE_H), 0u32).w(GUTTER_W),
        core::array::from_fn::<_, MAX_TOK, _>(|j| (j < n).then(|| text(Line(toks[j].0, 2.0 * LINE_H), 0u32).margin(toks[j].1, 0.0, toks[j].2, 0.0))),
        text(Line(tail, 2.0 * LINE_H), 0u32),
    ))
    .h(LINE_H)
}

pub struct Msg {
    pub name_w: f32,
    pub time_w: f32,
    pub text: Para,
    pub quote: Option<(f32, Para)>,
    pub pic: Option<(f32, f32)>,
    pub reacts: [Option<f32>; 3],
}

#[inline(always)]
fn avatar(size: f32, initials_w: f32, font: f32) -> impl Lay {
    row((text(Line(initials_w, 2.0 * (font + 4.0)), 0u32),)).center().w(size).h(size).shrink(0.0).bg(0u32)
}

/// chat.rs `message`: avatar, name and time, a bubble (quote, wrapped text, image, reaction pills).
#[inline(always)]
pub fn chat_row(m: &Msg) -> impl Lay + '_ {
    let quote = m.quote.as_ref().map(|(name_w, p)| column((text(Line(*name_w, 28.0), 0u32), text(p.clone(), 0u32))).gap(1.0).px(8.0).py(4.0).bg(0u32));
    let pills = row(m.reacts.map(|r| r.map(|w| row((text(Line(w, 28.0), 0u32),)).px(7.0).py(2.0).bg(0u32)))).gap(4.0);
    let pills = m.reacts[0].is_some().then_some(pills);
    let media = m.pic.map(|(w, h)| block(w, h).bg(0u32));
    row((
        avatar(36.0, 34.0, 12.0),
        column((
            row((text(Line(m.name_w, 32.0), 0u32), text(Line(m.time_w, 32.0), 0u32))).gap(8.0),
            column((quote, text(m.text.clone(), 0u32), media, pills)).gap(6.0).p(10.0).bg(0u32).w_pct(0.74),
        ))
        .gap(3.0)
        .grow(1.0),
    ))
    .gap(10.0)
    .px(16.0)
    .py(6.0)
}

/// A message's content from a seed (word widths as a shaped paragraph would give them).
pub fn msg(seed: u64) -> Msg {
    struct R(u64);
    impl R {
        fn next(&mut self, n: u64) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0 % n
        }
        fn words(&mut self, n: u64) -> Para {
            let w: Vec<f32> = (0..n).map(|_| 26.0 + 13.0 * self.next(6) as f32 + 0.5 * self.next(3) as f32).collect();
            Para::new(w.into(), 7.0, 36.0)
        }
    }
    let mut r = R(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let n = 12 * (1 + r.next(6)) - r.next(8);
    let text = r.words(n.max(3));
    let quote = (r.next(100) < 14).then(|| (120.0 + r.next(60) as f32, r.words(10)));
    let pic = (seed % 10 == 9).then_some((200.0, 133.0));
    let k = if r.next(100) < 20 { 1 + r.next(3) } else { 0 };
    let reacts = [0, 1, 2].map(|i| (i < k).then_some(30.0));
    Msg { name_w: 150.0 + r.next(40) as f32, time_w: 62.0, text, quote, pic, reacts }
}
