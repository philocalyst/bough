# Running tally: what has been tried, what it showed, what is still to try

Kept current as work lands. Each line says where the evidence is. "Refuted" entries are results; do not retry
them without a new idea. Numbers are instructions unless a unit is given; hosts are an M3 Pro Mac, `ilo-prod`
(Ryzen 7 3700X, Linux) and `mothership` (Xeon E-2236, Linux). The public scoreboard is zortax's gui-bench under
the fakeland compositor (`bench-r8/`); the dashboard is built from it.

## Where the scoreboard stands (round-7 façade binary, 60 Hz, 2026-11-12)

| cell | bough | best other | target |
|---|---|---|---|
| rows 3,000 startup to first present | 65.7 ms | iced/egui 165.6 | app share <= 5 ms |
| virtual 100,000 startup | 564.9 ms (7th of 11) | vizia 214.9 | same as rows |
| sheet 10,000 startup / idle RSS | 712.7 ms / 332.6 MiB | egui 246 ms / 157 MiB | exposure-proportional |
| sheet 10,000 scroll, resize | failed (connection reset) | egui 22 ms resize | pass, at the floor |
| rows hover, input to commit | 0.12-0.93 ms | iced 3.7 | <= 0.05 ms at any size |
| table fine remove, CPU per frame | 4.18 ms | freya 25.7 | <= 0.3 ms |
| typing handling | 0.43 ms | - | <= 0.05 ms |
| canvas 5,000 hover | 925.8 ms (software, one thread) | iced 85 | under the others |
| idle CPU / idle RSS (rows) | 0.03 ms/s / 35 MiB | freya 14.7 / iced 169 | keep; RSS <= 20 MiB |

Under a fixed 60 Hz compositor about 10 ms of every framework's hover total is the wait for the latch; the
matrix is being re-run with the compositor unthrottled so totals show the app.

## Kept (measured, in the tree or on a lane branch)

- **Round 6** (`docs/CHECKPOINT-2026-11-12.md`): write log; gapped directory; state planes; per-id columns with
  one index space; retained labels; overflow-rect cut in layout; owner grain and direct wires; exposure-driven
  mounting (dormant runs); row certificates in the presenter.
- **Round 7**: guarded turn (idle 188 -> 95); record-space paint damage; derived clips recomputed in rank order
  (fixed four stale-clip defects); transitions owned by their scope; rigid moves (a displaced run of unchanged
  siblings is a copy: 35 tiles instead of 200); commuting wires in segments (hover and theme 2-3x on five heavy
  scenes); one integer raster kernel written once through wgsl-rs, bit-identical on CPU, Metal and x86, with an
  occlusion cut (298 -> 18 entries per tile on layered scenes) and a palette table; bough on Linux/Wayland with
  pure-Rust text; async API as fourteen programs with holds as a plane over cells; lazy GPU device (first frame
  64-89 ms -> 10-15 ms, footprint roughly halved); five heavy scenes and a counters recipe.
- **Round 8, the new core** (`crates/bough-expr`, `crates/bough-x`, `crates/bough-paint`; design in
  `docs/r8/`): values are typed expressions whose stage (compile time, mount, turn, or while shading) is
  inferred; paint-only state is a 64-word block compared once per turn, no write barrier; damage is
  eval(old) != eval(new) over exposed pages, exact for opaque closures and vectorised by the compiler; one
  column per view call site, no per-record dispatch; nothing unexposed exists (764 records for a sheet of any
  size). Sheet, any size: idle 53, hover 6,909, select 694, exposed edit 1,433, theme 31,010; a still frame 6;
  an animating frame visits no record. Reflection as a typed walk costing nothing when unused. Events as pick
  plus monomorphised handlers with captured closures. Typed flex layout equal to the solver on 4,000 random
  trees. Staged text (chat first paint 94.6 K against 1.37 M). Per-archetype CPU kernels word-exact against the
  oracle; a GPU-resident path. Publishing through a working and a published half (no torn frames, the turn
  never waits).

## Refuted or killed (with the number that killed it)

- (page, slot) identity with a piece-table directory: parity or worse on edits, ordered walk 2.8x slower after
  churn.
- Shape memo for layout: a monomorphised `place` is cheaper than a key compare (claim under measurement).
- Per-paragraph break tables: cost more to build than shaping (12 against 7.8 per byte).
- "Arithmetic digits equal the shaper": Arial kerns "11"; 5.2 % of numbers differ. Tabular digits are a definition.
- Shortening the pointer chain from five dependent loads to two: no measurable difference with caches cold.
- Mapped scene image as a start-up lever on the new core: saves about 15 microseconds; mount is 41.
- Threads inside a turn: largest turn 16,000 cycles; any fork loses. Stage-2 projection across cores loses
  below 500,000 dirty records.
- Generic kernel as the default: 3x slower than the Metal presenter on canvas hover.
- LLVM auto-vectorising the blend; vector gradients (2x slower than scalar); flat-row replication on layered
  scenes; tile-program dedupe; Wallace 4-row shadow integration (off by up to 21/255).
- Deferral with a generic netting journal (965 -> 1,091); stage 1 as a scan of every page.
- "A transition is a journal withheld" as stated (inputs must show, journals carry no old values).
- Structural derivative rules per expression (replaced by evaluate-and-compare); branded handles (no handles).
- hotpath for small cells (about 310 instructions per measured call).
- A second benchmark host by shipping binaries over rex (21 KB/s).

## Open: to try, roughly by expected size

**Start-up** (largest multiple on the scoreboard)
- Take the 65.7 ms apart: 115 ms of virtual-100,000's 565 ms is before the Wayland connection opens.
- Static linking, nothing before `main`, no font discovery or parsing before the first frame.
- First frame's pixels handed to `wl_shm` from an image file; first commit in the same flush as the configure.
- Build-time shaping of constant strings into a mappable glyph image.
- GPU bring-up never on the first-frame path; plan and generated shader source cached on disk by binary.
- An allocator for start-up and mount: bump/arena per epoch, no free lists, huge-page-aligned reservations.

**Interaction with caches cold** (hover 3,194 cycles warm, 12,342 cold; about 210 data lines)
- Take the 210 lines apart; pack the interaction path's hot data into a few lines kept together.
- Prefetch at input arrival, before the handler runs, from the pick.
- Differentiate fewer pages: per-word reader lists exist; add page filters from structure.
- Kernel code size per archetype; shared code for archetypes of equal layout.

**Incremental computation**
- Sort, filter, group and join as incremental operators (search and table scenarios); aggregates (column
  totals) maintained from deltas.
- Order lane so an insert costs one row; nested lists as one child store.
- Layout validity intervals so a resize is a range check; holds / moved / changed classification.
- Cold cost of an edit that changes geometry (Turn stage) end to end.

**Memory that scales with exposure**
- The benchmarked sheet at 10,000 rows is 333 MiB because the façade mounts every cell; the new core's
  columns are 41.7 KB at any size. Put it in the benchmarked binary.
- Text: glyph atlas residency by exposure; memo sized by use; images as a 64x64 tile pool read in place.
- Store capacities from use, not worst-case reservations (4 GiB address space today in the façade).
- Allocation-free steady state everywhere; arena per frame for staging.

**Raster and GPU**
- Coverage memo for layers that are pure in constant operands (shadow corners are 18 of a 24 ms canvas frame).
- CPU kernel for small damage, resident GPU path above a measured area; one-record hover separate from 200.
- Bands owned by cores for the CPU kernels; the interaction path never queued behind a full frame.
- Late latching of pointer, time and scroll words; pick in the frame being drawn; speculation of likely next
  states into spare tiles.
- Per-archetype GPU source instead of per column; Vulkan compile cost.
- 1:1 image command; YUV tiles; SDF or coverage-profile glyphs for large or animated text.

**Latency after the write**
- Commit on input even when a frame callback is pending (newest state wins before the latch).
- Animation paced by the output's refresh (the spinner is stuck at 62.5 fps at 120 and 240 Hz).
- macOS: front-buffer repair into the surface on screen; presentation without a Metal device.

**API and surface** (the author's code must not change)
- Element layer: the façade's `View`, elements, `sig!`, handlers on the new core.
- Composition borrowed from Bevy: bundles, required components, typed queries, plugins, resources.
- Text API to gpui-ce's and the web's level; editor, IME, selection; fonts with fallback.
- Accessibility as a projection, including in motion and for unmaterialised rows.
- Async holds and transitions restated on the new core.
- Native views interleaved; Windows.

**Measurement**
- Cache and branch misses and flamegraphs for every hot path, warm and cold, on both remotes.
- A richer animation benchmark (only spinner cells exist); the five heavy scenes in four frameworks.
- Compile time and binary size per component as standing columns.
