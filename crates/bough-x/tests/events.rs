//! Events, the dataflow run backwards: hover through the pick, press and click, keys and text to the focused
//! record, Tab, drag with capture, handlers that capture their row, focus that survives scrolling, the core words
//! (`Pick`, `Focus`, `Press`, `PressAt`, `Env`), and the pixels still equal to every oracle after events.
use bough_x::event::{self, Button, Dispatch, Ev, Interact, Key, Mods, NONE, U_FOCUS, U_PRESS, U_PRESS_AT, kind};
use bough_x::expr::U_PICK;
use bough_x::prelude::*;
use bough_x::reflect::Reflect;
use bough_x::soft::{Immediate, Soft};

lens!(pub Theme: u32 = 0);
lens!(pub Sel: u32 = 1);
lens!(pub Hot: u32 = 2);
lens!(pub Scroll: i32 = 3);
const NWORDS: usize = 4;
const W: i32 = 800;
const H: i32 = 600;
const ROW_H: i32 = 40;
const TOP: i32 = 100;
const BODY: Rect = Rect { x0: 0, y0: TOP, x1: W, y1: H };
const BUTTON: Rect = Rect::new(20, 20, 120, 40);
const SPLIT: Rect = Rect::new(300, 20, 10, 60);
const FIELD: Rect = Rect::new(400, 20, 300, 40);
const NO_ROW: u32 = u32::MAX;

struct Inbox {
    u: U,
    items: Rel<Txt>,
    draft: Rel<Txt>,
    clicks: u32,
    drags: Vec<(f32, f32, bool)>,
    log: Vec<String>,
}

fn page(dark: u32) -> Color {
    Color(if dark != 0 { 0xff15171c } else { 0xffffffff })
}
fn row_fill(((dark, sel), hot, r): ((u32, u32), u32, u32)) -> Color {
    Color(match (sel == r, hot == r, dark != 0) {
        (true, _, false) => 0xffdbe8ff,
        (true, _, true) => 0xff26406b,
        (false, true, _) => 0xffeeeeee,
        (false, false, false) => 0xfff6f7f9,
        (false, false, true) => 0xff1b1e24,
    })
}

impl Inbox {
    fn new(n: usize) -> Inbox {
        let mut u = U::default();
        u.set::<Sel>(NO_ROW);
        u.set::<Hot>(NO_ROW);
        let items = (0..n).map(|i| Txt::new(&format!("msg {i}"))).collect();
        Inbox { u, items: Rel::new(items), draft: Rel::new(vec![Txt::default()]), clicks: 0, drags: Vec::new(), log: Vec::new() }
    }
    fn select(&mut self, r: usize) {
        self.u.set::<Sel>(r as u32);
        self.log.push(format!("select {r}"));
    }
    fn hover(&mut self, r: usize, on: bool) {
        if on {
            self.u.set::<Hot>(r as u32);
        } else if self.u.get::<Hot>() == r as u32 {
            self.u.set::<Hot>(NO_ROW);
        }
        self.log.push(format!("{} {r}", if on { "enter" } else { "leave" }));
    }
    fn root_key(&mut self, e: &Ev) {
        if e.key == Some(Key::Char('t')) {
            let t = self.u.get::<Theme>() ^ 1;
            self.u.set::<Theme>(t);
        }
        self.log.push(format!("root {:?}", e.key));
    }
    fn draft(&self) -> String {
        let (b, n) = self.draft.get(0).bytes();
        String::from_utf8_lossy(&b[..n]).into_owned()
    }
    fn type_text(&mut self, t: &str) {
        let s = self.draft() + t;
        *self.draft.edit(0) = Txt::new(&s[..s.len().min(12)]);
    }
    fn field_key(&mut self, e: &Ev) {
        if e.key == Some(Key::Backspace) {
            let mut s = self.draft();
            s.pop();
            *self.draft.edit(0) = Txt::new(&s);
        }
        self.log.push(format!("field {:?}", e.key));
    }
    fn scroll_to_row(&mut self, r: i32) {
        self.u.set::<Scroll>(r * ROW_H);
    }
}

const ROW_SHAPE: [Rect; 2] = [Rect::new(0, 0, W, ROW_H), Rect::new(10, 5, 200, 30)];
const FIELD_SHAPE: [Rect; 2] = [Rect::new(0, 0, 300, 40), Rect::new(0, 0, 300, 40)];

impl View for Inbox {
    fn block(&self) -> &U {
        &self.u
    }
    fn block_mut(&mut self) -> Option<&mut U> {
        Some(&mut self.u)
    }
    fn view<S: Sink<Self>>(&self, s: &mut S) {
        let theme = un::<Theme>();
        s.row(|| {
            (
                [Rect::new(0, 0, W, H), BUTTON, BUTTON, SPLIT],
                (
                    // the window: keys with nothing more specific focused
                    Fill(theme.map(page)).on_key(|s: &mut Inbox, e: &Ev| s.root_key(e)),
                    Fill(Kc::<0xff2f6bff>).on_click(|s: &mut Inbox| s.clicks += 1),
                    // a label on the button: no handler, so transparent to the pointer
                    Label(Txt::new("Send"), Kc::<0xffffffff>),
                    Fill(Kc::<0xff888888>).on_drag(|s: &mut Inbox, e: &Ev| s.drags.push((e.dx, e.dy, e.on))),
                ),
            )
        });
        s.scope(Space { clip: BODY, base: (0, TOP), scroll: Scroll::SLOT as u8 }, |s| {
            s.list(&List { n: self.items.len() as u32, row_h: ROW_H, x: 0, y: 0, shape: &ROW_SHAPE, edits: &self.items.journal }, |r| {
                (
                    // each row's handlers capture the row: the captures are an operand, written with the row
                    Fill(((theme, un::<Sel>()), un::<Hot>(), rec(r as u32)).map(row_fill))
                        .on_click(move |s: &mut Inbox| s.select(r))
                        .on_hover(move |s: &mut Inbox, e: &Ev| s.hover(r, e.on))
                        .focusable(),
                    Label(*self.items.get(r), theme.map(|d| Color(if d != 0 { 0xffe6e9ef } else { 0xff1d2129 }))),
                )
            });
        });
        // the field: its text is a relation of one row, so a keystroke re-describes that row and nothing else
        s.list(&List { n: 1, row_h: 40, x: FIELD.x0, y: FIELD.y0, shape: &FIELD_SHAPE, edits: &self.draft.journal }, |_| {
            (
                Fill(Kc::<0xfffafafa>).on_text(|s: &mut Inbox, t: &str| s.type_text(t)).on_key(|s: &mut Inbox, e: &Ev| s.field_key(e)),
                Label(*self.draft.get(0), Kc::<0xff000000>),
            )
        });
    }
}

fn mounted(n: usize) -> (Inbox, World<Soft>) {
    let v = Inbox::new(n);
    let mut w = World::<Soft>::new(W, H, NWORDS);
    w.mount(&v);
    (v, w)
}
/// A point inside row `r` of the list, given the scroll.
fn row_pt(v: &Inbox, r: i32) -> (i32, i32) {
    (500, TOP + r * ROW_H - v.u.get::<Scroll>() + ROW_H / 2)
}
fn place_of_row(w: &mut World<Soft>, v: &Inbox, r: u32) -> u32 {
    let refl = Reflect::learn(&w.root, v);
    let it = refl.items(&w.root, &w.shown).into_iter().find(|i| i.ent.path.n == 2 && i.ent.row == r && i.kind() == "fill").expect("row exposed");
    let (path, col, rec) = (it.ent.path, it.col, it.rec);
    w.place(&path, col, rec)
}

#[test]
fn hover_goes_through_the_pick() {
    let (mut v, mut w) = mounted(1_000);
    let (x, y) = row_pt(&v, 2);
    let d = w.pointer(&mut v, x, y);
    w.settle(&v);
    assert_eq!(v.log, ["enter 2"]);
    assert_eq!(d.kind, kind::HOVER);
    assert!(d.consumed);
    let p2 = place_of_row(&mut w, &v, 2);
    assert_eq!(w.shown.w[U_PICK], p2, "the pick names row 2's fill");
    assert_eq!(d.place, p2);
    // over the row's label: the label has no handler, so the row is still what the pointer is on
    w.pointer(&mut v, 30, TOP + 2 * ROW_H + 15);
    assert_eq!(v.log, ["enter 2"], "moving onto a label inside the row changes nothing");
    let (x, y) = row_pt(&v, 3);
    w.pointer(&mut v, x, y);
    assert_eq!(v.log, ["enter 2", "leave 2", "enter 3"]);
    w.settle(&v);
    assert_eq!(v.u.get::<Hot>(), 3);
    // off every handler: the pick is none, row 3 is left
    w.pointer(&mut v, 200, 50);
    assert_eq!(w.shown.w[U_PICK], NONE);
    assert_eq!(v.log.last().unwrap(), "leave 3");
}

#[test]
fn click_press_and_release() {
    let (mut v, mut w) = mounted(1_000);
    let d = w.click(&mut v, BUTTON.x0 + 5, BUTTON.y0 + 5);
    assert_eq!((v.clicks, d.kind, d.consumed), (1, kind::CLICK, true));
    // the label over the button does not take the click
    w.click(&mut v, BUTTON.x0 + 60, BUTTON.y0 + 20);
    assert_eq!(v.clicks, 2);
    // pressed on the button, released elsewhere: no click; `Press` is the button while held
    w.pointer_down(&mut v, BUTTON.x0 + 5, BUTTON.y0 + 5, Button::Primary);
    assert_ne!(w.shown.w[U_PRESS], NONE);
    assert_eq!(w.shown.w[U_PRESS_AT], ((BUTTON.x0 + 5) as u32) << 16 | (BUTTON.y0 + 5) as u32);
    let d = w.pointer_up(&mut v, 200, 50, Button::Primary);
    assert_eq!(w.shown.w[U_PRESS], NONE);
    assert_eq!((v.clicks, d), (2, Dispatch::NOTHING));
    // a row: the handler captured its row
    let (x, y) = row_pt(&v, 5);
    w.click(&mut v, x, y);
    w.settle(&v);
    assert_eq!(v.u.get::<Sel>(), 5);
}

#[test]
fn captures_follow_the_ring() {
    let (mut v, mut w) = mounted(1_000);
    // past the ring's capacity: row 75 lives in a slot that held another row before
    v.scroll_to_row(70);
    w.settle(&v);
    let (x, y) = row_pt(&v, 75);
    w.click(&mut v, x, y);
    assert_eq!(v.log.iter().filter(|l| l.starts_with("select")).last().unwrap(), "select 75");
    // an insert above shifts the rows under the pointer: the captures were rewritten with them
    v.items.insert(0, Txt::new("new"));
    w.settle(&v);
    w.click(&mut v, x, y);
    assert_eq!(v.log.iter().filter(|l| l.starts_with("select")).last().unwrap(), "select 75", "the slot shows row 75 again, whose handler says 75");
}

#[test]
fn keys_and_text_go_to_the_focused_record() {
    let (mut v, mut w) = mounted(100);
    // nothing focused: the window's key handler
    let d = w.key_down(&mut v, Key::Char('t'), Mods::default());
    assert!(d.consumed);
    assert_eq!(v.u.get::<Theme>(), 1);
    // press on the field: it is the nearest focusable record there
    w.click(&mut v, FIELD.x0 + 10, FIELD.y0 + 10);
    assert_ne!(w.shown.w[U_FOCUS], NONE);
    let d = w.text(&mut v, "hi!");
    assert!(d.consumed);
    assert_eq!(d.place, w.shown.w[U_FOCUS]);
    assert_eq!(v.draft(), "hi!");
    // a key while the field is focused is the field's, not the window's
    w.key_down(&mut v, Key::Backspace, Mods::default());
    w.key_down(&mut v, Key::Char('t'), Mods::default());
    assert_eq!(v.draft(), "hi");
    assert_eq!(v.u.get::<Theme>(), 1, "typing 't' into the field did not toggle the theme");
    // the field's row was re-described: reflection reads the new text off the stores
    w.settle(&v);
    let refl = Reflect::learn(&w.root, &v);
    let field = refl.items(&w.root, &w.shown).into_iter().find(|i| i.kind() == "label" && i.ent.path.n == 1).unwrap();
    assert_eq!(field.text().unwrap(), "hi");
    // pressing the background focuses the window (its key handler makes it a tab stop); the field lost focus
    let before = w.shown.w[U_FOCUS];
    w.click(&mut v, 200, 70);
    assert_ne!(w.shown.w[U_FOCUS], before);
    assert!(!w.text(&mut v, "x").consumed, "the window has no text handler");
}

#[test]
fn tab_walks_focusable_records_in_paint_order() {
    let (mut v, mut w) = mounted(5);
    let mut seen = Vec::new();
    for _ in 0..8 {
        let d = w.key_down(&mut v, Key::Tab, Mods::default());
        assert_eq!(d.kind, kind::FOCUS);
        seen.push(w.focused().unwrap());
    }
    // the window, the five rows in row order, the field, then the window again
    let rows: Vec<u32> = seen[1..6].iter().map(|f| f.2).collect();
    assert_eq!(rows, [0, 1, 2, 3, 4]);
    assert_eq!(seen[0], seen[7]);
    assert_eq!(seen[6].0.n, 1, "the field's list is under the root");
    let d = w.key_down(&mut v, Key::Tab, Mods { shift: true, ..Mods::default() });
    assert_eq!(w.focused(), Some(seen[6]), "shift-tab goes back ({d:?})");
}

#[test]
fn focus_survives_scrolling_away_and_back() {
    let (mut v, mut w) = mounted(1_000);
    let (x, y) = row_pt(&v, 4);
    w.click(&mut v, x, y);
    let f = w.focused().unwrap();
    assert_eq!(f.2, 4);
    let p = w.shown.w[U_FOCUS];
    assert_ne!(p, NONE);
    v.scroll_to_row(300);
    w.settle(&v);
    assert_eq!(w.shown.w[U_FOCUS], NONE, "row 4 is not exposed: no place");
    assert_eq!(w.focused(), Some(f), "but it is still what is focused");
    v.scroll_to_row(0);
    w.settle(&v);
    let p4 = place_of_row(&mut w, &v, 4);
    assert_eq!(w.shown.w[U_FOCUS], p4);
}

#[test]
fn drag_captures_the_pointer() {
    let (mut v, mut w) = mounted(100);
    let (x0, y0) = (SPLIT.x0 + 5, SPLIT.y0 + 5);
    w.pointer_down(&mut v, x0, y0, Button::Primary);
    w.pointer(&mut v, x0 + 30, y0 + 200); // far outside the splitter: still its drag
    w.pointer(&mut v, x0 + 40, y0 + 10);
    let d = w.pointer_up(&mut v, x0 + 50, y0, Button::Primary);
    assert_eq!(d.kind, kind::DRAG);
    assert_eq!(v.drags, [(30.0, 200.0, true), (40.0, 10.0, true), (50.0, 0.0, false)]);
    assert!(v.log.is_empty(), "no hover while the drag held the pointer");
}

#[test]
fn reduced_motion_runs_time_ahead() {
    let (_v, mut w) = mounted(10);
    w.tick(1_000);
    assert_eq!(w.shown.w[bough_x::expr::U_TIME], 1_000);
    w.set_env(event::ENV_REDUCED_MOTION);
    w.tick(1_016);
    assert_eq!(w.now(), 1_016, "handlers stamp motions with the clock");
    assert_eq!(w.shown.w[bough_x::expr::U_TIME], 1_016 + event::REDUCED_MOTION_AHEAD);
}

/// After every event of a random script, the retained pixels equal a full redraw and the immediate-mode paint of
/// the view: events change state, and state reaches pixels by the same derivative as any other write.
#[test]
fn pixels_after_events_match_every_oracle() {
    let (mut v, mut w) = mounted(1_000);
    let mut retained = Soft::new(W, H);
    let mut seed = 0x9e37_79b9_7f4a_7c15u64;
    let mut rnd = |n: u32| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed % n as u64) as i32
    };
    for step in 0..300 {
        match rnd(7) {
            0 | 1 => {
                let (x, y) = (rnd(W as u32), rnd(H as u32));
                w.pointer(&mut v, x, y);
            }
            2 => {
                let (x, y) = (rnd(W as u32), rnd(H as u32));
                w.click(&mut v, x, y);
            }
            3 => {
                w.key_down(&mut v, if rnd(2) == 0 { Key::Tab } else { Key::Char('t') }, Mods::default());
            }
            4 => {
                w.text(&mut v, "ab");
            }
            5 => v.scroll_to_row(rnd(900)),
            _ => {
                w.key_down(&mut v, Key::Backspace, Mods::default());
            }
        }
        w.settle(&v);
        w.repick(&mut v);
        w.settle(&v);
        retained.repair(&w.tiles, |s| w.paint(s));
        w.tiles.clear();
        let mut full = Soft::new(W, H);
        w.paint(&mut full);
        assert!(retained.px == full.px, "step {step}: retained != full redraw");
        let mut now = Soft::new(W, H);
        v.view(&mut Immediate::new(&mut now, &w.shown));
        assert!(full.px == now.px, "step {step}: stores != immediate");
    }
}

#[test]
fn the_wheel_scrolls_the_space_under_it() {
    let (mut v, mut w) = mounted(1_000);
    let (x, y) = row_pt(&v, 1);
    let d = w.wheel(&mut v, x, y, 0.0, 3.0);
    assert!(d.consumed);
    assert_eq!(v.u.get::<Scroll>(), 3 * ROW_H, "three lines, a row each");
    w.settle(&v);
    // the end of the list: clamped to the extent
    w.wheel(&mut v, x, y, 0.0, 1.0e6);
    assert_eq!(v.u.get::<Scroll>(), 1_000 * ROW_H - (H - TOP));
    w.wheel(&mut v, x, y, 0.0, -1.0e6);
    assert_eq!(v.u.get::<Scroll>(), 0);
    // outside the body: nothing scrolls
    let d = w.wheel(&mut v, 50, 50, 0.0, 3.0);
    assert!(!d.consumed);
    assert_eq!(v.u.get::<Scroll>(), 0);
}

#[test]
fn enter_activates_the_focused_record() {
    let (mut v, mut w) = mounted(10);
    w.key_down(&mut v, Key::Tab, Mods::default()); // the window
    w.key_down(&mut v, Key::Tab, Mods::default()); // row 0
    w.key_down(&mut v, Key::Tab, Mods::default()); // row 1
    let d = w.key_down(&mut v, Key::Enter, Mods::default());
    assert_eq!((d.kind, d.consumed), (kind::CLICK, true));
    assert_eq!(v.log.last().unwrap(), "select 1");
    let d = w.key_down(&mut v, Key::Char(' '), Mods::default());
    assert_eq!(d.kind, kind::CLICK);
}
