# Hand-off: dormant runs (mounting driven by exposure), lane `memory`, 2026-11-12

Design as built: `../DORMANT.md`. Card: `../cards/2026-11-11-dormant-runs.md`. Code: `crates/bough/src/dormant.rs`.

## State

* Branch `lane/memory/dormant`, head = the commit that adds this file (parent `ab0333c6c2`). Based on
  `lead/core-r6` (merged twice, last at `d1ac1e753e`, which includes 4f4c469ef5) plus `lane/surface-api/round6`
  cherry-picked (5 commits: scroll containers must re-describe their content for any of this to work).
* Nothing of this branch is merged into `round6`.
* Also on this branch, outside the card: `comparisons/bough-guibench` copied from lane `generalization`;
  `bough-zgui/src/shell.rs` fixed to build against surface-api's route API (`Press`/`Drag` take a `PhantomData`);
  hover is re-evaluated after a wheel scroll and a glide tick (OVERFIT finding 8); `settle_input` reclaims
  released scopes (finding 4b); `On<E, HoverBg>` and `On<E, Focusable>` seal; list edits of unmounted rows no
  longer queue a re-description (see Numbers).

## Numbers

Instructions are calling-thread counts (`tinstr`), headless, M3 Pro under heavy load from other lanes (load does
not move instruction counts; the spread shown is run to run). "base" = `lead/core-r6` + surface-api without this
work (binaries built from a temporary worktree at 108f71c517 merged with core-r6; removed since).

**Plain `each` of N rows of 32 pt in a 700 pt scroll box** (`cargo run --release -p bough --example
dormant_scale -- 100 10000 1000000`, default `Caps`, 3 runs each, all shown runs agree within 2.3 %):

| rows | records | mount instr | scroll frame (2 pt) | jump to middle | plain-field turn | same, aimed at a dormant row | wake / item | footprint of mount |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 100 | 77 | 7.95-8.23 M | 27.1-27.8 K | 0.48-0.65 M | 15.3-15.6 K | 14.3 K | 26.1-26.8 K | 4.8-5.1 MiB |
| 10,000 | 77 | 7.83-7.97 M | 27.2 K | 0.71-0.73 M | 12.3 K | 12.3 K | 23.5-24.9 K | 4.8-5.1 MiB |
| 1,000,000 | 77 | 7.80-7.97 M | 27.2 K | 0.73 M | 13.3 K | 13.3 K | 23.7 K | 4.8 MiB |
| 16 M, 30 M | 77 | 7.8-8.1 M | 27.2-27.4 K | 0.74-0.77 M | 14.1-14.2 K | 14.1-14.2 K | 23.6-23.9 K | 4.8-5.3 MiB |
| 40 M, 60 M, 100 M | **fails** | | | | | | | |

The mount cost is the application's fixed cost (the same 7.8 M with 5 rows). Eagerly (`EAGER=1 RAISED=1`):
100 rows 304 records / 20.7 M mount / 41.8 K per scroll frame; 1,000 rows 3,004 records / 40.2 M / 385 K; 10,000 rows
do not mount at default caps. Verified: 3 runs per size for 100 / 10 K / 1 M, 1 run for the others.

**The failure above 2^29 px**: layout coordinates are cephal's f32. Positions are exact below 2^24 px
(524 K rows of 32 px) and beyond only while the pitch is a multiple of the float spacing; at 40 M rows the jump
lands where the spacing is 64 px, items collapse onto each other and realisation runs away until a column fills.
`DormStats::inexact` counts run extents set in the inexact range. Not fixed (see avenues: virtual coordinates).

**gui-bench, R0, median of 3 blocks of 8** (`headless --scenario S --rows N [--variant fine] --op OP --repeats 8
--blocks 3`, `GUIBENCH_CAPS=default`; one run per cell; identical at 1,000 / 10,000 / 1,000,000 unless shown):

| cell | base | this branch |
|---|---:|---:|
| table fine select | 14,648 / 14,678 / 15,338 | 14,656 / 14,686 / 14,706 |
| table fine hover | 2,224 | 2,220 |
| table fine scroll (16 pt wheel) | 35,843 / 35,911 / 36,267 | 38,683 / 38,752 / 38,912 (**+8 %**: the hover re-test after a scroll) |
| ticker fine K=1 | 17,287 / 15,618 / 15,712 | 15,595 / 15,635 / 15,717 |
| ticker fine K=100 | 113,054 / 98,382 / 99,669 | 117,252 / 98,387 / 99,769 |
| table fine update10 (every 10th row's label) | 236,944 / 4,857,065 / **35,212,119,891** | 196,876 / 1,298,402 / **126,802,875** |
| virtual hover | 4,527 | 4,541 |
| virtual scroll | 32,859 / 32,902 / 33,794 | 35,551 / 35,824 / 35,884 (+8 %, same cause) |
| virtual jump | 917 K / 908 K / 908 K | 914 K / 914 K / 914 K |
| rows (plain `each`) hover | 2,943,766 / does not mount / does not mount | 62,655 at every N |
| rows scroll | 2,954,836 / - / - | 134,788 at every N |
| rows jump | 2,958,918 / - / - | 259,067 at every N |

* update10 was quadratic in the number of edits (`lists.redesc.contains` per edit, added by surface-api's
  commit): 352 K instructions per write at 1 M rows; now ~1,270 per write to an unmounted row at 10 K and 1 M
  (most of it the model's own string work and the list's edit log; not broken down).
* The `fine` cells go through `list()` (listview's window), which this work does not replace: they are
  unchanged by construction, and shown to be unchanged.
* Pixel gates (`--gate --ring 4|1 --repeats 4`) passed for rows and virtual at 10,000 (hover, scroll, jump) and
  table fine at 10,000 (select, update10). rows resize passed at an earlier commit. Not run: the other cells.

**No tax when unused** (`redescribe_cost 10 100`, instructions per turn; base / new; one run each, repeatable to
the instruction):

| rows | owned | static | bare |
|---:|---:|---:|---:|
| 10 (all fit) | 12,740 / 11,143 (0.87x) | 12,763 / 10,453 (0.82x) | 4,956 / 5,323 (**1.07x: fails the 1.02 gate**) |
| 100 | 68,163 / 14,667 | 67,321 / 12,611 | 20,004 / 6,806 |
| churn pair at 1,000 (`CHURN=1`) | 15,988,008 / 479,548 | | |

The bare-box loss is ~200 instructions per walk per dormant-capable part plus 12 per realised row; the inlined
per-turn exposure test is about 40 of them. Why text rows got cheaper was not isolated (code generation of the
indexed walk versus the iterator walk). The `once` column moved 155 -> 247 with the merge of core-r6, not with
this work.

**Memory**: a dormant item costs 0 bits (nothing is kept per item: footprint at 10 K, 1 M and 30 M rows is the
same 4.8 MiB within the 0.5 MiB run-to-run spread). A run costs one 256-byte entry, one record, one layout node;
a part 296 bytes plus 8 bytes per entry in the scan columns; a remembered size costs 12 bytes per run-length
group of equal sizes (0 for sizes equal to the estimate).

**Cache lines of a scroll step** (counted from the layout, not measured): when the exposure key holds, 8
(four scan columns, the clip rect, the space origin, the layout counter, the window size). When the offset
moved and nothing enters: about 26 (the part's state 5, its entry columns 4 B per entry per column: 9 for 64
entries, the container's `LInfo` and style 4). A row entering adds the re-description walk.

## Gates

| gate | result | command |
|---|---|---|
| presented == retained == fresh == fresh with every item mounted, rings 1..4, scripted + random sequences | pass | `cargo test --release -p bough --test dormant` (12 tests) |
| variable sizes at the same anchor; a resize that changes every height | pass | same |
| convergence (`unconverged == 0`), record bound after a 3,000-step scroll | pass | same |
| mutants: one item too few / never release / no anchoring | each fails its own gate | `BOUGH_DORM_MUTANT=short|norelease|noanchor cargo test --release -p bough --test dormant` |
| focused item held 720,000 px away, exact on return; Tab walks into dormant rows | pass | same file |
| `cargo test --release -p bough -p bough-suite -p bough-awagy -p bough-zgui` | pass at ed991fda82's parent state (before the last merge and the seal/list edits) | |
| gui-bench rows at 10,000 at default caps | mounts (138 records); both pixel gates pass for hover, scroll, jump, resize | |

## Not verified / known broken

* **Not run on the head**: the full suites, `cargo check --workspace --all-targets`, rustfmt on changed files,
  the seals mutant, gui-bench gates.
* Above 2^24 px of content, exactness depends on the pitch; above 2^29 px (32 px rows) it breaks outright.
* Fractional scale factors (item sizes that are not whole px): release may move content by a sub-pixel; untested.
* A part under a transformed ancestor: only the part's own space affine is inverted; nested transforms untested;
  no test at all exercises the transformed path.
* Parts in list rows, entity views, `Config::once()`, row containers, definite-height columns, non-start
  `justify`, growing or absolute items, and non-indexable sources mount eagerly (by design; `App::dynamic_parts`
  shows which).
* The focus ring is only moved at the end of input turns, not of `App::turn` (existing behaviour, met in a test).
* gui-bench `rows` rows do not seal (135 K per scroll step is mostly an unsealed walk); which element refuses was
  not traced.
* `DormStats::inexact` is counted but nothing acts on it.
* Releases happen only on turns: an idle window keeps what the last turns left unexposed for up to 3 more turns.

## Experiments

| # | mechanism | predicted | measured | verdict |
|---|---|---|---|---|
| 1 | seal an item when it is realised, so its first walk steps over it | scroll frame -10 % | scroll frame 27,429 -> 27,177 (-0.9 %), wake +4 %, jump +2.6 % | killed (kept behind `BOUGH_DORM_SEAL=1`) |
| 2 | scan keys and entry boxes as columns, bit-mask test | fewer chained loads | scroll frame 33.7 K -> 27.1 K (together with releasing on the next pass) | kept |
| 3 | release rides only on walks that realise (no walk of its own) | fewer walks | stale rows after a jump were never released (24 K vs 12 K per later turn) | killed: release after `HOLD` passes |
| 4 | inline the stale test into the turn | -100 instr/turn | -45 instr/turn | kept |
| 5 | `iter.clone().nth(i)` as the index | O(1) | `Map::nth` is O(i) (std source) | killed: `TrustedRandomAccessNoCoerce` by specialisation |
| 6 | forced release beyond 12,000 px (i16 inks) | needed | not needed after core's exact inks: held at 720,000 px exact | removed |

## Underexplored and exotic avenues

1. **Virtual coordinates (removes the 2^24 px ceiling).** A part keeps an i64 base outside the layout: the leading
   run's leaf is sized `extent - base` and the view's content origin adds the base back; the scroll offset becomes
   i64 device px. Buys 100 M+ rows exact and removes OVERFIT finding 15. Cost: the offset signal's type (API), the
   view code. Experiment: 40 M rows of 31 px, jump to the middle, pixel gate against a 300-row eager list at
   the same anchor. Depends on surface-api (offset type).
2. **`List<T>` on dormant runs (delete listview's window, index and per-row spaces).** A keyed source knows its
   deltas, so entries are spliced by the log instead of matched by seals; rows stay owners (their class, their
   wires). What must happen: `update_src` over a keyed source, the row-owner mount in `realise`, anchoring from
   here, move animation as a paint offset. Buys one mechanism, no space per row (finding 4a), 12 B per
   size group instead of 4 B per row. Risk: listview's animation and nested-region code. Experiment: gui-bench
   `fine` and `virtual` cells and `tests/list_window.rs` on the new path.
3. **Walk-free realisation.** Today a row entering re-runs the root's `view()` (13 K to 62 K per walk). With
   owners as blocks, or a part that retains its source (a `'static` description closure per part), realising
   is `source.item(i)` and a mount: predicted scroll step with a row entering ~25 K (the wake alone) instead of
   27 K average / 135 K on gui-bench rows. Depends on surface-api's owner blocks.
4. **Damage as virtualisation beyond lists.** The same run idea per axis: a grid is runs of rows whose realised
   rows hold runs of cells (nested parts already nest: tested with sections of rows); a tree is nested parts whose
   collapsed subtrees are one run; a text document is runs of paragraphs with the shaper as the source; a canvas
   needs runs keyed by a spatial index (a source that answers "items meeting this rect") rather than an interval.
   Cheapest experiment: `row(each(..))` (the horizontal axis; needs min-content handled) and a 2-D grid of
   1 M x 1 K cells.
5. **Computed geometry for runs.** A source that can state its item extent (`fixed(32.0)`, or a prefix-sum
   closure) makes the estimate exact: no anchoring, no remembered sizes, exact scrollbar. Zero cost when absent.
6. **Compressed remembered sizes.** Run-length groups are already it; for text documents a two-level
   (page sum, per-item u16 delta) table would bound a fully visited 1 M-paragraph document at ~2 MiB.
7. **Paging dormant state out entirely.** A part with no realised item and no demand keeps a 296-byte `Dorm`
   and one run; a closed panel's parts could be dropped to "count + estimate" (16 bytes) in the parent's entry.
   For realised-but-hidden subtrees (tabs), release the whole subtree to one run of the parent ("a run of one
   item of remembered size") and realise on exposure: the same mechanism, applied to `Option<E>`.
8. **Signal writes to dormant items.** In `each`, a dormant row has no wires, so a wired write costs the store and
   a plain-field write costs the walk of the realised rows (13 K, measured equal for a dormant and a realised
   target). With lead's write log the first is already write-proportional; the second needs seals as columns
   (lead's experiment) to fall under 30 per realised row.
9. **Accessibility, find, focus.** Tab enters runs through `dorm_run_toward` + `dorm_pin` (tested). No
   accessibility tree or find exists in bough yet; both should query the *source* (count, item text by index)
   and call `App::scroll_to_item`, never walk records. An a11y provider needs "children of a part" = count plus
   lazily realised items: the `Source` trait is the right seam.
10. **Parallel realisation.** A demanded range mounts through one `insert_run`; describing and shaping the items
    is independent per item: pool it with a grain (first frame of a jump is 26 items x 23.7 K).
11. **Composition with core's directory and store's columns.** A dedicated `REC_DORMANT` kind lets the cull
    return runs meeting damage, so the scan disappears into the cull; `Dorm`'s `tops/bots/first/isrun` columns
    are exactly a side-page column group and should move there, with `DynItem` shrinking from 256 B to a slot.
12. **Hysteresis by velocity.** Overscan proportional to scroll speed instead of a constant 64 pt would cut the
    frames that both realise and release during a fling. Unmeasured.

## Where I think the lead or the plan is wrong

* "Exposure-driven mounting must be shown first on `List<T>`": `list()` is already exposure-windowed; what is
  missing there is unification, not the mechanism. The measurable win was in plain `each` (50x to "does not
  mount -> flat") and in the quadratic list edit found on the way.
* The 1.02x no-tax threshold on a 5 K-instruction turn is 100 instructions; a walk prologue is that big. The
  threshold should be absolute (instructions per part per turn) rather than relative.
* `HOLD` in "frames" is the wrong unit while releases only run on turns; it should be time or scroll distance.

## Next

1. Virtual coordinates (avenue 1): 100 M rows exact; predicted scroll frame unchanged (27 K).
2. `List<T>` on runs (avenue 2): gui-bench `fine` cells unchanged or better, `Caps::spaces` no longer a row limit.
3. Find and fix what stops gui-bench `rows` from sealing: predicted scroll step 135 K -> ~45 K.
