# Expr lane, round 8: journal

Branch `r8/expr` (worktree `wt/r8-expr`), from `r7/compute` `78635ba4c4`. Brief: `bough/runtime/lead/ROUND8.md`
sections 1 to 5 and 8. Crate: `crates/bough-x` (no dependency on `crates/bough`). The seam with the raster lane is
`docs/r8/SEAM.md`. Newest entries last. Every experiment has its prediction and kill criterion written before
the run.

Provenance unless a line says otherwise: the shared M3 Pro (aarch64, macOS 15.6), `nice -n 15`,
`CARGO_BUILD_JOBS=2`, toolchain nightly-2026-10-17, profile `dist` (fat LTO, one codegen unit). Unit:
instructions retired on the calling thread (`bough_mem::pmu`, `thread_selfcounts`), median over the steps of a
phase. ilo-prod is reserved; no cache or branch-miss counters exist on the Mac, so those columns are empty until
the lead frees it.

## 0. The baseline (the façade on the same scene; not re-measured by this lane)

`wt/r7-bench/bench/results/r7/workloads/mac-13253be95f-sheet/once-cpu.tsv` (commit `13253be95f`, `dist`, Mac,
`RENDER=0`, 10,000 rows x 10 columns, window 1280 x 800 at scale 2, load 3.3). `RENDER=0` is the façade without
its raster, which is the level this lane's phase 1 works at.

| phase | instructions p50 | wall p50 |
|---|---:|---:|
| mount | 3,575,855,202 | 232.6 ms |
| steady (edit / selection move / wheel, cycled) | 1,054 (p95 75,559) | 0.6 µs |
| hover a column header (10,000 dependents) | 15,380,284 | 2.96 ms |
| theme (200,000 dependents) | 57,353,195 | 2.79 ms |
| idle turn | 241 | |
| idle turn, re-describing mode (with render) | 12,062 | |

Process: 320 MiB peak, binary 6,255,840 bytes, 240,055 records.

What the baseline includes that phase 1 of `bough-x` does not: a flex layout engine (about half of mount, round 7
core journal B3) and shaped text (an eighth). `bough-x` places the sheet's fixed-size boxes by arithmetic in the
view and draws numbers with tabular digits formatted while shading. Both are stated wherever a mount number is
compared.

## 1. The design as built, where it departs from ROUND8.md (reasons in SEAM.md)

1. An expression value is its own per-record constants; there is no `Consts` type.
2. `delta` is a finite difference over exposed pages (`eval(old) != eval(new)`, 64 records at a time), exact for
   opaque pure functions. Structural rules are page filters only.
3. State needs no write barrier: Frame-stage state is a `#[repr(C)]` block of words that handlers write as plain
   fields; a turn ends with one compare against the copy the raster last saw.
4. A store is non-generic; columns are created per view call site (position) and written through typed
   pointers. No type lists a component's archetypes.
5. Every incremental operation is the same `view` code driven by a different sink: mount, exposure, row
   re-description after a relation edit, whole re-description, hit testing targets.
6. A list materialises a contiguous window of rows in a ring addressed by `row mod cap`.

## 2. Predictions and kill criteria for phase 1 (written 2026-11-12 before any `bough-x` code ran)

Scene: the sheet of `comparisons/bough-workloads/src/sheet.rs` (r7/bench): toolbar, header of 10 hoverable
cells, 10,000 rows of (gutter fill, row number, 10 x (cell fill, cell number)), footer of 10 totals, a selection
outline, a scrolling body. Window 1280 x 800 points at scale 2.

| # | cell | prediction (instructions) | kill |
|---|---|---|---|
| P1 | mount at 10,000 rows (exposed rows only, about 34) | at most 60,000 | more than 1.10x the mount at 100 rows: exposure is not the only cost |
| P2 | hover a header (pointer move, hit, handler, block compare, delta, tile marks) | at most 4,000 | above 20,000, or differing by more than 5 % between 100 and 10,000 rows |
| P3 | theme | at most 12,000 | above 40,000, or dependent on rows |
| P4 | edit one exposed cell (value, its column total) | at most 800 | above 3,000 |
| P5 | edit one unexposed cell | at most 150 | above 500 |
| P6 | selection move | at most 500 | above 2,000 |
| P7 | scroll by three rows (exposure: three rows leave, three mount) | at most 8,000 | above 30,000 |
| P8 | insert one row in the exposed window; remove one | at most 20,000 each | above 60,000 |
| P9 | idle turn | at most 60 | above 150 |
| P10 | idle turn in re-describing mode (the same view code compared against the columns) | at most 12 per exposed record, about 9,000 | above 30 per record |
| P11 | finite-difference delta of the cell fill, per page of 64 | at most 200 (vectorised) | above 600: the compiler did not vectorise it and a structural rule is needed after all |
| P12 | indirect calls or `TypeId` compares inside any per-record loop of the delta, hit and re-description paths (read in the disassembly) | none in delta and hit; one compare per emit in mount and re-description | any indirect call per record |
| P13 | chained (dependent) cache lines from a block write to the tile bitmap | 3: block, column header, page | more than 4 |
| P14 | compile time and size, N distinct row components each with its own expressions and functions, N = 8, 32, 128 | time and `.text` linear in N: per-component cost at 128 within 1.3x of the cost at 32 | per-component cost growing more than 2x from 32 to 128 |
| P15 | retained raster == full redraw == fresh mount on every step of a random script (oracle), and a mutant that drops the old-value half of the derivative fails it | holds | any pixel differs |

P13's budget in ROUND8 is two chained lines. The design has three by construction (the store's column table
sits between the block and the page); it is predicted to miss that budget and the journal will say so.
