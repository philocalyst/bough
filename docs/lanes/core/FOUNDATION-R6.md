# Round 6 foundation: relevance

State: `LEAD` (a direction with its reasons, written by the lead; the cards that execute it are listed at the end).
Nothing here is measured unless a number is given with its source.

## The claim

bough tracks one thing exactly: which 32 px tiles of the window hold pixels that are no longer right (damage).
Everything fast about it follows from doing work only where damage is. Everything that does not generalise is a
place where work is still decided some other way:

| work | decided today by | consequence |
|---|---|---|
| painting | damage | exact, cheap |
| mounting, shaping, instance memory | the description: everything described is mounted (`list()` alone mounts by exposure, as a special container) | 10 K rows cost 10 K rows of memory and mount time; the author must pick a virtual list |
| re-description | the owner: one plain field re-walks the whole owner | 2,742 instructions per row per click before this round (`crates/bough/examples/redescribe_cost.rs`) |
| writes to hidden content | nothing: a write under an opaque panel damages its tiles and the GPU repaints them | tickers under a sheet, rows behind a sidebar cost full price |
| structure edits | position: everything after the edit is touched (directory, prefix sums, chunk summaries) | O(pages) per row inserted |
| moved content (scroll, insert, expand) | one special case (`scroll_run_copy`, one clip per epoch) | every other motion repaints what it moves |
| scaled or tiny content | nothing: a subtree at 5 % zoom shapes and shades every glyph | cost independent of what can be seen |

The generalisation is to make the question damage answers, *does this place need work?*, the only way any work
is admitted, and to ask it of more than pixels. Call the answer **relevance**: a record is relevant to a frame
iff it can change a device pixel the user can see. Four independent reasons make a record irrelevant, and each is
a set the Bough can represent with what it already has (page summaries, tile planes, paint order):

1. **Unexposed**: its ink does not meet the window through its clip chain. *(virtualisation)*
2. **Occluded**: every tile it touches has an opaque cover later in paint order. *(occlusion)*
3. **Unchanged**: its description and inputs are bit-identical to what was retained. *(seals, wires)*
4. **Unresolvable**: at the scale it is drawn, its detail is below a device pixel. *(level of detail)*

A relevant record still needs work only where it is damaged. So the invariant of the whole framework becomes:

> **Work is proportional to (relevant ∩ damaged), for every kind of work: describing, mounting, shaping, layout,
> binning, shading, and holding memory.** Exactness is unchanged: the presented frame equals a full redraw of a
> fresh mount, at every ring age.

## 1. Unexposed: materialisation is driven by damage

Scrolling already produces exactly the right signal: the exposed strip is damage, and repair asks the Bough for
the records that touch damaged tiles. If the records there are not mounted yet, that same query is the demand to
mount them. No list widget is involved.

* A dynamic part (`each(..)`, `Vec`, `List`) mounts as **dormant runs**: one record per run of consecutive items,
  kind `REC_DORMANT`, whose ink is the run's estimated extent and whose payload names the part and the item range.
  Dormant runs live in the same pages, summaries and layout as real records, so cull, hit, scroll by copy and
  layout treat them like any box of that size.
* The cull's result is partitioned: real records go to the draw list; dormant runs that touch damage are
  **realised** for the item range the damage covers (split the run, mount the items between, re-measure) before
  the frame is drawn. Realisation is a `mount` of exactly the code path that exists.
* The description supplies items by index without describing the others: `each((a..b).map(f))` and slice
  iterators have O(1) `nth`, so realising items `i..j` re-runs the owner's `view` (cost: the owner's fixed part,
  which seals make near zero) and skips straight to `i`. A source without cheap `nth` realises eagerly: correct,
  and measurably the author's choice of source.
* Leaving is the reverse, with hysteresis: a realised run that has been unexposed for some epochs and holds no
  focus, capture or IME composition is released back to a dormant record carrying its *measured* extent
  (CSS `contain-intrinsic-size: auto`: the remembered size is the estimate, `research/r6/web-engines.md` item 9).
* Estimates: the first realised item of a part, then the running mean of measured items. A correction when a
  run is realised moves everything after it; the scroll record anchors the first visible record so the correction
  is itself a scroll (already exact through scroll by copy).

What this buys: mount time, memory and re-description are O(exposed) for every container, with no API. gui-bench's
`rows` (10 K real rows) and `virtual` (1 M) become the same program. The gate is the existing one (retained ==
fresh mount of the same state, where the fresh mount is *fully realised*), plus: the set of realised items is a
function of the exposed tiles only (a mutant that realises one item too few must fail the pixel gate; one that
never releases must fail a memory bound).

## 2. Occluded: a write under a cover is not damage

For each tile keep the **floor**: the handle of the last record in paint order that covers the whole tile
opaquely (`NONE` if there is none). It is maintained where damage is marked: a record whose opaque ink grows or
shrinks over whole tiles updates those tiles' floors (the same span arithmetic as a mark).

* **Marking**: `mark(record, span)` skips tiles whose floor is after the record in paint order. Paint order of
  two handles is a rank compare (page rank, slot). Cost when nothing is covered: one load and compare per marked
  tile row (floors are stored as a per-row "any floor" bit plus the per-tile handles, so rows with no cover take
  the existing path).
* **Shading**: the tile kernel starts at the floor instead of scanning from the bottom (lane `gpu` found the
  current opaque floor is recomputed inside each 128-entry batch, so earlier batches are shaded and discarded).
* **Realisation**: a dormant run entirely under floors is not realised.
* When a cover goes away (the sheet closes) its tiles are damage, and the records beneath are found by the cull
  as always. Nothing was cached, so nothing is stale.

Exactness condition: "opaque" is certified from the record's kind and paint (alpha 255, no radius inside the
tile, no transform), never from pixels. A translucent or rounded cover simply is not a floor.

## 3. Unchanged: descriptions are compared as bytes

Built this round (`crates/bough/src/seal.rs`): every dynamic item keeps the bytes of what its description said
(specs by index into a table of the window's distinct specs, colours, radii, text), preceded by its inherited
context. A re-description compares the new description with those bytes in place and steps over the item when
they are equal. `Spec` is now plain bytes with compile-time proofs that it has no padding and no uninitialised
byte, so a spec comparison is eleven word compares, not a hash-table probe.

Measured (`redescribe_cost`, instructions per row per turn, 1,000 rows, one plain field changed):

| row shape | base | now |
|---|---:|---:|
| box + integer label + string label, as an application writes it | 2,742 | 574 |
| box + two `&'static str` labels | 2,313 | 522 |
| box alone | 465 | 148 |

Next steps on this axis, in order of expected yield:

* **Owners as blocks** (lane `surface-api`): a handler re-describes only its receiver. With dormant runs a row is
  an owner only while realised.
* **Seal runs, not items**: when a whole part's seals are contiguous and the new description's items are built
  into one buffer, the comparison is one streaming compare over the part, and the first mismatch gives the first
  changed item.
* **Edits by description**: when item `k`'s new description equals the retained seal of item `k+1` (or `k-1`),
  the description shifted by one: splice one run instead of rewriting every later item. Exact, because seals are
  the operands themselves, and it turns a `Vec` remove from O(rows × update) into O(rows × compare) + one splice.
  It must refuse for items with state (they do not seal).

## 4. Unresolvable: detail below a pixel is not drawn, and static detail is not redrawn

Two mechanisms with one decision rule (the cost model lane `core` owes: copied pixels vs repaired tiles vs entries
per tile):

* **Coverage cut**: a record whose transformed ink is under one device pixel in both axes contributes nothing a
  full redraw would show beyond its box's average colour. Certify per page (pages carry the affine): a page whose
  scale puts every record under the threshold is drawn as its summary rectangle. This needs the full-redraw oracle
  to define the same cut, so it is a rendering rule, not an approximation of one.
* **Retained raster of a quiet subtree**: a subtree that has not been damaged for N epochs but is being moved or
  scaled (a panel sliding, a zoom gesture, a list row being dragged) is rastered once into a tile cache and
  composited from there while it stays quiet (Chromium's raster scale vs ideal scale, `web-engines.md` item 7).
  This is scroll by copy generalised from "one clip translating" to "any quiet region under any affine", and the
  first damage inside it ends the cache. Under a non-integer or scaling transform the copy is not bit-identical to
  a redraw, so it is admitted only while the motion is in progress and replaced by an exact repaint at rest
  (browsers do the same on pinch end); the gate compares rest frames exactly and motion frames against the same
  cached-raster rule in the oracle.

## 5. Parallelism and SIMD follow from pages

A page is 64 records, self-aligned, type-stable, with its own offset and affine. That is a unit of work that needs
no lock if two workers never hold the same page:

* **Realisation** of dormant runs in different pages is independent (shaping, instance fill, record init) and is
  the largest cold cost (first frame, jump scroll, resize): parallel over runs with a join before layout.
* **Seal comparison** of a large part is a pure function over two byte ranges: split by item ranges.
* **Layout** of independent groups (cephal already has the pool).
* **Cull** over chunks of the directory.
* Each needs a measured grain: below it the single-thread path is the same code with one range.

SIMD stops being a property of a few kernels when every per-record test is a column test: exposure (ink vs
window, exists), occlusion (rank vs floor, a u32 compare per tile), unchanged (bytes), resolvable (ink width and
height vs threshold: two lane compares on the existing NRect column).

## Cards

| card | lane | first gate |
|---|---|---|
| dormant runs for `each` over an indexable source, realised by the frame's cull | `core` + `memory` | gui-bench `rows` at 10 K: mount and footprint O(exposed); retained == fully realised fresh mount |
| tile floors: skip marks and realisation under opaque covers; kernel starts at the floor | `core` + `gpu` | a ticker under an opaque sheet: zero damage, zero GPU work; pixel gates; a translucent-cover mutant must repaint |
| owners as blocks; routes in rows | `surface-api` | click in one row re-describes one row |
| edits by description (shift detection) | lead | `Vec` remove at 5 K rows: from 116.5 M instructions to one splice + compares |
| per-region motion (N scroll records per epoch) with the copy-or-repaint cost model | `core` + `gpu` | nested scrolls and row insert copied, not repainted; exact at every ring age |
| coverage cut and quiet-subtree raster cache | `gpu` | zoomed-out document: cost independent of record count at rest |
| parallel realisation and seal compare, with measured grain | `core` | first frame of 10 K rows; jump scroll |
