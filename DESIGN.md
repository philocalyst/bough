# bough — design

A retained Rust GUI framework built from scratch to beat zgx and zortax/zgui on every benchmark they publish,
keeping the GPUI-rework public API (`View::view(&self) -> El<'_>`, inline `Signal<T>`, `Source` routes,
`List<T>`, `Entity<T>`, `Reader<T>` with fetch-high/block-low async, `changed`).

Evidence base: `research/00-bbtree-synthesis.md` (first-hand), `research/01..03` (agent surveys),
`research/04-reformulation-method.md` (the method every iteration follows), zgx `docs/architecture.md` and
`docs/performance/floor-plan.md`, the archive `reactivity/{DESIGN,CONTRACT}.md` and campaign record.

## 0. What has to be true to win

zgx's own measurements (macOS, heavy stream) put a frame at **8.30 M instructions** (update 2.2 M, layout 1.5 M,
damage 1.4 M, render 3.2 M) against a hand-written floor of **2.72 M** that is mostly text shaping (1.42 M,
~13 K per changed label) and wgpu/Metal submission (0.99 M). One colour write costs zgx ~24 K instructions of its
own plus ~83 K of Metal. zgx's GPU cost includes a full-surface copy of its retained texture every frame.

So "obliterate" means going **below zgx's own floor**, which requires attacking all of:

1. **Update**: a write must *be* the patch. No effect re-run, no `Styles` rebuild, no dirty queue sort.
2. **Layout**: none for paint-only changes; incremental (cephal) over our own storage, no second tree.
3. **Damage**: O(changed), no flags to clear, no ancestor walks, exact old/new support.
4. **Paint**: no paint phase. The retained GPU instance *is* the paint output; bindings write it in place.
5. **Text**: a table-driven SIMD shaper for the common case, verified bit-identical against full shaping.
6. **Submission**: direct Metal, one uber-shader draw over a paint-ordered draw list, a few K instructions.
7. **GPU**: buffer-age repair into a ring of surfaces we own — no full-surface copy; fragment work ∝ damage.
8. **Memory**: shared immutable templates, 64-record pages from a type-stable pool, no per-node heap objects.

The previous GPUI campaign (168 branches) failed because every gain was a *bolt-on* fast path that taxed the
tiny/flat case by 5–10%. Here each mechanism is the only path and must be cheapest in both regimes.

## 1. The Bough: one integrated display tree

One structure per window holds structure, geometry, paint, hit-testing and damage. No separate scene graph,
spatial index, dirty queue, or inspector mirror.

**Records in paint preorder.** Every node is a record; a subtree is a *contiguous run*. Mount/unmount/reorder
are run splices. Children order = paint order.

**Pages.** Records live in fixed 64-slot pages (power of two, self-aligned, from a type-stable pool that never
returns memory to the allocator mid-session; a page's version only grows). Columns are SoA inside the page:

| column | type | purpose |
|---|---|---|
| `depth` | `u16` | tree shape (preorder + depth ⇒ parent, subtree end, children by SIMD scan; no pointers) |
| `ink` | `NRect = [i16; 4]` | absolute device-pixel ink bounds in *negated-max* form `(x0, y0, −x1, −y1)` |
| `stamp` | `u32` | epoch of the last visual change (versions, not flags) |
| `kind` | `u8` | record kind (decided once per run, never per pixel) |
| `payload` | `u32` | index into kind-specific storage (instance runs, text, images, layout cache) |
| `handle` | `u32` | stable handle id if something outside addresses this node, else 0 |

Padding slots carry sentinels (`depth = 0xFFFF`, empty `ink`, `stamp = 0`) so every SIMD scan runs at capacity
with no tail mask (bbtree invariant 4).

**Directory.** A flat, ordered array of page ids with SoA page summaries: union `ink`, `max_stamp`,
`min_depth`, `len`. Two levels total. For UI-scale trees (≤ ~100 K mounted records ⇒ ≤ ~1.6 K pages) a SIMD
scan of the summaries is cheaper than any internal-node descent (bbtree lesson 2: warm levels cost
instructions, not misses). A third level is added only if a benchmark shows it.

**NRect algebra.** With `r = (x0, y0, −x1, −y1)`: `intersect(a, b) = max(a, b)`, `union(a, b) = min(a, b)`,
`empty(r) ⇔ x0 + r[2] ≥ 0 ∨ y0 + r[3] ≥ 0`. Clip chains, damage accumulation, cull and hit tests are lane-wise
min/max — one instruction per pair, 2 rects per NEON register, 4 per AVX2.

**Structure queries without pointers** (depth column + page `min_depth`):
- subtree end of `i`: first `j > i` with `depth[j] ≤ depth[i]` — compare + movemask + ctz per 16 slots;
  whole pages skipped when `min_depth > depth[i]`.
- parent of `i`: last `j < i` with `depth[j] = depth[i] − 1`.
- inherited state (translation, clip, opacity) during a run: a depth-indexed stack `state[d] =
  combine(state[d−1], local)` — one load/combine/store per record, branchless.

**Stable addressing.** Only nodes addressed from outside (binding destinations, focus/capture targets,
component roots) get a handle; the handle table maps handle → (page, slot) + generation and is fixed up when a
splice moves records (scan the moved run's `handle` column for non-zero). Template-internal nodes are
addressed as `root + ordinal`.

## 2. Damage: versions in, tiles out

**Writes stamp, they don't flag.** Every visual mutation sets the record's `stamp` to the current epoch and
lowers the page's `max_stamp` invariant (`max_stamp = max(stamps)`, maintained on write: one `max`). Nothing
is ever cleared. Any number of consumers — each of the 2–3 presentation surfaces, accessibility, the inspector,
a remote mirror — keep their own "last seen epoch" and ask *what changed since e* independently
(WebRender keeps 64 frames of per-tile dirty bits; stamps make that unbounded and per-record).

**Old support is captured at write time.** Before a record's `ink` changes, its old `ink` is unioned into
the epoch's damage; after layout, the new `ink` is. Paint-only writes (old = new) union once. Removal unions
the whole run's page summaries. This is write-through; there is no later pass that rediscovers changes.

**Damage planes.** An epoch's damage is a tile bitmap (16×16 device px tiles; rows padded to whole words,
phantom tiles past the edge) plus its NRect bounding box. A ring of the last N planes gives buffer-age repair:
`repair(age) = OR of the last age planes` — a handful of vector ORs. Rect → tile bits is branch-free
(row mask = `((1 << tx1) − 1) & !((1 << tx0) − 1)` OR'd into rows `ty0..ty1`).

**Cull to a draw list.** For a repair region: SIMD-test page summaries' `ink` against the region box, then the
surviving pages' record `ink` (and optionally the tile plane), emitting instance runs in paint order. This is the
only per-frame traversal and it streams contiguous columns.

## 3. Paint: the instance is the output

There is no paint phase. Each drawable record owns a run of GPU instances in an `InstancePool` (power-of-two size
classes, free lists, never irregular sizes). Bindings write instance fields directly (a colour write = 4 bytes).
On Apple silicon the pool is a `storageModeShared` `MTLBuffer`: CPU writes are GPU-visible; nothing is uploaded.

Positions: instances store coordinates relative to a **space** (a node origin / scroll container / transform).
The vertex shader adds `spaces[instance.space]`. Moving or scrolling a subtree rewrites one space entry, not its
instances. Absolute `ink` in the Bough is still updated (SIMD add over the contiguous run) because damage and
culling need it.

**One uber-shader, one draw.** All primitive kinds (solid/rounded rect, border, shadow, glyph, image, gradient)
share a 48-byte instance format; the fragment shader branches on kind (coherent per tile on TBDR GPUs). The
frame's draw list (u32 instance indices in paint order, for the repair region only) feeds one instanced draw,
scissored to the repair box and stencil/tile-masked to the repair plane.

## 4. Presentation: buffer age without a copy

zgx renders into a retained texture and copies it to the drawable every frame. We instead present from a ring of
3 IOSurface-backed textures we own (or `CAMetalLayer` drawables when their identity/age can be proven), track each
surface's age exactly, and repaint `repair(age)` into it directly. GPU work is proportional to damage; there is no
full-surface copy. Idle does nothing at all.

## 5. Text

A per-font **simple-script table** (glyph id, advance and kerning class for each codepoint in a covered set,
plus the set of GSUB/GPOS triggers that require full shaping) shapes common UI text by table lookup:
NEON `tbl` maps 16 bytes of ASCII to glyph ids and advances at once, a SIMD prefix sum places them, kerning pairs
come from a class-pair table. Any string touching a trigger falls back to full shaping (rustybuzz/swash).
Exactness is a test: the fast path must equal the full shaper bit-for-bit over exhaustive short strings and real
corpora. Glyphs rasterize once into an atlas (power-of-two shelves). A text record's glyph run is its instance run.

## 6. Layout

cephal (the user's retained, incremental, parallel CSS engine, Spineless Traversal) runs over our storage via its
`LayoutTree`/`CacheAccess`/`Incremental` traits: styles come from shared const templates, caches live in a side
column only for nodes that need them. Paint-only and translation changes never enter layout. Layout writes new
geometry → the Bough updates `ink` (capturing old support) and the space table.

## 7. Reactivity and the API

Public surface follows the archive contract: `View::view(&self) -> El<'_>` (typed builders implement `Element`;
`El` erases only where branches unify), inline `Signal<T>` (value + 4-byte wire head), `Source` consts as
routes, `List<T>` with durable `Slot` identity, `Entity<T>`, `Reader<T>` for async, `changed` for effects.

- **Mount lowers types to templates.** A builder tree's type determines its topology at compile time
  (`const NODES`, const records); mounting memcpys the const template into pages and installs wires.
- **Wires are intrusive and typed.** A `Signal<T>` holds the head of its wire list (Ripple's `tracked.sb`,
  Solid 2's dual-list edges). `DerefMut` enqueues the signal once per turn (a bit in the head); the turn's
  settle runs each queued signal's wires, each a monomorphic patch `fn(&T, dest)` that writes record + instance
  fields and stamps. No graph walk for static wires, no owner re-render for direct wires.
- **Creation order is topological order** (Ripple): owners get monotonic ids; the settle queue sorts only if
  pushed out of order.
- **Dynamic dependencies** (projections, branches) use recycled read-order links and a height heap (Solid 2).
- **Async = fetch high, block low** (Solid 2.0): an async value is a memo; reading a not-ready value is
  `Pending`, which only consumers observe; on revalidation pending sources *hold* their old value and everything
  derived holds with them until the cohort lands together (no tearing, no transition API); `is_pending`/`latest`
  are per-signal questions. Boundaries show fallbacks only for first readiness.

## 8. Memory

Shared immutable templates (topology + const styles) are interned once and borrowed by every instance. Per-node
cost is the hot columns (~20 B) plus only the deviations it actually has. Pages, instance runs, glyph runs and
text buffers come from size-classed pools with free lists; steady state allocates nothing. Departed pages (route
transitions) freeze into compacted, position-independent page images that the GPU can still draw from during the
transition and that restore by memcpy. Per-turn scratch is a reset arena.

## 9. Parallelism and SIMD as structure

- SIMD is the *shape of the data*, not an afterthought: depth scans for structure, NRect min/max for geometry,
  stamp compares for change, tile planes for damage, `tbl` lookups for shaping, prefix sums for text and flex
  placement, SoA animation lanes evaluated 4–8 at a time.
- Threads follow immutable epochs: pages are copy-on-write per presentation epoch (pages are 1–4 KiB, so COW
  is cheap), so the render/encode thread, accessibility and the inspector read a frozen snapshot with no locks;
  the UI thread keeps mutating the next epoch. Heavy cold work (initial mount layout, big relayouts, bulk shaping)
  fans out over cephal's pool and a shaping pool above measured grain thresholds. CPU% is a summed metric, so
  the steady-state hot path stays on one core and does very little.

## 10. Measurement

zgx's own harness is the judge: `comparisons/` adapter contract (heavy, lab, animate, vlist, blink, field),
`micro_bench` (state, vscroll-fixed, vscroll-variable: write and frame), headless instruction suites, the macOS
windowed suite (CPU %, GPU Mcycles/s, memory, p99 frame interval, blank rows). Baselines live in
`bench/baselines/`. Every subsystem keeps a dumb oracle (full recompute) and differential tests.
