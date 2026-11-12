# Lane `layout`, round 7: journal

Branch `r7/layout` (worktree `wt/r7-layout`), from `round6` `12804f6899` (round-6 core, store and layout lanes
merged), with `r7/bench` merged in for the five standing scenes (`c95ff8e04b`). Brief: `bough/runtime/lead/ROUND7.md`
plus the lead's layout brief (memo of identical subtrees, resize as a first-class operation, zero allocations and
SoA lanes, exposure, text leaves). Newest entries last. Every experiment carries its prediction and kill criterion,
written before the run.

Provenance unless a line says otherwise: Mac = the shared M3 Pro (aarch64, macOS 15.7), under load from other lanes
(instruction counts do not depend on load; wall time does), nightly-2026-10-17, profile `release` (thin LTO).
ilo = ilo-prod (Ryzen 7 3700X, Zen 2, x86-64), shared and busy, same toolchain, `nice -n 10`. Unit: instructions
retired on the calling thread.

## Instruments built (commit `4c5df67a78`)

* `comparisons/bough-workloads/src/bin/lay_probe.rs`: a scene's mount and each step of the resize sweep, split into
  `App::resize` (which lays out) and the frame, with the layout profiler's compute share, the write-back's visited and
  written nodes, text measure calls and (feature `cephal-stats`) cephal's query counters per step. `DUMP=path`
  (feature `dump`) writes the scene's layout trees as JSON: interned styles, every node's style and children,
  measured text as its word table, the group roots with their available space, and per resize step every root's
  available space.
* `vendor/cephal/examples/replay.rs`: lays such a dump out through cephal's own `Tree` (cold, then the recorded
  sweep), with instruction counts per phase and the query counters. On the sheet at scale 0.3 it reproduces bough's
  counters exactly (99,076 cheap leaves, 108,068 size hits, 45,031 size misses, 39,029 final misses, 84,060 fused flex
  runs at mount; the same per step) and bough's compute cost within about 5 %. It is the lane's instrument for
  cephal changes: platform-independent, so it runs under `perf` on ilo.
* `LProf` (`layout.rs`) reads the calling thread's counter (`bough_mem::pmu`) instead of the process's
  (`proc_pid_rusage`), and the text spans inside the write-back no longer reset the enclosing span (they did: the
  write-back figure of round 6's probe was only the tail after the last text placed).

## R1. The sheet's resize spikes (bench E6), explained

Reproduction: `SCENE_SCALE=0.3 REPS=24 RENDER=0 PHASES=resize STEPS=1 workload sheet` (3,000 rows, 30,000 cells).

| tree | step 6 (1184 pt) | step 7 (1184 -> 1115) | step 17 | step 18 (1115 -> 1184) |
|---|---:|---:|---:|---:|
| `r7/bench` `392d870c13` (base `round6` `3b0b00c698`), Mac release | 46.5 M | **569.0 M** | 22.3 M | **538.3 M** |
| `r7/layout` `c95ff8e04b` (base `round6` `12804f6899`), Mac release | 36.2 M | 96.4 M | 22.0 M | 80.6 M |

What happens at those two steps (from `lay_probe` and cephal's counters, identical on both trees):

* The sheet's content is 56 + 10 x 110 = 1,156 pt wide. Each data row is `row((gutter .w(56), row(10 cells .w(110))))`
  stretched to the window. Steps 7 and 18 are the only steps that cross 1,156 pt.
* Below 1,156 pt the row must shrink. The inner row cannot (its automatic minimum, CSS `min-width: auto`, is its
  min-content width, 1,100 pt: ten cells of fixed width). The gutter can: its automatic minimum is
  `min(56, its content's min-content width)`, the row number's label plus padding, 19, 26, 33 or 40 pt for one to four digits (measured: the gutters of the 3,000 rows are 38, 52, 66 and 80 px wide at step 7) depending on the digit
  count. So the gutter shrinks to its minimum and **every one of the 30,000 cells moves left**, each row by a
  different amount (rows 1-9 by up to 37 pt, rows 1000-3000 by 16 pt: the sheet's columns are ragged below
  1,156 pt, which is CSS-correct and almost certainly not what the scene's author wanted; the same automatic-minimum
  rule is the round-6 decision 6 the lead holds). Crossing back (step 18) moves them all back.
* Layout compute at those steps rises only by one flex run per row (the gutter's new size): 59.5 M against 43.7 M at
  step 6 (cephal: `miss_final` 6,007 against 3,006; `flex_fused` 9,011 against 6,010). The write-back visits 69,052
  nodes and writes 63,036 (every cell's box and label, every gutter box) against 3,030 visited and 15 written at an
  ordinary step.
* On the bench lane's tree a written label cost about 8,000 instructions (the label was a shaped leaf placed as
  line windows: `place_text`, `lines_point`, run allocation, damage of the old and the new ink); since the store
  lane's `3ae78a4966` (merged into `round6` at `2db93d2120`) a laid-out one-line text is a retained label placed by
  moving its origin (`place_label`), about 700 per written node. That is the whole difference between 569 M and 96 M.

So the 14x steps were the write-back of a layout change that really moves every cell, at a per-label placement cost
that no longer exists on the integrated base; on `round6` they are 2.7x (step 7) and 3.7x (step 18) their
neighbours. What is left of them is: (a) the cells moving at all, which a rigid-move write-back (core's
`wb_rigid`) or a ragged-free scene would remove, and (b) layout work proportional to rows on every resize step,
which R2 is about.

## R2. Where a resize step's 22-46 M go (sheet, 3,000 rows)

Every ordinary step re-lays out every row: per row and step, cephal runs the row's flex algorithm for its final
layout at the new width (`miss_final` 3,006) and, in steps 0-12 and 23, once more for the row's height under the new
width (`miss_size` 3,004: the column's flex-basis question to a stretched row carries the row's width as a known
dimension); the ten cells and the gutter answer from their caches (`hit_size` 30,015, `hit_final` 6,003). Steps
13-22 cost half because the sweep returns to widths it already visited and each node keeps eleven size answers (an
LRU of the last widths), so the size question hits; only one final answer is kept, so the final question never
does. Cost: about 7,000 instructions per row per flex run. Nothing in a row changes except its own width, which
nothing paints.

## R3. The sheet's mount layout taken apart

`lay_probe` (Mac, release, `cephal-stats`) and the replay (ilo, release, the same dump), sheet at 0.3: 3,000 rows,
30,000 cells, 72,055 layout nodes (39,029 flex containers, 33,026 leaves; per row: the row, the gutter box and its
label, the inner row, ten cell boxes and their ten labels).

* **84,060 flex runs for 39,029 containers** (2.15 per container), every one through the fused path. Per cell: the
  inner row asks each cell its min-content width (CSS automatic minimum: `min-width: auto` on a box of fixed width
  still reads its content), then its final layout: 2 runs. Per row: 5 size runs (the column asks the row its height
  and its min-content height; the row asks the inner row its max-content and min-content width and the gutter its
  min-content width) and 3 final runs.
* 108,068 size answers from node caches, 99,076 cheap-leaf answers (each label is answered three times, never
  cached: cheaper to recompute than to look up), 45,031 size misses, 39,029 final misses.
* Cost: the replay's cold layout is 413.7 M (ilo, no counters) = **5,742 per node, 13,800 per cell**; about 4,900
  per flex run including the questions it asks. bough's own figure is 11,300 per cell (core lane, at 0.1).
* Where inside: a flat profile (`perf record`, ilo, 12 cold layouts): the fused flex algorithm 43 % (row containers
  26 %, column containers 17 %), `compute_flexbox_layout`'s preamble 15 % (resolving the container's style and its
  constants on every run), `compute_child_layout` and the cache 10 %, final-pass closures 3.5 %, `Questions` and
  scratch 3 %, the rest spread thin. No hot spot: the cost is the general algorithm run 84,060 times.
* **How many of those runs are needed**: the subtrees repeat. Interning every subtree by (style, children's shapes)
  (`runtime/lanes/layout/r7/tools/sigstats.py` over the dumps): the 39,029 containers have **738 distinct shapes**.
  A top-down walk that stops at the first container whose shape it has already seen reaches 6,811 nodes; the other
  65,244 (90.5 %) sit under a repeated subtree.

The same count for the other scenes (nodes under a repeated subtree, distinct container shapes / containers):
chat 34.5 % (7,010 / 12,771; 2,274 wrapping paragraphs are unique), ide 30.0 % (2,564 / 5,026), dash 27.5 %
(1,064 / 1,693), canvas 38.4 % (62 / 531).

Replay of all five (ilo, release, no counters; mount = cold layout of every group, resize = the recorded sweep):

| scene | nodes | mount | per node | resize step mean / max |
|---|---:|---:|---:|---:|
| sheet (0.3) | 72,055 | 413.7 M | 5,742 | 34.4 M / 51.6 M |
| chat | 23,387 | 317.1 M | 13,560 | 96.7 M / 125.2 M |
| ide | 30,166 | 77.4 M | 2,567 | 64.6 M / 77.4 M |
| dash | 4,108 | 24.3 M | 5,916 | 4.4 M / 11.4 M |
| canvas | 1,247 | 4.7 M | 3,750 | 3.6 M / 4.2 M |

The IDE's resize step costs 83 % of its whole cold layout: every editor line is a row stretched to the editor's width
and is re-run at every width, though its tokens have fixed sizes.
