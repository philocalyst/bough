//! cephal's `LayoutTree` / `CacheAccess` / `Incremental` implemented directly over bough's storage (LAYOUT.md
//! "Variant B, deep-kids": the one layout path; the mirror into cephal's own `Tree` is gone). There is no
//! second tree:
//!
//! * a layout node **is** a Bough record (painted ones are the quad / text records; paint-less boxes get an
//!   empty-ink `REC_BOX` record) and its cephal `NodeId` is the record's handle index;
//! * structure comes from the depth column: `Incremental::parent` / `depth` are `Bough::parent` /
//!   `depth_at`; a container's child ids are kept in the side column as they are appended at mount (the
//!   structure does not change while a group lives), so `children` never scans siblings;
//! * **absolute positioning at any depth**: each container's out-of-flow record (`AbsoluteContext`) is kept
//!   in a side map, so an absolutely positioned box finds its containing block through any number of
//!   static ancestors (cephal does not keep placements here: every out-of-flow pass re-places);
//! * styles are interned `cephal::owned::StyleTable` entries shared between all nodes with the same `Spec`
//!   (one table lookup per node at mount, no `Style` is built or hashed after the first);
//! * the answer caches (`NodeCache`: one word until the first store) and the layouts sit in side columns
//!   indexed by handle index; cheap leaves (paint-only blocks) never store a cache entry
//!   (`leaf_is_cheap`), measured text leaves and containers do.
//!
//! Nothing is mirrored and nothing is marked dirty at mount: a freshly mounted group has empty caches, and
//! the first `layout` computes it cold.
use crate::layout::{FxBuild, LeafCtx, Spec, SpecKey};
use bough_text::pool::GlyphPool;
use bough_core::{Bough, Handle};
use cephal::compute::block::BlockContext;
use cephal::compute::plan::StylePlan;
use cephal::owned::StyleTable;
use cephal::schedule::{Incremental, Scheduler, META_MASK};
use cephal::style::Display;
use cephal::tree::{AbsoluteContext, CacheAccess, Layout, LayoutInput, LayoutOutput, LayoutTree, MeasureInput, MeasureOutput, NodeCache, NodeId, NodeKind, QueryMeta};
use cephal::{AvailableSpace, Size, Style};
use std::collections::HashMap;


struct DNode {
    style: u32,
    flags: u8,
    live: bool,
    root: bool,
    hgen: u32,
    nkids: u32,
    kids: Vec<u32>,
    touched: u32,
    layout: Layout,
    cache: NodeCache,
    /// Index of its text state in `Deep::ctxs` (`NOCTX`: none).
    ctx: u32,
}

const NOCTX: u32 = u32::MAX;

impl DNode {
    fn blank() -> DNode {
        DNode { style: 0, flags: 0, live: false, root: false, hgen: 0, nkids: 0, kids: Vec::new(), touched: 0, layout: Layout::ZERO, cache: NodeCache::default(), ctx: NOCTX }
    }
}

pub struct Deep {
    n: Vec<DNode>,
    styles: StyleTable,
    spec_ids: HashMap<SpecKey, u32, FxBuild>,
    generation: u32,
    scheduler: Scheduler,
    last_root: Option<(u32, Size<AvailableSpace>)>,
    changed: Vec<u32>,
    /// Out-of-flow records of containers (by node), see the module docs.
    abs: HashMap<u32, AbsoluteContext, FxBuild>,
    /// Text states of removed leaves, waiting for their glyph blocks to be released (`Rt::reclaim`), and
    /// released ones kept for reuse (their buffers' capacity: a mount after a teardown allocates nothing).
    pub orphans: Vec<u32>,
    pub recycled: Vec<u32>,
    /// Text states of measured / sized text leaves, by index (`DNode::ctx`), and the free indices.
    ctxs: Vec<LeafCtx>,
    ctx_free: Vec<u32>,
}

#[inline(always)]
fn nid(i: u32) -> NodeId {
    NodeId::new(i as u64)
}
#[inline(always)]
fn ix(n: NodeId) -> usize {
    n.raw() as usize
}

fn meta_bits(styles: &StyleTable, style: u32, measured: bool) -> u8 {
    let st = styles.get(style);
    let dep = styles.parent_size_dependency(style);
    let not_cheap = measured || st.display == Display::Grid;
    (st.generates_no_box() as u8) << 4 | (dep.width as u8) << 5 | (dep.height as u8) << 6 | (not_cheap as u8) << 7
}

impl Deep {
    pub fn new() -> Deep {
        Deep {
            n: Vec::new(),
            styles: StyleTable::new(),
            spec_ids: HashMap::default(),
            generation: 1,
            scheduler: Scheduler::new(),
            last_root: None,
            changed: Vec::new(),
            abs: HashMap::default(),
            ctxs: Vec::new(),
            ctx_free: Vec::new(),
            orphans: Vec::new(),
            recycled: Vec::new(),
        }
    }

    /// A text leaf state: a recycled one (buffers kept) or a new one.
    pub fn fresh_ctx(&mut self, size_px: f32, lh_px: f32, wrap: bool) -> LeafCtx {
        match self.recycled.pop() {
            Some(c) => {
                self.ctx_free.push(c);
                std::mem::take(&mut self.ctxs[c as usize]).reset(size_px, lh_px, wrap)
            }
            None => LeafCtx::new(size_px, lh_px, wrap),
        }
    }

    /// The interned style of `spec` (one hash of the spec's words; the `Style` is built once per spec).
    pub fn style_id(&mut self, spec: &Spec, scale: f32) -> u32 {
        match self.spec_ids.get(&SpecKey(*spec)) {
            Some(&id) => id,
            None => {
                let id = self.styles.intern(spec.to_style(scale));
                self.spec_ids.insert(SpecKey(*spec), id);
                id
            }
        }
    }
    pub fn style_of(&self, n: u32) -> u32 {
        self.n[n as usize].style
    }
    /// Restyle node `n` (a re-description changed its spec) and invalidate it.
    pub fn set_style(&mut self, b: &Bough, n: u32, style: u32) {
        let measured = self.ctx_ref(n).is_some_and(|c| c.user.is_none());
        let keep = self.n[n as usize].flags & !META_MASK;
        self.n[n as usize].style = style;
        self.n[n as usize].flags = meta_bits(&self.styles, style, measured) | keep;
        self.n[n as usize].cache.clear();
        self.invalidate(b, n);
    }

    pub fn add(&mut self, hd: Handle, spec: &Spec, scale: f32, ctx: Option<LeafCtx>) -> u32 {
        let i = hd.idx;
        if self.n.len() <= i as usize {
            self.n.resize_with(i as usize + 1, DNode::blank);
        }
        let style = self.style_id(spec, scale);
        let flags = meta_bits(&self.styles, style, ctx.as_ref().is_some_and(|c| c.user.is_none()));
        let ci = match ctx {
            Some(c) => {
                let ci = match self.ctx_free.pop() {
                    Some(ci) => {
                        self.ctxs[ci as usize] = c;
                        ci
                    }
                    None => {
                        self.ctxs.push(c);
                        (self.ctxs.len() - 1) as u32
                    }
                };
                ci
            }
            None => NOCTX,
        };
        let nd = &mut self.n[i as usize];
        nd.style = style;
        nd.flags = flags;
        nd.live = true;
        nd.root = false;
        nd.hgen = hd.generation;
        nd.nkids = 0;
        nd.kids.clear();
        nd.touched = 0;
        nd.layout = Layout::ZERO;
        nd.cache.clear();
        let old = std::mem::replace(&mut nd.ctx, ci);
        if old != NOCTX {
            self.orphans.push(old);
        }
        i
    }

    #[inline]
    fn ctx_ref(&self, n: u32) -> Option<&LeafCtx> {
        let c = self.n[n as usize].ctx;
        (c != NOCTX).then(|| &self.ctxs[c as usize])
    }

    /// Release the glyph block of a removed leaf's text state and keep the state for reuse (`Rt::reclaim`).
    pub fn reclaim_orphan(&mut self, pool: &mut GlyphPool) -> bool {
        let Some(c) = self.orphans.pop() else { return false };
        self.ctxs[c as usize].release(pool);
        self.recycled.push(c);
        true
    }

    pub fn mark_root(&mut self, n: u32) {
        self.n[n as usize].root = true;
    }

    pub fn append(&mut self, parent: u32, child: u32) {
        let p = &mut self.n[parent as usize];
        p.nkids += 1;
        p.kids.push(child);
    }

    /// Number of children of container `n`.
    pub fn kid_count(&self, n: u32) -> usize {
        self.n[n as usize].kids.len()
    }
    /// The children of `n` (mount order = flow order).
    pub fn kids(&self, n: u32) -> &[u32] {
        &self.n[n as usize].kids
    }
    /// Structural edit of container `n`'s children (a list region's rows): `kids[at..at+del]` becomes `ins`;
    /// the container is invalidated (its algorithm reruns over its children at the next layout).
    pub fn splice_kids(&mut self, b: &Bough, n: u32, at: usize, del: usize, ins: &[u32]) {
        {
            let nd = &mut self.n[n as usize];
            nd.kids.splice(at..at + del, ins.iter().copied());
            nd.nkids = nd.kids.len() as u32;
        }
        self.invalidate(b, n);
    }
    /// Rotate the last `k` children of `n` to index `at` (rows mounted at the end, then placed).
    pub fn place_tail_kids(&mut self, n: u32, at: usize, k: usize) {
        let kids = &mut self.n[n as usize].kids;
        let len = kids.len();
        if k > 0 && at + k < len {
            kids[at..].rotate_right(k);
        }
    }
    /// Overwrite `kids[at..at+new.len()]` (a permutation of them) and invalidate `n`.
    pub fn permute_kids(&mut self, b: &Bough, n: u32, at: usize, new: &[u32]) {
        self.n[n as usize].kids[at..at + new.len()].copy_from_slice(new);
        self.invalidate(b, n);
    }

    pub fn ctx_mut(&mut self, n: u32) -> Option<&mut LeafCtx> {
        let c = self.n[n as usize].ctx;
        if c == NOCTX { None } else { Some(&mut self.ctxs[c as usize]) }
    }

    pub fn invalidate(&mut self, b: &Bough, n: u32) {
        let mut sched = std::mem::take(&mut self.scheduler);
        sched.mark(&mut DView { d: self, b, speculation: 0, observed: 0 }, nid(n));
        self.scheduler = sched;
    }

    /// Remove the whole tree at `root` in one pass over its kids lists (no per-node allocation, no
    /// lookups): every node is marked dead and clean, its kids list emptied (capacity kept), its text state
    /// moved to the orphans (glyph blocks released later, `Rt::reclaim`). `dead` receives the node ids.
    pub fn remove_group(&mut self, root: u32, stack: &mut Vec<u32>, dead: &mut Vec<u32>) {
        stack.clear();
        stack.push(root);
        let abs = !self.abs.is_empty();
        while let Some(i) = stack.pop() {
            dead.push(i);
            let nd = &mut self.n[i as usize];
            nd.live = false;
            nd.flags &= META_MASK;
            nd.cache.clear();
            stack.extend_from_slice(&nd.kids);
            nd.kids.clear();
            nd.nkids = 0;
            let c = std::mem::replace(&mut nd.ctx, NOCTX);
            if c != NOCTX {
                self.orphans.push(c);
            }
            if abs {
                self.abs.remove(&i);
            }
        }
    }

    pub fn remove_tree(&mut self, _b: &Bough, _root: u32, all: &[u32]) {
        for &i in all {
            let nd = &mut self.n[i as usize];
            nd.live = false;
            // a queued entry of a removed node pops as clean (see `Incremental::depth`)
            nd.flags &= META_MASK;
            nd.cache.clear();
            let c = std::mem::replace(&mut nd.ctx, NOCTX);
            if c != NOCTX {
                self.orphans.push(c);
            }
            nd.kids.clear();
            nd.nkids = 0;
            if !self.abs.is_empty() {
                self.abs.remove(&i);
            }
        }
    }

    pub fn layout(&mut self, b: &Bough, root: u32, avail: Size<AvailableSpace>) {
        self.generation = self.generation.wrapping_add(1).max(1);
        self.changed.clear();
        let root_query_changed = self.last_root != Some((root, avail));
        let mut sched = std::mem::take(&mut self.scheduler);
        {
            let mut v = DView { d: self, b, speculation: 0, observed: 0 };
            sched.run(&mut v, nid(root), root_query_changed);
            if root_query_changed {
                v.d.n[root as usize].cache.clear();
            }
            let icb = sched.take_initial_containing_block();
            cephal::compute::compute_root_layout_incremental(&mut v, nid(root), avail, icb);
        }
        self.scheduler = sched;
        self.last_root = Some((root, avail));
    }

    #[inline]
    pub fn rect(&self, n: u32) -> (f32, f32, f32, f32) {
        let l = &self.n[n as usize].layout;
        (l.location.x, l.location.y, l.size.width, l.size.height)
    }

    #[inline]
    pub fn changed_at(&self, i: usize) -> Option<u32> {
        self.changed.get(i).copied()
    }

    pub fn children_into(&self, b: &Bough, n: u32, out: &mut Vec<u32>) {
        let v = DViewRef { d: self, b };
        out.extend(v.kids(n).map(|c| c.raw() as u32));
    }

    /// Test support: a copy with no layout state, laid out cold.
    pub fn cold_rects(&self, b: &Bough, root: u32, avail: Size<AvailableSpace>) -> Vec<(u32, (f32, f32, f32, f32))> {
        let mut cold = Deep {
            n: self
                .n
                .iter()
                .map(|x| DNode {
                    style: x.style,
                    flags: x.flags & META_MASK,
                    live: x.live,
                    root: x.root,
                    hgen: x.hgen,
                    nkids: x.nkids,
                    kids: x.kids.clone(),
                    touched: 0,
                    layout: Layout::ZERO,
                    cache: NodeCache::default(),
                    ctx: x.ctx,
                })
                .collect(),
            styles: self.styles.clone(),
            spec_ids: HashMap::default(),
            generation: 1,
            scheduler: Scheduler::new(),
            last_root: None,
            changed: Vec::new(),
            abs: HashMap::default(),
            ctxs: self.ctxs.clone(),
            ctx_free: Vec::new(),
            orphans: Vec::new(),
            recycled: Vec::new(),
        };
        cold.layout(b, root, avail);
        let mut out = Vec::new();
        let mut stack = vec![root];
        while let Some(n) = stack.pop() {
            out.push((n, cold.rect(n)));
            cold.children_into(b, n, &mut stack);
        }
        out.sort_by_key(|e| e.0);
        out
    }
}

impl Default for Deep {
    fn default() -> Self {
        Self::new()
    }
}

// ------------------------------------------------------------------ structure over the Bough

/// Read-only structure queries.
struct DViewRef<'a> {
    d: &'a Deep,
    b: &'a Bough,
}

struct Kids<'a>(std::slice::Iter<'a, u32>);
impl Iterator for Kids<'_> {
    type Item = NodeId;
    #[inline]
    fn next(&mut self) -> Option<NodeId> {
        self.0.next().map(|&i| nid(i))
    }
}

impl<'a> DViewRef<'a> {
    #[inline]
    fn pos(&self, i: u32) -> usize {
        self.b.pos_of(Handle { idx: i, generation: self.d.n[i as usize].hgen }).expect("layout node is in the Bough")
    }
    fn kids(&self, i: u32) -> Kids<'a> {
        Kids(self.d.n[i as usize].kids.iter())
    }
}

// ------------------------------------------------------------------ the trait view

struct DView<'a> {
    d: &'a mut Deep,
    b: &'a Bough,
    speculation: u32,
    observed: u8,
}

impl DView<'_> {
    #[inline]
    fn r(&self) -> DViewRef<'_> {
        DViewRef { d: self.d, b: self.b }
    }
}

#[inline(never)]
fn measured_leaf(v: &mut DView<'_>, node: NodeId, inputs: LayoutInput) -> LayoutOutput {
    cephal::compute::leaf::compute_leaf_layout(v, node, inputs)
}

impl LayoutTree for DView<'_> {
    type ChildIter<'b>
        = Kids<'b>
    where
        Self: 'b;

    #[inline]
    fn children(&self, node: NodeId) -> Kids<'_> {
        Kids(self.d.n[ix(node)].kids.iter())
    }
    #[inline]
    fn child_count(&self, node: NodeId) -> usize {
        self.d.n[ix(node)].nkids as usize
    }
    fn child_at(&self, node: NodeId, index: usize) -> NodeId {
        self.children(node).nth(index).expect("child index in range")
    }
    #[inline]
    fn style(&self, node: NodeId) -> &Style {
        self.d.styles.get(self.d.n[ix(node)].style)
    }
    #[inline(always)]
    fn query_meta(&self, node: NodeId) -> QueryMeta {
        let nd = &self.d.n[ix(node)];
        let f = nd.flags;
        let leaf = nd.nkids == 0;
        QueryMeta {
            hidden: f & 0x10 != 0,
            cheap_leaf: f & 0x80 == 0 && leaf,
            dependency: Size { width: f & 0x20 != 0, height: f & 0x40 != 0 },
            kind: if leaf { NodeKind::LeafIgnoringHeight } else { self.d.styles.kind(nd.style) },
        }
    }
    #[inline]
    fn style_plan(&self, node: NodeId) -> Option<&StylePlan> {
        self.d.styles.plan(self.d.n[ix(node)].style)
    }
    #[inline]
    fn leaf_is_cheap(&self, node: NodeId) -> bool {
        !self.is_measured_leaf(node)
    }
    /// A paragraph is measured; a one-line label is a sized leaf (its style holds its size).
    #[inline]
    fn is_measured_leaf(&self, node: NodeId) -> bool {
        self.d.ctx_ref(ix(node) as u32).is_some_and(|c| c.user.is_none())
    }
    #[inline(always)]
    fn compute_leaf(&mut self, node: NodeId, inputs: LayoutInput, _: Option<&mut BlockContext<'_>>) -> LayoutOutput {
        if !self.is_measured_leaf(node) {
            return cephal::compute::leaf::compute_empty_leaf_layout(self, node, inputs);
        }
        measured_leaf(self, node, inputs)
    }
    fn measure(&mut self, node: NodeId, input: MeasureInput) -> MeasureOutput {
        match self.d.ctx_mut(ix(node) as u32) {
            Some(c) => c.measure(input),
            None => MeasureOutput::from_size(input.known_dimensions.unwrap_or(Size::ZERO)),
        }
    }
    fn set_unrounded_layout(&mut self, node: NodeId, layout: &Layout) {
        if self.speculation > 0 {
            return;
        }
        let hgen = self.d.generation;
        let i = ix(node);
        let nd = &mut self.d.n[i];
        if nd.touched != hgen && nd.layout != *layout {
            nd.touched = hgen;
            self.d.changed.push(i as u32);
        }
        // written even when equal: equal layouts may differ in the sign of a zero
        self.d.n[i].layout = *layout;
    }
    fn unrounded_layout(&self, node: NodeId) -> Option<Layout> {
        Some(self.d.n[ix(node)].layout)
    }
    fn compute_child_layout(&mut self, node: NodeId, inputs: LayoutInput, block_ctx: Option<&mut BlockContext<'_>>) -> LayoutOutput {
        cephal::compute::compute_child_layout(self, node, inputs, block_ctx)
    }
    #[inline]
    fn forwards_child_layout(&self) -> bool {
        true
    }
    #[inline]
    fn take_observed(&mut self) -> u8 {
        std::mem::take(&mut self.observed)
    }
    #[inline]
    fn note_observed(&mut self, axes: u8) {
        self.observed |= axes;
    }
    #[inline]
    fn measure_ignores_available_height(&self) -> bool {
        true
    }
    fn set_absolute_context(&mut self, node: NodeId, context: AbsoluteContext) {
        if self.speculation > 0 {
            return;
        }
        self.d.abs.insert(ix(node) as u32, context);
    }
    #[inline]
    fn clear_absolute_context(&mut self, node: NodeId) {
        if self.speculation > 0 || self.d.abs.is_empty() {
            return;
        }
        self.d.abs.remove(&(ix(node) as u32));
    }
    fn absolute_context(&self, node: NodeId) -> Option<&AbsoluteContext> {
        self.d.abs.get(&(ix(node) as u32))
    }
    fn take_absolute_context(&mut self, node: NodeId) -> Option<AbsoluteContext> {
        self.d.abs.remove(&(ix(node) as u32))
    }
}

impl CacheAccess for DView<'_> {
    #[inline]
    fn cache(&self, node: NodeId) -> &NodeCache {
        &self.d.n[ix(node)].cache
    }
    #[inline]
    fn cache_mut(&mut self, node: NodeId) -> &mut NodeCache {
        &mut self.d.n[ix(node)].cache
    }
    #[inline]
    fn generation(&self) -> u32 {
        self.d.generation
    }
    #[inline]
    fn speculation(&self) -> u32 {
        self.speculation
    }
    #[inline]
    fn set_speculation(&mut self, depth: u32) {
        self.speculation = depth;
    }
}

impl Incremental for DView<'_> {
    fn parent(&self, node: NodeId) -> Option<NodeId> {
        let i = ix(node) as u32;
        if self.d.n[i as usize].root {
            return None;
        }
        let r = self.r();
        let p = self.b.parent(r.pos(i))?;
        Some(nid(self.b.record(p).handle))
    }
    fn depth(&self, node: NodeId) -> u32 {
        // a node removed while queued (a list row) answers depth 0: the scheduler re-files it there and
        // pops it clean (its flags were cleared by `remove_tree`)
        if !self.d.n[ix(node)].live {
            return 0;
        }
        let r = self.r();
        self.b.depth_at(r.pos(ix(node) as u32)) as u32
    }
    #[inline]
    fn flags(&self, node: NodeId) -> u8 {
        self.d.n[ix(node)].flags
    }
    #[inline]
    fn set_flags(&mut self, node: NodeId, flags: u8) {
        self.d.n[ix(node)].flags = flags;
    }
}
