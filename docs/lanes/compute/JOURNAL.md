# compute lane, round 7: journal

Branch `r7/compute` (worktree `wt/r7-compute`), started 2026-11-12 from `round6` `3b0b00c698`. Nothing is
pushed. Raw logs live in the ignored `runtime/lanes/compute/r/`. Host for every number unless stated: M3 Pro
(6P + 6E), macOS 15, shared with the other lanes (1-minute load is recorded next to each number).

The brief (ROUND7.md, "compute"): one compute model for CPU and GPU; shaders in wgsl-rs; exactness as a test;
a gradient one core -> many cores -> GPU chosen by a measured cost model; SDF and effects under load.

## 0. Merges (checkpoint section 5, step 8)

* `82b83dd072` (gpu): conflicts in both manifests (kept both lanes' gate features), `fused.rs` (column views +
  `cpu_written` + the bin-growth loop kept together with certificates and the oracle), `shader.metal` (column
  table kept; the prefix-expanded bin taken and its three `runs` reads moved to `fcols(pool).runs`). Memory's
  explicit `fma` was re-applied to the two blend sites gpu added (glyph loop, plain loop).
* `oracle.metal` was the round-6 *base* shader, which reads columns as `pool + offset`; the app now runs on
  column views, so that oracle would read unmapped ranges. It is now `round6`'s `shader.metal` (the pre-gpu
  kernels with the column table and memory's written-out arithmetic).
* `98834fc3bf` (memory WIP: clip-id refusal, `BOUGH_CPU_VERIFY`): merged clean.

## 1. The formulation this lane is testing

The lead's three items assume two executors that are made to agree. The stronger statement:

> **A frame is a fold of one pure integer function over tile programs.** `pixel = store(fold(blend, 0,
> [shade(cmd, x, y) for cmd in program(tile)]))`, with `shade`, `blend`, `store` written once (Rust, wgsl-rs
> subset), all arithmetic in integers (Q15 colour, Q8 geometry). Floats appear only as *guesses* that an
> integer check corrects (`isqrt`), never as values.

Three algebraic identities then hold exactly, not approximately, and each deletes something:

1. `shade = 0` outside a command's window and `blend(acc, 0) = acc`: any **superset** program gives the same
   pixels. So binning may differ per executor (CPU cull lists, GPU bin pass) with no seam.
2. `blend(acc, opaque) = opaque`: a command that covers a tile opaquely **cuts** the program beneath it,
   exactly. Occlusion is a property of the program, not a heuristic.
3. Coverage saturates to exactly ONE inside a command's core: span fills on the CPU and block fills on the GPU
   are the same function.

What should become unnecessary if this holds: the second arithmetic model (`cpu_raster.rs`' IEEE-half twin of
the MSL), the per-machine self-check and its cache file (`repair.rs`), the "refused" path (kinds the CPU does
not model), `oracle.metal` and the GPU full redraw inside `reference_pixels` (the oracle is the same source run
natively, so the pixel gates no longer need a GPU and could run on Linux), fma/`precise::sqrt`/half
reinterpretation as porting gaps, and the full repaint on executor switch that memory's handoff feared.

## 2. Experiments (prediction and kill criterion written before the run)

| id | hypothesis | prediction | kill |
|---|---|---|---|
| E0 | the merged tree still passes the gpu-mode pixel gates | heavy/scenes/vlist: 0 failed | an oracle mismatch that is not arithmetic means the oracle swap was wrong |
| E1 | the 22 CPU-mode failures are structural, not arithmetic | `BOUGH_CPU_VERIFY` names one or two causes in the presenter (held surfaces, copy sources), and after the gpu merge also stale certificates | if the first differing pixel is a one-LSB difference, the f16 model is not exact and the plan to replace it gains weight |
| E2 | fixed point makes CPU == GPU a consequence of the source | 0 differing bytes over >= 10,000 random tiles (all kinds, adversarial sub-pixel positions) between (a) the wgsl-rs source run natively pixel by pixel, (b) the fast CPU executor (spans, cores, occlusion cut), (c) Metal running naga's translation of the same source; three single-constant mutants each caught | any mismatch whose fix needs executor-specific arithmetic |
| E3 | the integer kernel is not slower on a CPU than the f16 model | full-tile translucent solid <= 3.0 K instructions per command, opaque <= 1.5 K; rounded-box edge pixel <= 40; glyph pixel <= 12 | more than 2x the f16 model's instructions on the same content |
| E4 | integer shading costs the GPU little | kernel time within 1.2x of a float variant in the same harness | more than 1.5x |
| E5 | there is a window where bands across cores beat both one core and the GPU | wall time to pixels: one core wins below ~150-300 tiles of UI content, 6 cores to ~1,000-1,500, the GPU above; by instructions (energy) the CPU-GPU crossover is at 10-30 tiles because a Metal frame costs 72-90 K | if at the tile count where one core loses to the GPU, N cores also lose, the middle tier is dropped |
| E6 | the exact occlusion cut matters in layered scenes (the gpu lane measured ~0 gain on the existing scenes) | brutal scene (5,000 layered nodes): commands per tile -40 %, CPU time -30 % | under 10 % |
| E7 | a blurred rounded-rect shadow has an integer closed form good enough to replace the float erf | box-spline (three box blurs) CDF as truncated cubes: max error <= 2/255 against a brute-force convolution at radius 0; <= 60 instructions per pixel | error over 3/255 at radius 0, or over 150 instructions per pixel |
| E8 | resize and hover-with-thousands-of-dependents are full-window repaints and the model picks the right tier | at 2560x1600 (4,000 tiles) the GPU wins wall time; many-core within 2x of it | n/a (measurement) |
| E9 | unified memory lets the GPU kernel store through a buffer that aliases the surface | a no-copy `MTLBuffer` over an IOSurface's base address is accepted and compute writes appear in the surface | the wrap is refused, or writes are not visible without a lock/seed step |
| E10 | GUI pixel rows repeat vertically: inside a command's y-flat interval (`shade` provably independent of y) a row equals the row above | on non-text UI tiles >= 60 % of pixel rows are copies of the row above; CPU time per such tile -50 % | under 20 % of rows on the brutal scene |
| E11 | the integer kernel is translation invariant (`shade(cmd + d, p + d) == shade(cmd, p)` for integer d), so tiles with equal origin-relative programs are one raster and N copies | a 10,000-cell grid without text: distinct programs <= 1 % of tiles and a full repaint costs <= 5 % of shading every tile; text tiles do not dedupe | under 2x on the grid |
| E12 | a transition is two staged commands and one Q15 parameter; the presenter interpolates them with integer arithmetic and repairs their bounds, with no turn | per frame: <= 60 instructions per animated command to restage, then the ordinary repair; identical bytes on every executor | any difference between executors |
| E13 | a user shader is one more `sh_*` function in the same Rust source, so it runs on all three executors | 0 differing bytes; no executor-specific code for it | it needs its own CPU twin |

### Amendment 1 (received while E0 was building)

The user's worry about this lane is a generic compute abstraction. What is being built is the opposite of
general: one kernel that knows only GUI primitives, and executors that exploit what GUI frames are. No kernel
graph, no scheduler framework (the gradient is one comparison of three measured costs), no second wgpu (the
GPU side is the existing Metal presenter fed generated MSL). E10 to E13 were added in response; they ask, of
each groove, what it lets the executor not do: rows repeat (E10), tiles repeat (E11), a transition is a
parameter (E12), a user shader is a node of the same kernel (E13).

## 3. Log

### E0: merged tree, gpu mode (2026-11-12, load 25-49, release profile)

`cargo test --release -p bough-heavy -p bough-scenes -p bough-vlist`: 31 passed, 0 failed, 1 ignored (one
run). The oracle swap holds: the gpu lane's kernels equal round6's kernels with the column table on every
frame the gates render. Mutants not re-run on the merge.

### E12 revised after the lead's notes (R7-LEAD-NOTES section 3)

The lead proposes the record carry (from, to, t0, duration, ease) and the kernel evaluate it at the
presenter's `now`. Adopted, with one addition that the superset identity gives for free: the animated command
is staged with the **swept window** and the **common core**, so the tile program does not change for the
whole transition; a frame of a running transition is "write `now`, repair the swept tiles". No rebinning, no
restaging, no turn. Prediction: a static command pays one flag test (CPU fast path within 1.05x on a static
scene); exact on all executors at any `now`. Kill: any executor difference, or more than 1.3x GPU kernel time
on a scene with no transitions. Layout transitions are the same mechanism (geometry lanes interpolate, Q8,
split product so no 64-bit multiply is needed). User nodes receive `now` the same way (E13).

### E2: exactness by construction: CONFIRMED (2026-11-12, M3 Pro, release, load 25)

New crate `crates/bough-kernel`. `src/kernel.rs` is the only place a pixel's value is defined (two wgsl-rs
modules: `fx`, the user nodes, and `kernel`). build.rs turns it into WGSL (wgsl-rs) and MSL (naga); Metal
compiles the MSL text when the GPU executor is created (there is no `xcrun metal` on this machine, and no
build-time shader compiler is needed). `cargo test --release -p bough-kernel --test exact` with
`BOUGH_EXACT_SEEDS=125`:

* 125 adversarial scenes of 320x250 (110 commands of every kind: solid, rounded, border, glyph, shadow,
  gradient, user node; 1/8 px positions, clips, radii past the root-shift threshold, one command in eight a
  transition sampled at four clock values) = 10,000 distinct tiles;
* `cpu::raster` in five configurations (cut on/off, flat rows on/off, 1 to 4 threads) = 50,000 tile repairs,
  the damaged-subset path, and Metal with and without the cut = 20,000 GPU tile repairs;
* **0 differing words** against `cpu::naive` (the kernel folded over every command at every pixel). Wall time
  of the whole gate: 7.5-9.3 s. It passed on the first run that compiled; nothing was tuned to make it pass.
* Negative controls, all caught: GPU kernel with HALF = 16383 (3 of 4 scenes: it is a 1-in-32768 event per
  product); `mutant_cut` (cut on a half-opaque cover); `mutant_flat` (corner rows called flat);
  `mutant_core` (core one pixel wider). The first form of `mutant_core` was NOT caught: the core carried a
  whole spare pixel. That was a finding, not a test bug: the margin is now half a pixel plus 8/256, which
  enlarges every core, and the mutant is caught.
* naga writes signed arithmetic through unsigned (`as_type<int>(as_type<uint>(a) * as_type<uint>(b))`), so
  no integer operation in the generated MSL has undefined behaviour; float-to-int goes through a clamping
  helper. The single float operation is the root guess in `isqrt`.
* wgsl-rs gaps met: none for this kernel. Of the gpu lane's list (G13), half reinterpretation and fma
  contraction control are not needed at all in integers. `sizes_buffer` must be given to naga for
  runtime-sized arrays even though no length is read.

The reference hash of the 125 specification frames is `b06da2304ca40bf6` (for the x86 run on ilo-prod).

### The gate caught a false identity (same day)

When shadows got a core and flat rows, the gate failed on seed 3: one channel off by one at one pixel. Cause:
"the blur saturates beyond 1.5 w" depended on `(1.5 w * inv) >> 16` reaching 384, which an odd `w` misses by
one. `in_w` now returns exactly +-384 at and beyond 1.5 w, so saturation is a comparison, not a rounding.
This is what the gate is for: an executor shortcut is only as good as the identity under it.

### E7: shadows (kernel at `5c009dede7`; `cargo run --release -p bough-kernel --example shadowerr`)

Error is coverage against the exact convolution of the rounded box with the spline kernel (floats, chord
integrated over y at 1/32 px), in 1/255 units, maximum over every pixel of the shadow's window; boxes 80x60,
24x18 and 200x9 at a 3/8, 5/8 px offset; radius 0-30; sigma 0.5-16. Raw tables: `runtime/.../shadowerr-*.tsv`.

| formulation | radius 0 | rounded, worst | rounded, typical mean |
|---|---:|---:|---:|
| Wallace's construction with the spline, 4 midpoint rows (first version; what the float shader does with erf) | 0.02 | 21.6 (r 30, sigma 0.5) | 0.2-0.9 |
| separable box minus corner slivers, 4 midpoint rows | 0.02 | 21.6 | 0.04-0.5 |
| ... 4 Gauss-Legendre rows | 0.02 | 42.8 for narrow blurs; <= 1.5 for r <= 2.25 w | |
| arc distance profile, uncorrected | | 26 at r = w | |
| arc profile minus first-order curvature term `density(u) w / (8 r)`, weighted by arc-ness | | 3.7 at r = 4 w, 2.0 at 8 w, <= 1.9 beyond | 0.01-0.4 |
| **kept: Gauss-Legendre slivers below r = 3 w, corrected arc profile from 3 w** | **0.02** | **6.3** (r 30, sigma 4), 5.1 on a pill | 0.01-0.36 |

Prediction (radius 0: error <= 2/255) confirmed with room: the spline shadow of a square-cornered box is
exact to the quantisation. The rounded case was not predicted; it is 3.4x better at worst than the
construction the float shader uses, and its error is confined to a few pixels at each corner. What the form
buys beyond accuracy: away from the corners a shadow is a product of two 1-D profiles, so its interior is a
core (exactly ONE) and the rows along its straight sides are flat. Not done: the 1-D profiles as two
32-entry vectors per tile on the CPU (the fast executor still calls the kernel per pixel there).

### E3, E10: CPU cost per tile-command (`kbench micro`, dist profile, M3 Pro, load 33, one run)

One kind stacked K times over 512x512 (256 tiles); slope between K = 2 and K = 10 per tile-command (a
command covering a whole 32x32 tile), single core. "flat" = with flat-row replication.

| kind | instr | instr, flat | ns, flat | rows copied | GPU kernel ns |
|---|---:|---:|---:|---:|---:|
| solid, opaque | 2,094 | 283 | 10 | 97 % | ~60-120 |
| solid, translucent | 7,821 | 474 | 20 | 97 % | ~60-120 |
| rounded box r 8 (mostly core) | 13,184 | 1,540 | 102 | 93 % | ~10-190 |
| border r 8 | 13,705 | 1,727 | 90 | 93 % | ~100-170 |
| gradient at 57 degrees | 24,522 | 24,411 | 3,007 | 1 % | ~35-140 |
| shadow r 0, sigma 6 | 42,032 | 20,791 | 919 | 84 % | ~130-230 |
| shadow r 12, sigma 6 | 62,804 | 37,299 | 1,910 | 80 % | ~140-200 |
| user node (stripes) | 54,127 | 54,492 | 2,532 | 1 % | ~100-220 |
| text, 8 glyph boxes per tile | 22,404 | 22,307 | 1,544 | 7 % | ~110-220 |

Against the predictions: translucent solid without replication is 7.6 instr/px, not <= 3 (LLVM vectorises
across the four channels of one pixel, not across pixels): FAILED as predicted, and made irrelevant by
identity 4 (474 per tile). Glyph pixels ~13 instr (predicted <= 12). Per-pixel kinds (gradient, shadow, user)
are 24-60 instr/px: scalar. The GPU column is the slope of minimum kernel times and is noisy (the two runs
disagree by 2x); read it as "50-250 ns per tile-command whatever the kind".
**The finding: executor choice is a function of content class, not only of tile count.** Flat GUI content
(boxes, borders, backgrounds) costs a CPU core 10-100 ns per tile-command, less than the GPU's kernel time;
per-pixel effects cost it 1-3 us, 10-50x the GPU's.

### E5, E6, E10 on whole scenes (`kbench scene ... 2560 1600`, dist, M3 Pro, load 26-34, one run each)

Raw: `runtime/lanes/compute/r/scene-*.tsv`. us are minimum of repetitions; "cadence" is one frame per 4 ms
(median), which is what a display sees: parked workers, cold caches, a GPU that has clocked down.

| scene | commands | entries/tile no cut -> cut | rows copied | full frame: 1 core | 11 threads | GPU wall (kernel) | at cadence: 1 core / 11 / GPU |
|---|---:|---|---:|---:|---:|---:|---|
| grid (hairlines, no text) | 95 | 2.68 -> 2.68 | 90 % | 0.80 ms, 14.2 M instr | 0.52-0.65 ms | 0.96 (0.74) ms | 3.4 / 1.16 / 1.98 ms |
| gridtext (1,800 cells with numbers) | 7,322 | 5.91 -> 5.91 | 43 % | 3.72 ms, 68.6 M | 1.09 ms | 1.24 (0.99) ms | 3.8 / 1.80 / 1.95 ms |
| canvas (5,000 layered cards: shadow, body, border, gradient header, text) | 234,292 | 297.9 -> 18.2 | 14 % | 238 ms, 2.86 G | 40 ms | 5.49 (5.25) ms | 239 / 54 / 5.9 ms |

* **E6 confirmed far beyond the prediction** (-40 % commands): the cut removes 94 % of the canvas's tile
  entries and 94 % of the CPU's instructions (47.2 G -> 2.86 G). The gpu lane's "0.00-0.01 layers per pixel"
  on the existing scenes and this are both true: the cut is worth nothing on flat scenes and everything on
  layered ones. It costs nothing where it does not apply.
* **E10 mixed.** 90 % of rows on the plain grid (2.6x fewer instructions), 43 % with text in every cell, 14 %
  on the canvas (under the 20 % kill line there: shadows, clipped gradients and text leave few flat rows).
  Kept: it is a bitmask AND per command and it is the reason flat content beats the GPU.
* **E5: the crossover moves by 50x with content.** Wall time, back to back / at cadence:
  gridtext: one core beats the GPU up to ~256 tiles (229 vs 446 us) / ~256; all cores to ~1,000 / beyond
  2,000 (1.8 vs 1.95 ms for the whole window); the plain grid: one core wins at every size back to back.
  canvas: one core wins only to ~8 tiles, several cores to ~32, the GPU beyond (5.5 ms for a frame that
  takes 11 threads 40 ms).
  By instructions: a GPU frame costs this harness 185-195 K on the CPU whatever it draws (one command buffer
  and encoder per frame; the presenter's pre-armed stream is 72-90 K); a gridtext tile costs 33 K, a grid
  tile 2-8 K, a canvas tile 700 K. The CPU is cheaper in instructions below ~3-6 gridtext tiles, ~10-30 grid
  tiles, and never on the canvas.
* **Waking workers is not free**: a 4-tile repair is 2.3 us on the caller and 25-35 us (+180 K instructions)
  when ten workers are woken for it. Workers are worth it above roughly 50-100 us of estimated work.
* **Binning as written is O(commands)** (6.9 ms for the canvas's 234 K commands): it stands in for the
  runtime's page cull, which is retained. It is outside every raster time above.

Consequence for the design: the choice is `min` over three estimates that the binner can accumulate per
frame from per-kind weights (the table above) and the tile count; no framework is needed for it.

### E2 across architectures: CONFIRMED (ilo-prod, Ryzen 7 3700X, Linux x86-64, load 20-23, commit `5c009dede7`)

`BOUGH_EXACT_SEEDS=125 cargo test --release -p bough-kernel --test exact` and `kbench hash` on ilo-prod print
`spec hash 64bf21a9ea02ff88`, the hash the M3 Pro prints for the same commit, with 0 of 125 scenes differing
between the specification and the fast executor there. So aarch64 CPU == x86-64 CPU == Apple GPU, byte for
byte, from one source; the crate built on Linux unmodified (the Metal module is behind `cfg(target_os)`),
which means the pixel gates' oracle no longer needs a GPU or a Mac.
(The hash changed from `b06da2304ca40bf6` because the shadow formulation changed between the two commits.)

CPU executor on Zen 2 (same commit, dist profile, busy host; raw output in the lane's runtime notes; kbench's
instruction columns are 0 on Linux, they read a macOS counter):

| scene, 2560x1600 full frame | 1 core | 8 threads | 16 threads |
|---|---:|---:|---:|
| grid | 3.2-4.5 ms | | 0.57 ms |
| gridtext | 8.9-9.3 ms | 2.1 ms | 1.36-2.6 ms |
| canvas (5,000 cards) | 420-442 ms | | 45 ms |

Zen 2 is 2-2.5x slower per core than the M3's performance cores on this code, and scales 6.8x (gridtext) to
9.3x (canvas) on 16 threads. The shape of the curves is the same: a 1-tile gridtext repair is 3 us, waking
workers for under ~16 tiles loses, the cut takes the canvas from 7.6 s to 0.44 s per frame on one core.
`perf stat` (user mode, whole kbench run, pinned to one CPU): 42.84 G instructions, 17.6 G cycles (IPC 2.4),
L1d misses 3.1 % of loads, L2 misses 87.7 M (2 per 1,000 instructions): the executor is not memory bound.

### E9: the GPU kernel can store through a buffer that aliases the surface: CONFIRMED (M3 Pro)

`cargo run --release -p bough-metal --example iosurf_alias`: `newBufferWithBytesNoCopy` accepts an IOSurface's
base address (page aligned; its allocation is a whole number of pages); a compute kernel that writes words to
that buffer leaves them, all 32,768 of 32,768, both at the surface's base address and in the texture view the
presenter makes of the same surface. One run, headless; not checked: a surface that is purgeable-volatile, or
in use by the window server, while the buffer exists; whether the compositor needs the lock/unlock seed bump
(the CPU path already does one).

What this deletes: the GPU kernel's texture stores (and with them the "which of four textures" selection that
was a wgsl-rs gap, the unorm conversion that made the old CPU model's exactness depend on the hardware, and
the residency-set entry per surface). Both executors now have the same output contract:
`out[y * stride + x] = bgra`, into the memory the compositor reads.
