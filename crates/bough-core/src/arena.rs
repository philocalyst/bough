//! `ArenaTree`: a faithful (lean) model of zgx's `Scene` storage and damage, as the baseline.
//!
//! Modelled from zgx `docs/architecture.md` ("Retained fibers and layout", "Damage and
//! composition") and `crates/zgx/src/scene.rs`:
//!
//! * nodes in an arena of slots, `NodeId(slot, generation)`, parent id + `Vec<NodeId>` children,
//!   absolute f32 bounds (moving a subtree rewrites every descendant), a lazily computed cached
//!   subtree ink (`Cell`, invalidated up the ancestor chain until an already-invalid ancestor),
//! * `invalidate` -> dirty flags + a deduplicated dirty queue; `flush` turns paint-dirty nodes
//!   into damage via an ancestor clip walk,
//! * damage: outward-quantised rect list that merges overlapping rects (one scan per join), a
//!   4-entry "recent" containment fast path, and collapse to the viewport above 64 rects
//!   (the task's rule; zgx itself spills to a cell grid),
//! * paint walk culls whole subtrees whose cached ink misses the damage; hit test walks forward
//!   collecting hits (last wins) with the same subtree-ink prune, like `hit_test_walk`.
//!
//! The real zgx `Node` is far larger (style, measurement caches, effects...); this node keeps only
//! what these operations touch, so its memory and cache footprint are a *lower bound* of zgx's.

use std::cell::Cell;
use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }
    #[inline]
    pub fn intersects(&self, o: &Rect) -> bool {
        self.x < o.x + o.w && o.x < self.x + self.w && self.y < o.y + o.h && o.y < self.y + self.h
    }
    #[inline]
    pub fn intersection(&self, o: &Rect) -> Option<Rect> {
        let x0 = self.x.max(o.x);
        let y0 = self.y.max(o.y);
        let x1 = (self.x + self.w).min(o.x + o.w);
        let y1 = (self.y + self.h).min(o.y + o.h);
        (x0 < x1 && y0 < y1).then(|| Rect::new(x0, y0, x1 - x0, y1 - y0))
    }
    #[inline]
    pub fn union(&self, o: &Rect) -> Rect {
        let x0 = self.x.min(o.x);
        let y0 = self.y.min(o.y);
        let x1 = (self.x + self.w).max(o.x + o.w);
        let y1 = (self.y + self.h).max(o.y + o.h);
        Rect::new(x0, y0, x1 - x0, y1 - y0)
    }
    #[inline]
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }
    #[inline]
    pub fn translated(&self, dx: f32, dy: f32) -> Rect {
        Rect::new(self.x + dx, self.y + dy, self.w, self.h)
    }
    /// Round outward to whole pixels.
    #[inline]
    pub fn quantized(&self) -> Rect {
        let x0 = self.x.floor();
        let y0 = self.y.floor();
        let x1 = (self.x + self.w).ceil();
        let y1 = (self.y + self.h).ceil();
        Rect::new(x0, y0, x1 - x0, y1 - y0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NodeId {
    pub slot: u32,
    pub generation: u32,
}

const PAINT: u8 = 1;
const BOUNDS_VALID: u8 = 1;
pub const MAX_RECTS: usize = 64;
const HISTORY: usize = 8;

struct Node {
    parent: Option<NodeId>,
    children: Vec<NodeId>,
    bounds: Rect,
    /// Cached subtree ink (everything this subtree can paint).
    ink: Cell<Option<Rect>>,
    bounds_valid: Cell<u8>,
    dirty: u8,
    queued: bool,
    paints: bool,
    clips: bool,
    payload: u32,
}

struct Slot {
    generation: u32,
    node: Option<Node>,
}

/// Merging rect list with a "recent" fast path (zgx `add_damage_as`).
#[derive(Clone, Default)]
pub struct DamageList {
    pub rects: Vec<Rect>,
    recent: [[f32; 4]; 4],
}

impl DamageList {
    fn edges(r: &Rect) -> [f32; 4] {
        [r.x, r.y, r.x + r.w, r.y + r.h]
    }
    pub fn clear(&mut self) {
        self.rects.clear();
        self.recent = [[0.0; 4]; 4];
    }
    pub fn add(&mut self, rect: Rect, viewport: &Rect) {
        let rect = rect.quantized();
        let Some(mut rect) = rect.intersection(viewport) else { return };
        let e = Self::edges(&rect);
        // a recent rectangle already holds it
        if self.recent.iter().any(|o| o[0] <= e[0] && o[1] <= e[1] && o[2] >= e[2] && o[3] >= e[3]) {
            return;
        }
        self.recent.rotate_right(1);
        self.recent[0] = e;
        while !self.rects.is_empty() {
            let (x, y) = (rect.x, rect.y);
            let (r, b) = (rect.x + rect.w, rect.y + rect.h);
            let meets = move |o: &Rect| x < o.x + o.w && o.x < r && y < o.y + o.h && o.y < b;
            let Some(i) = self.rects.iter().position(meets) else { break };
            rect = self.rects.swap_remove(i).union(&rect);
        }
        self.rects.push(rect);
        if self.rects.len() > MAX_RECTS {
            self.rects.clear();
            self.rects.push(*viewport);
        }
    }
}

pub struct ArenaTree {
    slots: Vec<Slot>,
    free: Vec<u32>,
    root: Option<NodeId>,
    dirty: Vec<NodeId>,
    damage: DamageList,
    history: VecDeque<DamageList>,
    viewport: Rect,
    stack: Vec<NodeId>,
    scratch: DamageList,
    live: usize,
}

impl ArenaTree {
    pub fn new(w: f32, h: f32) -> Self {
        ArenaTree {
            slots: Vec::new(),
            free: Vec::new(),
            root: None,
            dirty: Vec::new(),
            damage: DamageList::default(),
            history: VecDeque::new(),
            viewport: Rect::new(0.0, 0.0, w, h),
            stack: Vec::new(),
            scratch: DamageList::default(),
            live: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.live
    }
    pub fn is_empty(&self) -> bool {
        self.live == 0
    }
    pub fn root(&self) -> Option<NodeId> {
        self.root
    }

    #[inline(always)]
    fn node(&self, id: NodeId) -> &Node {
        let s = &self.slots[id.slot as usize];
        debug_assert_eq!(s.generation, id.generation, "stale NodeId");
        s.node.as_ref().expect("stale NodeId")
    }
    #[inline(always)]
    fn node_mut(&mut self, id: NodeId) -> &mut Node {
        let s = &mut self.slots[id.slot as usize];
        debug_assert_eq!(s.generation, id.generation, "stale NodeId");
        s.node.as_mut().expect("stale NodeId")
    }
    pub fn contains(&self, id: NodeId) -> bool {
        self.slots.get(id.slot as usize).is_some_and(|s| s.generation == id.generation && s.node.is_some())
    }

    fn alloc(&mut self, node: Node) -> NodeId {
        self.live += 1;
        if let Some(slot) = self.free.pop() {
            let s = &mut self.slots[slot as usize];
            s.node = Some(node);
            NodeId { slot, generation: s.generation }
        } else {
            let slot = self.slots.len() as u32;
            self.slots.push(Slot { generation: 0, node: Some(node) });
            NodeId { slot, generation: 0 }
        }
    }

    /// Add a node as `parent`'s child at `index` (`None` = append). Marks it paint-dirty.
    pub fn add(&mut self, parent: Option<NodeId>, index: Option<usize>, bounds: Rect, paints: bool, clips: bool, payload: u32) -> NodeId {
        let id = self.alloc(Node {
            parent,
            children: Vec::new(),
            bounds,
            ink: Cell::new(None),
            bounds_valid: Cell::new(0),
            dirty: 0,
            queued: false,
            paints,
            clips,
            payload,
        });
        match parent {
            Some(p) => {
                let pn = self.node_mut(p);
                match index {
                    Some(i) => pn.children.insert(i, id),
                    None => pn.children.push(id),
                }
                self.invalidate_ancestors(p);
            }
            None => self.root = Some(id),
        }
        self.invalidate(id, PAINT);
        id
    }

    /// Clear cached ink up the chain, stopping at the first ancestor that was already invalid.
    #[inline]
    fn invalidate_ancestors(&self, from: NodeId) {
        let mut cur = Some(from);
        while let Some(a) = cur {
            let n = self.node(a);
            if n.bounds_valid.replace(0) & BOUNDS_VALID == 0 {
                break;
            }
            cur = n.parent;
        }
    }

    #[inline]
    fn invalidate(&mut self, id: NodeId, flags: u8) {
        self.invalidate_ancestors(id);
        let n = self.node_mut(id);
        n.dirty |= flags;
        if !n.queued {
            n.queued = true;
            self.dirty.push(id);
        }
    }

    /// A paint-only write (e.g. a colour change).
    pub fn mark_paint(&mut self, id: NodeId) {
        self.invalidate(id, PAINT);
    }

    /// A layout write of one node's bounds (zgx: old visible bounds damaged now, the node queued
    /// so `flush` damages the new ones; ancestor ink caches invalidated).
    pub fn set_bounds(&mut self, id: NodeId, r: Rect) {
        if self.node(id).paints {
            self.damage_node(id);
        }
        let n = self.node_mut(id);
        n.bounds = r;
        n.bounds_valid.set(0);
        self.invalidate(id, PAINT);
    }

    /// Visible bounds: own bounds clipped by every clipping ancestor (zgx `visible_bounds` walk).
    fn visible(&self, id: NodeId) -> Option<Rect> {
        let n = self.node(id);
        let mut r = n.bounds;
        let mut cur = n.parent;
        while let Some(p) = cur {
            let pn = self.node(p);
            if pn.clips {
                r = r.intersection(&pn.bounds)?;
            }
            cur = pn.parent;
        }
        Some(r)
    }

    fn damage_node(&mut self, id: NodeId) {
        if let Some(r) = self.visible(id) {
            let vp = self.viewport;
            self.damage.add(r, &vp);
        }
    }

    /// Turn queued paint changes into damage and clear their flags (zgx `flush`).
    pub fn flush(&mut self) {
        if self.dirty.is_empty() {
            return;
        }
        let dirty = std::mem::take(&mut self.dirty);
        for &id in &dirty {
            if !self.contains(id) {
                continue;
            }
            let flags = self.node(id).dirty;
            if flags & PAINT != 0 {
                self.damage_node(id);
            }
            let n = self.node_mut(id);
            n.dirty = 0;
            n.queued = false;
        }
        self.dirty = dirty;
        self.dirty.clear();
    }

    /// End the frame: flush, then retire this frame's damage into the history ring.
    pub fn end_frame(&mut self) {
        self.flush();
        let mut done = std::mem::take(&mut self.damage);
        if self.history.len() == HISTORY {
            let mut old = self.history.pop_front().unwrap();
            old.clear();
            self.damage = old;
        }
        self.history.push_back(std::mem::take(&mut done));
    }

    /// Damage a buffer of `age` frames needs: the pending frame plus the last `age - 1` retired ones.
    pub fn repair(&mut self, age: usize) -> &[Rect] {
        self.flush();
        let vp = self.viewport;
        self.scratch.clear();
        for r in &self.damage.rects {
            self.scratch.add(*r, &vp);
        }
        for h in self.history.iter().rev().take(age.saturating_sub(1)) {
            for r in &h.rects {
                self.scratch.add(*r, &vp);
            }
        }
        &self.scratch.rects
    }

    /// `repair(age)` then `cull` against it.
    pub fn cull_repair(&mut self, age: usize, out: &mut Vec<u32>) {
        self.repair(age);
        let sc = std::mem::take(&mut self.scratch);
        self.cull(&sc.rects, out);
        self.scratch = sc;
    }

    pub fn pending_damage(&self) -> &[Rect] {
        &self.damage.rects
    }

    // -------------------------------------------------------------------------- ink / queries

    /// Cached union of everything this subtree paints.
    fn ink(&self, id: NodeId) -> Option<Rect> {
        let n = self.node(id);
        if n.bounds_valid.get() & BOUNDS_VALID != 0 {
            return n.ink.get();
        }
        let mut acc = if n.paints { Some(n.bounds) } else { None };
        for &c in &n.children {
            if let Some(ci) = self.ink(c) {
                acc = Some(match acc {
                    Some(a) => a.union(&ci),
                    None => ci,
                });
            }
        }
        // a clipping node's children can't paint outside it
        if n.clips && let Some(a) = acc {
            acc = a.intersection(&n.bounds);
        }
        n.ink.set(acc);
        n.bounds_valid.set(BOUNDS_VALID);
        acc
    }

    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        self.node(id).parent
    }

    /// The node after `id`'s subtree in paint order (what a preorder `subtree_end` denotes):
    /// find `id` among its siblings, else climb.
    pub fn after_subtree(&self, id: NodeId) -> Option<NodeId> {
        let mut cur = id;
        while let Some(p) = self.node(cur).parent {
            let sibs = &self.node(p).children;
            let i = sibs.iter().position(|&c| c == cur).unwrap();
            if let Some(&n) = sibs.get(i + 1) {
                return Some(n);
            }
            cur = p;
        }
        None
    }

    pub fn children(&self, id: NodeId) -> &[NodeId] {
        &self.node(id).children
    }

    /// Payloads, in paint order, of painting nodes intersecting any region rect; whole subtrees
    /// whose cached ink misses the region are skipped.
    pub fn cull(&mut self, region: &[Rect], out: &mut Vec<u32>) {
        out.clear();
        let Some(root) = self.root else { return };
        let mut stack = std::mem::take(&mut self.stack);
        stack.clear();
        stack.push(root);
        while let Some(id) = stack.pop() {
            let Some(ink) = self.ink(id) else { continue };
            if !region.iter().any(|r| r.intersects(&ink)) {
                continue;
            }
            let n = self.node(id);
            if n.paints && region.iter().any(|r| r.intersects(&n.bounds)) {
                out.push(n.payload);
            }
            for &c in n.children.iter().rev() {
                stack.push(c);
            }
        }
        self.stack = stack;
    }

    /// zgx-style hit test: forward walk collecting hits, last one wins.
    pub fn hit(&self, x: f32, y: f32) -> Option<NodeId> {
        let root = self.root?;
        let mut best = None;
        let mut stack: Vec<NodeId> = vec![root];
        while let Some(id) = stack.pop() {
            let Some(ink) = self.ink(id) else { continue };
            if !ink.contains(x, y) {
                continue;
            }
            let n = self.node(id);
            if n.paints && n.bounds.contains(x, y) {
                best = Some(id);
            }
            for &c in n.children.iter().rev() {
                stack.push(c);
            }
        }
        best
    }

    /// The better algorithm (reverse order, early exit) for an honest upper bound on the baseline.
    pub fn hit_rev(&self, x: f32, y: f32) -> Option<NodeId> {
        fn go(t: &ArenaTree, id: NodeId, x: f32, y: f32) -> Option<NodeId> {
            let ink = t.ink(id)?;
            if !ink.contains(x, y) {
                return None;
            }
            let n = t.node(id);
            for &c in n.children.iter().rev() {
                if let Some(h) = go(t, c, x, y) {
                    return Some(h);
                }
            }
            (n.paints && n.bounds.contains(x, y)).then_some(id)
        }
        go(self, self.root?, x, y)
    }

    // -------------------------------------------------------------------------- structure edits

    /// Move every node of the subtree by (dx, dy): absolute bounds are rewritten per node, ink
    /// caches invalidated, and old and new visible bounds of each painting node damaged.
    pub fn translate_subtree(&mut self, id: NodeId, dx: f32, dy: f32) {
        let clip_old = self.ancestor_clip(id);
        let clip_new = clip_old;
        self.invalidate_ancestors(id);
        self.translate_rec(id, dx, dy, clip_old, clip_new);
    }

    fn ancestor_clip(&self, id: NodeId) -> Option<Rect> {
        let mut clip: Option<Rect> = Some(self.viewport);
        let mut cur = self.node(id).parent;
        while let Some(p) = cur {
            let pn = self.node(p);
            if pn.clips {
                clip = clip.and_then(|c| c.intersection(&pn.bounds));
            }
            cur = pn.parent;
        }
        clip
    }

    fn translate_rec(&mut self, id: NodeId, dx: f32, dy: f32, clip_old: Option<Rect>, clip_new: Option<Rect>) {
        let vp = self.viewport;
        let (old, new, paints, clips) = {
            let n = self.node_mut(id);
            let old = n.bounds;
            n.bounds = old.translated(dx, dy);
            n.bounds_valid.set(0);
            (old, n.bounds, n.paints, n.clips)
        };
        if paints {
            if let Some(c) = clip_old && let Some(r) = old.intersection(&c) {
                self.damage.add(r, &vp);
            }
            if let Some(c) = clip_new && let Some(r) = new.intersection(&c) {
                self.damage.add(r, &vp);
            }
        }
        let (co, cn) = if clips {
            (clip_old.and_then(|c| c.intersection(&old)), clip_new.and_then(|c| c.intersection(&new)))
        } else {
            (clip_old, clip_new)
        };
        let nchildren = self.node(id).children.len();
        for i in 0..nchildren {
            let c = self.node(id).children[i];
            self.translate_rec(c, dx, dy, co, cn);
        }
    }

    /// Remove the subtree: damage its former ink, detach, free every slot (generation bump).
    pub fn remove_subtree(&mut self, id: NodeId) {
        if let Some(ink) = self.ink(id) {
            let clip = self.ancestor_clip(id);
            if let Some(r) = clip.and_then(|c| ink.intersection(&c)) {
                let vp = self.viewport;
                self.damage.add(r, &vp);
            }
        }
        if let Some(p) = self.node(id).parent {
            let pn = self.node_mut(p);
            if let Some(i) = pn.children.iter().position(|&c| c == id) {
                pn.children.remove(i);
            }
            self.invalidate_ancestors(p);
        } else {
            self.root = None;
        }
        let mut stack = std::mem::take(&mut self.stack);
        stack.clear();
        stack.push(id);
        while let Some(n) = stack.pop() {
            let s = &mut self.slots[n.slot as usize];
            let node = s.node.take().expect("live node");
            s.generation = s.generation.wrapping_add(1);
            self.free.push(n.slot);
            self.live -= 1;
            stack.extend_from_slice(&node.children);
        }
        self.stack = stack;
    }

    // -------------------------------------------------------------------------- memory

    pub fn node_bytes() -> usize {
        std::mem::size_of::<Slot>()
    }

    /// Heap bytes: slot array, free list, per-node children Vec capacity, dirty queue.
    pub fn heap_bytes(&self) -> usize {
        let mut b = self.slots.capacity() * std::mem::size_of::<Slot>() + self.free.capacity() * 4 + self.dirty.capacity() * 8;
        for s in &self.slots {
            if let Some(n) = &s.node {
                b += n.children.capacity() * std::mem::size_of::<NodeId>();
            }
        }
        b + (self.history.len() + 2) * MAX_RECTS * std::mem::size_of::<Rect>()
    }
}
