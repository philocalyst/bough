//! A transition belongs to the element it animates (round 7): unmounting the element releases it. The repro
//! is the bench lane's (r7/bench `comparisons/bough-workloads/tests/defects.rs`, found by the chat scene): a
//! list row's layout transition, started after the row mounted, in a scroll box whose rows unmount by
//! exposure; a resize in the same step panicked with `layout node is in the Bough`, because the released
//! row's transition kept restyling its dead layout node.
use bough::layout::prelude::*;
use bough::raw::*;

const FONT: &[u8] = zgx_workload::FONT_BYTES;

struct Row {
    h: Signal<f32>,
}
impl View for Row {
    fn view(&self) -> impl Element<Self> + '_ {
        row((block(200.0, 0.0).bg(rgb(0x2f6bff)).h_op(sig!(self.h).transition(Curve::out(0.3))), text("arrival")))
    }
}
struct Feed {
    scroll: Signal<f32>,
    rows: List<Row>,
}
impl View for Feed {
    fn view(&self) -> impl Element<Self> + '_ {
        column((
            column(each((0..200).map(|i| para(format!("history message {i}: a paragraph long enough to wrap onto two or three lines when the window is narrow, and onto one when it is wide")).font(12.0, 18.0).w_pct(1.0)))),
            list(sig!(self.rows)).w_pct(1.0),
        ))
        .w_pct(1.0)
        .h_pct(1.0)
        .scroll(sig!(self.scroll))
    }
}

#[test]
fn a_list_rows_transition_ends_with_the_row() {
    let mut a = App::new(Feed { scroll: Signal::new(0.0), rows: List::new() }, Config::new(400.0, 300.0, 1.0, FONT.to_vec()).once(), false);
    a.frame();
    a.resize(900.0, 400.0);
    a.frame();
    for k in 0..8 {
        // a row arrives mounted at h = 28, the view scrolls to the end, a second turn starts the transition
        // (28 -> 0), and in the same step the window narrows (the history re-wraps taller, so the new row
        // leaves the mounted window) or widens again
        a.turn(|v| {
            v.rows.push(Row { h: Signal::new(28.0) });
        });
        a.wheel_points(100.0, 100.0, 1.0e7);
        a.turn(|v| {
            let n = v.rows.len();
            v.rows.edit(n - 1).h.set(0.0);
        });
        a.tick(1.0 / 60.0);
        a.resize(if k % 2 == 0 { 260.0 } else { 900.0 }, if k % 2 == 0 { 300.0 } else { 400.0 });
        a.tick(1.0 / 60.0);
        a.frame();
    }
    // let every transition still owned by a mounted row finish, then none runs
    for _ in 0..60 {
        a.tick(1.0 / 60.0);
        a.frame();
    }
    assert!(!a.rt.transitions.running(), "a transition is still running after its duration");
}

/// The count of running transitions is the count of running transitions of mounted elements: rows removed
/// while theirs run take them along.
struct Bars {
    rows: List<Row>,
}
impl View for Bars {
    fn view(&self) -> impl Element<Self> + '_ {
        column(list(sig!(self.rows))).w(300.0)
    }
}

#[test]
fn removing_a_row_stops_its_transition() {
    let mut a = App::new(Bars { rows: List::new() }, Config::new(400.0, 300.0, 1.0, FONT.to_vec()).once(), false);
    a.turn(|v| {
        for _ in 0..6 {
            v.rows.push(Row { h: Signal::new(10.0) });
        }
    });
    a.frame();
    a.turn(|v| {
        for i in 0..6 {
            v.rows.edit(i).h.set(40.0);
        }
    });
    a.tick(1.0 / 60.0);
    assert_eq!(a.rt.transitions.active, 6);
    a.turn(|v| {
        v.rows.remove(4);
        v.rows.remove(1);
    });
    a.tick(1.0 / 60.0);
    a.frame();
    assert_eq!(a.rt.transitions.active, 4, "the removed rows' transitions went with them");
    // new rows reuse the released slots: the table does not grow with churn
    let before = a.rt.transitions.len();
    for _ in 0..50 {
        a.turn(|v| {
            v.rows.push(Row { h: Signal::new(10.0) });
        });
        a.turn(|v| {
            let n = v.rows.len();
            v.rows.remove(n - 1);
        });
    }
    assert!(a.rt.transitions.len() <= before + 1, "transition slots are reused ({} -> {})", before, a.rt.transitions.len());
}
