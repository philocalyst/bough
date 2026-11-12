# r6: how other frameworks handle fine-grained updates, keyed lists, virtualisation, async state, and gui-bench

Scope: the question is how a retained Rust GUI with plain-Rust state, `Signal<T>` writes that patch GPU fields directly, and paint-ordered record pages can avoid the costs of (a) re-describing a 1,000-row plain list per click, (b) positional reconciliation that replaces every row on one removal, and (c) lists being the only virtualised container.

Conventions:
- `[local path:line]` = a file in `/Users/philocalyst/Downloads/designs/reactivity/ref/`. Line numbers were read directly unless marked `~`.
- URLs are primary sources unless marked `(secondary)`.
- `UNCONFIRMED` = not verified in a primary source this pass.
- Numbers appear only where a source states them.

---

## 0. Cross-cutting findings (read this first)

1. Every fast system in this set keeps a stable identity per item (a block, a scope, a fiber, a row signal, a node id) and moves that identity, not the data. Positional reconciliation is the exception, and it is the one the user's framework has.
2. Keyed diffs fall into three families: (a) prefix/suffix trim plus a key map plus LIS over old indices (Ripple `for`, Dioxus, Vue Vapor, Inferno per secondary sources); (b) a cursor walk that moves one run at a time (Svelte `each`); (c) a positional scan that tracks a forward shift per item (Leptos/tachys). Only (a) gives a minimum-move guarantee among the ones read here.
3. Skip mechanisms are either version stamps (Svelte, Solid heap, Ripple tracked `c` clock), tree pointer equality (Dioxus `Rc::ptr_eq`), or explicit memo keys (Xilem `Memoize`, React Compiler, Flutter `canUpdate`/`identical`).
4. Virtualisation everywhere is the same algorithm: a visible index range from (scroll offset, viewport, row height) plus overscan, a placeholder extent for the rest, and a stable key per visible row. The differences are in how heights are measured and corrected (Section 3).
5. Async systems that avoid tearing do it with a staging/committed split: new values are held until the set that must appear together is ready; the rest of the screen keeps the committed value. Solid's hold model is the most explicit one in the set (Section 4).

---

## 1. Fine-grained propagation

### 1.1 Ripple (local: `/Users/philocalyst/Downloads/designs/reactivity/ref/ripple`)

Compiler (`packages/tsrx-ripple/src/`):

| Fact | Local source |
|---|---|
| Static markup is hoisted: `state.hoisted.push(b.var(template_id, template_call))` where the call is `_$_.template(html, flags)` or `_$_.template_el(...)` for a few static elements, which build DOM with calls instead of HTML parsing. | `tsrx-ripple/src/transform/client/index.js:8429-8470` |
| The compiler emits the runtime helpers `_$_.set_text`, `_$_.set_attribute`, `_$_.set_class`, `_$_.render`, `_$_.item`, `_$_.for_keyed`, `_$_.get`, and `_$_.if`. Dynamic parts become tracked reads inside render blocks. | `tsrx-ripple/src/transform/client/index.js` (grep of `_$_.` call sites) |
| A function that contains a reactive read is flagged `node.metadata.tracked = true`. | `tsrx-ripple/src/analyze/index.js:779-795` (`mark_as_tracked`) |

Runtime (`packages/ripple/src/runtime/internal/client/`):

| Fact | Local source |
|---|---|
| A `Tracked` value holds `sb`, the head of a doubly linked list of `Dependency` records. Each dependency has `t` (tracked), `r` (reaction/block), `sn`/`sp` (next/prev). | `runtime.js:604-630` (class `Tracked`), `runtime.js:955-1001` (`link_subscriber`, `unlink_subscriber`) |
| A write `set(tracked, value)` is a no-op when `value === old_value`; otherwise it sets `tracked.c = increment_clock()` and calls `mark_subscribers`. There is no equality function beyond identity unless the tracked has a custom `set`. | `runtime.js:1498-1531` |
| `mark_subscribers` walks `tracked.sb`. Deriveds are recursed into; destroyed readers are unlinked; every other reaction goes to `schedule_update(block)`. | `runtime.js:1102-1124` |
| `schedule_update` sets a `SCHEDULED` flag and pushes the block into `queue.blocks`; if `block.i < queue.last`, the queue is marked unsorted. Block ids are creation order, so parents (created first) run before children. | `runtime.js:1383-1401` |
| `flush_queue` sorts unsorted queues by block id, clears flags, then runs in one phase or in three phases (pre-effect, render, effect) depending on kinds. | `runtime.js:1206-1240` |
| Flushes are scheduled on a microtask (`queue_microtask`, `flush_microtasks`); `flush_count > 1001` reports an update loop. | `runtime.js:1324-1369` |

Keyed `for` (local: `packages/ripple/src/runtime/internal/client/for.js`):

| Fact | Local source |
|---|---|
| Each item is a `BRANCH_BLOCK` allocated by `create_item`; its `k` (key) is fixed for the block's life and its `v` (value) is a `tracked` only when the body reads it. | `for.js:38-77` |
| `patch_item` writes the index tracked only if `state.x` (index is read) and the value tracked only if `state.k !== undefined`. `update_value` compares by identity (`state.r !== value`) before any write; a new object with an equal key always writes. | `for.js:87-96`, `for.js:538-560` |
| First run creates all blocks; every later run goes to `reconcile_by_key_diff`. | `for.js:565-592`, `for.js:603` |
| Diff order: trim common prefix and suffix; match from both ends (including crossed ends for reversals and far-end moves); then a key map for the middle; `sources[]` holds old index+1 per new position; unmatched old items are destroyed unless `fast_path_removal`. | `for.js:603-860` (outline: prefix/suffix loops near 700; both-ends loop near 716) |
| Move plan: `lis_algorithm(sources)`, then a relay fallback when `(patched - seq.length) * 3 > b_left * 2`, i.e. when more than about two thirds of the kept items must move, the whole range is re-laid in order. | `for.js:862-875` (~) |
| `lis_algorithm` is the standard patience-sorting LIS with predecessor array `p` and `Int32Array` scratch, reused across runs (`max_len` growth). | `for.js:1301-1360` |

Cost model: prefix/suffix and both-end matches are O(1) per item. The middle costs one Map build plus LIS, O(m log m) for m unmatched items. Each moved item costs one DOM `insertBefore` per block node (`move(block, anchor)` at `for.js:161`). Removal of a middle item is O(1) through `destroy_block`.

Identity: the block (`s.start`/`s.end` nodes, `s.k` key, `s.v`/`s.i` tracked), stored in `state.blocks[]` and `state.keys[]`, parallel to `state.array`.

Ripple compiles each component body once, not each render. A reactive hole is a block whose run function re-executes only when a tracked it read changes. There is no tree diff for the static parts.

### 1.2 Solid 2.0, `next` branch (local: `/Users/philocalyst/Downloads/designs/reactivity/ref/solid`)

Checked out at `ce5b288` on branch `next`.

| Fact | Local source |
|---|---|
| Dirty nodes are queued in a height-indexed heap (`_heap[height]`, intrusive `_nextHeap`/`_prevHeap`). `actualInsertIntoHeap` sets `n._height = parent._height + 1` when a parent is taller. | `packages/signals/src/core/heap.ts:37-53` |
| `enqueueSub` lowers `dirtyQueue._min` and inserts. `insertIntoHeap` is idempotent (returns when `REACTIVE_IN_HEAP`); an entry into a marked heap is marked on the spot. | `heap.ts:19-22`, `heap.ts:54-77` |
| `markHeap` marks every heap entry DIRTY once per flush; `markNode` propagates `REACTIVE_CHECK` to subscribers. A node that is already DIRTY stops propagation. | `heap.ts:104-121` |
| `runHeap` walks heights upward. A node still IN_HEAP is recomputed; otherwise `adjustHeight` moves it to a height above its deps, which may re-insert subscribers at new heights. | `heap.ts:123-148` |
| `updateIfNecessary` is a pull: it recurses into CHECK deps first and recomputes only if the node is DIRTY or has an unsettled error. A node that is mid-recompute or disposed is never re-entered. | `packages/signals/src/core/core.ts:1126-1150` |
| Signals are plain nodes; computeds have `_height`. Writes mark subscribers; flush drains the heap. | `core/heap.ts` (above), `core/scheduler.ts` (`flush`, line 1662) |

Stores and projections (`packages/signals/src/store/`):

| Fact | Local source |
|---|---|
| `reconcile(value, key)` is an immutable-diff adoption channel: it swaps the backing object at each proxied level (no clones, no merge writes), notifies value nodes only where nodes exist, and descends into a changed child pair only where a listened node exists below it. Reference-equal unowned pairs skip. | `packages/signals/src/store/reconcile.ts:1-20` (doc), `reconcile.ts:46` (`reconcileState`) |
| Keyed arrays in reconcile: the aligned prefix is walked by position; from the first misalignment rows match by key, and surviving rows keep their proxy identity. | `reconcile.ts:1-20` |
| A projection is a computed store. Its derive runs inside a computed (the "firewall"); its output is adopted through reconcile in replace mode, so the root proxy is stable. Readers pull the derive for freshness without subscribing (the propagation barrier). | `packages/signals/src/store/projection.ts:1-18`, `projection.ts:379`, `projection.ts:441` |
| `mapArray` (the `<For>` engine) reuses mapped values for unchanged items. Keyed mode compares by identity or by an extracted key; index-only mode is also available. | `packages/signals/src/map.ts:20-40`, `map.ts:52` |
| Recent `mapArray` change: one-item inserts and removals no longer allocate list-sized arrays; removals use two pointers with no key map (`f3973ea`). | `packages/signals/CHANGELOG.md` (entry `f3973ea`) |

Transitions: the 2.0 surface does not export `startTransition` or `useTransition`. `packages/solid/src/index.ts` has a `/* Not Implemented` block (around line 305) that lists `startTransition`, `useTransition`, `createResource`, `Suspense`, `SuspenseList`, and `onMount` as removed or replaced. The hold model in `packages/signals/docs/SPEC-ASYNC-SEMANTICS.md:31` replaces transitions. Section 4 covers it.

Identity: heap nodes live on the graph node (`_height`, `_nextHeap`); store identity is the proxy target (`store/target.ts`); mapped rows live in `mapArray`'s owner per item (`map.ts`).

Cost model: a write costs O(subscribers) marks plus O(dirty) recomputes, each recompute at most once per flush, in height order. A mount of N effects that all write into one flush was O(N²) before fix `#3350` (`heap.ts:70-73`).

### 1.3 Svelte 5 runes (sources: fetched from `main`; local checkout not present)

| Unit of work | Skip mechanism | List cost | Source |
|---|---|---|---|
| Effect (per block, `each` item is a branch effect). A write marks direct reactions DIRTY and deriveds MAYBE_DIRTY. | `is_dirty` walks deps and recomputes deriveds on demand; an effect re-runs only if some dependency's write version `wv` exceeds the reaction's version. A derived whose new value is `equals` the old keeps its `wv`, so downstream effects are skipped. | Not LIS. A cursor walk over the linked effect list. An out-of-place item is moved to the front or back, whichever is cheaper (`dest.before(node)` for each DOM node of the item). `to_destroy` collects the unvisited. Per-move numbers UNCONFIRMED. | `packages/svelte/src/internal/client/dom/blocks/each.js` (raw.githubusercontent.com, `main`); `.../reactivity/sources.js` (`internal_set`, `mark_reactions`); `.../runtime.js` (`get`, `update_dependencies`, `is_dirty`); `.../reactivity/deriveds.js` |

Identity: `state.items: Map<key, {v, i, e}>` where `e` is the item's effect; effects are a doubly linked list (`state.effect.first/last`). Read counts: `rv` (read version) dedupes reads within one run; `wv` (write version) is the staleness stamp.

Cycle guard: `mark_reactions` switches to a `seen` set once the count of dependents exceeds 100,000 (secondary summary; line number UNCONFIRMED).

### 1.4 Vue 3.6 Vapor mode (source: `packages/runtime-vapor/src/apiCreateFor.ts` on the `minor` branch)

| Unit of work | Skip mechanism | List cost | Source |
|---|---|---|---|
| The whole `renderList` runs in the list's render effect. Each item is a `ForBlock` with its own EffectScope; updates go to shallowRefs (`itemRef`, `keyRef`, `indexRef`) that are written only when changed. | Item effects that read those refs re-run; the block is not re-rendered. | Three phases: (1) trim from the end while keys match; (2) same-position pass; (3) `oldKeyIndexMap` for the middle, with `sources[q] = oldIndex+1` (0 for new). `planMoves` runs `getSequence` (LIS) to pick the blocks that stay; the sparse path runs LIS per pinned segment and is not globally minimal (its comment says so). The apply loop runs back to front, anchoring each block on the next finalised node. `textContent=''` fast path when the whole list is removed. | https://raw.githubusercontent.com/vuejs/core/minor/packages/runtime-vapor/src/apiCreateFor.ts |

Vapor's release status (3.6 stable) is UNCONFIRMED; the source is the `minor` branch.

### 1.5 Leptos (tachys, `keyed`)

| Unit of work | Skip mechanism | List cost | Source |
|---|---|---|---|
| Each row owns a reactive owner (`parent.with(Owner::new)`); a list change re-runs `keyed(...)` only. Index changes go to a row's index setter. Rows are not rebuilt. | Per-row signals update in place. | Not LIS. A positional scan over `0..max_len` compares old and new keys at each index; a moved key gets `moves_forward_by = to_item.0 - index`; moves are grouped with `group_adjacent_moves`; `move_in_dom` is false when the shift is explained by net adds and removes. Order: clear, remove, move out, resize, move in, add, drop holes. Per-move numbers UNCONFIRMED. | https://docs.rs/crate/tachys/0.2.15/source/src/view/keyed.rs ; https://raw.githubusercontent.com/leptos-rs/leptos/main/leptos/src/for_loop.rs |

Identity: `hashed_items: IndexSet<K>` (FxHasher) in key order, plus `rendered_items: Vec<Option<(VFS, V::State)>>` at matching indices, and a placeholder marker. Tachys moved into the Leptos repo after 0.2.15 (secondary).

### 1.6 Sycamore (`sycamore-reactive/src/iter.rs`)

| Unit of work | Skip mechanism | List cost | Source |
|---|---|---|---|
| Keyed: one child scope per item (`create_child_scope`), disposed with its `NodeHandle`. Indexed: one child scope per position. | Keyed: an unchanged key keeps its mapped value (`mapped_tmp[j] = mapped[i].clone()`). Indexed: `item != Some(&new_item)` recreates the mapped value. | Keyed: trims the common prefix and suffix, builds `new_indices: HashMap<K, usize>` over the middle, then moves and disposes in two steps. Not LIS. Indexed: positional; inserting near the front recreates every later item (the `indexed_use_previous_computation` test). | https://raw.githubusercontent.com/sycamore-rs/sycamore/main/packages/sycamore-reactive/src/iter.rs |

Identity: `items: Vec<T>`, `mapped: Vec<U>`, `disposers: Vec<Option<NodeHandle>>`.

### 1.7 Dioxus 0.7.7 (docs.rs source)

| Unit of work | Skip mechanism | List cost | Source |
|---|---|---|---|
| A `VNode` is a static `Template` plus `dynamic_nodes` and `dynamic_attrs`; diffing touches only the dynamic slots. | VNode equality is `Rc::ptr_eq`; templates compare by pointer when built at compile time. | Keyed middle (`diff_keyed_middle`) runs LIS (`longest_increasing_subsequence::lis_with`) over `new_index_to_old_index`. LIS members are diffed in place; the rest are created or diffed and inserted before or after an anchor found by `find_first_element`/`find_last_element`. Prefix and suffix are trimmed first (`diff_keyed_ends`). Per-move numbers UNCONFIRMED. | https://docs.rs/crate/dioxus-core/0.7.7/source/src/diff/iterator.rs ; https://docs.rs/crate/dioxus-core/0.7.7/source/src/nodes.rs |

Identity: `VNodeInner.key: Option<String>`; `old_key_to_old_index: FxHashMap<&str, usize>`; `new_index_to_old_index: Box<[usize]>` (`usize::MAX` for new keys); `lis_sequence: Vec<usize>`. `Template.roots` is `&'static [TemplateNode]` with `TemplateNode::Dynamic { id }` indexing `dynamic_nodes`. LIS confirmed only for 0.7.7.

### 1.8 Xilem (`xilem_core`)

| Unit of work | Skip mechanism | List cost | Source |
|---|---|---|---|
| `Memoize` subtree: rebuilt only if `dirty` is set (by `MessageResult::RequestRebuild`) or `prev.data != self.data` (`Data: PartialEq`). | `MemoizeState { view, view_state, dirty }`. | UNCONFIRMED | https://raw.githubusercontent.com/linebender/xilem/main/xilem_core/src/views/memoize.rs |
| `View` trait, `ViewId`/`ViewPathTracker` | UNCONFIRMED (definitions are not in `xilem_core/src/lib.rs`, which only re-exports modules) | UNCONFIRMED | https://raw.githubusercontent.com/linebender/xilem/main/xilem_core/src/lib.rs |
| `Adapt` | Not found under that name; `views/lens.rs`, `map_state.rs`, `map_message.rs` exist; the correspondence is UNCONFIRMED. | UNCONFIRMED | https://github.com/linebender/xilem/tree/main/xilem_core/src/views |

Note: `memoize.rs` appears to pass the old view as both `prev` and receiver before storing the new one. Possibly a bug; UNCONFIRMED.

### 1.9 Floem, Makepad, egui, Slint

| Framework | Unit of work | Skip mechanism | List cost | Source |
|---|---|---|---|---|
| Floem | UNCONFIRMED (`src/view.rs` 404) | UNCONFIRMED | UNCONFIRMED | https://github.com/lapce/floem (repo metadata only; reactivity is in the separate `floem_reactive` crate, per search) |
| Makepad | Item drawn only inside the visible window (`next_visible_item`). | Off-screen items are not drawn; pool reuse via `reusable_items`. | Walk forward from `first_id` until past the viewport, then backward. Scrollbar maps `(first_id - range_start)/(...)`. No per-item numbers. Read at docs.rs `makepad-widgets 1.0.0`; the upstream path did not appear. | https://docs.rs/crate/makepad-widgets/1.0.0/source/src/portal_list.rs |
| egui | Immediate mode: UI code runs every frame. `show_rows` calls the closure with a `Range<usize>` of visible rows only. | Rows outside the range are never passed to the closure; uniform heights only. | O(visible). `min_row = floor(viewport.min.y / row_h)`, `max_row = ceil(viewport.max.y / row_h) + 1`, clamped to total; `skip_ahead_auto_ids(min_row)` keeps widget IDs stable. | https://raw.githubusercontent.com/emilk/egui/master/crates/egui/src/containers/scroll_area.rs |
| Slint | Partial renderer: `compute_dirty_regions` walks back to front; items draw only if their `PropertyTracker` is dirty. | Not a list diff; repaint bounded by dirty regions. `DirtyRegion` holds up to `MAX_COUNT = 3` rectangles; a fourth merges into the rect whose area grows least. | `ItemTreeNode::Item { children_index, children_count, parent_index, item_array_index }`; children are contiguous; `PartialRendererCache` is a slab with a generation counter. | https://raw.githubusercontent.com/slint-ui/slint/master/internal/core/partial_renderer.rs ; .../item_tree.rs |

egui's layout reuse across frames and where its time goes: UNCONFIRMED (not read).

### 1.10 Flutter (widgets, rendering, slivers)

| Unit of work | Skip mechanism | List cost | Source |
|---|---|---|---|
| Element (per widget in the tree). `Element.updateChild` branches in order: `newWidget == null` deactivates; same widget instance keeps the child; `canUpdate` calls `child.update(new)`; otherwise deactivate and inflate. | `Widget.canUpdate`: `runtimeType` and `key` equal. Same instance is identical and skipped. | Keyed multi-child moves are in `RenderObjectElement`/`updateChildren`; body not fetched, move cost UNCONFIRMED. GlobalKey reparenting reclaims an inactive element through `_retakeInactiveElement`. | https://github.com/flutter/flutter/blob/master/packages/flutter/lib/src/widgets/framework.dart |
| RenderObject (layout and paint). Layout runs depth-sorted, shallow first. A relayout boundary is a node whose parent does not read its size (`parentUsesSize` false). | Relayout boundary stops upward propagation of dirtiness. | n/a | https://github.com/flutter/flutter/blob/master/packages/flutter/lib/src/rendering/object.dart |
| RepaintBoundary: `isRepaintBoundary` gets its own layer; `_compositeChild` reuses a clean child's layer and repaints a dirty one. | Clean layer reused. | n/a | same file |
| Sliver list: `RenderSliverList.performLayout` fills backward with `insertAndLayoutLeadingChild` and forward with `insertAndLayoutChild` until the target end, then `collectGarbage`. Children are created only inside `childManager`. | Only children in the scroll-plus-cache window exist. | O(visible + cache). Extent beyond the last built child uses `estimateMaxScrollOffset`. | https://github.com/flutter/flutter/blob/master/packages/flutter/lib/src/rendering/sliver_list.dart ; .../sliver_multi_box_adaptor.dart ; .../widgets/scroll_view.dart |
| Impeller: precompiles a smaller shader set and builds pipeline state up front; default on iOS, Android API 29+, and desktop as of 3.47 (per the docs page). Display list and tessellation internals UNCONFIRMED. | n/a | n/a | https://docs.flutter.dev/perf/impeller |

### 1.11 Jetpack Compose (runtime, stability, lists)

| Unit of work | Skip mechanism | List cost | Source |
|---|---|---|---|
| Restartable composable (recomposition scope). Strong skipping (default in Kotlin 2.0.20+) makes every restartable composable skippable: stable params compare with `equals()`, unstable ones with `===`. | Skipping by stability. `@Stable`/`@Immutable` are opt-in; standard `List` is unstable; `ImmutableList` makes element-by-element `equals` which can cost more than recomposing. | `LazyColumn` composes and lays out only visible items plus a buffer. Keys preserve remembered state across moves. `contentType` restricts reuse to same-type items. | https://developer.android.com/develop/ui/compose/performance/stability ; .../stability/fix ; https://developer.android.com/develop/ui/compose/lists |
| Slot table: a gap buffer (design doc). Group index ignores the gap; group address is absolute. | n/a | n/a | https://android.googlesource.com/platform/frameworks/support/+show/2435c8283887ff3b5fe818c44d03f7fc079e2c87/compose/runtime/design/how-compose-works.md |
| Snapshot state: `mutableStateOf` defaults to `structuralEqualityPolicy`; a same-value write schedules nothing. Reads go through `next.readable(this)`. | Equivalence check before invalidation. | n/a | https://raw.githubusercontent.com/androidx/androidx/androidx-main/compose/runtime/runtime/src/commonMain/kotlin/androidx/compose/runtime/SnapshotState.kt |
| `LazyLayoutPrefetchState`: queued requests by priority; each step checks a time budget (`shouldExecute(available, average)`); costs are weighted moving averages per content type; precomposition is pausable. | Budget per frame. | n/a | https://github.com/androidx/androidx/blob/androidx-main/compose/foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/lazy/layout/LazyLayoutPrefetchState.kt |

`SlotTable.kt`, `Snapshot`, and `DerivedSnapshotState` internals: UNCONFIRMED (source paths 404 this pass).

### 1.12 SwiftUI (attribute graph)

| Unit of work | Skip mechanism | List cost | Source |
|---|---|---|---|
| View body and attribute nodes (private). | Public docs: a changed dependency recomputes affected bodies. Reverse-engineered: a node recomputes only when an incoming edge is pending. | `LazyVStack` creates children on demand near the viewport; exact timing UNCONFIRMED. | WWDC21 10022 https://wwdcnotes.com/notes/wwdc21/10022/ (secondary notes); WWDC23 10160 https://developer.apple.com/videos/play/wwdc2023/10160/ ; reverse engineering https://chris.eidhof.nl/note/attribute-graph/ , https://rensbr.eu/blog/swiftui-attribute-graph (secondary) |

The name "AttributeGraph" is not confirmed as public Apple documentation.

### 1.13 React (compiler, lanes, keyed children)

| Unit of work | Skip mechanism | List cost | Source |
|---|---|---|---|
| Component render output. The React Compiler memoises JSX, functions and values per reactive region, not per component. | Memo cache slots (layout UNCONFIRMED). | Not a list mechanism. | https://react.dev/learn/react-compiler/introduction |
| Fiber lanes: a 31-bit mask; `SyncLane` highest; transitions `TransitionLane1..10` with rotation; `RetryLane1..4`; idle and offscreen never expire. `mergeLanes` is OR; entanglement in `entangledLanes`. | Work at lower priority is skipped while a higher lane is pending. | n/a | https://github.com/facebook/react/blob/main/packages/react-reconciler/src/ReactFiberLane.js |
| Keyed children: pass 1 matches by index until a key mismatch; pass 2 builds a Map of remaining old fibers. `lastPlacedIndex` is the high-water mark of old indices kept in place; a reused fiber whose old index is below the mark gets `Placement`. | Reuse by key. | Heuristic, not minimal: `A,B,C -> C,A,B` produces two `Placement`s where moving only C suffices (traced from the code). Per move: one flag plus one Map insert and lookup. | https://github.com/facebook/react/blob/main/packages/react-reconciler/src/ReactChildFiber.js |

Suspended subtree retention and `useDeferredValue`: UNCONFIRMED.

### 1.14 Fine-grained summary

| Framework | Unit of work | Skip mechanism | List cost | Identity storage |
|---|---|---|---|---|
| Ripple | item block, tracked reactions | identity check on tracked write; creation-order block queue | prefix/suffix, both-end, Map, LIS; relay when most move | `state.blocks[]`, `state.keys[]`, per-item `tracked` |
| Solid 2.0 | computed node in height heap | height-ordered pull (`updateIfNecessary`), CHECK/DIRTY flags | `mapArray`: map plus two-pointer removal path (CHANGELOG) | graph node `_height`; store target; mapped row owner |
| Svelte 5 | branch effect per item, `wv`/`rv` versions | version stamp, `equals` keeps old version | cursor walk, move-to-front or back (no LIS) | `state.items` Map, linked effect list |
| Vue Vapor | `ForBlock` with EffectScope | shallowRef write only on change | three-phase, LIS (sparse path not global) | `oldKeyIndexMap` |
| Leptos (tachys) | row owner | per-row signals | positional scan with forward shift | `IndexSet<K>` plus rendered vec |
| Sycamore | child scope per item | identity compare (Indexed); kept mapped value (Keyed) | prefix/suffix plus key HashMap, two-step move | `Vec`s plus disposers |
| Dioxus | VNode (template plus dynamic slots) | `Rc::ptr_eq`; static template | LIS over `new_index_to_old_index` | key `String`, FxHashMap |
| Xilem | view (memoised) | `Data: PartialEq`, `dirty` flag | UNCONFIRMED | `MemoizeState` |
| Flutter | Element and RenderObject | `canUpdate`; relayout and repaint boundaries | keyed moves UNCONFIRMED; sliver window | element tree; GlobalKey registry |
| Compose | restartable composable | strong skipping by stability | `LazyColumn` window plus budgeted prefetch | slot table; item keys |
| SwiftUI | attribute node (private) | pending-edge recompute (reverse-engineered) | LazyVStack on demand | private graph |
| React | fiber | key reuse, `lastPlacedIndex` | heuristic moves | fiber tree |
| egui | none (immediate) | `show_rows` range | O(visible) | `skip_ahead_auto_ids` |
| Slint | item with `PropertyTracker` | dirty flag per item; dirty regions | not a list diff | contiguous item array |

---

## 2. List reconciliation algorithms

### 2.1 Algorithms, costs and what they map to

Operations for "a run of records moved" (the user's framing): a run of k contiguous kept records that all shift position is one operation if the record set is kept in order. The LIS of the old-index sequence is the set of kept records; everything else is a move (or an insert or delete). LIS gives the minimum number of moves for a fixed set of kept records.

| Algorithm | Used by | Cost | Moves per change | Notes |
|---|---|---|---|---|
| Positional (index match) | Svelte's "unkeyed", naive React | O(n) | every item after the change | Replaces every row on one removal. This is the user's current failure. |
| Prefix and suffix trim, then key map on the middle | Sycamore, Ripple first pass | O(n) for ends, O(m) map for middle | not minimal | Cheap for one insert or removal at an end. |
| Two-ended match (Ripple, both ends including crossed ends) | Ripple | O(n) for matched items | handles far-end moves and reversals as plain moves | Ripple checks `a_keys[a_start]==b_keys[b_start]`, then `a_keys[a_end]==b_keys[b_end]`, then the crossed cases. |
| LIS over old indices | Ripple (`lis_algorithm`, `for.js:1301`), Dioxus, Vue Vapor (dense path), Inferno (secondary) | O(m log m) | minimum for kept set | Kept set is the LIS; others are moved. |
| Cursor walk, move-front or move-back | Svelte `each` | O(n) per run | not minimal, bounded by cheaper direction | Simpler; costs more moves on long-distance swaps. |
| Forward-shift positional scan | Leptos tachys | O(max_len) | moves grouped by adjacency | Good for shifted runs; no LIS. |
| Heuristic `lastPlacedIndex` | React | O(n) | not minimal (`C` to front gives two placements) | No Map needed for the common prefix. |
| Myers diff | not used by any framework read here | O(ND) | minimal edit script | Not checked against a framework source in this pass (UNCONFIRMED for absence). |

### 2.2 Ripple's relay threshold

When most surviving items must move (`(patched - seq.length) * 3 > b_left * 2`, `for.js:~866`), Ripple re-lays the whole range in order instead of moving the non-LIS items one by one. This is the cost-model switch the user should copy: above roughly two thirds moved, one rebuild pass of the range is cheaper than many scattered moves.

### 2.3 Cost of a removal of one row in a 1,000-row list

| Algorithm | DOM operations | Map operations | Row state touched |
|---|---|---|---|
| Positional | ~1000 patches (every row after the removed index) | 0 | every row's text and attributes |
| Ripple both-ends + prefix/suffix | 1 destroy (`destroy_block`) | 1 delete from `state.keys` view (`reconcile_fast_clear` if all removed) | 0 other rows |
| Svelte cursor | 1 destroy | Map delete | 0 other rows |
| React lastPlacedIndex | 1 deletion | Map build over remaining old fibers (O(n)) | 0 other rows, but O(n) Map build |

The positional case is what makes the user's removal cost grow with list length. Any framework above avoids it with a key.

---

## 3. Virtualisation

### 3.1 The shared mechanism

Inputs: scroll offset, viewport height, row height (declared or measured), row count, overscan. Output: visible index range `[first, end)`, plus `lead` and `trail` spacer heights. Unbuilt rows cost nothing; the scroll extent is `lead + count*row + trail`.

### 3.2 zgui (local: `/Users/philocalyst/Downloads/designs/reactivity/ref/zgui`)

| Fact | Local source |
|---|---|
| `VirtualWindow { rows, first, count, lead, trail, row_size }`; `indices()` returns `first..first+count`; `total()` is `lead + count*row_size + trail`. | `crates/zgui-ui/src/virtualize/window.rs:36-85` |
| `window()` computes `visible = ceil(viewport/row_size) + 1` (the +1 covers a row straddling the edge); `first = max(0, anchor - overscan)`; `lead = first*row_size`; `trail` is the rest. | `window.rs:153-200` (the visible-count rule is at line 190) |
| Degenerate inputs: `row_size <= 0` builds `1 + overscan` rows and reports `row_size: 0` so the next frame can measure; a zero viewport is rounded to one row. | `window.rs:140-175` |
| Lists longer than `MAX_EXTENT = 4_000_000` CSS px are compressed: the scroll position maps to a fraction of the rows (`offset_of`), so a scrollbar drag moves many rows at once. | `window.rs:115-121`, `window.rs:27-35` |
| Row height is declared, not measured, because deciding the window before building rows cannot use their heights. A style sheet that disagrees gives the wrong scrollbar length, not wrong rows. | `crates/zgui-ui/src/virtualize/mod.rs:59-69` |
| `Virtualize::new` subscribes to the scroll container's observation; the memo recomputes only when the window changes, so scrolling within one row rebuilds nothing. | `virtualize/mod.rs:70-120` |
| `VirtualList` keys each row by its index. A row closure that captures data needs a list rebuild to refresh; one that reads a signal updates alone. | `crates/zgui-ui/src/virtualize/list.rs:51-56`, `list.rs:130-133` |
| Accessibility: each row reports its true position and the list length (`set_position(index+1, total)`). | `list.rs:161-166` |

Keyboard navigation over a virtualised list needs a roving tab stop (the doc says Tab cannot reach every row). `list.rs:58-65`.

### 3.3 zgx (local: `/Users/philocalyst/Downloads/designs/reactivity/ref/zgx`)

| Fact | Local source |
|---|---|
| `VirtualList::visible_range(offset, viewport)` is O(1): `first = floor(offset/row_h)`, `end = ceil((offset+viewport)/row_h)`, both widened by `overscan`, clamped to `count`. Overflow-safe: widens before adding pixels. | `crates/zgx/src/virtual_list.rs:33-56` |
| `VariableHeights`: point-updatable height index (`HeightIndex`, logarithmic point updates, no whole-index clones); `set_height` bumps a `revision` signal; `row_offset`, `row_at`, `content_height` are reactive reads. | `crates/zgx/src/compose_variable.rs:14-123` |
| `variable_virtual_list(offset, heights, overscan, key, build)` mounts only visible keyed rows plus overscan; point height updates keep the first visible index anchored. | `compose_variable.rs:144-152` |
| Anchoring on count change: when rows are added or removed above the first visible row, the viewport holds the anchor row by key (CSS scroll-anchoring analogue). Lookup is `anchor_moved`, which runs only on count change and checks `key(shifted) == kept`. | `compose_variable.rs:392-416` (`#[cold] fn anchor_moved`) |
| `anchor_end()` keeps a chat-like viewport at the end through height changes. | `compose_variable.rs:46-49` |
| Spare-time prebuild: rows just outside the viewport are built during idle frame time, with constants `AHEAD_VIEWPORTS = 1`, `KEPT_VIEWPORTS = 4`, `KEPT_ROWS = 256`, `KEPT_NODES = 8000`, `MOST_WAITED = 4`, `KEPT_REST = 30 s`. A headless host builds only what is in view. | `crates/zgx/src/spare.rs:1-35` |
| Estimate correction: `measured_virtual_list` updates the height cache from mounted natural content, with a one-pixel floor; offscreen estimates remain estimates until mounted again; total extent is approximate. | `compose_variable.rs:154-160` |

zgx's `variable_virtual_list` example is `crates/zgx-desktop/examples/variable_virtual_list.rs` (100,000 rows, heights cycled 24/40/64/88; the example is the source of that setup, not a benchmark).

### 3.4 gui-bench zgui virtual app

`apps/zgui/src/virtual_rows/mod.rs` builds `VirtualList(count, row_size = 32.0, class = "rows", row = Row(index))`. It uses zgui-ui's `VirtualList` directly, with the 32 px row height declared (the bench's row height). The `rows` scenario uses a plain `scroll` with every row built (`apps/zgui/src/rows/list.rs:10-25`, keyed by index).

### 3.5 Flutter, Compose, UIKit/AppKit, SwiftUI, CSS, GPUI

| Framework | Unit of work | Skip mechanism | Size estimate and correction | Source |
|---|---|---|---|---|
| Flutter slivers | child in the viewport window | only children in scroll plus cache exist; `collectGarbage` removes others | `estimateMaxScrollOffset` from known first and last offsets; corrected as children are laid out | sliver_list.dart (see 1.10) |
| Compose LazyColumn | item in window plus buffer | composes only visible items; prefetch under a per-frame time budget with moving-average cost per content type | item sizes measured when composed | developer.android.com/develop/ui/compose/lists ; LazyLayoutPrefetchState.kt (see 1.11) |
| UIKit `UITableView`/`UICollectionView` | cell (reused) | `dequeueReusableCell(withIdentifier:)`; prefetch callbacks are advisory | `estimatedRowHeight` pattern (general knowledge, UNCONFIRMED against primary text) | developer.apple.com/documentation/uikit/uitableview/dequeuereusablecell(withidentifier:for:) (title only) |
| AppKit `NSTableView` | view (reused) | `makeView(withIdentifier:owner:)` from a reuse queue | UNCONFIRMED | developer.apple.com/documentation/appkit/nstableview (title only) |
| SwiftUI `LazyVStack` | child created near viewport | on demand | UNCONFIRMED | developer.apple.com/documentation/swiftui/lazyvstack (title only) |
| CSS `content-visibility: auto` | box subtree | skips layout and paint offscreen; DOM and accessibility tree kept | `contain-intrinsic-size` placeholder; retains last rendered size (the `auto` keyword's exact rule UNCONFIRMED) | https://developer.mozilla.org/en-US/docs/Web/CSS/Reference/Properties/content-visibility |
| CSS scroll anchoring (`overflow-anchor`) | scroller and anchor node | anchor selected in child order (skipping `display:none`, fixed/sticky, `overflow-anchor: none`); deeper matches preferred | on anchor move, queue a scroll adjustment equal to the change in block-start edge; suppressed by top, size, position or transform changes | https://drafts.csswg.org/css-scroll-anchoring/ |
| GPUI `uniform_list` | item element | visible range `first = floor((scroll_top - padding)/h)`, `last = ceil((scroll_top + height)/h)`, capped at item count; measures one item | one measurement, all rows assumed the same height | https://github.com/zed-industries/zed/blob/main/crates/gpui/src/elements/uniform_list.rs |
| GPUI `list` | item element, measured | `SumTree<ListItem>` of `Unmeasured{size_hint}` or `Measured{size}`; summary tracks count, rendered count, height, unknown height; overdraw items are measured | splice marks new items unmeasured and adjusts `ScrollAnchor{ListOffset{item_ix, offset_in_item}}` | https://github.com/zed-industries/zed/blob/main/crates/gpui/src/elements/list.rs |

### 3.6 Designs that avoid a special list widget

- Slint's partial renderer virtualises paint, not layout: it redraws only dirty regions (`partial_renderer.rs`). A list of N items still lays out, but a change repaints only the damaged rectangle. Relevant to the user's damage-tile design.
- Xilem's `Memoize` makes an entire subtree free when its input data is unchanged; a list is one memoised child per row, not a special container (`memoize.rs`).
- Ripple and Dioxus make a list a keyed block inside an ordinary element, so the same skip mechanism applies everywhere; there is no separate list container.
- Makepad's `PortalList` is a list widget that draws only the visible range and pools items (`reusable_items`).

### 3.7 Scroll anchoring summary

Key-based anchoring (zgx `anchor_moved`) holds the first visible key and the offset inside it when rows change above the viewport. CSS anchoring does the same with a DOM node. The user's design needs key-based anchoring because an index is not stable under insertion.

---

## 4. Async state

### 4.1 Solid 2.0 (local: `/Users/philocalyst/Downloads/designs/reactivity/ref/solid/packages/signals`)

The local checkout's model is the hold model, not transitions.

Local documents: `docs/SPEC-ASYNC-SEMANTICS.md` (the rule set, "The hold model — L2" at line 31), `docs/INTERNALS-ASYNC-STATE.md` (working notes; the top section replaces §1 to §4 with the L2 model), `docs/RULES-FUNDAMENTALS.md` (draft fundamentals, 141 lines).

| Concept | Mechanism | Local source |
|---|---|---|
| Node value tiers | `_value` (committed), `_pendingValue` (staged for this flush), lane value on `_x._lane`. A held node is `CONFIG_HELD`; its transaction is `_transaction`. | `docs/INTERNALS-ASYNC-STATE.md:9-17` (section 0) |
| Transaction | `Transaction` holds `_nodes`, `_reruns` (stale readers to re-derive at landing), `_queues` (stashed effect runs), `_open` (actions). `settle` runs the heap, lane corrections, then holds every staged node of the flush and lands transactions no frame waits on. | `INTERNALS-ASYNC-STATE.md` section 0; `packages/signals/src/core/scheduler.ts` |
| Read rules | `frameRead` (render effect reads the screen), `joinPass` (a held node's staging, the pass is the transaction's), `stagedRead` (this flush's staging as screen). | `INTERNALS-ASYNC-STATE.md` section 0; `packages/signals/src/core/core.ts` |
| Stale reader | A reader of a node whose new value is held sees the committed value and re-derives at the commit. Rule A15 in SPEC: "A stale (render) reader is unchanged: the reveal carve-out (A15) serves it the committed value with no entanglement." | `SPEC-ASYNC-SEMANTICS.md` (Mechanism of A15, around line 125) |
| Entanglement | Writes whose async work is observed by a shared reader settle as one unit (no tearing); writes on disjoint graphs settle independently. Only a pass that observes a pending flight joins that flight's transaction. | `SPEC-ASYNC-SEMANTICS.md` (rule B3, around line 257) |
| Lanes (optimistic writes) | A lane is a sub-frame an optimistic write opens in the transaction of the frame it is made in. It sees the screen plus its own guesses, breaks out of the parent hold, and ends when the parent lands (revert or supersede). | `packages/signals/src/core/lanes.ts:1-32` |
| Loading boundary | A boundary whose content is pending shows its fallback, and the root never hears of it if the boundary has not shown content; a boundary already showing content forwards the hold. The reveal happens at the commit of the held frame. | `packages/signals/src/boundaries.ts:1-41` |
| `isPending` / `latest` | Verdict reads: `latest()` reads the committed or optimistic answer; `isPending()` reports pending. Both are a cold path off `read` while a window is open. | `packages/signals/src/core/verdict.ts:364` (`latest`), `:410` (`isPending`) |
| Actions | `action()` is a transaction opener. The supported idiom is `await` for typed results, then a bare `yield` before writes; `flush()` inside an action body is out of contract. | `SPEC-ASYNC-SEMANTICS.md` rule on `yield` (around line 117); `packages/signals/src/core/action.ts:129` |
| Projections and optimistic stores | `createProjection` derives inside a computed firewall and adopts its output through reconcile. Optimistic store guesses sit on lanes; a landing beneath a guess is held beneath it. | `store/projection.ts`, `store/optimistic.ts`, `SPEC-ASYNC-SEMANTICS.md` |

What "fetch high, block low" means in Solid: the phrase is NOT in the local checkout (grep of `packages/signals/docs`, `CHANGELOG.md`, `src`, and `documentation` returned nothing), and not in the 2.0.0-beta.0 release notes (https://github.com/solidjs/solid/releases/tag/v2.0.0-beta.0). The closest local model is the hold model above: a fetch is a flight; its readers that are on screen get the committed value (not blocked); the transaction waits only for readers that observed the flight (so the hold is on the screen, not on the fetch). Treat the user's phrase as their term for this, UNCONFIRMED against Solid's own text.

Operational consequence (from SPEC rules and the L2 description): a pending async memo inside a loading boundary that has not shown content makes the boundary show its fallback. Once content is shown, a new pending flight does not hide it; the staged result reveals at the commit. Tearing is prevented by holding every staged node whose pass joined the flight until the commit.

Where pending blocks: `isPending(() => expr)` observes; it does not derive (rule A23). A `latest(fn)` read creates a shadow whose backfill is an observation, never a blocker of the settle (rule A23, around line 125).

`createAsync`: the local 2.0 surface does not export it (`packages/signals/src/index.ts` exports `action`, `isPending`, `latest`, `refresh`, `until`, `createOptimistic`, `createMemo`, `createProjection`). The public docs for `createAsync` (`solid-next-docs.netlify.app`, April 2025, WIP) are older. UNCONFIRMED for 2.0 final.

Transitions removed: local evidence is the `/* Not Implemented` block in `packages/solid/src/index.ts` (around line 305), which lists `startTransition` and `useTransition` as not implemented in the 2.0 line, plus the replacement of transition terminology by holds and lanes in the L2 docs.

### 4.2 React

| Mechanism | Fact | Source |
|---|---|---|
| Transitions keep revealed content | "Transitions only 'wait' long enough to avoid hiding *already revealed* content." A nested Suspense boundary inside revealed content is not waited on. | https://react.dev/reference/react/useTransition |
| `isPending` | true from the first `startTransition` call until the actions finish and the final state is shown. | same |
| `use(promise)` | Suspends to the nearest Suspense fallback. The promise must be cached across renders, or the fallback repeats. Cannot be used inside try/catch. | https://react.dev/reference/react/use |
| Tearing | The mechanism that prevents tearing across a transition is NOT in the pages read (reactwg discussions 37 and 41 cover hydration and announcements). UNCONFIRMED. | https://github.com/reactwg/react-18/discussions/37 ; .../41 |
| Lanes | See Section 1.13. | ReactFiberLane.js |

### 4.3 Relay

| Mechanism | Fact | Source |
|---|---|---|
| Suspense fetch | `usePreloadedQuery` and `useLazyLoadQuery` suspend; a Suspense boundary shows the fallback. Switching content inside an already-mounted boundary hides existing content (future concurrent support may avoid this). | https://relay.dev/docs/guided-tour/rendering/loading-states/ |
| Store | Records keyed by `id`. A mutation response with a matching id merges into the existing record, so one update reaches every view. | https://relay.dev/docs/guided-tour/updating-data/graphql-mutations/ |
| Optimistic | Optimistic responses roll back on success or error. If the store changes mid-flight, Relay reverts the optimistic update, applies the store change, and reapplies. | same |
| Stale | Mutations can mark data stale so Relay refetches on next render. | same |

### 4.4 SWR (source: `vercel/swr`, `src/index/use-swr.ts`, line numbers approximate)

| Mechanism | Fact | Source |
|---|---|---|
| Dedupe | `shouldStartNewRequest = !FETCH[key] || !opts.dedupe` (about line 500). | https://github.com/vercel/swr/blob/main/src/index/use-swr.ts |
| Dedupe window | `setTimeout(cleanupState, config.dedupingInterval)` (about line 545). | same |
| Stale while revalidate | Returns `cachedData` while revalidating; background revalidation starts in a mount effect via `rAF(softRevalidate)` (about lines 780-810). | same |
| Key | `serialize(_key)` (about line 250); `stableHash` location UNCONFIRMED. | same |

### 4.5 TanStack Query (optimistic)

`onMutate`: `cancelQueries` so an in-flight fetch cannot overwrite the optimistic value; snapshot with `getQueryData`; write with `setQueryData`; return the snapshot. On error restore; on settle `invalidateQueries` to refetch the server truth. Rendering from `variables` is simpler for one component; cache writes are better when several components need the value. https://tanstack.com/query/latest/docs/framework/react/guides/optimistic-updates . Query keys and `staleTime` pages not fetched (UNCONFIRMED specifics).

### 4.6 Recoil and Jotai

Recoil: `useRecoilValueLoadable` returns `Loadable` with `hasValue`, `hasError`, `loading`, so components can handle async selectors without Suspense. https://recoiljs.org/docs/api-reference/core/useRecoilValueLoadable . Jotai async atoms and `loadable`: UNCONFIRMED (fetch failed).

### 4.7 Async summary table

| System | Unit that pends | Blocks what | Tearing prevention | Source |
|---|---|---|---|---|
| Solid 2.0 (local) | flight attached to a node; transaction | only pass readers that observed the flight, plus the transaction's commit | staged values held until the commit; stale readers serve committed | packages/signals docs (above) |
| React transitions | update lane | transition-lane render; revealed content kept | UNCONFIRMED mechanism | react.dev |
| Relay | query fragment | Suspense boundary | store normalises by id | relay.dev |
| SWR | key | nothing (stale returned) | dedupe per key | vercel/swr |
| TanStack Query | query key | nothing (stale returned) | cache write per key | tanstack.com |
| Recoil | selector | per consumer via Loadable | UNCONFIRMED | recoiljs.org |

---

## 5. gui-bench and fakeland

### 5.1 gui-bench (local: `/Users/philocalyst/Downloads/designs/reactivity/ref/gui-bench`)

Scenarios (README table, `README.md:18-27`): `rows`, `table`, `ticker`, `spinner`, `typing`, `search`, `virtual`, `resize`. Variants (`README.md:39-59`): `fine`, `coarse`, `shared` for `table`/`ticker`.

Measurement per scenario (runner, `runner/src/scenarios/measure.rs:26-62`, `runner/src/steps.rs`):

| Scenario | What runner sends (fakeland) | What is measured | Where framework cost shows |
|---|---|---|---|
| `rows` | hover probe: pointer to row 5 from row 1 (`steps.rs:99-111`); scroll glide of 800 px/s finger, 4 events per refresh, 1 s (`steps.rs:121-137`) | hover latency (pixel probe in row 5, optionally app mark), scroll smoothness, startup | a framework that scales or animates scroll input is undone by the app (README `:168-183`) |
| `table` | click select on row 4 (toggle) (`steps.rs:143-152`, `measure.rs:97-104`); keys F1 to F8 with undo keys (`measure.rs:10-20`, OPS: update10 F1/F1 row 0; swap F2/F2 row 1; remove F3/F4 row 2; insert F4/F3 row 2; replace F5/F5 row 0; append F6/F7 status; clear F8/F8 row 0); churn: 4 warmup then N presses of F5 with memory sample before and after (`steps.rs:61-85`) | input-to-photon per operation; app CPU time per operation (iteration includes undo); memory kept per churn | Ops in `spec.rs:198-281` define the data change: Update10 writes every 10th row; Swap rows 1 and len-2; Remove index 2; Insert index 2 (`EDIT_AT`); Replace new rows; Append 1000; Clear |
| `ticker` | pacing for 3 s (`steps.rs:93-95`); timer every 16 ms updating 1 or 100 rows and the header (`spec.rs:294-304`) | CPU and GPU time per second; delivered frames; damaged area | Coarse variant rebuilds all rows per tick (`apps/zgui/src/table/coarse.rs:23-33`) |
| `spinner` | pacing (3 s) | CPU and GPU time per second; frames; damage | GPUI and iced re-render the page per spinner frame (README `:55-59`) |
| `typing` | 120 wpm typing into focused input, cleared between iterations (`steps.rs:28-34`) | key to photon | input widget's own cost |
| `search` | key `z` filters all rows then BackSpace (`measure.rs:44-48`, `spec.rs:306-312`); typing afterwards | key to filtered list | filter path of each app |
| `virtual` | same rows as `rows` in the framework's virtual list, 100 k and 1 M rows (README `:26`) | startup, hover, scroll | virtual list cost (Section 3) |
| `resize` | resize storm 640x480 to 1200x800 in 1 s; scale switch 1 to 2 to 1 (`steps.rs:36-47`, `measure.rs:53-59`) | configure to present; frames at stale size; memory at scale 2 | |

Every cell also measures startup, a layout check screenshot, and idle (3 s with the middle second suspended) (`README:31-37`, `steps.rs:81-90`). Metrics: `resources` (CPU, wakeups, RSS/PSS/private, VRAM, GPU busy) and `frame_cost` (frames, CPU ms per frame, GPU fence time, damaged fraction) (`README:61-82`).

Spec (`apps/shared/spec.rs`): 1280x800 window; rows 32 px; header 40 px; table data is `Row { id, label, value }` built from ids (`spec.rs:134-151`); `Op::apply` is the single definition of each table operation (`spec.rs:248-281`); the same pixels for every framework. `TableRow` lets apps carry per-row state (`spec.rs:154-169`).

zgui app (`apps/zgui`):
- `rows` list: plain scroll container, `for index in 0..count, key = index`, a `Row` per index (`apps/zgui/src/rows/list.rs:10-25`); not virtualised.
- `virtual`: zgui-ui `VirtualList(row_size = 32)` (`apps/zgui/src/virtual_rows/mod.rs`).
- `table` fine variant: `KeyedFields` with one lens per field (`LABEL`, `VALUE`), an order signal of ids, keyed `for`, and a `Selector` so a selection change wakes two rows (`apps/zgui/src/table/fine.rs:1-14`, `run_op` at `fine.rs:53-97`, `TableRow` at `fine.rs:146-166`).
- `table` coarse variant: one `RwSignal<Vec<Row>>`; the list is one reactive hole that rebuilds every row on any change (`apps/zgui/src/table/coarse.rs:16-67`).
- `shared` variant: every row compares the one selection signal (fine.rs `shared` flag, `fine.rs:118-121`).

zgui's own list benchmark (`crates/zgui-bench/src/bin/list-slope/main.rs:1-40`): 100,000 rows under fast wheel and touchpad. Two sweeps: "virtualisation sweep" (ns per model row; right answer is zero) and "glide sweep" (glide ns per realised box). Gate: ratio of largest to smallest document cost in one process.

Framework fast and slow paths (from the spec and runner):
- Fast: keyed fine-grained rows (zgui fine, floem `dyn_stack`, freya `PartialEq` props, iced `keyed_column` + `lazy`, gpui-kit `Entity.cached()`); a click or a single-row edit touches two rows.
- Slow: coarse variants rebuild every row (zgui coarse, floem coarse, vizia coarse); iced and GPUI re-run the page on every message or spinner frame; egui builds every row every frame (README `:39-59`).

Published results for gui-bench's frameworks (async agent, checked this pass):

| Framework | What was measured | Number | Date | URL |
|---|---|---|---|---|
| Zed GPUI | Frame-rate fix on macOS (Metal) | 120 FPS target; frame render under 4 ms while stuttering; frame times 8 to 16 ms in one failed attempt | 2024-02-07 | https://zed.dev/blog/120fps |
| GPUI `uniform_list` | none published | none | n/a | https://docs.rs/crate/gpui/0.2.2/source/src/elements/uniform_list.rs |
| Tauri, Iced, egui (Kalbertodt) | startup, input lag, resize FPS, binary size (Ubuntu 20.04, X11, 60 Hz) | startup: iced 217 to 333 ms, egui 200 to 300 ms; resize FPS: iced 12 to 30, egui 12 to 30; binary: iced 17 MB, egui 18 MB | 2023-02-03 | https://lukaskalbertodt.github.io/2023/02/03/tauri-iced-egui-performance-comparison.html |
| IronOxidizer gui-toolkit-benchmarks | startup, memory, binary size; rendering speed, input latency, CPU/GPU TODO | not read (results site) | 2026 | https://github.com/IronOxidizer/gui-toolkit-benchmarks |
| Freya | docs advise `VirtualScrollView` above a few hundred items | none | n/a | https://docs.freyaui.dev/ |
| Vizia | docs describe `VirtualList` as high-performance | none | n/a | https://docs.rs/astrelis-ui/0.2.4/src/astrelis_ui/virtual_scroll.rs.html (third-party) |
| Floem | docs claim "lists of millions of items" | none | n/a | https://lapce.dev/floem/src/floem/views/virtual_stack.rs.html |
| iced | `lazy` docs make no performance claim | none | n/a | https://docs.rs/iced/latest/iced/widget/fn.lazy.html |
| egui | `show_rows` docs; 10,000-row demo | none | n/a | https://github.com/emilk/egui/blob/beebef9c287bb932a519626bc6e7cb99a07e50d3/crates/egui_demo_lib/src/demo/scrolling.rs |

No numeric list-scroll benchmark was found for GPUI `uniform_list`, Freya, Vizia, Floem, iced, or egui. gui-bench's own results are not committed (no `*.json` or `*.csv` under `ref/gui-bench`), so this pass has no zgui numbers.

### 5.2 fakeland (local: `/Users/philocalyst/Downloads/designs/reactivity/ref/fakeland`)

Model (`README.md:16-48`): a headless Wayland compositor on smithay. Every timestamp is taken on the compositor thread: input sent, commit, GPU ready, latch, present. Time is wall clock; display presentation is modelled; everything up to "ready" is measured.

Trace events relevant to latency (`crates/fakeland-trace/src/event/kind.rs`):
- `InputSent { id, kind, surface, x, y, detail, source }` (line 249-265): sent after the flush to the client socket.
- `Commit { surface, commit, buffer, damage, frame_callback, presentation_feedback, sync, fifo_barrier, target, ... }` (line 165-192).
- `Ready` (fence signalled; line 193), `Latched { output, seq }` (line 200-210), `Presented { output, seq, time, refresh, flags }` (line 211-227, from `wp_presentation_feedback.presented`), `Discarded`, `Released`, `AppMark { surface, commit, mark }` (line 266), `ProbeStarted/ProbeInput/ProbeDetected/ProbeTimedOut` (around 305-332).

Stage computation (`crates/fakeland/src/probes/result.rs:8-41`):
- `input_to_commit = committed - input`
- `commit_to_ready = ready - committed`
- `ready_to_latch = latched - ready`
- `latch_to_present = presented - latched`
- `total = presented - input`
All `None` when one end is unknown.

Probe detection (`crates/fakeland/src/probes/hit.rs`): a probe injects an input at a chosen vblank phase; the detector walks commits of the target and reads back the probe region of each candidate buffer (`read_region`); the first commit whose pixels match (changed, or matching) is the detected frame. Detection finishes immediately if the commit is already on screen. Readback cost is recorded separately (`result.readback_cost`, `hit.rs:58`). An app mark can stand in for the pixel detector (`detector = app_mark` in `steps::hover`).

Analyzers (`crates/fakeland-bench/src/analyzers/latency/`): `Latency` takes probe results from each iteration, optional label filter, `phase_bins = 8`, `skip_repeats`. Report fields: `total_ms`, `input_to_commit_ms`, `commit_to_ready_ms`, `ready_to_latch_ms`, `latch_to_present_ms`, median confidence intervals, `by_phase`, `histogram`, `readback_cost_ms`, and per-sample rows (`report.rs:13-60`).

Resources and frame cost (README `:61-82`): `resources` samples every process tree every 50 ms (`SamplingConfig`); CPU from schedstat; wakeups; RSS/PSS/private; VRAM and GPU busy (DRM fdinfo, NVML); `frame_cost` gives frames, CPU per frame, GPU fence time per frame (`gpu_fence_ms_p50`), and damaged fraction. The damaged fraction is not discriminating on this host: every app damages its whole surface.

Compositor invariants that matter for fair measurement (`CLAUDE.md`): fakeland sets no environment variable and advertises no global that names it; time is read only through the session clock; trace event variants are append-only; analyzers return typed reports and never talk to the compositor.

Limitations stated in the README: a run is invalid when the compositor wakes late (each report carries validity); power numbers are device-wide and context only; presentation is modelled, not measured.

---

## 6. Ten ideas to steal for a Rust retained framework (no macro, no hooks, no VDOM)

Each idea states the mechanism and the source. All are compatible with `Signal<T>` writes that patch GPU fields directly.

1. **Version-stamped dirty checks, not flags alone.** Each source keeps a write version `wv`; each reaction keeps the version it last ran against. A reaction re-runs only if some dependency's `wv` is newer. A derived whose recomputed value is equal keeps its old `wv`, so its downstream readers are skipped. Svelte does this with `source.wv`, `reaction.rv`, and `is_dirty`. Source: `packages/svelte/src/internal/client/reactivity/sources.js` (`internal_set`, `mark_reactions`) and `.../runtime.js` (`is_dirty`, `update_dependencies`) on `main`. Adapt: store `wv: u64` in each `Signal` and `rv` in each record reader, no closure allocation per read.

2. **Height-ordered flush with an intrusive heap.** Give each computed a height (one more than its deepest dependency). Dirty nodes sit in an array of intrusive lists indexed by height; flush walks upward, so each node runs once after its inputs and no glitches occur. A node whose height changes re-inserts its subscribers. Solid 2.0: `packages/signals/src/core/heap.ts:19-148` (`enqueueSub`, `actualInsertIntoHeap`, `markHeap`, `runHeap`, `adjustHeight`). Cheaper alternative in Ripple: block creation id as the order (`runtime.js:1383-1401`, `flush_queue` sort at `runtime.js:1206-1215`), valid when parents are always created before children. Adapt: a `Vec<Vec<NodeId>>` by height, no per-flush allocation.

3. **Per-field lenses over keyed records, with a selection selector.** Store records as plain values in one map keyed by id; each field read goes through a `Lens` (index, getter, setter); a field gets a signal only while a row reads it. The selection is a `Selector`: one subscription per row that fires only for the two rows that change, not N subscriptions on one signal. zgui: `docs/guide/reactivity.md` (section "Large keyed collections"); `ref/gui-bench/apps/zgui/src/table/fine.rs:1-14` and `:146-166`. Adapt: this matches the user's plain-Rust state with `Signal<T>` patching GPU fields, since a field write touches only the instance field its row reads.

4. **Keyed diff: ends first, then LIS over old indices, then a relay threshold.** (a) Trim the common prefix and suffix by key equality. (b) Match from both ends, including crossed ends for far-end moves and reversals. (c) Build a key-to-old-index map only for the middle. (d) Build `sources[new] = old+1` (0 for new). (e) Run LIS on `sources` to pick the kept set. (f) If `(moved - kept) * 3 > middle * 2`, re-lay the whole range in order instead of many moves. Ripple: `packages/ripple/src/runtime/internal/client/for.js:603-1360` (both-ends loop near 716, relay test near 866, `lis_algorithm` at 1301). Dioxus LIS: `dioxus-core 0.7.7` `src/diff/iterator.rs`. Cost: O(n) for stable lists, O(m log m) for the middle. This removes the user's "replace every row on one removal" failure because a removal is a destroy with no moves.

5. **Declared fixed row height, a pure window function, and lead/trail spacers.** `window(rows, row_size, viewport, offset, overscan) -> {first, count, lead, trail}` is a pure function; a row count and offset produce the same indices every time, so the list is a keyed block over `first..first+count` with key = index. A list longer than a bound (zgui uses 4,000,000 CSS px) maps scroll position to a fraction of the rows. Source: `ref/zgui/crates/zgui-ui/src/virtualize/window.rs:36-200` and `mod.rs:59-69` (why the height is declared, not measured). Adapt: make the list one container type that every framework feature (not a special widget) can use, and key rows by stable record id, not by index, if records can move.

6. **Variable heights in a point-updatable index, anchored by key.** Keep row heights in a log-time index (point update, prefix sums for offsets, search by offset). On a count change above the viewport, find the first visible key again and keep the offset inside it; only run this lookup when the count changes. Source: `ref/zgx/crates/zgx/src/compose_variable.rs:14-152` (`VariableHeights`, `variable_virtual_list`), `:392-416` (`anchor_moved`), `ref/zgx/crates/zgx/src/spare.rs:1-35` (spare-time prebuild, `KEPT_ROWS = 256`, `KEPT_NODES = 8000`). Adapt: `anchor_moved`'s cold path means insertions above the viewport cost nothing in the common scroll case.

7. **Hold the staged value; serve the committed value to readers that are not on the screen.** A write creates a staged value tied to a transaction. Readers that are on screen and do not observe the pending flight keep the committed value; readers that observe the flight join the transaction; the transaction commits (reveals) once all observed flights land, so no frame shows parts of two states. Solid 2.0: `packages/signals/docs/SPEC-ASYNC-SEMANTICS.md` (A15, B3 around lines 125 and 257) and `packages/signals/src/core/scheduler.ts` (`Transaction`, `settle`, `land`). Adapt for the framework's record pages: a page's paint uses committed records; a handler's new records go to a staged page that paints at the next commit. Keeps paint-ordered record pages consistent without locks.

8. **Template hoisting: static skeleton once, dynamic slots indexed.** A view's static markup is a `&'static` template built once; the view value holds only the dynamic slot values, and diffing touches only those slots. Ripple hoists `_$_.template(html)` (`ref/ripple/packages/tsrx-ripple/src/transform/client/index.js:8429-8470`); Dioxus `Template { roots, node_paths, attr_paths }` with `TemplateNode::Dynamic { id }` (`dioxus-core 0.7.7 src/nodes.rs`) and `VNode` equality by `Rc::ptr_eq`. Adapt: for "re-description of a view after a walked handler", describe static structure once and re-describe only the dynamic slots, so a click re-describes O(dynamic) not O(all rows).

9. **Damage tracking per item with a bounded region list.** Each item holds a dirty tracker (a property-level version or flag); a paint walk visits items in order and draws only items whose tracker is dirty; dirty rectangles are kept in a fixed-size list (Slint uses 3), and a fourth merges into the rectangle whose area grows least. Source: `https://raw.githubusercontent.com/slint-ui/slint/master/internal/core/partial_renderer.rs` (`compute_dirty_regions`, `DirtyRegion::MAX_COUNT = 3`, `filter_item`). Adapt: this is the user's tile damage design with a concrete bound on the region list and a merge rule.

10. **Stage decomposition of input-to-photon, with the probe that detects the response.** Record five timestamps on one clock: input sent, commit, buffer ready, latch, presented. Derive `input_to_commit`, `commit_to_ready`, `ready_to_latch`, `latch_to_present`, and `total`, each `None` when an end is missing. Detect the response by reading back a probe region of each candidate commit; the detected commit is the first whose pixels match. Source: `ref/fakeland/crates/fakeland/src/probes/result.rs:8-41` (`ProbeStages::compute`), `probes/hit.rs` (candidate test), `crates/fakeland-trace/src/event/kind.rs:165-280` (event variants). Adapt: the user's own bench needs the same split so that a regression in the framework's record path can be separated from a regression in paint or compositor.

---

## 7. Unconfirmed items (consolidated)

- Solid "fetch high, block low": not found in the local checkout or the 2.0.0-beta.0 release notes. The user's term may be their own shorthand for the hold model.
- Solid `createAsync` in 2.0 final: not exported by `packages/signals/src/index.ts`; public docs are a 2025 WIP.
- React tearing mechanism across transitions; Suspense retention; `useDeferredValue`.
- Inferno keyed diff (paths 404); ivi (no source found).
- Leptos/tachys per-move numbers; Vue Vapor 3.6 stability.
- Xilem `View` trait, `ViewId` storage, `Adapt` correspondence.
- Floem all questions (`src/view.rs` 404).
- Makepad upstream PortalList path and live DSL.
- egui layout reuse and time profile.
- Slint compiled binding pipeline and `PropertyTracker` implementation.
- Dioxus LIS in releases after 0.7.7.
- Svelte per-move costs and the 100,000 cycle-guard line number (secondary summary).
- Flutter keyed `updateChildren` body and relayout-boundary selection condition.
- Compose `SlotTable.kt`, `Snapshot`, `DerivedSnapshotState`.
- Apple UIKit, AppKit, `LazyVStack` detail (title-only pages).
- GPUI `Context::notify` path (truncated).
- Jotai async and TanStack query keys (not fetched).
- Every framework's benchmark numbers beyond the table in 5.1.
- gui-bench results: none committed; no zgui numbers in this pass.
- bbtree (`ref/bbtree`, a concurrent B-tree ordered map) was not part of the listed topics; its README states 5x single-thread speedup over `BTreeMap` and zero dependencies, which the README claims (not verified here). Possible use: an ordered index from key to slot for range queries over records.
