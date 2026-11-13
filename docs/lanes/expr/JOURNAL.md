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

## 3. What was built (commits `c65b30261b`, `066a59aee1`, `4f54042d54` and the one that adds this section)

Two crates. `crates/bough-expr` (no dependencies, `no_std`): the uniform block, lenses, `Words`, `Code`, stages as
types and as a constant, the `Expr` trait, the leaves, and `Map` for a Rust function. `crates/bough-x` (depends
on `bough-expr` only): pages and stores (`store.rs`), sinks, rows and relations (`sink.rs`), the driver
(`world.rs`), oracle paint kinds (`kinds.rs`) and an oracle raster with an immediate-mode sink (`soft.rs`). The
scene is `crates/bough-x/scenes/sheet.rs`; the gates are `crates/bough-x/tests/sheet.rs`; the measurements are
`crates/bough-x/examples/sheet_x.rs` and `bench/r8/compile_scaling.sh`.

The mechanisms, all of them:

1. **A turn is a compare.** `World::settle` compares the application's block with the copy the pixels were
   shaded with (the words in use; 14 for the sheet) and loads one epoch word that every relation edit bumps. If
   both are unchanged it returns. Hover, theme, selection, scroll and the totals are plain field writes.
2. **Damage is a finite difference over exposed pages.** Each column whose constant `READS` mask meets the
   changed words runs one monomorphised kernel: per page, `changed(old, new)` for 64 records into a mask, then
   the rects of the set bits into the tile bitmap (one bounding box when the changed records fill most of it).
   A page with at most 12 live records evaluates only those.
3. **A row is a value.** The fixed part of a view is a row; a list's row is a pure function from an index to a
   row. A row is a tuple or array of records, its type is the shape, and it carries operands only. Where its
   records are is a slice of rects per shape (the layout's answer), compared by identity.
4. **A list is a window on a relation.** Exposed rows live in a ring (`row mod cap`); a `Rel` logs its last
   eight edits (`Update(i)`, `Shift(i)`) and a list replays what it missed: an update compares one row, a shift
   rewrites the rows from there on, a scroll writes the rows that entered. Nothing else exists.
5. **One view, four sinks.** `Write` fresh (mount), `Write` comparing (re-describing mode), `Patch` (after a
   turn: fixed rows are not even built, lists are brought up to date), `Immediate` (the oracle: paints each
   record as it is described, with no store).

What the type system would not take directly, and the form it did take:

* `type Scope<'r>: Sink<V> where Self: 'r` cannot be named under `for<'r>` (the bound ranges over `'static`,
  where `Self: 'r` fails for a sink that borrows). Spelled as a supertrait with a defaulted
  `Implied = &'r Self` parameter (`sink::ScopeOf`), the implied bound restricts the quantifier and
  `F: for<'r> FnOnce(&mut <Self as ScopeOf<'r, V>>::Scope)` compiles, with closures that borrow `&self`.
* A view cannot name the tuple of its archetypes when one of them contains a closure type. Stores are therefore
  not generic: a column is created from the row value the first time it is described (`Store::discover`, cold,
  taking a `&'static Desc`) and written afterwards through a pointer kept per site.
* A blanket `impl<A: Arch> Row for A` beside the tuple and array impls is accepted by coherence because `Arch`
  is local; a record is a row of one.
* Branded lifetimes: not used. There is no handle to brand: a record's identity is (site, slot), reached by
  arithmetic, and the one unchecked access (a row written through its sites) is licensed by one `TypeId` compare
  per row value or per list batch.
* A handler and a mapped function are zero-sized (asserted in a `const` block) and are conjured where they are
  called, so neither is stored anywhere.

## 4. The gates

`cargo test --release -p bough-x --test sheet`: after every step of a random script (pointer onto and off
headers, theme, cell edits exposed and not, selection, scrolls of three rows, of a few pixels and of pages, row
inserts and removes), in mount-once mode (400 steps at 10,000 rows, 200 at 20 rows) and in re-describing mode
(300 steps, edits made with no journal entry):

    retained (damaged tiles repaired) == full redraw from the stores == immediate-mode paint by the view
    == (every 16th step) a fresh mount

and `nothing_unexposed_exists`: 764 records, under 256 KiB of columns, at 10,000 rows.
Mutants, each failing the gates: `mutant_delta` (the derivative forgets every other record), `mutant_journal`
(a list ignores the relation's journal), `mutant_moves` (a moved record damages only where it is now).

The third oracle is the one that checks paint order: the stores paint column by column, the immediate sink
paints in the view's own order, and they agree because the columns of a store are formed so that they do
(`Store::discover`: a site joins an existing column of its archetype only if no record described since, in a
later column, overlaps it; the test is made on the first row's layout, which is sound only while rows of a list
share one layout, as they do here).

## 5. Results against the predictions

`bench/results/r8/expr/sheet_x-rows{100,10000,1000000}.tsv`; M3 Pro, `dist`, load average 11 to 16;
instructions and cycles of the calling thread, bracket cost subtracted, median of 32 steps. A step is the
handler and the settle, up to the damage bitmap. The façade column is the recorded round 7 run (section 0), not
re-run. **No cache or branch-miss counters exist on the Mac; those columns wait for ilo-prod.**

| cell | façade (recorded) | bough-x, 100 rows | 10,000 rows | 1,000,000 rows | cycles | prediction | verdict |
|---|---:|---:|---:|---:|---:|---|---|
| mount | 3,575,855,202 | 187,114 | 201,376 | 228,566 | 93,691 | at most 60,000; kill above 1.10x of the 100-row mount | **missed, and by the letter killed** (1.08x at 10,000, 1.22x at 1,000,000). The work is identical; the growth is the instruction counter including kernel work on this thread (page faults in a larger heap). See below |
| idle turn | 241 | 73 | 73 | 73 | 21 | at most 60; kill above 150 | missed by 13 |
| hover a header (hit, handler, settle) | 15,380,284 | 7,173 | 7,173 | 7,173 | 1,303 | at most 4,000; kill above 20,000 or size-dependent | missed; identical at every size |
| pointer between two headers | | 8,343 | 8,343 | 8,343 | 1,580 | | |
| theme | 57,353,195 | 30,755 | 30,755 | 30,755 | 6,220 | at most 12,000; kill above 40,000 | missed |
| edit an exposed cell | (steady p50 1,054) | 1,566 | 1,566 | 1,566 | 395 | at most 800; kill above 3,000 | missed |
| edit an unexposed cell | | 1,067 | 1,067 | 1,067 | 210 | at most 150; kill above 500 | **killed** |
| selection move | | 776 | 776 | 776 | 187 | at most 500; kill above 2,000 | missed |
| scroll three rows | | 5,521 | 5,521 | 5,521 | 1,054 | at most 8,000 | holds |
| scroll six pixels (no row enters) | | 1,426 | 1,426 | 1,426 | 256 | | |
| scroll a page (34 rows enter) | | 30,685 | 30,685 | 30,685 | 5,072 | | 40 per record |
| insert a row in the window | | 11,188 | 85,438 | 7,510,954 | | at most 20,000; kill above 60,000 | holds for the UI: the growth is `Vec::insert` moving the application's own rows (40 B each); the relation's storage is the application's choice and a `Vec` is the wrong one at a million rows |
| idle, re-describing mode | 12,062 (with render) | 15,704 | 15,704 | 15,704 | 2,445 | at most 12 per exposed record; kill above 30 | 20.6 per record: missed |

Other predictions:

* **P11, the finite difference per page: about 550 instructions for the cell fill (predicted at most 200, kill
  above 600).** Read in the disassembly (`delta_col::<Fill<Map<fill, ..>>>`): the compiler vectorised the opaque
  Rust function (a `match` on three booleans) into NEON compares and bit selects, four records at a time, about
  15 vector operations per four records and side, then eight multiplies to pack the mask. So "an opaque closure
  has the weakest derivative" is refuted in the strongest form: the derivative of an opaque pure function is
  exact *and* vectorised, with no structural rule written. It is 2.7x the prediction because both sides are
  evaluated for dead slots too.
* **P12, dispatch.** Read in the disassembly of `World::changed`, `delta_col`, `hit_col`, `list_sync`:
  one indirect call per column that reads a changed word (`Store::delta`); none in any per-record loop; no
  `TypeId` compare in any per-record loop (one per row value in `row_sites`, one per list pass in `list_plan`).
  The view driven by the `Patch` sink is inlined into `World::changed` as about 70 straight-line instructions of
  exposure arithmetic: the fixed part of the view compiles to nothing.
* **P13, chained cache lines from a block write to the tile bitmap: 4, predicted 3, budget 2.** The chain is
  block (compared) -> `Store.cols` (the column table: `reads`, `desc`, page pointer) -> `Desc` (the kernel's
  address) -> the page. The descriptor is the extra one; putting the kernel pointer in the column header removes
  it. The tile bitmap and the store's live words are not chained (their addresses do not depend on loaded
  data). Not measured as misses: needs ilo-prod.
* **P15** holds (section 4).

### Where the instructions are (attribution, by parts measured apart and by disassembly)

* Hover, 7,173: the pointer's hit test 660 (one sparse page of ten headers; most of it is the walk and the
  kernel's prologue), the handler about 20, then the delta: ten pages of cell fills at about 550 each, of which
  two change. A page filter that knew the column of a page would skip eight of them; the general form is to
  evaluate the expression on the *distinct* record values of a page (20 in the whole sheet), not built.
* Theme, 30,755: about 25 pages, each differentiated (550) and marked as one bounding box (about 600, the scan
  of the changed rects for the box).
* An unexposed edit, 1,067 = about 800 for the one thing it does expose (the column total in the footer: one
  column's delta, whose fixed cost is about 300 of prologue and walk) and about 270 for the relation (the view
  driven with the `Patch` sink, the list finding the row outside its window). Nothing unexposed is touched; the
  kill line assumed the total cost nothing.
* A fresh row, about 890 for 22 records: the row function (ten `f32` to hundredths conversions and the tuple),
  22 operand stores, 88 rect stores from the shape, one tile mark.
* Mount, 187,114 to 228,566 for 764 records: of which about 31,000 is writing rows (the scroll-a-page cell does
  the same row work). The rest is one-time: 49 cold `discover` calls, 32 page allocations zero-filled and
  first-touched by the kernel (counted by `thread_selfcounts`), the store tree. Wall: 21 to 38 µs.

## 6. Compile time and size as components grow (P14)

`bench/r8/compile_scaling.sh`: a generated program of N row components (each its own scope and list), built at
`dist`. `distinct`: every component has three functions of its own, so every archetype in the program is distinct
and gets its own kernels. `shared`: components differ in shape only. Time is user CPU of the whole build (the
bough-x library is already built; the row is the program), Mac under load, one run.

| mode | N | user s | `__text` bytes | file bytes | per added component: s | per added component: text |
|---|---:|---:|---:|---:|---:|---:|
| distinct | 1 | 2.32 | 234,272 | 417,392 | | |
| distinct | 8 | 3.23 | 321,568 | 538,784 | 0.130 | 12.5 KB |
| distinct | 32 | 7.03 | 604,000 | 939,824 | 0.158 | 11.8 KB |
| distinct | 128 | 23.81 | 1,742,944 | 2,531,088 | 0.175 | 11.9 KB |
| shared | 1 | 2.17 | 234,272 | 417,328 | | |
| shared | 8 | 2.93 | 274,080 | 473,232 | 0.109 | 5.7 KB |
| shared | 32 | 3.83 | 387,168 | 603,856 | 0.038 | 4.7 KB |
| shared | 128 | 9.67 | 824,224 | 1,115,184 | 0.061 | 4.6 KB |

(`bench/results/r8/expr/compile-scaling.tsv`, commit `4f54042d54` plus the size work of this commit, load 5.7.
The `instructions` column of that file counts only the cargo process and is to be ignored.)

**P14 holds: linear.** The cost of a component at 128 is 1.10x its cost at 32 in time and 1.01x in size when
every archetype is distinct (predicted within 1.3x; kill at 2x). Types stay flat because nothing nests: a
component is a function over a sink, its row a tuple of records.

What a fully distinct component costs, 11.9 KB, by symbol (`nm` on the 32-component build): 5.4 KB is the
*oracle raster's* per-archetype paint (`paint_col::<A, Soft>`, 4.2 archetypes at 1.3 KB; the real one is the
raster lane's to measure), 4.5 KB is the component's own view body instantiated for two sinks with its list's
write and compare loops inlined, 1.2 KB its delta kernels, 0.8 KB the rest. Before the size work of this commit
it was 18.1 KB: `Store::discover` was generic (0.8 KB per archetype of cold code; now one body taking a
descriptor), a list's window logic was generic over the row and the closure (4.8 KB per list; now `list_plan`,
one body, and a generic loop that only asks for rows and writes or compares them), and every delta kernel
carried its own copy of the tile marking (now `mark_rects`, one body over the rect lanes). The hit kernel is
instantiated only for archetypes that have a handler.

The baseline program (N = 1) is 234 KB of text of which bough-x is about 30 KB; the rest is `std` (formatting,
backtrace symbolisation). The façade's workload binary is 6.26 MB with layout, text and Metal in it, so the two
are not comparable yet.


## 7. Refuted or failed, in one place

1. ROUND8: "an opaque closure has the weakest derivative". Its derivative is exact by evaluation at both
   blocks, and the compiler vectorises it.
2. ROUND8: "Frame-stage state costs no per-record CPU work". The delta evaluates exposed records twice (about
   9 instructions per record, vectorised). It is free of the *number of dependents*, not of exposed records;
   making it free of those too means differentiating on the GPU, which is the raster lane's to say.
3. ROUND8: `Expr::delta(old, new, cols) -> Affected` and a `Consts` type. Neither exists.
4. ROUND8: `fn view(.., s: S) -> S::Done` and "branded lifetimes tie handles to their tree". Neither is needed:
   no type lists the view's archetypes and no handle exists.
5. ROUND8: "state write to damaged tiles in at most two chained lines". Four today, three with one field moved.
6. Own: closures as Frame expressions. A Rust closure has no GPU form (wgsl-rs transpiles tokens); it is CPU
   stage. Its cost on the CPU is the delta's, so nothing extra is paid until a GPU has to shade it.
7. Own: per-record `emit` calls into a cursor (first version). 24 instructions per record of which 12 were
   rect work the view should not own and 6 were pointer reloads; replaced by rows as values.
8. Own: driving the view with a no-op sink on every settle (first version: idle 641). A design error, as the
   lead said: the compare and one epoch word decide.
9. Own predictions missed: mount, hover, theme, selection, exposed edit, idle (by 13), re-description (20.6 per
   record against 12), P11. Killed: unexposed edit (the kill line was wrong about what is exposed).
10. Paint order by column is exact only while a store's rows share one layout and do not overlap; the layered
    canvas needs the raster lane's `Layered` path.

## 8. Next, in order

1. **The layout seam** (the lead's condition (c); the façade's sheet is half layout). `List::shape` is already
   the hook: a slice of rects per shape, compared by identity. Replace the hand-placed const array with a
   layout memo: a row's *style* (a second, const value per row type: sizes, flex, padding, and the measured
   sizes of text leaves) is the key; `vendor/cephal` computes the rects once per distinct key; a list's extent
   is `n x row_h` only when every row has one key, and a prefix sum over keys otherwise (round 7's layout lane:
   738 shapes for 39,029 containers). Rects stay lanes of the same pages. Prediction to write first: the sheet's
   mount with cephal in the loop within 2x of today's, since one shape is laid out once.
2. **Text as an expression's output**: a `Text` paint whose operand is a handle to shaped glyphs produced at
   Mount or Turn stage by `bough-text`, as a third column of glyph instances (raster's answer 2). `Num` stays
   for tabular numbers.
3. **Rows through `bough-paint`'s `Paint`**: `bough-x` on top of the raster's crate (SEAM 6.3), deleting
   `kinds.rs` and `soft.rs` except as oracles; `Row::PAINTS` as const data; the start-up plan assembly as a pure
   function of the store tree.
4. **Counters on ilo-prod** when it is free: L1/L2 and branch misses for every cell of section 5, and P13.
5. Columns reserved and never moved (SEAM 6.1), the kernel pointer in the column header (P13), the dictionary
   form of the delta (hover's eight wasted pages), nested lists (a row that holds a list), a relation whose
   storage is not a `Vec`.

## 9. Takeover (2026-11-12, afternoon): the WIP made green, and the hot uniforms

The lane changed hands at `9469bf18f1` (the previous agent's uncommitted work, saved unbuilt). It built; its
gates failed 3 of 4. Cause: a motion was registered only for records the finite difference named, but a glide
that starts now shows exactly what was shown before, so the difference is empty. Fixed at `0e0ec02e9e` (every
live record of a timed column is asked; the fresh-mount oracle mounts at the same instant; `mutant_moves`, which
had silently stopped failing because nothing jumps any more, also drops the current position from a sweep).
What the WIP contained and is kept: the kernel pointer in the column header (`RawCol.delta`; the chain is now
block, column header, page: three lines, as P13 predicted), `Glide`, `World::tick`, `mutant_sweep`. The layout
seam had not been started in code.

Baseline for this section, `0e0ec02e9e`, M3 Pro, `dist`, 10,000 rows, load 6.3 (whole table in
`bench/results/r8/expr/sheet_x-rows10000-0e0ec02e9e.tsv`): idle 55, hover 7,104, theme 31,203, select 773,
frame while the outline glides 631 (a walk of the store tree and one mark of a sweep of 2,520 tiles), frame at
rest 6.

### 9.1 Predictions for the hot uniforms (written before the code)

The question: `Time` changes every frame and the pointer words on every pointer event. Under the plain rule
(a column that reads a changed word differentiates its exposed pages) every record that reads one of them is
evaluated twice per frame. The reformulation: **a hot word is never differentiated; its readers declare bounds
in the word's own domain.** For `Time` the bound is an interval and a window: `until` (after which the record is
at rest) and the box it stays inside until then, both functions of the other words and the record's constants,
so they change only in a turn. A turn that touches a timed column recomputes that column's entry in a short
list kept with the damage (`Tiles::sweeps`); a frame marks the entries and evaluates nothing. `Time` is not
part of the block compare at all.

| # | cell | prediction (instructions) | kill |
|---|---|---|---|
| H1 | frame, nothing in motion | at most 10, the same at any size and with any number of records that read `Time` | above 30 |
| H2 | frame, k entries in motion | at most 40 + 60 k plus the tile rows of the marks; no record evaluated, no store or column touched (disassembly) | any per-record work, or a walk of the stores |
| H3 | the turn that starts a motion (select) | at most 1,100 (773 today) | above 1,500 |
| H4 | a frame of an endless animation (a scene of g exposed records whose colour is an opaque function of `Time`) | at most 100 plus the mark, the same for g = 64 and g = 4,096 | grows with g |
| H5 | the same scene scrolled: rows in endless motion enter and leave | the retained == full == immediate == fresh gates hold; `mutant_sweep` fails them | any pixel |
| H6 | pointer move over a scene where no record reads the pointer words | hit test plus at most 15 | above 60 |
| H7 | pointer move, n exposed records read the pointer words (a proximity glow) | n differentiated per move at no more than 14 instructions each; this is the cell with no bound yet, and the number says whether a pointer bound (an influence box per record, tested like a hit) is needed | above 30 per record |

### 9.2 Results (`479687b4f1`; M3 Pro, `dist`, load 5.6; `bench/results/r8/expr/anim_x-h{208,10288}.tsv`, `sheet_x-rows10000-sweeps.tsv`)

Built as predicted, with two changes of form: the sweeps are a list beside the damage (`Tiles::sweeps`), not
fields of the column, so a frame visits no store; and a column's entry is *recomputed* by a per-archetype kernel
(`active_col`) whenever a turn touches the column, not accumulated, so it is tight and an endless motion that
scrolls out of exposure leaves nothing behind. `Expr::until` is defaulted in `bough-expr`: an expression that
reads `Time` and says nothing never rests.

| # | cell | measured | verdict |
|---|---|---:|---|
| H1 | frame, nothing in motion | 6 | holds |
| H2 | frame, the sheet's outline gliding | 174 for 252 tiles (was 631 for 2,520) | holds; `Tiles::tick` calls only `Tiles::mark` (disassembly) |
| H3 | the turn that starts a motion | select 745 (was 773); a fade's start 583 at either size | holds |
| H4 | frame of an endless animation, 32 and 2,048 animating records | 159 and 2,049 | **holds for records (none is evaluated), fails as written**: the cost is the mark, about 6 instructions per tile row of the sweep (7 rows and 322 rows). The expansion to tile bits is the raster's input; handed the sweep as a rect, the frame is H1's 6 |
| H5 | endless rows entering and leaving, pointer, fade, all oracles | hold; `mutant_sweep` and `mutant_moves` fail | holds |
| H6 | pointer event, nothing reads the pointer words | two compares and two stores (not separately metered; the sheet's `hover` cell did not move: 7,083) | holds |
| H7 | pointer event, n exposed records read the pointer words | 17,781 at n = 2,048 (8.7 each); 3,977 at n = 32 (124 each: every changed record is its own mark, 60 per mark) | per record holds at scale; **no bound exists**, so the count differentiated per event is n |

So, to the question as asked: per frame, `Time` differentiates 0 records on any scene. The pointer words
differentiate every exposed record that reads them: 0 on the sheet as written, and on a canvas whose 5,000
exposed nodes read the pointer about 45,000 instructions per event (extrapolated from the fixture's per-record
cost, not measured on a canvas: `bough-x` has no layered store). A pointer bound (an influence box tested like a
hit, about 3 instructions per record) would gain under 3x and is not built. The cost that does scale with
animating records is in turns: a three-row scroll with 2,048 endless records exposed is 51,967 (the sweep is
found by scanning the live records).

Also found while doing it: a turn that both scrolls a space and starts a motion in it skipped the column
(the space-moved branch of `Store::delta`), so the motion was never swept; the branch now re-sweeps.

### 9.3 The path from a block write to a page: the journal's count was wrong

Section 5 counted block, column table, descriptor, page. That is the path for a column of the root store. The
sheet's cells are in a list store inside a scope store, and stores hold their children and columns in `Vec`s,
so the dependent loads are: world, scope store, list store, column header, page: **five**, growing with
nesting depth. The kernel pointer in the header (the WIP) removed the descriptor, not the tree.

## 10. The element layer, and the review's core findings (written at the close, 2026-11-13)

The lane stopped (an outage) with the element layer and its review fixes uncommitted; the closing agent committed
them as `c6fce8e823` (WIP), merged `r8/surface` (`911a16b590`) and `r8/layout` `fc830898f4`, and finished them
in the commits after. Everything below was run on the Mac (M3 Pro, release) unless it says otherwise.

### 10.1 What the element layer is

`crates/bough-x/src/el.rs`. The author writes the façade's idiom: `fn view(&self) -> impl Element<Self> + '_`,
`div()`, `row((..))`, `column((..))`, `.bg(..)`, `.border(..)`, `.translate(..)`, `text(..)`, `.on_click(..)` and
the other handlers, `each(iter.map(row))`, `.scroll(sig!(self.y))`, `.transition(curve)`, and plain state in
`Signal` fields that handlers set. An element is a value whose type is both its records (`Row<V>`) and its layout
(`bough_lay::Shape`); `Element::drive<S: Sink<V>>` hands it to whatever sink asks (mount, re-description, a turn's
list pass, the immediate-mode oracle, reflection). Nothing is a second runtime:

* `sig!(self.field)` is a lens whose word is the field's byte offset (`offset_of!`): the block is *gathered* from
  the view's own fields by the table of slots the columns name, so a handler's write is a plain store, found by
  the block compare. No field is numbered by the author.
* `.map(closure)`: the closure's captures are the record's words, so `move |l| fill(*l, col, row)` is one
  archetype for every cell.
* `.transition(curve)`: where the value started from, and when, are words of the record, written by the turn that
  moves its target (`Expr::carry`); frames evaluate nothing.
* `each(iter)` under a scrolled box is a list by exposure: the iterator is indexed (`TrustedRandomAccess`), never
  walked, so 10,000 rows cost what is exposed.
* handlers are components of the box's record; their captures are a lane beside the column.

### 10.2 What the close changed, and why

1. **Layout as data (compile time).** The layer called bough-lay's typed layout (`Lay`: the flex algorithm
   monomorphised per element type). The façade's sheet as one view type (`tests/sheet_el.rs`) type-checked in
   seconds and its code generation did not finish in hours, optimised or not (the lane killed it after 3 h 32 min;
   it went behind a feature, `el_sheet`). The layout lane measured the same cause (a chat row type: 204 s, 2.75 MB)
   and replaced the typed form with layout as data (`bough_lay::shape`: an element emits one 24-word node, one
   layout function lays out any row, a list keeps a `Memo` of the last shape's rects). The element layer now uses
   it: `Text`, `Hd` (a box with handlers) and `Scroll` implement `Shape` (a scrolled box is a box with one child
   out of flow at its corner, as large as the box: the hole its content's space is cut from); `Element::drive`
   lays the root out through `Flat` and `place_root`; a list of elements lays an entering row out through the
   list's `Memo` (a row of the kept shape at a width inside the kept answer's validity interval is a copy, any
   other row goes through the one function). **`tests/sheet_el.rs` now builds in 4 s** and the feature gate is
   gone. The memo's key is a 64-bit hash of the row's layout words (the layout lane's choice; a collision would
   reuse another shape's rects, which the gates below would show but nothing prevents).
2. **Mount read the view's fields before it knew them.** The slot table is learned from the columns, which exist
   only after the first description, so every element view was first described and painted with its fields read
   as zero (the sheet: `look` 0, so column A showed hovered until the first hover). `World::mount` now describes,
   learns the slots, and describes again from nothing when the gathered block differs (cold, once per mount; a view
   that declares its block is described once). Described again, not settled from zero: a settle would start a
   transition from a value never shown. A later pass that makes columns naming fields no column named before
   (`World::learn`) gathers and settles them; such a word's first change can start a transition from zero (stated,
   not fixed).
3. **The immediate-mode oracle cannot show a transition in flight.** A description from state alone is at rest;
   the in-flight start is a record word. `tests/sheet_el.rs` compares stores == immediate whenever nothing is
   moving (as its fresh-mount check already did) and retained == full redraw at every step.

### 10.3 Gates

At the head of this section's commits: `cargo test --release -p bough-x -p bough-expr -p bough-lay -p bough-ax`
green: bough-x anim 2, el_small 2, events 12, reflect 7, review 7, review_core 12, sheet 4, **sheet_el 4** (the
façade's sheet, 10,000 rows: 300 random steps mount-once, 200 re-describing, 20 rows 150 steps; nothing unexposed
exists), table 1; bough-lay extent 3, oracle 4; bough-ax 11 (1 ignored); bough-expr 1.
Mutants, each failing a gate: `mutant_delta` fails el_small (2 of 2) and sheet_el (3 of 4: retained != full
redraw at step 1); `mutant_journal` fails events (2 of 12); `mutant_moves` fails sheet (3 of 4); `mutant_sweep`
fails anim (2 of 2) and sheet_el's 20-row script (step 5); `mutant_reflect` fails reflect (1 of 7); bough-ax's
`mutant_fast` (3 of 12) and `mutant_removed` (1 of 12) fail ax; bough-lay's `mutant_gap` and `mutant_valid` fail
oracle (2 of 4 each). sheet_el alone does not catch `mutant_journal` (its rows are a range, not an edited
relation), `mutant_moves` or `mutant_reflect`; the core's sheet, events and reflect gates do. Not run on the
remotes at this head; `c6fce8e823` and `911a16b590` (before the layout merge) passed the same suite and mutants on
ilo-prod.

### 10.4 What the façade's sheet needed changed (`scenes/sheet_el.rs`, from `comparisons/bough-workloads/src/sheet.rs`)

The import line; a run of children inside a row is an array (`std::array::from_fn(|c| cell(r, c))`) where the
façade writes `each((0..COLS).map(..))` (a row's record count is in its type until rows can hold runs); no
benchmark `Scene` impl. State, handlers, `sig!`, capturing closures, builder chains, `each` for the rows, the
scrolled body and the selection's transition are as written there.

### 10.5 Limits, stated

* Text is a stand-in measure (12 device pixels a character, 15 tall, 24 of inset) and a stand-in raster; the
  text lane's `bough-face` is not wired in. `NumFmt`'s prefix and suffix are not drawn; interned strings are
  never released; a bordered box has no fill.
* Frame state is whole words: a field read by `sig!` is 4-aligned and at most 4 bytes, and two fields in one word
  of the block cannot both be Frame state (asserted when the table is built). A mapped closure captures only
  `u32`, `i32`, `f32` or `Color` values (asserted at compile time).
* A row or a root has at most 96 elements and 12 wrapping leaves (`Flat`); a container at most 32 children in
  flow. Past that, a panic names the limit.
* Not measured: the element-layer sheet's turn and frame costs against the core's hand-written sheet
  (`examples/sheet_x.rs`). No driver example exists for it; that is the number that says what the idiom costs
  over the core's own, and it is the next measurement. The gui-bench app (`comparisons/bough-guibench-x`, e2e
  lane) does not use the element layer.

### 10.6 The review's core findings (C01-C13), state at the close

| finding | state | where |
|---|---|---|
| C01 erased type identity | **closed** | view: `World::mount` keeps the view's `TypeId`, every entry point asserts it (`world_rejects_different_view_type`). Storage (the close): `World::root` is private (read through `root()`), the slot table crate-private, `Store::paint` and `Store::delta` crate-private (their columns were described for one backend and view only the owning world knows); a test's reference delta is `World::tree_delta` |
| C02 list row compatibility not a type proof | **closed** | admission compares each leaf's archetype with its column, once per pass (`Same`); `same_size_list_type_change_is_not_silently_accepted` |
| C03 moving paint picked at old bounds | **closed** | pick applies the paint offset; `moving_target_is_picked_at_visible_position` |
| C04 pointer equality as layout identity | **closed** | the shape is compared by its bytes every pass; `in_place_shape_changes_update_layout` |
| C05 focus and press by position | **closed** (surface lane) | `focus_follows_item_through_insert`, `press_is_cancelled_when_slot_reused` |
| C06 pointer words | **closed** (surface lane) | `pointer_updates_expression_words` |
| C07 structural limits | **in part** | columns <= 4,096 asserted when the table is built; a list's rows positive in height, under 2^30, total under 2^31 px, refused where described (the close: `sink::admit`; `zero_height_list_is_refused_where_described`); the window grows its ring (`window_can_grow`). Standing: paths of six 16-bit components; 16-bit reader incidences (asserted when the table is built, not widened) |
| C08 ancestry by overlap | **stands** | surface journal section 8 |
| C09 dispatch allocates | **closed** (surface lane) | surface journal section 7 |
| C10 `Words` alignment | **closed** | a non-empty record type is whole words at alignment 4, asserted where its words are taken |
| C11 wrap-unsafe time | **closed** | modular deadlines (`ahead`, `latest`, `ramp_end`); `active_ramp_survives_clock_wrap` |
| C12 memory is a column subtotal | **in part** | `Store::bytes_all` counts every byte the store tree holds; application and renderer memory are not in it |
| C13 grouping after geometry changes | **closed** | a fixed record that moves over a column painted before it regroups the stores (`order_holds`); `changed_geometry_preserves_paint_order` |
| L03 (layout) in bough-x's list | **closed** | exposure is bough-lay's `Uniform` extent (64-bit) through `Extent::window`, a zero height refused where described; `exposure_is_the_rows_that_meet_the_clip` (200,000 random lists against a brute-force count) |
