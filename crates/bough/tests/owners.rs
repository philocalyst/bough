//! `changed` on any owner (a list row seeing its own list's deltas), producers owned by rows (revoked with
//! them), reader faults and `boundary` (loading -> child / failed, by clip switches; pixel-free here: the
//! shown branch is read from the clips).
use bough::layout::prelude::*;
use bough::raw::*;

const FONT: &[u8] = zgx_workload::FONT_BYTES;

struct Leaf {
    v: Signal<u32>,
}
impl View for Leaf {
    fn view(&self) -> impl Element<Self> + '_ {
        label(sig!(self.v).map(|v| format!("leaf {v}")))
    }
}
/// A row owning a list: its `changed` counts the deltas of its own list.
struct Group {
    leaves: List<Leaf>,
    seen: Signal<u32>,
    q: Signal<u32>,
    r: Reader<String>,
}
fn req(q: &u32) -> Request<String> {
    let q = *q;
    Request::fallible(move || if q % 3 == 2 { Err(format!("bad {q}")) } else { Ok(format!("got {q}")) })
}
impl View for Group {
    fn view(&self) -> impl Element<Self> + '_ {
        column((
            label(sig!(self.seen).map(|s| format!("seen {s}"))),
            list(sig!(self.leaves)),
            label(sig!(self.r).map(|v: Option<&String>| v.cloned().unwrap_or_else(|| "none".into()))),
            produce(sig!(self.q), sig!(self.r), req),
        ))
    }
    fn changed(&mut self, d: Changed<'_, Self>) {
        if let Some(ops) = d.list(sig!(self.leaves)) {
            *self.seen += ops.len() as u32;
        }
    }
}
struct Root {
    groups: List<Group>,
}
impl View for Root {
    fn view(&self) -> impl Element<Self> + '_ {
        list(sig!(self.groups))
    }
}

fn texts(a: &App<Root>) -> Vec<String> {
    let mut v: Vec<String> = a.rt.texts.iter().map(|t| String::from_utf8_lossy(t.bytes()).into_owned()).filter(|s| !s.is_empty()).collect();
    v.sort();
    v
}

#[test]
fn rows_see_their_own_boundaries_and_own_producers() {
    let g = |q| Group { leaves: List::new(), seen: Signal::new(0), q: Signal::new(q), r: Reader::new() };
    let mut a = App::new(Root { groups: List::from_iter_in([g(0), g(1)]) }, Config::new(300.0, 400.0, 1.0, FONT.to_vec()).once(), false);
    a.frame();
    // a row edits its own list: its changed sees the ops and its count re-renders in the same turn
    a.turn(|r| {
        let gr = r.groups.edit(0);
        gr.leaves.push(Leaf { v: Signal::new(1) });
        gr.leaves.push(Leaf { v: Signal::new(2) });
    });
    a.frame();
    assert_eq!(*a.owner.groups.get(0).unwrap().seen, 2);
    assert!(texts(&a).contains(&"seen 2".to_string()), "{:?}", texts(&a));
    // row-owned producers: both rows requested at mount; completions settle with the rows' class
    while a.run_jobs(8) > 0 {}
    a.poll();
    a.frame();
    let t = texts(&a);
    assert!(t.contains(&"got 0".to_string()) && t.contains(&"got 1".to_string()), "{t:?}");
    // a failing request keeps the last value and carries the fault
    a.turn(|r| *r.groups.edit(0).q = 2);
    while a.run_jobs(8) > 0 {}
    a.poll();
    assert_eq!(a.owner.groups.get(0).unwrap().r.fault(), Some("bad 2"));
    assert_eq!(a.owner.groups.get(0).unwrap().r.latest().map(|s| s.as_str()), Some("got 0"));
    // removing a row revokes its producer: an in-flight completion is discarded
    a.turn(|r| *r.groups.edit(1).q = 4);
    a.turn(|r| {
        r.groups.remove(1);
    });
    while a.run_jobs(8) > 0 {}
    a.poll();
    a.frame();
    assert_eq!(a.owner.groups.len(), 1);
}

struct B {
    q: Signal<u32>,
    r: Reader<String>,
}
impl View for B {
    fn view(&self) -> impl Element<Self> + '_ {
        (
            produce(sig!(self.q), sig!(self.r), req),
            boundary(sig!(self.r)).loading(text("loading").at(0.0, 0.0)).failed(text("failed").at(0.0, 20.0)).child(text(sig!(self.r).map(|v: Option<&String>| v.cloned().unwrap_or_default())).at(0.0, 40.0)),
        )
    }
}

#[test]
fn boundary_shows_loading_then_child_or_failed() {
    let shown = |a: &App<B>| -> Vec<u16> { vec![a.rt.boundary_shown(0) as u16] };
    let mut a = App::new(B { q: Signal::new(2), r: Reader::new() }, Config::new(200.0, 100.0, 1.0, FONT.to_vec()).once(), false);
    a.frame();
    assert_eq!(shown(&a), vec![0], "loading before first readiness");
    while a.run_jobs(8) > 0 {}
    a.poll();
    assert_eq!(shown(&a), vec![1], "failed, no value");
    a.turn(|b| *b.q = 3);
    while a.run_jobs(8) > 0 {}
    a.poll();
    assert_eq!(shown(&a), vec![2], "child once a value exists");
    a.turn(|b| *b.q = 5);
    while a.run_jobs(8) > 0 {}
    a.poll();
    assert_eq!(shown(&a), vec![2], "a failure after a value keeps the stale child");
    assert_eq!(a.owner.r.fault(), Some("bad 5"));
}
