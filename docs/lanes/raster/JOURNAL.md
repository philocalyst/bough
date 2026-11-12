# raster lane, round 8: journal

Branch `r8/raster` (worktree `wt/r8-raster`), started 2026-11-12 from `r7/compute` `78635ba4c4`. Nothing is
pushed. Host for every number unless stated: M3 Pro (6P + 6E), macOS 15, shared with the other lanes; the
1-minute load average is recorded next to each number. Commands run at `nice -n 15`, `CARGO_BUILD_JOBS=2`.
Headless only (no window). ilo-prod is reserved; nothing here ran there.

The brief: ROUND8.md sections 4, 6 and 7. A paint is a type composed from typed primitives; an archetype
(paint type plus its expression types) gets one monomorphised CPU kernel and one generated GPU function from
the same expression code; `Frame`-stage expressions are evaluated while shading; associated constants fix
the plan at compile time; the scene's columns are the GPU's buffers, and culling, binning, damage and picking
run there. `crates/bough-kernel` (round 7) is the exactness oracle and the fallback archetype and is not
extended.

## 0. Why the round-7 kernel was stopped (so this lane does not rebuild it)

The generic kernel interprets 32-word commands with a kind field. Three consequences, each visible in its
own journal:

1. The compiler sees nothing of the UI. Every tile entry is decoded (`load_at`: 20 to 32 words) and then
   switched on its kind, per row on the CPU and per pixel thread on the GPU. A card that is always "shadow
   under rounded body under border" is four unrelated commands to it.
2. State reaches pixels through restaging. A hover is: re-apply records, stage commands (330 instructions
   per glyph), bin, raster. That is why the canvas hover cost 14.1 M instructions against the presenter's
   236 K: the work is proportional to the commands of the damaged area, not to the one word that changed.
3. Each groove (palette table, kernel transitions, scroll copy) was added as a mechanism of its own.

What is kept: integers from one source (`kernel.rs` leaf functions are the only place a pixel's arithmetic is
defined), the occlusion cut, and writing words into the memory the compositor reads.

## 1. Predictions and kill criteria (written before any kernel code existed)

Scenes: **canvas** (2560 x 1600 px; 20 clipped frames, 5,000 layered nodes: cards with one or two shadows,
gradients, bordered boxes, labels, clipped groups, 1,000 component instances whose fill reads hover and
theme; chrome with a 505-row layers list; pan moves content and clips) and **anim** (hundreds of rounded
boxes, each moving, resizing, rounding and recolouring on its own phase from one `Time` word).
Baselines are the generic kernel's executors on the *same staged pixels* (`cpu::raster` with the cut and flat
rows, one core and the pool; its Metal executor), plus the façade's workloads where they build.

| id | hypothesis | prediction | kill |
|---|---|---|---|
| R1 | per-archetype kernels are exact against the oracle | 0 differing words: archetype CPU kernel == generated GPU function == `cpu::reference` of the plainly staged scene, on both scenes, across hover, pan, theme and time samples; four mutants caught | any mismatch whose fix needs executor-specific arithmetic |
| R2 | generic composition cannot reach WGSL through wgsl-rs's own generics | a generic `impl<C: Expr> Expr for Sel<C>` is dropped or rejected by the macro (it sees tokens, not trait resolution) | if it works, use it and delete the const assembly |
| R3 | const-assembled source works: each expression and paint type carries its code as an associated const tree; the per-archetype GPU function is rendered from it | the rendered WGSL validates under naga for every archetype of both scenes; direct MSL rendering equals naga's translation of the WGSL on pixels | a combinator that cannot be expressed as (leaf name, children) |
| R4 | CPU, full canvas frame, one core: specialisation and the static cut inside a composed paint pay | >= 1.3x fewer instructions than `cpu::raster` on the same pixels | under 1.15x: per-archetype CPU kernels do not matter for full frames, only for interaction |
| R5 | GPU, full canvas frame: no 32-word decode per entry per pixel thread | GPU kernel time <= 0.8x the generic kernel's on the same pixels | slower than the generic kernel |
| R6 | a hover is a uniform write: no restage, no CPU work per record | GPU-resident path: <= 1,000 instructions of our own on the frame thread plus Metal's fixed cost (<= 200 K in all), GPU time <= 1 ms; CPU path >= 1.5x fewer instructions than restage + bin + `cpu::raster` over the same damage | more instructions than the generic path on either executor |
| R7 | animation is one word per frame | zero turns; frame-thread instructions on the GPU path grow < 10 % from 600 to 6,000 animating elements; GPU time <= 2 ms at 600 | frame-thread cost proportional to elements |
| R8 | the pick runs where the scene is | pick == CPU reference pick on 10,000 pointer positions; one word read back; pick pass <= 50 us of GPU time | a mismatch, or a pick that needs the CPU to walk records |
| R9 | damage can be computed on the GPU by evaluating each record under the old and the new uniform block | retained frames == full redraws over a 200-step random interaction script (hover, pan, theme, time) with the CPU sending only uniform words | a frame where retained != full |
| R10 | the plan is const data | `PLAN` is evaluated in a `const` item (asserted at compile time) and printed; a scene type with no GPU-class archetype never creates a Metal pipeline | a plan input that only exists at run time (other than area) |
| R11 | monomorphisation stays small | <= 150 KB of text for both scenes' kernels | over 500 KB |

## 2. What the type system can and cannot carry to the GPU

**R2 confirmed** (`crates/bough-paint/src/probe.rs`, held by a test). wgsl-rs transpiles tokens. A trait
with a static method, implemented for a concrete type and reached through a generic function with a
turbofish, is monomorphised into WGSL (`shade_Hover` calling `Hover_eval`). A generic impl
(`impl<C: Ex> Ex for Twice<C>`) compiles and runs as Rust and is silently absent from the WGSL: the call to
`Twice_eval` is emitted and the function is not. No proc macro can do better, because trait resolution
happens after macros run. So nested combinators do not reach the GPU through wgsl-rs's generics.

**What does work, and is built** (R3 confirmed):

* *Leaves are concrete.* `src/ops.rs` is one `#[wgsl]` module of pure integer functions over the round-7
  kernel's arithmetic (which I split, unchanged, into a module of its own, `bough_kernel::km`, so both
  crates import one definition). These are the only function bodies in the system.
* *A combinator is a leaf's name.* `op!(Eq = x_eq(a: A: u32, b: B: u32) -> u32)` emits a `repr(C)` struct of
  its operands whose `eval` calls `ops::x_eq` on the operands' values and whose `CODE` is
  `&Code::Call("x_eq", &[A::CODE, B::CODE])`: a static tree in rodata mirroring the type (a reference to an
  associated constant of a generic parameter is a promoted constant). `prim!` does the same for a primitive
  from its stager leaf and its shade leaf. Neither face has a body of its own, so CPU and GPU cannot
  disagree about a combinator.
* *The GPU function is the tree rendered.* `src/emit.rs` walks `Paint::CODE` and writes six pure functions
  per archetype (`shade`, `texel`, `win`, `cover`, `chg`, `pick`) of (rect, record words, block words,
  pixel). Rendered expressions are calls on parameter names and `123u` literals: valid WGSL and valid MSL
  unchanged; only declarations differ. The leaves go through wgsl-rs and naga once, in this crate's
  `build.rs`. At run time the application's part of the shader is text assembled from constants; naga is
  not linked.
* Gates: the WGSL of every archetype validates under naga; every leaf a tree names exists under that name
  in naga's MSL (it renames identifiers that end in a digit, so leaves called from generated code do not).

**What the type system carries:** the tree of an expression and of a paint; the record layout (the value of
the type is the record: zero-sized block reads and constants, one word per `Rec`, depth-first order); the
stage; the block words read (`READS`) and those that move a window (`GEOM_READS`); whether a colour is
provably opaque (`OPAQUE`: a constant with alpha 255, `Pmo`, a select or mix of opaque operands) and so
whether a paint covers (`OPAQUE_CORE`) and where the cut between a record's own layers is (`Over` with an
opaque top hides its core from the layers beneath: decided when the archetype is compiled, no alpha is
inspected at run time); the cost class; the kind of every primitive call (a paint drives a `Sink` by
continuation, and each sink method is one kind, so no kernel tests a kind).

**What it cannot carry:**

* A Rust closure. It has no tree, so it is `Turn` stage: evaluated on the CPU into a record word
  (`bough_expr::Map`, `Code::Cpu`). Only named leaves are `Frame`. `emit.rs` refuses `Code::Cpu` and
  `Code::UAt` in a paint operand for now (the store must materialise the first; the second needs the block as
  an array parameter).
* The whole application's archetype list, under description by continuation: no type mirrors the view tree
  (SEAM 6.2). A store's row type gives a constant list; the whole-application half of the plan is folded at
  start-up by the same `const fn`.
* SPIR-V. Text can be assembled from constants; a binary module cannot. Vulkan needs naga at start-up or an
  application build step. Not built.
* Sharing an operand between layers of a composed paint except through the rect lanes: `Over<Shadow<B, ..>,
  RRect<B, ..>>` holds `B`'s record words twice (the canvas's animated node holds its phase and period three
  times). A `Let` combinator, or operands hoisted into a paint struct by a macro, would fix it. Not built.

## 3. The first back-to-back table, and where the GPU's time goes (2026-11-12, second session)

(The lane changed hands here. Sections 0 to 2 are the first session's; the code it left ran ahead of this
journal: `emit.rs`, `metal.rs`, `plan.rs`, `scenes.rs`, `driver.rs` and `pbench` existed and their gates were
green. `r8/expr` `0e0ec02e9e` is merged.)

**Against the façade on the same window** (M3 Pro, `dist`, one command, load average 12 to 20: busy; canvas at
2560 x 1600 px; the façade's scene is `comparisons/bough-workloads` from `r7/bench` `56e6a6a229` built in this
worktree, 10,856 records; this lane's is the re-description in `scenes.rs`, 19,476 records because every
glyph is a record; wall medians, input to pixels, GPU waited for):

| step | façade, Metal presenter (`BOUGH_REPAIR=gpu`) | façade, round-7 kernel (`=kernel`) | this lane, GPU resident (AG) |
|---|---|---|---|
| hover (a component's 200 instances) | 12.5 ms, 262 K instr | 36.2 ms, 15.2 M instr | 23 to 30 ms, 76 K instr |
| theme (everything) | 19.2 ms, 565 K | 55.1 ms, 18.1 M | 23 to 34 ms, 70 K |
| first frame | 60 ms (cold pipelines) | 190 ms | 31 to 37 ms |

**The gate fails as the code stood: the resident path is 1.5 to 2 times slower than the Metal presenter on
hover and theme**, with 4 to 8 times fewer frame-thread instructions. (The 5.5 ms the round-7 compute lane
quoted for a canvas frame was its own smaller kernel scene, not this window.)

**Where the 24 ms of a full frame goes** (`examples/gprobe.rs`: the generated source with parts removed, same
frame, GPU time, median of 12):

| variant | GPU ms |
|---|---|
| as generated | 24.3 to 27.5 |
| the per-pixel cut removed | 32.5 |
| shadows shaded as solids (staging and everything else live) | 6.7 |
| every layer shaded as a solid | 5.9 |
| the entry loop, window test and record load, no shade | 3.1 |
| no entry loop (window passes, bin, compact, store) | 2.0 to 2.7 |

So: the loop over a tile's entries, which a hardware rasteriser would remove, is under 1 ms; staging in every
pixel thread is not the cost either; **18 of 24 ms is the shadow's corner integral** (`km::corner3`: a
four-point quadrature with an integer square root per node, four corners, at every pixel within 1.5 blur
widths of a corner, which for the canvas's blurs of up to 40 px is most of every shadow).

Two things measured on the way: Metal keeps its own cache of compiled libraries keyed by source: the canvas's
639 ms compile is 5 to 17 ms in every later process (a new variant: 400 to 1,100 ms). And the hover's 1,661
tiles are real: the component's 200 instances with their shadows' windows, not every record of the columns
that read `Pick` (the window pass already damages only records whose operand values differ).

### R12 (written before the code): corner tables

`corner3` is a function of the pixel's offset from the corner and of (radius, blur width) only: not of the
box's size or position. For a box whose edges are on whole pixels the offsets of pixel centres are the same
for every box, so the correction is one table per distinct (radius, blur), filled once by the leaf itself and
read by both executors: exact against the oracle by construction (a table entry *is* the closed form at
that offset; a box off the pixel grid, or a table not yet filled, takes the closed form). The table's key is
stored in its header and checked while shading, so a stale handle can only cost time. This is the glyph
atlas's idea applied to a second resident: a content-addressed memo of a pure coverage function.

| | prediction | kill |
|---|---|---|
| canvas full frame, GPU | <= 9 ms | > 15 ms |
| canvas hover (1,661 tiles), GPU | <= 5 ms | > 10 ms (the presenter's step is 12.5 ms wall) |
| exactness | 0 differing words on every gate, boxes off the pixel grid included; a mutant that adds one to a table entry is caught | any mismatch |
| table memory, canvas | <= 2 MB | > 8 MB |
