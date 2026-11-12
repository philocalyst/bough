//! Reflection as a walk of the description (`probe.rs`, `tree.rs`, `find.rs`): the situations the card
//! names, on an app that knows nothing of tests (no ids on what tests click, no wrappers). Headless: no GPU.
//!
//! Negative control: `--features mutant_probe` makes the walk step one record instead of a subtree; the
//! attribution gates (`every_record_is_attributed_*`, the part and owner finds) must fail with it.
use bough::find::*;
use bough::prelude::*;
use bough::probe::Verb;
use bough::raw::{App, Config};
use bough::tree::States;

const FONT: &[u8] = zgx_workload::FONT_BYTES;
const S: f32 = 2.0;

/// A third-party component: it exposes one part key and nothing else.
mod picker {
    use bough::prelude::*;
    pub const DAY: Part<BoxStyle> = Part::new("picker.day");
    pub struct MonthPicker {
        pub month: Signal<u32>,
        pub selected: Signal<u32>,
    }
    impl MonthPicker {
        pub fn new(month: u32) -> MonthPicker {
            MonthPicker { month: Signal::new(month), selected: Signal::new(0) }
        }
        fn next(&mut self) {
            *self.month = *self.month % 12 + 1;
        }
    }
    impl View for MonthPicker {
        fn view(&self) -> impl Element<Self> + '_ {
            column((
                row((text(sig!(self.month).map(|m| format!("Month {m}"))), button(">").on_click(MonthPicker::next))).gap(4.0),
                row(each((1..=7u32).map(|d| div().w(20.0).h(20.0).bg(rgb(0xeeeeee)).on_click(move |p: &mut MonthPicker| *p.selected = d).part_at(DAY, d)))).gap(2.0),
            ))
        }
    }
}
use picker::{DAY, MonthPicker};

struct Item {
    title: Signal<String>,
}
impl View for Item {
    fn view(&self) -> impl Element<Self> + '_ {
        row((text(sig!(self.title)),)).h(20.0)
    }
}

struct Page {
    count: Signal<u32>,
    note: Signal<String>,
    scroll: Signal<f32>,
    depart: MonthPicker,
    items: List<Item>,
    plain: u32,
    covered: bool,
}
impl Page {
    fn inc(&mut self) {
        *self.count += 1;
    }
}
impl View for Page {
    fn view(&self) -> impl Element<Self> + '_ {
        column((
            text(sig!(self.count).map(|n| format!("Count: {n}"))),
            row((
                button("+").on_click(Page::inc),
                // an icon button: no text of its own, so its author names it
                div().w(24.0).h(24.0).bg(rgb(0x888888)).on_click(Page::inc).label("Increment"),
            ))
            .gap(4.0),
            text_input(sig!(self.note)),
            embed!(self.depart),
            list(sig!(self.items)),
            div().w(200.0).h(60.0).scroll(sig!(self.scroll)).child(column(each((0..10).map(|i| text(format!("line {i}")))))),
            (text(format!("plain {}", self.plain)), foreign(7).w(40.0).h(40.0)),
            // a button under an overlay: nobody can click it (when `covered`)
            div().relative().w(120.0).h(30.0).child(button("Hidden").on_click(Page::inc)).child(self.covered.then(|| div().abs(0.0, 0.0).w(120.0).h(30.0).bg(rgb(0x000000)))),
        ))
        .gap(4.0)
    }
}

fn page() -> Page {
    let mut items = List::new();
    for t in ["alpha", "beta", "gamma"] {
        items.push(Item { title: Signal::new(t.to_string()) });
    }
    Page { count: Signal::new(0), note: Signal::new(String::new()), scroll: Signal::new(0.0), depart: MonthPicker::new(3), items, plain: 0, covered: false }
}

fn app() -> App<Page> {
    let mut a = App::new(page(), Config::new(320.0, 900.0, S, FONT.to_vec()), false);
    a.poll();
    a
}

#[test]
fn every_record_is_attributed_and_the_walk_is_in_step() {
    let a = app();
    let t = a.tree();
    assert_eq!(t.lost, 0, "the walk went out of step\n{}", t.dump());
    // every record the description emitted has its element (the focus ring and other runtime records aside)
    let unattributed: Vec<_> = t.nodes.iter().filter(|n| n.element.is_none() && !n.inner).collect();
    assert!(unattributed.is_empty(), "records with no element: {unattributed:?}\n{}", t.dump());
    // the walk's elements are the builders the view used, in paint order
    let elems: Vec<&str> = t.nodes.iter().map(|n| n.elem).collect();
    assert_eq!(&elems[..3], &["column", "text", "row"]);
}

#[test]
fn a_button_is_found_by_its_text_and_clicked_through_the_pointer_routes() {
    let mut a = app();
    assert_eq!(a.get(has("Count")).name, "Count: 0");
    let b = a.get("button \"+\"");
    assert_eq!(b.role, Role::Button);
    assert_eq!(b.routes[0].0, Verb::Click);
    assert!(b.routes[0].1.is_some_and(|n| n.ends_with("Page::inc")), "{:?}", b.routes);
    a.click("button \"+\"");
    a.click(role(Role::Button).name("+"));
    assert_eq!(*a.owner.count, 2);
    assert_eq!(a.get(has("Count")).name, "Count: 2");
    // the icon button is named by its label
    a.click(name("Increment"));
    assert_eq!(*a.owner.count, 3);
}

#[test]
fn bound_signals_read_by_path_with_their_values() {
    let mut a = app();
    a.click("button \"+\"");
    let n = a.get(has("Count"));
    let b = n.bind("text").expect("the label is bound");
    assert_eq!(b.path.as_deref(), Some("count"));
    assert_eq!(b.ty, "u32");
    assert_eq!(b.value.as_deref(), Some("1"));
    // write it by path, as an agent would: an ordinary write in a turn
    assert!(a.reflect_set("count", "41"));
    assert_eq!(a.get(has("Count")).name, "Count: 41");
}

#[test]
fn a_part_of_a_third_party_component_by_key_and_index() {
    let mut a = app();
    let d3 = a.get(part(DAY).at(3));
    assert_eq!(d3.part, Some(("picker.day", 3)));
    assert_eq!(d3.role, Role::Button);
    assert!(d3.source.is_some_and(|l| l.file().ends_with("probe.rs")), "{:?}", d3.source);
    assert_eq!(a.find(part(DAY)).len(), 7);
    assert_eq!(a.find("@picker.day[5]").len(), 1);
    a.click(part(DAY).at(3));
    assert_eq!(*a.owner.depart.selected, 3);
}

#[test]
fn an_embedded_view_is_an_owner_with_its_own_fields() {
    let mut a = app();
    let t = a.tree();
    let m = t.get(has("Month"));
    assert_eq!(t.owner_path(m.pos), "Page > MonthPicker(depart)");
    assert_eq!(m.bind("text").and_then(|b| b.path.as_deref()), Some("month"));
    assert_eq!(a.find(owner::<MonthPicker>()).iter().filter(|n| n.role == Role::Button).count(), 8);
    a.click("MonthPicker button \">\"");
    assert_eq!(a.get(has("Month")).name, "Month 4");
    let owners = a.reflect_owners();
    assert_eq!(owners.len(), 2);
    assert!(a.reflect_set_owner(1, "month", "9"));
    assert_eq!(a.get(has("Month")).name, "Month 9");
}

#[test]
fn list_rows_are_items_with_their_own_owner_and_bindings() {
    let a = app();
    let t = a.tree();
    let items = t.find(role(Role::ListItem));
    assert_eq!(items.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(), ["alpha", "beta", "gamma"]);
    let beta = t.get(name("beta").role(Role::Text));
    assert_eq!(beta.bind("text").and_then(|b| b.path.as_deref()), Some("title"));
    assert_eq!(beta.bind("text").and_then(|b| b.value.as_deref()), Some("\"beta\""));
    assert!(t.owner_path(beta.pos).ends_with("Item"));
}

#[test]
fn a_text_field_is_focused_typed_into_and_read() {
    let mut a = app();
    a.type_text(role(Role::TextInput), "hi there");
    assert_eq!(*a.owner.note, "hi there");
    let f = a.get(role(Role::TextInput));
    assert_eq!(f.value.as_deref(), Some("hi there"));
    assert!(f.states.has(States::FOCUSED) && f.states.has(States::FOCUS_WITHIN));
    assert!(a.tree().nodes[0].states.has(States::FOCUS_WITHIN));
    assert_eq!(a.find(":focused").len(), 1);
}

#[test]
fn content_of_a_scroll_box_is_found_and_scrolled_into_view() {
    let mut a = app();
    assert_eq!(a.get("scroll").role, Role::ScrollArea);
    assert!(a.get("\"line 0\"").states.0 & States::HIDDEN.0 == 0);
    assert!(a.get("\"line 9\"").states.has(States::HIDDEN), "{}", a.tree().dump());
    assert_eq!(a.get("\"line 9\"").bounds.is_some(), true);
    a.scroll("scroll", 200.0);
    a.tick(1.0);
    assert!(!a.get("\"line 9\"").states.has(States::HIDDEN), "{}", a.tree().dump());
}

#[test]
fn a_foreign_element_is_in_the_tree_with_its_source() {
    let a = app();
    let f = a.get(role(Role::Foreign));
    assert_eq!(f.foreign, Some(7));
    assert!(f.source.is_some_and(|l| l.file().ends_with("probe.rs")));
    assert_eq!(f.bounds.map(|b| [b[2], b[3]]), Some([80.0, 80.0]));
}

#[test]
fn hover_state_follows_the_pointer() {
    let mut a = app();
    a.hover("button \"+\"");
    assert!(a.get("button \"+\"").states.has(States::HOVERED));
    assert_eq!(a.find(":hovered").len(), 1);
    a.hover(name("Increment"));
    assert!(!a.get("button \"+\"").states.has(States::HOVERED));
}

#[test]
fn a_covered_element_is_refused_and_a_hidden_one_too() {
    let mut a = app();
    assert!(a.try_click("button \"Hidden\"").is_ok());
    a.owner.covered = true;
    a.poll();
    match a.try_click("button \"Hidden\"") {
        Err(ActError::Covered { .. }) => {}
        r => panic!("expected Covered, got {r:?}"),
    }
    assert!(matches!(a.try_click("\"line 9\""), Err(ActError::Hidden(_))));
    assert!(matches!(a.try_click("\"nothing like this\""), Err(ActError::NotFound(_))));
    assert!(matches!(a.try_click(role(Role::Button)), Err(ActError::Many(..))));
}

#[test]
fn a_redescription_keeps_the_walk_in_step() {
    let mut a = app();
    a.turn(|p| p.plain = 7);
    assert_eq!(a.get(has("plain")).name, "plain 7");
    a.turn(|p| p.covered = true);
    let t = a.tree();
    assert_eq!(t.lost, 0, "{}", t.dump());
}

#[test]
fn selector_strings_and_typed_selectors_agree() {
    let a = app();
    let t = a.tree();
    for (s, typed) in [
        ("button \"+\"", role(Role::Button).name("+")),
        ("@picker.day[2]", part(DAY).at(2)),
        ("MonthPicker button", role(Role::Button).within(owner::<MonthPicker>())),
        ("list > listitem", role(Role::ListItem).within(role(Role::List))),
        ("text ~\"line\":nth(3)", role(Role::Text).has("line").nth(3)),
    ] {
        let a: Vec<usize> = t.find(s).iter().map(|n| n.pos).collect();
        let b: Vec<usize> = t.find(typed).iter().map(|n| n.pos).collect();
        assert_eq!(a, b, "{s}");
        assert!(!a.is_empty(), "{s}");
    }
    assert!(Sel::parse("button \"unterminated").is_err());
    assert!(Sel::parse(":nope").is_err());
}

#[test]
fn a_recorded_session_replays_to_the_same_state() {
    let mut a = app();
    let mut turns = Vec::new();
    turns.extend(a.try_click("button \"+\"").unwrap());
    turns.extend(a.try_type(role(Role::TextInput), "replayed").unwrap());
    turns.extend(a.try_click(part(DAY).at(6)).unwrap());
    turns.extend(a.try_scroll("scroll", 120.0).unwrap());
    turns.push(Turn::Tick(0.5));
    a.apply(&Turn::Tick(0.5));
    turns.push(Turn::Set { owner: 1, path: "month".into(), value: "11".into() });
    a.apply(turns.last().unwrap());
    let mut b = app();
    for t in &turns {
        b.apply(t);
    }
    let (ta, tb) = (a.tree(), b.tree());
    assert_eq!(ta.dump(), tb.dump());
    assert_eq!(a.reflect_fields().iter().map(|f| (f.path.clone(), f.value.clone())).collect::<Vec<_>>(), b.reflect_fields().iter().map(|f| (f.path.clone(), f.value.clone())).collect::<Vec<_>>());
}

#[test]
fn the_inspector_picks_a_record_and_reads_its_element_and_ancestors() {
    let a = app();
    let t = a.tree();
    let d = t.get(part(DAY).at(4));
    let (x, y) = d.center_pt(t.scale).unwrap();
    let pos = a.pick_record(x, y).unwrap();
    assert_eq!(pos, d.pos);
    let anc = a.record_ancestors(pos);
    assert!(anc.iter().any(|&p| t.nodes[p].elem == "row"));
    assert!(t.nodes[pos].element.is_some_and(|e| e.contains("LBox")));
    let rv = a.reflect_records();
    assert_eq!(rv[pos].element, t.nodes[pos].element);
    let parts = a.reflect_parts();
    assert_eq!(parts.iter().filter(|p| p.name == "picker.day").count(), 7);
}
