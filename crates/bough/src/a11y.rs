//! The accessibility projection: the reflected tree (`tree.rs`) as assistive technology reads it. It is the
//! same snapshot a test and the inspector read, pruned to what has meaning (roles, names, values, states,
//! actions), so it cannot drift from what is painted and hit-tested.
//!
//! * [`Tree::semantic`] is the projection; [`Semantic::delta`] the nodes that differ from an earlier one
//!   (added or changed nodes and removed ids: what a platform update carries).
//! * Actions come back through the routes real input takes: [`App::a11y_act`] (press, focus, set a text
//!   field's value, scroll into view), by node id with its handle's generation checked.
//! * With the cargo feature `a11y`, [`Semantic::to_accesskit`] builds an AccessKit `TreeUpdate`.
//!
//! Nothing here runs unless something asks: no tree is kept, no adapter exists until one is made. A
//! foreign element is a node of role `Foreign` whose accessible content is the native view's own (the
//! platform host hangs that view's accessibility element there).
use crate::app::{App, View};
use crate::input::{Key, Mods};
use crate::probe::Role;
use crate::tree::{Node, States, Tree};
use bough_core::Handle;

/// A node's id: its record handle (index and generation), so it survives re-descriptions and moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub u64);
impl NodeId {
    fn of(n: &Node) -> NodeId {
        if n.handle.idx != 0 { NodeId((n.handle.idx as u64) << 32 | n.handle.generation as u64) } else { NodeId(1 << 63 | n.pos as u64) }
    }
    /// The record handle this id names (none for a record without one).
    pub fn handle(self) -> Option<Handle> {
        (self.0 >> 63 == 0).then(|| Handle { idx: (self.0 >> 32) as u32, generation: self.0 as u32 })
    }
}

/// What assistive technology may ask of a node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum A11yAction {
    /// Activate (its click route, as a pointer press and release at its centre would).
    Click,
    Focus,
    /// Replace a text field's content.
    SetValue,
    /// Scroll its scroll area so that it shows.
    ScrollIntoView,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SemNode {
    pub id: NodeId,
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
    pub role: Role,
    pub name: String,
    pub value: Option<String>,
    /// Where it is, relative to its parent's origin (a scroll area's children: to its content's origin), and
    /// its size, device px. Relative, so that a move of a container or a scroll changes one node: the
    /// container's offset, the scroll area's `scroll`.
    pub offset: [f32; 2],
    pub size: [f32; 2],
    /// A scroll area's content offset (device px; how far its content is scrolled).
    pub scroll: [f32; 2],
    pub states: States,
    pub actions: Vec<A11yAction>,
    pub foreign: Option<u64>,
}

/// The projection of one tree: nodes in paint order, the first is the root.
#[derive(Clone, Debug, PartialEq)]
pub struct Semantic {
    pub nodes: Vec<SemNode>,
    pub focus: Option<NodeId>,
}

/// What changed between two projections.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Delta {
    /// New or changed nodes (a parent whose children changed is changed).
    pub changed: Vec<NodeId>,
    pub removed: Vec<NodeId>,
    pub focus_changed: bool,
}

fn meaningful(n: &Node) -> bool {
    n.role != Role::Generic || n.states.has(States::FOCUSABLE) || n.id.is_some()
}

impl Tree {
    /// The accessibility projection (module docs). The root record is always a node.
    pub fn semantic(&self) -> Semantic {
        let mut nodes: Vec<SemNode> = Vec::new();
        // per tree node: the semantic node it maps to (itself if kept, else its nearest kept ancestor)
        let mut map: Vec<Option<usize>> = vec![None; self.nodes.len()];
        // absolute origins of the kept nodes (device px)
        let mut origins: Vec<[f32; 2]> = Vec::new();
        for n in &self.nodes {
            let up = n.parent.and_then(|p| map[p]);
            // a text whose meaningful ancestor took its name from content is that name, not a node
            let folded = n.role == Role::Text && up.is_some_and(|u| matches!(nodes[u].role, Role::Button | Role::Link | Role::Checkbox | Role::Heading | Role::Tooltip | Role::TextInput));
            let keep = n.pos == 0 || (meaningful(n) && !folded);
            if !keep {
                map[n.pos] = up;
                continue;
            }
            let id = NodeId::of(n);
            let mut actions = Vec::new();
            if n.routes.iter().any(|r| matches!(r.0, crate::probe::Verb::Click | crate::probe::Verb::Press)) {
                actions.push(A11yAction::Click);
            }
            if n.states.has(States::FOCUSABLE) {
                actions.push(A11yAction::Focus);
            }
            if n.role == Role::TextInput {
                actions.push(A11yAction::SetValue);
            }
            if n.states.has(States::HIDDEN) && up.is_some() {
                actions.push(A11yAction::ScrollIntoView);
            }
            let abs = n.bounds.or(n.visible.map(|[x0, y0, x1, y1]| [x0 as f32, y0 as f32, (x1 - x0) as f32, (y1 - y0) as f32])).unwrap_or([0.0; 4]);
            let mut scroll = [0.0f32; 2];
            if n.role == Role::ScrollArea {
                // the content: the first laid-out record under the port
                if let Some(c) = self.nodes[n.pos + 1..].iter().take_while(|c| c.depth > n.depth).find(|c| c.bounds.is_some()) {
                    let cb = c.bounds.unwrap();
                    scroll = [abs[0] - cb[0], abs[1] - cb[1]];
                }
            }
            let origin = up.map_or([0.0, 0.0], |u| {
                let p = &origins[u];
                [p[0] - nodes[u].scroll[0], p[1] - nodes[u].scroll[1]]
            });
            let parent = up.map(|u| nodes[u].id);
            if let Some(u) = up {
                nodes[u].children.push(id);
            }
            origins.push([abs[0], abs[1]]);
            nodes.push(SemNode {
                id,
                parent,
                children: Vec::new(),
                role: n.role,
                name: n.name.clone(),
                value: n.value.clone(),
                offset: [abs[0] - origin[0], abs[1] - origin[1]],
                size: [abs[2], abs[3]],
                scroll,
                states: n.states,
                actions,
                foreign: n.foreign,
            });
            map[n.pos] = Some(nodes.len() - 1);
        }
        let focus = self.nodes.iter().find(|n| n.states.has(States::FOCUSED)).and_then(|n| map[n.pos]).map(|i| nodes[i].id);
        Semantic { nodes, focus }
    }
}

impl Semantic {
    pub fn get(&self, id: NodeId) -> Option<&SemNode> {
        self.nodes.iter().find(|n| n.id == id)
    }
    /// Node `id`'s absolute bounds `[x0, y0, x1, y1]` (device px): its offsets summed up the parents.
    pub fn bounds(&self, id: NodeId) -> Option<[f32; 4]> {
        let n = self.get(id)?;
        let (mut x, mut y) = (n.offset[0], n.offset[1]);
        let mut p = n.parent;
        while let Some(pid) = p {
            let q = self.get(pid)?;
            (x, y) = (x + q.offset[0] - q.scroll[0], y + q.offset[1] - q.scroll[1]);
            p = q.parent;
        }
        Some([x, y, x + n.size[0], y + n.size[1]])
    }
    /// The nodes that are new or differ from `prev`'s, and the ids `prev` had that this one does not.
    pub fn delta(&self, prev: &Semantic) -> Delta {
        let old: std::collections::HashMap<NodeId, &SemNode> = prev.nodes.iter().map(|n| (n.id, n)).collect();
        let now: std::collections::HashSet<NodeId> = self.nodes.iter().map(|n| n.id).collect();
        Delta {
            changed: self.nodes.iter().filter(|n| old.get(&n.id).is_none_or(|o| *o != *n)).map(|n| n.id).collect(),
            removed: prev.nodes.iter().filter(|n| !now.contains(&n.id)).map(|n| n.id).collect(),
            focus_changed: self.focus != prev.focus,
        }
    }
    /// Apply `d` (taken from `next.delta(self)`) to this projection: the result equals `next`. What a platform
    /// tree that receives only deltas holds.
    pub fn apply(&mut self, d: &Delta, next: &Semantic) {
        self.nodes.retain(|n| !d.removed.contains(&n.id));
        for id in &d.changed {
            let n = next.get(*id).expect("a changed node is in the newer projection").clone();
            match self.nodes.iter_mut().find(|m| m.id == *id) {
                Some(m) => *m = n,
                None => self.nodes.push(n),
            }
        }
        // the platform's tree is a set keyed by id; order it as the newer projection does to compare
        let order: std::collections::HashMap<NodeId, usize> = next.nodes.iter().enumerate().map(|(i, n)| (n.id, i)).collect();
        self.nodes.sort_by_key(|n| order.get(&n.id).copied().unwrap_or(usize::MAX));
        self.focus = next.focus;
    }

    /// An AccessKit tree update carrying every node (`full`) or only `delta`'s.
    #[cfg(feature = "a11y")]
    pub fn to_accesskit(&self, delta: Option<&Delta>) -> accesskit::TreeUpdate {
        use accesskit as ak;
        let role = |r: Role| match r {
            Role::Generic => ak::Role::GenericContainer,
            Role::Group => ak::Role::Group,
            Role::Text => ak::Role::Label,
            Role::Button => ak::Role::Button,
            Role::TextInput => ak::Role::TextInput,
            Role::Image => ak::Role::Image,
            Role::ScrollArea => ak::Role::ScrollView,
            Role::List => ak::Role::List,
            Role::ListItem => ak::Role::ListItem,
            Role::Foreign => ak::Role::GenericContainer,
            Role::Checkbox => ak::Role::CheckBox,
            Role::Link => ak::Role::Link,
            Role::Heading => ak::Role::Heading,
            Role::Tooltip => ak::Role::Tooltip,
        };
        let build = |n: &SemNode| {
            let mut a = ak::Node::new(role(n.role));
            if !n.name.is_empty() {
                a.set_label(n.name.clone());
            }
            if let Some(v) = &n.value {
                a.set_value(v.clone());
            }
            // bounds in the node's own space, placed by a translation in its parent's (a scroll area's
            // children additionally by its scroll offset): a move or a scroll is one node's update
            a.set_bounds(ak::Rect { x0: 0.0, y0: 0.0, x1: n.size[0] as f64, y1: n.size[1] as f64 });
            let ps = n.parent.and_then(|p| self.get(p)).map_or([0.0, 0.0], |p| p.scroll);
            a.set_transform(ak::Affine::translate(((n.offset[0] - ps[0]) as f64, (n.offset[1] - ps[1]) as f64)));
            if n.scroll != [0.0, 0.0] {
                a.set_scroll_x(n.scroll[0] as f64);
                a.set_scroll_y(n.scroll[1] as f64);
            }
            a.set_children(n.children.iter().map(|c| ak::NodeId(c.0)).collect::<Vec<_>>());
            for act in &n.actions {
                a.add_action(match act {
                    A11yAction::Click => ak::Action::Click,
                    A11yAction::Focus => ak::Action::Focus,
                    A11yAction::SetValue => ak::Action::SetValue,
                    A11yAction::ScrollIntoView => ak::Action::ScrollIntoView,
                });
            }
            (ak::NodeId(n.id.0), a)
        };
        let nodes = match delta {
            None => self.nodes.iter().map(build).collect(),
            // AccessKit does not apply a scroll offset to children: a scrolled area's children are updated too
            Some(d) => {
                let mut ids: Vec<NodeId> = d.changed.clone();
                for id in &d.changed {
                    if let Some(n) = self.get(*id).filter(|n| n.role == Role::ScrollArea) {
                        ids.extend(n.children.iter().filter(|c| !d.changed.contains(c)));
                    }
                }
                ids.iter().filter_map(|id| self.get(*id)).map(build).collect()
            }
        };
        let root = ak::NodeId(self.nodes[0].id.0);
        ak::TreeUpdate { nodes, tree: delta.is_none().then(|| ak::TreeInfo::new(root)), tree_id: ak::TreeId::ROOT, focus: self.focus.map_or(root, |f| ak::NodeId(f.0)) }
    }
}

impl<V: View> App<V> {
    /// The accessibility projection now.
    pub fn semantic(&self) -> Semantic {
        self.tree().semantic()
    }

    /// An assistive technology's action on node `id`, through the routes real input takes. `false` if the
    /// node is gone (its handle's generation no longer matches) or cannot take the action.
    pub fn a11y_act(&mut self, id: NodeId, action: A11yAction, value: Option<&str>) -> bool {
        let Some(h) = id.handle() else { return false };
        let t = self.tree();
        let Some(n) = t.of_handle(h) else { return false };
        let (pos, center) = (n.pos, n.center_pt(t.scale));
        match action {
            A11yAction::Click => match center.and_then(|_| self.point_of_pos(&t, pos)) {
                Some((x, y)) => {
                    for turn in [crate::find::Turn::Move { x, y }, crate::find::Turn::Down { x, y, button: crate::input::Button::Primary }, crate::find::Turn::Up { x, y, button: crate::input::Button::Primary }] {
                        self.apply(&turn);
                    }
                    true
                }
                None => false,
            },
            A11yAction::Focus => {
                if !n.states.has(States::FOCUSABLE) {
                    return false;
                }
                self.set_focus(Some(h), true);
                true
            }
            A11yAction::SetValue => {
                if n.role != Role::TextInput {
                    return false;
                }
                let old = n.value.clone().unwrap_or_default();
                self.set_focus(Some(h), true);
                self.key_down(Key::End, Mods::default());
                for _ in old.chars() {
                    self.key_down(Key::Backspace, Mods::default());
                }
                self.text_input(value.unwrap_or(""));
                true
            }
            A11yAction::ScrollIntoView => self.scroll_into_view(&t, pos),
        }
    }

    /// The point a pointer acts on record `pos` at, if nothing unrelated covers it (`find::point_of`).
    fn point_of_pos(&self, t: &Tree, pos: usize) -> Option<(f32, f32)> {
        let (x, y) = t.nodes[pos].center_pt(t.scale)?;
        let s = self.rt.scale;
        let top = self.rt.bough.hit_top((x * s).floor() as i32, (y * s).floor() as i32)?;
        (t.contains(pos, top) || t.contains(top, pos)).then_some((x, y))
    }

    /// Scroll the nearest scroll area around record `pos` so that its layout box shows (the wheel's entry
    /// point, by the distance to the port's edge).
    fn scroll_into_view(&mut self, t: &Tree, pos: usize) -> bool {
        let Some(b) = t.nodes[pos].bounds else { return false };
        let mut p = t.nodes[pos].parent;
        while let Some(a) = p {
            let n = &t.nodes[a];
            if n.role == Role::ScrollArea {
                let (Some([_, py, _, ph]), Some((x, y))) = (n.bounds, n.center_pt(t.scale)) else { return false };
                let s = t.scale;
                let dy = if b[1] < py {
                    b[1] - py
                } else if b[1] + b[3] > py + ph {
                    b[1] + b[3] - (py + ph)
                } else {
                    0.0
                };
                if dy != 0.0 {
                    self.apply(&crate::find::Turn::Wheel { x, y, dy: dy / s });
                }
                return true;
            }
            p = n.parent;
        }
        false
    }
}
