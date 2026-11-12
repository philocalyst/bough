//! The accessibility projection, held to itself and to the world: roles, names and states read off what the
//! view already says; 100,000 rows that are not materialised; deltas that, applied, equal a fresh projection
//! after every step of a random script; nothing done for a theme, a hover or a motion; settled targets.
#[path = "../../bough-x/scenes/sheet.rs"]
mod sheet;
use bough_ax::{Ax, Live, Node, NodeId, Role, Tag, Tree};
use bough_x::event::{Interact, NONE};
use bough_x::expr::{Code, Cx as XCx, Expr, Frame, Lens, Rec, U_FOCUS};
use bough_x::prelude::*;
use bough_x::soft::Soft;
use core::marker::PhantomData;
use sheet::*;

fn mounted(rows: usize) -> (Sheet, World<Soft>) {
    let s = Sheet::new(rows);
    let mut w = World::<Soft>::new(W, H, NWORDS);
    w.mount(&s);
    (s, w)
}
fn cents(v: f32) -> i32 {
    (v as f64 * 100.0).round() as i32
}
fn num(v: f32) -> String {
    bough_x::reflect::Val::Num(cents(v), 2).text()
}

#[test]
fn the_sheet_is_a_grid_of_100k_rows_with_its_headers() {
    let (s, w) = mounted(100_000);
    let mut ax = Ax::new();
    let up = ax.update(&w, &s);
    let t = ax.tree();
    assert!(up.changed.len() == t.nodes.len() && up.root.is_some(), "the first update is the whole tree");
    let grid = t.one(|n| n.role == Role::Grid);
    assert_eq!(grid.row_count, Some(100_000));
    assert_eq!(grid.col_count, Some(1 + COLS as u32), "the row number and ten cells");
    // published: the exposed window and a page on each side, never the 100,000
    assert!(grid.children.len() < 3 * 36, "{} rows published", grid.children.len());
    assert!(t.nodes.len() < 2_000, "{} nodes", t.nodes.len());
    // the cells of row 3, column C, by what they say
    let row3 = t.get(grid.children[3]).unwrap();
    assert_eq!((row3.role, row3.row_index), (Role::Row, Some(3)));
    let c = t.get(row3.children[3]).unwrap();
    assert_eq!((c.role, c.col_index), (Role::Cell, Some(3)));
    assert_eq!(c.name, num(value(3, 2)));
    // column headers: the header labels line up with the grid's columns
    let hc = t.one(|n| n.role == Role::ColumnHeader && n.name == "C");
    assert_eq!(hc.col_index, Some(3));
    assert_eq!(t.all(|n| n.role == Role::ColumnHeader).len(), COLS);
    // the totals read application words: a polite live region each
    let totals = t.all(|n| n.role == Role::StaticText && n.live == Live::Polite);
    assert_eq!(totals.len(), COLS);
    // the selection outline rests on cell (0, A): that cell is selected
    let sel = t.one(|n| n.selected == Some(true));
    assert_eq!((sel.id.row, sel.col_index), (0, Some(1)));
    // the body scrolls: a scroll view around the grid
    let sv = t.one(|n| n.role == Role::ScrollView);
    assert!(sv.children.contains(&grid.id));
    assert_eq!(sv.scroll.unwrap().1, 100_000 * CELL_H);
    // provenance: every node made from a record knows its line
    assert!(t.all(|n| n.id.tag == Tag::Rec).iter().all(|n| n.loc.is_some_and(|l| l.file().ends_with("sheet.rs"))));
}

#[test]
fn any_row_is_described_on_demand() {
    let (s, w) = mounted(100_000);
    let mut ax = Ax::new();
    ax.update(&w, &s);
    let grid = ax.tree().one(|n| n.role == Role::Grid).id;
    let before = w.root.stats();
    let nodes = ax.rows(&w, &s, grid, 77_777..77_778);
    assert_eq!(w.root.stats(), before, "nothing materialised");
    let row = nodes.iter().find(|n| n.role == Role::Row).unwrap();
    assert_eq!(row.row_index, Some(77_777));
    let cells: Vec<&Node> = nodes.iter().filter(|n| n.role == Role::Cell).collect();
    assert_eq!(cells.len(), 1 + COLS);
    assert_eq!(cells[0].name, "77778", "the row number");
    assert_eq!(cells[1 + 4].name, num(value(77_777, 4)));
    assert!(cells.iter().all(|c| c.place.is_none()), "not materialised: no place");
}

#[test]
fn a_theme_a_hover_and_a_motion_cost_nothing() {
    let (mut s, mut w) = mounted(10_000);
    let mut ax = Ax::new();
    ax.update(&w, &s);
    s.toggle_theme();
    w.settle(&s);
    assert!(ax.update(&w, &s).is_empty());
    assert_eq!(ax.projected, 0, "a colour is not what a screen reader is told");
    w.pointer(&mut s, GUTTER + 3 * CELL_W + 20, TOOLBAR + 10);
    w.settle(&s);
    assert!(ax.update(&w, &s).is_empty());
    assert_eq!(ax.projected, 0);
    // a selection that glides: one update, at the turn; the frames of the glide cost nothing
    s.select(13, w.now());
    w.settle(&s);
    let up = ax.update(&w, &s);
    assert!(!up.is_empty());
    for t in 1..10 {
        w.tick(t * 16);
        assert!(ax.update(&w, &s).is_empty());
        assert_eq!(ax.projected, 0);
    }
}

#[test]
fn in_motion_the_tree_reports_where_things_rest() {
    let (mut s, mut w) = mounted(1_000);
    let mut ax = Ax::new();
    w.tick(1_000);
    s.select(2 * COLS as u32 + 5, w.now()); // row 2, cell F
    w.settle(&s);
    w.tick(1_016); // one frame into a 180 ms glide: the pixels show the outline near cell (0, A)
    ax.update(&w, &s);
    let sel = ax.tree().one(|n| n.selected == Some(true));
    assert_eq!((sel.id.row, sel.col_index), (2, Some(6)), "the cell the outline is going to");
}

#[test]
fn an_edit_is_a_small_delta() {
    let (mut s, mut w) = mounted(10_000);
    let mut ax = Ax::new();
    ax.update(&w, &s);
    s.edit(4, 7, 1234.5);
    w.settle(&s);
    let up = ax.update(&w, &s);
    let names: Vec<&str> = up.changed.iter().map(|n| n.name.as_str()).collect();
    assert!(up.removed.is_empty());
    assert!(names.contains(&"1234.50"), "{names:?}");
    // the cell and its column's total; the grid's own node does not change
    assert!(up.changed.len() <= 3, "{} nodes changed: {names:?}", up.changed.len());
}

/// After every step of a random script, the tree kept by applying deltas equals the tree the projection holds,
/// and both equal a fresh projection of the same world.
fn script(seed: u64, steps: usize) {
    let (mut s, mut w) = mounted(10_000);
    let mut ax = Ax::new();
    let mut mirror = Tree::default();
    mirror.apply(&ax.update(&w, &s));
    let mut rng = seed | 1;
    let mut rnd = |n: u64| {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        rng % n
    };
    let mut t = 0u32;
    for step in 0..steps {
        t += 16 + rnd(300) as u32;
        w.tick(t);
        let n = s.rows.len();
        let top = (s.u.get::<Scroll>() / CELL_H) as usize;
        match rnd(8) {
            0 => s.toggle_theme(),
            1 => s.edit((top + rnd(30) as usize).min(n - 1), rnd(COLS as u64) as usize, rnd(100_000) as f32 / 100.0),
            2 => s.select(((top + rnd(30) as usize).min(n - 1) * COLS + rnd(COLS as u64) as usize) as u32, w.now()),
            3 => s.scroll_by(rnd(4_000) as i32 - 1_500),
            4 => s.insert_row((top + rnd(30) as usize).min(n), [1.0; COLS], false),
            5 => s.remove_row((top + rnd(30) as usize).min(n - 1), false),
            6 => {
                w.pointer(&mut s, GUTTER + rnd(COLS as u64) as i32 * CELL_W + 9, TOOLBAR + 10);
            }
            _ => s.edit(rnd(n as u64) as usize, 0, 7.0),
        }
        w.settle(&s);
        let up = ax.update(&w, &s);
        mirror.apply(&up);
        assert!(mirror == *ax.tree(), "step {step}: the tree kept by deltas differs from the projection's");
        let mut fresh = Ax::new();
        fresh.update(&w, &s);
        assert!(*fresh.tree() == mirror, "step {step}: deltas drifted from a fresh projection\n{}", diff(fresh.tree(), &mirror));
    }
}
fn diff(a: &Tree, b: &Tree) -> String {
    let mut s = String::new();
    for (id, n) in &a.nodes {
        if b.nodes.get(id) != Some(n) {
            s += &format!("fresh {n:?}\nkept  {:?}\n", b.nodes.get(id));
        }
    }
    for id in b.nodes.keys() {
        if !a.nodes.contains_key(id) {
            s += &format!("kept only {id:?}\n");
        }
    }
    s
}
#[test]
fn deltas_equal_a_fresh_projection() {
    script(0x5eed, 200);
}

// ---- an inbox: buttons, a text input, a list of items whose selection is an expression ------------------------

lens!(pub Sel: u32 = 1);
lens!(pub Scroll2: i32 = 3);
/// "word `L` equals this record's constant": the shape a selection takes when it is written as an expression
/// (the raster's `x_eq` leaf), which is what the projection reads a selected state from.
#[repr(C)]
struct Is<L>(Rec<u32>, PhantomData<L>);
impl<L> Clone for Is<L> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<L> Copy for Is<L> {}
// SAFETY: one word.
unsafe impl<L: 'static> bough_x::expr::Words for Is<L> {}
impl<L: Lens<T = u32>> Expr for Is<L> {
    type Out = bool;
    type Stage = Frame;
    const READS: u64 = 1 << L::SLOT;
    const CODE: &'static Code = &Code::Call("x_eq", &[&Code::U(L::SLOT), &Code::Rec]);
    fn eval(&self, cx: &XCx) -> bool {
        cx.u.w[L::SLOT] == self.0.0
    }
}
/// `if c { A } else { B }` (the raster's `x_sel`).
#[repr(C)]
#[derive(Clone, Copy)]
struct Choose<C, const A: u32, const B: u32>(C);
// SAFETY: transparent over a `Words` operand.
unsafe impl<C: bough_x::expr::Words, const A: u32, const B: u32> bough_x::expr::Words for Choose<C, A, B> {}
impl<C: Expr<Out = bool>, const A: u32, const B: u32> Expr for Choose<C, A, B> {
    type Out = Color;
    type Stage = Frame;
    const READS: u64 = C::READS;
    const CODE: &'static Code = &Code::Call("x_sel", &[C::CODE, &Code::Lit(A), &Code::Lit(B)]);
    fn eval(&self, cx: &XCx) -> Color {
        Color(if self.0.eval(cx) { A } else { B })
    }
}

struct Inbox {
    u: U,
    items: Rel<Txt>,
    draft: Rel<Txt>,
    sent: u32,
}
const IW: i32 = 800;
const IH: i32 = 600;
const ITOP: i32 = 100;
const ROW: i32 = 40;
const IROW: [Rect; 2] = [Rect::new(0, 0, IW, ROW), Rect::new(10, 5, 200, 30)];
const IFIELD: [Rect; 2] = [Rect::new(0, 0, 300, 40), Rect::new(0, 0, 300, 40)];
impl Inbox {
    fn new(n: usize) -> Inbox {
        let mut u = U::default();
        u.set::<Sel>(u32::MAX);
        Inbox { u, items: Rel::new((0..n).map(|i| Txt::new(&format!("msg {i}"))).collect()), draft: Rel::new(vec![Txt::new("hello")]), sent: 0 }
    }
    fn text(&self) -> String {
        let (b, n) = self.draft.get(0).bytes();
        String::from_utf8_lossy(&b[..n]).into_owned()
    }
}
impl View for Inbox {
    fn block(&self) -> &U {
        &self.u
    }
    fn block_mut(&mut self) -> Option<&mut U> {
        Some(&mut self.u)
    }
    fn view<S: Sink<Self>>(&self, s: &mut S) {
        s.row(|| {
            (
                [Rect::new(20, 20, 120, 40), Rect::new(20, 20, 120, 40)],
                (Fill(Kc::<0xff2f6bff>).on_click(|s: &mut Inbox| s.sent += 1).focusable(), Label(Txt::new("Send"), Kc::<0xffffffff>)),
            )
        });
        s.list(&List { n: 1, row_h: 40, x: 400, y: 20, shape: &IFIELD, edits: &self.draft.journal }, |_| {
            (Fill(Kc::<0xfffafafa>).on_text(|s: &mut Inbox, t: &str| {
                let x = s.text() + t;
                *s.draft.edit(0) = Txt::new(&x[..x.len().min(12)]);
            }), Label(*self.draft.get(0), Kc::<0xff000000>))
        });
        s.scope(Space { clip: Rect { x0: 0, y0: ITOP, x1: IW, y1: IH }, base: (0, ITOP), scroll: Scroll2::SLOT as u8 }, |s| {
            s.list(&List { n: self.items.len() as u32, row_h: ROW, x: 0, y: 0, shape: &IROW, edits: &self.items.journal }, |r| {
                (
                    Fill(Choose::<_, 0xffdbe8ff, 0xfff6f7f9>(Is::<Sel>(rec(r as u32), PhantomData))).on_click(move |s: &mut Inbox| s.u.set::<Sel>(r as u32)).focusable(),
                    Label(*self.items.get(r), Kc::<0xff1d2129>),
                )
            });
        });
    }
}

#[test]
fn roles_names_and_states_from_what_is_there() {
    let mut v = Inbox::new(500);
    let mut w = World::<Soft>::new(IW, IH, 4);
    w.mount(&v);
    let mut ax = Ax::new();
    ax.update(&w, &v);
    let t = ax.tree();
    let send = t.one(|n| n.role == Role::Button);
    assert_eq!(send.name, "Send", "a button named by the text inside it");
    assert!(send.focusable && send.place.is_some());
    let field = t.one(|n| n.role == Role::TextInput);
    assert_eq!(field.value.as_deref(), Some("hello"));
    let list = t.one(|n| n.role == Role::List && n.row_count == Some(500));
    assert_eq!(list.row_count, Some(500));
    let item = t.get(list.children[3]).unwrap();
    assert_eq!((item.role, item.name.as_str(), item.row_index), (Role::ListItem, "msg 3", Some(3)));
    assert_eq!(item.selected, Some(false), "the row's fill compares Sel with its own constant");
    // select row 3 with a click: the state follows the word
    let p = item.place.unwrap();
    w.activate(&mut v, p);
    w.settle(&v);
    let up = ax.update(&w, &v);
    assert_eq!(up.changed.len(), 1, "only the selected item changed: {:?}", up.changed.iter().map(|n| &n.name).collect::<Vec<_>>());
    assert_eq!(ax.tree().one(|n| n.selected == Some(true)).name, "msg 3");
    // focus: Tab to the button, then into the list; the tree's focus follows the core's
    w.tab(&mut v, false);
    ax.update(&w, &v);
    assert_eq!(ax.tree().get(ax.tree().focus.unwrap()).unwrap().name, "Send");
    w.tab(&mut v, false);
    w.tab(&mut v, false);
    ax.update(&w, &v);
    let f = ax.tree().get(ax.tree().focus.unwrap()).unwrap();
    assert!(f.focused);
    assert_eq!((f.role, f.row_index), (Role::ListItem, Some(0)));
    // text typed into the focused field is the input's value
    w.click(&mut v, 410, 30);
    w.text(&mut v, "!");
    w.settle(&v);
    ax.update(&w, &v);
    assert_eq!(ax.tree().one(|n| n.role == Role::TextInput).value.as_deref(), Some("hello!"));
    assert_ne!(w.shown.w[U_FOCUS], NONE);
}

#[test]
fn focus_on_a_row_that_scrolled_away() {
    let mut v = Inbox::new(500);
    let mut w = World::<Soft>::new(IW, IH, 4);
    w.mount(&v);
    let mut ax = Ax::new();
    ax.update(&w, &v);
    w.click(&mut v, 300, ITOP + 4 * ROW + 5);
    ax.update(&w, &v);
    let f = ax.tree().focus.unwrap();
    assert_eq!(f.row, 4);
    // the row leaves exposure and the published margin: the tree still says what is focused only if published
    let list = ax.tree().one(|n| n.role == Role::List && n.row_count == Some(500)).id;
    let path = list.path();
    assert!(w.scroll_into_view(&mut v, &path, 300), "an explicit action scrolls");
    w.settle(&v);
    ax.update(&w, &v);
    assert_eq!(w.shown.w[U_FOCUS], NONE);
    assert_eq!(w.focused().map(|f| f.2), Some(4), "the core still knows");
    assert!(ax.tree().get(ax.tree().one(|n| n.role == Role::List && n.row_count == Some(500)).children[0]).unwrap().row_index.unwrap() > 250);
    let _ = NodeId { path: [0; 6], depth: 0, row: 0, pos: 0, tag: Tag::Rec };
}

#[test]
fn rows_appended_at_the_end_are_announced() {
    let mut v = Inbox::new(3);
    let mut w = World::<Soft>::new(IW, IH, 4);
    w.mount(&v);
    let mut ax = Ax::new();
    ax.update(&w, &v);
    v.items.insert(3, Txt::new("new mail"));
    w.settle(&v);
    let up = ax.update(&w, &v);
    assert_eq!(up.announce, ["new mail"]);
}

#[test]
#[ignore]
fn dump() {
    let (s, w) = mounted(100);
    let mut ax = Ax::new();
    ax.update(&w, &s);
    println!("{}", ax.tree().dump().lines().take(60).collect::<Vec<_>>().join("\n"));
    let v = Inbox::new(5);
    let mut w = World::<Soft>::new(IW, IH, 4);
    w.mount(&v);
    let mut ax = Ax::new();
    ax.update(&w, &v);
    println!("{}", ax.tree().dump());
}
