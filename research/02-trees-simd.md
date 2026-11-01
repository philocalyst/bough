# Cache-efficient and SIMD-parallel tree representations for a retained GUI tree

Scope: the core retained tree of a GUI framework. The tree holds UI nodes, gets frequent local edits (insert, remove, move, reorder children), keeps ordered children, maintains subtree aggregates (bounds, damage, dirty flags), and is traversed in paint order. This report surveys the search-tree, SIMD, order-maintenance, flat-encoding and parallel-traversal literature, then ends with ranked design levers.

**Verification status.** Every claim carries a URL. Claims marked **[read]** come from a page or PDF I actually fetched in this session. Claims marked **[search-summary]** come only from a search-result snippet and should be checked against the paper before quoting. Claims marked **[from memory]** name a well-known result whose primary source I did not open in this session; the URL given is the canonical source, but treat the numbers as unverified. Several primary PDFs (Rao & Ross, Bender-Hu, Blelloch, Leis ART, the 2008 Wald paper) returned binary or 403/404 responses, so their details are either search-summary or from memory.

---

## 1. Cache-conscious search trees

The common thread of this section is that a search over N keys on a modern CPU is dominated by cache misses and serial dependent loads, not by comparisons. The design moves are: make nodes exactly one or a few cache lines, remove child pointers in favour of arithmetic, and make the per-node search branch-free and SIMD-parallel.

### 1.1 CSS-trees and CSB+-trees (Rao & Ross)

Rao and Ross, "Cache Conscious Indexing for Decision-Support in Main Memory," VLDB 1999, pp. 78-89, introduced Cache-Sensitive Search trees (CSS-trees). A CSS-tree is a directory laid over a sorted array. Nodes are sized to the cache line and have no child pointers: children are found by computing an offset in a level-ordered array. The authors compare against hash indexes, binary search trees, T-trees, B+-trees, interpolation search and binary search on arrays, and attribute the gap largely to cache misses in a cache simulation [search-summary: https://www.vldb.org/dblp/db/conf/vldb/RaoR99.html ; paper PDF https://courses.cs.duke.edu/spring03/cps216/papers/rao-ross-1999.pdf].

The follow-up, "Making B+-Trees Cache Conscious in Main Memory" (SIGMOD 2000), proposes CSB+-trees, which try to keep CSS-tree cache behaviour while still supporting incremental updates, since CSS-trees are built for relatively static data [search-summary: https://www.paperdigest.org/certificate/f084c5de797efd0a].

The layout idea, from memory of the CSB+ paper: all children of a node are stored contiguously in a "node group," so an internal node stores only one pointer to the group plus its keys. That saves roughly half of the per-node pointer bytes, which lets the node fan out to a full cache line of keys. [from memory; primary: the CSB+ paper, see the DBLP index above.]

Sketch of the CSB+ internal node, in the spirit of the paper:

```c
// CSB+-style internal node: one pointer to a contiguous child group.
// Children are allocated together so child i is group_base + i.
struct csb_internal {
    uint32_t  nkeys;          // <= 14 for a 64-byte node with 32-bit keys
    uint32_t  keys[14];       // separators
    struct csb_node *group;   // all children stored contiguously
};
```

### 1.2 The S-tree / S+-tree layout (Algorithmica)

The Algorithmica HPC book has the most explicit, measured treatment of an implicit B-tree layout that I could read in this session. It is the best single primary-ish source for the tradeoffs.

**Layout [read: https://en.algorithmica.org/hpc/data-structures/s-tree/].** The "S-tree" is an implicit, pointer-free B-tree with B = 16 keys per node, which is 64 bytes, one cache line. Each node has up to 17 children. Node k's children are numbered k*(B+1)+i+1, the (B+1)-ary generalization of Eytzinger numbering. The "S+ tree" variant stores internal nodes in one flat array, with key i in a node being the smallest key in child i+1's subtree. Leaves hold only sorted keys, and layers are stored one after another with the leaves in layer 0 and the root in layer H-1. Internal nodes take roughly 1/16 of the original array size, and the article quotes the S+ overhead as 6-7%.

**Rank computation [read].** The fast version compares both 8-element halves with `_mm256_cmpgt_epi32`, merges the two results with `_mm256_packs_epi32`, takes a single `_mm256_movemask_epi8`, and then `__tzcnt_u32(mask) >> 1`. The search key is `x - 1`, which turns the count into a lower bound. The packs instruction interleaves output, so internal nodes are permuted once during preprocessing and a 17-entry `translate` table maps the permuted rank back to the original order. Leaves are not permuted and use a direct mask-based rank. The article notes, as an untested option, that `popcnt` instead of `tzcnt` would make the key order irrelevant and remove the permutation.

Node shape, as a Rust sketch of the internal node (the article's version is C/intrinsics):

```rust
// Internal node of an implicit S+ tree, 32-bit keys, one cache line.
#[repr(C, align(64))]
struct SNode {
    keys: [u32; 16],   // B = 16, node fills one cache line
}
// Children of node k live at layer-relative index k*17 + i + 1.
// No pointers stored anywhere.
```

**Measured numbers [read, Zen 2, Clang 10, AVX2]:**
- S-tree vs `std::lower_bound`: up to 8x faster, using the same space as the sorted array.
- S+ tree vs `std::lower_bound`: up to 15x faster.
- The optimizations on the S-tree "saved us 15-20% or so."
- S+ tree vs optimized S-tree: 1.5-3x faster.
- A 32-element node variant lowered latency but did not improve throughput.
- Measured as real latency rather than throughput, "not that impressive," because about three queries overlap on average.
- The article reports throughput on large arrays, and TLB misses appear as the array grows past about 2^25 integers with 2 MB pages.

The lesson for a GUI tree is that the best node width is one cache line of keys, that SIMD compare plus movemask plus tzcnt is the rank primitive, and that the implicit layout costs only about 6-7% of extra space.

### 1.3 Eytzinger, B-tree (S-tree) and binary-search layouts

**Eytzinger [read: https://en.algorithmica.org/hpc/data-structures/binary-search/].** The sorted array is renumbered as an implicit binary tree with the ahnentafel scheme: node k has children 2k and 2k+1. Construction is a recursive in-order traversal. Search is:

```c
// Eytzinger search, 1-indexed. t[1..n] is the Eytzinger array.
int k = 1;
while (k <= n) {
    k = 2 * k + (t[k] < x);   // branchless step
}
// recover the answer: strip trailing 1-bits, shift once more
k >>= __builtin_ctz(~k) + 1;
```

The article's numbers (Zen 2, DDR4-2666, Clang 10): branchless binary search is 2.5-3x faster than `std::lower_bound` on small arrays but slower on large ones without prefetching. Plain Eytzinger "starts taking off much sooner" on large arrays. Eytzinger with prefetching is 3-4x faster than plain Eytzinger on large arrays and about 2x faster than `std::lower_bound`. Descendants 4k..4k+3 are adjacent, and 16k..16k+15 fill one cache line for a 64-byte-aligned array, so prefetching t+16k fetches about four levels ahead. The article gives no measured miss counts, only reasoning about cache-line sharing.

**van Emde Boas layout and cache-oblivious B-trees.** Bender, Demaine and Farach-Colton, "Cache-Oblivious B-Trees," SIAM J. Comput. 35(2), 2005, pp. 341-358 (FOCS 2000), reach the search bound Theta(1 + log_{B+1} N) memory transfers without knowing B, for B = Omega(log N log log N), and give amortized Theta(log_B N + (log^2 N)/B) memory transfers for insertion and deletion [search-summary: https://dblp.dagstuhl.de/rec/journals/siamcomp/BenderDF05.html ; https://erikdemaine.org/papers/CacheObliviousBTrees_SICOMP/ ; DOI 10.1137/S0097539701389956]. The search snippets did not describe the vEB recursive layout itself, so I am stating it from memory: the tree is split at mid-height into a top subtree and about sqrt(N) bottom subtrees of sqrt(N) nodes each, recursively, so that every block of size B contains a connected subtree. This is primary-source-level knowledge that I did not verify in this session.

The Algorithmica page on binary search does not cover vEB, which is consistent with the article's own framing: Eytzinger plus prefetch is the practical winner on modern hardware, and B-tree (S-tree) layouts beat both at large sizes with SIMD.

### 1.4 FAST (Kim et al., SIGMOD 2010)

Kim, Chhugani et al., "FAST: Fast Architecture Sensitive Tree Search on Modern CPUs and GPUs," SIGMOD 2010, pp. 339-350 (DOI 10.1145/1807167.1807206). The ACM page returned 403 to my fetch. The paper's design blocks a binary search tree so that the tree is organized around the page size, cache-line size and SIMD width of the hardware, and uses SIMD to compare several keys per step [search-summary: https://hgpu.org/?p=1493 ; https://www.doi.org/10.1145/1807167.1807206]. A later TODS journal version appeared in 2011 under the title "Designing Fast Architecture Sensitive Tree Search on Modern Multi-Core/Many-Core Processors" [search-summary: https://hgpu.org/?p=6702].

The headline throughput in search snippets is about 50 million queries per second on CPU and 85 million on GPU for trees of 64M elements [search-summary; check the paper before quoting]. The key idea I rely on, from memory: SIMD-blocked binary trees with a hierarchical layout (cache-line blocks inside page blocks) cut the number of TLB and cache misses per lookup. The Algorithmica S-tree article is the more recent and readable treatment of the same idea.

### 1.5 Adaptive radix tree (ART), Masstree, Bw-tree, Bf-tree

These four are relevant as alternatives to a sorted-array or B-tree node, but I could not read their primary sources in this session. The descriptions below are from memory and should be verified before any numeric claim is quoted.

- **ART (Leis, Kemper, Neumann, ICDE 2013).** Radix tree with four node types of 4, 16, 48 and 256 children. Node4 and Node16 store sorted key bytes with a linear or SSE compare; Node48 uses a 256-byte index array into 48 child slots; Node256 is a direct array. Path compression and lazy expansion collapse single-child chains [from memory; paper: https://db.in.tum.de/~leis/papers/ART.pdf, the PDF returned binary to my fetch]. ART is the strongest reference for "keys with a shared prefix in a radix layout," which matters for GUI IDs or paths.

- **Masstree (Mao, Kohler, Morris, EuroSys 2012).** A trie of B+-trees where each layer indexes 8-byte keyslices. Each leaf keeps a permutation word that gives the sorted order of its slots without moving key bytes, so inserts write a slot and then update the permutation atomically [from memory; the ACM page returned 403].

- **Bw-tree (Levandoski, Lomet, Sengupta, ICDE 2013).** A latch-free B-tree where updates are prepended as delta records to a logical page and a mapping table translates logical page IDs to physical pointers, so the tree is updated with compare-and-swap [from memory].

- **Bf-tree (VLDB 2024).** A buffer-managed B-tree that uses "mini-pages" for caching and a circular buffer for updates. My fetch of the vldb.org index page returned no paper. I have no verified numbers for Bf-tree.

**Takeaway for a GUI tree.** None of these is a good core tree for a retained UI, because UI trees are small (typically thousands to tens of thousands of nodes), are mostly traversed in order rather than searched by key, and need subtree aggregates, which none of these structures provide. Their node-level tricks (fixed-width SIMD rank, permutation words for lazy reordering, small inline key prefixes) are the useful part.

---

## 2. SIMD inside nodes: rank, inline keys, fence keys

### 2.1 Rank with movemask and popcount or tzcnt

The rank primitive is: broadcast the query, compare against a vector of keys, pack the comparison results into a bit mask, and count the set bits. On x86 this is `cmp` plus `movemask` plus `popcnt` (or `tzcnt` on a sorted run). The Algorithmica article uses the tzcnt form with a packs-based permutation [read: https://en.algorithmica.org/hpc/data-structures/s-tree/]. On AVX-512, a comparison writes a mask register directly and the count is `popcnt` with no movemask.

The bbtree design (below) uses a count over every slot instead of a binary search, because on out-of-order cores the dependent loads of a binary search dominate. That is the modern consensus: for node sizes of 16 to 64 slots, a branch-free full-width count beats a binary search even though it does more arithmetic.

### 2.2 bbtree: inline keys, fence keys, and the cost model

bbtree (Xiangpeng Hao, GitHub) is a B-tree with a documented design folder. The folder's file list was readable, but the individual .md files were not fetched in the way I expected for 04/05, so I give the 06 and 04/05 content only as far as the fetches returned it. The design folder includes 04-inline-key.md, 05-fence-key.md, 06-cost-model.md, 07-node-plan.md and 12-batch-api.md [read listing: https://github.com/XiangpengHao/bbtree/tree/main/design]. I read 04, 05 and 06 through raw.githubusercontent URLs. I did not read 07 or 12 in this session.

**The problem bbtree addresses [read: 04-inline-key.md, https://raw.githubusercontent.com/XiangpengHao/bbtree/main/design/04-inline-key.md].** For keys whose comparison reads the heap, such as String, paths and [u8; 32], bbtree measured 0.70-0.95x of std at 1M entries, while integer keys were faster. The cause: every comparison reads the key's heap bytes, and in a binary search the next probe depends on that result, so those misses are serial.

**The inline key [read].** An inline key is a small Copy value derived from a key, stored in a parallel dense array beside the keys (not inside a fatter element). The contract: if inline(a) != inline(b), their order matches the order of a and b; ties fall back to full-key comparison. For byte-like keys the inline key is an 8-byte big-endian zero-padded window; internal nodes keep 4 bytes, and byte arrays use 2. Keys are not length-prefixed in the inline key, so equality for short keys can be settled from the key's own length header without touching the heap.

```rust
// Shape of the parallel-array layout described in bbtree's 04-inline-key.md.
// Both layouts cost 8 bytes per entry; the parallel one keeps the
// search run in 4 cache lines instead of 13 for a 1 KB leaf.
struct LeafParallel<K, V> {
    hdr:    Header,
    inline: [u64; N],   // searched densely, register-comparable
    keys:   [K; N],     // real keys, contiguous, handed out as &[K]
    vals:   [V; N],
}
// Fat-element alternative: [(u64, K); N] — same bytes, worse search.
```

**The search [read].** The search runs over inline keys, which compare in a register. The run of equal-prefix keys is found by a count over every slot, clipped to len, rather than by binary search. Slots past len hold the greatest inline key. On AVX-512 each count is three compares with no loop. Explicit vector code exists for AVX-512, SSE2 (the x86-64 baseline) and NEON. An early-exit scan was 6-18% slower than the count. Node prefetch was not used.

**Measured numbers [read].**
- Section 2 of 04: an 8-byte inline prefix with binary search over 1 KB nodes gives 192 ns against std's 191 ns on get.
- Q3, wiki titles, 10M, get_hit: no inline keys 1073 ns, 83.5 DRAM fills; inline keys 704 ns, 42.2 fills; internal-nodes-only 748 ns, 46.2 fills. About 325 of the 369 ns saved comes from internal nodes. Memory rises from 68.8 to 80.5 bytes per entry (+17%), and the map gains a level (4 to 5).
- Q6, 1M hash32 hits: the count took 167 ns against 246 ns for an emulated 64-bit count, with 476 against 1,477 instructions on baseline x86-64.
- Paths regressed to 1.2-1.7x std time until prefix stripping was added, which brought str-path to 0.66-0.76x of its earlier time at 1M.
- Tie rates on sorted neighbours with 8-byte inline keys: crates.io names 36%, Wikipedia titles 70%, Linux paths 99%. This is the number that decides whether the inline key helps; if your keys have long shared prefixes, the inline key gives no benefit and fence-key prefix stripping matters more.

**Fence keys [read: 05-fence-key.md, https://raw.githubusercontent.com/XiangpengHao/bbtree/main/design/05-fence-key.md].** Each node has a low and high fence, the nearest separators on each side in its ancestors. Every key k in the node satisfies low <= k < high, so all keys share the prefix P = LCP(low, high). Each node stores only `prefix_len: u16`, and its inline keys are the windows just past that prefix. Fences are not stored as separate copies, which avoids requiring K: Clone. A child's prefix is never shorter than its parent's. Inserts do not change a node's prefix, because the new key already lies between its fences; only separator changes do. Any prefix_len up to LCP(low, high) is correct, so prefixes can be refitted lazily. Splits give both halves new fences. Separator removal shrinks the first border's prefix to min(own, LCP(s, s')) before the change. Searches run user callbacks before any change, so a panic cannot leave the tree half-rebalanced.

For a GUI tree, fences are the useful idea: the parent already knows the bounds of a child, so a child does not need to store a copy of them, and a single u16 gives the common prefix length.

**Cost model [read: 06-cost-model.md, https://raw.githubusercontent.com/XiangpengHao/bbtree/main/design/06-cost-model.md].** The model is a set of closed-form sizes plus a dependency-chain latency model.

```text
leaves          L ~= (N + 1) / (u_l * C_l + 1)
fanout          f  = u_i * C_i + 1
internal nodes  ~= L / (f - 1)
node bytes      ~= L * S_l + (L / (f - 1)) * S_i

lookup  ~= sum over levels [ read header + search summaries + read edge ]
           + final key equality + value read            (a dependent chain)
insert  ~= shift ~half a leaf + split (alloc, move half, insert separator, refit)
scan    ~= one descent, then a leaf crossing every ~40 u64 pairs or ~18 String pairs at 70% fill
```

Where C_l and C_i are leaf and internal capacities, S_l and S_i node sizes, and u_l and u_i fill fractions. The document says the formulas "predict depth, not guarantee it." Its rules, quoted in substance: charge summary search at capacity, not fill; removing a warm level saves instructions, not DRAM time; fewer cache misses and lower latency do not always go together; first remove dependencies and instructions, then consider bigger nodes; read the generated assembly before timing; use counted per-level fill rather than one constant (measured fill was 65% and 60% for 1 KiB and 2 KiB internals at 10M u64, and 77% and 86% at 1M).

Tested decisions [read]: 2 KiB internal nodes reduced depth 5 to 4 at 10M u64 but raised u64 hits from 131.0 to 164.0 ns, so they were rejected as a default. 512-byte leaves reduced random insert by 10.5% but raised sequential insert by 13.7%, so they were rejected. 2-byte inline keys for fixed-width arrays gave hash32 hits 318.1 to 300.4 ns with DRAM fills per hit falling 10.7 to 6.1, so they were accepted. A last-leaf append cache cut ordered u64 insert from 31.1 to 5.3 ns.

The most transferable parts for a UI tree are the fill-aware closed-form sizes and the "latency over miss counts" rule.

### 2.3 Summary: which SIMD node tricks transfer to a UI tree

- Full-width count over a cache-line of keys instead of binary search (bbtree, S-tree).
- Parallel dense key array separate from the payload (bbtree inline keys).
- Prefix stripping via fence keys (bbtree).
- Permutation word for lazy reordering of children without moving payloads (Masstree).

---

## 3. Order maintenance and packed-memory arrays

A retained GUI tree must keep children in paint order (z-order) and, when laid out as a flat array in DFS order, must keep the array sorted by that order while nodes are inserted and moved. This is the order-maintenance problem.

### 3.1 Order maintenance

The problem: maintain a totally ordered set under insert(X, Y) (place X immediately after Y), order(X, Y) (does X precede Y?) and delete(X) [read: https://en.wikipedia.org/wiki/Order-maintenance_problem]. The article gives the lineage:

- Dietz (1982): insert in O(log n) amortized, order in O(1), no delete.
- Tsakalidis (1984): adds delete with O(log n) bounds, using BB[alpha] trees; with indirection, insert and delete become O(1) amortized.
- Dietz and Sleator (1987): worst-case O(1).
- Bender, Cole and Zito (2002): "two simplified algorithms for maintaining order in a list" (ESA 2002). The article says these simplify the Dietz-Sleator scheme.
- Bender, Fineman, Gilbert, Kopelowitz and Montes (2017): deamortized solution.

The article also states the list-labeling reductions: for label space m = n^(1+Theta(1)), an O(log n) amortized solution is known (Itai, Konheim, Rodeh 1981); for exponentially larger m, a constant amortized solution exists (Bulanek, Koucky, Saks 2015) [read, same URL].

The standard indirection trick is what matters here: group elements into O(N / log N) sublists of size O(log N) and keep a higher-level list over them. This gives O(1) amortized insert and delete with O(1) order queries [read, same URL].

### 3.2 Packed-memory arrays

A PMA keeps a sorted list in an array with gaps, so that inserts shift only nearby elements. Bender and Hu, "An Adaptive Packed-Memory Array," ACM TODS 32(4), 2007 (journal version of PODS 2006) [search-summary: https://dblp.uni-trier.de/pid/97/2712.html ; https://typeset.io/papers/an-adaptive-packed-memory-array-4ce30n6ysv].

- Worst-case bound (traditional PMA): O(log^2 N) amortized element moves and O(1 + (log^2 N)/B) amortized memory transfers per update [search-summary, the authors' statement].
- Adaptive PMA (APMA): on common patterns, O(log N) amortized moves and O(1 + (log N)/B) memory transfers [search-summary].
- Empirical [search-summary]: for sequential insertions of about 1.4M elements, APMA made four times fewer element moves per insertion and ran more than seven times faster than the traditional PMA.

The PMA layout, from memory: the array is divided into leaf segments of size Theta(log N). A density-threshold tree over segments (an implicit binary tree, one level per power of two) defines an upper density bound per level. An insert goes to the leaf segment that should contain the key; if the segment is over its density bound, the algorithm rebalances the smallest enclosing window that is within bound, spreading elements evenly. This is the source of the O(log^2 N) bound.

Sketch of the structure for an order-preserving child list (not a published API, a design sketch):

```rust
// A PMA of child IDs in paint order. Gaps are sentinel slots.
struct Pma {
    slots: Vec<u32>,        // NONE = gap; occupied slots are sorted
    seg:   usize,           // segment size, ~ log2(N)
    // implicit density tree: level l has thresholds (upper, lower)
    // interpolated between leaf and root bounds, from Bender-Hu
}
// insert(after, id): find the slot after `after`, shift nearby gaps,
// rebalance the smallest in-bound window if its segment overflows.
// Amortized O(log^2 N) element moves, per Bender-Hu 2007.
```

Tera-PMA and rewired-memory PMA: I did not verify these in this session. The phrase "rewired memory" refers to Schuhknecht et al. and related work that remaps virtual pages to physical pages so that a PMA can be resized or rebalanced by page-table manipulation rather than copying. Treat any specific numbers for these as unverified until the papers are read.

### 3.3 Applicability to a UI tree

Using a PMA for children of a node in paint order gives cheap insert-at-position with gaps and cache-friendly scans. It is overkill for a node with ten children. It is interesting for the flat global array of all nodes in DFS order, where local edits near one subtree must not move the whole array. The list-labeling view is the cleaner one for paint order: give each node a label that orders it, relabel locally when labels run out, and do order queries in O(1) by comparing labels.

---

## 4. Flat tree encodings for traversal

### 4.1 Preorder arrays with skip pointers

A tree in DFS preorder is a flat array of nodes where every subtree is a contiguous range starting at its root. Store for each node `skip` = index of the first node after its subtree (its "escape" index). Then a traversal that wants to skip a clean subtree does one index jump, `i = skip[i]`, with no stack. This is the stackless BVH traversal technique: the escape index is the classic output of linearized BVH construction. I did not open a primary source for a linearized-BVH paper in this session, so the citation for this technique is from memory; the Embree source (below) and the Lier et al. survey are the readable references for wide-BVH traversal order.

```rust
#[derive(Clone, Copy)]
#[repr(C)]
struct Node {
    bounds:   [f32; 4],   // minx, miny, maxx, maxy
    skip:     u32,        // index after this subtree: clean -> jump here
    first:    u32,        // first child = i + 1 in preorder, implicit
    flags:    u32,        // dirty / visible / hit-testable
    payload:  u32,        // index into payload arrays
}
// paint order = array order. Visit i; if subtree clean, i = skip[i];
// else i += 1 (descend into first child, which is i+1).
```

Insert and remove are expensive in this encoding because they shift the array and invalidate every skip pointer that spans the edit point. The standard remedy: keep the preorder array rebuilt lazily from a mutable tree, or use a gap buffer or PMA over preorder positions (Section 3).

### 4.2 Euler-tour trees

An Euler-tour tree (ETT) represents a tree as the sequence of its Euler-tour events (enter and exit per node), in a balanced BST or a splay/treap. Subtree move is then a split and concatenation, O(log n) each. ETTs are the standard dynamic-tree encoding for link/cut and subtree-move problems. For a UI tree they give O(log n) edits with O(log n) traversal overhead and poor locality, so they are a fallback for huge dynamic forests, not a render tree. I did not verify a primary source in this session; this is from memory.

### 4.3 Balanced parentheses and range min-max trees

The balanced-parentheses (BP) encoding writes a tree as an open paren per node entry and a close paren per exit, so a 2n-bit sequence represents an n-node tree. Navarro and Sadakane, "Fully Functional Static and Dynamic Succinct Trees," ACM Trans. Algorithms 10(16), 2014 (preliminary at SODA 2010), use BP plus a range min-max tree (rmM-tree) to support navigation primitives such as findclose, enclose, depth and subtree size in O(1) or O(log n) time with 2n + o(n) bits [search-summary: https://cs.uwaterloo.ca/~imunro/cs840/ProjectPapers/SODA10_013_sadakanek.pdf ; https://users.dcc.uchile.cl/~gnavarro/ps/talg12.pdf ; the Cordova-Navarro simplification https://www.arxiv.org/pdf/1601.06939]. The static structure builds in O(n) time with O(n) bits; the same framework yields the first fully-functional dynamic succinct trees [search-summary].

For a UI tree the BP encoding is a space-efficient cold representation: 2 bits per node plus an rmM-tree over blocks. It does not give cheap edits, and it is not what a per-frame traversal should touch. Its value is in a compact persistent snapshot (serialize to disk or sync to a peer) and in fast subtree-size and depth queries.

### 4.4 LOUDS and level-order encodings

LOUDS (Jacobson, 1989) writes the tree in breadth-first order with a unary code of child counts, giving 2n + o(n) bits with child and parent navigation via rank/select. Breadth-first order is the order a layout pass wants when it processes one depth at a time, and it is the same order as the SoA hierarchies in Section 6. I did not verify the LOUDS primary source in this session; it is from memory.

### 4.5 Comparing the encodings for a GUI tree

| Encoding | Subtree skip | Paint-order scan | Local edit | Memory |
|---|---|---|---|---|
| Preorder + skip | one index jump | sequential | O(n) shift or PMA | 1 index/node + payload |
| Euler-tour tree | split/concat | pointer chase | O(log n) | heavy |
| BP + rmM | findclose O(log n) | rank/select | rebuild or dynamic BP | ~2 bits/node |
| LOUDS | via rank/select | level-order | rebuild | ~2 bits/node |

The preorder-plus-skip encoding is the only one that gives a single-index subtree skip and a streaming paint-order scan at the same time, which is the core requirement. The cost is edit locality, which is the job of the PMA or a gap buffer.

---

## 5. SIMD and parallel tree algorithms beyond node search

### 5.1 Blelloch scan (parallel prefix sums)

Blelloch, "Prefix Sums and Their Applications," CMU-CS-90-190 (1990), and the 1993 IEEE TC paper "Prefix sums and their applications" (PDF https://www.cs.cmu.edu/~guyb/papers/Ble93.pdf), define the scan with O(n) work and O(log n) depth via an up-sweep and down-sweep over a balanced binary tree [from memory; the PDF returned binary to my fetch]. Scan computes offsets: given per-node sizes, the exclusive scan gives each node's position in a flat output array, which is exactly what a layout pass needs to compact a dirty set or to assign DFS offsets to a parent-vector tree.

For a UI tree, a scan over subtree sizes gives each node's index in a preorder array in O(n) work and O(log n) depth, and a scan over dirty flags gives compacted lists of dirty nodes for a parallel layout pass.

### 5.2 Wide BVHs and packet traversal (Embree, Wald et al.)

Embree is the readable reference for SIMD tree traversal with wide nodes.

- **Instruction sets [read: https://raw.githubusercontent.com/RenderKit/embree/master/README.md].** "with support for SSE, AVX, AVX2, and AVX-512 instructions, and uses runtime code selection to choose between these kernels." It "requires at least an x86 CPU with support for SSE2 or an Apple M1 CPU." EMBREE_MAX_ISA selects among SSE2, SSE4.2, AVX, AVX2, AVX512. The README does not mention NEON for the ISA list, so NEON kernels should not be assumed from it.
- **Packets [read].** Ray packet queries are supported for packet sizes 4, 8 and 16 (rtcIntersect4, 8, 16). Packet traversal kernels are "turned ON by default." The README notes that packets passed to these calls are kept intact in callbacks.
- **Wide BVH.** The README does not name a BVH variant; the BVH4/BVH8 terminology comes from Embree's source and papers, not from this README, and I did not verify it here.

The 2008 wide-BVH work by Wald, Benthin and Slusallek is the origin of SIMD traversal of wide BVHs. I did not open it. A survey by Lier, Stamminger and Selgrad [search-summary: https://folk.idi.ntnu.no/elster/tdt24/tdt24-f09/holger-summary.pdf] states that Wald et al. (2008) endorse strict front-to-back order for their SIMD traversal of 16-wide BVHs, which requires sorting intersections during traversal; it also says a wide BVH can be built efficiently by pulling up nodes from an efficient binary BVH. The same survey mentions 4-wide SIMD with ray-direction-ordered traversal by Dammertz et al. (2008).

The GUI analogue is direct. A node with 4 to 8 children whose bounds are stored as SoA lanes (minx[4], miny[4], maxx[4], maxy[4]) tests all four children's bounds for a hit, a damage intersection or a clip in a handful of instructions. The AoS-to-SoA transposition of child bounds is done once per build.

```rust
// Four children's bounds in SoA lanes, tested against a damage rect.
#[repr(C, align(16))]
struct Wide4 {
    minx: [f32; 4], miny: [f32; 4],
    maxx: [f32; 4], maxy: [f32; 4],
    child: [u32; 4],    // index of child node, or u32::MAX for empty lane
}
// Intersect test with rect (x0,y0,x1,y1): one set of 4-lane compares
// and an AND, then movemask to get the child bitmask.
```

### 5.3 Parallel layout: Meyerovich, Superconductor, and the Spineless paper

- **Meyerovich et al., parallel CSS layout (Superconductor).** I did not verify a primary source in this session. Their published claim is that layout parallelism is available across independent subtrees, and that the bottleneck is the dependency between a parent's size and its children's positions. Treat this as background.

- **Spineless Traversal for Layout Invalidation (Kirisame, Wang, Panchekha, PLDI 2025).** Abstract, read from arXiv (https://arxiv.org/abs/2411.10659, version 8, dated September 26, 2025): standard incremental layout marks changed elements as dirty but must search the page to find them, and that search touches auxiliary nodes, causing cache misses and stalls. Spineless Traversal uses a more cache-friendly priority queue to visit only dirty nodes without touching the auxiliary nodes. The paper claims dramatic speedups on latency-critical interactions such as hover, typing and animation.
- **Numbers [read at the PLDI page, https://pldi25.sigplan.org/details/pldi-2025-papers/73/Spineless-Traversal-for-Layout-Invalidation, via search-summary].** Spineless Traversal is faster than the standard approach on 83.0% of 2,216 benchmarks, with a mean speedup of 1.80x. On the 65.6% of frames where at most 1% of fields are recomputed, the speedup is 2.22x [read at arXiv v8 HTML: https://arxiv.org/html/2411.10659v8].
- **Caution.** The arXiv v1 abstract reported a mean 3.23x on 78.2% of benchmarks. Cite the 1.80x figure from the final version and do not mix the two.
- **Implementation.** The paper implements the algorithm in Megatron, a compiler for incremental layout algorithms, so the Double Dirty Bit baseline and the spineless variant are compared on the same substrate [search-summary, arXiv abstract page].

For a GUI tree, the transferable idea is that a dirty set represented as a priority queue ordered by tree position beats a scan for dirty flags when the dirty set is small. This is the same conclusion as the hierarchical-bitset approach in Section 7, from a different angle.

### 5.4 Data-parallel tree representations: parent vectors (Co-dfns)

Hsu's Co-dfns compiler represents syntax trees as vectors of node indices, not pointers. Marshall Lochbaum's comparison states that the core compiler works on the syntax tree, which is mostly held as parent and sibling vectors, meaning lists of node indices [read: https://mlochbaum.github.io/BQN/implementation/codfns.html]. The APL Wiki says Co-dfns represents trees with vectors and that Hsu's thesis describes the code in detail [search-summary: https://aplwiki.com/wiki/Co-dfns]. Hsu's PLDI paper, "Accelerating Information Experts through Compiler Design," calls the compiler a pure, data-parallel program written in the dfns dialect, and notes that parent-child relationships are evident without traversing the matrix [search-summary: https://static.aminer.org/pdf/20170130/pdfs/pldi/3iqdysbvwhle4fjppexd6lu5acukktv2.pdf].

Caveats from Lochbaum [read]: the compiler has never self-hosted; the thesis GPU measurements used a hand-translated version; the design relies on repeat-until-convergence iteration and nested arrays; and the project has moved away from strict data-parallel style since.

The parent-vector representation is attractive for a GUI tree because every per-node computation becomes a gather, a scan or a reduction over index vectors, and each of these maps to SIMD or GPU. The cost is that a naive parent-vector traversal needs a fixed-point iteration for ancestor-dependent values (for example, the accumulated transform of a node), which is where level-order processing (Section 6) is cheaper.

---

## 6. Structure-of-arrays hierarchies in ECS engines

### 6.1 Bevy transform propagation

Bevy's transform propagation, as found in the bevy_transform source on docs.rs, is a root-first walk. The root query selects entities that have `Children` but no `ChildOf`, then descends [search-summary: https://docs.rs/crate/bevy_transform/0.17.1/source/src/systems.rs]. In 0.17 the transform query is iterated with a parallel iterator (`par_iter_mut`) when the std feature is on, and a serial path otherwise.

Newer versions (0.18 and 0.19) add `mark_dirty_trees`, which flags entities whose Transform, ChildOf or GlobalTransform changed, or whose ChildOf was removed, and propagates a dirty bit up to ancestors. Propagation then skips whole subtrees that lack the bit [search-summary: https://docs.rs/crate/bevy_transform/0.19.0/source/src/systems.rs ; https://doc.qu1x.dev/bevy_trackball/bevy_transform/systems/fn.mark_dirty_trees.html]. The relationship model is a ChildOf relationship on the child and a Children relationship target on the parent [search-summary: https://docs.rs/crate/bevy_app/0.19.0/source/src/propagate.rs].

I did not find a source describing archetype-level behaviour, and I did not verify parallel scheduling. Bevy's model is a good example of: relationships as components on the child (ChildOf) plus a derived target list on the parent (Children), and dirty bits that flow upward so that a root-first pass can prune. For a GUI tree the equivalent is a dirty bit that propagates to ancestors and a down-pass that prunes on it.

### 6.2 flecs hierarchies, Unity DOTS, MassEntity

- **flecs.** flecs stores parent-child as relationships (ChildOf) in archetypes, with hierarchy traversal through query terms. I did not verify its breadth-first table ordering in this session, so I am not stating specific claims about it.
- **Unity DOTS transforms.** The Unity Entities transform system uses `LocalToWorld` and a parent-to-child propagation over a hierarchy with `Parent` and `Child` buffers, and it is parallelized with the Burst compiler. I did not read the current source in this session, so specifics are not stated here.
- **MassEntity (Unreal).** MassEntity is an ECS framework in Unreal. I did not verify its hierarchy propagation in this session.

These three are listed because the task requested them, but I did not gather primary numbers for them. The general lesson for SoA hierarchies is the one from Section 4: a breadth-first or preorder array of transforms, with the parent index in a separate column, lets each level be transformed as a SIMD loop over contiguous rows, and parent-to-child propagation becomes a gather over the parent column.

### 6.3 Level-order SoA propagation

A concrete layout for transform propagation over a level-ordered array:

```rust
// Level-ordered SoA. Level l occupies [level_start[l], level_start[l+1]).
// parent[i] is the index of i's parent, which lies in an earlier level.
struct Hierarchy {
    level_start: Vec<u32>,
    parent:      Vec<u32>,
    local_x:     Vec<f32>,   // local transform columns
    local_y:     Vec<f32>,
    world_x:     Vec<f32>,   // computed per level
    world_y:     Vec<f32>,
}
// For each level l in order:
//   world[i] = world[parent[i]] composed with local[i], for i in the level.
// Within a level, every i is independent: a gather from parent columns
// plus a SIMD compose, with no cross-lane dependency.
```

The level loop serializes across levels and parallelizes within each level, which is the right trade for UI trees that are shallow (depth ~ 10 to 30) and wide.

---

## 7. Bit-parallel set operations on dirty sets

The dirty set in a retained GUI tree is a set of node indices that changed since the last frame. It is written often (every edit), read once per frame, and its members are usually a tiny fraction of the nodes. Three representations are relevant.

### 7.1 Hierarchical bitsets

A hierarchical bitset uses 64-ary summary levels: level 0 holds one bit per node, level 1 holds one bit per 64-node word that is nonzero, and so on, so a set over N = 64^k nodes uses k levels. Iteration finds the next set bit with `tzcnt` at the lowest level and walks upward only when a word is empty.

hibitset (Rust) is the best-known implementation of this design. I attempted to read its README through GitHub and got a 404, so I cannot quote its branching factor, level count or benchmarks from this session. Treat hibitset's design as recalled, not verified.

```rust
// Two-level 64-ary dirty set over up to 4096 nodes.
struct DirtySet {
    l0: Vec<u64>,   // one bit per node; len = N / 64
    l1: u64,        // bit w set iff l0[w] != 0
}
impl DirtySet {
    fn set(&mut self, i: u32) {
        let w = (i >> 6) as usize;
        self.l0[w] |= 1u64 << (i & 63);
        self.l1 |= 1u64 << w;               // N <= 4096 fits one u64
    }
    fn next(&self) -> Option<u32> {          // iterate in ascending index order
        if self.l1 == 0 { return None; }
        let w = self.l1.trailing_zeros() as usize;
        Some(((w as u32) << 6) | self.l0[w].trailing_zeros())
    }
}
```

In a retained tree where the index is preorder (or paint order), the set's ascending iteration is paint order, so a dirty-set walk visits dirty nodes in the order a repaint must follow.

### 7.2 Roaring bitmaps

Chambi, Lemire, Kaser and Godin, "Better bitmap performance with Roaring bitmaps," Software: Practice and Experience 46(5), 2016 (arXiv:1402.6407). The abstract, read in this session, says Roaring "often compress significantly better (e.g., 2 times)" than RLE-based WAH and Concise, and is "up to 900 times faster for intersections" [read: https://arxiv.org/abs/1402.6407]. The container types (array, bitmap, run) and the 4096-element switch-over are from memory; the fetched abstract page did not state them, and they should be verified in the paper before use.

Roaring's relevance: a dirty set that is clustered (a subtree's nodes often have adjacent indices in preorder) compresses to run containers, and set intersection across frames is a bitwise AND of containers.

### 7.3 SIMD and pext/pdep scanning

- **tzcnt loops** over 64-bit words are the scalar baseline for iterating a set bit mask.
- **pext/pdep** (BMI2 on x86): pext gathers bits selected by a mask into the low bits, pdep scatters bits into mask positions. pext compacts the indices of set bits from a mask in one step per word when combined with a precomputed index vector. This is how a compacted dirty list is built from a bitmap without branching.
- **NEON:** AArch64 has no pext. The equivalent is a 2-bit-per-lane shift and lookup, or a per-byte table. I did not verify a specific NEON bitmap-compaction routine in this session, so no NEON numbers are given.
- **AVX-512** compress (VPCOMPRESSD) writes the active lanes of a vector contiguously, which compacts an index vector by a dirty mask in one instruction.

### 7.4 Integrating bitsets with traversal

The combined pattern: keep the tree in preorder with skip indices (Section 4.1), keep the dirty set as a 64-ary bitset in the same index space, and for each dirty word, iterate set bits, and for each set bit, jump to the subtree root's skip index to skip clean siblings. The per-frame cost is O(dirty words + dirty nodes), not O(N). This is the same economy as Spineless Traversal (Section 5.3), with the priority queue replaced by a bitset in index order.

---

## 8. Design levers: 15 representations and algorithms, ranked

The requirements are: (i) O(1)-ish edits; (ii) streaming paint-order traversal with no pointer chasing; (iii) SIMD over 4-16 nodes at once for bounds, hit test and damage; (iv) skipping clean subtrees in one jump; (v) parallel processing. The ranking favours items that satisfy several requirements together and have a primary source to point to.

1. **Preorder array with escape (skip) index per node, plus SoA payload columns.** Satisfies (ii) with a sequential scan, (iv) with one index jump, and (iii) because bounds can be SoA columns. Source: stackless BVH escape-index traversal, Embree wide-BVH layout discussion [README: https://raw.githubusercontent.com/RenderKit/embree/master/README.md]. Weakness: edits.

2. **PMA / list-labeling over the preorder array for edits.** Gives O(log^2 N) amortized moves with local rebalance, which fixes the edit weakness of item 1 without giving up sequential scans. Sources: Bender and Hu, TODS 2007 [https://typeset.io/papers/an-adaptive-packed-memory-array-4ce30n6ysv], and the order-maintenance survey [https://en.wikipedia.org/wiki/Order-maintenance_problem]. Ranks second because the bounds are amortized, not worst-case.

3. **Order-maintenance labels with indirection for paint order.** O(1) amortized insert and O(1) order query, so paint-order comparisons are integer compares. Source: Dietz-Sleator, Bender-Cole-Zito [https://en.wikipedia.org/wiki/Order-maintenance_problem]. Useful for z-order comparisons across a tree that is restructured often.

4. **Wide (4- or 8-way) nodes with SoA child bounds and a movemask-based child bitmask.** Satisfies (iii) directly: one compare set tests 4 or 8 children. Sources: Embree packet and wide-BVH support [README], Lier et al. survey [https://folk.idi.ntnu.no/elster/tdt24/tdt24-f09/holger-summary.pdf].

5. **Dirty-bit propagation to ancestors plus root-first pruned walk.** Satisfies (iv) and gives aggregate invalidation for bounds and damage. Source: Bevy mark_dirty_trees [https://doc.qu1x.dev/bevy_trackball/bevy_transform/systems/fn.mark_dirty_trees.html]; Spineless Traversal motivates why the walk should not scan for dirty nodes [https://arxiv.org/abs/2411.10659].

6. **Hierarchical 64-ary dirty bitset in preorder index space.** Gives O(dirty words + dirty nodes) per frame, ascending iteration in paint order, and a branch-light word scan. Sources: hibitset design (recalled; README not fetched), tzcnt-based iteration described in the bbtree and Algorithmica tzcnt/movemask discussion [https://en.algorithmica.org/hpc/data-structures/s-tree/].

7. **Spineless-style priority queue of dirty nodes ordered by tree position.** Strongest published speedup for incremental layout (1.80x mean, 83.0% of 2,216 benchmarks, 2.22x on sparse frames) [https://pldi25.sigplan.org/details/pldi-2025-papers/73/Spineless-Traversal-for-Layout-Invalidation ; https://arxiv.org/html/2411.10659v8]. Ranks seventh because it is a dirty-set policy rather than a layout; combine with item 6.

8. **Parallel inline-key array plus full-width count for child search over keys.** Directly from bbtree; measured 1073 to 704 ns on wiki titles at 10M keys, with the internal-node inline keys providing most of the win [https://raw.githubusercontent.com/XiangpengHao/bbtree/main/design/04-inline-key.md]. Relevant for id or path lookup in the tree, less so for paint order.

9. **Fence keys with u16 prefix length on every node.** Saves per-node key bytes and gives the prefix-stripped inline window. Source: bbtree 05-fence-key.md [https://raw.githubusercontent.com/XiangpengHao/bbtree/main/design/05-fence-key.md]. Relevant if the tree is ever keyed by a path or string ID.

10. **Eytzinger or S-tree implicit layout for any sorted index of node IDs.** 8x to 15x over std::lower_bound in the Algorithmica benchmarks, with no pointers [https://en.algorithmica.org/hpc/data-structures/s-tree/ ; https://en.algorithmica.org/hpc/data-structures/binary-search/]. Ranks lower because a UI tree is mostly traversed, not searched by key.

11. **Parent-vector and sibling-vector representation with scan-based offset computation.** Data-parallel tree computation with gather and scan; maps to SIMD and GPU. Sources: Lochbaum's Co-dfns comparison [https://mlochbaum.github.io/BQN/implementation/codfns.html]; Blelloch scan [https://www.cs.cmu.edu/~guyb/papers/Ble93.pdf, from memory]. Ranks mid because fixed-point iteration for ancestor-dependent values costs more than level order.

12. **Level-ordered SoA hierarchy with per-level parallel compose.** The Bevy and DOTS pattern: parent column plus per-level SIMD transform. Shallow, wide UI trees make it efficient. Sources: Bevy transform propagation [https://docs.rs/crate/bevy_transform/0.17.1/source/src/systems.rs]; general level-order argument. Ranks mid because the published numbers for Bevy and DOTS were not gathered in this session.

13. **CSB+-style node groups (contiguous children, one pointer per node).** Halves child pointer bytes and gives contiguous child access. Source: Rao and Ross, SIGMOD 2000 [https://www.paperdigest.org/certificate/f084c5de797efd0a]. Useful as a pattern for a node with many children; less useful for a tree where every node is a separate allocation.

14. **Balanced-parentheses plus rmM-tree for a compact persistent snapshot.** About 2 bits per node with O(log n) or O(1) navigation for depth and subtree size. Source: Navarro and Sadakane, TALG 2014 [https://users.dcc.uchile.cl/~gnavarro/ps/talg12.pdf]. Not a live structure; a serialization and sync format.

15. **Euler-tour tree for dynamic forests and subtree moves.** O(log n) subtree move and split, with poor locality. Ranked last because the render path does not need it; keep it only if the tree must support huge dynamic reparenting. Source: from memory; not verified in this session.

**Summary ranking rationale.** Items 1 through 3 together form the core: a preorder array with skip indices, a PMA or order labels for edits, and SoA payload columns for SIMD. Items 4 through 7 make the traversal and dirty-tracking cheap. Items 8 through 15 are reusable local tricks or specialized alternatives.

---

## 9. Source list

Read in this session (fetched and quoted):
- https://en.algorithmica.org/hpc/data-structures/s-tree/
- https://en.algorithmica.org/hpc/data-structures/binary-search/
- https://github.com/XiangpengHao/bbtree/tree/main/design (listing only)
- https://raw.githubusercontent.com/XiangpengHao/bbtree/main/design/04-inline-key.md
- https://raw.githubusercontent.com/XiangpengHao/bbtree/main/design/05-fence-key.md
- https://raw.githubusercontent.com/XiangpengHao/bbtree/main/design/06-cost-model.md
- https://en.wikipedia.org/wiki/Order-maintenance_problem
- https://raw.githubusercontent.com/RenderKit/embree/master/README.md
- https://arxiv.org/abs/2411.10659 and https://arxiv.org/html/2411.10659v8
- https://arxiv.org/abs/1402.6407
- https://mlochbaum.github.io/BQN/implementation/codfns.html

Seen in search results only (verify before quoting numbers):
- https://www.vldb.org/dblp/db/conf/vldb/RaoR99.html ; https://courses.cs.duke.edu/spring03/cps216/papers/rao-ross-1999.pdf
- https://www.paperdigest.org/certificate/f084c5de797efd0a
- https://hgpu.org/?p=1493 ; https://hgpu.org/?p=6702 ; https://www.doi.org/10.1145/1807167.1807206
- https://dblp.uni-trier.de/pid/97/2712.html ; https://typeset.io/papers/an-adaptive-packed-memory-array-4ce30n6ysv
- https://dblp.dagstuhl.de/rec/journals/siamcomp/BenderDF05.html ; https://erikdemaine.org/papers/CacheObliviousBTrees_SICOMP/
- https://cs.uwaterloo.ca/~imunro/cs840/ProjectPapers/SODA10_013_sadakanek.pdf ; https://users.dcc.uchile.cl/~gnavarro/ps/talg12.pdf ; https://www.arxiv.org/pdf/1601.06939
- https://aplwiki.com/wiki/Co-dfns ; https://static.aminer.org/pdf/20170130/pdfs/pldi/3iqdysbvwhle4fjppexd6lu5acukktv2.pdf
- https://docs.rs/crate/bevy_transform/0.17.1/source/src/systems.rs ; https://docs.rs/crate/bevy_transform/0.19.0/source/src/systems.rs ; https://docs.rs/crate/bevy_app/0.19.0/source/src/propagate.rs ; https://doc.qu1x.dev/bevy_trackball/bevy_transform/systems/fn.mark_dirty_trees.html
- https://folk.idi.ntnu.no/elster/tdt24/tdt24-f09/holger-summary.pdf
- https://pldi25.sigplan.org/details/pldi-2025-papers/73/Spineless-Traversal-for-Layout-Invalidation

Cited from memory, primary not opened (verify before use):
- https://db.in.tum.de/~leis/papers/ART.pdf (Leis et al., ICDE 2013; the PDF returned binary)
- Masstree (EuroSys 2012, https://dl.acm.org/doi/10.1145/2213836.2213910 returned 403)
- Bw-tree (ICDE 2013); Bf-tree (VLDB 2024; the vldb.org index URL I fetched did not contain it)
- Blelloch scan, https://www.cs.cmu.edu/~guyb/papers/Ble93.pdf (binary)
- bbtree 07-node-plan.md and 12-batch-api.md (not fetched)
- hibitset README (404 on the URL tried)
- Wald, Benthin, Slusallek 2008 wide-BVH paper (not located)
- Rewired-memory and Tera-PMA papers (not located)

## 10. Open items

The following would need one more pass with a PDF text extractor (for example `pdftotext`) or an authenticated fetch: the exact ART node sizes and SSE code, Masstree's permutation encoding and throughput, the Bw-tree mapping table, Bf-tree's mini-page numbers, the Blelloch work/depth proofs (which are standard but not quoted here), the FAST throughput figures from the paper itself, the Roaring container thresholds, and the hibitset branching factor. The numbers in Sections 1.5, 3.2 (beyond the TODS abstract), 5.1, 6.2 and 7.1 should not be quoted in external material until those are verified.
