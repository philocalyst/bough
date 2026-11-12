# Round 8 (2026-11-12): one abstraction, decided by the compiler

All paths are under `/Users/philocalyst/Downloads/designs/reactivity`. Round 7's lanes were stopped by the user;
their work is committed on `r7/*` (WIP commits where marked) and their journals are in
`wt/r7-<lane>/docs/lanes/<lane>/JOURNAL.md`. The rules of `bough/runtime/lead/ROUND7.md` (no pushing, no
synthetic input, no focus-taking windows, rex-only access to ilo-prod, stop only your own processes, the Mac is
overloaded: `nice -n 15`, `CARGO_BUILD_JOBS=2`, one cargo per lane) still bind.

## What the user said (binding, in their words where it matters)

- "I can feel that you're overfitting again, and not really tackling things in a fundamental way."
- "there are some issues around dispatch that you're falling victim to, as well as pretty weak forms of
  parallelism."
- "This idea of compute cores is very weak ... it makes things too abstract for Rust's type system to really
  tackle it ... the goal is that Rust's type system can determine the sort of compute structure which makes the
  most sense based on the UI it's compiling, GPU/CPU split, etc."
- "these compute special case reductions are always going to be poorly scaled, when really we should be
  improving the fundamental algos around damage tracking, and especially around incremental computation."
- "if highlighting and hovering and all of these things need dedicated effort, it just shows me how weak we're
  at right now."
- Use far more of Rust: closures with higher-ranked lifetimes and continuation-passing style
  (https://stackoverflow.com/questions/71855149/lifetime-and-closures-and-continuation-passing-style), `unsafe`
  where it lets bits pack for instant access; vendoring exists "to avoid dispatch and leverage Rust typing".
- Threading "designed in a way to maximize cache hits (which we need to start measuring, along with branch
  predictions, especially for the hot paths), and also to avoid stuttering on large scenes."
- "bbtree's insights still haven't been used appropriately." Blender (operations kept on the GPU) and Xilem's
  internals are references. SDF and GPU compute for interaction.
- Order-of-magnitude gains in first paint and interaction; animation as a benchmark at the level Freya reaches;
  the best default accessibility, "even in motion and complex situations"; finish porting the inspector; compile
  time must not balloon with big UI graphs, binaries small, monomorphisation maximal.
- Start with up-to-date benchmarks (other frameworks extrapolated as always); remotes cleaned first.

## What was wrong with round 7, in the code

1. **Dispatch.** A wire is `unsafe fn(&mut Rt, &Wire, *const u8, *const u8)` plus a 16-byte recipe, reached
   through a linked chain from the signal's head (`rt.rs`: `PatchFn`, `add_wire`, `run_head`). A write that
   fans out to n records is n chained loads and n indirect calls on erased pointers. Rows mount through a
   transmuted `fn`. The raster kernel is an interpreter of 32-word commands with a kind field. The compiler
   sees none of the UI's structure at any of these points, so it can specialise nothing.
2. **Special cases.** Palette tables, `is(key)`, kernel transitions, scroll copy, rigid moves, the shadow
   nine-patch: each was one instance of the same missing thing, built separately.
3. **Parallelism.** A worker pool over tiles, added after the fact, with no ownership of data by cores.
4. **Measurement.** Instruction counts on micro cells. No flamegraphs, almost no cache or branch-miss numbers
   on the paths that matter.

## The abstraction

**A UI is a dataflow from state to pixels whose shape is known when it is compiled. Every value in it is a
typed expression; its type decides when it is evaluated, where, and what its change damages.**

### 1. Expressions are types
`sig!(self.hover).eq(row.id).select(HI, LO)` is a zero-sized type `Select<Eq<Sig<..>, Rec<Id>>, K, K>` plus a
small POD of per-record constants. One trait:

```rust
trait Expr {
    type Out; type Consts: Pod;
    const STAGE: Stage;                       // inferred from the inputs' types
    fn eval(inp: &Inputs, c: &Self::Consts) -> Self::Out;       // also emitted as WGSL by wgsl-rs
    fn delta(old: &Inputs, new: &Inputs, cols: &Cols<Self::Consts>) -> Affected;  // the derivative
}
```
An opaque closure is still an expression (`Map<F, ..>`); it just has the weakest derivative ("all dependents").

### 2. The stage comes from the type
- `Const`: no inputs. Baked into the template at compile time.
- `Mount`: per-record constants only. Evaluated once, written to a column.
- `Turn`: reads state and feeds geometry. Evaluated on the CPU when the state changes, then incremental layout.
- `Frame`: reads state but only feeds paint (colour, opacity, transform, clip, time). **Not evaluated on the
  CPU at all**: the state it reads is a word in a uniform block, the per-record constants are a column the
  rasteriser already has, and the kernel evaluates the expression while shading. A write is one store to the
  uniform block plus the damage from `delta`.

Hover, selection, theme, focus rings, scroll offset, pointer position, time (every animation and transition),
reduced motion, zoom: all `Frame`. None needs dedicated machinery; none costs per-record CPU work; fan-out to
100,000 dependents is free on the CPU by construction. Round 7's palette, `is(key)`, kernel transitions and
scroll copy are four instances of this rule and are deleted as mechanisms.

### 3. Damage is the derivative
`delta` is structural: `Eq(sig, rec)` affects records whose constant is the old or the new value (an index on
the column, or arithmetic when the constant is the row number); `Table[sig][rec]` affects users of entries
that differ; an expression over `Time` affects the records in its active set, with swept bounds; `Map<F>`
affects every dependent. Lists are relations: `each(items.map(row))` is an incremental map whose deltas are
inserts, removes and moves (rigid moves on screen); filter and sort are incremental operators. Layout is an
incremental function too: its derivative with respect to available size is the validity interval. One notion
of change, from state to tiles, and exposure is the same notion read the other way (what has never been asked
for has not been computed).

### 4. Archetypes: dispatch once per kind, never per record
Records created by the same view call site share an archetype: (paint type, tuple of expression types). An
archetype is a set of SoA columns in 64-record pages with bit-planes, one monomorphised CPU kernel and one
generated WGSL function, both from the same expression code, so CPU and GPU agree by construction. A frame is
a static list of archetype passes. There are no wires, no function pointers, no recipes, no command
interpreter on the hot path. A signal's dependents are a `&'static [Dep]` in rodata, not a heap chain. The
generic 32-word kernel remains as the fallback archetype and the exactness oracle.

### 5. Description by continuation
`view` does not return a tree. It drives a sink: `fn view<'a, S: Sink<'a, Self>>(&'a self, s: S) -> S::Done`,
children as `for<'r>` closures, state borrowed straight through with no boxing and no `'static`. The sink at
mount writes columns; in re-describing mode the same code compares operand bytes against the columns as it
goes (the description never exists as a value); for accessibility and the inspector it is a third sink.
Branded lifetimes tie handles to their tree so indexing needs no check. Each component is a monomorphisation
firewall: its body compiles once to (static template, column writer, update function, `&'static Descriptor`);
parents hold the descriptor for cold paths and call generically only where it is hot. That keeps types flat,
compile time linear in components, and cold code shared.

### 6. The compute structure is chosen at compile time
Each paint type carries associated constants (opaque, per-pixel cost class, needs neighbourhood such as blur).
A `const fn` over the app's archetype list fixes the plan: which archetypes are CPU spans, which are GPU SDF
passes, which layers are GPU-resident, what the uniform block is. Only area-dependent thresholds stay dynamic.

### 7. The scene lives where it is drawn (Blender's lesson)
Columns are the GPU's buffers (same pages on unified memory). Culling, binning, `Frame`-stage evaluation and
picking run there; the pick is an id per tile, so hover is one word read back, not a tree walk. The CPU sends
deltas: uniform words and dirty column ranges.

### 8. The memory hierarchy is the cost model (bbtree, applied this time)
Cost order: chained cache lines, then instructions, then coherence traffic. Budget per interaction: state write
to damaged tiles in at most two chained lines. Fixed power-of-two pages, inline keys, batch edits sorted by
page, no pointer chains. `bbtree-mmap`'s lesson for first paint: templates are rodata and the mounted scene
(columns, layout answers by shape, shaped text) is a position-independent image that can be mapped from the
last run, so start-up is map plus raster of what is exposed. Measured on ilo-prod with perf: L1/L2 misses,
branch misses and flamegraphs for every hot path, before and after.

### 9. Threads own pages
Each page has an owning worker, so a core's cache holds its own pages frame after frame; passes are archetype
column loops chunked by page; nothing is shared for writing. Readers (raster, accessibility, inspector) read
pages optimistically by version, as bbtree's readers do, and never block the writer. The interaction path
(a uniform write, a small `delta`, a front-of-queue repair) never waits for bulk work: materialising exposure,
a big relayout or a decode run in slices on workers and land as an epoch. That is what removes stutter on
large scenes.

### 10. Animation, accessibility, inspector fall out
Animation: expressions over the `Time` uniform (curves, springs, keyframes as types); zero turns; benchmarked
against Freya. Accessibility: one more projection of the same archetypes through their descriptors, with
deltas from the same `delta`; in motion the tree reports settled targets, and reduced motion is a uniform that
collapses `Time` expressions to their end state. Inspector (port from `~/projects/gpui-ce*`): a walk of
descriptors plus live edits of the uniform block and tables; theming is exactly that.

## How round 8 runs

Phase 0 (now): remotes cleaned (Haiku), then the up-to-date benchmark run on ilo-prod (Sonnet): the zortax
matrix with bough, the heavy scenes in four frameworks, an animation scenario, with perf counters and
flamegraphs for bough's hot paths. ilo-prod is reserved for that until the lead says otherwise.

Phase 1: the abstraction proved in its smallest real form, a new crate `crates/bough-x` (no dependency on the
façade's `rt`), carrying three scenes end to end: the 100,000-cell sheet (hover, selection, theme, mount by
exposure), the layered canvas (shadows, hover, pan), and an animation scene. Predictions and kill criteria are
written first. Numbers: instructions, cycles, L1/L2 and branch misses, wall, RSS, binary size, compile time.

Phase 2: the façade moves onto it; what it replaces is deleted.

Lanes, each owning a seam of `bough-x` and attacking the others' assumptions:
- **expr** (Fable): sections 1-5 and 8. Expression types, stages, derivatives, archetype columns, the sink.
- **raster** (Fable): sections 4, 6, 7. Per-archetype kernels from expression types through wgsl-rs, the
  compile-time plan, the GPU-resident scene, picking, SDF passes for the costly paints.
- **threads** (Fable): sections 8-9. Page ownership, optimistic readers, the interaction path, sliced bulk
  work, the mapped scene image; owns the cache and branch-miss methodology.
- **layout** (Opus, continues `r7/layout`): the shape memo, extents of unmaterialised runs, validity intervals.
- **async** (Opus, continues `r7/async`): the API programs; holds and transitions restated as expressions.
- **surface** (Opus): accessibility projection, the inspector port, theming access, media (continues
  `r7/memory`'s tile pool design).
- **bench** (Sonnet): the runs, the dashboard, flamegraphs, the animation benchmark, other frameworks.
The integrator's `round6` (surface-api merged, unverified at 281aa9dfc0) stays the base for the façade.

## Amendment 1 (2026-11-12): state after the first day, and the reformulations still missing

State. `crates/bough-expr` (shared, no deps) and `crates/bough-x` are on `r8/expr`; `crates/bough-paint` on
`r8/raster`. The seam is `wt/r8-expr/docs/r8/SEAM.md` (v1, both lanes agreed). Measured on the sheet below
pixels (Mac, instructions, same at 100 / 10,000 / 1,000,000 rows): idle 73, hover 7,173, theme 30,755, exposed
edit 1,566; 764 records exist at any size. Per-archetype CPU kernels are word-exact against the round-7 oracle.
Corrections to this document, measured by the lanes: the derivative is eval(old) != eval(new) over exposed
pages (exact for opaque closures, vectorised by the compiler), not a structural method; Frame state costs about
9 instructions per exposed record per change on the CPU; a Rust closure is CPU stage, only named leaf functions
shade; state to page is four chained lines today (budget two); no handles exist, so no branding; the whole-app
plan is assembled at start-up from the first mount, per-store paint lists are const.

The user, same day: lanes should "share more related objectives"; "still some huge reformulations you're
missing"; the benchmark results must go into the graphs.

Shared objective for every lane: **the five heavy scenes (sheet, ide, dash, canvas, chat) plus an animation
scene running end to end on the new core, with real layout, real text and real pixels, beating the façade and
the other frameworks by an order of magnitude on first paint and interaction, measured on ilo-prod with cache
and branch misses.** A lane's result counts when it moves that, not a cell of its own.

Reformulations not yet tried (the lead's list; attack and extend it):
1. **Layout is staged like everything else.** cephal interprets a style tree at run time: dispatch again, 84,060
   flex runs for the sheet. A row shape is a type; its layout is a monomorphised function of the available
   space and the leaf sizes, mostly constant-folded (a row of fixed and flex children is arithmetic). Stage it:
   `Const` layouts are baked into the template at compile time, `Mount` ones run once per shape, only `Turn`
   dimensions are solved at run time, and the solver is a fallback for what types cannot express, not the path.
2. **Text is staged.** A constant string is shaped at build time into a glyph-instance column in rodata; a
   numeric cell is formatted and placed while shading (fixed advances); only free text is shaped at run time,
   off the interaction path, and lands as a paint write.
3. **The first frame is build-time data.** The first mount is a pure function of the initial state. Run it in
   a build step: the scene image (columns, shapes, rect slices, glyph columns, the plan, the generated GPU
   source) is mapped at start, and optionally the first frame's tiles too. First paint is map plus present.
4. **Late latching.** A frame is a pure function of (64-word block, columns). The block can be re-sampled at
   the last moment before the compositor latches, on the raster side, with no turn: the pointer, time and
   scroll words reach pixels up to a frame sooner than any turn-based framework can manage.
5. **Events are the dataflow run backwards.** The pick is an id per tile written by the rasteriser; an id names
   (store, row, position), and the handler is a monomorphised function in the store's descriptor. Hit testing,
   hover and dispatch need no tree and no boxed closure.
6. **Speculation.** Because Frame state is small and frames are pure, the likely next states (the next scroll
   strip, the neighbouring hover) can be rastered into spare surface tiles while idle and presented by swap.
7. **Relations all the way.** Sort, filter, group and join over collections as incremental operators with
   delta in, delta out (the search and table scenarios), so a filter keystroke costs the changed rows.
8. **Accessibility and the inspector are sinks** of the same `view`, and motion is handled by the block:
   reduced motion collapses `Time` expressions; the accessibility tree reports settled targets.

## Amendment 2 (2026-11-12, from the user; binding)

- **The consumer API should not significantly diverge.** "All of this perf work should be relegated to the
  core." The author keeps writing bough as the façade has it today (`View::view`, elements and modifiers,
  `sig!`, plain state structs, handlers as methods and closures); the new core sits underneath and accepts that
  description. `crates/bough-x/scenes/sheet.rs` in its current form (slot-numbered lenses, hand-placed rects,
  tuple paths, const-generic turbofish) is a core test fixture, not an API. An element of the façade is a typed
  value that drives the sink; nothing the author writes should mention stages, slots, columns or words.
- **State management should be cleaner and feel like plain Rust.** The user does not like event watching: no
  subscriptions, no observers, no `on_change` callbacks, no effect registration. State is fields; handlers
  mutate them; the framework notices by comparing (the block compare for Frame state, journals inside
  collection types, a description re-run for the rest). Derived values are expressions or plain methods.
- **Composability is still open, and should borrow a lot from Bevy.** Read Bevy's ECS as a composition model,
  not as a runtime: bundles and required components (an element is a set of components; a foreign component is
  extended or overridden by adding or replacing one), queries (a typed walk over archetype columns is exactly
  what the inspector, tests, theming and accessibility need: one mechanism, statically typed, no reflection
  tables), plugins (a unit that contributes components, paints, handlers and resources), resources (app-wide
  state such as the theme), relationships. Our stores already are archetype tables; say which of Bevy's ideas
  fall out for free, which need design, and which (observers, event readers) are excluded by the previous point.

## Amendment 3 (2026-11-12, from the user; binding)

- **Use the Linux remotes for compute, as well as this Mac.** "PLEASE do the benchmarks on the LINUX machines
  that we have REMOTES on with REX, and put compute there while also loading this machine." ilo-prod is no
  longer reserved. Every lane builds, tests, fuzzes and measures its platform-independent code there, in
  `/root/work/r8/<lane>` with its own `CARGO_TARGET_DIR`, through rex only, with every command prefixed
  `taskset -c 8-15 nice -n 19` and at most `-j4`, so that CPUs 0-7 stay free for the benchmark runner's timing
  runs (which record the load and the harness's validity checks as before). Source goes over as a git bundle
  (recipe in ROUND7.md, "Remote recipe"); follow `/root/work/r7/bench/env.sh` for the toolchain. Close your rex
  blocks; stop only your own processes.
- **mothership** (second remote, `rex -S mothership`, session `bough-probe`): Xeon E-2236, 12 threads, idle, 31 GB
  of memory, but its disk is full (about 1 GB free). Do not build there and do not write more than a few MB.
  It may be used to run small prebuilt x86-64 binaries copied over (counters, model benches, fuzzers that keep
  no corpus on disk); it is an Intel CPU, so last-level-cache events exist there, which ilo-prod's Zen 2 lacks.
- **The inspector is not a UI project now.** The user: no JSON styles editor and no Rust styles editor ("a
  waste of time"); working against source locations is welcome; "if more reflection was moved into the core it
  would become trivial"; "we need to keep focusing on the core". The fork with the large inspector work is a
  private worktree the lead has not located; do not port from `gpui-component`'s inspector.

## Amendment 4 (2026-11-12)

- **mothership now has about 28 GB free** (the lead cleared cargo target directories and gradle caches there at
  the user's request; log in `/home/miles/build-cache-cleanup-20261112.txt`). It is an idle 12-thread Xeon
  E-2236 with 31 GB of memory, reached with `rex -S mothership`, session `bough-probe`, as user `miles` (not
  root). Lanes may build and run small, platform-independent work there under `/home/miles/work/r8/<lane>`
  (create it; keep each lane under 4 GB and delete your target directory when done). It is quiet, so it is the
  preferred host for counters and micro-timing; being Intel it has last-level-cache events. A helper that runs a
  local script there: `<scratchpad>/mrx.sh SCRIPT [tries]` next to `rxf.sh`.
- **The inspector work to learn from** is at `~/projects/Private-Forks/gpui-ce-inspector` (branch
  `preserve/wip-2026-10-30`, commit eaa93a2; read-only). It is a reference for what reflection the core must
  offer, not a UI to port now.

## Amendment 5 (2026-11-12, from the user, looking at the dashboard; binding)

"I immediately see a lot of the problems I'm trying to highlight, around startup, hover and selection, typing
latency, fine update, etc. We should be getting order of magnitude level improvements in all of these cases,
no compromising on it ... will require some really really exotic approaches to not regress (or even improve
our other wins) ... reformulate."

**The scoreboard is the zortax matrix** (gui-bench under fakeland on ilo-prod), and what is on it today is the
round-7 façade. Nothing done on the new core counts until it is in that binary. The measured state (cmp-2,
60 Hz, bough valid reports) and the targets:

| cell | today | of which the app | target |
|---|---|---|---|
| rows 3,000 startup to first present | 65.7 ms | ~55 ms | app share <= 5 ms |
| virtual 100,000 startup to first present | 564.9 ms (7th of 11) | ~555 ms | same as rows: <= 5 ms of app time |
| rows hover, input to commit | 0.12 ms (100 rows), 0.93 ms (3,000) | all | <= 0.05 ms at any size |
| table select / update10 / swap, input to commit | 0.12 / 0.44 / 0.59 ms | all | <= 0.05 ms |
| table fine remove, input to commit; CPU per frame | 4.22 ms; 4.18 ms | all | <= 0.3 ms |
| typing, handling; CPU per frame | 0.43 ms; 0.47 ms | all | <= 0.05 ms |
| scroll 3,000, CPU per frame | 5.74 ms | all | <= 0.5 ms |
| resize, configure to present | 17.2 ms (iced 14.4) | most | at the compositor's floor |
| idle RSS | 35.1 MiB | all | <= 20 MiB |

What the totals are made of, from the same data: hover total 11.52 ms = 0.93 input-to-commit + 9.58 waiting
for the compositor's latch + 1.00 latch-to-present; with input-to-commit at 0.12 ms the wait is 10.42 and the
total 11.54. Under this harness (fixed 60 Hz grid, latch 1 ms before vblank, input at a seeded phase) the wait
is the same for every framework, so on the *total* columns bough is at the floor and cannot move further; the
order of magnitude has to be shown where the app has a share: input-to-commit, CPU per frame, startup, memory,
and the totals at high refresh and variable refresh, where the floor is the app. The lead has told the user
this plainly. Anything that would beat the latch itself must be real (a later sample of state, never a faked
timestamp or a benchmark-specific path).

Startup is where the largest multiple is, and it needs the whole chain reformulated, not one step: process
start (static linking, no relocations to speak of, nothing run before main), no font discovery or parsing on
the path to the first frame, the scene mapped rather than mounted (reformulation 3), only exposure
materialised, the first frame drawn by the CPU kernel straight into the first wl_shm buffer, the first commit
sent in the same flush as the configure acknowledgement, nothing waiting on a frame callback, and GPU bring-up
never on that path.
