# Lane `layout`, round 8: journal

Branch `r8/layout` (worktree `wt/r8-layout`), from `r8/expr` `9469bf18f1`, fast-forwarded to `0e0ec02e9e` (the
green base). Brief: `bough/runtime/lead/ROUND8.md` Amendment 1 item 1 (layout staged by type) and Amendment 2
(the author keeps the façade's builder calls). Reformulation 2 (text) moved to a `text` lane on the first day;
this lane keeps the boundary where a measured text size enters a layout. The round-7 journal of this lane is
`wt/r7-layout/docs/lanes/layout/JOURNAL.md` (instruments, the façade's layout costs). Newest entries last.

Provenance unless a line says otherwise: the shared M3 Pro (aarch64, macOS 15.6), `nice -n 15`,
`CARGO_BUILD_JOBS=2`, nightly-2026-10-17, profile `dist`. Unit: instructions retired on the calling thread
(`bough_mem::pmu`). No cache or branch-miss counters exist on the Mac; those wait for ilo-prod, which is
reserved.

## 0. Baseline (the façade, recorded by the bench lane; not re-measured here)

`wt/r7-bench/bench/results/r7/workloads/mac-6deab7e4e7/once-cpu.tsv` (`dist`, `RENDER=0`, scale 1, 1280 x 800 at
device scale 2, load 15.9):

| scene | mount | resize step p50 / p95 | records |
|---|---:|---:|---:|
| sheet (100,000 cells) | 3,606,896,865 | 119,584,391 / 2,125,434,765 | 240,055 |
| ide | 337,734,094 | 57,544,430 / 81,289,985 | 30,166 |
| chat | 887,242,656 | 181,231,462 / 207,443,705 | 23,593 |

Layout's share (round-7 journal R3): 11,300 instructions per sheet cell of about 21,000 at mount; about 4,900
per flex run, 84,060 runs for 39,029 containers; the IDE's resize step is 83 % of its cold layout. A mount-once
number write is about 6,000 and a string write 6,500 to 6,880 (`SC_STABLE=1`, ROUND7 correction).

## 1. The seam (agreed 2026-11-12)

`docs/r8/LAYOUT-SEAM.md` v0, accepted by the lead and by the expr lane in full, with these amendments:

* No derived-origin kind for spaces. `Space.scroll` stays a block word; a list's settle writes it as a
  core-owned `i32` = `extent.top(anchor_row) + offset`. For a `Uniform` extent that is the identity (scroll stays
  a Frame word the raster can latch late); for a prefix-sum extent it is a Turn-stage value in the same word.
* The author's scroll state stays the façade's `f32` pixel offset; reading gives the derived offset, writing is
  converted through the extent. A `(row, offset)` type is offered in addition.
* Order lane: `order[k]: u16` = ring slot of the k-th exposed row (expr's side).
* `each` inside a row: expr provides one child store per parent list with a (parent slot, count, first) lane;
  fixtures use counted arrays until then.
* expr's two conditions on this lane: `place` of the sheet row must fold to immediates plus the row's y
  (today: 88 stores from a const slice, scroll-a-page 30,062 for 34 rows), and the interval check stays in the
  list pass, never in a delta kernel.
* Text: `Measure` (LAYOUT-SEAM section 5) is relayed to the text lane; until it answers, fixtures use stand-in
  leaves with the same trait.

## 2. Predictions and kill criteria (written 2026-11-12 before any `bough-lay` code existed)

The lead asked for the "no shape memo" claim as the first measured result, on the sheet row and a chat row,
against the cephal oracle, with compile time and code size per distinct row type.

| # | cell | prediction | kill |
|---|---|---|---|
| L1 | `place` of the sheet row (22 rects, literal values) into rect lanes | at most 150 instructions per row (7 per record); the disassembly shows stores of immediates plus the row's y, no call | above 400, or a call to the flex slow path on the row's path: the memo is needed after all |
| L2 | `place` of a chat message row (run-time values, avatar, name, time, bubble with a wrapped paragraph, optional quote, image, up to three pills) with the paragraph's interval holding | at most 600 | above 5,000 (one façade flex run) |
| L2b | the same tree through `vendor/cephal`: cold, and warm at a new width | reported, not predicted | |
| L3 | typed `place` == cephal, rects equal as integers, on the scenes' rows and 10,000 random trees of the expressible subset; a mutant in the flex arithmetic fails it | holds | a mismatch that is not a documented tie |
| L4 | validity: every width sampled inside a returned interval gives identical rects (10,000 trees x 8 widths); a mutant that drops the shrink threshold fails | holds | any counterexample |
| L5 | compile time and `__text` per added distinct row type, N = 8, 32, 128 row types | at most 0.05 s and 3 KB on top of what expr measured (0.13 to 0.18 s, 11.9 KB per component); per-type cost at 128 within 1.3x of 32 | above 0.15 s or 10 KB per type, or more than 2x growth from 32 to 128 |
| L6 | sheet, scroll a page with `place` per entering row | within 1.1x of 30,062 | above 1.5x |
| L7 | sheet: mount within 1.2x of expr's 201,376; a resize step (window width changes, no row crosses its interval) | resize at most 2,000 | resize above 50,000, or dependent on rows |
| L8 | ide resize step; chat resize step (rows that cross their interval are re-measured, the rest range-checked by block) | ide at most 5,000; chat at most 2,000,000 at p50 | ide above 100,000; chat above 20,000,000 |
| L9 | extents: `Uniform` folds to a multiply and a divide; prefix sum `row_at` | at most 80 instructions and two chained lines; insert at most 300 | above 400, or a walk proportional to rows |
| L10 | scroll conversion: a written pixel offset to (row, offset) | `Uniform`: a divide; prefix sum: one `row_at` | |

## 3. Design notes fixed before building

* Arithmetic is `f32` device pixels in cephal's operation order; edges are rounded cumulatively
  (`round(abs edge)`, cephal's `round`), so a rect is four rounded absolute edges.
* A node answers two questions, monomorphised: `size` (border box under known dimensions) and `place` (final
  size and absolute origin in, rects out). There is no cache. The fast path of a flex line (no free space to
  distribute, or nothing that takes it) is inline; the freeze loop of CSS 9.7 is one shared, outlined function
  over a slice of items, so row types that differ only in which properties are set share it.
* Automatic minimum sizes (min-content of the content) are computed only inside the slow path: they are
  unused unless an item shrinks.
* Validity is tracked as the function runs: every definite size carries whether it is independent of the
  available size, equal to it plus a constant, or some other function of it. A decision on a size of the second
  kind narrows the interval exactly; any use of the third kind in an emitted rect makes the interval a point.
