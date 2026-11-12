//! Owner grain (`grain.rs`, `describe::OwnerSeal`): a handler costs what it changed.
//!
//! * A handler that writes only signals (read through `sig!`) does not describe its owner again.
//! * A plain field written by a handler, or a cell the description read plainly (`*self.n`, `&self.s` as a
//!   constant, `self.items.len()`), describes it again.
//! * A description bit-identical to the retained one is stepped over whole.
//!
//! The oracle after every step: the retained tree's full redraw equals a fresh `App` mounted from the same
//! state, and the presented frame equals the full redraw, at ring sizes 1..4. View counts say whether the
//! owner described itself again. Mutants: `--features mutant_grain_plan` (the plan's last plain range is not
//! compared), `mutant_grain_track` (plain reads subscribe nothing), `mutant_owner_seal` (the owner seal answers
//! "unchanged" without comparing): each fails a test here.
use bough::prelude::*;
use bough::raw::{App, Config};
use std::cell::Cell;

const W: usize = 240;

fn config(ring: usize) -> Config {
    let mut c = Config::new(W as f32, 200.0, 1.0, zgx_workload::FONT_BYTES.to_vec());
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
fn gate<V: View>(a: &mut App<V>, fresh: V, ring: usize, context: &str) {
    a.frame();
    let retained = a.reference_pixels();
    compare(&a.presented_pixels(), &retained, &format!("ring {ring} {context}: presented/full"));
    let mut f = App::new(fresh, config(ring), true);
    f.frame();
    compare(&retained, &f.reference_pixels(), &format!("ring {ring} {context}: retained/fresh"));
}

// =============================================================================== an owner of plain and wired state

struct Item {
    tint: Signal<u32>,
    label: u32,
    views: Cell<u32>,
}
impl View for Item {
    fn view(&self) -> impl Element<Self> + '_ {
        self.views.set(self.views.get() + 1);
        row((block(30.0, 16.0).bg(sig!(self.tint).map(|c| rgb(*c))), text(self.label).font(10.0, 12.0).color(rgb(0x000000)))).h(16.0)
    }
}

struct Panel {
    /// plain, read by the description
    width: u32,
    picked: Option<u32>,
    mode: Mode,
    /// plain, never read by the description
    clicks: u32,
    /// read through `sig!` only
    tint: Signal<u32>,
    /// read plainly: as a constant operand and inside a `format!`
    caption: Signal<String>,
    count: Signal<u32>,
    items: List<Item>,
    views: Cell<u32>,
}
#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Calm,
    Loud,
}
impl Panel {
    fn new() -> Panel {
        Panel {
            width: 120,
            picked: None,
            mode: Mode::Calm,
            clicks: 0,
            tint: Signal::new(0x336699),
            caption: Signal::new("caption".into()),
            count: Signal::new(3),
            items: List::from_iter_in((0..4).map(|i| Item { tint: Signal::new(0x204060 + i * 0x101010), label: 100 + i, views: Cell::new(0) })),
            views: Cell::new(0),
        }
    }
    fn copy(&self) -> Panel {
        Panel {
            width: self.width,
            picked: self.picked,
            mode: self.mode,
            clicks: self.clicks,
            tint: Signal::new(*self.tint),
            caption: Signal::new((*self.caption).clone()),
            count: Signal::new(*self.count),
            items: List::from_iter_in(self.items.iter().map(|i| Item { tint: Signal::new(*i.tint), label: i.label, views: Cell::new(0) })),
            views: Cell::new(0),
        }
    }
    fn item_views(&self) -> Vec<u32> {
        self.items.iter().map(|i| i.views.get()).collect()
    }
}
impl View for Panel {
    fn view(&self) -> impl Element<Self> + '_ {
        self.views.set(self.views.get() + 1);
        let c = if self.mode == Mode::Loud { 0xcc2222 } else { 0x22cc22 };
        column((
            block(self.width as f32, 12.0).bg(rgb(c)),
            block(40.0, 12.0).bg(sig!(self.tint).map(|c| rgb(*c))),
            text(&self.caption).font(10.0, 12.0).color(rgb(0x000000)),
            text(format!("n = {} of {}", *self.count, self.items.len())).font(10.0, 12.0).color(rgb(0x000000)),
            self.picked.map(|p| block(20.0 + p as f32, 6.0).bg(rgb(0x8800aa))),
            list(sig!(self.items)).w(200.0),
        ))
        .w(220.0)
        .bg(rgb(0xffffff))
    }
}

/// Every kind of change, one at a time: whether the root (and which rows) described themselves again, and the
/// pixels against a fresh mount.
#[test]
fn a_handler_costs_what_it_changed() {
    for ring in 1..=4 {
        let mut a = App::new(Panel::new(), config(ring), true);
        a.frame();
        let views = |a: &App<Panel>| a.owner.views.get();
        let mut step = |a: &mut App<Panel>, what: &str, redescribes: bool, f: &dyn Fn(&mut Panel)| {
            let v0 = views(a);
            a.turn(|p| f(p));
            let described = views(a) != v0;
            assert_eq!(described, redescribes, "ring {ring} {what}: the root described itself again: {described}");
            let fresh = a.owner.copy();
            gate(a, fresh, ring, what);
        };
        step(&mut a, "nothing", false, &|_| {});
        step(&mut a, "a wired signal", false, &|p| drop(p.tint.set(0x993311)));
        step(&mut a, "a plain field the view reads", true, &|p| p.width = 150);
        step(&mut a, "a plain field the view does not read", true, &|p| p.clicks += 1);
        step(&mut a, "a fieldless enum", true, &|p| p.mode = Mode::Loud);
        step(&mut a, "an Option appears", true, &|p| p.picked = Some(5));
        step(&mut a, "an Option's payload", true, &|p| p.picked = Some(40));
        step(&mut a, "an Option goes", true, &|p| p.picked = None);
        step(&mut a, "a signal read as a constant operand", true, &|p| p.caption.push_str(" and more"));
        step(&mut a, "a signal read inside format!", true, &|p| drop(p.count.set(9)));
        step(&mut a, "a list's length (read plainly)", true, &|p| drop(p.items.push(Item { tint: Signal::new(0x445566), label: 7, views: Cell::new(0) })));
        let rows0 = a.owner.item_views();
        step(&mut a, "a row's wired signal, through the list", false, &|p| drop(p.items.edit(1).tint.set(0x00ffff)));
        assert_eq!(a.owner.item_views(), rows0, "ring {ring}: a row whose only change is a wired signal is not described again");
        step(&mut a, "a row's plain field, through the list", false, &|p| p.items.edit(2).label = 555);
        let rows1 = a.owner.item_views();
        assert_eq!(rows1[2], rows0[2] + 1, "ring {ring}: the row whose plain field changed is described again");
        assert_eq!((rows1[0], rows1[1], rows1[3]), (rows0[0], rows0[1], rows0[3]), "ring {ring}: and no other row");
        step(&mut a, "a list's order (read plainly)", true, &|p| p.items.swap(0, 3));
        step(&mut a, "nothing again", false, &|_| {});
    }
}

// =============================================================================== the owner seal

struct Board {
    note: &'static str,
    unread: u32,
    views: Cell<u32>,
}
impl View for Board {
    fn view(&self) -> impl Element<Self> + '_ {
        self.views.set(self.views.get() + 1);
        column((text(self.note).font(10.0, 12.0).color(rgb(0x000000)), block(30.0, 10.0).bg(rgb(0x2266aa)))).w(200.0).bg(rgb(0xffffff))
    }
}

/// A description identical to the retained one is stepped over; one that differs only in text bytes of the
/// same length is not (the `mutant_owner_seal` mutant steps over it and leaves stale text).
#[test]
fn the_owner_seal_steps_over_only_identical_descriptions() {
    for ring in 1..=4 {
        let mut a = App::new(Board { note: "first note", unread: 0, views: Cell::new(0) }, config(ring), true);
        a.frame();
        for (what, f) in [
            ("a plain field the view does not read", (|b: &mut Board| b.unread += 1) as fn(&mut Board)),
            ("text of the same length", |b| b.note = "other note"),
            ("text of another length", |b| b.note = "a longer note than before"),
            ("back", |b| b.note = "first note"),
        ] {
            a.turn(f);
            let fresh = Board { note: a.owner.note, unread: a.owner.unread, views: Cell::new(0) };
            gate(&mut a, fresh, ring, what);
        }
    }
}
