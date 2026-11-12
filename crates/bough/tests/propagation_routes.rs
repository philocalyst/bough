//! Routes after a re-description and routes on owners below the root (round 6, defects 2 and 3).
//!
//! * A re-described element's route takes the new description's captures (`On::update`): the mutant is
//!   `--features mutant_route_stale` (the route keeps the closure it was mounted with).
//! * A route's receiver is resolved along the owner chain when it is mounted (the design's L4): a list row's
//!   handler runs on the row, marks its cells and re-describes its plain fields; a handler may also name an
//!   owner enclosing the element. A route never runs on an owner that is gone, or on the item that reused its
//!   slot: the mutant is `--features mutant_route_owner` (no liveness check of the pressed element, no
//!   generation check of the owner).
//!
//! After every operation the retained pixels equal a fresh mount of the same state, at ring sizes 1..4.
use bough::app::{App, Config, View};
use bough::input::Button;
use bough::prelude::*;

const W: usize = 240;

fn config(ring: usize) -> Config {
    let mut c = Config::new(W as f32, 200.0, 1.0, zgx_workload::FONT_BYTES.to_vec());
    c.ring = ring;
    c
}
pub(crate) fn compare(a: &[u8], b: &[u8], context: &str) {
    assert_eq!(a.len(), b.len(), "{context}: dimensions");
    let n = a.chunks(4).zip(b.chunks(4)).filter(|(a, b)| a != b).count();
    if n != 0 {
        let first = a.chunks(4).zip(b.chunks(4)).position(|(a, b)| a != b).unwrap();
        panic!("{context}: {n} unequal pixels, the first at ({}, {})", first % W, first / W);
    }
}
fn gate<V: View>(a: &mut App<V>, fresh: App<V>, context: &str) {
    let mut fresh = fresh;
    a.frame();
    let retained = a.reference_pixels();
    compare(&a.presented_pixels(), &retained, &format!("{context}: presented/full"));
    fresh.frame();
    compare(&retained, &fresh.reference_pixels(), &format!("{context}: retained/fresh"));
}
fn click<V: View>(a: &mut App<V>, x: f32, y: f32) {
    a.pointer_move(x, y);
    a.pointer_down(x, y, Button::Primary);
    a.pointer_up(x, y, Button::Primary);
}

// =============================================================================== defect 2: captures follow

/// A plain `Vec` of rows (rung 0): each row's routes capture the row's id.
struct Table {
    rows: Vec<(u32, u32)>,
    selected: Option<u32>,
    clicked: Vec<u32>,
    dragged: Vec<(u32, i32)>,
    hover: u32,
    next: u32,
    scroll: Signal<f32>,
}
impl Table {
    fn new() -> Self {
        Self { rows: (0..6).map(|i| (i, 0x203040 + i * 0x101010)).collect(), selected: None, clicked: Vec::new(), dragged: Vec::new(), hover: 0x505050, next: 100, scroll: Signal::new(0.0) }
    }
    fn snapshot(&self) -> Self {
        Self { rows: self.rows.clone(), selected: self.selected, clicked: Vec::new(), dragged: Vec::new(), hover: self.hover, next: self.next, scroll: Signal::new(*self.scroll) }
    }
    fn replace(&mut self) {
        for r in &mut self.rows {
            r.0 = self.next;
            self.next += 1;
        }
    }
}
const ROW: f32 = 20.0;
impl View for Table {
    fn view(&self) -> impl Element<Self> + '_ {
        column(each(self.rows.iter().map(|&(id, c)| {
            row((
                block(100.0, ROW).bg(rgb(if self.selected == Some(id) { 0xffcc00 } else { c })).on_click(move |t: &mut Table| {
                    t.selected = Some(id);
                    t.clicked.push(id);
                }),
                block(40.0, ROW).bg(rgb(0x994444)).on_click(move |t: &mut Table| t.rows.retain(|r| r.0 != id)),
                // (a press on an element with a drag route is a drag, never a click: its own element)
                block(30.0, ROW).bg(rgb(0x449944)).on_drag(move |t: &mut Table, e: &bough::input::Ev| t.dragged.push((id, e.dy as i32))),
                block(20.0, ROW).bg(rgb(c ^ 0x0f0f0f)).hover_bg(rgb(self.hover)),
            ))
        })))
        .w(220.0)
        .h(160.0)
        .scroll(sig!(self.scroll))
    }
}

/// Rows replaced, removed and reinserted: the route under the pointer is the one this description gave the
/// row now at that position, inside a scroll box (so the description reaches it only through defect 1's fix).
#[test]
fn route_captures_follow_the_description() {
    for ring in 1..=4 {
        let mut a = App::new(Table::new(), config(ring), true);
        a.frame();
        let sizes = a.rt.table_sizes();
        let y = |k: usize| k as f32 * ROW + 10.0;
        let check = |a: &mut App<Table>, what: &str| {
            let fresh = App::new(a.owner.snapshot(), config(ring), true);
            gate(a, fresh, &format!("ring {ring} {what}"));
        };
        click(&mut a, 50.0, y(2));
        assert_eq!(a.owner.clicked, [2]);
        check(&mut a, "click row 2");
        // every row replaced by a new id: the same position names the new row
        a.turn(|t| t.replace());
        click(&mut a, 50.0, y(2));
        assert_eq!(a.owner.clicked, [2, 102], "the route captured the id of the row it was mounted for");
        assert_eq!(a.owner.selected, Some(102));
        check(&mut a, "click after replace");
        // remove the first row with its own button: every row below shifts up one position
        click(&mut a, 120.0, y(0));
        assert_eq!(a.owner.rows.iter().map(|r| r.0).collect::<Vec<_>>(), [101, 102, 103, 104, 105]);
        check(&mut a, "remove row 0");
        click(&mut a, 50.0, y(0));
        assert_eq!(a.owner.clicked, [2, 102, 101]);
        // remove through the button at position 3 (id 104), then at position 0 (id 101)
        click(&mut a, 120.0, y(3));
        click(&mut a, 120.0, y(0));
        assert_eq!(a.owner.rows.iter().map(|r| r.0).collect::<Vec<_>>(), [102, 103, 105]);
        check(&mut a, "two more removals");
        // reinsert at the front: position 0 is the new row, position 1 the old first
        a.turn(|t| t.rows.insert(0, (7, 0x00ff88)));
        click(&mut a, 50.0, y(0));
        click(&mut a, 50.0, y(1));
        assert_eq!(a.owner.clicked[3..], [7, 102]);
        check(&mut a, "reinsert");
        // a drag whose rows are replaced under it: the element at that position is still mounted (positional
        // identity), so the drag goes on, with the captures of the row described there now
        a.pointer_down(155.0, y(2), Button::Primary);
        a.pointer_move(155.0, y(2) + 4.0);
        a.turn(|t| t.replace());
        a.pointer_move(155.0, y(2) + 9.0);
        a.pointer_up(155.0, y(2) + 9.0, Button::Primary);
        let ids: Vec<u32> = a.owner.dragged.iter().map(|d| d.0).collect();
        assert_eq!(ids, [103, 108, 108], "the drag route's id before and after the rows were replaced: {:?}", a.owner.dragged);
        check(&mut a, "drag across a replace");
        // a new hover colour reaches the hovered element at once (the fresh mount is not hovered: an expected pixel)
        a.pointer_move(180.0, y(1));
        a.turn(|t| t.hover = 0x00ffff);
        a.frame();
        let p = a.reference_pixels();
        let i = ((y(1) as usize) * W + 180) * 4;
        assert_eq!((p[i + 2], p[i + 1], p[i]), (0x00, 0xff, 0xff), "the new hover colour shows under the pointer");
        a.pointer_leave();
        check(&mut a, "hover colour, after leaving");
        // nothing was installed by any of this: the tables hold what a mount of the same rows holds
        // (wires, scrollers, views, clips, hooks; acts are not returned yet, see the lane report)
        a.turn(|t| {
            t.rows = Table::new().rows;
            t.selected = None;
        });
        let now = a.rt.table_sizes();
        assert_eq!([now[0], now[1], now[3], now[4], now[5], now[6]], [sizes[0], sizes[1], sizes[3], sizes[4], sizes[5], sizes[6]], "wires, scrollers, views, clips, hooks, tooltips");
    }
}

/// A drag whose element is removed ends: no route of it runs again, on the root or on anything that took
/// the element's place, and the release is not a click.
#[test]
fn drag_ends_with_its_element() {
    for ring in 1..=4 {
        let mut a = App::new(Table::new(), config(ring), true);
        a.frame();
        let y = |k: usize| k as f32 * ROW + 10.0;
        a.pointer_down(155.0, y(5), Button::Primary);
        a.pointer_move(155.0, y(5) + 3.0);
        assert_eq!(a.owner.dragged, [(5, 3)]);
        // the last row goes away under the pointer
        a.turn(|t| t.rows.truncate(3));
        a.pointer_move(155.0, y(5) + 6.0);
        a.pointer_up(155.0, y(5) + 6.0, Button::Primary);
        assert_eq!(a.owner.dragged, [(5, 3)], "no drag route after the element was removed");
        assert!(a.owner.clicked.is_empty());
        // rows come back at that position: they are other elements; the old capture is not theirs
        a.turn(|t| t.rows.extend([(40, 0x111111), (41, 0x222222), (42, 0x333333)]));
        a.pointer_move(155.0, y(5) + 8.0);
        assert_eq!(a.owner.dragged, [(5, 3)]);
        let fresh = App::new(a.owner.snapshot(), config(ring), true);
        gate(&mut a, fresh, &format!("ring {ring}"));
    }
}

/// An element removed while hovered: leaving it later writes nothing (its instance may be another element's by
/// then) and runs nothing.
#[test]
fn hover_of_a_removed_element_is_dropped() {
    for ring in 1..=4 {
        let mut a = App::new(Table::new(), config(ring), true);
        a.frame();
        let y = |k: usize| k as f32 * ROW + 10.0;
        // hover the last row, then remove it and mount other things (which reuse what it released)
        a.pointer_move(180.0, y(5));
        a.frame();
        a.turn(|t| t.rows.truncate(5));
        for _ in 0..4 {
            a.frame();
            a.turn(|_| {});
        }
        a.turn(|t| t.rows.insert(0, (77, 0x00aa00)));
        a.frame();
        a.pointer_leave();
        let fresh = App::new(a.owner.snapshot(), config(ring), true);
        gate(&mut a, fresh, &format!("ring {ring}"));
    }
}

// =============================================================================== defect 3: rows own their routes

/// A list row: a view of its own, with plain fields and a signal, whose handlers take `&mut Line`.
struct Line {
    id: u32,
    c: u32,
    n: Signal<u32>,
    open: bool,
    drags: u32,
}
impl Line {
    fn new(id: u32) -> Line {
        Line { id, c: 0x304050 + id * 0x0b0704, n: Signal::new(0), open: false, drags: 0 }
    }
    fn copy(&self) -> Line {
        Line { id: self.id, c: self.c, n: Signal::new(*self.n), open: self.open, drags: self.drags }
    }
}
impl View for Line {
    fn view(&self) -> impl Element<Self> + '_ {
        let id = self.id;
        column((
            row((
                // the row's own handler: a plain field and a signal of the row
                block(60.0, ROW).bg(rgb(self.c)).on_click(|l: &mut Line| {
                    l.c ^= 0x00ffff;
                    *l.n += 1;
                }),
                // the root's handler from inside the row (an enclosing owner), capturing the row's id
                block(40.0, ROW).bg(rgb(0x777700)).on_click(move |s: &mut Sheet| s.picked.push(id)),
                // a dynamic part of the row
                block(30.0, ROW).bg(rgb(0x007777)).on_click(|l: &mut Line| l.open = !l.open),
                // a drag on the row
                block(30.0, ROW).bg(rgb(0x770077)).on_drag(|l: &mut Line, _| l.drags += 1),
                block(8.0 + 6.0 * (*self.n % 5) as f32, 6.0).bg(rgb(0xffffff)),
                text(sig!(self.n).map(|n| format!("{n}"))),
            )),
            self.open.then(|| block(150.0, 8.0).bg(rgb(self.c ^ 0xffffff))),
        ))
    }
}
struct Sheet {
    lines: List<Line>,
    picked: Vec<u32>,
    title: u32,
}
impl Sheet {
    fn new() -> Sheet {
        Sheet { lines: List::from_iter_in((0..7).map(Line::new)), picked: Vec::new(), title: 0x224466 }
    }
    fn snapshot(&self) -> Sheet {
        Sheet { lines: List::from_iter_in(self.lines.iter().map(Line::copy)), picked: self.picked.clone(), title: self.title }
    }
}
const HEAD: f32 = 12.0;
impl View for Sheet {
    fn view(&self) -> impl Element<Self> + '_ {
        column((block(200.0, HEAD).bg(rgb(self.title + self.picked.len() as u32 * 0x110000)), list(sig!(self.lines)).w(220.0)))
    }
}

/// The y of the middle of row `k`'s first line, given which rows above it are open.
fn line_y(s: &Sheet, k: usize) -> f32 {
    HEAD + s.lines.iter().take(k).map(|l| ROW + if l.open { 8.0 } else { 0.0 }).sum::<f32>() + ROW / 2.0
}

#[test]
fn a_row_handler_runs_on_its_row() {
    for ring in 1..=4 {
        let mut a = App::new(Sheet::new(), config(ring), true);
        a.frame();
        let check = |a: &mut App<Sheet>, what: &str| {
            let fresh = App::new(a.owner.snapshot(), config(ring), true);
            gate(a, fresh, &format!("ring {ring} {what}"));
        };
        // the row's handler: its plain colour and its signal change, nothing else does
        let y3 = line_y(&a.owner, 3);
        click(&mut a, 30.0, y3);
        let state = |a: &App<Sheet>| a.owner.lines.iter().map(|l| (l.id, l.c, *l.n, l.open)).collect::<Vec<_>>();
        let mut want: Vec<(u32, u32, u32, bool)> = (0..7).map(|i| (i, 0x304050 + i * 0x0b0704, 0, false)).collect();
        want[3].1 ^= 0x00ffff;
        want[3].2 = 1;
        assert_eq!(state(&a), want, "row 3's handler ran on row 3");
        check(&mut a, "row click");
        // again and on another row
        click(&mut a, 30.0, y3);
        let y0 = line_y(&a.owner, 0);
        click(&mut a, 30.0, y0);
        want[3].1 ^= 0x00ffff;
        want[3].2 = 2;
        want[0].1 ^= 0x00ffff;
        want[0].2 = 1;
        assert_eq!(state(&a), want);
        check(&mut a, "two more row clicks");
        // the root's handler from inside a row
        let y5 = line_y(&a.owner, 5);
        click(&mut a, 80.0, y5);
        assert_eq!(a.owner.picked, [5]);
        assert_eq!(state(&a), want, "a root handler leaves the rows alone");
        check(&mut a, "root handler from a row");
        // a dynamic part of the row: the row grows, the rows below move
        let y1 = line_y(&a.owner, 1);
        click(&mut a, 115.0, y1);
        want[1].3 = true;
        assert_eq!(state(&a), want);
        check(&mut a, "row opens");
        let y4 = line_y(&a.owner, 4);
        click(&mut a, 115.0, y4);
        let y2 = line_y(&a.owner, 2);
        click(&mut a, 30.0, y2);
        want[4].3 = true;
        want[2].1 ^= 0x00ffff;
        want[2].2 = 1;
        assert_eq!(state(&a), want);
        check(&mut a, "another opens; one below the first open row is clicked");
        let y1 = line_y(&a.owner, 1);
        click(&mut a, 115.0, y1);
        want[1].3 = false;
        assert_eq!(state(&a), want);
        check(&mut a, "row closes");
        // `List::edit` from a root handler: the row's plain fields re-describe too
        a.turn(|s| {
            let l = s.lines.edit(6);
            l.c = 0xff0000;
            l.open = true;
        });
        check(&mut a, "List::edit of plain fields");
        // list operations around handled rows
        a.turn(|s| s.lines.swap(0, 3));
        check(&mut a, "swap");
        let y0 = line_y(&a.owner, 0);
        click(&mut a, 30.0, y0);
        assert_eq!(a.owner.lines.get(0).map(|l| (l.id, *l.n)), Some((3, 3)), "the row that moved to position 0 is row 3");
        check(&mut a, "click after swap");
    }
}

/// Removal, reinsertion and slot reuse: a route never reaches a removed row, nor the row that took its slot.
#[test]
fn a_removed_row_is_never_dispatched_to() {
    for ring in 1..=4 {
        let mut a = App::new(Sheet::new(), config(ring), true);
        a.frame();
        let check = |a: &mut App<Sheet>, what: &str| {
            let fresh = App::new(a.owner.snapshot(), config(ring), true);
            gate(a, fresh, &format!("ring {ring} {what}"));
        };
        // press the drag handle of row 2 and drag
        let y2 = line_y(&a.owner, 2);
        a.pointer_down(145.0, y2, Button::Primary);
        a.pointer_move(145.0, y2 + 2.0);
        assert_eq!(a.owner.lines.get(2).unwrap().drags, 1);
        // the row is removed while the drag is held ...
        let slot = a.owner.lines.slot(2).unwrap();
        a.turn(|s| {
            s.lines.remove(2);
        });
        a.pointer_move(145.0, y2 + 4.0);
        check(&mut a, "removed while dragged");
        // ... and in a later turn another row takes its slot, at the same place on screen
        a.turn(|s| {
            s.lines.insert(2, Line::new(50));
        });
        assert_eq!(a.owner.lines.slot(2).unwrap().index(), slot.index(), "the slot is reused");
        assert!(a.owner.lines.get_slot(slot).is_none(), "the old slot is dead");
        a.pointer_move(145.0, y2 + 6.0);
        a.pointer_up(145.0, y2 + 6.0, Button::Primary);
        assert_eq!(a.owner.lines.iter().map(|l| (l.id, l.drags)).collect::<Vec<_>>(), [(0, 0), (1, 0), (50, 0), (3, 0), (4, 0), (5, 0), (6, 0)], "the removed row's drag reached no other row");
        check(&mut a, "slot reused");
        // the new row's own routes work
        click(&mut a, 30.0, y2);
        a.pointer_down(145.0, y2, Button::Primary);
        a.pointer_move(145.0, y2 + 1.0);
        a.pointer_up(145.0, y2 + 1.0, Button::Primary);
        assert_eq!(a.owner.lines.get(2).map(|l| (l.id, *l.n, l.drags)), Some((50, 1, 2)), "a move and the release of the new row's own drag");
        check(&mut a, "new row handled");
        // remove and reinsert many times: every click lands on the row shown there
        for i in 0..40u32 {
            a.turn(|s| {
                s.lines.remove((i % 5) as usize);
            });
            a.turn(|s| {
                s.lines.insert((i % 3) as usize, Line::new(200 + i));
            });
            let k = (i % 7) as usize;
            let y = line_y(&a.owner, k);
            let mut want: Vec<(u32, u32)> = a.owner.lines.iter().map(|l| (l.id, *l.n)).collect();
            want[k].1 += 1;
            click(&mut a, 30.0, y);
            assert_eq!(a.owner.lines.iter().map(|l| (l.id, *l.n)).collect::<Vec<_>>(), want, "cycle {i}: the handler of the row at position {k} ran, and no other");
        }
        check(&mut a, "after 40 cycles");
    }
}

// =============================================================================== nested owners

/// A list of groups, each with a list of cells: three owner levels. A cell's element can address the cell, its
/// group or the root.
struct Cell {
    v: u32,
}
impl View for Cell {
    fn view(&self) -> impl Element<Self> + '_ {
        row((
            block(24.0, 14.0).bg(rgb(0x202020 + self.v * 0x30)).on_click(|c: &mut Cell| c.v += 1),
            block(24.0, 14.0).bg(rgb(0x805000)).on_click(|g: &mut Group| g.hits += 1),
            block(24.0, 14.0).bg(rgb(0x005080)).on_click(|b: &mut Board| b.hits += 1),
        ))
    }
}
struct Group {
    cells: List<Cell>,
    hits: u32,
}
impl View for Group {
    fn view(&self) -> impl Element<Self> + '_ {
        column((block(100.0 + 10.0 * self.hits as f32, 6.0).bg(rgb(0xaa0000)), list(sig!(self.cells)).w(200.0)))
    }
}
struct Board {
    groups: List<Group>,
    hits: u32,
}
impl Board {
    fn new() -> Board {
        Board { groups: List::from_iter_in((0..3).map(|g| Group { cells: List::from_iter_in((0..3).map(|c| Cell { v: g * 3 + c })), hits: 0 })), hits: 0 }
    }
    fn snapshot(&self) -> Board {
        Board { groups: List::from_iter_in(self.groups.iter().map(|g| Group { cells: List::from_iter_in(g.cells.iter().map(|c| Cell { v: c.v })), hits: g.hits })), hits: self.hits }
    }
    fn state(&self) -> (u32, Vec<(u32, Vec<u32>)>) {
        (self.hits, self.groups.iter().map(|g| (g.hits, g.cells.iter().map(|c| c.v).collect())).collect())
    }
}
impl View for Board {
    fn view(&self) -> impl Element<Self> + '_ {
        column((block(60.0 + 10.0 * self.hits as f32, 8.0).bg(rgb(0x00aa00)), list(sig!(self.groups)).w(220.0)))
    }
}
/// Cell `c` of group `g`: groups are 6 + 3 x 14 tall, below the 8 pt head.
fn cell_y(g: usize, c: usize) -> f32 {
    8.0 + g as f32 * 48.0 + 6.0 + c as f32 * 14.0 + 7.0
}

#[test]
fn nested_owners_each_get_their_handler() {
    for ring in 1..=4 {
        let mut a = App::new(Board::new(), config(ring), true);
        a.frame();
        let check = |a: &mut App<Board>, what: &str| {
            let fresh = App::new(a.owner.snapshot(), config(ring), true);
            gate(a, fresh, &format!("ring {ring} {what}"));
        };
        let mut want = Board::new().state();
        // the cell itself (two list hops from the root)
        click(&mut a, 12.0, cell_y(1, 2));
        want.1[1].1[2] += 1;
        assert_eq!(a.owner.state(), want);
        check(&mut a, "cell handler");
        // the cell's group (the nearest enclosing owner of that type), from a cell of group 2
        click(&mut a, 36.0, cell_y(2, 0));
        want.1[2].0 += 1;
        assert_eq!(a.owner.state(), want);
        check(&mut a, "group handler from a cell");
        // the root, from a cell
        click(&mut a, 60.0, cell_y(0, 1));
        want.0 += 1;
        assert_eq!(a.owner.state(), want);
        check(&mut a, "root handler from a cell");
        // a group is removed: the groups below move up, and their cells' routes follow their owners
        a.turn(|b| {
            b.groups.remove(0);
        });
        want.1.remove(0);
        click(&mut a, 12.0, cell_y(0, 2));
        want.1[0].1[2] += 1;
        click(&mut a, 36.0, cell_y(1, 1));
        want.1[1].0 += 1;
        assert_eq!(a.owner.state(), want);
        check(&mut a, "after a group was removed");
        // a cell is removed from a group through the root, another inserted; the group's other cells answer
        a.turn(|b| {
            let g = b.groups.edit(1);
            g.cells.remove(0);
            g.cells.push(Cell { v: 40 });
        });
        want.1[1].1.remove(0);
        want.1[1].1.push(40);
        click(&mut a, 12.0, cell_y(1, 2));
        want.1[1].1[2] += 1;
        assert_eq!(a.owner.state(), want);
        check(&mut a, "after cells changed");
    }
}

/// A handler whose receiver type is not on the element's owner chain is a mount error that names both types.
#[test]
#[should_panic(expected = "no owner of that type encloses the element")]
fn a_receiver_off_the_owner_chain_is_a_mount_error() {
    struct Other;
    struct Lone;
    impl View for Lone {
        fn view(&self) -> impl Element<Self> + '_ {
            block(10.0, 10.0).bg(rgb(0xffffff)).on_click(|_: &mut Other| {})
        }
    }
    let _ = App::new(Lone, config(1), false);
}

// =============================================================================== entity views

mod entities {
    use super::{click, config, gate};
    use bough::app::App;
    use bough::layout::prelude::*;
    use bough::raw::*;

    pub struct Doc {
        pub c: u32,
        pub n: Signal<u32>,
    }
    impl View for Doc {
        fn view(&self) -> impl Element<Self> + '_ {
            row((
                block(40.0, 20.0).bg(rgb(self.c)).on_click(|d: &mut Doc| {
                    d.c = (d.c << 8 | d.c >> 16) & 0xffffff;
                    *d.n += 1;
                }),
                block(10.0 + 5.0 * (*self.n % 6) as f32, 20.0).bg(rgb(0xdddddd)),
            ))
        }
    }
    pub struct Desk {
        pub docs: Entities<Doc>,
        pub shown: Vec<Entity<Doc>>,
    }
    impl View for Desk {
        fn view(&self) -> impl Element<Self> + '_ {
            // each entity is shown twice: two views of one owner
            column((column(each(self.shown.iter().map(|&e| entity_view(sig!(self.docs), e)))), block(200.0, 4.0).bg(rgb(0x444444)), column(each(self.shown.iter().map(|&e| entity_view(sig!(self.docs), e))))))
        }
    }
    fn desk(state: &[(u32, u32)]) -> Desk {
        let mut docs = Entities::with_capacity(8);
        let shown = state.iter().map(|&(c, n)| docs.insert(Doc { c, n: Signal::new(n) })).collect();
        Desk { docs, shown }
    }
    fn state(d: &Desk) -> Vec<(u32, u32)> {
        d.shown.iter().filter_map(|&e| d.docs.get(e)).map(|d| (d.c, *d.n)).collect()
    }

    /// A route in an entity's view runs on the entity; every view of it shows the entity's plain fields again.
    #[test]
    fn an_entity_handler_runs_on_its_entity() {
        for ring in 1..=4 {
            let mut a = App::new(desk(&[(0x112233, 0), (0x445566, 0), (0x778899, 0)]), config(ring), true);
            a.frame();
            let check = |a: &mut App<Desk>, what: &str| {
                let fresh = App::new(desk(&state(&a.owner)), config(ring), true);
                gate(a, fresh, &format!("ring {ring} {what}"));
            };
            // the first view of entity 1
            click(&mut a, 20.0, 30.0);
            assert_eq!(state(&a.owner), [(0x112233, 0), (0x556644, 1), (0x778899, 0)]);
            check(&mut a, "first view clicked");
            // the second view of entity 2 (below the first three and the 4 pt rule)
            click(&mut a, 20.0, 64.0 + 40.0 + 10.0);
            assert_eq!(state(&a.owner), [(0x112233, 0), (0x556644, 1), (0x889977, 1)]);
            check(&mut a, "second view clicked");
            // an entity removed (its views dropped from the description in the same turn), another shown
            a.turn(|d| {
                let e = d.shown.remove(0);
                d.docs.remove(e);
            });
            click(&mut a, 20.0, 10.0);
            assert_eq!(state(&a.owner), [(0x664455, 2), (0x889977, 1)]);
            check(&mut a, "after a removal");
            a.turn(|d| {
                let e = d.docs.insert(Doc { c: 0x00ff00, n: Signal::new(4) });
                d.shown.insert(0, e);
            });
            click(&mut a, 20.0, 10.0);
            assert_eq!(state(&a.owner), [(0xff0000, 5), (0x664455, 2), (0x889977, 1)]);
            check(&mut a, "after an insertion");
        }
    }

    /// An entity removed while its view stays described keeps its last pixels; a click on them reaches no one
    /// (the handle is dead), and a later entity in that slot is not it.
    #[test]
    fn a_removed_entity_is_never_dispatched_to() {
        let mut a = App::new(desk(&[(0x112233, 0), (0x445566, 0)]), config(2), true);
        a.frame();
        let e0 = a.owner.shown[0];
        a.turn(|d| {
            d.docs.remove(e0);
        });
        a.frame();
        click(&mut a, 20.0, 10.0);
        a.frame();
        // a new entity takes the slot (a later turn); the stale views still name the dead handle
        let e2 = {
            let mut e = None;
            a.turn(|d| e = Some(d.docs.insert(Doc { c: 0xabcdef, n: Signal::new(9) })));
            e.unwrap()
        };
        click(&mut a, 20.0, 10.0);
        assert_eq!(a.owner.docs.get(e2).map(|d| (d.c, *d.n)), Some((0xabcdef, 9)), "the stale view's route reached no other entity");
        assert_eq!(a.owner.docs.get(a.owner.shown[1]).map(|d| (d.c, *d.n)), Some((0x445566, 0)));
        a.frame();
        let retained = a.reference_pixels();
        super::compare(&a.presented_pixels(), &retained, "presented/full");
    }
}

// =============================================================================== what a turn re-describes

mod grain {
    use super::{click, compare, config, ROW, W};
    use bough::app::App;
    use bough::prelude::*;
    use std::cell::Cell;

    struct Item {
        c: u32,
        views: Cell<u32>,
    }
    impl View for Item {
        fn view(&self) -> impl Element<Self> + '_ {
            self.views.set(self.views.get() + 1);
            row((block(60.0, ROW).bg(rgb(self.c)).on_click(|i: &mut Item| i.c ^= 0xffffff), block(60.0, ROW).bg(rgb(0x333333)).hover_bg(rgb(0x999999))))
        }
    }
    struct Page {
        items: List<Item>,
        plain: Vec<u32>,
        y: Signal<f32>,
        hits: u32,
        views: Cell<u32>,
    }
    impl Page {
        fn new(y: f32) -> Page {
            Page { items: List::from_iter_in((0..4).map(|i| Item { c: 0x102030 * (i + 1), views: Cell::new(0) })), plain: (0..12).map(|i| 0x201000 + i * 0x0a0a0a).collect(), y: Signal::new(y), hits: 0, views: Cell::new(0) }
        }
    }
    impl View for Page {
        fn view(&self) -> impl Element<Self> + '_ {
            self.views.set(self.views.get() + 1);
            column((
                list(sig!(self.items)).w(200.0),
                block(100.0 + self.hits as f32, ROW).bg(rgb(0x004400)).on_click(|p: &mut Page| p.hits += 1),
                each(self.plain.iter().map(|&c| row((block(80.0, ROW).bg(rgb(c)), block(80.0, ROW).bg(rgb(c ^ 0x00ff00)).hover_bg(rgb(0xffffff)))))),
            ))
            .w(220.0)
            .h(160.0)
            .scroll(sig!(self.y))
        }
    }

    /// Only the owner a handler ran on describes itself again: a hover colour, a wheel and a glide re-describe
    /// nothing; a row's handler re-describes that row; a root handler the root (and no row).
    #[test]
    fn a_turn_redescribes_the_owners_it_handled() {
        let mut a = App::new(Page::new(0.0), config(2), true);
        a.frame();
        let counts = |a: &App<Page>| (a.owner.views.get(), a.owner.items.iter().map(|i| i.views.get()).collect::<Vec<_>>());
        let base = counts(&a);
        // hover crossings of two hover colours (a row's and a plain item's), a wheel, a glide to its end
        a.pointer_move(90.0, 10.0);
        a.pointer_move(120.0, 5.0 * ROW + 10.0);
        a.pointer_move(120.0, 6.0 * ROW + 10.0);
        a.wheel_points(120.0, 6.0 * ROW + 10.0, 7.0);
        a.wheel(120.0, 6.0 * ROW + 10.0, 0.0, 1.0);
        while a.animating() {
            a.tick(1.0 / 60.0);
            a.frame();
        }
        a.wheel_points(120.0, 100.0, -1000.0);
        a.pointer_leave();
        assert_eq!(counts(&a), base, "hover colours and scrolling re-describe nothing");
        // a row's handler: that row
        click(&mut a, 30.0, 2.0 * ROW + 10.0);
        assert_eq!(counts(&a), (base.0, vec![base.1[0], base.1[1], base.1[2] + 1, base.1[3]]), "a row handler re-describes its row");
        // the root's handler: the root
        click(&mut a, 50.0, 4.0 * ROW + 10.0);
        assert_eq!(a.owner.hits, 1);
        assert_eq!(counts(&a), (base.0 + 1, vec![base.1[0], base.1[1], base.1[2] + 1, base.1[3]]), "a root handler re-describes the root");
        a.frame();
        let retained = a.reference_pixels();
        compare(&a.presented_pixels(), &retained, "presented/full");
    }

    /// Hover follows the tree under a resting pointer: after a scroll (by the wheel, by a glide, by a handler)
    /// the element now under the pointer is the hovered one. The fresh mount is given the same pointer.
    #[test]
    fn hover_follows_a_scroll() {
        for ring in 1..=4 {
            let mut a = App::new(Page::new(0.0), config(ring), true);
            a.frame();
            let (x, y) = (120.0, 5.0 * ROW + 10.0);
            let check = |a: &mut App<Page>, what: &str| {
                a.frame();
                let retained = a.reference_pixels();
                compare(&a.presented_pixels(), &retained, &format!("ring {ring} {what}: presented/full"));
                let mut fresh = App::new(Page::new(*a.owner.y), config(ring), true);
                fresh.pointer_move(x, y);
                fresh.frame();
                compare(&retained, &fresh.reference_pixels(), &format!("ring {ring} {what}: retained/fresh with the pointer there"));
                // the hovered element shows its hover colour under the pointer
                let i = (y as usize * W + x as usize) * 4;
                assert_eq!((retained[i + 2], retained[i + 1], retained[i]), (0xff, 0xff, 0xff), "ring {ring} {what}: the hover colour is under the pointer");
            };
            a.pointer_move(x, y);
            check(&mut a, "hovered");
            a.wheel_points(x, y, 20.0);
            check(&mut a, "precise scroll by one row");
            a.wheel_points(x, y, 13.0);
            check(&mut a, "precise scroll by part of a row");
            a.wheel(x, y, 0.0, 1.0);
            while a.animating() {
                a.tick(1.0 / 60.0);
                a.frame();
            }
            check(&mut a, "after a glide");
            a.turn(|p| {
                p.y.set(3.0);
            });
            check(&mut a, "scrolled by a handler");
        }
    }
}
