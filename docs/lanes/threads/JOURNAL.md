# Threads lane, round 8: journal

Branch `r8/threads` (worktree `wt/r8-threads`), from `r8/expr` `9469bf18f1`. Nothing is pushed. Brief:
`bough/runtime/lead/ROUND8.md` sections 8 and 9 and Amendment 1; the measurement method for every lane. Newest
entries last. Every experiment has its prediction and kill criterion written before the run; section 2 was
committed before any code of this lane existed.

Provenance unless a line says otherwise: the shared M3 Pro (aarch64, macOS 15.6), `nice -n 15`,
`CARGO_BUILD_JOBS=2`, nightly-2026-10-17, profile `dist`. The Mac has no cache or branch-miss counters and its
wall time is meaningless under load, so Mac numbers are instructions and cycles of the calling thread. ilo-prod
(Zen 2) is reserved; every cache, branch-miss and wall-time column is empty until the lead frees it, and says so.

## 0. State of the base

`r8/expr` `9469bf18f1` builds, but 3 of its 4 sheet gates fail (the unfinished `Time` / glide work in the saved
WIP). The lead said the expr lane is fixing it and not to debug it here. One observation passed on, not a fix:
the failing pixels are whole selection outlines (2,048 = one outline's border), and a glide that starts where
its record already is has `offset(old) == offset(new)` at that instant, so the record is not in the changed
mask and `delta_col` never registers its sweep. Until the fixed hash arrives, nothing here is gated on the
selection glide.

## 1. What the brief gets wrong or leaves open, after reading the code

1. **The chain is longer than the expr journal counted.** It counts block -> column table -> page (3, after
   moving the kernel pointer into the column header). But the sheet's cells are in `root.subs[0].subs[0]`:
   `World` -> `root.subs` buffer -> the scope's `Store` -> its `subs` buffer -> the list's `Store` -> its
   `cols` buffer -> the page. Each `Vec` is a dependent load: **five chained lines** from a block write to a
   cell page, and the depth grows with the nesting of scopes. The budget is two.
2. **Almost nothing in the new core is worth a thread.** At 10,000 rows the whole delta of a theme switch is
   30,755 instructions, a page of exposure 30,685, a whole re-description 15,704: 3 to 7 microseconds each.
   Round 7 measured a scoped spawn at 78 to 384 microseconds; even a parked worker's wake is several
   microseconds. So for the turn, the right amount of parallelism is none, decided statically, and the
   brief's "passes chunked by page across workers" would lose on every pass the sheet has. Where the bytes
   are: pixels (a full frame is 16 MB written; all columns together are under 256 KiB) and the Mount-stage
   products that are expensive per record (shaped text, decoded media, real layout).
3. **"A page has an owning worker so its cache holds its pages" is probably the wrong unit for raster.** A
   core's L2 (512 KiB on Zen 2) holds every column of the sheet whoever owns them; what does not fit anywhere
   is the surface. Ownership has to be stated for both: a worker owns a **band of the surface** (the pixels it
   writes) and the **ring slots** whose rows are in that band at rest (the pages it materialises products for
   and reads). For a flow store these are the same rows until it scrolls. Prediction T2 below is written to
   decide which of the two matters; if page affinity is not measurable, the design keeps bands only.
4. **Optimistic readers cannot serve the GPU**, and for a CPU raster a version check per page gives frames that
   mix two turns. bbtree's version lock is right for readers that want a few records at an arbitrary moment
   (accessibility, the inspector, a worker reading a record's operands). For the frame it is the wrong tool:
   see section 3.3.
5. **The image saves little for the sheet as it stands.** Mount is 190 to 230 thousand instructions (21 to 38
   microseconds). Mapping the columns can save most of that and nothing else. The image earns its place when
   it holds what is expensive: layout shapes, shaped glyph columns, decoded media, and the first frame's
   tiles. The format is built for those; the sheet's numbers will be reported as they are.

## 2. Predictions and kill criteria (written 2026-11-12, before any code of this lane)

Scenes: the sheet of `crates/bough-x/scenes/sheet.rs` at 10,000 rows (100,000 cells) and 1,000,000 rows;
a chat with media (this lane builds it in `bough-x`: rows with a decoded image and a shaped text product).

| id | hypothesis | prediction | kill |
|---|---|---|---|
| M1 | the recipe is one script | `bench/r8/perf/hot.sh BIN CELL...` runs unchanged for any binary that honours `OP`, `REPS`, `EVICT`: per cell a table of instructions, cycles, branch misses, L1 misses, L2 misses, refills from L3 and DRAM, each warm and after evicting L1 / L2 / L3, plus a flamegraph; perf stat slopes and the in-process counters agree within 5 % on ilo-prod | a cell whose two methods differ by more than 10 % with `run%` at 100 |
| C1 | chained lines, state write to a cell page, as built by expr | 5 (read in the code and the disassembly), growing by 2 per enclosing scope | fewer than 4: section 1.1 is wrong |
| C2 | a flat pass list (the plan) makes it 2 | block compare -> plan entry -> page, at any nesting; hover and theme instructions within 3 % of the tree walk's or lower | a third dependent load on the path, or instructions up by more than 3 % |
| C3 | warm misses per interaction | at 10,000 rows, L1 misses per hover under 4 and branch misses under 12; theme under 8 and under 40 | hover over 30 L1 misses warm: something on the path is not staying in L1 between turns, find it |
| C4 | cold footprint is the real cost of an interaction (other work runs between inputs) | after L2 eviction a hover touches 100 to 160 distinct lines with the tree, at least 20 % fewer with the plan, and its cycles drop by at least 15 % | under 5 % in cycles: the chain is not what a cold turn pays for; say what is |
| T1 | no pass of the turn is worth splitting | a parked worker's wake-to-run latency is at least 3 microseconds p50 on both hosts; a `const fn` over a store's row constants and capacity classes every pass of the sheet and of the chat's operand rows as `Inline` | a pass of either scene that the `const fn` classes `Split` and that then runs slower split than inline (a false positive), or one it classes `Inline` whose inline cost exceeds twice the measured fork cost (a false negative) |
| T2 | what a worker should own | owner-affine page assignment against a rotating one (same work, same threads): L2 misses and cycles per frame differ by under 5 % on full repaints of the sheet, and by over 20 % (L2 misses) on small repeated repairs (hover strips, an animation in place) only when the *tiles* stay with a worker | if page affinity alone moves full-frame cycles by more than 10 %, section 1.3 is wrong and pages are the unit; if nothing moves by 5 % anywhere, ownership is not worth any mechanism and the design is "bands, statically cut" |
| T3 | the writer never waits and no frame mixes turns | columns double-buffered with replay of the previous turn's dirty pages: hover and theme cost unchanged (no column is written), edit and scroll cells up by at most 10 %; a snapshot reader thread walking every exposed record while the writer runs 1,000,000 turns sees no torn row and retries under 1 % of its reads; the writer's instructions per turn are the same with and without the reader. Mutant: a reader that skips the version check sees torn rows | a torn row with the check in place; or the writer's turn costing more than 2 % more instructions while a reader runs |
| T4 | the interaction path never waits for bulk work | chat scrolled a page per frame with decodes and shaping on workers: the turn thread's cost per frame has max under 2x its median and under 1 ms of thread time, at every frame; a hover written while bulk work is queued reaches the damage bitmap in the same number of instructions as with nothing queued (within 2 %). Negative control: the same products computed inline have frames over 8.3 ms | any frame's turn over 1 ms of thread time with workers on; or the control not exceeding the budget (then the scene has no bulk work and proves nothing) |
| T5 | sheet at 100,000 cells scrolled fast | every frame's turn under 60,000 instructions (two pages of exposure) whatever the speed, flat from 10,000 to 1,000,000 rows; no allocation after the first frame | a frame whose turn grows with scroll distance beyond one window of rows |
| I1 | the mapped image | map plus the exact check (the view re-described against the mapped columns) is at most a fifth of mount's instructions; a corrupted or foreign image is refused or repaired, never undefined behaviour, shown by a fuzz of bit flips and truncations; saving for the sheet at most 200,000 instructions (so: not worth it for columns alone, section 1.5) | map plus check over half of mount; or any corruption that reaches an out-of-bounds access |

## 3. Design (before building; what was built and how it differs follows in later sections)

### 3.1 One arena, laid out like bbtree's file map

A world's scene is one reserved range of address space, committed on demand, never moved:

```text
base    head      magic, fingerprint, counts, the block as last shown, epoch
        plan      one 64-byte entry per column, in paint order: reads mask, kernel, page offsets (front and
                  back), page count, live-word offset, space index, owner
        spaces    {clip, base, scroll word} per space
        live      the live words of every store
        columns   each column's pages, VM-page aligned, front and back copy
```

What every interaction crosses (plan, spaces, live) lies together at the front, as bbtree-mmap puts the levels
a lookup crosses first. Everything in it is an offset from `base` (kernels: an offset from an anchor in the
binary's text), so the same bytes are the running scene and the file image, with no fix-up pass on mapping
(bbtree: "edges are offsets from their node"). The store tree of the expr lane stays what the sink uses to
find sites; nothing on the damage or frame path reads it.

Chained lines from a block write: plan entry (sequential, so the prefetcher runs ahead), then the page. Two.

### 3.2 Parallelism decided statically

A store's row type gives, as constants, the records per row and the bytes per record; its capacity is bounded
by the surface. A `const fn` classes each pass of each store: `Inline` (the turn thread, always) or `Bulk`
(sliced on workers). The turn itself (compare, delta, exposure of operands, re-description) is `Inline` for
every store whose worst-case pass is under the fork threshold; that is every store of the five scenes at
window sizes. What is `Bulk` is named by type: a record's *product* (shaped text, a decoded tile, a laid-out
shape) is a function of the record's own operand words only, never of the application's state, so it can run
on any thread without borrowing the model. That makes it one more stage between `Mount` and `Turn`.

### 3.3 Who may write a page a frame is reading: nobody

Three candidates for a frame (CPU workers or the GPU) reading columns while the next turn runs:

* *version per page, reader retries* (bbtree's lock): impossible for the GPU, and on the CPU a retried tile
  shows the next turn's records beside tiles of this turn: a frame that mixes turns.
* *copy-on-write per dirty page* (the raster lane's assumption): a page then needs an indirection (one more
  chained line on every pass) or a remap system call per dirty page, and the column stops being one
  contiguous GPU buffer.
* *two copies of each column, written alternately*: a frame reads the front copy, which nobody writes for the
  whole frame; the turn writes the back copy after replaying into it the pages the previous turn dirtied
  (the per-column page bitmask the raster already receives as its upload set); settle flips. No indirection:
  both offsets are in the plan entry. Cost: the columns twice (512 KiB for the sheet) and one copy of each
  dirty page per turn. A turn that writes no column (hover, theme, time: every Frame-stage write) replays
  and flips nothing.

The third is the design. It is copy-on-write per dirty page with the copy made one turn late and into a fixed
place. The pipeline holds one frame in flight: turn N+1 may run while frame N is shaded; it flips when frame
N's readers are done. If they are not, the turn thread does not wait: it keeps the block, which is its own
copy, and the relations' journals, which are logs; the next flip takes all of it. Input is never queued behind
raster.

Readers that are not the frame (accessibility, the inspector, a worker reading a record's operands for its
product) read optimistically: epoch, then the front copy, then the epoch again, and for long walks a version
per page so that only the pages a turn wrote are re-read. They write nothing shared: no reader count.

### 3.4 Bulk work lands as an epoch

A worker owns a range of a list's ring slots. Jobs name pages, not records (a page index and a mask of slots
that need their product); the worker reads the operands from the front copy by version, computes into an
arena only it writes, and posts (page, slot, generation, handle) on a ring only it writes. At the start of a
turn the turn thread drains the rings: a posting whose generation still matches its slot becomes one operand
write and one tile mark, exactly as an edit would. A record whose product has not landed paints its
placeholder; its extent is already known, so nothing moves when it lands.

### 3.5 Measurement (section 8, for every lane)

`bench/r8/perf/hot.sh` and a harness convention: a binary takes `OP` (the cell), `REPS` (so a slope between
two values subtracts set-up), and `EVICT` (`none`, `l1`, `l2`, `l3`: walk a buffer of that size between
repetitions so each operation starts with that level cold). Warm misses say what the steady state costs; the
evicted runs count the lines an operation touches (L1 misses after L1 eviction) and how many of them are
chained (cycles after L3 eviction divided by a DRAM fill is a lower bound on the dependent ones). An input
arrives after other code has run, so the cold numbers are the ones that predict interaction latency.
