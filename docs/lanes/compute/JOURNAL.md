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
