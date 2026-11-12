//! The reflected tree: every record of a window, what the runtime retains about it, and what its description
//! said (`docs/lanes/composition/DESIGN.md` §4). One read for four consumers: a test's queries (`find.rs`),
//! the inspector, the devtools server (`devtools.rs`) and the accessibility projection (`Tree::semantic`).
//!
//! A [`Tree`] is a snapshot, built on demand by [`App::tree`]: one pass over the records in paint order (the
//! hierarchy is the depth column; geometry is the layout box or the visible ink; text, paint, routes, focus
//! and hover are read from the columns that hold them) joined with one walk of the description
//! (`probe.rs`: element types, sources, parts, author statements, owners, bound signals). Nothing is kept
//! between snapshots and nothing is written while the app runs.
use crate::app::{App, View};
use crate::color::Color;
use crate::probe::{Bind, OwnerKind, Role, Verb, Walk};
use crate::rt::*;
use bough_core::{Handle, KF_FOCUS, KF_INTERACT, KF_NOHIT, NRect};
use std::panic::Location;

/// Interaction and visibility states of a record, as bits.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct States(pub u32);
impl States {
    pub const HOVERED: States = States(1);
    pub const PRESSED: States = States(2);
    pub const FOCUSED: States = States(4);
    /// It or a descendant has focus.
    pub const FOCUS_WITHIN: States = States(8);
    pub const FOCUSABLE: States = States(16);
    /// It has an act (routes, hover style, a field, a scroller).
    pub const INTERACTIVE: States = States(32);
    /// Nothing of it is visible (clipped away, empty, or outside the window).
    pub const HIDDEN: States = States(64);
    /// Pointer-transparent.
    pub const NOHIT: States = States(128);
    pub const NAMES: [(States, &'static str); 8] = [
        (States::HOVERED, "hovered"),
        (States::PRESSED, "pressed"),
        (States::FOCUSED, "focused"),
        (States::FOCUS_WITHIN, "focus-within"),
        (States::FOCUSABLE, "focusable"),
        (States::INTERACTIVE, "interactive"),
        (States::HIDDEN, "hidden"),
        (States::NOHIT, "nohit"),
    ];
    pub fn has(self, s: States) -> bool {
        self.0 & s.0 == s.0
    }
    fn set(&mut self, s: States, on: bool) {
        if on {
            self.0 |= s.0;
        }
    }
    /// A state by name (`"focused"`).
    pub fn named(name: &str) -> Option<States> {
        Self::NAMES.iter().find(|n| n.1 == name).map(|n| n.0)
    }
}
impl std::fmt::Debug for States {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names: Vec<&str> = Self::NAMES.iter().filter(|n| self.has(n.0)).map(|n| n.1).collect();
        write!(f, "{{{}}}", names.join(" "))
    }
}

/// The kind of record.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Box,
    Quad,
    Text,
    Foreign,
    Other,
}

/// One record as reflection reads it.
#[derive(Clone, Debug)]
pub struct Node {
    /// Position in the paint preorder (also the node's index in [`Tree::nodes`]).
    pub pos: usize,
    /// The record's handle (index and generation): what an action names. Zero for a record with none.
    pub handle: Handle,
    pub depth: u16,
    pub parent: Option<usize>,
    pub kind: Kind,
    /// What it is: what its author said, else what its act and kind imply.
    pub role: Role,
    /// Its accessible name: the author's label, else its text, else (buttons, items, labelled groups) the
    /// text of its subtree.
    pub name: String,
    /// A text record's text.
    pub text: Option<String>,
    /// A text field's content.
    pub value: Option<String>,
    /// The layout box (absolute device px `[x, y, w, h]`), if the record is a layout node.
    pub bounds: Option<[f32; 4]>,
    /// What of it can show: the layout box (or the ink) ∩ its clip ∩ the window, device px `[x0, y0, x1, y1]`.
    pub visible: Option<[i32; 4]>,
    /// Painted colour (a quad's fill as drawn now: the hover colour while hovered), corner radius.
    pub fill: Option<Color>,
    pub radius: f32,
    /// A text's colour and size (device px).
    pub text_color: Option<Color>,
    pub font_px: Option<f32>,
    pub states: States,
    /// The routes input reaches it by, with their closures' type names where the description named them.
    pub routes: Vec<(Verb, Option<&'static str>)>,
    /// Signals bound into its operands.
    pub binds: Vec<Bind>,
    /// The element type that emitted it and the builder's short name (`"row"`, `"text"`, ...).
    pub element: Option<&'static str>,
    pub elem: &'static str,
    /// Where it was built (parts, `Describe`, foreign elements).
    pub source: Option<&'static Location<'static>>,
    pub part: Option<(&'static str, u32)>,
    pub id: Option<&'static str>,
    /// Index of its owner in [`Tree::owners`].
    pub owner: u32,
    /// A foreign element's id.
    pub foreign: Option<u64>,
    /// It lies inside an element the walk sees whole (a text field's caret and label, a tooltip's bubble):
    /// a record of that element's implementation, not an element of the description.
    pub inner: bool,
}

impl Node {
    /// The visible rect's centre in points (where a pointer acts on it), or `None` if nothing of it shows.
    pub fn center_pt(&self, scale: f32) -> Option<(f32, f32)> {
        let [x0, y0, x1, y1] = self.visible?;
        Some(((x0 + x1) as f32 * 0.5 / scale, (y0 + y1) as f32 * 0.5 / scale))
    }
    /// The bound signal of property `prop` (`"text"`, `"bg"`, ...).
    pub fn bind(&self, prop: &str) -> Option<&Bind> {
        self.binds.iter().find(|b| b.prop == prop)
    }
}

/// An owner as the tree lists it.
#[derive(Clone, Debug)]
pub struct OwnerView {
    pub ty: &'static str,
    pub kind: OwnerKind,
    pub parent: Option<u32>,
    /// The field path from its parent owner (`embed!`).
    pub field: Option<String>,
}

/// A window's records with what is known about each.
#[derive(Clone, Debug)]
pub struct Tree {
    pub nodes: Vec<Node>,
    pub owners: Vec<OwnerView>,
    /// Attributions the walk refused (its description was out of step with the records there; 0 normally).
    pub lost: u32,
    /// Device px per point.
    pub scale: f32,
}

impl Tree {
    /// The children of node `i`.
    pub fn children(&self, i: usize) -> impl Iterator<Item = &Node> {
        self.nodes.iter().filter(move |n| n.parent == Some(i))
    }
    /// Is node `a` an ancestor of (or) node `b`?
    pub fn contains(&self, a: usize, b: usize) -> bool {
        let mut p = Some(b);
        while let Some(x) = p {
            if x == a {
                return true;
            }
            p = self.nodes[x].parent;
        }
        false
    }
    /// The node of a handle (generation checked).
    pub fn of_handle(&self, h: Handle) -> Option<&Node> {
        self.nodes.iter().find(|n| n.handle == h && h.idx != 0)
    }
    /// The owner path of node `i` (`"Trip > MonthPicker(depart)"`).
    pub fn owner_path(&self, i: usize) -> String {
        let mut v = Vec::new();
        let mut o = Some(self.nodes[i].owner);
        while let Some(k) = o {
            let ow = &self.owners[k as usize];
            v.push(match &ow.field {
                Some(f) => format!("{}({f})", short(ow.ty)),
                None => short(ow.ty).to_string(),
            });
            o = ow.parent;
        }
        v.reverse();
        v.join(" > ")
    }
    /// One line per node, indented by depth: what an agent or the inspector's tree shows.
    /// `role "name" =value #id @part[i] elem x,y wxh {states}` (meaningless boxes are listed as `-`).
    pub fn dump(&self) -> String {
        let mut out = String::new();
        for n in &self.nodes {
            use std::fmt::Write;
            let _ = write!(out, "{}{:?}", "  ".repeat(n.depth as usize), n.role);
            if !n.name.is_empty() {
                let _ = write!(out, " {:?}", n.name);
            }
            if let Some(v) = &n.value {
                let _ = write!(out, " ={v:?}");
            }
            if let Some(id) = n.id {
                let _ = write!(out, " #{id}");
            }
            if let Some((p, i)) = n.part {
                let _ = write!(out, " @{p}[{i}]");
            }
            let _ = write!(out, " {}", n.elem);
            if let Some([x, y, w, h]) = n.bounds {
                let _ = write!(out, " {x},{y} {w}x{h}");
            }
            if n.states.0 != 0 {
                let _ = write!(out, " {:?}", n.states);
            }
            for b in &n.binds {
                let _ = write!(out, " {}<-{}", b.prop, b.path.as_deref().unwrap_or("?"));
            }
            out.push('\n');
        }
        out
    }
}

/// The last path segment of a type name, without generics (`bough::layout::LBox<..>` is `LBox`).
pub fn short(ty: &str) -> &str {
    let t = ty.split('<').next().unwrap_or(ty);
    t.rsplit("::").next().unwrap_or(t)
}

/// Node index of each live façade node by its record handle (`Rt::nodes` is indexed by its own ids here).
fn nodes_by_handle(rt: &Rt) -> std::collections::HashMap<u32, u32> {
    let mut m = std::collections::HashMap::new();
    for (i, n) in rt.nodes.iter().enumerate() {
        if n.live && n.handle.idx != 0 {
            m.insert(n.handle.idx, i as u32);
        }
    }
    m
}

/// Join the walk `w` with the records of `rt`.
pub(crate) fn build(rt: &Rt, w: Walk) -> Tree {
    let by = nodes_by_handle(rt);
    let win = rt.bough.window();
    let focus = rt.input.focus;
    let hover = rt.input.hovered();
    let pressed = rt.input.pressed_handle();
    let mut nodes: Vec<Node> = Vec::with_capacity(w.recs.len());
    let mut stack: Vec<usize> = Vec::new();
    for (pos, r) in w.recs.iter().enumerate() {
        while stack.last().is_some_and(|&p| nodes[p].depth >= r.depth) {
            stack.pop();
        }
        let parent = stack.last().copied();
        let kind = match r.kind & 0x1F {
            REC_BOX => Kind::Box,
            REC_QUAD => Kind::Quad,
            REC_TEXT => Kind::Text,
            REC_FOREIGN => Kind::Foreign,
            _ => Kind::Other,
        };
        let hd = if r.handle != 0 { rt.bough.handle_gen(r.handle) } else { Handle::default() };
        let seen = w.at_rec.get(pos).filter(|&&i| i != u32::MAX).map(|&i| &w.seen[i as usize]);
        // geometry: the layout box, else the ink; what shows of it: ∩ clip ∩ window
        let bounds = (r.handle != 0).then(|| rt.lay.info.get(r.handle as usize)).flatten().filter(|l| l.live && l.written).map(|l| {
            let o = rt.space(l.space);
            [o[0] + l.ax, o[1] + l.ay, l.aw, l.ah]
        });
        let clip = if r.clip == 0 { win } else { rt.bough.clip_rect(r.clip).intersect(win).canon() };
        let shown = match bounds {
            Some([x, y, bw, bh]) => NRect::new(x.floor() as i32, y.floor() as i32, (x + bw).ceil() as i32, (y + bh).ceil() as i32).intersect(clip).canon(),
            None => rt.bough.visible_at(pos).intersect(win).canon(),
        };
        let visible = (!shown.is_empty()).then(|| shown.to_xyxy());
        // retained content
        let node = (r.handle != 0).then(|| by.get(&r.handle).copied()).flatten();
        let tn = node.map(|n| &rt.nodes[n as usize]).filter(|n| n.text != NONE).map(|n| &rt.texts[n.text as usize]);
        let text = tn.map(|t| String::from_utf8_lossy(t.bytes()).into_owned());
        let (mut fill, mut radius) = (None, 0.0);
        let mut image = false;
        if kind == Kind::Quad && r.payload & 0xC000_0000 == 0 && (r.payload as usize) < rt.inst.len() {
            let i = &rt.inst[r.payload as usize];
            fill = Some(Color(if i.kind == KIND_BORDER { i.color2 } else { i.color }));
            radius = i.radius;
            image = i.kind == crate::deco::KIND_IMAGE;
        }
        // the act: routes, a field, a scroller
        let a = if r.handle != 0 { rt.input.act_of_handle(hd) } else { 0 };
        let act = (a != 0).then(|| rt.input.acts[a as usize]);
        let value = act.filter(|a| a.field != NONE).map(|a| String::from_utf8_lossy(rt.texts[rt.input.fields[a.field as usize].ti as usize].bytes()).into_owned());
        let named = |v: Verb| seen.and_then(|s| s.routes.iter().find(|r| r.0 == v)).map(|r| r.1);
        let mut routes = Vec::new();
        if let Some(a) = act {
            for (on, v) in [
                (a.click.is_some(), Verb::Click),
                (a.press.is_some(), Verb::Press),
                (a.hover.is_some(), Verb::Hover),
                (a.key.is_some(), Verb::Key),
                (a.drag.is_some(), Verb::Drag),
                (a.scroll != NONE, Verb::Scroll),
                (a.field != NONE, Verb::Edit),
            ] {
                if on {
                    routes.push((v, named(v)));
                }
            }
        }
        let mut states = States::default();
        states.set(States::HOVERED, r.handle != 0 && hover.contains(&hd));
        states.set(States::PRESSED, r.handle != 0 && pressed == Some(hd));
        states.set(States::FOCUSED, r.handle != 0 && focus == Some(hd));
        states.set(States::FOCUSABLE, r.kind & KF_FOCUS != 0);
        states.set(States::INTERACTIVE, r.kind & KF_INTERACT != 0);
        states.set(States::NOHIT, r.kind & KF_NOHIT != 0);
        states.set(States::HIDDEN, visible.is_none());
        let elem = seen.map_or("", |s| s.name);
        let parent_role = parent.map(|p| nodes[p].role);
        let implied = if act.is_some_and(|a| a.field != NONE) {
            Role::TextInput
        } else if act.is_some_and(|a| a.click.is_some() || a.press.is_some()) {
            Role::Button
        } else if act.is_some_and(|a| a.scroll != NONE) {
            Role::ScrollArea
        } else if kind == Kind::Foreign {
            Role::Foreign
        } else if kind == Kind::Text {
            Role::Text
        } else if image || elem == "image" {
            Role::Image
        } else if elem == "list" {
            Role::List
        } else if elem == "tooltip" {
            Role::Tooltip
        } else if parent_role == Some(Role::List) {
            Role::ListItem
        } else if seen.is_some_and(|s| s.part.is_some() || s.label.is_some()) {
            Role::Group
        } else {
            Role::Generic
        };
        let role = seen.and_then(|s| s.role).unwrap_or(implied);
        let name = seen.and_then(|s| s.label).map(str::to_string).or_else(|| text.clone()).unwrap_or_default();
        let owner = match seen {
            Some(s) => s.owner,
            None => parent.map_or(0, |p| nodes[p].owner),
        };
        nodes.push(Node {
            pos,
            handle: hd,
            depth: r.depth,
            parent,
            kind,
            role,
            name,
            text,
            value,
            bounds,
            visible,
            fill,
            radius,
            text_color: tn.map(|t| Color(t.color)),
            font_px: tn.map(|t| t.size_px),
            states,
            routes,
            binds: seen.map(|s| s.binds.clone()).unwrap_or_default(),
            element: seen.map(|s| s.ty),
            elem,
            source: seen.and_then(|s| s.at),
            part: seen.and_then(|s| s.part),
            id: seen.and_then(|s| s.id),
            owner,
            foreign: (kind == Kind::Foreign).then(|| rt.foreign.slot_of(hd).map(|i| rt.foreign.slots[i].id)).flatten(),
            inner: seen.is_none() && parent.is_some_and(|p| nodes[p].inner || w.leaf_at(p)),
        });
        stack.push(pos);
    }
    // names from content: a button, an item, a labelled group or a link takes the text of its subtree
    let takes = |n: &Node, own_label: bool| !own_label && n.text.is_none() && matches!(n.role, Role::Button | Role::ListItem | Role::Link | Role::Checkbox | Role::Heading | Role::Tooltip | Role::Group);
    let labelled: Vec<bool> = nodes.iter().map(|n| w.at_rec.get(n.pos).is_some_and(|&i| i != u32::MAX && w.seen[i as usize].label.is_some())).collect();
    for i in 0..nodes.len() {
        let Some(t) = nodes[i].text.clone() else { continue };
        let mut p = nodes[i].parent;
        while let Some(a) = p {
            if takes(&nodes[a], labelled[a]) {
                let n = &mut nodes[a].name;
                if !n.is_empty() {
                    n.push(' ');
                }
                n.push_str(&t);
            }
            p = nodes[a].parent;
        }
    }
    // focus within
    if let Some(f) = focus.and_then(|f| nodes.iter().position(|n| n.handle == f)) {
        let mut p = Some(f);
        while let Some(a) = p {
            nodes[a].states.0 |= States::FOCUS_WITHIN.0;
            p = nodes[a].parent;
        }
    }
    let owners = w.owners.iter().map(|o| OwnerView { ty: o.ty, kind: o.kind.clone(), parent: o.parent, field: o.field.clone() }).collect();
    Tree { nodes, owners, lost: w.lost, scale: rt.scale }
}

impl<V: View> App<V> {
    /// The window's reflected tree, now (`tree.rs`): the records, joined with one walk of the description.
    pub fn tree(&self) -> Tree {
        build(&self.rt, crate::probe::walk_root(&self.rt, &*self.owner))
    }
}
