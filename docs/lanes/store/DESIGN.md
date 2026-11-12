# Lane `store`: what the design became, and why

State: working notes of the lane that executes `docs/lanes/core/STORE-R6.md`. Each section says where the
build follows the lead's design, where it departs, and the reason. Numbers are from
`crates/bough/examples/redescribe_cost.rs` and `signal_cost.rs` unless a source is given.

## 0. The order of work (changed by the user's priorities)

The first judge of the storage is **what one signal write costs end to end** (lead, late 2026-11-11: a colour
write under 100 instructions, a numeric label write under 400), then mount, heap and subtree moves. The
chain a wire walks (`Wire.node -> Node -> Run -> Instance`, and `Node.handle -> htab -> page`) is the same
chain mount and re-description walk, so the stages are the same; their order inside the lane is:

1. type-stable storage, one index space, no per-node heap object (stages 0 and 1);
2. the signal path: settle proportional to what was written, the appliers' chains, the turn's fixed cost;
3. hot/cold columns (stage 2) and the layout cards absorbed from the stopped `layout` lane;
4. side pages on the core's `ColumnPlan` (stage 3, when lane `core` lands the trait), fixed-point geometry;
5. parallel bulk mount (stage 4).

## 1. Storage that never moves: one reserved block, not chunks (departure)

STORE-R6 asks for "chunked, type-stable storage (power-of-two chunks, a free list, no reallocation)". Built
instead: **every façade table is a `bough_mem::Col` carved from one `mmap`ed block** sized from `Caps`
(`rt::StoreCaps`), exactly as the GPU-visible columns already are.

* Reason: a chunk table costs a dependent load on every access (`chunks[i >> k][i & m]`); a carved column is
  `base + i * size`, one load, and the base is a field of `Rt`. bbtree's rule is to count chained lines first.
* It is type-stable in the sense that matters (an entry's address never changes while the window lives) and
  nothing is copied when the app grows; untouched capacity is not resident.
* The block must be `mmap`, not the allocator's zeroed memory: `alloc_zeroed` of a block this size (with a
  16 KiB alignment it is `posix_memalign` + `bzero`) is cleared eagerly. Measured: an empty App went from
  13.3 M to 428 M instructions with the allocator, and is 13.3 M with `mmap`.
* Cost: a hard capacity (twice the records the page column can hold). The page column already has one; when
  lane `memory`/`core` make columns growable the same mechanism applies here (reserve more address space,
  it is free).
* Honest accounting: carved bytes are not the allocator's, so the harness reports "allocator live bytes +
  touched column bytes" (`HEAP=1`, last line), and `Rt::store_ledger` lists both kinds.

`store::Spans` is the one arena for variable-length element state (power-of-two word spans, free lists
threaded through the freed spans themselves): bar series today; tapes, line windows and release lists next.

## 2. One index space: node id = layout id = record handle index (as designed)

A node's index is its record handle's index (`Rt::alloc_node` allocates the handle; `Rt::node_at(h)` for an
element that made its handle first). `LInfo.rtn`, `Node.lnode`, `Node.serial` and `Node.live` are gone:

* validity is the handle's (`Rt::node_live`: the node's stored handle is the live handle of that index), so
  a reused index cannot be mistaken for its previous occupant (what `serial` did for `select`);
* a static element (no wire, mount-once owner) keeps **no node**: its handle goes to a spare list for the
  next `alloc_node`, its record carries handle 0 as before (the frame-path scenes mount thousands of these:
  giving them handles would put them in every page shift's fix-up);
* **a scope owns resources, not nodes**: `Scope.texts` (text states), `Scope.insts` (single instances),
  `Scope.runs` (multi-instance runs), `Scope.series`. Releasing a subtree never reads a node, so a handle
  index can be reissued by the core the moment its record is removed (the old order, where a deferred
  `free_node` could run after the index was reused, cannot happen);
* `Run` exists only for an element that owns more than one instance (bars, lanes, glyph-instance text);
  a quad's node holds its instance index (`Node.inst`), one load from the node.

Found while unifying: `input.rs` searched the scope's node list for "the node whose handle is this
record's" (linear, three places) and a tooltip replaced a dynamic element's handle with a fresh one. Both
are now "the node is the record's handle index".

## 3. Layout children: an array of ids in the span arena (a chain was tried and measured out)

STORE-R6: "a container's layout children are its direct children in the depth column: an iteration over the
page, not a heap `Vec` per container". Built: the `Vec` is gone; a container's children are **an array of
element ids in a span of one word arena** (`DNode::kids`, `store::Spans`: power-of-two spans, never moved).

* First attempt (stage 0a, commit eb156f9): a chain through the nodes (`first`/`last`/`next`), with a sibling
  cursor in the re-description walk. The Opus review (IDEAS-OPUS A12b) warned it makes child iteration a
  chain of dependent loads. **Measured** at 4,096 children, per child, reading what a layout visit reads
  first: chain 3.4 ns in mount order and 6.6 ns after churn; array of ids 1.2 ns and 1.0 ns (same instruction
  count, 9 to 10 per child: the difference is the dependency). The chain is gone; the cursor went with it.
* Why not the depth-column scan yet: the layout tree is not exactly the record tree (a group root nested
  under a port, and record children that are no layout node), and the scan needs a cursor API in bough-core
  whose page paths lane `core` is rewriting. When side pages land the array becomes a bit column ("is a
  layout child of its record parent") plus the scan; the engine's interface (`kids(n) -> &[u32]`) hides which.

## 4. The signal path (in progress)

Measured before any change (BASELINE.md): an idle turn costs 14 instructions per wired cell of the owner
(`settle` scans every cell for its QUEUED bit), a colour write ~240 on top.

* **Settle proportional to writes** is the lead's (the wire queue in `signal.rs` / `rt.rs`: `Wire`,
  `settle`, `settle_cls`, `run_head`, `Signal::deref_mut` are not touched by this lane). What this lane
  found while reading that path, for the lead: the scan is O(cells) only for the root owner (rows and
  entities are settled per edited owner), so the queue needs to cover class 0 only; the root owner's memory
  is alive at every settle, a row's or a dropped list's may not be between a write and the settle, so a
  queue entry must not be dereferenced for other classes; a write must verify the wire record's head
  address is its own before linking (a moved signal), and must not leave QUEUED set without being linked
  (with a scan a stale QUEUED costs one extra run; with a queue it would block every later link).
* **The applier's chain**: `Wire.node` is the handle index; the node's instance index is in the node; the
  record is `htab[node]`. Next: hot columns so a colour write loads one node line, and the turn's fixed
  cost (`sync`, `admit`, `effects`, `lay_flush` guards).
* **Wired operands seal** (a row bound to signals is described by which cells it is bound to), so a
  re-describing owner steps over signal-driven rows instead of walking them.

## 5. Layout cards absorbed from lane `layout`

* (a) A subtree that only moved: the lane's wip (`81dca613ea`) is a second placement path beside the full
  one. Here it will be the only path's degenerate case: geometry is a column, a moved subtree is an add over
  the records' range (stage 3), and text keeps its line windows because nothing about them depends on the
  position. Its extension of `tests/seals.rs` (a width change, nesting) is taken as the gate.
* (b) Cold layout of rows: ~9,000 of the ~24,000 instructions per mounted row are cephal's cold flex pass
  (3 nodes). Two mechanisms, both general: a sized leaf's size is node data, not a style per distinct width
  (today every label width interns its own `Style`); and identical subtrees share one computed layout
  (keyed by the subtree's shape: styles and measured sizes in preorder, and the question asked).
