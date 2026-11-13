//! The independent review's probes for the surface lane's area (runtime/lanes/independent-review/
//! recent-20261112, findings C01 events part, C03, C05, C06, S01), as tests of this crate. The scenes are the
//! review's, unchanged except for formatting.
use bough_x::event::{Button, Key, Mods, U_FOCUS};
use bough_x::prelude::*;
use bough_x::reflect::Reflect;
use bough_x::soft::Soft;

lens!(Offset: i32 = 0);
lens!(Scroll: i32 = 1);

struct Moving {
    u: U,
    clicks: u32,
}
impl View for Moving {
    fn block(&self) -> &U {
        &self.u
    }
    fn view<S: Sink<Self>>(&self, s: &mut S) {
        s.row(|| ([Rect::new(0, 0, 10, 10)], Outline::<1, _, _>(Kc::<0xffffffff>, un::<Offset>().map(|x| (x, 0))).on_click(|v: &mut Moving| v.clicks += 1)));
    }
}

/// C03: a record its paint moves is picked where it is drawn.
#[test]
fn moving_target_is_picked_at_visible_position() {
    let mut v = Moving { u: U::default(), clicks: 0 };
    v.u.set::<Offset>(40);
    let mut w = World::<Soft>::new(100, 100, 2);
    w.mount(&v);
    w.click(&mut v, 40, 2);
    assert_eq!(v.clicks, 1);
}

/// C06: the pointer entry point moves the pointer words in the same operation.
#[test]
fn pointer_updates_expression_words() {
    let mut v = Moving { u: U::default(), clicks: 0 };
    let mut w = World::<Soft>::new(100, 100, 2);
    w.mount(&v);
    w.pointer(&mut v, 35, 41);
    assert_eq!((w.shown.w[bough_x::expr::U_PTR_X], w.shown.w[bough_x::expr::U_PTR_Y]), (35, 41));
}

struct Rows {
    u: U,
    shape: Vec<Rect>,
    items: Rel<u32>,
    clicked: u32,
    h: i32,
}
impl View for Rows {
    fn block(&self) -> &U {
        &self.u
    }
    fn view<S: Sink<Self>>(&self, s: &mut S) {
        s.scope(Space { clip: Rect::new(0, 0, 100, self.h), base: (0, 0), scroll: Scroll::SLOT as u8 }, |s| {
            s.list(&List { n: self.items.len() as u32, row_h: 10, x: 0, y: 0, shape: &self.shape, edits: &self.items.journal }, |r| {
                let id = *self.items.get(r);
                Fill(Kc::<0xffff0000>).on_click(move |v: &mut Rows| v.clicked = id).focusable()
            })
        });
    }
}
fn rows() -> Rows {
    Rows { u: U::default(), shape: vec![Rect::new(0, 0, 20, 10)], items: Rel::new((0..200).collect()), clicked: 999, h: 30 }
}

/// C05: focus follows its item through an insert before it (and is dropped when its item is removed).
#[test]
fn focus_follows_item_through_insert() {
    let mut v = rows();
    let mut w = World::<Soft>::new(100, 100, 2);
    w.mount(&v);
    w.click(&mut v, 5, 15);
    assert_eq!(v.clicked, 1);
    v.items.insert(0, 900);
    w.settle(&v);
    let place = w.shown.w[U_FOCUS];
    w.key_down(&mut v, Key::Enter, Mods::default());
    let logical = w.focused().unwrap();
    assert_eq!(logical.2, 2, "focused item 1 moved to row 2; place={place}");
    assert_eq!(v.clicked, 1, "Enter activated the focused item, now at row 2");
    v.items.remove(2);
    w.settle(&v);
    assert!(w.focused().is_none(), "the focused item was removed");
}

/// C05: a press whose place shows another row at release (a scroll reused the slot) does not activate it.
#[test]
fn press_is_cancelled_when_slot_reused() {
    let mut v = rows();
    let mut w = World::<Soft>::new(100, 100, 2);
    w.mount(&v);
    w.pointer_down(&mut v, 5, 5, Button::Primary);
    v.u.set::<Scroll>(640);
    w.settle(&v);
    w.pointer_up(&mut v, 5, 5, Button::Primary);
    assert_eq!(v.clicked, 999, "press on item 0 activated item 64 after ring reuse");
}

#[repr(C)]
struct A {
    u: U,
    n: u32,
}
#[repr(C)]
struct B {
    u: U,
    n: u32,
}
impl View for A {
    fn block(&self) -> &U {
        &self.u
    }
    fn view<S: Sink<Self>>(&self, s: &mut S) {
        s.row(|| ([Rect::new(0, 0, 30, 30)], Fill(Kc::<0xffffffff>).on_click(|v: &mut A| v.n += 1)));
    }
}
impl View for B {
    fn block(&self) -> &U {
        &self.u
    }
    fn view<S: Sink<Self>>(&self, s: &mut S) {
        s.row(|| ([Rect::new(0, 0, 30, 30)], Fill(Kc::<0xffffffff>).on_click(|v: &mut B| v.n += 100)));
    }
}

/// C01, events: a world mounted with one view type refuses an event that brings another, before any handler
/// (instantiated for the first type) is called with it.
#[test]
#[should_panic(expected = "an event for a view of another type")]
fn world_rejects_different_view_type() {
    let a = A { u: U::default(), n: 0 };
    let mut b = B { u: U::default(), n: 0 };
    let mut w = World::<Soft>::new(100, 100, 0);
    w.mount(&a);
    w.click(&mut b, 5, 5);
}

/// S01: reflection learned from one world's stores is refused on another's (it would read records as the
/// first world's types).
#[test]
#[should_panic(expected = "reflection used on a store tree it was not learned from")]
fn reflection_is_tied_to_its_store_tree() {
    let a = A { u: U::default(), n: 0 };
    let mut wa = World::<Soft>::new(100, 100, 0);
    wa.mount(&a);
    let r = Reflect::learn(&wa.root, &a);
    let mut v = rows();
    let mut wb = World::<Soft>::new(100, 100, 2);
    wb.mount(&v);
    v.clicked = 0;
    let _ = r.items(&wb.root, &wb.shown);
}

/// S01: a record whose type holds a closure is a different type in the reflecting sink's instantiation of
/// `view`; its column is refused, never read as another type.
#[test]
fn records_of_another_instantiation_are_refused() {
    let v = Moving { u: U::default(), clicks: 0 };
    let mut w = World::<Soft>::new(100, 100, 2);
    w.mount(&v);
    let r = Reflect::learn(&w.root, &v);
    assert_eq!(r.refused(), 1, "the outline's offset is a closure");
    assert!(r.items(&w.root, &w.shown).is_empty());
}
