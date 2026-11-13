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

## 4. What was built, in order, and what each form measured

Crate `crates/bough-lay` (depends on `bough-expr`; `vendor/cephal` and `bough-mem` as dev-dependencies).
All numbers: M3 Pro, `dist`, `crates/bough-lay/examples/rows.rs`, instructions and cycles per row, median of 15
batches, load average 17 to 35 (instructions do not depend on it). No cache or branch-miss counters: nothing of
this lane ran on ilo-prod or mothership (section 8).

### 4.1 The flex arithmetic, against the solver (kept)

`src/flex.rs`: single-line flexbox written generically over a container's children (`Kids`, `Pass`), in
cephal's operation order. `src/val.rs`: every size carries how it depends on the available size (independent,
equal to it plus a constant, some other function), a decision on a dependent size narrows the answer's
validity interval, an emitted rect that depends on it makes the interval a point. `tests/oracle.rs` gives the
same style tree to `vendor/cephal` and demands equal integer rects.

**L3 holds, L4 holds.** 4,000 random trees x 8 widths, with and without wrapping leaves: equal rects and
heights; 384,000 widths sampled inside returned intervals lay out identically; 41 % of answers hold over more
than one pixel. The scenes' rows: the sheet row at 6 widths, 400 IDE lines x 3 widths, 400 chat rows x 4
widths. Mutants `mutant_gap` (the flex line forgets a gap) and `mutant_valid` (decisions do not narrow the
interval): see section 7.

What matching the solver took, each found as a failing seed: distributed justification falls back to the start
when there is no free space; a minimum applies to a container's content size but not to a childless box's;
content minimums apply when grow factors sum below one (a target can come out under its basis); a row of
unknown width in definite space shrinks to fit, with its min-content width as the floor; a column of unknown
width gives its stretched items its fit-content width; an item reports at least its padding across its own
cross axis and the solver advances by that; the space handed to an item is for its border box (margins come
off once, in the parent); float association in offsets.

Excluded from the generator, by design and stated (review L04): a percentage of an indefinite size. The case
that showed why: a percentage child of a content-sized row that shrinks to fit, measured for height by a
column parent. The solver lays the row out at its fit-content width first; a single measure cannot. Also not
generated: baseline alignment, wrap, aspect ratios, percentage padding, a real text measure (the leaves are
stand-ins with the agreed `Measure` contract).

### 4.2 Monomorphised `place` per row type (refuted)

The form the brief asked for: `Lay` implemented for the builder's types, `place` inlined down the tree.

| cell | instructions | cycles | prediction / kill | verdict |
|---|---:|---:|---|---|
| sheet row, 22 rects | 25,486 | 5,771 | at most 150 / above 400 | **killed** |
| ide line, 10 rects | 4,801 | 974 | | |
| chat row | 56,086 | 19,495 | at most 600 / above 5,000 | **killed** |
| chat row, size only | 23,610 | 7,664 | | |
| copy of a kept slice of 22 rects (what the expr lane does) | 203 | 50 | | |

The compiler did not fold a general algorithm into constants: items of a line were a stack array with a
run-time count walked by slice loops, so each of the row's 13 containers cost about 2,000 instructions,
cephal's own figure per flex run. Typing removed the tree and the caches, not the work.

Compile time: with `size` inlined the chat row type did not finish compiling in 12 minutes (a size question
nests four ways per level: basis, content minimum, cross size, fit-content width). With a container's `size`
as a never-inlined function it built, at this cost per row type (`bench/r8/lay_compile.sh`, user seconds and
`__text` over an example with no row):

| row type | user s | `__text` |
|---|---:|---:|
| sheet | +13.4 | +498 KB |
| ide | +4.1 | +199 KB |
| chat | +203.8 | +2,748 KB |

**L5 killed** (kill: above 0.15 s or 10 KB per type).

### 4.3 The same with the items as a tuple in the type (refuted)

One attempt, boxed by the lead at 1,000 instructions for the sheet row: `Kids::St` is a tuple shaped like the
children, every pass a tuple recursion, the freeze loop copies to a slice on its cold path only. Sheet row
28,611 instructions (7,959 cycles), ide line 6,021, chat row 58,794. Worse than 4.2. **Killed.** Not pursued:
the disassembly was not read, so *why* the tuple form does not fold is not established; the suspicion is the
arrays of children built by `from_fn` and the dependence bookkeeping in every size.

### 4.4 Layout as data, one layout function, a memo (kept; what is on the branch)

`src/shape.rs`. The type does only what it measured as good at: the builder calls become a flat array of
24-word elements (`Nd`) in depth-first order, by code that is a few stores per element (`Shape::emit`). The
flex arithmetic of 4.1 is instantiated **once**, for a cursor into that array (`At`), so it is one body for
every row type of a program. A row's shape is the hash of its words; `Memo` keeps the rects of the last shape
a list laid out with their validity interval; a row of the same shape at a width inside it is a copy.

| cell | instructions | cycles |
|---|---:|---:|
| sheet row: shape, memo hit, copy | 2,850 | 2,839 |
| of which the shape (hash of 23 elements) | 2,677 | 2,806 |
| sheet row: copy of a kept slice | 200 | 45 |
| sheet row: memo miss (words + layout) | 52,730 | 12,393 |
| ide line: shape + layout, every line its own | 12,657 | 4,419 |
| ide line: the shape alone | 1,100 | 1,277 |
| chat row: words + layout | 91,359 | 28,719 |
| chat row: size only | 38,554 | 11,740 |
| sheet row through cephal, tree built each time | 346,420 | 94,749 |
| sheet row through cephal, kept tree, a new width | 21,600 | 4,090 |
| chat row through cephal, tree built each time | 298,739 | 81,709 |
| chat row through cephal, kept tree, a new width | 87,079 | 29,287 |

Compile cost per row type: sheet +0.21 s and +4.7 KB, ide +0.54 s and +3.5 KB, chat +0.44 s and +17.1 KB
(against 13 to 204 s and 0.2 to 2.7 MB in 4.2). L5 is still missed by its own line (0.15 s, 10 KB) for the
chat row; it is three orders better than the form it replaces.

What these numbers say, plainly:

* **The hit path is not what was hoped.** The prediction was that the hash is a compile-time constant where
  the author's values are literals, so a hit costs a compare. It is not folded: 2,677 instructions per row to
  hash 23 elements. A memo hit is therefore 14x the copy it guards. For a list whose row type is `Hk::Fixed`
  the usable form today is the expr lane's: lay the first row out, keep the slice, copy it for the others,
  with the shape checked once per list pass and not per row. That check is not sound if the author varies a
  value per row; making it sound without the per-row hash is open (section 9).
* **The miss path is the general solver's speed, not better.** One body over a flat array costs 52,730 for
  the sheet row, 2.4x cephal with a kept tree (21,600), and 91,359 for a chat row, the same as cephal with a
  kept tree (87,079). It is 3.3 to 6.6x cheaper than cephal building its tree each time, and it keeps nothing
  per row (no tree, no node caches), which is what "nothing unexposed exists" needs. It is not a win over a
  retained solver in instructions.
* **The earlier chat figure for cephal with a kept tree (1,645) was a measuring error** (the widths repeated
  across batches, so the tree was unchanged after the first); 87,079 is at a width no row had seen.
* **For chat rows the validity interval is a point at every width**: 0.0 % of 102,400 (row, one-pixel step)
  pairs were a range check, because the bubble is 74 % of a column that follows the list's width, so a rect
  changes at every pixel. The interval still tells the text leaf its line structure holds. The review's L05 is
  right that an equality interval is the wrong derivative here: the arrangement (which items froze, where
  lines break) holds over a range while the rects move affinely inside it. Not built.

### 4.5 Extents (kept, with a standing finding)

`src/extent.rs`. `Uniform` (rows of one height; `new` refuses a height that is not positive) and `Heights`
(a blocked prefix sum, 64 rows per block: exact for measured rows, the rounded mean for the others, per-row and
per-block validity intervals, insert and remove). After the review's **L03**: logical coordinates are `i64`
(`top`, `row_at`, `total`), `window(y, h)` hands the list a viewport-local 32-bit frame, heights are
validated. `tests/extent.rs`: a hundred million rows of 44 pixels; 40 cases x 300 random sets, inserts and
removes against a plain model; the interval scan reports exactly the rows outside their interval. Writing that
model test found two defects in the first `Heights` (insert and remove left stale summaries; the estimate did
not return to its initial value when the last measured row was removed).

**L02 stands.** `Heights` stores a height and two interval bounds for every logical row, measured or not (12
bytes per row), an insert or remove at the front moves all three lanes, and a change of the mean recomputes
every block top. It is proportional to rows, not to exposure. What would close it is the review's proposal,
which I agree with: uniform spans as count x height, measured rows as sparse exceptions in a Fenwick or
blocked summary, so storage follows what was measured. Not built. L9 and L10 (instruction counts of `row_at`,
insert, and the scroll conversion) were not measured.

## 5. Predictions against results

| # | prediction | result |
|---|---|---|
| L1 | sheet row `place` at most 150, kill above 400 | **killed**: 25,486 monomorphised, 28,611 with tuple state; 2,850 for a memo hit in the kept form |
| L2 | chat row `place` at most 600, kill above 5,000 | **killed**: 56,086 monomorphised; 91,359 in the kept form |
| L2b | the same trees through cephal | reported in 4.4 |
| L3 | typed == cephal on scene rows and random trees; a mutant fails | holds (section 7 for the mutants) |
| L4 | every width inside an interval lays out the same; a mutant fails | holds |
| L5 | at most 0.05 s and 3 KB per row type, kill above 0.15 s or 10 KB | **killed** for the monomorphised form (4 to 204 s); missed for the kept form (0.21 to 0.54 s, 3.5 to 17 KB) |
| L6 | sheet scroll-a-page within 1.1x of 30,062 | **not measured**: nothing of this lane is wired into `bough-x` |
| L7 | sheet mount and resize step | **not measured**, same reason |
| L8 | ide and chat resize steps | **not measured** end to end; per row, a chat row costs 91,359 per width, so 15 exposed rows are about 1.4 M per step against the façade's 181 M for 2,000 |
| L9, L10 | extents' instruction counts | **not measured** |

## 6. What is not done

* **Nothing is integrated.** `bough-x`'s lists still take a slice and a row height; the e2e binary still places
  boxes by arithmetic. The deliverable "the sheet, ide and chat scenes' layout on the new core" is not met:
  what exists is the three scenes' *row shapes*, laid out and checked against the solver, in a crate beside
  the core.
* No measurement on ilo-prod or mothership: no L1/L2 or branch-miss numbers exist for anything here.
* The holds / moved / changed classification of a resize step is a design in `docs/r8/LAYOUT-SEAM.md`, not
  code.
* `guess(w)` for unmeasured rows, pending measurements, the (row, offset) scroll anchor: agreed with the text
  lane and the lead, not built. `Extent::anchor` and `window` are the only parts that exist.
* Fixed (window-level) rows, scroll ports and nested lists are not expressed in `bough-lay` at all.

## 7. Gates and how to run them

    cargo test --release -p bough-lay                          # extent model tests; oracle, 10,000 trees by default
    LAY_TREES=2000 cargo test --release -p bough-lay --test oracle
    cargo test --release -p bough-lay --test oracle --features mutant_gap     # must fail
    cargo test --release -p bough-lay --test oracle --features mutant_valid   # must fail
    cargo run --profile dist -p bough-lay --example rows       # section 4.4's table
    bench/r8/lay_compile.sh dist                               # compile cost per row type

Both mutants fail, each in both tests (run at `LAY_TREES=1000` on the kept form): `mutant_gap` at seed 1 of
the random trees and at chat row 0; `mutant_valid` at seed 17 (a width inside an unbounded interval lays out
differently) and in the sheet row's memo (a kept answer reused at a width it does not hold for).

## 8. Hosts

Everything ran on the Mac. ilo-prod was probed once (to find a checkout to clone from) and nothing was built
or left there; no directory under `/root/work/r8/layout` exists; the rex blocks the helper opened were closed
by it. mothership was not used.

## 9. Next, and what not to do again

Would do next, in order:
1. Wire `Memo` and `Extent` into `bough-x`'s list (`place` into the lanes on a miss, the kept slice on a hit,
   the interval as the resize check) and measure L6 to L8 there and in the e2e binary. Expected: the sheet
   unchanged within noise for a `Hk::Fixed` row type checked once per pass; a chat resize step about 1.4 M.
2. Make the shape check free where it can be: hash only the words that vary (a `const` hash of the literal
   part, when the row is built in a `const` context or by a build step), or have the element type carry a
   `const` shape for parts built from `const` recipes. Measure before believing it folds.
3. The review's L05: keep the arrangement (frozen set, line breaks) with its breakpoints and re-evaluate rects
   by affine terms, so a chat row inside one arrangement costs arithmetic and not 91,359.
4. The review's L02: sparse exceptions over uniform spans.
5. Read the disassembly of the tuple form before concluding anything about typed layout beyond "these two
   forms did not fold".

Would not do again: predict that a compiler will constant-fold an algorithm with loops, optional values and
float decisions because its inputs are literals, and build a crate on that before reading one function's
assembly. The 203-instruction copy was measurable on the first afternoon.
