//! Accessibility as one more projection of the stores and the view.
//!
//! Nothing here exists until an assistive technology asks: [`Ax`] is created by whoever serves one, and every
//! update is pulled ([`Ax::update`]) after a turn. The tree is computed from reflection (`bough_x::reflect`): the
//! records the stores hold for fixed shapes, and, for lists, rows described from the view's own row function, so
//! a list of 100,000 rows publishes its exposed window plus a page on each side and says it has 100,000 rows;
//! any other row is described on demand ([`Ax::rows`]) with nothing materialised and nothing scrolled.
//!
//! What an author does not write: roles, labels, states. They are read off what is there:
//! * a record with a click handler is a button named by the text inside it; a text handler makes a text input
//!   whose value is the text inside it; a drag handler a splitter; a focusable record a group;
//! * a list whose rows hold one interactive record is a list of items named by their texts; a list whose rows
//!   hold several texts is a grid, its cells the texts, its column headers the texts elsewhere that line up with
//!   its columns;
//! * a record is *selected* when one of its operands compares a block word with one of its own constants
//!   (`x_eq(U(w), Rec)` in the expression's code) and they are equal, or when a selection indicator (an outline)
//!   rests on it; *focused* when it is the core's `Focus`;
//! * a text or number that reads application state is a polite live region; rows appended at the end of a list
//!   are announced.
//!
//! In motion the tree reports where things will rest: everything is evaluated under the *settled* block (the
//! clock run [`REDUCED_MOTION_AHEAD`] ahead, as reduced motion shows it), so an animation causes no update at all.
//!
//! Deltas: an update first compares the block words the tree was built from (the content's and the states'
//! reads, the scroll words, `Focus`) and the relation epoch with what it last saw; if none moved (a theme, a hover,
//! a motion) it does nothing. Otherwise it projects the published set again and compares node by node. The gate
//! (`tests/ax.rs`) holds the tree kept by applying deltas equal to a fresh projection after every step.
use bough_x::event::{NONE, REDUCED_MOTION_AHEAD, U_FOCUS, kind};
use bough_x::expr::{APP_WORDS, Code, U, U_TIME};
use bough_x::reflect::{Kind as SKind, Part, Reflect, Snap, StoreInfo, Val};
use bough_x::sink::{REL_EPOCH, View};
use bough_x::store::{Backend, NO_WORD, Path, Rect, Store};
use bough_x::world::World;
use core::sync::atomic::Ordering;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    Window,
    Group,
    ScrollView,
    List,
    ListItem,
    Grid,
    Row,
    Cell,
    ColumnHeader,
    Button,
    TextInput,
    StaticText,
    Splitter,
}

/// A node's identity: the store, the row (a row call of a fixed shape, or a list's row), the position in the
/// row's shape, and what the node stands for. It names the same thing across scrolling and re-description.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct NodeId {
    pub path: [u16; 6],
    pub depth: u16,
    pub row: u32,
    pub pos: u32,
    pub tag: Tag,
}
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Tag {
    Window,
    /// A store (a scrolling scope, a list).
    Store,
    /// A list's row.
    Row,
    /// A record.
    Rec,
}
impl NodeId {
    fn of(path: &Path, row: u32, pos: u32, tag: Tag) -> NodeId {
        NodeId { path: path.at, depth: path.n, row, pos, tag }
    }
    pub fn path(&self) -> Path {
        Path { at: self.path, n: self.depth }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Live {
    Off,
    Polite,
}

/// Actions a node accepts.
pub mod act {
    pub const CLICK: u32 = 1;
    pub const FOCUS: u32 = 2;
    pub const SET_VALUE: u32 = 4;
    pub const SCROLL: u32 = 8;
}

#[derive(Clone, PartialEq, Debug)]
pub struct Node {
    pub id: NodeId,
    pub role: Role,
    pub name: String,
    pub value: Option<String>,
    /// On screen, where it will rest.
    pub bounds: Rect,
    pub children: Vec<NodeId>,
    pub focusable: bool,
    pub focused: bool,
    pub selected: Option<bool>,
    pub actions: u32,
    pub row_index: Option<u32>,
    pub row_count: Option<u32>,
    pub col_index: Option<u32>,
    pub col_count: Option<u32>,
    pub live: Live,
    /// A scroll view: its offset and the extent of what it scrolls.
    pub scroll: Option<(i32, i32)>,
    /// The record it was made from, when it is one, and where the view described it.
    pub place: Option<u32>,
    pub loc: Option<&'static core::panic::Location<'static>>,
}
impl Node {
    fn new(id: NodeId, role: Role, bounds: Rect) -> Node {
        Node { id, role, name: String::new(), value: None, bounds, children: Vec::new(), focusable: false, focused: false, selected: None, actions: 0, row_index: None, row_count: None, col_index: None, col_count: None, live: Live::Off, scroll: None, place: None, loc: None }
    }
}

/// The accessibility tree.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Tree {
    pub nodes: BTreeMap<NodeId, Node>,
    pub root: Option<NodeId>,
    pub focus: Option<NodeId>,
}
impl Tree {
    pub fn get(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(&id)
    }
    /// Every node for which `f` holds, in tree order.
    pub fn all(&self, f: impl Fn(&Node) -> bool) -> Vec<&Node> {
        let mut out = Vec::new();
        if let Some(r) = self.root {
            self.walk(r, &mut |n| {
                if f(n) {
                    out.push(n)
                }
            });
        }
        out
    }
    /// The one node for which `f` holds (panics otherwise: a test's assertion).
    pub fn one(&self, f: impl Fn(&Node) -> bool) -> &Node {
        let v = self.all(f);
        assert!(v.len() == 1, "{} nodes match, not one", v.len());
        v[0]
    }
    pub fn walk<'a>(&'a self, id: NodeId, f: &mut impl FnMut(&'a Node)) {
        if let Some(n) = self.nodes.get(&id) {
            f(n);
            for &c in &n.children {
                self.walk(c, f);
            }
        }
    }
    /// Apply an update, as a platform adapter would.
    pub fn apply(&mut self, u: &Update) {
        for id in &u.removed {
            self.nodes.remove(id);
        }
        for n in &u.changed {
            self.nodes.insert(n.id, n.clone());
        }
        if let Some(r) = u.root {
            self.root = Some(r);
        }
        if let Some(f) = u.focus {
            self.focus = f;
        }
    }
    /// The tree as text: role, name, value, states, one node a line (a dump for people and for agents).
    pub fn dump(&self) -> String {
        let mut s = String::new();
        fn go(t: &Tree, id: NodeId, d: usize, s: &mut String) {
            let Some(n) = t.nodes.get(&id) else { return };
            let mut line = format!("{:w$}{:?}", "", n.role, w = d * 2);
            if !n.name.is_empty() {
                line += &format!(" {:?}", n.name);
            }
            if let Some(v) = &n.value {
                line += &format!(" value={v:?}");
            }
            if let Some(c) = n.row_count {
                line += &format!(" rows={c}");
            }
            if let Some(c) = n.col_count {
                line += &format!(" cols={c}");
            }
            if let Some(r) = n.row_index {
                line += &format!(" row={r}");
            }
            if let Some(c) = n.col_index {
                line += &format!(" col={c}");
            }
            if n.selected == Some(true) {
                line += " selected";
            }
            if n.focused {
                line += " focused";
            }
            if n.live == Live::Polite {
                line += " live";
            }
            s.push_str(&line);
            s.push('\n');
            for &c in &n.children {
                go(t, c, d + 1, s);
            }
        }
        if let Some(r) = self.root {
            go(self, r, 0, &mut s);
        }
        s
    }
}

/// What changed since the last update: nodes added or changed (whole), nodes removed, the focus if it moved,
/// and what to announce.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Update {
    pub changed: Vec<Node>,
    pub removed: Vec<NodeId>,
    pub root: Option<NodeId>,
    pub focus: Option<Option<NodeId>>,
    pub announce: Vec<String>,
}
impl Update {
    pub fn is_empty(&self) -> bool {
        self.changed.is_empty() && self.removed.is_empty() && self.focus.is_none() && self.announce.is_empty()
    }
}

/// The accessibility projection of one world, kept by whoever serves an assistive technology.
pub struct Ax {
    tree: Tree,
    refl: Option<Reflect>,
    /// What the last projection was made from: the block, the relation epoch, the focus, the words it read.
    block: U,
    epoch: u32,
    focus: Option<(Path, u32, u32)>,
    reads: u64,
    /// Each list's row count as last published (for announcing appended rows).
    counts: BTreeMap<NodeId, u32>,
    /// Nodes projected by the last update (0 when it found nothing to do).
    pub projected: usize,
}

impl Default for Ax {
    fn default() -> Self {
        Ax::new()
    }
}

impl Ax {
    pub fn new() -> Ax {
        Ax { tree: Tree::default(), refl: None, block: U::default(), epoch: u32::MAX, focus: None, reads: !0, counts: BTreeMap::new(), projected: 0 }
    }
    pub fn tree(&self) -> &Tree {
        &self.tree
    }

    /// Bring the tree up to date after a turn; what changed.
    pub fn update<V: View, B: Backend>(&mut self, w: &World<B>, v: &V) -> Update {
        let epoch = REL_EPOCH.load(Ordering::Relaxed);
        let focus = w.focused();
        let current = self.refl.as_ref().is_some_and(|r| r.current(&w.root));
        if current && epoch == self.epoch && focus == self.focus && self.block.diff(&w.shown, 64) & self.reads == 0 {
            self.projected = 0;
            return Update::default();
        }
        if !current {
            self.refl = Some(Reflect::learn(&w.root, v));
        }
        let mut p = Project::new(w, v, self.refl.as_ref().unwrap());
        p.run();
        let (fresh, reads) = (p.tree, p.reads);
        self.projected = fresh.nodes.len();
        let mut up = Update::default();
        for (id, n) in &fresh.nodes {
            if self.tree.nodes.get(id) != Some(n) {
                up.changed.push(n.clone());
            }
        }
        for id in self.tree.nodes.keys() {
            if !fresh.nodes.contains_key(id) {
                up.removed.push(*id);
            }
        }
        if fresh.root != self.tree.root {
            up.root = fresh.root;
        }
        if fresh.focus != self.tree.focus {
            up.focus = Some(fresh.focus);
        }
        // rows appended at the end of a list are announced (a chat's arrival, a log's line)
        for n in fresh.nodes.values().filter(|n| matches!(n.role, Role::List | Role::Grid)) {
            let (Some(c), Some(&was)) = (n.row_count, self.counts.get(&n.id)) else { continue };
            if c > was {
                for r in fresh.nodes.values().filter(|r| r.id.tag == Tag::Row && r.id.path() == n.id.path() && r.id.row >= was) {
                    up.announce.push(text_of(&fresh, r.id));
                }
            }
        }
        self.counts = fresh.nodes.values().filter_map(|n| n.row_count.map(|c| (n.id, c))).collect();
        self.tree = fresh;
        self.block = w.shown;
        self.epoch = epoch;
        self.focus = focus;
        self.reads = reads;
        up
    }

    /// Rows `rows` of a list node, described on demand: rows nobody materialised, with nothing written and
    /// nothing scrolled (what a screen reader's table navigation asks for).
    pub fn rows<V: View, B: Backend>(&mut self, w: &World<B>, v: &V, list: NodeId, rows: core::ops::Range<u32>) -> Vec<Node> {
        if !self.refl.as_ref().is_some_and(|r| r.current(&w.root)) {
            self.refl = Some(Reflect::learn(&w.root, v));
        }
        let mut p = Project::new(w, v, self.refl.as_ref().unwrap());
        let path = list.path();
        let Some(info) = p.r.root.at(&path) else { return Vec::new() };
        let SKind::List(li) = &info.kind else { return Vec::new() };
        let grid = p.list_is_grid(&path, li.n);
        let ids = p.list_rows(&path, rows, grid, None);
        let mut out: Vec<Node> = Vec::new();
        for id in ids {
            p.tree.walk(id, &mut |n| out.push(n.clone()));
        }
        out
    }
}

/// The text a person would hear for a node and its descendants.
fn text_of(t: &Tree, id: NodeId) -> String {
    let mut parts = Vec::new();
    t.walk(id, &mut |n| {
        if !n.name.is_empty() && !matches!(n.role, Role::Row | Role::ListItem) {
            parts.push(n.name.clone());
        }
    });
    if parts.is_empty()
        && let Some(n) = t.nodes.get(&id)
    {
        parts.push(n.name.clone());
    }
    parts.join(" ")
}

/// One projection: the published tree and the block words it read.
struct Project<'a, V, B> {
    w: &'a World<B>,
    v: &'a V,
    r: &'a Reflect,
    /// The settled block: the clock run ahead, so everything is where it will rest.
    u: U,
    tree: Tree,
    reads: u64,
    /// Record -> the node that stands for it (a record's own node, or the one that took its text).
    owner: BTreeMap<(Path2, u32, u32), NodeId>,
    /// Selection indicators (outlines) at rest, on screen.
    rings: Vec<Rect>,
}
type Path2 = ([u16; 6], u16);
fn p2(p: &Path) -> Path2 {
    (p.at, p.n)
}

/// A record as the projection reads it.
struct R<'s> {
    s: &'s Snap,
    screen: Rect,
    kinds: u32,
    content: Option<String>,
    live: bool,
    selected: Option<bool>,
}

const INTERACTIVE: u32 = kind::CLICK | kind::PRESS | kind::DRAG | kind::TEXT | kind::KEY | kind::FOCUSABLE | kind::WHEEL;

impl<'a, V: View, B: Backend> Project<'a, V, B> {
    fn new(w: &'a World<B>, v: &'a V, r: &'a Reflect) -> Self {
        let mut u = w.shown;
        u.w[U_TIME] = w.now().wrapping_add(REDUCED_MOTION_AHEAD);
        Project { w, v, r, u, tree: Tree::default(), reads: 1 << U_FOCUS, owner: BTreeMap::new(), rings: Vec::new() }
    }

    fn run(&mut self) {
        let root = Path::default();
        let id = NodeId::of(&root, 0, 0, Tag::Window);
        let mut win = Node::new(id, Role::Window, self.w.root.space.clip);
        // selection indicators anywhere, at rest
        let u = self.u;
        for it in self.r.items(&self.w.root, &u) {
            if it.kind() == "outline" {
                self.rings.push(it.screen);
            }
        }
        win.children = self.store(&self.r.root, &self.w.root, root);
        self.tree.nodes.insert(id, win);
        self.tree.root = Some(id);
        self.headers();
        self.focus();
    }

    /// The nodes of a store's rows and child stores, in the order the view described them.
    fn store(&mut self, info: &StoreInfo, st: &Store, path: Path) -> Vec<NodeId> {
        let u = self.u;
        let items: Vec<Snap> = self.r.items(&self.w.root, &u).into_iter().filter(|i| i.ent.path == path).map(|i| i.snap()).collect();
        let mut out = Vec::new();
        for part in &info.seq {
            match *part {
                Part::Row(k) => {
                    let row: Vec<&Snap> = items.iter().filter(|s| info.sites.get(site_of(info, s)).and_then(|x| x.as_ref()).is_some_and(|x| x.row == k)).collect();
                    out.extend(self.row(&path, k, &row, false, st.space.origin(&u)));
                }
                Part::Sub(q) => {
                    let (Some(si), Some(ss)) = (info.subs.get(q as usize), st.subs.get(q as usize)) else { continue };
                    let mut p = path;
                    p.at[p.n as usize] = q as u16;
                    p.n += 1;
                    match &si.kind {
                        SKind::List(li) => {
                            let n = li.n;
                            let grid = self.list_is_grid(&p, n);
                            let page = ss.hi.saturating_sub(ss.lo).max(1);
                            let range = ss.lo.saturating_sub(page)..(ss.hi + page).min(n);
                            let id = NodeId::of(&p, 0, 0, Tag::Store);
                            let rows = self.list_rows(&p, range, grid, Some(si));
                            let mut node = Node::new(id, if grid { Role::Grid } else { Role::List }, ss.space.clip);
                            node.row_count = Some(n);
                            node.children = rows;
                            node.loc = si.loc;
                            if grid {
                                node.col_count = node.children.first().and_then(|r| self.tree.nodes.get(r)).map(|r| r.children.len() as u32);
                            }
                            self.tree.nodes.insert(id, node);
                            out.push(id);
                        }
                        _ => {
                            let kids = self.store(si, ss, p);
                            if ss.space.scroll != NO_WORD {
                                let id = NodeId::of(&p, 0, 0, Tag::Store);
                                let mut node = Node::new(id, Role::ScrollView, ss.space.clip);
                                let word = ss.space.scroll as usize;
                                self.reads |= 1 << word;
                                let extent = ss.subs.iter().filter(|s| s.src.is_some()).map(|s| s.geo.1 + s.n as i32 * s.geo.2).max().unwrap_or(0);
                                node.scroll = Some((u.w[word] as i32, extent));
                                node.actions = act::SCROLL;
                                node.children = kids;
                                node.loc = si.loc;
                                self.tree.nodes.insert(id, node);
                                out.push(id);
                            } else {
                                out.extend(kids);
                            }
                        }
                    }
                }
            }
        }
        out
    }

    /// Rows of the list at `path`, described from the view (materialised or not).
    fn list_rows(&mut self, path: &Path, rows: core::ops::Range<u32>, grid: bool, _info: Option<&StoreInfo>) -> Vec<NodeId> {
        let u = self.u;
        let snaps = Reflect::rows(self.v, &u, path, rows.clone());
        let org = self.w.root.at(path).map_or((0, 0), |s| s.space.origin(&u));
        let mut out = Vec::new();
        for r in rows {
            let row: Vec<&Snap> = snaps.iter().filter(|s| s.ent.row == r).collect();
            if row.is_empty() {
                continue;
            }
            let kids = self.row(path, r, &row, grid, org);
            let id = NodeId::of(path, r, 0, Tag::Row);
            let bounds = kids.iter().filter_map(|k| self.tree.nodes.get(k)).map(|n| n.bounds).reduce(|a, b| a.or(b)).unwrap_or_default();
            // a row that is one interactive item (and its texts) is that item; otherwise a row of cells
            let item = (!grid && kids.len() == 1).then(|| self.tree.nodes.get(&kids[0]).cloned()).flatten();
            let mut node = match item {
                Some(mut n) if n.role != Role::StaticText => {
                    self.tree.nodes.remove(&kids[0]);
                    for v in self.owner.values_mut() {
                        if *v == kids[0] {
                            *v = id;
                        }
                    }
                    n.id = id;
                    n.role = Role::ListItem;
                    n
                }
                _ => {
                    let mut n = Node::new(id, if grid { Role::Row } else { Role::ListItem }, bounds);
                    n.children = kids;
                    if !grid {
                        n.name = n.children.iter().filter_map(|k| self.tree.nodes.get(k)).map(|c| c.name.clone()).collect::<Vec<_>>().join(" ");
                    }
                    n
                }
            };
            node.row_index = Some(r);
            if node.selected.is_none() && self.rings.contains(&node.bounds) {
                node.selected = Some(true);
            }
            self.tree.nodes.insert(id, node);
            out.push(id);
        }
        out
    }

    /// A list is a grid when its rows hold more than one text and no interactive record takes them.
    fn list_is_grid(&mut self, path: &Path, n: u32) -> bool {
        if n == 0 {
            return false;
        }
        let u = self.u;
        let row = Reflect::rows(self.v, &u, path, 0..1);
        let contents = row.iter().filter(|s| s.vals.iter().any(|v| v.is_content())).count();
        let interactive = row.iter().any(|s| s.on.is_some_and(|o| o.kinds & INTERACTIVE != 0));
        contents >= 2 && !interactive
    }

    /// The nodes of one row: interactive records named by the texts inside them, other texts on their own,
    /// decoration dropped. In a grid every text is a cell.
    fn row(&mut self, path: &Path, row: u32, snaps: &[&Snap], grid: bool, org: (i32, i32)) -> Vec<NodeId> {
        let u = self.u;
        let mut rs: Vec<R> = snaps
            .iter()
            .map(|s| {
                let mut live = false;
                let mut content = None;
                for (i, v) in s.vals.iter().enumerate() {
                    if v.is_content() && content.is_none() {
                        content = Some(v.text());
                        let op = &s.ops[i];
                        self.reads |= op.reads;
                        live = op.reads & ((1u64 << APP_WORDS) - 1) != 0;
                    }
                }
                R { s, screen: s.rect.shift(org), kinds: s.on.map_or(0, |o| o.kinds), content, live, selected: selected(s, &u, &mut self.reads) }
            })
            .collect();
        rs.sort_by_key(|r| r.s.ent.pos);
        let mut out = Vec::new();
        let mut taken = vec![false; rs.len()];
        let mut col = 0u32;
        // interactive records first: each takes the texts inside it
        for i in 0..rs.len() {
            if rs[i].kinds & INTERACTIVE == 0 || grid {
                continue;
            }
            let r = &rs[i];
            let id = NodeId::of(path, row, r.s.ent.pos, Tag::Rec);
            let k = r.kinds;
            let role = if k & kind::TEXT != 0 {
                Role::TextInput
            } else if k & kind::CLICK != 0 {
                Role::Button
            } else if k & kind::DRAG != 0 {
                Role::Splitter
            } else {
                Role::Group
            };
            let mut n = Node::new(id, role, r.screen);
            let mut texts = Vec::new();
            for j in 0..rs.len() {
                if j != i && !taken[j] && rs[j].content.is_some() && rs[j].kinds & INTERACTIVE == 0 && inside(rs[j].screen, r.screen) {
                    taken[j] = true;
                    texts.push(rs[j].content.clone().unwrap());
                    self.owner.insert((p2(path), row, rs[j].s.ent.pos), id);
                }
            }
            if role == Role::TextInput {
                n.value = Some(texts.join(" "));
                n.actions |= act::SET_VALUE;
            } else {
                n.name = texts.join(" ");
            }
            n.focusable = k & kind::FOCUSABLE != 0;
            if n.focusable {
                n.actions |= act::FOCUS;
            }
            if k & kind::CLICK != 0 {
                n.actions |= act::CLICK;
            }
            n.selected = r.selected;
            if n.selected.is_none() && self.rings.contains(&r.screen) {
                n.selected = Some(true);
            }
            n.loc = Some(r.s.loc);
            n.place = self.place(path, row, r.s);
            taken[i] = true;
            self.owner.insert((p2(path), row, r.s.ent.pos), id);
            self.tree.nodes.insert(id, n);
            out.push(id);
        }
        // texts on their own (in a grid: the cells)
        for (i, r) in rs.iter().enumerate() {
            if taken[i] || r.content.is_none() {
                continue;
            }
            let id = NodeId::of(path, row, r.s.ent.pos, Tag::Rec);
            let mut n = Node::new(id, if grid { Role::Cell } else { Role::StaticText }, r.screen);
            n.name = r.content.clone().unwrap();
            n.live = if r.live { Live::Polite } else { Live::Off };
            n.selected = r.selected;
            if grid {
                n.col_index = Some(col);
                col += 1;
                // a cell is selected when an indicator rests on the cell it is drawn in
                if n.selected.is_none() && self.rings.iter().any(|g| *g == r.screen || rs.iter().any(|c| c.screen == *g && inside(r.screen, c.screen))) {
                    n.selected = Some(true);
                }
            }
            n.loc = Some(r.s.loc);
            n.place = self.place(path, row, r.s);
            self.owner.insert((p2(path), row, r.s.ent.pos), id);
            self.tree.nodes.insert(id, n);
            out.push(id);
        }
        out
    }

    /// The place of a record, if it is materialised.
    fn place(&self, path: &Path, row: u32, s: &Snap) -> Option<u32> {
        let st = self.w.root.at(path)?;
        let site = st.sites.get(s.ent.pos as usize)?;
        let slot = if st.cap == 1 { 0 } else if row >= st.lo && row < st.hi { row % st.cap } else { return None };
        let rec = site.base + slot;
        // the world numbers columns in paint order; find this one without needing `&mut World`
        let mut i = 0u32;
        let mut found = None;
        fn walk(st: &Store, path: &mut Path, want: &Path, col: u32, i: &mut u32, found: &mut Option<u32>) {
            for p in &st.order {
                match *p {
                    bough_x::store::Pass::Col(j) => {
                        if *path == *want && j == col && found.is_none() {
                            *found = Some(*i);
                        }
                        *i += 1;
                    }
                    bough_x::store::Pass::Sub(q) => {
                        path.at[path.n as usize] = q as u16;
                        path.n += 1;
                        walk(&st.subs[q as usize], path, want, col, i, found);
                        path.n -= 1;
                    }
                }
            }
        }
        walk(&self.w.root, &mut Path::default(), path, site.col, &mut i, &mut found);
        found.map(|c| c << bough_x::event::PLACE_REC_BITS | rec)
    }

    /// Texts outside a grid that line up with its columns are its column headers.
    fn headers(&mut self) {
        let grids: Vec<(NodeId, Vec<(i32, i32)>)> = self
            .tree
            .nodes
            .values()
            .filter(|n| n.role == Role::Grid)
            .filter_map(|g| {
                let first = self.tree.nodes.get(g.children.first()?)?;
                Some((g.id, first.children.iter().filter_map(|c| self.tree.nodes.get(c)).map(|c| (c.bounds.x0, c.bounds.x1)).collect()))
            })
            .collect();
        for (gid, cols) in grids {
            let gpath = gid.path();
            let mut hits = Vec::new();
            for n in self.tree.nodes.values() {
                if n.role == Role::StaticText && n.live == Live::Off && !n.id.path().at[..n.id.depth as usize].starts_with(&gpath.at[..gpath.n as usize]) {
                    if let Some(c) = cols.iter().position(|&(x0, x1)| x0 == n.bounds.x0 && x1 == n.bounds.x1) {
                        hits.push((n.id, c as u32));
                    }
                }
            }
            // a header row: most columns matched by texts of one row
            if hits.len() * 2 >= cols.len() {
                for (id, c) in hits {
                    let n = self.tree.nodes.get_mut(&id).unwrap();
                    n.role = Role::ColumnHeader;
                    n.col_index = Some(c);
                }
            }
        }
    }

    /// The node that stands for the focused record.
    fn focus(&mut self) {
        let Some((path, site, row)) = self.w.focused() else { return };
        let Some(info) = self.r.root.at(&path) else { return };
        let Some(Some(si)) = info.sites.get(site as usize) else { return };
        let row = if matches!(info.kind, SKind::List(_)) { row } else { si.row };
        if let Some(&id) = self.owner.get(&(p2(&path), row, si.pos)) {
            self.tree.focus = Some(id);
            if let Some(n) = self.tree.nodes.get_mut(&id) {
                n.focused = true;
            }
        }
        let _ = (NONE, self.w.shown.w[U_FOCUS]);
    }
}

/// The index of the site a stored record came from (its position in the store's shape).
fn site_of(_info: &StoreInfo, s: &Snap) -> usize {
    s.ent.pos as usize
}

fn inside(a: Rect, b: Rect) -> bool {
    a.x0 >= b.x0 && a.y0 >= b.y0 && a.x1 <= b.x1 && a.y1 <= b.y1
}

/// Whether a record's operands compare a block word with one of its own constants (`x_eq(U(w), Rec)`), and if
/// so whether they are equal now: the record is the one that word selects.
fn selected(s: &Snap, u: &U, reads: &mut u64) -> Option<bool> {
    let mut out = None;
    for op in s.ops {
        let mut word = op.word as usize;
        eq_words(op.code, &mut word, &mut |w, k| {
            *reads |= 1 << w;
            if let Some(&c) = s.words.get(k) {
                out = Some(out.unwrap_or(false) | (u.w[w] == c));
            }
        });
    }
    out
}
fn eq_words(c: &Code, next: &mut usize, f: &mut impl FnMut(usize, usize)) {
    match c {
        Code::Rec => *next += 1,
        Code::Call("x_eq", args) if args.len() == 2 => {
            let k = *next;
            match (args[0], args[1]) {
                (Code::U(w), Code::Rec) | (Code::Rec, Code::U(w)) => {
                    f(*w, k);
                    *next += 1;
                }
                _ => {
                    for a in *args {
                        eq_words(a, next, f);
                    }
                }
            }
        }
        Code::Call(_, args) | Code::Cpu(args) => {
            for a in *args {
                eq_words(a, next, f);
            }
        }
        Code::UAt(..) => *next += 1,
        _ => {}
    }
}

#[doc(hidden)]
pub fn _val_is_content(v: &Val) -> bool {
    v.is_content()
}
#[doc(hidden)]
pub fn _unused(_: &BTreeSet<u32>) {}
