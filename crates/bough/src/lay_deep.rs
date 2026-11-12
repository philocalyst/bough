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
use cephal::compute::memo::{Memo, ShapeTable, MAX_SHAPE_CHILDREN, NO_SHAPE, SHAPE_UNSET};
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
    /// The node's layout children: `nkids` element ids in flow order, a span of `Deep::spans` of
    /// `1 << kclass` words at `kids` (0: none yet). An array, not a chain through the nodes: a container's
    /// children are read by independent loads from contiguous words (measured at 4,096 children, per child:
    /// chain 3.4 ns in mount order and 6.6 ns after churn, array 1.2 and 1.0), and no container owns a heap
    /// object (the spans are words of one arena that never moves).
    kids: u32,
    kclass: u8,
    touched: u32,
    layout: Layout,
    cache: NodeCache,
    /// Index of its text state in `Deep::ctxs` (`NOCTX`: none).
    ctx: u32,
    /// Its layout parent (`NOCTX`: a group root or detached).
    parent: u32,
    /// Its subtree's shape for cephal's shape memo (`SHAPE_UNSET` until asked; see `Deep::shape_at`).
    shape: u32,
}

const NOCTX: u32 = u32::MAX;

impl DNode {
    fn blank() -> DNode {
        DNode { style: 0, flags: 0, live: false, root: false, hgen: 0, nkids: 0, kids: 0, kclass: 0, touched: 0, layout: Layout::ZERO, cache: NodeCache::default(), ctx: NOCTX, parent: NOCTX, shape: SHAPE_UNSET }
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
    /// The containers' children arrays (`DNode::kids`).
    spans: crate::store::Spans,
    /// Interned subtree shapes and the answers equal shapes share (cephal's `compute::memo`). Styles
    /// are never released here (`spec_ids` keeps them), so a style id always means the same style.
    shapes: ShapeTable,
    pub memo: Memo,
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
    /// The engine's columns as memory-ledger lines: nodes (style index, layout, answer cache pointer, kids span),
    /// the boxed answer caches, the text leaves' states (shaped run, text, wraps, paragraph halves), the
    /// interned styles and the maps.
    pub fn ledger(&self, l: &mut bough_mem::Ledger) {
        use bough_mem::Scales::Content;
        const G: &str = "layout";
        // nodes, text states and child arrays are columns carved from the façade's store block (lane store)
        l.col(G, &self.n, Content);
        l.col(G, self.spans.column(), Content);
        // cephal's cache box is private: its size is measured once through the counting allocator (when
        // the binary installs it; otherwise only the count is reported). Spill boxes (> 3 entries) are not
        // counted here (they fall in the ledger's residual).
        let caches = self.n.iter().filter(|d| !d.cache.is_empty()).count();
        // a clone of a live box allocates exactly what the box holds (Inner, plus a spill box if it has one)
        let per = match self.n.iter().find(|d| !d.cache.is_empty()) {
            Some(d) if bough_mem::count::installed() => {
                let before = bough_mem::count::live();
                let c = d.cache.clone();
                let b = bough_mem::count::live().saturating_sub(before);
                drop(c);
                b
            }
            _ => 0,
        };
        let cb = caches * per;
        l.many(G, format!("layout answer caches ({caches} boxed, {per} B each)"), Content, cb, cb);
        l.col(G, &self.ctxs, Content);
        let (mut run, mut text, mut hot, mut cold) = (0usize, 0usize, 0usize, 0usize);
        for c in self.ctxs.iter() {
            run += c.run.gid.capacity() * 2 + c.run.x.capacity() * 4 + c.run.cluster.capacity() * 4;
            text += c.text.capacity() + c.starts.capacity() * 4;
            let (h, k) = c.para.heap_bytes();
            hot += h;
            cold += k;
        }
        l.many(G, "text leaf shaped runs (gid, x, cluster)", Content, run, run);
        l.many(G, "text leaf text + line starts", Content, text, text);
        l.many(G, "paragraph hot halves (text, words, wrap)", Content, hot, hot);
        l.many(G, "paragraph cold halves", Content, cold, cold);
        let maps = self.spec_ids.capacity() * (size_of::<(SpecKey, u32)>() + 1) + self.abs.capacity() * (size_of::<(u32, AbsoluteContext)>() + 1);
        l.many(G, "spec intern map + out-of-flow map", Content, maps, maps);
        let misc = (self.changed.capacity() + self.orphans.capacity() + self.recycled.capacity() + self.ctx_free.capacity()) * 4;
        l.many(G, "engine queues", Content, misc, misc);
        let st = self.styles.len() * (size_of::<Style>() + 24);
        l.many(G, format!("interned styles ({})", self.styles.len()), Content, st, st);
    }
    /// Bytes `new` carves for `ids` nodes and `texts` text states.
    pub fn column_bytes(ids: usize, texts: usize) -> usize {
        let a = |n: usize| n.next_multiple_of(256) + 256;
        a(ids * size_of::<DNode>()) + a(texts * size_of::<LeafCtx>()) + a(ids * 4 * 4)
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
            spans: crate::store::Spans::new(store.carve("layout children", ids * 4)),
            orphans: Vec::new(),
            recycled: Vec::new(),
            shapes: ShapeTable::default(),
            memo: Memo::new(),
        }
    }

    /// The shape of node `i`'s subtree for the shape memo, interned on first use: its style and its
    /// children's shapes. No shape (its answers are not shared): measured text, an out-of-flow box,
    /// a box that generates none, a grid, more than `MAX_SHAPE_CHILDREN` children, or any of these
    /// below it. A one-line label is a sized leaf: its style holds its size.
    fn shape_at(&mut self, i: u32) -> u32 {
        let s = self.n[i as usize].shape;
        if s != SHAPE_UNSET {
            return s;
        }
        let nd = &self.n[i as usize];
        let st = self.styles.get(nd.style);
        let measured = nd.ctx != NOCTX && self.ctxs[nd.ctx as usize].user.is_none();
        let n = nd.nkids as usize;
        let mut ok = !measured
            && !st.position.is_out_of_flow()
            && !st.generates_no_box()
            && st.display != Display::Grid
            && (n == 0 || matches!(st.display, Display::Flex | Display::Block | Display::FlowRoot))
            && n <= MAX_SHAPE_CHILDREN;
        if ok {
            for k in 0..n {
                let c = self.spans.words(self.n[i as usize].kids, n)[k];
                ok &= self.shape_at(c) != NO_SHAPE;
            }
        }
        let shape = if ok {
            let style = self.n[i as usize].style;
            let key = &mut self.shapes.key;
            key.clear();
            key.push(style);
            key.push(n as u32);
            let nd = &self.n[i as usize];
            for &c in self.spans.words(nd.kids, n) {
                key.push(self.n[c as usize].shape);
            }
            self.shapes.intern_key()
        } else {
            NO_SHAPE
        };
        self.n[i as usize].shape = shape;
        shape
    }

    /// Node `i`'s style, children or content changed: its shape and every ancestor's are unknown.
    #[inline]
    fn reset_shapes_up(&mut self, mut i: u32) {
        loop {
            let nd = &mut self.n[i as usize];
            nd.shape = SHAPE_UNSET;
            if nd.parent == NOCTX {
                return;
            }
            i = nd.parent;
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
        self.reset_shapes_up(n);
    }

    /// Restyle node `n` (a re-description changed its spec) and invalidate it.
    pub fn set_style(&mut self, b: &Bough, n: u32, style: u32) {
        let measured = self.ctx_ref(n).is_some_and(|c| c.user.is_none());
        let keep = self.n[n as usize].flags & !META_MASK;
        self.n[n as usize].style = style;
        self.n[n as usize].flags = meta_bits(&self.styles, style, measured) | keep;
        self.n[n as usize].cache.clear();
        // (the negative control keeps the old shape: lay_diff must fail)
        #[cfg(not(feature = "cephal-mutant-memo-shape"))]
        self.reset_shapes_up(n);
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
        let (ko, kc) = (std::mem::take(&mut nd.kids), nd.kclass);
        nd.touched = 0;
        nd.layout = Layout::ZERO;
        nd.cache.clear();
        nd.parent = NOCTX;
        nd.shape = SHAPE_UNSET;
        let old = std::mem::replace(&mut nd.ctx, ci);
        if old != NOCTX {
            self.orphans.push(old);
        }
        self.spans.free(ko, kc);
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
        let (ko, kc) = (std::mem::take(&mut nd.kids), nd.kclass);
        nd.touched = 0;
        nd.layout = Layout::ZERO;
        nd.cache.clear();
        nd.parent = NOCTX;
        nd.shape = SHAPE_UNSET;
        let old = std::mem::replace(&mut nd.ctx, ci);
        if old != NOCTX {
            self.orphans.push(old);
        }
        self.spans.free(ko, kc);
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
        let p = &mut self.n[parent as usize];
        let n = p.nkids as usize;
        if p.kids == 0 {
            // four children to start with (a row of a few cells never grows)
            (p.kids, p.kclass) = (self.spans.alloc(2), 2);
        } else if n == 1usize << p.kclass {
            self.grow_kids(parent);
        }
        let p = &mut self.n[parent as usize];
        self.spans.words_mut(p.kids, n + 1)[n] = child;
        p.nkids += 1;
        self.n[child as usize].parent = parent;
        self.reset_shapes_up(parent);
    }
    /// Double the children span of `parent` (its children move to a span of the next class; the old one
    /// goes back to its class's list).
    #[cold]
    fn grow_kids(&mut self, parent: u32) {
        let p = &self.n[parent as usize];
        let (old, class, n) = (p.kids, p.kclass, p.nkids as usize);
        let new = self.spans.alloc(class + 1);
        // one arena: copy through a split at the higher span (the two never overlap)
        for i in 0..n {
            let v = self.spans.words(old, n)[i];
            self.spans.words_mut(new, n)[i] = v;
        }
        self.spans.free(old, class);
        let p = &mut self.n[parent as usize];
        (p.kids, p.kclass) = (new, class + 1);
    }

    /// Number of children of container `n`.
    #[inline]
    pub fn kid_count(&self, n: u32) -> usize {
        self.n[n as usize].nkids as usize
    }
    /// The children of `n` (mount order = flow order).
    #[inline(always)]
    pub fn kids(&self, n: u32) -> &[u32] {
        let nd = &self.n[n as usize];
        self.spans.words(nd.kids, nd.nkids as usize)
    }
    /// Structural edit of container `n`'s children (a removed item's boxes): `kids[at..at+del]` are taken
    /// out; the container is invalidated (its algorithm reruns over its children at the next layout).
    pub fn splice_kids(&mut self, b: &Bough, n: u32, at: usize, del: usize) {
        let nd = &mut self.n[n as usize];
        let len = nd.nkids as usize;
        self.spans.words_mut(nd.kids, len).copy_within(at + del..len, at);
        nd.nkids = (len - del) as u32;
        self.reset_shapes_up(n);
        self.invalidate(b, n);
    }
    /// Rotate the last `k` children of `n` to index `at` (items mounted at the end, then placed).
    pub fn place_tail_kids(&mut self, n: u32, at: usize, k: usize) {
        let nd = &self.n[n as usize];
        let len = nd.nkids as usize;
        if k > 0 && at + k < len {
            self.spans.words_mut(nd.kids, len)[at..].rotate_right(k);
            self.reset_shapes_up(n);
        }
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
        self.reset_shapes_up(n);
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
            let (ko, kc, kn) = (std::mem::take(&mut nd.kids), nd.kclass, std::mem::take(&mut nd.nkids) as usize);
            let c = std::mem::replace(&mut nd.ctx, NOCTX);
            if c != NOCTX {
                self.orphans.push(c);
            }
            if abs {
                self.abs.remove(&i);
            }
            stack.extend_from_slice(self.spans.words(ko, kn));
            self.spans.free(ko, kc);
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
            let (ko, kc) = (std::mem::take(&mut nd.kids), nd.kclass);
            nd.nkids = 0;
            self.spans.free(ko, kc);
            if !self.abs.is_empty() {
                self.abs.remove(&i);
            }
        }
    }

    pub fn layout(&mut self, b: &Bough, root: u32, avail: Size<AvailableSpace>) {
        self.memo.trim();
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
        out.extend_from_slice(self.kids(n));
    }

    /// The engine's tables as `(name, entries, bytes)` (`Rt::store_ledger`). A node's answer cache is one
    /// word until its first store, then a boxed block (counted at `size_of` of what the word points at when
    /// it is set).
    pub fn store_ledger(&self) -> Vec<(&'static str, usize, usize)> {
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
            ("col lay.eng children (id arrays)", self.spans.touched(), self.spans.touched() * 4),
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
                    kids: x.kids,
                    kclass: x.kclass,
                    touched: 0,
                    layout: Layout::ZERO,
                    cache: NodeCache::default(),
                    ctx: x.ctx,
                    parent: x.parent,
                    shape: x.shape,
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
            spans: self.spans.clone_owned(),
            shapes: self.shapes.clone(),
            // the reference a cold layout gives: computed without the memo
            memo: {
                let mut m = Memo::new();
                m.enabled = false;
                m
            },
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

struct Kids<'a>(std::slice::Iter<'a, u32>);
impl Iterator for Kids<'_> {
    type Item = NodeId;
    #[inline(always)]
    fn next(&mut self) -> Option<NodeId> {
        self.0.next().map(|&i| nid(i))
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
        Kids(self.d.kids(ix(node) as u32).iter())
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
    #[inline]
    fn shape(&mut self, node: NodeId) -> u32 {
        self.d.shape_at(ix(node) as u32)
    }
    #[inline]
    fn memo(&mut self) -> Option<&mut Memo> {
        Some(&mut self.d.memo)
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
        let nd = &self.d.n[ix(node)];
        if nd.root {
            return None;
        }
        // the layout parent is kept at `append` (a group's structure is the Bough's depth column, which
        // the debug build checks)
        debug_assert_eq!(Some(nd.parent), self.b.parent(self.r().pos(ix(node) as u32)).map(|p| self.b.record(p).handle), "layout parent column");
        (nd.parent != NOCTX).then(|| nid(nd.parent))
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
