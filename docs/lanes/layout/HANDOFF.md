# Lane `layout`, round 6 (second pass): handoff

Session: one Fable agent, 2026-11-11 night to 2026-11-12, stopped by the wrap-up order. Card:
`docs/lanes/layout/cards/2026-11-11-layout-proportional-r6.md`.

## State

* Branch `lane/layout/round6`, head `76817a1af1` (+ this file). Based on `lane/store/round6` at `b68c141937`.
  The stopped first layout lane's wip (`81dca613ea`, moved-only placement) is kept as branch
  `lane/layout/round6-stopped`; it was not merged.
* Nothing of this lane is merged into `round6`. The lane branch has not been rebased onto the store lane's newer
  work (the lead said a store merge is coming; the conflict surface is small: `layout.rs` `attach`, one hunk in
  `describe.rs`).
* Commits: `b088e6841e` card; `76817a1af1` the overflow cut in cephal, the differential test, two defect fixes,
  the mutants, the `signal_cost` probe, `vendor/README.md` section for cephal.
* Of the brief's four items: (1) relayout boundaries: done in its exact form (see below); (2) the unchanged-child
  scan, (3) cold layout, (4) moved-only translation: measured or analysed only, not built.

## What was built

1. **The parent-visible cut (cephal).** Measured cause of "a change inside a fixed box re-runs its ancestors": not
   the size. A label resizing inside a `.w(960).h(32)` row left the row's answers equal except its overflow
   rectangle, which `same_for_parent` compared exactly, so the scheduler marked the root column for an in-flow
   overflow gather (`gather_inflow_overflow`) over **all N rows** (cephal stats: `inflow_gathered=1` per write).
   A parent reads an in-flow child's overflow only through `scrollable_overflow_contribution`, which clamps it to
   the box the parent placed: `contribution_extent(size, start, end) = (start.min(0), size.max(end))`. Equal extent
   bits at the placed size (read from the child's stored layout, because a grid can place an item at its style
   size rather than its answer's) mean the parent's union is bit-identical, so the change stops at the child.
   Out-of-flow boxes keep the old comparison: their containing block's placement record holds their overflow.
2. **`crates/bough/tests/lay_diff.rs`**: random three-level trees through the public builders (fixed, percent and
   automatic sizes, minimums, padding, margins, gaps, grow/shrink, alignment, clip, both directions; labels,
   wrapping paragraphs, blocks), random edits through ordinary re-description (texts, specs, parts appearing or
   going, leaf and box inserts/removes). Every step checks (a) incremental rects == cold layout, bit for bit;
   (b) retained records and what they draw == a fresh mount; (c) the write-back's last written absolute rect of
   every node == the layout's; (d) the retained layout tree (shape, styles, rects in order) == a fresh mount's.
3. **Two defects it found, fixed:**
   * `describe.rs` (store branch, opus experiment A1a, `6f237e89ed`): the hot loop that steps over sealed items did
     not advance the sibling cursor `m.prev`, so an item inserted or removed after sealed items was linked or
     unlinked at the wrong layout child. This is why `tests/seals.rs` failed at `remove middle` on
     `lane/store/round6` `b68c141937` (verified: fails on the base, passes with the one-line fix).
   * `layout.rs` `attach`: a subtree added under a written parent whose first layout equals the zero layout it
     starts from (a 0x0 box at its parent's origin) was never written back (instance left at the origin).
4. **Negative controls:** cephal features `mutant-boundary` (a box whose style fixes width and height is treated
   as a boundary whatever its answers: the content minimum and baselines it reports are ignored) and
   `mutant-overflow-cut` (only the end of the extent compared).

### Which boxes are boundaries (a change below stops there), exactly

A change below box X re-runs X's parent iff one of X's answers to a question the parent actually asked differs in
a way the parent reads. For a flex parent (bough's only container algorithm today):

| what the parent reads of X | when it asks | content-dependent? |
|---|---|---|
| base size (`ComputeSize`, main axis, content sizing) | X's main size and flex-basis are auto | yes |
| min-content main size | X's min main size is auto **and** X's overflow on the main axis is `visible` (automatic minimum) | yes |
| hypothetical cross size | X's cross size is auto and X is not stretched in a single line of known cross size | yes |
| final answer: size | always | no (the parent passes both sizes as known) |
| final answer: baselines | always compared; read only for baseline alignment and the container's own first/last baseline | yes |
| final answer: overflow | always compared | only if it leaves the box the parent placed (now cut) |

So a box with a definite main size and either an explicit min main size or `clip_overflow()`, and a definite or
stretched cross size, is a **static** boundary: its parent asks it nothing that reads content, and only a baseline
change or overflow escaping the box crosses it. Every other box is a **dynamic** boundary: the scheduler recomputes
it with its last questions and stops if the answers are equal (the signal_cost row is this case: the column asks
its min-content height, which a width edit does not change). Not boundaries: auto-sized boxes whose size answer
changes, any box whose baseline changes while it is first/last in its parent or baseline-aligned, overflow that
leaves the placed box on a visible axis, out-of-flow boxes (their containing block re-places them).

## Numbers

All instruction counts on the calling thread (`bough_metal::perf::tinstr`), M3 Pro under heavy load (instruction
counts are load-independent), single runs unless stated.

| cell | before (`b68c141937` + probe) | after (`76817a1af1`) | command |
|---|---:|---:|---|
| cephal compute per width-changing label write, 16 rows | 13,152 | 10,318 | `PHASES=1 signal_cost 16 64 256` |
| same, 64 rows | 18,310 | 10,483 | same |
| same, 256 rows | 38,443 | 10,625 | same |
| per-row slope of compute | 105 instr/row | ~1.3 instr/row | same |
| whole turn of that write, 16 / 64 / 256 rows | not measured without the profiler | 13,852 / 16,030 / 24,284 | `PHASES=1` (profiler off for this column) |
| turn slope over rows | | 43.5 instr/row = the idle settle scan (lead's code) | |
| write-back per write | 542 | 543 | `PHASES=1` (2 nodes visited, 1 written) |
| signal_cost default `once` string cell, 256 rows | 25,547 | 16,182 | `signal_cost 16 64 256` (slope is noisy: most writes do not change width) |
| `once` idle / colour / number at 256 | 11,339 / 11,298 / 11,949 | 10,928 / 11,121 / 11,728 | same; equal within run noise |
| `redescribe` cells | 493,810 .. 514,162 at 256 | 494,372 .. 506,547 | same; no change expected |

Gates run after the last code edit (Mac): `cargo test --release -p bough --test seals --test layout --test
lay_diff`: all pass. `LAY_DIFF_SEEDS=1500` (37,500 edits) passed once before the final non-functional edits.
cephal (ilo-prod, rust 1.97, sources identical to `vendor/cephal`, verified by hashing): full suite passes, plus
`CEPHAL_SEEDS=3000 --test incremental`. Mutants: `mutant-boundary` fails `lay_diff` (seed 21 step 20) and cephal's
`incremental_matches_cold_layout` / `overflow_edits_match_cold_layout` (seeds 2, 4); `mutant-overflow-cut` fails
both cephal tests (seed 35 and `overflow_edits`).

Where the remaining ~10,300 instructions of a one-row relayout go (time profile, `sample`, NOPROF loop, 16 rows):
`flex::fused::compute` 666 samples, `compute_flexbox_layout` 299, `settle_cls` 267 (lead's), `set_unrounded_layout`
140, `compute_child_layout` 138, `compute_root_layout_incremental` 130, `final_layout_pass` 114, `run_queue` 102,
`lay_writeback_ready` 88, `ask_cheap_leaf` 86, `NodeCache::drain_observed` 78, `store_observed` 73, `answer_changed`
61, `hash_one::<SpecKey>` 61 (the façade's per-width style interning), `Bough::parent` 57, `Bough::record` 55, TLS
scratch pools 53 + 30, `Questions::new` 50. Two flex runs of a two-label row (its min-content question and its final
layout) cost most of it. Raw file: `runtime/lanes/layout/r6/sample-write.txt` (ignored path).

## Not verified / known broken

* Not run: `bough-suite`, `bough-vlist` pixel suites; `cargo check --workspace --all-targets`; cephal's tests on
  the Mac (only on ilo-prod, x86-64, rust 1.97 stable, not the pinned nightly).
* bough's differential test compares rects, records and instances, not cephal's overflow rectangles (bough reads
  none); the overflow cut is gated by cephal's own tests only.
* The lead's numbers ("numeric label write 2,766 at 16 rows, 8,756 at 256; string 3,525 and 20,854") were not
  reproduced on this branch: the probes here give different absolute cells (the store branch at `b68c141937`
  versus whatever the lead measured). After the cut, the per-row growth of the string write is gone in my probe.
* Items 2, 3, 4 of the brief: not built. No per-cached-child instruction count was taken (the 58 instr/child in the
  brief was, as far as I can tell, the overflow gather per sibling, which the cut removes for this case, not the
  flex re-run per cached child; that one was not isolated).
* Suspect: the zero-layout write-back hole (fixed for subtrees added under written parents) may exist for other
  paths that create layout nodes outside `attach` (templates, `ltemplate.rs`, list rows); not checked.

## Experiments

| # | mechanism | prediction | result |
|---|---|---|---|
| E1 | cut overflow changes inside the child's box at the scheduler (answer size) | per-row term of the label write -> 0 | win on the probe (105 -> ~1 instr/row); **killed** by cephal seed 1030 (a grid places an item at its style size, not its answer's: the extent must use the placed size) |
| E2 | E1 also for out-of-flow boxes | same | **killed** by reasoning + the same seed: a containing block keeps the box's placement record with its overflow; restricted to in-flow |
| E3 | E1 with the placed size from the child's stored layout, in-flow only | same, exact | **kept**: 3,000 seeds + full suite pass, both mutants caught |
| E4 | a static Spec-derived boundary bit | the brief's form | **not built, argued against**: the signal_cost row is not a static boundary (its parent asks its content minimum height), so a Spec rule either misses it or is wrong for shrinking columns; `mutant-boundary` is exactly that rule and fails the gates |
| E5 | write-back attribution via `LProf` inside the turn | | the profiler's counter reads are system calls and inflated the first turn figures (47,570 vs 13,852 at 16 rows); turn and phases are now measured in separate runs |

## Underexplored and exotic avenues

Each: mechanism; estimated gain; cost/risk; cheapest decisive experiment; dependencies.

1. **Cheaper flex run for small plain containers (item 2, and most of item 3).** Most of the ~10K for relaying one
   row is per-run overhead, not per-child work: `compute_constants` re-resolves the container's style every run,
   `generate_items` re-reads each child's `Style`, `StylePlan` and `QueryMeta` through three loads, cheap leaves are
   recomputed through `compute_empty_leaf_layout` two or three times per run, `Questions` take scratch from
   thread-local `RefCell` pools, `set_unrounded_layout` compares and writes a 100-byte `Layout` per child. Fold
   per-style constants and per-style item templates into the `StylePlan` (computed once per interned style), answer
   a cheap leaf with definite size straight from its plan, write a child's layout only where it differs (compare
   location and size first), keep scratch in the tree. Estimate: 10.3K -> 3K per one-row relayout; ~9K -> ~3K
   per row of cold mount. Risk: bit-exactness of the reordered expressions (keep the same expressions; cephal's
   differential tests decide). Experiment: count instructions per `fused::compute` on the 2-leaf row before/after
   moving `compute_constants` into the plan. Depends on nothing.
2. **Memo by seal (the lead's "trace replay", degenerate form first).** A container's answer to a question is a
   pure function of (its style id, the question key, its children's style ids and answers). Key a per-container
   memo by the subtree's seal (bough already seals rows: specs, texts, operands) plus the question: identical rows
   (zebra tables, menus, chat bubbles of equal height, tree rows) reuse the first row's child layouts; rows whose
   labels differ only in width miss. Estimate: cold mount of uniform rows ~9K -> <500 per row; resize re-lay of a
   list of uniform rows the same. Risk: keys that forget an input (the seal must carry the measured sizes, not just
   the text). Experiment: in `redescribe_cost MOUNT=1`, count rows whose seal (with label widths) repeats; if under
   50% for realistic labels, go to avenue 3.
3. **Parametric replay (F5 proper).** Record the fused flex path's float operations for one item shape with branch
   guards, replay for items whose guards hold; parametric in leaf widths, so differing labels still replay. Estimate
   (opus): ~150 instr/row. Risk: maintaining a second program that must stay bit-identical to cephal's; guard
   flips. Experiment: hand-write the row's trace and compare 10,000 rows with varied label widths bit for bit
   (opus's kill criterion: not identical or above 500 instr/row).
4. **Incremental flex without walking siblings.** When a container re-runs because one child's main size changed
   (an accordion opening, a row growing), every following child's offset changes: O(N) `set_unrounded_layout` and
   O(N) write-back. Store main-axis offsets as a blocked prefix sum (per 64 children a base and in-block deltas):
   a size change updates one block and the block bases (O(N/64 + 64)), and absolute positions are read as base +
   delta at write-back, where the moved run is one `Bough::translate_run` (O(pages)). Estimate: a row-height change
   in a 4,096-row column from ~O(4,096 x ~300) to O(130) layout work plus O(pages) record translation. Risk: cephal's
   `Layout.location` is a stored absolute-in-parent value read everywhere; needs a "location is derived" mode.
   Experiment: count instructions of a one-row height change at 4,096 rows today (not measured). Depends on the
   store lane's geometry columns for the write-back half.
5. **Moved-only rows as translations (item 4).** After remove+insert, rows below keep size and move by one row:
   the layout already knows (location changed, size not, subtree untouched); the write-back should emit
   `translate_run(start, len, 0, dy)` for the contiguous run instead of per-node placement, ink and damage. The
   stopped lane's wip (`lane/layout/round6-stopped`) is a per-node version. Estimate (from the brief's ~12K
   instr/row churn): under 500 per row, mostly the remaining flex re-run. Risk: instances hold absolute positions,
   so a translation must also shift instance positions (a vector add over the instance column) or the GPU must
   apply page offsets. Experiment: `CHURN=1 redescribe_cost 100 1000` with the write-back's moved-only branch
   counting instructions per node. Depends on store (instance columns, page offsets).
6. **Fixed-advance numeric cells.** A numeric label in a fixed-width cell (`text(num).w(64)`) never needs layout,
   but today `lay_text_changed` still builds a sized spec and hashes 84 bytes (`style_id` -> `hash_one::<SpecKey>`)
   to find the same style id; a label whose user spec fixes both axes should skip `restyle` entirely (decided at
   mount: a bit in the node), and a tabular-digit change should patch glyph ids only. Estimate: number write at 256
   rows ~800 over idle -> ~150 over idle. Experiment: the `number` cell of `signal_cost` with the bit set by hand.
7. **Sized leaves without a style per width.** Every distinct label width interns a new cephal `Style` (84-byte
   spec hash + style table entry), at mount and at every width change. Make a sized leaf's size node data (the
   plan's size read from a per-node column for leaves flagged "sized"), keep one style per user spec. Estimate: the
   `hash_one::<SpecKey>` and intern share of mount (store's profile: 5.1% + 1.7% + 3.4% memcmp) and ~100-200 instr
   per width-changing write. Risk: cephal reads `style.size` in several places, not only the plan.
8. **Parallel subtree layout.** cephal already has a `Parallel` executor that distributes `compute_child_layouts`
   batches; bough's `DView` reports `batches_help() = false`. Cold mount of large lists and resize are batches of
   independent row questions. Estimate: near-linear on cold layout beyond a grain of ~64 rows (cephal README's own
   numbers). Risk: `DView` borrows the `Bough` and the text leaf states; `Send` boundaries. Experiment: enable
   batching with a rayon-free scoped pool on `MOUNT=1 6000`.
9. **SIMD layout of uniform runs.** Children of one container with the same style id and cheap-leaf answers form
   runs; the final pass for a run is offsets = prefix sum of (size + margins + gap): a NEON prefix sum over f32
   columns, then a column write of locations. Requires item 1's column form of the item records first. Estimate:
   per cached child ~60-100 -> ~5 instructions. Risk: the IEEE order of the sequential sum must be kept (it can:
   prefix sums in order, 4 lanes as 4 independent runs are NOT the same order; a SIMD scan with the same
   association order is needed, or accept only exact integer-valued sizes where order does not matter).
10. **Constraint caching keyed by seals across turns.** The cache key today is the question bits; a row described
    exactly as before (sealed) whose parent asks the same question could skip even the cache probe: one compare of
    (seal id, question generation) per row. Gains on resize-free re-runs of a container. Depends on store's seal
    columns.
11. **Text measurement caching.** Wrapped paragraphs re-wrap per width (`Para::wrap` caches per width). A list of
    paragraphs re-laid at a new width wraps every one; keep the break positions as a function of width intervals
    (a paragraph's wrap is constant over width intervals between break thresholds, computable from the word
    table), so a resize that stays inside the interval costs one compare. Estimate: resize of a chat log of 1,000
    messages from O(words) to O(messages).
12. **Scheduler lookups through the Bough.** `DView::parent` does `pos_of` + `parent` + `record` (three lookups,
    one of them a search) and `depth` another `pos_of`; `is_out_of_flow` calls `parent` again. The layout parent is
    in `LInfo.parent` already: answer `parent`/`depth` from columns (depth stored at attach). Estimate: ~150-300
    instr per scheduled node. Trivial; not done for lack of time.
13. **Default `min-size: 0` for boxes with a fixed main size (semantics question).** CSS's automatic minimum makes
    every fixed-size flex item a content-reading item; most native toolkits have no such rule. If bough's `Spec`
    lowered `.h(32)` in a column to `min-height: 0` unless asked, the static boundary would cover the common row.
    A behaviour change for overflowing columns; the lead/user must decide.

## Where I think the lead or the plan is wrong

* "Build relayout boundaries as a first-class property derived from Spec" names the wrong cause for the measured
  case. The measured re-run was an overflow-rectangle propagation, not a size dependence, and the signal_cost row
  is not a static boundary under CSS rules (its column asks its content minimum height). The exact answer is the
  scheduler's own: compare what the parent reads; the static property is the special case where the parent asks
  nothing content-dependent. A Spec-derived bit would be wrong exactly where `mutant-boundary` is.
* "~58 instructions per cached child" is the cost of the overflow gather per sibling, not of the flex re-run's
  unchanged-child path; the cut removes it for this case. The unchanged-child path was not measured separately and
  should be before anyone optimises it.
* Experiments committed to a lane branch that other lanes base on (`6f237e89ed`) must pass the lane's gates first:
  that one broke `tests/seals.rs` and the layout chain; the layout lane spent time finding it.
* Stopping and restarting the layout lane cost a full context rebuild; the consolidation note said layout
  write-back belongs to store, and this pass shows the scheduler half (cephal) is separable from the storage half.

## Next (in order, with predictions)

1. Avenue 1 (cheaper fused flex run): one-row relayout 10.3K -> ~3K instr; cold mount cephal share per row
   ~9K -> ~3K. Gate: cephal's suite + `lay_diff`.
2. Avenue 12 + 6 (scheduler parent/depth from columns, fixed-size labels skip restyle): another ~1K off every
   relayout and ~600 off every numeric write.
3. Avenue 2 (memo by seal) measured on `MOUNT=1` with three shapes (column of rows, wrapping grid, nested
   percent stacks): uniform rows < 500 instr of layout per row; if the hit rate on realistic labels is low, avenue 3.
