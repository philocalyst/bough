//! Record state flags read by variant tables (`flags.rs`): ten situations, one mechanism.
//!
//! Row hover, pressed, a keyboard focus ring, focus-within on a form, a disabled subtree, single and shift-range
//! selection over 10,000 rows, a checked toggle, find-in-page over many matches and a drag-over target are all
//! `.when(flag, look)` on an element plus a flag write (input's, a list item's, a description's). After every
//! step the retained tree's full redraw equals a fresh `App` mounted from the same state (the same pointer,
//! press and focus replayed), the presented frame equals the full redraw, at ring sizes 1..4; no step
//! re-describes an owner (view counts), and expected colours are probed where retained and fresh could agree
//! on a wrong picture. Mutant: `--features mutant_flags_range` (a list's flag change skips its last mounted
//! row): `range_selection_and_search_hits` fails.
use bough::input::Button;
use bough::prelude::*;
use bough::raw::{App, Config};
use std::cell::Cell;

const W: usize = 240;
const H: f32 = 200.0;
const ROW: f32 = 16.0;

fn config(ring: usize) -> Config {
    let mut c = Config::new(W as f32, H, 1.0, zgx_workload::FONT_BYTES.to_vec());
    c.ring = ring;
    c
}
fn compare(a: &[u8], b: &[u8], context: &str) {
    assert_eq!(a.len(), b.len(), "{context}: dimensions");
    let n = a.chunks(4).zip(b.chunks(4)).filter(|(a, b)| a != b).count();
    if n != 0 {
        let first = a.chunks(4).zip(b.chunks(4)).position(|(a, b)| a != b).unwrap();
        panic!("{context}: {n} unequal pixels, the first at ({}, {})", first % W, first / W);
    }
}
fn px(p: &[u8], x: f32, y: f32) -> u32 {
    let i = (y as usize * W + x as usize) * 4;
    (p[i + 2] as u32) << 16 | (p[i + 1] as u32) << 8 | p[i] as u32
}

// =============================================================================== a list of 10,000 rows

const N: usize = 10_000;
const SEL: u32 = 0x2f6bff;
const HIT: u32 = 0xffd400;
const HOV: u32 = 0xdddddd;
const CHK: u32 = 0x22aa55;

struct Line {
    id: u32,
    views: Cell<u32>,
}
impl View for Line {
    fn view(&self) -> impl Element<Self> + '_ {
        self.views.set(self.views.get() + 1);
        row((block(12.0, 12.0).bg(rgb(0xbbbbbb)).when(Flag::CHECKED, |s| s.bg(rgb(CHK))), text(self.id).font(10.0, 12.0).color(rgb(0x202020)).when(Flag::SELECTED, |s| s.color(rgb(0xffffff)))))
            .w(200.0)
            .h(ROW)
            .bg(rgb(0xf5f5f5))
            .when(Flag::SEARCH_HIT, |s| s.bg(rgb(HIT)))
            .when(Flag::HOVERED, |s| s.bg(rgb(HOV)))
            .when(Flag::SELECTED, |s| s.bg(rgb(SEL)))
    }
}
struct Sheet {
    lines: List<Line>,
    y: Signal<f32>,
    views: Cell<u32>,
}
impl Sheet {
    fn new() -> Sheet {
        Sheet { lines: List::from_iter_in((0..N as u32).map(|id| Line { id, views: Cell::new(0) })), y: Signal::new(0.0), views: Cell::new(0) }
    }
    /// The same state: the items and their flags, the offset.
    fn copy(&self) -> Sheet {
        let mut s = Sheet { lines: List::from_iter_in(self.lines.iter().map(|l| Line { id: l.id, views: Cell::new(0) })), y: Signal::new(*self.y), views: Cell::new(0) };
        for (k, slot) in self.lines.slots().enumerate() {
            for f in [Flag::SELECTED, Flag::CHECKED, Flag::SEARCH_HIT] {
                if self.lines.has_flag(slot, f) {
                    s.lines.set_flag_at(k, f, true);
                }
            }
        }
        s
    }
}
impl View for Sheet {
    fn view(&self) -> impl Element<Self> + '_ {
        self.views.set(self.views.get() + 1);
        div().w(220.0).h(H).bg(rgb(0xffffff)).scroll(sig!(self.y)).child(list(sig!(self.lines)).w(200.0))
    }
}

fn sheet_gate(a: &mut App<Sheet>, ring: usize, what: &str, pointer: Option<(f32, f32)>) -> Vec<u8> {
    a.frame();
    let retained = a.reference_pixels();
    compare(&a.presented_pixels(), &retained, &format!("ring {ring} {what}: presented/full"));
    let mut f = App::new(a.owner.copy(), config(ring), true);
    if let Some((x, y)) = pointer {
        f.pointer_move(x, y);
    }
    f.frame();
    let fr = f.reference_pixels();
    if std::env::var_os("FLAGS_DBG").is_some() {
        for (x, y) in [(5.0, 40.0), (14.0, 40.0), (20.0, 40.0), (30.0, 40.0), (150.0, 40.0)] {
            eprintln!("{what} ({x},{y}): retained {:06x} fresh {:06x}", px(&retained, x, y), px(&fr, x, y));
        }
    }
    compare(&retained, &fr, &format!("ring {ring} {what}: retained/fresh"));
    retained
}

/// Single selection, a shift-range over thousands of rows (most not mounted), checked items, many search
/// hits, hover over a selected row; no owner describes itself again.
#[test]
fn range_selection_and_search_hits() {
    for ring in 1..=4 {
        let mut a = App::new(Sheet::new(), config(ring), true);
        a.frame();
        let views = |a: &App<Sheet>| (a.owner.views.get(), a.owner.lines.iter().take(20).map(|l| l.views.get()).sum::<u32>());
        let v0 = views(&a);
        let y = |k: usize| k as f32 * ROW + ROW / 2.0;
        a.turn(|s| s.lines.set_flag_at(2, Flag::SELECTED, true));
        let p = sheet_gate(&mut a, ring, "select row 2", None);
        assert_eq!(px(&p, 150.0, y(2)), SEL, "ring {ring}: row 2 shows selected");
        assert_eq!(px(&p, 150.0, y(1)), 0xf5f5f5);
        // shift-click: rows 2..=7000 (the mounted ones show it now, the rest when they mount)
        a.turn(|s| s.lines.set_flag_range(2..7001, Flag::SELECTED, true));
        let p = sheet_gate(&mut a, ring, "range 2..=7000", None);
        for k in [2, 3, 7, 11] {
            assert_eq!(px(&p, 150.0, y(k)), SEL, "ring {ring}: row {k} selected");
        }
        assert_eq!(views(&a), v0, "ring {ring}: selecting re-described no owner");
        // scroll to rows that mounted after the range was set (rows mount: their views run, a mount)
        a.turn(|s| drop(s.y.set(6990.0 * ROW)));
        let p = sheet_gate(&mut a, ring, "scrolled into the range's end", None);
        assert_eq!(px(&p, 150.0, ROW / 2.0 + 0.0), SEL, "ring {ring}: row 6990 (mounted later) selected");
        assert_eq!(px(&p, 150.0, 11.0 * ROW + ROW / 2.0), 0xf5f5f5, "ring {ring}: row 7001 not selected");
        a.turn(|s| drop(s.y.set(0.0)));
        // single selection again: clear all, select one
        a.turn(|s| {
            s.lines.clear_flag(Flag::SELECTED);
            s.lines.set_flag_at(5, Flag::SELECTED, true);
        });
        let p = sheet_gate(&mut a, ring, "select 5 only", None);
        assert_eq!(px(&p, 150.0, y(3)), 0xf5f5f5);
        assert_eq!(px(&p, 150.0, y(5)), SEL);
        // checked: the row's box only
        a.turn(|s| s.lines.set_flag_at(4, Flag::CHECKED, true));
        let p = sheet_gate(&mut a, ring, "check row 4", None);
        assert_eq!(px(&p, 6.0, y(4)), CHK);
        // find in page: every row whose id ends in 3 (1,000 hits, 1 mounted in 10)
        a.turn(|s| {
            for k in (3..N).step_by(10) {
                s.lines.set_flag_at(k, Flag::SEARCH_HIT, true);
            }
        });
        let p = sheet_gate(&mut a, ring, "search hits", None);
        assert_eq!(px(&p, 150.0, y(3)), HIT);
        assert_eq!(px(&p, 150.0, y(5)), SEL);
        // hover over a hit and over the selected row
        a.pointer_move(150.0, y(3));
        let p = sheet_gate(&mut a, ring, "hover a hit", Some((150.0, y(3))));
        assert_eq!(px(&p, 150.0, y(3)), HOV);
        a.pointer_move(150.0, y(5));
        let p = sheet_gate(&mut a, ring, "hover the selected row", Some((150.0, y(5))));
        assert_eq!(px(&p, 150.0, y(5)), SEL);
        assert_eq!(px(&p, 150.0, y(3)), HIT);
        a.pointer_leave();
        a.turn(|s| s.lines.clear_flag(Flag::SEARCH_HIT));
        sheet_gate(&mut a, ring, "hits cleared", None);
        assert_eq!(views(&a).0, v0.0, "ring {ring}: the sheet never described itself again");
    }
}

// =============================================================================== a form

struct Form {
    locked: bool,
    sent: u32,
    count: u32,
    note: Signal<String>,
    hand: Signal<f32>,
    dropped: u32,
    views: Cell<u32>,
}
const RING: u32 = 0x7ee3ff;
const FW: u32 = 0xeef3ff;
const PRESS: u32 = 0x113355;
const OVER: u32 = 0x55aa55;
const GREY: u32 = 0x999999;
impl Form {
    fn new() -> Form {
        Form { locked: false, sent: 0, count: 0, note: Signal::new(String::new()), hand: Signal::new(0.0), dropped: 0, views: Cell::new(0) }
    }
    fn copy(&self) -> Form {
        Form { locked: self.locked, sent: self.sent, count: self.count, note: Signal::new((*self.note).clone()), hand: Signal::new(*self.hand), dropped: self.dropped, views: Cell::new(0) }
    }
}
impl View for Form {
    fn view(&self) -> impl Element<Self> + '_ {
        self.views.set(self.views.get() + 1);
        column((
            // a form: its background shows focus within it; locked, its whole subtree is disabled
            column((
                block(100.0, 20.0).bg(rgb(0x3366cc)).border(2.0, rgb(0x3366cc)).focusable().focus(|s| s.border(rgb(RING))).disabled(|s| s.bg(rgb(GREY))),
                block(100.0, 20.0).bg(rgb(0x226622)).on_click(|f: &mut Form| f.sent += 1).active(|s| s.bg(rgb(PRESS))).disabled(|s| s.bg(rgb(GREY))),
            ))
            .gap(4.0)
            .p(6.0)
            .bg(rgb(0xffffff))
            .when(Flag::FOCUS_WITHIN, |s| s.bg(rgb(FW)))
            .flag(Flag::DISABLED, self.locked),
            // a drag source and a drop target
            row((
                block(30.0, 30.0).bg(rgb(0xaa3333)).on_drag(|f: &mut Form, e| {
                    *f.hand = e.dx;
                    if !e.on && e.dx > 60.0 {
                        f.dropped += 1;
                    }
                }),
                block(30.0, 30.0).margin(80.0, 0.0, 0.0, 0.0).bg(rgb(0xcccccc)).when(Flag::DRAG_OVER, |s| s.bg(rgb(OVER))),
            )),
        ))
        .w(220.0)
        .h(H)
        .bg(rgb(0xf0f0f0))
    }
}

fn form_gate(a: &mut App<Form>, ring: usize, what: &str, replay: &dyn Fn(&mut App<Form>)) -> Vec<u8> {
    a.frame();
    let retained = a.reference_pixels();
    compare(&a.presented_pixels(), &retained, &format!("ring {ring} {what}: presented/full"));
    let mut f = App::new(a.owner.copy(), config(ring), true);
    replay(&mut f);
    f.frame();
    compare(&retained, &f.reference_pixels(), &format!("ring {ring} {what}: retained/fresh"));
    retained
}

/// Pressed, a keyboard focus ring, focus-within, a disabled subtree (look and behaviour), a drag-over target.
#[test]
fn input_flags_and_a_disabled_subtree() {
    for ring in 1..=4 {
        let mut a = App::new(Form::new(), config(ring), true);
        a.frame();
        let v0 = a.owner.views.get();
        // the field: (6+50, 6+10); the button: (56, 40); the drag source (15, 75), the target (125, 75)
        a.pointer_move(56.0, 40.0);
        a.pointer_down(56.0, 40.0, Button::Primary);
        let p = form_gate(&mut a, ring, "pressed", &|f| {
            f.pointer_move(56.0, 40.0);
            f.pointer_down(56.0, 40.0, Button::Primary);
        });
        assert_eq!(px(&p, 56.0, 40.0), PRESS, "ring {ring}: the pressed look");
        a.pointer_up(56.0, 40.0, Button::Primary);
        assert_eq!(a.owner.sent, 1);
        let views_after_click = a.owner.views.get();
        let p = form_gate(&mut a, ring, "released", &|f| f.pointer_move(56.0, 40.0));
        assert_eq!(px(&p, 56.0, 40.0), 0x226622);
        // keyboard focus: the ring on the field, focus-within on the form
        a.key_down(Key::Tab, Mods::default());
        let p = form_gate(&mut a, ring, "tab to the field", &|f| {
            f.pointer_move(56.0, 40.0);
            f.key_down(Key::Tab, Mods::default());
        });
        assert_eq!(px(&p, 7.0, 16.0), RING, "ring {ring}: the focus ring (the field's border)");
        assert_eq!(px(&p, 2.0, 2.0), FW, "ring {ring}: focus within the form");
        // lock the form: its subtree is disabled (grey), the button runs no route
        a.turn(|f| f.locked = true);
        let p = form_gate(&mut a, ring, "locked", &|f| {
            f.pointer_move(56.0, 40.0);
            f.key_down(Key::Tab, Mods::default());
        });
        assert_eq!((px(&p, 56.0, 16.0), px(&p, 56.0, 40.0)), (GREY, GREY), "ring {ring}: the disabled subtree");
        a.pointer_down(56.0, 40.0, Button::Primary);
        a.pointer_up(56.0, 40.0, Button::Primary);
        assert_eq!(a.owner.sent, 1, "ring {ring}: a disabled button runs no route");
        a.turn(|f| f.locked = false);
        let p = form_gate(&mut a, ring, "unlocked", &|f| f.pointer_move(56.0, 40.0));
        assert_eq!(px(&p, 56.0, 40.0), 0x226622);
        // drag the source over the target and drop
        a.pointer_move(15.0, 75.0);
        a.pointer_down(15.0, 75.0, Button::Primary);
        a.pointer_move(125.0, 75.0);
        let p = form_gate(&mut a, ring, "dragging over the target", &|f| {
            f.pointer_move(15.0, 75.0);
            f.pointer_down(15.0, 75.0, Button::Primary);
            f.pointer_move(125.0, 75.0);
        });
        assert_eq!(px(&p, 125.0, 75.0), OVER, "ring {ring}: the drop target shows the drag over it");
        a.pointer_up(125.0, 75.0, Button::Primary);
        assert_eq!(a.owner.dropped, 1);
        let p = form_gate(&mut a, ring, "dropped", &|f| f.pointer_move(125.0, 75.0));
        assert_eq!(px(&p, 125.0, 75.0), 0xcccccc);
        // only the handlers that changed the form's plain state described it again (the click, the lock and
        // unlock, the drop)
        assert!(views_after_click == v0 + 1, "ring {ring}: the press itself did not describe the form");
    }
}
