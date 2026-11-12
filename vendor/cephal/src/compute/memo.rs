//! Answers shared between identical subtrees (bough lane `layout`, round 7).
//!
//! A subtree's answer to a question is a function of its **shape** (the styles of its nodes, its
//! structure, and the content of its leaves) and of the question, nothing else, as long as nothing
//! outside it is involved: no out-of-flow box in it (its containing block keeps its placement), no
//! box that generates none, no measured content (the tree measures it), and a question asked
//! outside any shared block formatting context. A tree interns such subtrees bottom-up into
//! shape ids ([`ShapeTable`]); [`crate::compute::compute_child_layout`] then asks the [`Memo`] for
//! `(shape, question)` before running a node's algorithm, and stores what the algorithm answered.
//!
//! A final-layout entry also keeps, for every child, the layout the node gave it and the final
//! question it asked it. A hit is **stamped**: each child gets that layout and is asked that
//! question through `compute_child_layout`, which in turn hits the child's own cache, the memo, or
//! runs. So every node a stamp reaches ends up with the layout and the final cache entry a run of
//! the algorithm would have left. What no cache below a memo-served node records is what its
//! algorithm (and the algorithms below it that it skipped) would have asked: the node keeps a mark
//! ([`crate::tree::NodeCache::memo_served`]), and the scheduler makes any change below a marked
//! node reach it (`schedule::Scheduler::run_queue`); the scheduler then recomputes it with its own
//! algorithm (the memo is skipped for the node it recomputes), after which its children record what
//! it asks and later changes below it stop where they stop without the memo.
//!
//! Shapes are exact: interned by value, never hashed into one word. A tree must reset a node's
//! shape and its ancestors' whenever the node's style, children or content change, and must clear
//! both tables when an interned style id is reused for another style.

use crate::hash::FxBuild;
use crate::tree::cache::CacheKey;
use crate::tree::{Layout, LayoutOutput, RunMode};
use std::collections::HashMap;

/// No shape: the node's answers depend on something outside its subtree, or it is not worth
/// sharing.
pub const NO_SHAPE: u32 = u32::MAX;
/// A tree's mark for "shape not computed yet".
pub const SHAPE_UNSET: u32 = u32::MAX - 1;
/// Containers with more children than this get no shape: their algorithm is linear in the
/// children anyway, they almost never repeat, and interning them would cost a pass over the children
/// after every edit below.
pub const MAX_SHAPE_CHILDREN: usize = 64;

/// Subtree shapes interned by value: `(style id, child count, child shapes...)`.
#[derive(Default, Clone)]
pub struct ShapeTable {
    ids: HashMap<Box<[u32]>, u32, FxBuild>,
    /// Scratch for building a key.
    pub key: Vec<u32>,
}

impl ShapeTable {
    /// The shape of the key in `self.key` (filled by the caller), interned.
    pub fn intern_key(&mut self) -> u32 {
        if let Some(&id) = self.ids.get(self.key.as_slice()) {
            return id;
        }
        let id = self.ids.len() as u32;
        debug_assert!(id < SHAPE_UNSET);
        self.ids.insert(self.key.clone().into_boxed_slice(), id);
        id
    }
    pub fn clear(&mut self) {
        self.ids.clear();
    }
    pub fn len(&self) -> usize {
        self.ids.len()
    }
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }
}

/// A child of a stamped final layout: the layout its parent gave it and the final question its
/// parent asked it (`None` for a cheap leaf, which is never asked through the cache).
#[derive(Clone, Copy, Debug)]
pub struct MemoKid {
    pub layout: Layout,
    pub key: Option<CacheKey>,
}

#[derive(Clone, Copy, Debug)]
struct Entry {
    output: LayoutOutput,
    observed: u8,
    kids: u32,
    nkids: u32,
}

/// `(shape, run mode, question)` -> answer, and for final layouts the children's layouts.
///
/// Entries are only added during a layout call, so an index stays valid until [`Memo::trim`],
/// which a tree calls between layout calls.
pub struct Memo {
    index: HashMap<(u32, u8, CacheKey), u32, FxBuild>,
    entries: Vec<Entry>,
    kids: Vec<MemoKid>,
    /// Whether lookups and stores happen at all.
    pub enabled: bool,
    /// Entry and child budgets; [`Memo::trim`] clears everything past either.
    pub max_entries: usize,
    pub max_kids: usize,
    pub hits: u64,
    pub stores: u64,
    /// A node the scheduler is recomputing: its own questions skip the memo (see
    /// `schedule::Scheduler::recompute`).
    pub skip: Option<crate::tree::NodeId>,
}

impl Default for Memo {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for Memo {
    fn clone(&self) -> Self {
        Self { index: self.index.clone(), entries: self.entries.clone(), kids: self.kids.clone(), ..Self::empty_like(self) }
    }
}

impl Memo {
    pub fn new() -> Self {
        let enabled = std::env::var_os("CEPHAL_NO_MEMO").is_none();
        Memo { index: HashMap::default(), entries: Vec::new(), kids: Vec::new(), enabled, max_entries: 1 << 16, max_kids: 1 << 20, hits: 0, stores: 0, skip: None }
    }
    fn empty_like(o: &Memo) -> Self {
        Memo { index: HashMap::default(), entries: Vec::new(), kids: Vec::new(), enabled: o.enabled, max_entries: o.max_entries, max_kids: o.max_kids, hits: 0, stores: 0, skip: None }
    }

    /// Forgets every entry (shapes stay valid).
    pub fn clear(&mut self) {
        self.index.clear();
        self.entries.clear();
        self.kids.clear();
    }

    /// Between layout calls: clears the memo when it outgrew its budget.
    pub fn trim(&mut self) {
        if self.entries.len() > self.max_entries || self.kids.len() > self.max_kids {
            self.clear();
        }
    }

    #[inline]
    fn mode(m: RunMode) -> u8 {
        match m {
            RunMode::PerformLayout => 0,
            RunMode::ComputeSize => 1,
            RunMode::PerformHiddenLayout => 2,
        }
    }

    /// The entry answering `(shape, mode, key)`.
    #[inline]
    pub fn find(&self, shape: u32, mode: RunMode, key: &CacheKey) -> Option<u32> {
        if self.entries.is_empty() {
            return None;
        }
        self.index.get(&(shape, Self::mode(mode), *key)).copied()
    }

    /// Entry `e`'s answer, observed axes and child count.
    #[inline]
    pub fn answer(&self, e: u32) -> (LayoutOutput, u8, usize) {
        let x = &self.entries[e as usize];
        (x.output, x.observed, x.nkids as usize)
    }

    /// Child `i` of final-layout entry `e`.
    #[inline]
    pub fn kid(&self, e: u32, i: usize) -> MemoKid {
        let x = &self.entries[e as usize];
        self.kids[x.kids as usize + i]
    }

    /// Stores a size answer.
    pub fn store_size(&mut self, shape: u32, key: CacheKey, output: LayoutOutput, observed: u8) {
        let e = self.entries.len() as u32;
        self.entries.push(Entry { output: output.size_answer(), observed, kids: 0, nkids: 0 });
        self.index.insert((shape, Self::mode(RunMode::ComputeSize), key), e);
        self.stores += 1;
    }

    /// Starts a final-layout entry; its children follow with [`Memo::push_kid`] and it is published by
    /// [`Memo::finish_final`] (or dropped by [`Memo::abandon_final`]).
    pub fn begin_final(&mut self) -> usize {
        self.kids.len()
    }
    pub fn push_kid(&mut self, kid: MemoKid) {
        self.kids.push(kid);
    }
    pub fn abandon_final(&mut self, start: usize) {
        self.kids.truncate(start);
    }
    pub fn finish_final(&mut self, start: usize, shape: u32, key: CacheKey, output: LayoutOutput, observed: u8) {
        let e = self.entries.len() as u32;
        let nkids = (self.kids.len() - start) as u32;
        self.entries.push(Entry { output, observed, kids: start as u32, nkids });
        self.index.insert((shape, Self::mode(RunMode::PerformLayout), key), e);
        self.stores += 1;
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    /// Bytes held (entries, children, index), for memory ledgers.
    pub fn bytes(&self) -> usize {
        self.entries.capacity() * size_of::<Entry>() + self.kids.capacity() * size_of::<MemoKid>() + self.index.capacity() * (size_of::<((u32, u8, CacheKey), u32)>() + 1)
    }
}
