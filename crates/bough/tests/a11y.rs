//! The accessibility projection (`a11y.rs`): roles, names, values, states and actions read from the same tree
//! tests and the inspector read; deltas that, applied to the previous projection, equal a fresh one after
//! every kind of change; actions through the real routes. Headless. Mutants (a delta without its removals,
//! without a renamed node, without a moved node) must fail the delta gate.
use bough::a11y::{A11yAction, Semantic};
use bough::find::*;
use bough::prelude::*;
use bough::raw::{App, Config};
use bough::tree::States;

const FONT: &[u8] = zgx_workload::FONT_BYTES;

struct Row {
    title: Signal<String>,
}
impl View for Row {
    fn view(&self) -> impl Element<Self> + '_ {
        row((text(sig!(self.title)),)).h(20.0)
    }
}

struct Inbox {
    unread: Signal<u32>,
    filter: Signal<String>,
    rows: List<Row>,
    show_help: bool,
    scroll: Signal<f32>,
}
impl Inbox {
    fn read_all(&mut self) {
        *self.unread = 0;
    }
}
impl View for Inbox {
    fn view(&self) -> impl Element<Self> + '_ {
        column((
            text(sig!(self.unread).map(|n| format!("{n} unread"))),
            button("Mark all read").on_click(Inbox::read_all),
            div().w(20.0).h(20.0).bg(rgb(0x444444)).on_click(Inbox::read_all).label("Archive"),
            text_input(sig!(self.filter)),
            list(sig!(self.rows)),
            self.show_help.then(|| text("Help: press the button")),
            div().w(200.0).h(40.0).scroll(sig!(self.scroll)).child(column(each((0..8).map(|i| text(format!("entry {i}")))))),
        ))
        .gap(4.0)
    }
}

fn app() -> App<Inbox> {
    let mut rows = List::new();
    for t in ["from Ada", "from Bo"] {
        rows.push(Row { title: Signal::new(t.into()) });
    }
    let mut a = App::new(Inbox { unread: Signal::new(3), filter: Signal::new(String::new()), rows, show_help: false, scroll: Signal::new(0.0) }, Config::new(320.0, 480.0, 2.0, FONT.to_vec()), false);
    a.poll();
    a
}

#[test]
fn roles_names_values_and_actions() {
    let a = app();
    let s = a.semantic();
    let names: Vec<(Role, &str)> = s.nodes.iter().map(|n| (n.role, n.name.as_str())).collect();
    assert!(names.contains(&(Role::Text, "3 unread")), "{names:?}");
    assert!(names.contains(&(Role::Button, "Mark all read")), "{names:?}");
    assert!(names.contains(&(Role::Button, "Archive")), "{names:?}");
    assert!(names.contains(&(Role::ListItem, "from Ada")), "{names:?}");
    // a button's text is its name, not a second node
    assert!(!names.contains(&(Role::Text, "Mark all read")), "{names:?}");
    let b = s.nodes.iter().find(|n| n.name == "Mark all read").unwrap();
    assert!(b.actions.contains(&A11yAction::Click));
    let input = s.nodes.iter().find(|n| n.role == Role::TextInput).unwrap();
    assert!(input.actions.contains(&A11yAction::SetValue) && input.actions.contains(&A11yAction::Focus));
    // the hierarchy: list items are children of the list
    let list = s.nodes.iter().find(|n| n.role == Role::List).unwrap();
    assert_eq!(list.children.len(), 2);
    // entries the scroll area clips are present, hidden, and can be scrolled into view
    let e7 = s.nodes.iter().find(|n| n.name == "entry 7").unwrap();
    assert!(e7.states.has(States::HIDDEN) && e7.actions.contains(&A11yAction::ScrollIntoView));
}

#[test]
fn actions_go_through_the_routes() {
    let mut a = app();
    let id = |a: &App<Inbox>, name: &str| a.semantic().nodes.iter().find(|n| n.name == name).unwrap().id;
    assert!(a.a11y_act(id(&a, "Archive"), A11yAction::Click, None));
    assert_eq!(*a.owner.unread, 0);
    let f = a.semantic().nodes.iter().find(|n| n.role == Role::TextInput).unwrap().id;
    assert!(a.a11y_act(f, A11yAction::SetValue, Some("urgent")));
    assert_eq!(*a.owner.filter, "urgent");
    assert!(a.a11y_act(f, A11yAction::SetValue, Some("ok")));
    assert_eq!(*a.owner.filter, "ok");
    assert_eq!(a.semantic().focus, Some(f));
    assert!(a.a11y_act(id(&a, "entry 7"), A11yAction::ScrollIntoView, None));
    a.tick(1.0);
    assert!(!a.semantic().nodes.iter().find(|n| n.name == "entry 7").unwrap().states.has(States::HIDDEN));
    // a stale id (the record was replaced) is refused
    let old = id(&a, "from Bo");
    a.turn(|v| {
        v.rows.remove(1);
    });
    assert!(!a.a11y_act(old, A11yAction::Click, None));
}

/// Apply every delta, compare with a fresh projection; return the deltas' sizes.
fn gate(a: &mut App<Inbox>, steps: &[(&str, &dyn Fn(&mut App<Inbox>))]) -> Vec<(String, usize, usize)> {
    let mut held: Semantic = a.semantic();
    let mut sizes = Vec::new();
    for (what, f) in steps {
        f(a);
        let now = a.semantic();
        let d = now.delta(&held);
        let mut applied = held.clone();
        applied.apply(&d, &now);
        assert_eq!(applied, now, "after {what}: applied deltas differ from a fresh projection");
        sizes.push((what.to_string(), d.changed.len(), d.removed.len()));
        held = applied;
    }
    sizes
}

#[test]
fn deltas_applied_equal_a_fresh_projection_and_are_bounded_by_the_change() {
    let mut a = app();
    let n = a.semantic().nodes.len();
    let sizes = gate(
        &mut a,
        &[
            ("label by signal", &|a| a.turn(|v| *v.unread = 9)),
            ("plain-field re-description (show)", &|a| a.turn(|v| v.show_help = true)),
            ("plain-field re-description (hide)", &|a| a.turn(|v| v.show_help = false)),
            ("insert", &|a| {
                a.turn(|v| {
                    v.rows.push(Row { title: Signal::new("from Cy".into()) });
                })
            }),
            ("remove", &|a| {
                a.turn(|v| {
                    v.rows.remove(0);
                })
            }),
            ("focus move", &|a| a.click(role(Role::TextInput))),
            ("type", &|a| a.type_text(role(Role::TextInput), "x")),
            ("scroll", &|a| {
                a.scroll("scroll", 30.0);
                a.tick(1.0);
            }),
        ],
    );
    eprintln!("{n} nodes; deltas (changed, removed): {sizes:?}");
    // what changed, and nothing else: a label is one node, a focus move the two nodes whose focus changed and
    // the field's container whose focus-within did, typing the field, a scroll the scroll area alone; an
    // insertion or removal the parent, the item, and the siblings after it (they moved)
    let at = |what: &str| sizes.iter().find(|s| s.0 == what).map(|s| (s.1, s.2)).unwrap();
    assert_eq!(at("label by signal"), (1, 0), "{sizes:?}");
    assert!(at("focus move").0 <= 3, "{sizes:?}");
    assert_eq!(at("type"), (1, 0), "{sizes:?}");
    // a scroll: the area's offset, and the entries whose visibility it flipped (30 pt over 20 pt rows in a
    // 40 pt port: at most two leave and two enter); never the entries that stayed in or out of view
    assert!(at("scroll").0 <= 5 && at("scroll").1 == 0, "{sizes:?}");
    for w in ["plain-field re-description (show)", "plain-field re-description (hide)", "insert", "remove"] {
        let (c, r) = at(w);
        // the item (and its text) added or removed, the list's children, the moved sibling rows and the one
        // sibling after the list that moved: never the untouched siblings' subtrees (relative offsets)
        assert!(c + r <= 8, "{w}: {c} changed + {r} removed: {sizes:?}");
    }
    // a label change is one node
    assert_eq!(sizes[0].1, 1, "{sizes:?}");
}

#[test]
fn mutated_deltas_fail_the_gate() {
    let mut a = app();
    let held = a.semantic();
    a.turn(|v| {
        v.rows.remove(0);
        *v.unread = 4;
    });
    a.scroll("scroll", 10.0);
    a.tick(1.0);
    let now = a.semantic();
    let d = now.delta(&held);
    assert!(!d.removed.is_empty() && !d.changed.is_empty());
    // omitted removed child
    let mut m = d.clone();
    m.removed.clear();
    let mut x = held.clone();
    x.apply(&m, &now);
    assert_ne!(x, now, "a delta without its removals must fail");
    // stale name: the renamed label left out
    let renamed = now.nodes.iter().find(|n| n.name == "4 unread").unwrap().id;
    let mut m = d.clone();
    m.changed.retain(|&c| c != renamed);
    let mut x = held.clone();
    x.apply(&m, &now);
    assert_ne!(x, now, "a delta without the renamed node must fail");
    // stale geometry: the scrolled area left out (its children's places are relative to its scroll)
    let moved = now.nodes.iter().find(|n| n.role == Role::ScrollArea).unwrap().id;
    let mut m = d.clone();
    m.changed.retain(|&c| c != moved);
    let mut x = held.clone();
    x.apply(&m, &now);
    assert_ne!(x, now, "a delta without the moved node must fail");
}

#[test]
fn a_signal_selector_needs_no_string() {
    let mut a = app();
    {
        let p = &*a.owner;
        assert_eq!(a.get(bound(sig!(p: Inbox => unread))).name, "3 unread");
        assert_eq!(a.find(bound(sig!(p: Inbox => filter))).len(), 1);
    }
    a.turn(|v| *v.unread = 1);
    let p = &*a.owner;
    assert_eq!(a.get(bound(sig!(p: Inbox => unread))).name, "1 unread");
    let row = a.owner.rows.get(0).unwrap();
    // a row's signal names every row's (the selector is the item type and the field)
    assert_eq!(a.find(bound(sig!(row: Row => title))).len(), 2);
}

#[cfg(feature = "a11y")]
#[test]
fn an_accesskit_update_carries_the_projection() {
    let a = app();
    let s = a.semantic();
    let u = s.to_accesskit(None);
    assert_eq!(u.nodes.len(), s.nodes.len());
    assert!(u.tree.is_some());
    let b = u.nodes.iter().find(|(_, n)| n.label() == Some("Mark all read")).unwrap();
    assert_eq!(b.1.role(), accesskit::Role::Button);
}
