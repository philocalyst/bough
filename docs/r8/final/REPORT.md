# bough: closing report for rounds 6 to 8 (2026-11-12)

This is the hand-over for whoever continues: what stands, how it compares, what was refuted, every avenue still
open, and what has to be true before any of it is called finished. The running record of experiments is
`docs/TALLY.md`; the design of the new core is `docs/r8/ROUND8.md` with its seams beside it; each lane's own
final note is in `docs/r8/final/`. An independent review of all of this work (about ninety findings, sixteen
failing probes) is in the `independent-review` lane's report of 2026-11-12, and section 3 here follows its gates.

Hosts: an M3 Pro Mac; `ilo-prod` (Ryzen 7 3700X, Linux, shared with other tenants); `mothership` (Xeon E-2236,
Linux, quiet). Unless a unit is given, a number is instructions. A number is "measured" only for the binary and
host named with it.

## 1. State in one page

There are three bodies of code, and they are not yet one executable. Every claim below belongs to one of them.

| | what it is | where | what ran through the public benchmark |
|---|---|---|---|
| A. the façade | the framework an author uses today: `View`, elements, `sig!`, handlers; rounds 6 and 7 underneath (write log, gapped directory, exposure-driven mounting, guarded turn, record-space damage, rigid moves, segmented wires, one integer raster kernel, Linux/Wayland host, lazy GPU device) | `crates/bough*`, on `main` | all of zortax's gui-bench matrix, fixed 60 Hz and unthrottled, plus sheet and canvas |
| B. the new core | UI as typed dataflow: staged expressions, a 64-word state block compared once per turn, one column per view call site, nothing unexposed exists; with events, reflection, an accessibility projection, staged text, typed layout, per-archetype kernels, two-half publication | `crates/bough-expr`, `bough-x`, `bough-paint`, `bough-lay`, `bough-face`, `bough-ax`, `bough-host`, `bough-span`; partly on `main`, the rest on `r8/*` branches | by hand under the same compositor (start-up and rows hover); the runner registration was in progress when the round closed |
| C. lane experiments | arena and scene image (threads), corner tables and GPU-resident path (raster), the text API proposal and stand-in shaper design (text), the round-7 async engine | `r8/*`, `r7/*` branches | nothing |

What that means in practice:

* The scoreboard's bough row is A. The order-of-magnitude numbers (start-up 4.3 ms, hover 0.08 ms, 764 records
  for a sheet of any size) are B, on fixtures written against the core directly, with a prepared Latin-1 face,
  opaque paints and rows placed by arithmetic. They show what the representation costs; they are not yet what an
  author's program costs.
* The one piece of work that converts B's numbers into the framework's numbers is the **element layer**: the
  façade's `View` and elements driving the new core through `Element::drive<S: Sink<V>>`. Its first version is
  on `main` (`bough-x/src/el.rs`): `fn view(&self) -> impl Element<Self>`, `div`, `row`, `text`, `sig!` by
  `offset_of!`, captures as record words, `.transition`, indexed `each`, rows laid out by `bough-lay`. The
  façade's 10,000-row sheet written with it holds the oracles (retained equals full redraw at every step, and
  equals immediate paint and a fresh mount whenever nothing is in motion). It has **not been measured** against
  the hand-written sheet, has no text through `bough-face`, no runs inside a row, and the benchmarked new-core
  app does not use it. Until the façade's scenes run on it, B is a prototype.
* The first attempt at that sheet on typed (monomorphised) layout type-checked in 1.8 s and then did not finish
  code generation in 3 h 32 min. On layout as data it builds in 4 s. Scaling measured on flat generated
  components had said nothing about one deep real view.
* The author's API has not changed and must not: all of the performance work is below it.

**What is on `main` at the close (`cc65ee215f`)**: the façade; the new core with the element layer, events,
reflection, the accessibility projection, typed layout and extents; the text lane's staged text; the raster
lane's per-archetype kernels; the new host and the new-core benchmark app; this report and the tally. The
whole-workspace suite on the Mac one commit earlier ran 209 test targets: 401 passed, 0 failed, 3 ignored. Both
bough benchmark binaries built from `main` passed the runner's layout and behaviour checks in all eight
scenarios (14 cells, quick mode, fixed and unthrottled).
**Not on `main`**: the threads lane (`r8/threads`: the arena, two-half publication, scene image, and its fixes
for P01-P04), which conflicts with the core in 19 places including a second allocation-free pointer path; the
round-7 async engine; the heavy scenes' benchmark crate; some round-6 lane work (section 4.1).

## 2. Standing against the other frameworks

Measured on `ilo-prod` under the fakeland compositor (software rendering for everyone: bough's CPU executor into
`wl_shm` on one thread, the Skia and wgpu frameworks on lavapipe with many). Benchmarks pinned to CPUs 0-7.
Unthrottled means the compositor presents when the client commits, so no number contains a wait for a 60 Hz
latch (under the fixed latch about 10 ms of every framework's hover total is that wait).

**Façade binary (A), unthrottled, against the best other valid value**

| cell | bough | best other | |
|---|---|---|---|
| rows 3,000 hover, total | 0.99 ms | iced 5.35 | ahead 5x |
| typing | 0.25 ms | zgui 4.06 | ahead 16x |
| search filter | 6.99 ms | iced 9.86 | ahead 1.4x |
| table update 10 / remove | 0.37 / 3.73 ms | iced 7.16 / freya 8.1 | ahead 19x / 2x |
| resize | 5.8 ms | iced 8.2 | ahead 1.4x |
| virtual 100,000 start-up | 39.1 ms | vizia 104.5 | ahead 2.7x |
| rows 3,000 start-up | 56.3 ms | iced, egui about 165 (60 Hz run) | ahead |
| idle CPU, idle RSS (rows) | 0.03 ms/s, 34.8 MiB | freya 14.7 ms/s; iced 169 MiB | ahead |
| spinner and ticker frame rate | 62.5 fps | egui 1,248, freya 153 | **behind**: the benchmark app drives animation from a 16 ms clock, not from the output |
| sheet 10,000: hover / theme / RSS / start-up | 19.4 ms / 27.9 ms / 331 MiB / 677 ms | egui 9.95 / 11.2 / 149 / 136 | **behind 2x to 5x**: the façade's sheet mounts every cell |
| sheet 10,000 scroll and resize | failed (connection reset) | egui 22 ms resize | **failed** |
| canvas 1,000: hover / theme | 83 / 286 ms | iced 17.6 / 20 | **behind 5x to 14x** (shadows on one CPU thread) |
| canvas 5,000: theme / resize | 1,151 / 386 ms | egui 71 / 70 | **behind 5x to 16x** |

**New core (B), by hand with the harness's own presets, same compositor**

| cell | new core | façade | best other |
|---|---|---|---|
| spawn to first buffer, rows 3,000 | 4.3 ms (an empty client: 4.2) | 64.7 ms | iced, egui about 165 to first present |
| spawn to first buffer, virtual 100,000 | 4.3 ms | 35.9 ms | vizia 104.5 |
| rows hover, input to commit, unthrottled | 0.074 to 0.079 ms at 3,000 and 100,000 rows | 0.90 ms | iced 5.35 total |
| sheet, any size (Mac, instructions) | idle 53; hover 6,826; select 694; exposed edit 1,433; theme 31,010; a still frame 6 | | |
| event dispatch, 100 to 1,000,000 rows | pointer onto a row 1,270; click 3,397; key 120; 0 allocations | | |
| sheet columns | 41.7 KB at any row count (a column subtotal, not RSS: review C12) | 331 MiB RSS at 10,000 | egui 149 MiB |

**How far ahead, said plainly.** On lists, tables, typing, resize, idle cost and start-up the façade is first in
the matrix, by 1.4x to 19x, and the new core adds another 10x on the two cells it has been run on. On the two
heavy scenes (a 10,000-row sheet and a layered canvas) the benchmarked binary is behind, and one sheet cell
fails outright. The new core removes the sheet's cause (it holds 764 records at any size) but has not been run
as that benchmark. The canvas is a raster problem that is not solved: the best CPU path is 316 ms a full frame on
one core, 93 ms on six, and the GPU-resident path is 17 to 19 ms on Metal, against a 9 ms target.

**What these comparisons do not show** (review B01-B03, all accepted):
* no hardware-GPU configuration was measured, on any framework;
* CPUs 0-7 and 8-15 on that host are SMT siblings, and other tenants were loading it (load 4 to 21); builds were
  niced, not stopped;
* the first matrix's start-up column was taken unpinned under load and is superseded by the pinned run;
* invalid cells exist for every framework (130 of 587 reports in the 60 Hz matrix); the dashboard keeps measured,
  invalid, and estimated values apart, and no ranking uses an estimate.

Dashboard: three views (unthrottled, fixed 60 Hz, variable refresh), with per-cell validity and extrapolated
values marked as such. Its source and data are `bench-r8/` in the lead's worktree.

**Final re-run (2026-11-13, from `cc65ee215f`)**: the whole matrix again on `ilo-prod`, once with the 60 Hz latch
(`GB_TIMING=fixed`) and once unthrottled: 658 cells each, runner exit 0, host load 0.61 to 22.31 (mean 5.7) and 0.10 to
23.41 (mean 6.0). Binaries: `bough` (the façade, 55,883,088 bytes) and `bough-x` (the new core's app, 1,109,864 bytes).
`bough` has three cells with errors in the fixed run and two in the unthrottled: search at 60 Hz with 10,000 rows
("filter: no matching window", "typing: timed out"), rows at 144 Hz with 10,000 ("scroll: no matching window", fixed
run only) and the coarse table at 60 Hz with 10,000 ("op-clear: no matching window", "churn: the wait timed out"), so the façade still fails on the largest sizes. `bough-x` had no cell errors in either run. Host-noise invalid
measurements: 16 (`bough`) and 12 (`bough-x`) in the fixed run. floem fails the layout check in 54 of 56 cells,
vizia in 8, gpui-kit in 3. The heavy scenes were not re-run (the only build on the host was round 7's). The runs are
published as a static copy of the zortax explorer (the `bench-r8/dashboard/explorer/` snapshots); its controls are
inactive.

## 3. What must be true before this is called finished

These are the review's five gates. They come before any new optimisation.

1. **Safe interfaces must be sound.** Unsafe invariants were held by comments at several boundaries. Closed on
   `main`, with the review's probes turned into tests: C01 (view identity checked at the entry points at run
   time; the store tree, slot table, `Store::paint` and `Store::delta` no longer public), C02, C03, C04, C05,
   C06, C09, C10 (alignment), C11, C13; S01, S03, S04; T01, T02, T03, T07; R01-R05, K01, K02, K08; O01, O02;
   H02, H04 (for opaque surfaces), H06, H07; L03. **Still open**: A04 and the rest of A01-A06 (the round-7 async
   engine, not on `main`); K03 (unvalidated GPU programs and device ownership); P01-P04, fixed only on
   `r8/threads`, which is not merged; C07 in part (paths of six 16-bit components and 16-bit reader incidences
   stand; 4,096 columns and list sizes are now admitted or refused where described); C10's purity half (an
   expression is assumed pure and nothing enforces it); the list memo is keyed by a 64-bit hash with no
   equality check. C01 is a run-time check, not `World<V, B>`. None of Miri, Loom or a sanitizer has been run on
   any of it.
2. **The sixteen failing probes pass on the integration branch**, not on lane branches. The ten core probes do
   (as `tests/review_core.rs`), as do the three text probes and the over-aligned carve; the cross-thread `Line`
   probe is closed by a compile-fail test. Re-running the review's own probe files against `main` unchanged is
   the confirmation still owed.
3. **One end-to-end candidate**: every scenario (rows, virtual, table, typing, search, spinner, sheet, chat, ide,
   dashboard, canvas) through one binary built from one branch, with the runner's layout and behaviour checks,
   and a fallback for every paint and text capability the fast paths do not admit.
4. **Whole-operation measurement under control**: exact manifests per binary, randomised paired blocks, a quiet
   host or disjoint physical cores, cold and warm, p50/p95/p99, all-thread CPU, retained and peak memory.
5. **Every "flat" claim tested past its admission limit**, with its full retained-state ledger.

Known limits of the new core that a continuation will hit (C07): six path components of 16 bits; 12 bits of
column in a packed place while 65,535 columns are admitted; 16-bit reader incidences; `i32` list positions in
`bough-x` (`bough-lay` has `i64` extents; `bough-x` has not adopted them, so a zero row height still divides by
zero and a viewport larger than its first ring still panics there).

## 4. Every avenue still open

Grouped by area. Each entry: what, why it should pay, the smallest experiment that decides it, and what is
known. "Size" is effort: S (days), M (a week or two), L (more). Entries marked **[review]** are the review's
reformulations; the rest came from the lanes or the lead.

### 4.1 Integration (do first)

| avenue | size | notes |
|---|---|---|
| Element layer, the rest of it: every façade element and modifier, handlers as methods, text through `bough-face`, runs inside a row, nested lists, the layer driving the benchmark app and the five heavy scenes | L | The decisive item. The first version is on `main` and holds the oracles on the façade's sheet. Owed at once: its cost against the hand-written sheet (hover 6,826, theme 31,010, mount about 230 K), and compile time and code size per component on a deep real view, measured early. Row identity must not be a Rust type that depends on the sink. Predicted by the text lane: the façade's text programs run unchanged in source. |
| Merge `r8/threads` (arena, two-half publication, scene image, P01-P04) | M | 19 conflicting hunks in the core's world, store and sink; both sides have an allocation-free pointer path. Decide which path stands, then port the arena under it; or go straight to frame versions (X5) and take only the arena. |
| The edit regression after the review fixes: an exposed edit went 1,433 to 2,612, mostly in one function (`part_journal_only`, 359 to 1,538), cause not attributed | S | Measured once at load 96, so first re-measure on a quiet host. |
| One machine-generated matrix of feature x commit x gate for the integration branch **[review O13]** | S | The "kept" list is not one executable today; `main` descends from before some round-6 lane work (direct wires, owner grain, dormant runs, composition), which lives on `round6` and lane branches. |
| Round-7 async engine merged, with the held-state regression carried through the merged runtime (A01) | M | `main` still has the round-6 reader cohorts and a thread per request. |
| `bough-glyph` and `bough-face` made one prepared-face format; generic kernel, per-archetype kernels and the span executor under one paint contract (alpha, clips, order, numeric ranges) **[review]** | M | Two text stacks and three rasters exist. The generic integer kernel stays as the executable specification. |
| The five heavy scenes and all runner scenarios on the new core | M | Only rows, virtual, table, search fixtures exist there. |

### 4.2 The typed core and incremental computation

| avenue | size | notes |
|---|---|---|
| Typed owner with a validated erased schema **[review]**: `World<V, B>`; a descriptor that proves leaf types, layout and handlers once at admission | M | Closes C01/C02/S01 for good; the hot path must gain no per-record check. |
| Partial evaluation by dependency **[review]**: hoist record-only subexpressions, route a changed word only to columns that read it | M | Per-word reader lists exist; page filters from structure do not. Measure code size and I-cache, not only instructions. |
| A pointer bound: an influence box per record that reads the pointer words | S | Today every exposed reader is differentiated per pointer event (17,781 at 2,048 readers; an estimated 45,000 on a 5,000-node canvas). Expected under 3x. |
| Sweeps handed to the raster as rects instead of tile bits | S | An endless animation's frame costs 6 per tile row of the mark and nothing per record; as a rect it is the 6 of a still frame. |
| The live-record scan when rows in endless motion scroll (51,967 for a three-row scroll with 2,048 animating) | S | Keep the active set per column incrementally. |
| Edits that change geometry (Turn stage), cold, end to end | M | Not measured as one operation. |
| Collection operators with maintained indexes **[review]**: filter, sort, group, join, aggregates from deltas | L | The search cell is a full scan per keystroke by design now (the app-level prefix cache was deleted as benchmark-specific). The framework-level operator is the legitimate version: a substring index (suffix array or n-gram postings), result deltas, exposure-limited materialisation. Test append, backspace, middle edit, replacement, dataset edits, low-selectivity queries; count index memory. |
| Order lane so an insert costs one row; `Rel` as something other than a `Vec` | M | `Rel` insert and remove still move a suffix (C12). Gapped blocks from round 6 are the known answer for the directory; not applied to relations in the new core. |
| Nested lists as one child store; depth-independent path from a block write to a page | M | Five dependent loads on the sheet, growing with nesting. Shortening them did not help cold (refuted), so the reason to do this is the limits in C07, not speed. |
| Column grouping revalidated when geometry changes (C13) | S-M | Painter order is decided at discovery and kept; a moved record can paint under a sibling. Needs a stable order lane and commuting runs proven per change. |
| Type-level techniques not yet used: continuation-passing with higher-ranked lifetimes to brand a turn's borrows; const-generic stage sets; `min_specialization` for row kinds | S each | Branded handles were built and dropped (the core has no handles). The CPS form is still the candidate for making "this reference cannot outlive the published frame" a type error, which is what P02/R01/R03 need. |
| Bit-packed places and descriptors with generations | S | The packing exists; it is too narrow (C07) and has no generation, which is the cause of C05 and S05. |

### 4.3 Identity and publication

| avenue | size | notes |
|---|---|---|
| Item key and generation in relations; one publication transaction that remaps focus, hover, press, accessibility ids and reflection **[review]** | M | Surface made focus follow its item and cancels a press whose slot shows another row; hover leave on a reused slot is still lost; accessibility node ids are positional. A decision for the user (section 7). |
| Immutable frame versions with reclamation **[review]** | M-L | Two halves are in place and tested (2,000,000 publishes), and the threads lane moved readers to atomics; the review's point stands that a fixed published half serialises publication behind a long frame (P05). Versions also give the interaction path a frame it can repair while a full frame is in flight. |
| A frame generation that binds sampled state, pick, and accessibility projection (R10) | S | The raster driver's CPU branch picks before it updates. |
| Lexical ancestry as a cold parent column, separate from spatial bins (C08) | M | Bubbling and key routing are reconstructed from overlap today; Tab cannot be declined; no key-up, repeat or function keys. |

### 4.4 Layout

| avenue | size | notes |
|---|---|---|
| Wire the kept form (layout as flat words, one function, a memo; `Extent`) into `bough-x` and the benchmarked binary | L | Hit 2,850, copy of a kept slice 200, miss 12,657 to 91,359. Chat resize estimated 1.4 M against 181 M. Not wired anywhere. |
| Parametric answers and line-break topology **[review L05]** | M | Equality intervals were useless on chat rows (0.0 % of row-pixel pairs). An arrangement with affine width terms and breakpoints is the derivative that survives a bubble sized as a fraction of the column. Oracle: the general solver at every sampled width. |
| Hierarchical extents with sparse exceptions **[review L02]** | M | Variable heights cost 12 bytes per logical row today, exposed or not. Uniform spans as count x height, exceptions in a blocked summary; 100 M rows as the test. |
| A sound shape check for Fixed lists without hashing each row (the hash is 2,677 of a 2,850 hit) | S | |
| A straight-line solver for fixed and flex rows under proven conditions, the general solver on misses | M | The monomorphised general algorithm was 125x worse than copying and cost 204 s to compile for one row type: specialise the regime, not the algorithm. |
| `guess(w)` for unmeasured rows (93-98 % exact in the text lane), pending measures, a (row, offset) scroll anchor | M | |
| Coverage: indefinite percentages, min/max-content, baseline, absolute children, DPI changes, a real measure callback (L04) | M | The random-tree oracle excludes these. |
| The round-7 shape memo's eviction and interning cliffs (L06) | S | Bulk clear at a budget; `ShapeTable` unbounded. |
| Sheet resize at the gutter breakpoint (L07) | S | A semantic trigger (every cell moves when the gutter shrinks), not a scheduler cliff; a rigid translation per compatible row is the available saving. |

### 4.5 Text (the widest gap to gpui-ce and the web)

The proposal is `docs/r8/TEXT-API.md`: fourteen programs in the façade's style, a gap table of about forty
capabilities against gpui-ce and CSS, and an order. Nothing of the API is built. What exists is the staged
representation underneath (chat first paint 94,600 against 1.37 M; typing one character about 1,200 at 1,000
or 300,000 lines; appends flat in length).

| avenue | size | notes |
|---|---|---|
| The full shaper behind the memo, off the interaction path | M | Designed: a worker owns the vendored parley; a job copies its text; the interaction draws a stand-in and re-describes when the result lands. Without it the fast path refuses anything outside prepared Latin-1 pairs, and one fixture turned a refusal into an empty run (T05). |
| `text`, `para`, `text_input` as elements on the new core | M | Blocked on the element layer. |
| Spans: weight, size, colour, decoration, features, axes per run; style inheritance; families with fallback | L | Order 1 in the gap table. |
| Wrap modes with UAX 14, truncation, clamp, alignment | M | Order 2. |
| Selection of static text, copy, index-to-point and point-to-index, measure without mounting | M | Order 3. Selection needs a paint whose geometry is a Frame-stage operand. |
| `TextState`: editing as a plain value; IME composition; undo groups; platform motions; multi-line editor | L | Order 4. gpui-ce's editor is single-style, single-cursor, O(n) per edit: the target is above it. |
| Large documents and highlighting by ranges and a palette operand (the ide scene is 14,585 hand-made spans) | M | Order 5. |
| Inline boxes, links, interactive ranges | M | Order 6. |
| Bidi, complex scripts, colour emoji | L | Order 7, through parley. |
| Generational glyph runs with a collector **[review T04]** | M | The memo keeps every distinct text for ever; a `Run` is offsets with no generation. In-place compaction through a typed walk of line columns is the lane's design. |
| Streaming cliffs (T06): an ever-growing unbroken token is O(length) per append; a huge line moves all following bytes | S-M | Segment end pens; byte-bounded blocks. |
| Build-time shaping of constant strings into a mappable glyph image | M | For start-up and for binaries with no shaper linked. |
| Rendering quality: gamma or contrast tables per platform, hinting policy on low-density displays, SDF or coverage-profile glyphs for large and animated text | M | A user decision (section 7). |
| Two-stage scanning in the simdjson manner for break opportunities, newlines and token classes (a structural bitmap pass, then a branch-light walk) | S-M | Not tried. The per-paragraph break table was refuted because building it cost more than shaping (12 against 7.8 per byte); a SIMD classification pass changes that arithmetic and should be re-measured, as a new idea, not a retry. |

### 4.6 Raster and GPU

| avenue | size | notes |
|---|---|---|
| Tile prefix caching **[review]**: damage carries the lowest changed painter depth; reuse what lies below it | M | The largest candidate for canvas hover (shadows are 16 of 22.5 ms a frame and do not change on hover). Reject if prefix storage and invalidation exceed the shading saved on mixed scenes. |
| Hover damage by layer and visibility | built | Canvas hover 1,576 to 527 tiles, 6.9 ms of GPU time; theme 4,000 to 3,628 tiles, 19.3 ms. A one-record hover (30 tiles) is 40-43 µs on one core against 254-498 µs wall through the GPU, so the executor must be chosen per damage, which the static plan does not do (refuted as it stood). Measured on the Mac under load; not in the benchmarked binary. |
| Executor choice per damage from the affected tiles' work | S-M | Today one `Wide` paint anywhere pushes every estimate to the 8-tile threshold (R06). |
| The new raster as the benchmarked path on Linux: per-archetype kernels behind the façade's canvas, six bands | M | One core does a canvas full frame in 4.48 G instructions (287-315 ms) against the round-7 generic kernel's 21.1 G (1.7-2.0 s), and 66-77 ms on 5+1 cores. The benchmark's canvas cells (83 ms to 1.15 s) are the old path on one thread. |
| GPU start-up: 2.5 s to compile new Metal source and 19 pipelines, 6 ms from the system's cache | M | A build-time shader plan and per-archetype (not per-column) source; first frame 31-37 ms against the presenter's 60. |
| Work estimation learned per machine, from post-cut work and usable bands, with regret reported **[review]** | M | Constants are M3 Pro's; Linux never chooses the GPU; the pool is created eagerly (K04). |
| Incremental binning: changed-page bin deltas, retained overflow lists | M | Any moved window rebins the whole lane; the GPU bin pass has a fixed 192-entry slab with CPU redraw on overflow (R07). |
| Occlusion masks: worker-local scratch, compressed identical masks, stop at full cover (R08) | S | The certificate can cost more than the shading it saves on sparse text. |
| Bands owned by cores for the CPU kernels; the interaction path never queued behind a full frame | M | Six threads give 3.4x on a canvas frame; `Scene::shade` does not check that tiles are disjoint (R02). |
| A cross-platform GPU executor from the same typed shader source: vendored wgpu or direct Vulkan beside Metal; build-time shader plan, per-archetype source | L | The GPU path is handwritten Metal orchestration compiled at run time (R09). Vulkan compile cost unmeasured. |
| Shaders and transitions as first-class paints: an author-written `wgsl-rs` function as a paint kind with its operands staged like any other | M | The kernel is written once through wgsl-rs and is word-exact on CPU, Metal and x86, so the mechanism exists; the author-facing form does not. |
| Blender-style GPU practice not yet taken: batched draw lists with persistent buffers and sub-range updates, a dependency-graph of offscreen passes, shader variants specialised by a small key and cached on disk | M | Sub-range updates exist as in-place record rewrite; the rest is open. |
| Late latching of pointer, time and scroll words; pick in the frame being drawn; speculation of likely next states into spare tiles | M | The state block makes late latching a 64-word copy. |
| Animation endpoints (K08): interpolation keeps derived fields of the first command | S | A shadow whose blur changes never reaches its target. |
| Coordinates beyond 16 bits in raster windows (R04); a 33rd archetype (R05); empty atlas (K02) | S each | |
| Independent visual fixtures: transformed effects, fractional DPI, anisotropic transforms, atlas misses (R11, K07) | M | "Exact" today means paths agree with each other. |
| Images: 1:1 blit command, a 64 x 64 tile pool read in place, YUV tiles, decode admission and cancellation **[review]** | L | Design only. |
| Canvas on the GPU under 9 ms | M | Corner tables did not get there (16.8 to 18.9 ms): with a table, the remaining per-pixel arithmetic is two thirds of the closed form. Next candidates: prefix caching above; shadows rasterised once per (radius, blur, size class) into an atlas and stretched as nine patches; separable profile textures. |

### 4.7 Start-up

Taken apart on the new core: connect at 1.4 ms (all before `main`: exec and the loader), handshake 20 µs of
client time, mount 27 µs, and **2.6 ms of first touch of the buffer's pages** (1,000 page faults), which is 60 %
of spawn to first buffer for an empty client and for the app alike.

| avenue | size | notes |
|---|---|---|
| M1 `fallocate` plus `MAP_POPULATE` for the first buffer | S | Predicted 2.6 to at most 1.0 ms. Not run. |
| M2 huge pages for the memfd | S | Two faults instead of 1,000 if the host's shmem policy allows. Not run. |
| M3 touch pages from other threads while the main thread connects and mounts | S | Not run. |
| M5 static linking, nothing before `main` | S | Predicted 0.3 ms. Not run. |
| A first-frame pixel image handed to `wl_shm` from the page cache, verified by the idle raster and corrected by damage if wrong **[review, as an optional cache]** | M | Measured in isolation on mothership for a 2560 x 1600 frame: an anonymous shared mapping written once is 5,965 µs and 4,000 faults; the same pixels mapped from a file in the page cache are 42 µs and 11 faults. On the Mac there is no gain (690-900 against 930-1,140 µs). Predicted first-buffer cost under 0.1 ms on a warm cache; kill: a compositor that copies the pool on attach. The format slot is reserved in the image head; the section is not written. Conditions: ext4 with large folios, warm cache; tmpfs unmeasured. The scene image itself saves about 15 µs and is not a start-up lever (refuted). |
| The façade's start-up (56 to 65 ms) on its own terms: 115 ms before the Wayland connection in the worst cell, font discovery, GPU bring-up | M | Moot once the element layer lands; otherwise the first thing to take apart on A. |
| macOS start-up on the new core | M | Not measured. Round 7's lazy device took the façade from 64-89 ms to 10-15 ms. |

### 4.8 Latency after the write

| avenue | size | notes |
|---|---|---|
| What the 80 µs of a hover is made of (wake-up of the process, two system calls, repair of two rows in two buffers), cold and warm | S | Target was 50 µs; missed by 1.6x. The event path's allocations are already gone. |
| Cold interaction: a hover is 3,100 cycles warm and 9,300 to 12,300 with L3 cold; about 210 data lines **[review: locality before shorter chains]** | M | Attributed so far: record pages 18 of 93 lines, store tree 24, heap 24, code 60 to 86. Co-locate descriptor, interaction mapping, live mask and changed words; prefetch from the pick at input arrival. Change one layout choice at a time on the quiet host. |
| Commit on input when a frame callback is pending (newest state wins before the latch) | S | |
| Animation paced by the output: the spinner and ticker cells are stuck at 62.5 fps | S | The single clear loss on the list scenarios, and a benchmark-app clock, not a framework limit. Fix it in the framework's animation driver (frame callbacks or presentation feedback), not in the app. |
| macOS presentation latency actually measured | S + a decision | Input-to-vsync has been inferred, not observed (O11). Needs an on-display, non-activating window or external equipment: a user decision (section 7). |
| Queue release policy with hysteresis (O09); deadline pacing re-run on a quiet machine (O10) | S | |

### 4.9 Memory and allocation

| avenue | size | notes |
|---|---|---|
| A full retained-state ledger for the new core (C12): capture lanes, `Vec` capacities, sites, tables, bitmaps, relations, layouts, glyph memos, framebuffers; RSS after scrolling the whole dataset | S | The 41.7 KB figure is page bytes of columns only. |
| A global allocator choice | done | Measured: turns allocate nothing and fault nothing; mount makes 61 allocations; under a bump allocator that never frees (the cheapest possible) mount is -5.9 % instructions and unchanged in cycles. So swapping the global allocator buys at most 6 % of mount and no time, on the new core. On the façade (A) this is unmeasured and worth one run, because A allocates in turns. |
| One reservation extended to hold the store tree's vectors, column table, reader lists and damage bitmap | M | Removes the 61 allocations and, more usefully, puts a turn's bookkeeping into the few cache lines of 4.8's interaction block. Predicted mount -6 %; kill: nothing measurable cold. |
| A bump arena per epoch, two of them flipped at publish, for what a turn or frame produces and nobody keeps (list plans, event chains, sweeps, frame staging, decode scratch); typed slabs with a free bitmap for the few long-lived things that come and go | M | bumpalo's shape with the lifetime tied to the epoch. Matters once bulk products exist (layout answers, shaped lines, operator results). |
| First-touch control: pack a small scene's first 16 KiB as one front page; let the first frame read the working half | S | Scene faults 39 to about 20. Small beside the frame buffer's 4,000. |
| Huge pages for the column region | S | Predicted no effect on any turn of the five scenes; not run. |
| `MADV_FREE` return after resize peaks; CPU surfaces as VM-owned mappings so allocator caches do not keep a peak resident (O09) | S | |
| Store capacities from use; the façade reserves 4 GiB of address space | S | |
| Glyph atlas residency by exposure; the memo sized by use | M | With 4.5's generational runs. |
| Resource pages as owned immutable frame inputs with pin counts **[review]** | M | Makes "no copy" for images and glyph tables an optimisation behind ownership instead of an external promise (R03). |

### 4.10 Threads and parallel work

Settled by measurement: nothing inside a turn is worth a thread (the largest turn is 16,000 cycles), with the
review's caveat that the sheet has no layout, shaping, filtering or decoding in its turns (P05). So parallelism
belongs to those: the shaper worker (4.5), collection operators over large inputs (4.2), decode (4.6), raster
bands (4.6), page pre-touch at start-up (4.7).

| avenue | size | notes |
|---|---|---|
| Workers designed around cache ownership: a band or a column range stays on one core across frames; work handed over as ranges of the published half, never as boxed jobs | M | Designs and unrun predictions (T1, T2, T4, T5, L3, L4, P1) in the threads lane's design note. |
| Scoped pool that survives a panic and refuses overlapping runs (K01) | S | A soundness item, first. |
| Bounded, prioritised work queue with cancellation honoured before start (A03) | S-M | The round-7 queue is unbounded FIFO. |
| Cache and branch misses as standing columns | S | The recipe exists (`hot.sh`, `lines.sh`, `alloc_x`), with seven flamegraphs and one table per host. Flamegraphs of cells of a few microseconds need the counter bracket switched off (93 % of hover's graph is the bracket). Branch misses are already low: about 3 per hover, 11 per page scroll. L1 misses: 6-10 hover, 317-326 theme, 390-463 scroll. |

### 4.11 Async

| avenue | size | notes |
|---|---|---|
| Restate holds and transitions on the new core: a hold as a plane over the state block, a landing as one publication | M | Round 7 has fourteen API programs on its own engine; programs 13 and 14 (motion, interruption) and host wake and timer wiring were open. |
| Generational, engine-bound question and action ids; owned registration guards (A02) | S-M | |
| Per-flight wake tokens (A05); pinning enforced by type (A04) | S | |
| Cost with many active questions, burst completions and cancellation storms (A06) | S | Only the idle path is O(1) by measurement. |

### 4.12 The author's API, composition and state

The API must not diverge; composition is open; state should read as plain Rust with no event watching.

| avenue | size | notes |
|---|---|---|
| Composition borrowed from Bevy, as a cold schema over hot typed data **[review S07]**: bundles, required components, typed queries over archetypes (the columns already are archetypes), plugins, resources | M-L | No runtime ECS per record and no observers. One sink-independent element type avoids the closure-identity problem. Measure shared against distinct component code size, nesting depth, overrides and branch variants. |
| State as plain fields mutated by handlers; derived values as expressions, not subscriptions | M | The new core already works this way (a handler writes a field, the turn compares the block). The façade's `sig!` maps onto it; the element layer is where this is settled. |
| Behaviour overridden at points the component's author declares, resolved at mount | M | Decided with the user on 2026-11-11 in the reflection session; a sketch exists (an action is a type, a route may return one); not built. |
| Owner-grain re-description needs a stated dependency contract and an invalidation escape (O06) | S | A description that reads a global, a `Cell` or the clock is not covered by a structural plan. |
| Dormant runs: the conditions under which a list falls back to eager mounting (non-indexable sources, some nesting, once mode), and the pure-description contract for skipped mapper calls (O07) | S-M | Becomes moot where the new core replaces it. |
| Demand that is not exposure **[review]**: find, selection ranges, offscreen accessibility queries, focused editors | M | "Nothing unexposed exists" needs a second, budgeted kind of demand for semantics without pixels. |
| Native views interleaved; multiple windows | L | Design only. |

### 4.13 Accessibility

Built: a projection of the typed walk (a 100,000-row sheet publishes under 2,000 nodes; theme, hover and glide
frames project nothing; an edit changes at most three nodes; a no-op is five compares); reduced motion as a time
offset plus an environment bit; unmaterialised rows described on demand.

| avenue | size | notes |
|---|---|---|
| The macOS adapter (NSAccessibility with lazy children), then AT-SPI | L | No native adapter exists, so nothing has been validated with a real screen reader (S05). This is the difference between a projection and accessibility. |
| Typed semantic components and overrides: role, label, selected, expanded, disabled, live policy, relationships | M | Inference from paint and handlers is a default, not a source of truth: it mistook hover comparisons for selection and made every state-driven text a live region before the fix (S02, part closed). |
| Node ids by item key and generation | M | With 4.3. |
| Accessibility in motion: what a screen reader is told during a transition; announcements for streaming text without flooding | M | Only the reduced-motion half is built. |
| Text selection and editing semantics; virtual focus and actions on offscreen rows | M | With 4.5. |
| A static lint over reflection: contrast, target size, missing labels, under alternate state blocks | S-M | The walk makes this cheap. |

### 4.14 Reflection, inspector and the developer loop

Built: reflection as a sink and a typed walk in the core, zero cost when unused (the same 621 symbols, every
steady cell identical), tied to its store's schema; source locations per row, scope and list from
`#[track_caller]`; operand provenance from type names.

| avenue | size | notes |
|---|---|---|
| The inspector port | L | The reference is the user's private gpui-ce inspector work (about 80,000 lines). Direction given: work against source locations; no JSON or Rust style editors; keep effort on the core. What is ported is the projection it needs, not its UI. The UI's form is a user decision. |
| Per-element source locations (today per row, scope, list); a stable schema instead of `type_name` text (S06) | S-M | |
| Validated mutation through reflection: theme operands and record-time reads | M | |
| The socket protocol for agents and tests: query the tree, inject turns, record and replay a turn log, diff pixels | M | Designed in the reflection session; the write log is most of the record format. |
| Three-line tests with typed selectors (by component type, by the signal a node is bound to, by role and text) | M | |
| Hot reload | L | Parked unbuilt at the round-6 checkpoint. |

### 4.15 Platforms

| avenue | size | notes |
|---|---|---|
| Fractional scale carried as a rational through layout, glyph selection, damage and input (H01) | M | At 125 % the buffer is 1.25x and layout is 2x today. |
| Keyboard: groups, AltGr, compose, repeat; focus loss clearing a held press (H03) | M | The new host reads two levels of group one by hand, on purpose (nothing `dlopen`ed). Options: the round-7 host's xkbcommon path or a pure-Rust xkb. |
| IME on Wayland (`text-input-v3`) and macOS | M | |
| Resize without remounting the world (H08) | S-M | Loses focus and press identity today. |
| Translucent surfaces and premultiplied operands in the span executor | S | |
| Windows; X11; web | L each | Not started. The new host shows the shape: a host is about a handshake, a buffer ring and an event translation. |
| The new core on macOS as an app (AppKit host over `bough-x`) | M | The new core's app binary is Linux-only; on the Mac it has run as tests and benchmarks. |

### 4.16 Animation

| avenue | size | notes |
|---|---|---|
| An animation benchmark at the level of Freya's: many independent springs and transitions, interruption, staggered lists, layout-affecting motion, with frame-time distributions | M | Not built. Only spinner and ticker cells exist, and they measure a clock. |
| Springs and interruptible transitions as expressions of `Time` with `until`, so a resting animation costs nothing | M | The mechanism holds: a frame with motion visits no record; cost is the mark. |
| Layout-affecting motion without re-solving: rigid moves and parametric answers (4.4) | M | |

### 4.17 Measurement

| avenue | size | notes |
|---|---|---|
| The new core through the runner, every scenario, with layout checks, on the dashboard as its own row with its admitted contract stated (B01, B10, B14) | M | In progress at close. |
| A matched-CPU configuration and a hardware-GPU configuration (B02) | M | Needs a host with a GPU. |
| Randomised paired blocks on a quiet host; builds stopped during timing (B03) | S | `mothership` is quiet and set up for counters as a non-root user. |
| Validity fail-closed; unknown distinct from valid (B06); runner exit status propagated (B09) | S | |
| Paired per-frame phase samples instead of marginal medians (B07) | S | |
| Estimates: hold-out validation, refuse outside the training regime, no winner counts (B04, B05) | S | The dashboard already separates them. |
| Counter scope stated per number: thread or process, user or kernel (B08); the perf-stat floor that leaves the same damage | S | |
| Workload families missing (B11): a large editable document; emoji, bidi and IME text; variable-height filtered lists; nested scrollers; distinct-image churn; many translucent layers; DPI changes; several windows; active async streams | L | |
| The five heavy scenes in all frameworks (two exist in four) | M | |
| Binary size and memory on equal terms (B12): the same app, same profile, symbols and assets separated | S | 1.07 MB against 55.5 MB is not a framework comparison. |

### 4.18 Compile time and binary size

| avenue | size | notes |
|---|---|---|
| Compile time and size per component as standing columns in the gate | S | Known points: layout as data costs +0.2 to 0.5 s and +3.5 to 17 KB per row type; monomorphised layout cost up to 204 s and 2.75 MB for one row type (refuted for that reason too). |
| Shared code for archetypes of equal layout; kernel code size per archetype | M | One kernel per archetype is the design; the count of archetypes in a real app is unmeasured. |
| A capability manifest per profile (O12) so a size reduction is not a silently removed fallback | S | |
| The reflection walk and accessibility projection kept out of binaries that do not call them, as a gate | S | Holds today by symbol count. |

### 4.19 Register of experiments written down and not run

Each has a prediction and the result that would kill it, stated before any code. X1-X12, X16, X18 and X19 are
the lanes' own written predictions, from their measurements. The others are thresholds proposed by the lead at
close, with less behind them: treat those as a first bid. Run each as written, or replace the prediction before
running; do not fit a prediction to a result.

| # | experiment | prediction | kill |
|---|---|---|---|
| X1 | One interaction block: everything a pointer event and a block write touch (shown block, hit list, hover set, per-word reader lists, scratch) in one allocation of about 16 lines, laid out in read order | cold hover data lines 26 to about 12 of 70; cold hover -10 % cycles | under 4 % |
| X2 | Page summaries: per page and per word read, a 64-bit mask of the record-constant classes present (a small Bloom mask for `Eq`-like expressions); the delta skips pages that cannot match the old or new value | record lines 12 to 4; cold hover -8 % cycles; warm hover -40 % instructions (550 a page today) | summaries cost over 2 % on scroll-a-page |
| X3 | Code footprint of a hover: sparse delta path as the common one; one outlined body for mask packing | 60 code lines to 45 | any warm cell dearer by 2 % |
| X4 | Prefetch at input arrival from the last event of the same kind | at most 10 % (cold misses already overlap: about 19 cycles per line) | do X1-X3 first; drop if under 3 % |
| X5 | Immutable frame versions: page-granular versions sharing pages, one atomic root (plan offset, epoch), pins by epoch slot, reclamation when unpinned; Loom or Shuttle on root and pins | a publish never waits for a frame; turn cost unchanged; frame paint +1 dependent line per column; memory bounded by (frames in flight + 1) copies of dirtied pages | turn cost up; unbounded retention under a stalled reader |
| X6 | Banded CPU kernels: bands of whole tile rows of equal summed weight, owned by a worker until a band is off by a quarter; damage under the CPU threshold shaded by the caller, never queued | at least 4.8x on six cores on the canvas (3.4x today); L2 misses per frame at least 20 % under a rotating assignment; no gain on sheet scroll beyond memory bandwidth | within 5 % of today's six-thread number (then the gap is bandwidth or serial staging) |
| X7 | First buffer `fallocate` + `MAP_POPULATE` (M1) | first raster 2.6 to at most 1.0 ms | under 20 % |
| X8 | Huge pages for the memfd (M2) | at most 0.5 ms | policy `never`, or no gain |
| X9 | Pages touched by other threads during connect and mount (M3) | at most 2.6 / cores + 0.1 ms | worse than X7 under load |
| X10 | Model built after the first write (M4) | table and search connect as early as rows | no change |
| X11 | Static linking (M5) | connect 0.3 ms earlier | under 0.1 ms |
| X12 | Pixel section from the page cache as the first `wl_buffer` | first buffer under 0.1 ms warm | compositor copies on attach |
| X13 | Pointer influence box per record reading the pointer words | under 3x on pointer-reading scenes, about 3 per record tested | no scene in the standing set reads the pointer per record: build the scene first |
| X14 | Tile prefix caching on canvas hover | hover cost follows the changed suffix, not the shadows below (16 of 22.5 ms a frame are shadows) | prefix storage and invalidation exceed the saving on theme and move cells |
| X15 | Shadows as nine-patch from an atlas keyed by (radius, blur, size class) | GPU full frame under 9 ms | error against the integer kernel beyond the accepted envelope, or atlas over 8 MB |
| X16 | Layout memo and extents wired into the list of the new core | chat resize 1.4 M against 181 M; sheet row hit 2,850, kept slice 200 | hit cost above a third of a miss on Fixed rows |
| X17 | Parametric layout answers (affine terms and breakpoints) on chat rows | most (row, 1 px) steps become an affine update (0.0 % were a range check under equality intervals) | under 30 % of steps |
| X18 | Shaper worker with stand-ins | about 303 per byte on the worker (grafted parley; 2,178 upstream), 27,000 per glyph first rasterised; interaction path never waits | a visible reflow on every Latin line (then widen the table's coverage first) |
| X19 | Scalable text for Frame-stage size: (a) two-tap scaled master, (b) SDF per glyph, (c) analytic coverage for very large glyphs | the type selects the archetype; cost under twice the glyph leaf | visible corner rounding at body sizes; cost above 2x |
| X20 | SIMD classification pass for breaks, newlines and token classes before a branch-light walk | break finding under the 7.8 per byte of shaping, so a break table pays | at or above 7.8 |
| X21 | Substring index for the filter operator (n-gram postings or a suffix array), with result deltas | a keystroke costs candidates plus output change, flat in dataset size for selective queries; index memory stated | low-selectivity queries slower than the scan, or index memory above the dataset's |
| X22 | Output-paced animation driver (frame callbacks or presentation feedback) | spinner and ticker at the output's rate at 120 and 240 Hz; unthrottled, bounded by frame cost | any idle wake-up when nothing animates |
| X23 | A global allocator swap on the façade (A) | unknown; A allocates in turns | under 3 % on every cell |
| X24 | Typed owner `World<V, B>` and schema-validated rows | wrong-view, wrong-backend and same-size row substitution become compile errors or checked refusals; hot path unchanged to the instruction | any per-record check on a valid path |
| X25 | Item key + generation in `Rel` with one remap at publication | press, insert, filter, sort, scroll, resize, release: the same item or an explicit cancel; cost follows exposed remapping and the edit log | hover or scroll cells dearer by 5 % |
| X26 | Randomised paired re-measurement on mothership, builds stopped | the unthrottled ordering holds; intervals stated | an ordering flips: report it |
| X27 | Hardware-GPU configuration for every framework | unknown; the canvas and sheet rankings are most likely to move | none: it is a missing measurement |
| X28 | Miri on `bough-expr` and `bough-x`'s safe API with the review's probes; Loom or Shuttle on publication | no undefined behaviour on the admitted API | any finding is a release blocker |

Lane predictions never run, kept under their own names in the threads design note: T1, T2, T4, T5 (workers),
L3, L4 (cold lines and prefetch: X1-X4 above), P1 (bands: X6).

## 5. Refuted: do not retry without a new idea

Each with the number that killed it; details in `docs/TALLY.md`.

* (page, slot) identity over a piece table: parity or worse; ordered walk 2.8x slower after churn.
* Monomorphised typed layout: 25,486 against 203 for a sheet row; 56,086 for chat; 204 s to compile.
* The element layer on typed layout for a view the size of the sheet: code generation unfinished after
  3 h 32 min (4 s on layout as data).
* Validity intervals on fraction-sized rows: 0.0 % hits.
* Per-paragraph break tables: 12 against 7.8 per byte. Digits by arithmetic: 5.2 % of numbers differ under kerning.
* Table-shaper coverage of real fonts: one of seven macOS faces.
* Shortening the dependent-load chain from five to two: 12,245 against 12,342 cycles cold.
* The scene image as a start-up lever: about 15 µs of a 41 µs mount.
* Threads inside a turn. Stage-2 projection across cores below 500,000 dirty records.
* The generic kernel as the default executor: 3x slower than the Metal presenter on canvas hover.
* Corner tables on the GPU: 16.8 to 18.9 ms against a 9 ms target. Per-corner tap leaves: no gain.
* LLVM auto-vectorising the blend; vector gradients; flat-row replication on layered scenes; tile-program
  dedupe; a four-row shadow integration (off by up to 21/255).
* A generic netting journal for deferral; stage 1 as a scan of every page.
* "A transition is a journal withheld" as stated.
* Structural derivative rules per expression (evaluate-and-compare replaced them); branded handles.
* List row identity by Rust type; events that cost nothing on hover (+685 before two fixes).
* A faster global allocator as a lever on the new core: at most 6 % of mount's instructions, no time.
* "Record pages are at least half of a cold hover's data lines": 18 of 93; half was event bookkeeping.
* A filter cache in the benchmark app (deleted: it removed work from the measured cell).
* `hotpath` for cells of a few hundred instructions (about 310 per measured call).
* A second benchmark host fed by shipping binaries over the terminal multiplexer (21 KB/s).
* Masking the record kind in the scroll-band test on the façade: fails two pixel gates.

## 6. Lessons that cost the most to learn

* Read one function's assembly before believing the compiler folds a general algorithm.
* A cost removed from one stage reappears in another unless the whole operation is the cell (direct wires moved
  cost to the frame; lazy GPU moved it to the first scroll; the image moves it to the first input).
* A gate whose inputs are all ASCII, all opaque, all uniform or all on whole pixels proves that subset.
* A fast path is a contract: state what it admits, and keep the adversarial case that forces the fallback.
* An unsafe invariant held by a comment is a defect waiting for a second caller.
* Under a fixed-rate compositor most interactive cells measure the compositor.
* Pattern-based process kills and shared target directories destroy other sessions' work.

## 7. Questions that are the user's to decide

1. **Relations and identity**: should `Rel` carry an item key and generation, so that focus, presses and
   accessibility nodes name items and not positions?
2. **macOS latency measurement**: is a small on-display, non-activating window acceptable for measuring
   presentation latency, or is external equipment the only allowed route?
3. **Accessibility**: NSAccessibility first, then AT-SPI? What may a screen reader's window policy be under the
   no-focus rule for tests? Are typed semantic components the default, with inference as the fallback?
4. **Text** (ten questions in `docs/r8/TEXT-API.md` section 5; the ones that block work): who owns editing state
   by default (`TextState` or `String`); where fonts come from (a bundled UI face with system fallback, all from
   the system, or all registered); how text should look (linear coverage is lighter than native macOS text;
   contrast tables; LCD anti-aliasing; hinting); whether static text is selectable by default; whether
   `TextState::text()` may return a chunked reference so storage can be a rope.
5. **Inspector**: what form the UI takes, given that style editors are out and source locations are in.
6. **Composition**: how far Bevy's vocabulary goes into the author's API (bundles and required components
   certainly; queries and plugins to be shown as programs first).
7. **Live-edit pins and pointer transparency** in reflection: left undecided in the reflection session.
8. **GPU portability**: vendored wgpu, or direct Vulkan and Direct3D beside Metal from the one shader source.

## 8. Where things are

* `main`: the façade (rounds 6 and 7 as integrated), the new core as merged from the expression lane, the tally
  and the round-8 design. `main-next` locally is the integration branch that is pushed to it.
* Heads at the close: `main` `cc65ee215f`; `r8/expr` `ded8ff4e25`; `r8/e2e` `68fd365e79`; `r8/layout`
  `fc830898f4`; `r8/surface` `dc6dbfb9e5`; `r8/raster` `a187c31709`; `r8/text-main` `5d7a6bc39a`; `r8/threads`
  `392d90a133` (unmerged).
* Round-8 lane branches: `r8/expr`, `r8/layout`, `r8/raster`, `r8/text`, `r8/threads`, `r8/surface`, `r8/e2e`.
  Round-7: `r7/*` (async is unbuilt work in progress at its head). Round-6 lane work not on `main`: `round6`
  and `lane/*/round6`.
* Designs: `docs/r8/ROUND8.md` (with seven amendments in the lead's copy), `docs/r8/SEAM.md`,
  `LAYOUT-SEAM.md`, `TEXT-API.md`, `TEXT-SEAM.md`, `ARENA.md`, `IMAGE.md`, `THREADS-DESIGNS.md`; per-lane
  journals under `docs/lanes/<lane>/JOURNAL.md`; the counters recipe in `docs/lanes/threads/MEASURE.md`.
* Benchmarks: zortax's gui-bench with a timing patch (`GB_TIMING=unthrottled` or `vrr:48-144`), scripts, raw
  results and the dashboard generator under `bench-r8/` in the lead's worktree.
* The independent review: the `independent-review` lane, report of 2026-11-12, with its evidence index and four
  probe files.

## 9. A suggested order

1. Gate 1 and gate 2 of section 3 on the integration branch.
2. The element layer, then every runner scenario and the five heavy scenes on the new core (gate 3).
3. Item identity and one publication transaction (4.3): it unblocks accessibility ids, focus, collection
   operators and frame versions at once.
4. The shaper worker and the first two orders of the text API.
5. Layout memo and extents wired in; parametric answers.
6. Canvas: tile prefix caching, then learned estimation.
7. Start-up M1, M2, M5; the output-paced animation driver; the animation benchmark.
8. The macOS accessibility adapter.
9. A quiet-host, matched-configuration re-measurement of everything (gate 4), then the dashboard.
