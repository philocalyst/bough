# Round 6: the page is the node, for everything

State: `LEAD` (the lead's design for the façade's storage; the numbers are measured where a source is given).

## What bbtree says, applied honestly

bbtree's cost model, in its own order (`ref/bbtree/design/14-blogpost.md`, `06-cost-model.md`):

1. **chained cache lines** (a miss is ~100 ns, and a load whose address depends on the previous load cannot
   overlap with it);
2. **instructions** (~0.5 ns), and of those the **unpredictable branches** (~20 cycles each);
3. coherence traffic.

Its answers: put what one step needs in the lines that step already loads; search by *counting* over a
fixed-capacity column (compare everything, popcount) instead of branching; no pointers a reader must chase;
type-stable power-of-two nodes that never go back to the allocator; plans folded at monomorphisation.

bough-core follows this. **The façade above it does not, and that is where the time and the memory are.**

One painted box today is spread over seven tables in six index spaces:

| table | bytes | reached by |
|---|---:|---|
| core `Page` columns (depth, ink, kind, payload, handle, clip) | 23 | (page, slot) |
| `Bough::htab` `HEntry` | 24 | handle idx |
| `lay.eng.n` `DNode` (+ a heap `Vec` of children, + a boxed 360 B cache on first store) | 152 | handle idx |
| `lay.info` `LInfo` | 60 | handle idx |
| `rt.nodes` `Node` (holds a `Vec<f32>` for a kind almost no element is) | 120 | node id, from `LInfo.rtn` |
| `rt.runs` `Run` | 12 | run id, from `Node.run` |
| `inst` `Instance` (GPU) | 48 | `Run.first` |

A label adds `TextNode` (234 B, a `String`, a `TShape` with two `Vec`s) and `LeafCtx` (392 B, more `Vec`s). A
dynamic item adds `DynItem` (295 B: a tape `Vec` and a `Scope` of seven `Vec`s).

Measured consequences (this round):

* **2.5 KiB of Rust heap per mounted row** (lane `memory`'s ledger), against 23 B of core record.
* **~25,000 instructions to mount a row** of a box and two labels, 4,700 for a bare box
  (`redescribe_cost MOUNT=1`, slope 100 → 1,000 rows). The profile at 6,000 rows: cephal's cold flex pass ~40 %,
  `memmove` + malloc/free/madvise ~25 % (every one of these `Vec`s doubling and copying everything mounted so
  far), a `SpecKey` hash per element ~8 %.
* **Every update is a chain**: tape → `LInfo` → `Node` → `Run` → `Instance`, five dependent loads in five
  unrelated allocations to change one colour. Nothing overlaps, nothing vectorises, and at 10 K rows each is a miss.
* **A subtree that moves is rewritten node by node**, because each node's absolute position is a field of a
  struct in an array ordered by allocation history, not by paint order.

## The design

**Everything the framework knows about a record is a column of the record's page.** A core page already holds 64
records in paint order with SoA columns. The façade's state becomes more columns, in *side pages* that share the
core page's id and slot numbering:

```rust
/// One per core page, same id. CPU-only (the GPU never reads it). Columns are grouped by the pass that reads them
/// so a pass loads only its own lines: 64 slots x 16 B = one kilobyte per hot group.
#[repr(C, align(128))]
struct SidePage {
    // write-back, hit, translate: geometry in fixed point (1/256 px), space-local. 16 B/slot.
    geom: [[i32; 4]; 64],          // x, y, w, h
    // paint: 8 B/slot.
    color: [u32; 64],
    paint: [PaintBits; 64],        // kind, alpha, radius class, flags, margin class: u32
    // layout: 12 B/slot (+ sparse side tables for what few nodes have).
    style: [u32; 64],              // interned spec/style id
    lay_xy: [[f32; 2]; 64],        // cephal's location (size is geom.w/h)
    // links to the cold world, 4 B/slot: an index into a per-kind table (text, series, image), 0 = none.
    cold: [u32; 64],
}
```

* **Identity.** Inside the runtime a record is addressed by position while a pass streams, and by its core handle
  (index + generation) from outside. One hop (`htab[h] → page, slot`) replaces five. Node ids, run ids and layout
  ids disappear as separate spaces: the layout node id is the handle index, as it already is.
* **Moves with the record.** bough-core's `shift`, `copy_from`, `write` and `set_sentinel` on a page take a
  *column plan* (bbtree doc 07): the set of side columns that move with the records, folded at
  monomorphisation, so a split copies the side page's ranges in the same pass and no façade code ever fixes
  indices up after a splice.
* **Instances.** A single-instance element's GPU instance lives at the same (page, slot) in an instance page the
  GPU reads, so the record's payload is implied by its position and `Run` is gone. Multi-instance elements
  (bars, glyph-instance text) keep a run in a size-classed pool, referenced from `cold`.
* **Children.** A container's layout children are its direct children in the depth column: an iteration over the
  page (the SIMD depth scan the core already has), not a heap `Vec` per container.
* **Cold tables** (text state, wrap caches, series, images, hooks) are chunked arenas of fixed-size entries with
  type-stable storage (power-of-two chunks, a free list, no reallocation, no per-entry `Vec`: variable parts such
  as line spans live in one byte arena per window with (offset, len) references).

What falls out:

* **Mount is a bulk fill.** A row's records and side columns are written into consecutive slots of pages that are
  sized from the template; nothing reallocates, nothing is hashed twice, and independent pages can be filled by
  different threads.
* **A moved subtree is a vector add.** Geometry of a subtree is a contiguous range of `geom` columns in fixed
  point: `x += dx` over 64 slots is four NEON adds, exact (integers), and the snap-to-grid corrections that f32
  needed are gone (Joffe: widen the representation until the corrections disappear).
* **Write-back, re-description and the accessibility/inspector projections are linear scans** of pages in paint
  order: the hardware prefetcher streams them, and a per-page change stamp skips pages nothing touched.
* **Selection and other range state** (text selection, a selected-row set, focus-within, disabled-subtree) are
  bit columns or interval summaries on pages: "is anything in this subtree selected" is a page-summary test, and
  toggling a range is a mask write, not a per-row signal.
* **Memory.** Target for a painted box: 23 (core) + 48 (instance) + ~40 (side) ≈ 110 B, from ~440 B plus heap. For
  a label: + ~64 B of cold text state + its glyph block.

## Stages (each lands alone, each gated by every pixel suite and by measured cells)

| stage | change | gate cells |
|---|---|---|
| 0 | chunked, type-stable storage for the existing tables (no realloc, no copy); drop `Node.series` into a cold table; `Scope`/`DynItem` vectors into one arena per owner | mount instructions/row, heap bytes/row; every STATUS cell ≤ 1.02 |
| 1 | one index space: node id = layout id = handle index; `Run` only for multi-instance elements | update-chain length in the hot appliers (read the assembly); mount; redescribe |
| 2 | hot/cold split into SoA columns keyed by handle index (the layout of `SidePage`, still indexed by handle) | write-back and redescribe instructions per element |
| 3 | side columns become side pages moved by the core's column plan; geometry in fixed point; subtree translation as a column add | churn (remove/insert) per row; resize per row; scroll |
| 4 | parallel bulk mount over disjoint pages | cold mount of 10 K rows, single vs pooled, with the measured grain |

Targets by the end: mount ≤ 2,500 instructions per application row (10×), ≤ 300 B of heap per row (8×), a pure
subtree move ≤ 20 instructions per record (100×+ against today's ~2,700).
