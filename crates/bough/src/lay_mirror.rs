//! Variant A: the mirror. bough's layout nodes are mirrored into cephal's own `Tree` (its node
//! columns, interned styles, per-node caches and scheduler); layout results are read back out by
//! node id and written through bough-core's write path by `layout.rs`.
use crate::layout::LeafCtx;
use cephal::tree::NodeId;
use cephal::{AvailableSpace, Size, Style, Tree};

pub struct Mirror {
    tree: Tree<LeafCtx>,
    /// Dense index (the low 32 bits of a cephal id) -> id (which carries the slot generation).
    ids: Vec<NodeId>,
    changed: Vec<u32>,
}

#[inline]
fn dense(id: NodeId) -> u32 {
    id.raw() as u32
}

impl Mirror {
    pub fn new() -> Mirror {
        let mut tree = Tree::with_capacity(256);
        tree.disable_rounding();
        Mirror { tree, ids: Vec::new(), changed: Vec::new() }
    }

    pub fn add(&mut self, style: &Style, ctx: Option<LeafCtx>) -> u32 {
        let id = match ctx {
            Some(c) => self.tree.new_leaf_with_context(style.clone(), c),
            None => self.tree.new_leaf(style.clone()),
        };
        let d = dense(id);
        if self.ids.len() <= d as usize {
            self.ids.resize(d as usize + 1, id);
        }
        self.ids[d as usize] = id;
        d
    }

    pub fn append(&mut self, parent: u32, child: u32) {
        self.tree.append_child(self.ids[parent as usize], self.ids[child as usize]);
    }

    pub fn ctx_mut(&mut self, n: u32) -> Option<&mut LeafCtx> {
        self.tree.context_mut(self.ids[n as usize])
    }

    pub fn invalidate(&mut self, n: u32) {
        self.tree.invalidate(self.ids[n as usize]);
    }

    pub fn remove_tree(&mut self, root: u32) {
        self.tree.remove(self.ids[root as usize]);
    }

    pub fn layout(&mut self, root: u32, avail: Size<AvailableSpace>) {
        self.tree.compute_layout_with_measure(self.ids[root as usize], avail, |ctx: &mut LeafCtx, input, _style: &Style| ctx.measure(input));
        self.changed.clear();
        for n in self.tree.changed() {
            self.changed.push(dense(*n));
        }
    }

    /// (x, y relative to the parent's border box, width, height), px.
    #[inline]
    pub fn rect(&self, n: u32) -> (f32, f32, f32, f32) {
        let l = self.tree.layout(self.ids[n as usize]);
        (l.location.x, l.location.y, l.size.width, l.size.height)
    }

    #[inline]
    pub fn changed_at(&self, i: usize) -> Option<u32> {
        self.changed.get(i).copied()
    }

    pub fn children_into(&self, n: u32, out: &mut Vec<u32>) {
        out.extend(self.tree.children(self.ids[n as usize]).map(dense));
    }

    /// Test support: the engine's results for the tree under `root` against a cold layout of a copy.
    pub fn cold_rects(&self, root: u32, avail: Size<AvailableSpace>) -> Vec<(u32, (f32, f32, f32, f32))> {
        let mut cold = self.tree.clone_cold();
        let r = self.ids[root as usize];
        cold.compute_layout_with_measure(r, avail, |ctx: &mut LeafCtx, input, _style: &Style| ctx.measure(input));
        let mut out = Vec::new();
        let mut stack = vec![r];
        while let Some(n) = stack.pop() {
            let l = cold.layout(n);
            out.push((dense(n), (l.location.x, l.location.y, l.size.width, l.size.height)));
            stack.extend(cold.children(n));
        }
        out.sort_by_key(|e| e.0);
        out
    }
}

impl Default for Mirror {
    fn default() -> Self {
        Self::new()
    }
}
