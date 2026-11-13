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

## 4. What was built (2026-11-12; commits `2347684e44`, `0f436a0574`, `81da84ea75` and the one adding this section)

Amendments 2 to 5 arrived while this was being built. Nothing here appears in author code (amendment 2): the
arena, publishing, the image and the measurement cells are all under `World`. Amendment 5 moved the image to
the front; it is built for columns.

1. **`crates/bough-x/src/arena.rs`**: the scene in one reserved mapping: a shared part, a working half W, a
   published half F, both of one layout, addressed by offsets only. The plan (one 64-byte entry per column,
   indexed by a column number that never changes), the paint order, the spaces, the live words and the column
   pages are all in a half. `docs/r8/ARENA.md` is the description offered to the other lanes.
2. **Passes over the plan** (`store::delta`, `paint`, `stats`): the store tree is only what the sink uses to
   find sites. The expr lane's store at `8af3f228d0` (hot words, sweeps) was ported onto it by hand.
3. **Publishing**: the sink notes what it writes as dirty ranges; `publish` copies them W to F under an epoch;
   `Ar::read` is the optimistic reader. Design section 3.3 changed while building: not two alternating copies
   but a fixed working half and a fixed published half. The copy is the same; no site pointer or kernel has
   to know which copy is current, and on a discrete GPU the published half need not exist on the CPU.
4. **The image**: `World::save`, `World::open`, `World::adopt` (`docs/r8/IMAGE.md`).
5. **Measurement**: `examples/hot_x.rs`, `bench/r8/perf/hot.sh`, `hot_merge.py`, `bench/r8/remote/mini.sh`,
   `docs/lanes/threads/MEASURE.md`; both remotes set up (`/root/work/r8/threads`, `/home/miles/work/r8/threads`).

Not built: workers of any kind, the static split decision as code, product columns and their landing, the
chat scene, the pixel section of the image, per-page versions for readers (the epoch alone is used).

## 5. Results against the predictions

Hosts: **mothership** (Xeon E-2236, load 1, user-mode counters, `dist`, pinned to CPU 3), the host to believe
for cycles and wall time; **ilo-prod** (Ryzen 7 3700X, load 5 to 14, pinned to CPU 9); the Mac for instructions
only. Files: `bench/results/r8/threads/`. Sheet, 10,000 rows, 764 records exposed. x86-64 and aarch64
instruction counts are not comparable with each other.

### The A/B: store tree walk (`9466480f5d`) against the plan (`81da84ea75`), mothership, per repetition

| cell | level | tree: instr | cycles | L1 fills | LLC miss | plan: instr | cycles | L1 fills | LLC miss |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|
| idle turn | warm | 50 | 24 | 0 | 0 | 48 | 8 | 0 | 0 |
| hover a header | warm | 10,381 | 3,180 | 20 | 0 | 10,238 | 3,194 | 2 | 0 |
| theme | warm | 36,168 | 11,477 | 449 | 0 | 36,094 | 11,961 | 411 | 0 |
| edit an exposed cell | warm | 2,220 | 822 | 5 | 0 | 2,164 | 842 | 12 | 0 |
| scroll three rows | warm | 6,561 | 2,390 | 28 | 0 | 7,173 | 2,859 | 103 | 0 |
| scroll a page | warm | 43,455 | 15,387 | 287 | 0 | 44,187 | 16,043 | 353 | 0 |
| mount (user mode) | warm | 123,496 | 60,059 | 3,673 | 0 | 86,823 | 50,850 | 1,156 | 0 |
| hover a header | L1 cold | 10,381 | 3,668 | 196 | 0 | 10,238 | 5,313 | 203 | 0 |
| hover a header | L3 cold | 10,381 | 12,245 | 212 | 470 | 10,238 | 12,342 | 214 | 473 |
| theme | L3 cold | 36,168 | 25,106 | 575 | 980 | 36,094 | 23,117 | 551 | 919 |
| edit an exposed cell | L3 cold | 2,220 | 7,547 | 141 | 336 | 2,164 | 8,769 | 140 | 318 |
| scroll a page | L3 cold | 43,455 | 23,985 | 475 | 846 | 44,187 | 25,519 | 506 | 906 |
| full frame (reference raster) | warm | 8,719,438 | 9,432,215 | 536,027 | 615,824 | 8,719,204 | 9,442,120 | 535,561 | 615,434 |

Branch misses, warm, plan: idle 0, hover 3, theme 3, edit 1, scroll a page 15, mount 151, full frame 6,593.
Wall time, plan, warm / L3 cold: idle 7 ns / 43 ns; hover 213 ns / 2.9 us; re-describe 1.5 us / 3.9 us; full
frame 2.12 ms / 2.30 ms.

### Verdicts

| id | verdict |
|---|---|
| M1 | **Partly.** One script runs unchanged on three hosts and produced every table above. The perf stat cross-check (`-x`) has not been run, so "the two methods agree within 5 %" is untested. Flamegraphs: see section 6. |
| C1 | **Holds**, and the expr lane found the same independently (its SEAM section 9): five dependent loads through the store tree for the sheet's cells. |
| C2 | **Holds.** Two dependent loads at any nesting (read in the code: `World.ar` is in `World`; the entry's address is base plus a constant; the page is `entry.pages`). Hover and theme instructions are lower (-1.4 %, -0.2 %). Not asked by the prediction and worse: scroll three rows +9 % instructions and +20 % cycles (dirty notes for 22 sites and the live words on every list pass that writes a row). |
| C3 | **Hover holds** (2 L1 fills, 3 branch misses warm). **Theme killed**: 411 L1 fills warm, predicted under 8. The prediction forgot that the pages a theme switch differentiates (about 100 KiB) do not fit a 32 KiB L1; they are served from L2 (2 L2 load misses), which is the level they stay in. Nothing is wrong on the path; the number to watch for theme is L2 misses. |
| C4 | **First half holds, second half killed.** Cold is the real cost: hover is 3,194 cycles warm and 12,342 with L3 cold (3.9x); an edit 842 and 8,769 (10x). But the plan does not make it cheaper: 12,245 against 12,342 cycles, 470 against 473 lines from DRAM, 212 against 214 L1 fills. Three fewer chained metadata lines are invisible beside about 210 data lines and the path's own code, which the eviction also makes cold. A cold hover's cycles over its DRAM fills is about 19 cycles a line: the misses already overlap almost completely. **So "chained lines from a state write" is not what an interaction on this core pays for**, and ROUND8's budget of two is met without it mattering. What a cold turn pays for is its footprint: lines of pages differentiated and lines of code. Reducing that means differentiating fewer pages (the expr lane's dictionary form: hover differentiates ten pages to change two) and smaller kernels, not shorter chains. |
| T3 | **Holds except the retry rate.** Hover and theme cost nothing more (no column written, nothing published). A reader thread over 2,000,000 publishes saw no torn scene on the Mac and on ilo-prod; `mutant_torn` saw 381,982. Retries were 0.2 to 0.9 per read, not under 1 %: the test publishes back to back (about 1.4 million a second), so a reader is interrupted most of the time by construction; at a frame rate the same reader would retry about once in 10,000 reads. The prediction should have named the publish rate. Not measured: the writer's instructions with and without a reader (they share no written line but the epoch, written twice per publish). |
| I1 | **Killed, as section 1.5 expected.** Mapping is 0.62x of mounting in wall time (25.5 against 41.3 us), not under a fifth; map and adopt is as much as mount. In user instructions mapping is 11x cheaper (7,620 against 86,398), but both are system-call time. The corruption half holds: 3,000 corruptions on ilo-prod, 762 refused, 252 repaired, 1,986 harmless, none out of bounds. The saving for the sheet is about 15 us. See `IMAGE.md` section 4 for where an image would matter (the first frame's pixels: 2.1 ms). |
| T1, T2, T4, T5 | **Not run**: no worker exists yet. T5's turn cost is visible already: scroll a page is 44,187 instructions and 16,043 cycles (4.5 us) whatever the row count, and `scroll_far` (every row new, a jump of 100 windows) is the same. |

### What the numbers say about threads, before any is built

The full frame through the reference raster is 9.4 million cycles (2.1 ms), 536,000 L1 fills and 616,000 lines
from DRAM; the largest turn is 16,000 cycles. Three orders of magnitude. Every turn of this scene fits in
L2 warm and costs single microseconds cold. So section 1.2 stands with numbers: nothing in the turn is worth a
fork, and parallelism belongs to the frame (pixels) and to products that do not exist yet in this scene. The
frame's 616,000 DRAM lines for 4 million pixels (16 MB) is the surface being written: ownership has to be
stated for the surface, as section 1.3 said. That is the next experiment (T2), and it needs the raster lane's
kernels rather than the oracle.

## 6. Flamegraphs

`hot.sh -f` on ilo-prod: `perf record -e cycles:u --call-graph dwarf` records and `perf script` prints stacks
(checked by hand). The first run of the script produced no drawings: inferno was not on the host and
`nix shell nixpkgs#inferno` had to fetch nixpkgs first; the script now builds it once (`nix build`) or takes
`INFERNO=<dir>`. A second run was started (`/root/work/r8/threads/out/flame`); whether its files exist is in
the hand-over note at the end of this journal.

## 7. Refuted or failed, in one place

1. Own: C4's second half and I1 (above). The chain is not the cost; the image of columns is not worth much.
2. Own: C3 for theme (L1 is 32 KiB on both x86 hosts).
3. Own: T3's retry rate (the prediction did not name a publish rate).
4. Own design, section 3.3: alternating halves. Replaced by fixed W and F while building (no pointer changes
   hands; nothing has to track which copy is current).
5. Own: `setsid nohup` from an `rx.sh` script to start a remote job (it never started; the bench lane's
   `bg.sh` is the way). A group of six events on mothership is not counted; three are.
6. Own: the first hot_x cells for the image measured an image saved at set-up against a state later cells had
   scrolled, so "adopt" there includes a repair of 3,670 tiles. It is labelled; the same-state cost is the
   test's (no damage, no page written).
7. The lead's request for column bases computable from the table index: not built, with the reason and the
   measurement in `ARENA.md` section 2. Open for agreement.
8. Mount on the Mac got more expensive with the arena (165,744 -> 289,624 instructions including the
   kernel's): a mapping per world and its first-touch faults. In user mode on x86 it got cheaper (123,496 ->
   86,823). Wall on the quiet host: 41 us. Not chased.

## 8. Next, in order

1. Agree `ARENA.md` with expr and raster through the lead (the computable-base point; who calls `publish`;
   what a frame is handed). Until then both lanes are editing `store.rs`: expr should take this branch's
   store rather than build its own flat table.
2. For e2e: the pixel section and a `wl_shm` pool made from the image's file (IMAGE.md 4.1), once e2e says what
   its first-commit path accepts; `Arena` over memory it did not reserve (IMAGE.md 5).
3. The frame on workers, on the raster lane's kernels: bands of the surface owned by workers, the published
   half read by all; T1 (wake latency, the static split decision as a `const fn`) and T2 (bands against pages)
   on mothership.
4. Products and their landing (design 3.4), with the chat scene; T4.
5. `hot.sh -x` once, to close M1; kernel-mode counts for the start-up cells on ilo-prod (root).

## 9. Hand-over note (2026-11-12, at the first report to the lead)

* Head of `r8/threads`: see `git log`; the tree is clean; every test of `bough-x` passes on the Mac at it
  (anim 2, image 4, publish 2, sheet 4) and passed on ilo-prod at `81da84ea75`.
* **Not merged: `r8/expr` `054b83c929`**, which has its own flat column table (`ColEnt`), `World.root` boxed,
  `store::SHAPE`, `APP_WORDS = 56`. This branch has the same thing as `arena::Entry` in a half. One of the two
  has to go; `docs/r8/ARENA.md` is the proposal. The merge will conflict in `store.rs`, `world.rs`, `sink.rs`.
* Remote jobs of this lane: on ilo-prod a flamegraph run (`job2.sh`, output `/root/work/r8/threads/out/flame`,
  log `job2.log`) was still building inferno through nix when this was written; nothing else is running.
  The first run's stuck script was stopped by PID. mothership: nothing running; `target` and `target-base`
  under `/home/miles/work/r8/threads` are about 100 MB together.

## 10. Second batch (2026-11-12, after the lead's decisions and amendment 6)

Decisions taken as given: no computable column bases; expr's `ColEnt` table is the turn's entry and the arena,
publishing and the image go under it (done: section 11); then the cold interaction path, allocation, the
parallel frame, the cross-check.

### Predictions, written before the runs

| id | hypothesis | prediction | kill |
|---|---|---|---|
| L1 | where a cold hover's data lines are (about 210 loaded with L1 cold, mothership), by sampling every L1-missing load's address (`lines.sh`) | record pages at least half (ten pages differentiated: about 40 lines of records, 32 of rects for the two that change, the header page); the store tree 10 to 15 % (the hit test still walks it); table, block, tiles and world under 15 % together; the model under 3 % | pages under a third: the footprint is bookkeeping, and the plan of section 12 is aimed at the wrong thing |
| L2 | code is a large share of what comes from memory | of about 470 last-level misses per cold hover, 150 to 250 are instruction fetches (lines of code from L3 or memory) | under 60: code does not matter and only data layout does |
| L3 | differentiating only the pages that can change removes most of the cold cost | with a per-page summary of the record constants an `Eq`-like expression compares (a 64-bit Bloom-style mask per page per word read), a hover touches 2 pages instead of 10: data lines down by at least 30 %, cold cycles by at least 20 % | under 10 % in cold cycles |
| L4 | a prefetch at input arrival pays | issuing prefetches for the lines of the last turn of the same kind (the block, the readers of the hot words, their pages' record lanes) when the event is read, before hit testing and the handler, cuts a cold hover's cycles by at least 25 % | under 10 %, or any warm cell dearer by more than 2 % |
| A1 | where start-up's faults and allocations are | mount makes under 40 heap allocations and under 25 page faults (4 KiB pages) for 764 records; the arena's own part is 2 system calls and under 12 faults; every turn after the first makes 0 allocations | a turn that allocates; or mount over 100 allocations |
| A2 | what a global allocator choice can buy | under 2 % of mount's instructions and nothing on any turn (turns do not allocate): a faster `malloc` has nothing to be fast on. Removing the remaining allocations (store-tree vectors, the table, tiles) into the arena is worth at most the faults they cause, under 10 % of mount | a bump global allocator making mount more than 10 % cheaper |
| P1 | the frame on cores, on the raster lane's kernels (canvas: 316 ms on one core, 93 ms on six, 3.4x) | the gap to linear is memory (shared tiles and scene read by all, bands not kept by a core); bands of equal weight owned by a core across frames reach at least 4.8x on six and cut L2 misses per frame by at least 20 % against a rotating assignment | no gain over raster's own six-thread number within 5 %: the gap is not placement |

## 11. The port under expr's table (commits `84a41de1c6`, `a896d255fb`)

The lead's decision: expr's `ColEnt` world-wide table (`054b83c929`) is the turn's entry; one of the two flat
tables had to go, and it was this lane's. What is under it now, and where, is `docs/r8/ARENA.md`. The first
ARENA.md (own plan as the entry, column numbers, an order array, `Ar::read`) is superseded and was rewritten.

Costs against `054b83c929` built apart (Mac, `dist`, instructions, expr's own `sheet_x` cells; the same
binary's cells that write nothing are +0.0 %):

| cell | expr | port, first | port, final | |
|---|---:|---:|---:|---|
| scroll three rows | 5,321 | 5,563 | 5,422 | +1.9 % |
| scroll a page | 29,945 | 30,307 | 30,022 | +0.3 % |
| edit an exposed cell | 1,510 | 1,575 | 1,548 | +2.5 % |
| edit an unexposed cell | 982 | 1,012 | 985 | +0.3 % |
| hover, theme, idle, select, frames in motion | | | | +0.0 % |

How the scroll cost came down: the slot pages a pass wrote are worked out from the three runs of rows that
are written, not row by row; a list has its parent's space slot and writes none; one note per list per pass
(a mask), with `publish` working out the pages of every site of every column and the live words.

**The Mac's mount rise was not mount.** `hot_x`'s mount cell made a world and dropped the previous one inside
the bracket; the arena's reservation and release alone are 131 K instructions with the kernel's (`world_new`,
9 us). Mount itself, four runs each: 166 K to 340 K on the port, 205 K to 314 K on expr's commit: the spread
is first-touch faults, and the port's floor is lower.

## 12. The cold interaction path (prediction L1 to L4)

`bench/r8/perf/lines.sh` and what it found are in `docs/r8/THREADS-DESIGNS.md` section 1.

| id | verdict |
|---|---|
| L1 | **Killed.** Record pages are 18 of a cold hover's 93 data lines (a fifth), not half. The store tree was 24 (the hit test walked it and recounted it on every pointer event) and the heap 24 (the event path allocated). The plan was aimed at pages; the first thing to remove was bookkeeping. |
| L2 | **Killed on the low side, above its kill line.** 86 code lines from memory per cold hover (60 after the change), predicted 150 to 250. Still as many as the data lines. |
| (built) | The pointer path over the column table: no store walk, no allocation. L3 cold, three interleaved runs each: 12,160 -> 9,890 cycles, 407 -> 315 last-level misses; warm 10,888 -> 10,094 instructions; data lines 93 -> 70, store tree 24 -> 5. Cells whose code did not change moved up to 15 % between runs: "about a fifth". |
| L3 | **Not run.** The page summary is designed (THREADS-DESIGNS 1.2); with records now 12 of 70 lines its ceiling is lower than predicted (about 8 % of cold cycles, not 20 %). |
| L4 | **Not run**, and re-predicted down to at most 10 %: a cold hover's misses already overlap (about 19 cycles per line from memory). |

## 13. Allocation (A1, A2) and the first buffer

`examples/alloc_x.rs`, `bump_x.rs`, `pixels_x.rs`; THREADS-DESIGNS sections 2 and 3.

| id | verdict |
|---|---|
| A1 | **Half.** Every turn cell: 0 allocations, 0 page faults (both hosts). Mount: 61 allocations (predicted under 40: missed), 21 faults at 4 KiB pages and 14 at 16 KiB (under 25: holds); the arena itself 3 allocations and 4 faults. |
| A2 | **Holds against its kill, misses its number.** A bump global allocator makes world-and-mount 5.9 % cheaper in user instructions (90,117 -> 84,830) and nothing in cycles (52,134 -> 52,199); predicted under 2 %, kill at 10 %. A turn has nothing for an allocator to do. |
| first buffer | A 16 MB anonymous shared buffer touched once: 5,965 us and 4,000 faults on mothership. The same bytes from a file in the page cache, mapped read-only and read: 42 us, 11 faults. On the Mac the two are the same (930 against 800 us, 1,000 faults each). So e2e's proposal holds on Linux with a warm page cache and large folios. |

## 14. The independent review (batch 5) and amendment 7

Read: `bough/runtime/lanes/independent-review/recent-20261112/REPORT.md`, P01 to P05 and O02. Closed: P01 (for
the safe API), P02, P03, P04 (for the safe API), O02. Stands: P05. The table with how each was closed and what
is left is ARENA.md section 3. Tests added for them: `nothing_is_painted_before_the_first_publish`,
`a_reader_outlives_its_world`, `another_backend_is_refused`, the reader gate on the atomic `Reader`, and
`crates/bough-mem/tests/align.rs` (the review's own probe).

What the review was right about and I had wrong: (1) I called an epoch-retried read of plain memory "what
bbtree's readers do" and tested it for two million publishes; it was a data race in Rust's model whatever the
test saw. It is atomics now. (2) `paint_front` read a count from the shared head that mount had written
before any publish. (3) "Nothing in a turn is worth a thread" was a conclusion from one scene with arithmetic
layout and no text; withdrawn as a general claim (it stands for that scene, with numbers). (4) The image's
identity check authorised code offsets; the safe path no longer follows one before `adopt` has matched it.

No workers were built (the lead's instruction). The banded-kernel work on raster's `shade_into` was not
started; its design and predictions are THREADS-DESIGNS section 5, and it should come after frame versions
(section 4 there), which is the review's priority-0 answer and mine.

## 15. Flamegraphs and the cross-check

`hot.sh -f` on ilo-prod drew seven cells at `81da84ea75` (`/root/work/r8/threads/out/flame/*.svg` and
`.folded`; not copied back: the transfer is the terminal). Leaf frames, by share of user cycles:

* theme: `mark_rects` 46 %, `delta_col<Fill<..fill..>>` 11 %, `Tiles::mark` 10 %. **Theme is dominated by
  scanning changed rects for a bounding box and marking tiles, not by differentiating records.** A page whose
  every live record changed needs no scan: its box is the store's. That is expr's to take.
* scroll a page: `list_plan` 31 %, the row closure 20 %, `World::changed` 10 %, libm `round` 9 % (the scene's
  `f32` to hundredths conversion, a call per cell), `Tiles::mark` 5 %.
* mount: `list_plan` 10 %, the row closures 11 %, `Store::discover` 6 %, `seal` 2 %, `_int_malloc` 1 %.
* **A defect of the method for cells of a few microseconds**: hover's graph is 93 % `[unknown]`: the two
  counter reads that bracket each repetition are system calls, and their user-side stubs out-weigh a 3 us
  cell. Flamegraphs of small cells need the bracket off (an environment switch in the harness; not added).

`hot.sh -x` ran on ilo-prod at `81da84ea75` (`pstat-ilo-prod-81da84ea75.tsv`; `run%` 100 everywhere). Per
repetition, perf stat slope minus `floor` against the in-process bracket:

| cell | instructions, perf stat | in-process | | branch misses | L1 misses |
|---|---:|---:|---|---|---|
| hover | 11,180 to 11,290 | 10,232 | +9.7 % | 2.7 / 3 | 10 / 6 |
| theme | 36,956 to 37,060 | 36,088 | +2.6 % | 2.9 / 3 | 326 / 317 |
| edit an exposed cell | 2,731 to 2,837 | 2,158 | +28 % | 2.8 / 1 | 23 / 18 |
| scroll a page | 45,132 to 45,267 | 44,181 | +2.4 % | 11.5 / 11 | 463 / 390 |

**M1's second half is killed by its letter** (a cell over 10 %), and the cause is the harness, not the
counters: the loop reads out and clears the damage after each repetition, outside the in-process bracket and
inside perf's count, and `floor` has no damage to read. The difference is 600 to 1,000 instructions per
repetition whatever the cell, which is 28 % of an edit and 2 % of a scroll. The in-process numbers are the
cell; a perf stat slope needs a floor that leaves the same damage. Not fixed.

## 16. Hand-over note (2026-11-12, final)

* **Head of `r8/threads`**: the commit that adds this note, on top of `r8/expr` `054b83c929`. At
  `57bfea8094` (the same code; later commits are documents and results): `cargo check --workspace
  --all-targets` passes on the Mac; `cargo test --release -p bough-x -p bough-mem` passes on the Mac and on
  mothership (bough-mem 7 + 1; bough-x anim 2, events 9, image 5, publish 4, reflect 6, sheet 4, table 1);
  `mutant_publish` and `mutant_torn` fail their gates; the reader gate ran 2,000,000 publishes with no mixed
  scene (3.6 retries per read at that publish rate). ilo-prod last ran the tests at `81da84ea75` (before the
  port).
* **Section 9's note is superseded.** `054b83c929` is merged; this branch has no second flat table.
* **Not built**: workers of any kind; product columns and landing; the chat scene and any latency
  distribution; the pixel section; frame versions; banded kernels. Their designs and predictions are in
  `docs/r8/THREADS-DESIGNS.md`. Predictions T1, T2, T4, T5, L3, L4 and P1 were never run.
* **Documents**: `docs/r8/ARENA.md` (what is built, the review's findings), `docs/r8/IMAGE.md`,
  `docs/r8/THREADS-DESIGNS.md`, `docs/lanes/threads/MEASURE.md` (the recipe), `docs/lanes/threads/TALLY.md`
  (lines for the lead). Earlier sections of this journal describe the first version (own plan as the entry,
  `Ar::read`, column numbers): they are history, and sections 11 and 14 say what replaced them.
* **Remotes**: nothing of this lane is running. ilo-prod: `/root/work/r8/threads` keeps the checkout
  (`81da84ea75` with `hot.sh` and `hot_merge.py` copied over it), `out/` (tables, seven flamegraphs, the
  cross-check) and `env.sh`; `target` deleted. mothership: `/home/miles/work/r8/threads` keeps `env.sh`,
  `out/`, the three benchmark binaries `hot_x.port`, `hot_x.flat` and the mini source; `target` and
  `target-base` deleted. The Mac's `target/release` and `target/debug` deleted; `target/dist` kept.
