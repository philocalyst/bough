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


#[derive(Clone)]
struct DNode {
    style: u32,
    flags: u8,
    live: bool,
    root: bool,
    hgen: u32,
    nkids: u32,
    /// The node's layout children as a chain through the nodes themselves: its first and last child, and
    /// each child's next sibling (0: none; node 0 does not exist, handle index 0 is the core's "no handle").
    /// A child's `next` sits beside the style and flags a visit of that child reads, so walking a
    /// container's children loads no line the visit would not load anyway, and no container owns a heap
    /// list (round 6, lane `store`: no per-node heap objects).
    first: u32,
    last: u32,
    next: u32,
    touched: u32,
    layout: Layout,
    cache: NodeCache,
    /// Index of its text state in `Deep::ctxs` (`NOCTX`: none).
    ctx: u32,
}

const NOCTX: u32 = u32::MAX;

impl DNode {
    fn blank() -> DNode {
        DNode { style: 0, flags: 0, live: false, root: false, hgen: 0, nkids: 0, first: 0, last: 0, next: 0, touched: 0, layout: Layout::ZERO, cache: NodeCache::default(), ctx: NOCTX }
    }
}

pub struct Deep {
    n: bough_mem::Col<DNode>,
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
    ctxs: bough_mem::Col<LeafCtx>,
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
    /// Bytes `new` carves for `ids` nodes and `texts` text states.
    pub fn column_bytes(ids: usize, texts: usize) -> usize {
        let a = |n: usize| n.next_multiple_of(256) + 256;
        a(ids * size_of::<DNode>()) + a(texts * size_of::<LeafCtx>())
    }

    /// The engine over columns carved from `store` (type-stable: a node never moves).
    pub fn new(store: &mut bough_mem::Region, ids: usize, texts: usize) -> Deep {
        Deep {
            n: store.carve("layout nodes", ids),
            styles: StyleTable::new(),
            spec_ids: HashMap::default(),
            generation: 1,
            scheduler: Scheduler::new(),
            last_root: None,
            changed: Vec::new(),
            abs: HashMap::default(),
            ctxs: store.carve("text leaf states", texts),
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
    /// Intern a resolved style (reflection's style edits).
    pub fn intern_style(&mut self, s: Style) -> u32 {
        self.styles.intern(s)
    }
    /// The resolved style of node `n` (reflection).
    pub fn style(&self, n: u32) -> &Style {
        self.styles.get(self.n[n as usize].style)
    }
    pub fn style_of(&self, n: u32) -> u32 {
        self.n[n as usize].style
    }
    /// Set node `n`'s style before it was ever laid out (a mount's operand values): no invalidation.
    pub fn set_style_fresh(&mut self, n: u32, style: u32) {
        let measured = self.ctx_ref(n).is_some_and(|c| c.user.is_none());
        let keep = self.n[n as usize].flags & !META_MASK;
        self.n[n as usize].style = style;
        self.n[n as usize].flags = meta_bits(&self.styles, style, measured) | keep;
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
        while self.n.len() <= i as usize {
            self.n.push(DNode::blank());
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
        (nd.first, nd.last, nd.next) = (0, 0, 0);
        nd.touched = 0;
        nd.layout = Layout::ZERO;
        nd.cache.clear();
        let old = std::mem::replace(&mut nd.ctx, ci);
        if old != NOCTX {
            self.orphans.push(old);
        }
        i
    }

    /// Intern a resolved cephal `Style` (templates resolve their declarations once per distinct style).
    pub fn intern(&mut self, st: Style) -> u32 {
        self.styles.intern(st)
    }

    /// `add` with an interned style id instead of a `Spec` (layout templates, `ltemplate.rs`).
    pub fn add_style(&mut self, hd: Handle, style: u32, ctx: Option<LeafCtx>) -> u32 {
        let i = hd.idx;
        while self.n.len() <= i as usize {
            self.n.push(DNode::blank());
        }
        let flags = meta_bits(&self.styles, style, ctx.as_ref().is_some_and(|c| c.user.is_none()));
        let ci = match ctx {
            Some(c) => match self.ctx_free.pop() {
                Some(ci) => {
                    self.ctxs[ci as usize] = c;
                    ci
                }
                None => {
                    self.ctxs.push(c);
                    (self.ctxs.len() - 1) as u32
                }
            },
            None => NOCTX,
        };
        let nd = &mut self.n[i as usize];
        nd.style = style;
        nd.flags = flags;
        nd.live = true;
        nd.root = false;
        nd.hgen = hd.generation;
        nd.nkids = 0;
        (nd.first, nd.last, nd.next) = (0, 0, 0);
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

    /// Append `child` to `parent`'s children.
    #[inline]
    pub fn append(&mut self, parent: u32, child: u32) {
        let after = self.n[parent as usize].last;
        self.link_after(parent, after, child);
    }

    /// Link `child` into `parent`'s children right after `after` (0: as the first child).
    #[inline]
    pub fn link_after(&mut self, parent: u32, after: u32, child: u32) {
        let next = if after == 0 { std::mem::replace(&mut self.n[parent as usize].first, child) } else { std::mem::replace(&mut self.n[after as usize].next, child) };
        self.n[child as usize].next = next;
        let p = &mut self.n[parent as usize];
        p.nkids += 1;
        if next == 0 {
            p.last = child;
        }
    }

    /// Take the `count` children that follow `after` (0: from the first) out of `parent`'s chain, and
    /// invalidate `parent` (its algorithm reruns over its children at the next layout). The removed nodes
    /// keep their own `next` until they are reused (`add`).
    pub fn unlink_after(&mut self, b: &Bough, parent: u32, after: u32, count: usize) {
        let mut c = self.next_kid(parent, after);
        for _ in 0..count {
            c = self.n[c as usize].next;
        }
        if after == 0 {
            self.n[parent as usize].first = c;
        } else {
            self.n[after as usize].next = c;
        }
        let p = &mut self.n[parent as usize];
        p.nkids -= count as u32;
        if c == 0 {
            p.last = after;
        }
        self.invalidate(b, parent);
    }

    /// Number of children of container `n`.
    #[inline]
    pub fn kid_count(&self, n: u32) -> usize {
        self.n[n as usize].nkids as usize
    }
    /// The first child of `n` (0: none), its last child, and the sibling after child `c` (0: none).
    #[inline(always)]
    pub fn first_kid(&self, n: u32) -> u32 {
        self.n[n as usize].first
    }
    #[inline(always)]
    pub fn last_kid(&self, n: u32) -> u32 {
        self.n[n as usize].last
    }
    #[inline(always)]
    pub fn next_sib(&self, c: u32) -> u32 {
        self.n[c as usize].next
    }
    /// The child of `parent` that follows `after` (0: its first child).
    #[inline(always)]
    pub fn next_kid(&self, parent: u32, after: u32) -> u32 {
        if after == 0 { self.n[parent as usize].first } else { self.n[after as usize].next }
    }
    /// The children of `n` (mount order = flow order).
    #[inline]
    pub fn kids(&self, n: u32) -> KidIds<'_> {
        KidIds { n: &self.n, c: self.n[n as usize].first }
    }

    /// Give node `n` a text state after the fact (a label whose text left the label kernel's coverage
    /// becomes a shaped leaf); its meta bits follow.
    pub fn set_ctx(&mut self, n: u32, ctx: LeafCtx) {
        let measured = ctx.user.is_none();
        let ci = match self.ctx_free.pop() {
            Some(ci) => {
                self.ctxs[ci as usize] = ctx;
                ci
            }
            None => {
                self.ctxs.push(ctx);
                (self.ctxs.len() - 1) as u32
            }
        };
        let nd = &mut self.n[n as usize];
        let old = std::mem::replace(&mut nd.ctx, ci);
        nd.flags = meta_bits(&self.styles, nd.style, measured) | (nd.flags & !META_MASK);
        if old != NOCTX {
            self.orphans.push(old);
        }
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
            let mut k = nd.first;
            (nd.first, nd.last, nd.nkids) = (0, 0, 0);
            let c = std::mem::replace(&mut nd.ctx, NOCTX);
            if c != NOCTX {
                self.orphans.push(c);
            }
            if abs {
                self.abs.remove(&i);
            }
            while k != 0 {
                stack.push(k);
                k = self.n[k as usize].next;
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
            (nd.first, nd.last, nd.nkids) = (0, 0, 0);
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

    pub fn children_into(&self, _b: &Bough, n: u32, out: &mut Vec<u32>) {
        out.extend(self.kids(n));
    }

    /// The engine's tables as `(name, entries, bytes)` (`Rt::store_ledger`). A node's answer cache is one
    /// word until its first store, then a boxed block (counted at `size_of` of what the word points at when
    /// it is set).
    pub fn ledger(&self) -> Vec<(&'static str, usize, usize)> {
        use std::mem::size_of;
        // SAFETY: `NodeCache` is `Option<Box<_>>`: one word, null when nothing was stored (asserted).
        const _: () = assert!(size_of::<NodeCache>() == 8);
        let set = |c: &NodeCache| unsafe { *(c as *const NodeCache as *const usize) } != 0;
        // cephal's boxed `Inner` (three inline size entries and the final entry): 360 B (STORE-R6)
        let ncache = self.n.iter().filter(|x| set(&x.cache)).count();
        let caches = ncache * 360;
        let ctx_heap = |c: &LeafCtx| {
            c.run.gid.capacity() * 2 + c.run.x.capacity() * 4 + c.run.cluster.capacity() * 4 + c.text.capacity() + c.starts.capacity() * 4 + c.para.text.capacity() + c.para.words.capacity() * size_of::<bough_text::para::Word>()
        };
        vec![
            ("col lay.eng.n (DNode)", self.n.len(), self.n.len() * size_of::<DNode>()),
            ("lay.eng.n caches (boxed on first store)", ncache, caches),
            ("col lay.eng.ctxs (LeafCtx)", self.ctxs.len(), self.ctxs.len() * size_of::<LeafCtx>()),
            ("lay.eng.ctxs heap (runs, text, word tables)", self.ctxs.len(), self.ctxs.iter().map(ctx_heap).sum::<usize>()),
            ("lay.eng misc (changed, orphans, recycled, free)", 0, (self.changed.capacity() + self.orphans.capacity() + self.recycled.capacity() + self.ctx_free.capacity()) * 4),
        ]
    }

    /// Test support: a copy with no layout state, laid out cold.
    pub fn cold_rects(&self, b: &Bough, root: u32, avail: Size<AvailableSpace>) -> Vec<(u32, (f32, f32, f32, f32))> {
        let mut cold = Deep {
            n: {
                let mut c = bough_mem::Col::owned("cold layout nodes");
                for x in self.n.iter() {
                    c.push(DNode {
                    style: x.style,
                    flags: x.flags & META_MASK,
                    live: x.live,
                    root: x.root,
                    hgen: x.hgen,
                    nkids: x.nkids,
                    first: x.first,
                    last: x.last,
                    next: x.next,
                    touched: 0,
                    layout: Layout::ZERO,
                    cache: NodeCache::default(),
                    ctx: x.ctx,
                    });
                }
                c
            },
            styles: self.styles.clone(),
            spec_ids: HashMap::default(),
            generation: 1,
            scheduler: Scheduler::new(),
            last_root: None,
            changed: Vec::new(),
            abs: HashMap::default(),
            ctxs: {
                let mut c = bough_mem::Col::owned("cold text leaf states");
                for x in self.ctxs.iter() {
                    c.push(x.clone());
                }
                c
            },
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

// ------------------------------------------------------------------ structure over the Bough

/// Read-only structure queries.
struct DViewRef<'a> {
    d: &'a Deep,
    b: &'a Bough,
}

/// A container's children, by their sibling chain (`DNode::next`).
pub struct KidIds<'a> {
    n: &'a [DNode],
    c: u32,
}
impl Iterator for KidIds<'_> {
    type Item = u32;
    #[inline(always)]
    fn next(&mut self) -> Option<u32> {
        let c = self.c;
        if c == 0 {
            return None;
        }
        // SAFETY-free: a chain holds only ids of nodes that were added (`link_after`), all `< n.len()`.
        self.c = self.n[c as usize].next;
        Some(c)
    }
}
struct Kids<'a>(KidIds<'a>);
impl Iterator for Kids<'_> {
    type Item = NodeId;
    #[inline(always)]
    fn next(&mut self) -> Option<NodeId> {
        self.0.next().map(nid)
    }
}

impl<'a> DViewRef<'a> {
    #[inline]
    fn pos(&self, i: u32) -> usize {
        self.b.pos_of(Handle { idx: i, generation: self.d.n[i as usize].hgen }).expect("layout node is in the Bough")
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
        Kids(self.d.kids(ix(node) as u32))
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
