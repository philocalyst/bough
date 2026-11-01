# bbtree → GUI: first-hand synthesis (read 2026-11-01)

Source: ~/projects/_ref/bbtree/design (14-blogpost, 06-cost-model, 07-node-plan, 12-batch-api, 03-decisions D9–D60).

## Mechanisms that matter

1. **Cost order**: chained cache lines (~100 ns) ≫ instructions (~0.5 ns) ≫ coherence. A *saved level* on warm upper
   levels saves instructions, not DRAM time (lesson 2). Charge SIMD node search **at capacity, not fill** (lesson 1).
2. **Pages**: power-of-two, self-aligned nodes (1 KiB leaves, 512 B internals). Never cross a 4 KiB page → 1 TLB
   entry, HW prefetcher streams the node. Freed low address bits carry the node **kind in the pointer** (D52):
   kind decided once per step from the edge tag, before the child is read ("branch early, loop late", D28).
3. **Summaries (inline keys) are the filter**: 2/4/8-byte order-preserving windows in one column, SIMD *count*
   (compare all, popcount = child index), branchless. Padding slots hold the max value so the count over the full
   capacity is exact without masking (invariant 4, D56). Scanning full keys instead was 2.7× slower (rejected).
4. **Implicit context from ancestors** (D26/D50): a node never stores the prefix its fences share — it is implied
   by the path. Only `prefix_len` is stored; windows are taken *after* the shared part. The root's "fences" live
   beside the root pointer.
5. **SoA inside a node** (D32/D49): internal nodes keep only summaries + edges (no full keys) → 2× fanout per byte.
6. **No parent pointers** (D11): splits/merges never touch moved children; iterators carry an inline node stack.
7. **Type-stable memory** (09): freed nodes go to a map-owned free list, never back to the allocator; a node's
   version only grows (free bumps, reuse bumps) → optimistic readers just retry; no epochs, no segfaults.
   Inner nodes optimistic, leaves latched; `Copy` values copied out seqlock-style (D57).
8. **Plans fold at monomorphization** (07/D47): "derive, don't pass"; plan-holding dispatchers `#[inline(always)]`;
   keep enums tiny or LLVM stops inlining (a third enum variant → function-pointer call, +142 instr/lookup).
   Zero-sized call-site markers keep separate instantiations from de-inlining each other (12).
9. **Batching**: a chain of dependent misses can only be hidden by interleaving independent chains. **Level-by-level**
   (all lanes at one depth, plain loops over `[Lane; N]`) beat async lanes by ~14 ns/key; async kept for code shape.
10. **Last-leaf append cache** (D45): ascending inserts 31 → 5.3 ns. Streaming appends should never descend.
11. **Bulk build of full nodes** (D25) and **position-independent file maps** (D58: edges = offsets from the node,
    level order, compacted) → a frozen tree is a relocatable blob read in place.
12. Sorted beats unsorted leaves everywhere except random insert (D16). End-biased splits halve sequential memory.
13. Measure **instructions** (deterministic) and lines-from-memory; read the asm; codegen flips results.

## Port to a GUI (candidate mappings, to be tested)

- **Paint-order sequence B+tree**: leaves = 1 KiB pages of node records in DFS/paint order; internal nodes =
  per-child *summaries* (quantized bounds, damage bits, depth min/max, counts). Paint = streaming leaf scan;
  damage/hit queries = SIMD descent over summaries. Subtree = contiguous run → mount/unmount/reorder = rope splice.
- **Inline keys ↔ quantized relative bounds**: child boxes as i16/u16 offsets relative to the parent frame (the
  "prefix" implied by ancestors), 32 boxes per SIMD pass → intersect/contain masks + popcount.
- **Depth min/max per child (rmM-tree)** in internal summaries → parent / next-sibling / subtree-end by SIMD descent,
  no parent pointers.
- **Kind in pointer tag** → node kind (leaf/internal, or record kind) decided before load.
- **Type-stable pages + versions** → stale handles detected for free; render thread reads optimistically.
- **Level-synchronous batch** for layout/damage propagation across all dirty nodes of a depth.
- **Frozen pages** for page transitions: compact a departed page into a position-independent read-only image.
