# 03 Reactivity cores and data-oriented memory for a Rust GUI update path

Status and method. This report is partial. Each claim is tagged:

- **[SRC]** read from a file in this run. The local path is given.
- **[DOC]** read from a fetched web page in this run.
- **[MEM]** from prior knowledge, not fetched in this run. Verify before relying on it.

Local copies live under `/Users/mileswirht/projects/bough/research/src/`:

- `src/solid-signals/package/dist/dev.js` (npm `@solidjs/signals@2.0.0-rc.0`, fetched via the registry; `npm` is not installed on this machine, so the tarball was fetched with curl from `https://registry.npmjs.org/@solidjs/signals/-/signals-2.0.0-rc.0.tgz`)
- `src/raw/reactivity_sources.js` and `src/raw/client_runtime.js` (Svelte 5, `packages/svelte/src/internal/client/...` on `main`)
- `src/raw/alien_system.ts` and `src/raw/alien_index.ts` (stackblitz/alien-signals, `master`)
- `src/raw/src_index.ts` (preactjs/signals, `packages/core/src/index.ts`)
- `src/raw/computed_arc_memo.rs` (leptos-rs/leptos, `reactive_graph/src/computed/arc_memo.rs`)
- `src/raw/xilem_view.rs` (linebender/xilem, `xilem_core/src/view.rs`)
- `src/raw/master_README.md` (salsa-rs/salsa README; read as a pointer only, not analyzed)
- Ripple: `/Users/mileswirht/projects/_ref/ripple/packages/ripple/src/runtime/internal/client/runtime.js` (repo HEAD `fa847e61d8da0e4eda4ad7384ab7ac7645d46253`, dated 2026-10-29)

Two gaps need stating up front:

1. The Solid blog post `https://www.solidjs.com/blog/async-solid-fetch-high-block-low` returned an empty body to WebFetch, and a curl fetch returned an HTML shell with no article text. **The async model below is therefore inferred from `dev.js`, not from the post.** Treat the "fetch high, block low" summary in section A1.4 as unverified.
2. Benchmarks were not run and no published numbers were fetched. Every "memory per node" figure below is a field count, not a measured byte count.

---

## Part A: Reactivity cores

### A1. Solid 2.0 (`@solidjs/signals`)

**A1.1 Node representation [SRC `dev.js`].**

Flags are a bitfield on `_flags` (`dev.js` lines 61-73): `REACTIVE_CHECK=1<<0`, `DIRTY=1<<1`, `RECOMPUTING_DEPS=1<<2`, `IN_HEAP=1<<3`, `IN_HEAP_HEIGHT=1<<4`, `ZOMBIE=1<<5`, `DISPOSED=1<<6`, `OPTIMISTIC_DIRTY=1<<7`, `SNAPSHOT_STALE=1<<8`, `LAZY=1<<9`, `MANUAL_WRITE=1<<10`, `REASK=1<<11`. Static configuration lives in a separate `_config` word (`CONFIG_OWNED_WRITE`, `CONFIG_AUTO_DISPOSE`, `CONFIG_SYNC`, and so on, lines 75-84).

A signal is a plain object built by `signal()` (`dev.js` 2853-2882) with these fields: `_equals`, `_config`, `_value`, `_pendingValue` (sentinel `NOT_PENDING` when nothing is staged), `_subs`, `_subsTail`, `_time` (a clock stamp), `_firewall`, `_nextChild`. A computed (`computed()` at 2707) adds `_deps`, `_depsTail`, `_depGen`, `_height`, `_fn`, `_parent`, `_firstChild`, the heap links (`_prevHeap`, `_nextHeap`), and a status word (`_statusFlags`: PENDING, ERROR, UNINITIALIZED).

**A1.2 Edge representation [SRC `dev.js` `link()` 1675-1737].** The edge is one object shared by two intrusive doubly-linked lists:

```
{ _dep, _sub, _nextDep, _prevSub, _nextSub, _gen, _pendingObserver }
```

`_nextDep` threads the subscriber's dependency list. `_prevSub`/`_nextSub` thread the dependency's subscriber list. `_subsTail` and `_depsTail` give O(1) append. This is the alien-signals design (Solid's comment at line 1674 credits `https://github.com/stackblitz/alien-signals/blob/v2.0.3/src/system.ts#L52`). Seven fields per edge, all in one JS object.

The extra fields are `_gen` (a per-recompute generation stamp, `sub._depGen`) and `_pendingObserver` (a bit marking an `isPending()` probe read versus a value read).

**A1.3 Write propagation [SRC `dev.js` `setSignal` 3154-3207, `insertSubs` 942-965, `enqueueSub` 1185-1197].**

1. `setSignal` checks equality with the staged or committed value. If unchanged it returns. Otherwise it stages `_pendingValue`, stamps `_time = clock`, calls `insertSubs(el)`, and calls `schedule()`.
2. `insertSubs` walks only the **direct** subscribers. For each it clears `REASK` when `reaskArmed`, flags snapshot-stale subscribers, and calls `enqueueSub(s._sub)`.
3. `enqueueSub` routes `EFFECT_TRACKED` nodes straight to an effect queue. Everything else goes to `insertIntoHeap(node, queue)` after pulling the heap cursor down: `if (queue._min > node._height) queue._min = node._height`.

Transitive invalidation does not happen at write time. A subscriber's own subscribers are only enqueued when the subscriber recomputes and its value changes, which calls `insertSubs` again. This is value-change push through the heap, not mark-everything-then-pull. The one exception is the read-time `markHeap`/`markNode` pair (lines 1256-1279). Those walk a heap, set `CHECK` on the pending nodes, and set `CHECK` on their subscribers. They are used for mid-tick reads so that a pull can see pending work.

**A1.4 Ordering and glitch freedom [SRC `dev.js` heap 1198-1300].**

The heap is an array of buckets indexed by height. Each queue has `_heap` (initialized to `new Array(2000)`, line 421), plus cursors `_min` and `_max`. Each bucket is an intrusive doubly-linked list through `_nextHeap` and `_prevHeap`, with the head's `_prevHeap` pointing to the tail, so append is O(1).

`actualInsertIntoHeap` sets a node's height to at least `parent._height + 1` (or the parent computed's height for a root-owned node), then appends it to bucket `height`. `runHeap` (1280-1290) sweeps buckets from `_min` upward:

```
for (heap._min = 0; heap._min <= heap._max; heap._min++) {
  while (el = heap._heap[heap._min]) {
    if (el._flags & IN_HEAP) recompute(el); else adjustHeight(el, heap);
  }
}
```

A dependency always has a lower height than its subscriber, so a subscriber is recomputed only after its dependencies have settled, which gives glitch freedom with no topological sort at runtime.

Dynamic height changes are handled by `adjustHeight` (1292-1312). When a node's dependencies have grown taller, it re-inserts itself at the new height. A write that dirties a lower bucket while the sweep is above it pulls `_min` back down, so the sweep does not skip it.

Heights are not a global topological numbering. They are maintained lazily per node, which is why a deep static chain pays one height write per node per structural change, not per write.

**A1.5 Dynamic dependencies [SRC `dev.js` `recompute` 2422-2470, `link` 1675, `trimStaleDeps` 1653].**

`recompute` sets `_depsTail = null`, increments `_depGen`, and sets `_flags = RECOMPUTING_DEPS`. Each read calls `link(dep, sub)`, which:

1. Early-returns if `_depsTail._dep === dep` (a repeat read in order).
2. Reuses the link at `nextDep` (the previous pass's next dependency) if it matches, moving `_depsTail` forward. This is the common steady-state path and allocates nothing.
3. Otherwise it checks `dep._subsTail` against `_gen` for an in-order repeat from this pass.
4. Only if none of those match does it allocate a new link object, inserting it at `_depsTail`.

After the compute, `trimStaleDeps` (1653) unlinks every link after `_depsTail`. `unlinkSubs` (1625) calls `dep._unobserved()` when a dependency loses its last subscriber. For auto-dispose nodes it calls `unobserved(c)`, which removes the node from the heap, unlinks all its deps, and disposes children (1664-1673). A pending node is exempt (line 1647-1648).

Steady state (same dependencies, same order): zero allocations per recompute, because every `link()` call reuses an existing object. Allocation happens only when the dependency set grows.

**A1.6 Pending, async, and transitions [SRC `dev.js`; the async semantics are inferred, not confirmed by the blog].**

Pending state is stored in `_pendingValue` (staged, not yet committed) and `_statusFlags` (STATUS_PENDING). Transitions are batches (`createBatch`, 518; `commitPendingNode`, 967). A write inside a transition stages `_pendingValue` and queues the node in `currentBatch._pendingNodes` (`queuePendingNode`, 932). The committed value is replaced only when the batch commits.

Recomputes that belong to a transition that has not committed go to a separate `zombieQueue` (`queueFor`, 1177-1179). `flush()` runs the dirty heap first, then drains the zombie heap for parked transitions, cancelling zombies when the parking batch is the transition itself (line 769-773). Pending reads and the `isPending` probe are recorded by `_pendingObserver` on the link (1685-1690).

I can state from the code that the staging model exists, that transitions hold recomputes in a parallel heap, and that `latest()` reads `_latestValueComputed` (line 2991-3010). I cannot state the user-facing "fetch high, block low" rule from the blog, because it was not retrieved. The closest the code comes to that rule is that pending values are staged rather than committed while a transition is open, which is the "block low" half.

**A1.7 Memory per node and edge [SRC field counts; not measured].** A signal has about 9 fields plus optional debug fields (`_name`, `_internal`). A computed has about 25 fields. Each edge has 7 fields. In V8 that is roughly one object per edge plus the heap-link fields on nodes. Byte counts require a heap snapshot, which this run did not do.

**A1.8 Benchmarks [MEM].** Not retrieved in this run. Check the `solid-js` and `js-framework-benchmark` repositories before quoting any numbers.

---

### A2. Ripple (`trueadm/ripple`)

**A2.1 Model [SRC `runtime.js`].** Ripple has no signal-graph-as-such. It has three kinds of reactive object:

- **Tracked values** (`TrackedValue`, constructed in `tracked()` at line 690). Fields include `__v` (value), `c` (a clock stamp), `b` (owning block), `sb` (head of the subscriber list), `a` (accessors), `f` (flags), and `o` (old value, held only while teardown needs it).
- **Derived values** (`derived()` at 708). Same shape plus a `d` (dependency list) and a `blocks` list.
- **Blocks** (`Block`, typed in `blocks.js`, 664 lines). A block is a node in a runtime tree with an integer id `i` assigned at creation. Blocks carry flags `f` including `SCHEDULED`, `DESTROYED`, `EFFECT_BLOCK`, `PRE_EFFECT_BLOCK`, `BRANCH_BLOCK`, and `ITEM_BLOCK` (`constants.js`).

**A2.2 Edges [SRC `runtime.js` 937-1025].** A `Dependency` is `{ c, t, n, r, sp, sn }`: stamp, tracked value, next dependency, reaction, and prev and next subscriber pointers. `create_dependency` recycles an existing dependency at the reaction's cursor (`reaction.d`) when one is available, so a stable block does not reallocate on rerun. This is the same recycling idea as Solid's `_depsTail` reuse.

**A2.3 Write propagation [SRC `runtime.js` `set` 1498-1538, `mark_subscribers` 1102-1129].** `set` compares old and new value, updates `__v`, takes a fresh clock with `increment_clock()`, and calls `mark_subscribers`. `mark_subscribers` walks the subscriber list:

- For a derived reader, it recurses into the derived's own subscribers first. A derived whose owner is destroyed and that nothing reads any more is unlinked at that point.
- For a destroyed block, it unlinks the dependency.
- Otherwise it calls `schedule_update(block)`.

So propagation is recursive through deriveds and stops at blocks. It does not compute heights.

**A2.4 Scheduling and ordering [SRC `runtime.js` 1383-1402, 1206-1262].** `schedule_update` pushes the block into `queue.blocks`, sets `SCHEDULED`, and tracks whether pushes arrived in ascending `block.i` order via `queue.last`. In microtask mode it queues one flush. `flush_queue` sorts only if the array is unsorted (`by_block_id`), then runs it in place. If the queue mixes pre-effects, effects, and render blocks, `run_phases` runs three passes in that order.

Glitch freedom comes from two things. Block ids are creation order, and a parent block is created before its children, so parents run first. Deriveds are pulled on read, so a block that reads a derived always sees the derived's current value.

**A2.5 Dynamic dependencies and pulls [SRC `runtime.js` 259-275, 1072-1100, 1131-1161].** `update_derived` recomputes a derived only when `is_tracking_dirty(computed.d)` is true. That function walks the dependency list, calls `update_derived` on any derived dependency (recursive pull), and returns true as soon as a dependency's `c` is greater than the derived's recorded `tracking.c`. `is_block_dirty` does the same for blocks, with extra checks for `BLOCK_HAS_RUN` and `activity_needs_run`.

So the Ripple algorithm is push-mark (subscribers scheduled) plus pull-validate (clock comparison at flush or read). It is closer to Svelte 4's version-stamp model than to Solid's height-ordered heap, and it does not need heights because blocks are already in topological creation order.

**A2.6 The compiler output [SRC `packages/ripple/tests/hydration/compiled/client/activity-browser.js`].** The compiler itself is not in this repo. `packages/ripple/src/compiler/index.js` is 5 lines and the real compiler is the external `@tsrx/core` package (`packages/ripple/package.json` line 101). The compiled output shows what the runtime sees:

- Static HTML becomes a `_$_.template(html, flags, id)` call that clones a fragment. There is no virtual DOM.
- Each dynamic expression becomes `_$_.expression(node, () => ...)`, which creates a block whose effect re-runs only that expression.
- Lists become `_$_.for(anchor, getArray, renderItem, flags)`.
- Components become `_$_.render_component(Component, __anchor, props)`.
- Every read inside a component body is wrapped in `_$_.with_scope(__block, () => ...)`, which sets the active block for dependency capture.

**A2.7 What is novel versus Svelte 5 runes [SRC both repos].**

- Ripple's reactive unit is the **block**, a runtime tree node with a creation-order id, and scheduling sorts by that id. Svelte 5 schedules **effects** (`mark_reactions` in `reactivity_sources.js` 356-420 calls `schedule_effect` on non-derived reactions), and uses a per-source `wv` write version and per-reaction `DIRTY`/`MAYBE_DIRTY` flags.
- Ripple's dirty check compares clock stamps on dependency edges (`tracking.c` against `tracked.c`). Svelte 5 marks reactions with flags and does not re-check versions on every flush.
- Ripple's `flush_queue` runs three phases in one array. I did not find an equivalent in the Svelte source fetched here. The Svelte file set I grepped did not include a `check_dirtiness` function, so I cannot make a precise comparison of its check path.

The honest summary: Ripple is a block-tree runtime with clock-stamped pulls. Its novelty is in putting the block tree at the centre of scheduling. I did not verify any claim about performance.

**A2.8 Scheduling for static graphs.** Ripple's compiled output still allocates a `Block` for each `expression` and `for` site. A fully static component with no expressions would allocate no blocks, but this run did not verify that.

---

### A3. alien-signals, Preact signals, Reactively, r3, Svelte 5

**A3.1 alien-signals [SRC `alien_system.ts`].** The `Link` interface is `{ version, dep, sub, prevSub, nextSub, prevDep, nextDep }`. It is the same shape Solid uses. Flags are `Mutable=1, Watching=2, RecursedCheck=4, Recursed=8, Dirty=16, Pending=32`.

- `link(dep, sub, version)` (around line 120 of the file) dedupes repeated reads in a single pass with `prevSub.version === version`. It reuses `sub.deps` nodes in place by comparing `nextDep.dep === dep`.
- `unlink` splices from both lists and calls `unwatched(dep)` when `dep.subs` becomes empty.
- `propagate(link, innerWrite)` (lines ~138-200) walks subscribers iteratively with an explicit `stack` of `Link`s, not recursion. It marks subscribers `Pending`, and if the subscriber is a `Mutable` (computed) it descends into its subscribers. `notify` is called only on `Watching` nodes.
- `checkDirty` is the pull side. It walks dependencies, recomputes deriveds that are `Pending`, and promotes to `Dirty` only when a dep really changed.

This is the **push-pull** algorithm. Writes mark the graph `Pending` in one walk, then each read or effect calls `checkDirty` to confirm whether a recompute is needed. Glitch freedom comes from the pull: a derived always recomputes against already-validated deps.

**A3.2 Preact signals [SRC `src_index.ts`].** A `Signal` has `_version` (number), `_node` (an optional cached `Node`), and `_targets`. A `Node` (around line 17-32) holds `_version`, `_source`, `_prevSource`, `_nextSource`, and `_rollbackNode`. The key detail is node recycling.

- When a signal is read inside an evaluation context (`signal._node` lookup, around line 213), the code reuses the cached `Node` if its `_version === -1`, which marks it as recyclable (line 250).
- A new node is allocated only when no cached node exists. Nodes are attached to the evaluation context's `_sources` list.
- `_refresh()` (line 381) and `snapshots` (lines 149-200) implement batching and rollback of `_node` between batches.

The version counter lets the subscriber compare `node._version` against `source._version` to decide whether a recompute is needed, the same idea as Ripple's clocks.

**A3.3 Reactively and r3 [MEM].** Not fetched in this run. From memory: Reactively uses `CacheClean`, `CacheCheck`, and `CacheDirty` colours, with a "maybe dirty" check and a heap of pending effects. r3 (Milo) uses a `DirtyState` on each node and an explicit check phase. Verify against the repositories before citing.

**A3.4 Svelte 5 runes [SRC `reactivity_sources.js`, `client_runtime.js`].**

- A `source` (`reactivity_sources.js` 73-88) is `{ f, v, reactions, equals, rv, wv }`. `wv` is the write version, `rv` is the read version.
- `set` (150-160) calls `internal_set`, which records the old value in `old_values` on the first write per flush, calls `increment_write_version()` (`client_runtime.js` 144-146, starting at 1), and calls `mark_reactions(source, DIRTY, ...)`.
- `mark_reactions` (356-420) sets the reaction's status to `DIRTY` (direct) or `MAYBE_DIRTY` (through a derived, recursing into the derived's reactions). Effects are scheduled with `schedule_effect`. A `seen` Set is created only when the counted dependencies exceed 100000 (line 372), to guard against cycles.
- `effect.wv = write_version` (`client_runtime.js` line 481) stamps an effect at run time. A dep's `wv` is set at line 488.

I did not find the string "unowned" in the fetched files, so the "unowned deriveds" claim in the brief is **unverified in this snapshot**. The Svelte version in `main` may differ from the release the brief refers to.

**A3.5 Comparison table (verified parts only).**

| System | Edge object | Propagation | Ordering | Dirty check |
|---|---|---|---|---|
| Solid 2.0 (`dev.js`) | shared object, 2 lists, `_gen` | push to direct subs, heap by height | heights, bucketed | `CHECK`/`DIRTY` flags, height sweep |
| alien-signals (`alien_system.ts`) | shared object, 2 lists, `version` | iterative `propagate` with explicit stack | none (pull) | `checkDirty` walk |
| Preact (`src_index.ts`) | `Node` per (source, eval) pair, recycled | version compare | none (pull) | `_version` compare |
| Svelte 5 (`reactivity_sources.js`) | `reactions` arrays on source, `deps` on reaction | `mark_reactions` recursion | effect scheduling order | `DIRTY`/`MAYBE_DIRTY` flags, `wv` |
| Ripple (`runtime.js`) | `Dependency` object, 2 lists, `c` clock | recursive `mark_subscribers` | block id (creation order) | clock compare (`is_tracking_dirty`) |

---

### A4. Leptos `reactive_graph` and Sycamore (Rust)

**A4.1 Leptos [SRC `computed_arc_memo.rs`].** The memo holds `inner: Arc<MemoInner<T, S>>` (line 97), built with `Arc::new_cyclic` (line 157) so the memo can hold a `Weak` to itself. Its sources and subscribers are stored as `Weak<dyn Source + Send + Sync>` and `Weak<dyn Subscriber + Send + Sync>` (lines 272-304). `S` defaults to `SyncStorage`.

So in this version, the memo's node storage is `Arc` plus `Weak`, not a slotmap. Reference counting and weak upgrades are on the hot path. The parameter `S` lets the author swap storage (for example to a thread-local storage) but the default is thread-safe `Arc`.

I did not read the arena-based `ArenaItem` or the owner module in this run. Claims about slotmap storage are **unverified**.

**A4.2 Sycamore [MEM].** Not fetched. From memory, Sycamore v0.9 uses an arena of signal nodes with `Copy` handles (generational index), which is the "handles, not pointers" design. Verify against `sycamore-reactive/src` before citing.

**A4.3 The avoid-Rc-RefCell question.** From the Leptos file alone, `Arc` plus `Weak` on every node is what Leptos does. A `Copy` handle over an arena (Sycamore's direction, [MEM]) removes the reference-count traffic at the cost of an explicit disposal story. This matters more for the target design than the exact library implementation.

---

### A5. Incremental computation theory

I did not fetch the sources in this section. Each entry is [MEM] unless noted and must be verified before citation. The URL is the canonical project page.

- **Adapton** (`https://github.com/Adapton/adapton.rust`). Demanded computation graph: nodes are "thunks" memoised by name, recomputed on demand, with dirty-marking on writes. Dependency edges are recorded per thunk. Dynamic dependencies are the default, not an add-on.
- **Acar's self-adjusting computation** (`https://www.umut-acar.org/self-adjusting-computation`, and the thesis at CMU). Three ingredients: an order-maintenance structure for the trace, memoisation of applications, and change propagation along trace edges. The order-maintenance list gives O(1) comparisons of trace positions. It is the theoretical basis for "only the affected part re-runs in the original order".
- **Jane Street Incremental** (`https://github.com/janestreet/incremental`). A height-based recompute heap with a cutoff on equal values. The heap is the same idea as Solid's height buckets. The README was not fetched (the raw path returned 404), so the details above are [MEM].
- **Salsa** (`https://github.com/salsa-rs/salsa`). Revisions with durability levels: each input carries a durability, and a query whose inputs are all high-durability is not re-verified on a low-durability write. This is a cheap cutoff that avoids graph walks. The README was fetched but not analysed in this run.

**Which ideas allow a static graph to compile to straight-line code.** A static (compile-time known) dependency graph needs no runtime edge list at all if the compiler can emit one function per node that reads its inputs by direct reference. Of the four families above:

- Height ordering (Solid, Jane Street) gives a fixed topological order, which can be precomputed. A static graph's heap can be replaced by a fixed sequence of calls.
- Self-adjusting computation's trace is a static graph's natural compiled form: each node is a closure over its inputs.
- Salsa's durability is a cheap skip for graphs whose inputs change rarely. It does not need dynamic edges.
- Adapton's name-based memo is the one that needs runtime identity, so it is the least useful for a static graph.

The compile-to-straight-line question is covered in section A6.

---

### A6. Compile-time reactivity

I did not fetch the compiler sources for Svelte, Solid's DOM-expressions, or Vue Vapor in this run. The statements below are [MEM] except where marked.

- **Svelte 5.** Compiles `$state` and `$derived` into `source` and `derived` calls (see A3.4). Runtime graph still exists. The compiler removes only the setup of templates and some static attribute updates.
- **Solid.** `dom-expressions` compiles JSX into template clones plus effects, one effect per dynamic expression. The runtime graph still exists and runs each effect through the height heap (A1).
- **Vue Vapor.** Compiles templates to direct DOM ops with a runtime `effect` per binding. Also keeps a runtime graph.
- **Ripple.** Compiles to `_$_.expression` and `_$_.for` blocks (A2.6). Runtime block tree still exists.
- **Xilem (typed view trees) [SRC `xilem_view.rs`].** The `View<State, Action, Context>` trait (line 53) has `type Element` and `type ViewState`, and `build(&self, ctx, app_state) -> (Element, ViewState)` (line 69). The view types are generic, so each view tree is a monomorphised value, and `View::build`/`rebuild` are statically dispatched. No `dyn` is needed for the view tree itself.
- **Dioxus.** [MEM] Templates with dynamic slots and a diff over the static parts. Not fetched.
- **Elm.** [MEM] Pure view functions diffed against the previous tree. Not fetched.

**How far static analysis can remove runtime tracking.** Compilers remove:

1. Template construction (static HTML is cloned, not built).
2. Dependency discovery for expressions whose inputs are syntactically visible (the compiler knows `count` is read, so it can emit a direct subscription).
3. Diffing of static subtrees (they are never visited).

They cannot remove the runtime graph when inputs are dynamic: a `for` over a list that changes, a `derived` whose body calls a function, or a conditional branch. For the target design, the first two are free. The third should be a typed, monomorphised view tree (Xilem style), with dynamic children stored as an enum or a fixed-capacity array, so that the update path has no `dyn` and no allocation.

---

## Part B: Data-oriented memory

**Status.** None of the sources in Part B were fetched or read in this run. Every claim below is [MEM]. Verify each URL before citing it, and do not treat the design levers that depend on them as verified.

### B1. Arenas and temporary allocators

- **Ryan Fleury, "Untangling Lifetimes: The Arena Allocator"** (`https://nullprogram.com` or `https://www.rfleury.com`). [MEM] Arenas tied to a frame or request, with a "scratch" pair so nested calls alternate between two arenas and never free each other's memory. This is the pattern Odin's `context.temp_allocator` follows.
- **Odin** (`https://odin-lang.org/docs/overview/#context-allocator`). [MEM] `context.allocator` and `context.temp_allocator`; the temp allocator is freed at a frame boundary by the program, not by the callee.
- **Zig std.heap** (`https://ziglang.org/documentation/master/std/#std.heap.ArenaAllocator`, `FixedBufferAllocator`). [MEM] `ArenaAllocator` frees everything in one call; `FixedBufferAllocator` uses a caller-supplied buffer, which is the basis for a per-frame allocation-free path.
- **Handles over pointers.** Andre Weissflog (floooh), "Handles are the better pointers" (`https://floooh.github.io/2018/06/17/handles-vs-pointers.html`). [MEM] A handle is an index plus generation, valid across reallocation. Generational indices (slotmap) are the Rust version.
- **Handmade Hero arenas** (`https://handmadehero.org`). [MEM] Casey Muratori's push-only memory arena with `PushStruct`/`PushArray` and a checkpoint/rollback.
- **Clay** (`https://github.com/nicbarker/clay`). [MEM] Single-arena layout where the entire UI tree lives in one preallocated array with integer indices.
- **Our Machinery** (`https://ourmachinery.com/post/`). [MEM] Posts on data-oriented design and per-frame memory. Not read in this run.

### B2. Ring buffers, virtual memory, and SPSC queues

- **Double and triple buffered command streams.** [MEM] Two or three command buffers, one written by the main thread and one read by the render thread, swapped at a frame fence. No locks on the hot path.
- **Virtual memory tricks.** [MEM] Reserve a large virtual range with `mmap(PROT_NONE)` and commit pages on demand. A mirrored ring buffer maps the same physical pages twice, adjacent, so a wrapped read is contiguous. Platform APIs: `mmap` and `mremap` on Unix, `VirtualAlloc` with `MEM_RESERVE`/`MEM_COMMIT` on Windows.
- **SPSC queues.** [MEM] A single-producer single-consumer ring with cache-line-padded head and tail indices and `Acquire`/`Release` ordering needs no CAS. Rust crates: `rtrb` and `heapless::spsc`. Verify.

### B3. Rust specifics

- **`bumpalo`** (`https://github.com/fitzgen/bumpalo`). [MEM] Bump arena with `alloc` returning `&mut T`. Reset is O(1) and does not run destructors.
- **`typed-arena`** (`https://github.com/SimonSapin/rust-typed-arena`). [MEM] Single-type arena that runs drops.
- **Branded arenas.** `generativity` (`https://github.com/danielhenrymantilla/generativity-rs`) and GhostCell (`https://plv.mpi-sws.org/rustbelt/ghostcell/`). [MEM] A lifetime brand ties references to an arena so the borrow checker prevents use after reset, without runtime checks. qcell is the runtime-checked alternative.
- **`zerocopy` and `bytemuck`.** [MEM] Derive safe zero-copy casts between `#[repr(C)]` structs and byte slices, which is what a command buffer needs.
- **Fixed capacity.** `arrayvec` and `heapless` (`https://github.com/rust-embedded/heapless`). [MEM] `ArrayVec<T, N>` and `heapless::Vec<T, N>` use const generics for capacity with no heap.
- **`MaybeUninit` arrays.** [MEM] Use `[MaybeUninit<T>; N]` for a fixed-capacity pool, with `assume_init` only after writing.

**Allocation-free per-frame pipeline in Rust.** The general shape:

1. A frame arena (`bumpalo::Bump`, or a `FixedBufferAllocator`-style fixed buffer) owns all per-frame nodes. Reset at frame end, O(1).
2. Persistent reactive nodes live in a slotmap of `Copy` generational handles (`slotmap` crate), with the slot storage preallocated to a known maximum.
3. The update path writes into a `[MaybeUninit<Command>; N]` ring, and the render thread reads it through an SPSC queue.
4. Branded lifetimes (GhostCell or `generativity`) prevent handles from outliving the arena that owns them, at compile time.

This is a design sketch, not a verified implementation. The `slotmap` and `bumpalo` APIs must be checked before writing code.

---

## Design levers

Ranked by expected value for a Rust GUI with (i) static structure compiled to direct code, (ii) cheapest known algorithm for dynamic parts, (iii) no steady-state allocation, and (iv) fetch-high/block-low async. "Verified" means the reason is backed by a file read in this run. Everything else is [MEM].

1. **Typed view tree as the static backbone (Xilem `View` trait).** Verified: `xilem_view.rs` line 53 defines `View` with associated `Element` and `ViewState` types, so each view is a monomorphised value and `build`/`rebuild` are static. Static parts need no graph. This is the largest single lever.
2. **Edge as one object on two intrusive lists, reused in place.** Verified in Solid `dev.js` 1675-1737 and alien `alien_system.ts`. Steady-state recompute allocates nothing if the dependency order is stable. Keep `_depsTail`/`_gen` reuse.
3. **Dependency reuse at a cursor (Ripple's `create_dependency` recycling, Preact's `_node` cache).** Verified in `runtime.js` 937-960 and `src_index.ts` 213-250. Avoids allocation on rerun.
4. **Height-bucketed heap with `_min`/`_max` cursors for dynamic dependencies.** Verified in `dev.js` 1177-1300. Handles dynamic changes via `adjustHeight`, and glitch freedom needs no topological sort.
5. **Compile static dependencies to direct subscriptions.** Solid and Svelte compilers do this for expressions with visible inputs (A6). Verified in Ripple's compiled output (`_$_.expression` per site). For the Rust design, emit a fixed subscriber array per static node, so no runtime edge list is needed.
6. **Clock-stamped pull (Ripple `is_tracking_dirty`) for dynamic deriveds.** Verified in `runtime.js` 1131-1160. Cheap to check and needs no height, which suits a dynamic branch with few nodes.
7. **push-pull split: write marks `Pending`, read validates (alien-signals).** Verified in `alien_system.ts` 138-200. Avoids recomputing unread deriveds.
8. **Generational handles in a preallocated slot array (Sycamore-style [MEM]).** Handles are `Copy`, so they do not add reference counts. Avoids Leptos's `Arc`/`Weak` on the hot path (Leptos verified in `computed_arc_memo.rs`).
9. **Frame arena with O(1) reset (bumpalo or fixed buffer) for per-frame nodes.** [MEM] Standard practice in game engines. Avoids per-frame frees.
10. **Fixed-capacity command ring with `MaybeUninit` slots and an SPSC handoff to the render thread.** [MEM] No allocation and no lock on the hot path. Requires a bound on frame command count.
11. **Branded lifetimes (GhostCell/generativity) for arena handles.** [MEM] Compile-time safety for the arena reset, no runtime cost. Adds some API friction.
12. **Value-equality cutoff at the write site (`setSignal` equality check, `set` in Ripple).** Verified in `dev.js` 3154-3165 and `runtime.js` 1498-1502. Stops propagation early at no extra cost.
13. **Transition staging with a separate zombie heap (Solid's `zombieQueue`).** Verified in `dev.js` 1177-1179 and 756-775. Gives fetch-high/block-low a concrete mechanism: a pending update is staged and does not block the committed value, which is the behaviour the brief asks for. The Solid blog post could not be read, so the user-facing rule is unverified.
14. **Staged pending value per node (`_pendingValue` + `NOT_PENDING` sentinel).** Verified in `dev.js` 3154-3165. Lets a write be previewed without committing, the minimum needed for "block low".
15. **Cutoff on durability (Salsa) for rarely-changing inputs.** [MEM]. Skips a whole subgraph when all inputs are high-durability. Useful for theme or locale inputs, which change rarely.

**Recommended first build.** Lever 1 (typed view tree), lever 2 (intrusive edge objects), lever 4 (height heap for the dynamic subset), lever 5 (static subscriptions emitted by the macro), and lever 9 (frame arena). Verify levers 9-11 against their [MEM] sources before committing to a design.

---

## Open items

- Fetch and read the Solid blog post (WebFetch returned empty; try the blog's RSS or GitHub source for the article).
- Read `solid-js` benchmark results and the `js-framework-benchmark` repository for numbers. None were measured here.
- Read Svelte's `check_dirtiness` (or confirm it is absent in `main`) so the Ripple/Svelte comparison in A2.7 can be made precise.
- Read Preact's `src/index.ts` in full for the batch-snapshot details (only parts were used).
- Read Leptos `reactive_graph/src/owner/arena.rs` (or equivalent) to confirm whether an arena is used at all.
- Verify every [MEM] URL in Parts A5, A6 (Vue, Dioxus, Elm), and B.
