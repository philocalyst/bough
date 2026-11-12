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
