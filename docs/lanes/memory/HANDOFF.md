# Lane memory, round 6: HANDOFF

## State

* Branch `lane/memory/round6` (worktree `wt/memory`). Head: the commit carrying this file (on top of the WIP
  commit below).
* **Merged into `round6`:** a1873e83fa (lead, 2026-11-12, no conflicts): the ledger and its instrumentation, region
  per-column views and `Region::heap` as an mmap, the written-out SDF/blends in shader.metal, `Fused::reference_pixels`
  growing bins, the CPU repair backend **opt-in** (`BOUGH_REPAIR` default `gpu`), the self-check, probes and tools.
* **Not merged:** the commit after a1873e83fa (WIP): the CPU stage refusing out-of-range clip ids (fixes the
  `gpu_bounds` panic in CPU mode; not re-run) and `BOUGH_CPU_VERIFY` (per-frame CPU-vs-GPU comparison with the
  entries at the first differing pixel; compiles, never run). Docs: `RESULTS.md`, `EXPERIMENTS.md`,
  `CPU-REPAIR.md`, this file.
* Expected conflicts (lead's notes): the store lane changed `Region::heap` to an mmap too (agree one
  implementation); lane gpu rewrote shader.metal at 82b83dd072: the written-out `f_sd` (`precise::sqrt(fma(m.y, m.y,
  m.x * m.x))`) and the two explicit fmas must be re-applied on its kernels, and a CPU repair must then write or zero
  the row certificates of the tiles it touches.

## Numbers (all on the M3 Pro, macOS 15, 2x display; load 40–130 throughout the round)

Ledger (`LEDGER.md`; `bench/memory/ledger.py` + `headline.py`/`table.py`, raw `bench/results/memory/2026-11-11-base`,
3 runs each, t = 3 s active):

| | counter | dashboard | scroll 100 | scroll 1 K | scroll 7 K | heavy |
|---|---:|---:|---:|---:|---:|---:|
| phys_footprint MiB | 122.5 | 166.2 | 151.4 | 153.6 | 164.6 | 145.9 |
| RSS MiB | 39.8 | 54.2 | 40.2 | 42.7 | 60.8 | 38.2 |
| driver, unmapped graphics + IOAccelerator/IOKit | 68.0 + 8.0 | same | same | same | same | same |
| surfaces drawn | 2 x 2.34 | 3 x 10.78 | 3 x 10.78 | 3 x 10.78 | 3 x 10.78 | 3 x 15.63 |
| region resident (in use) | 20.03 (0.05) | 20.03 (1.30) | 20.03 (0.07) | 20.03 (0.35) | 20.03 (2.17) | 2.03 (0.18) |
| malloc zones (Rust live) | 12.4 (3.1) | 23.9 (13.2) | 12.8 (3.4) | 15.4 (5.3) | 26.3 (20.0) | 12.1 (n/m) |

Calm (t = 9 s): counter 114.0, scroll 7 K 137.4; with keep-warm off 53.2 and 95.4. Per row (100 -> 7 K):
footprint 2.0 KiB, Rust heap 2.5 KiB, GPU data ~320 B. RSS-vs-footprint gap (104 MiB) = unmapped driver 76 + three
IOSurfaces 32 + region pages wired but never CPU-written ~18, minus ~22 of clean pages. Verified: 3 runs each,
spreads in LEDGER.md; the driver split is footprint minus `vmmap --summary` mapped residency.

| result | before | after | command | verified |
|---|---|---|---|---|
| region resident on the AWAGY windows | 20.03 MiB | **not measured after** (expected ~0.3–2.5 MiB: the views' committed prefixes) | ledger.py batch on a1873 | NO: the memory batch after the change was never run |
| capacity past APP_CAPS | panic (instance column full) | 70,000 quads drawn exactly | `cargo test --release -p bough --test capacity` | yes; mutant `sabotage_views` fails (68,976 wrong) |
| instruction slopes (14 cells, R1 and R0) | A d892f974da | B/A 0.992–1.013 | `bench/memory/slopes_ab.py 5` | 5 reps, 10x spread, interleaved, load ~70 |
| GPU time of the written-out shader | lab full 0.2787 ms | 0.2783 (x0.999); SDF-dense x1.012–1.013 | `kernel_bench`, `examples/kernel_ab` with `BOUGH_METAL_SRC` | 4–5 rounds, saturated bench; heavy/animate/vlist not measured |
| pixel suites, gpu mode | | 174 passed, 0 failed | `runtime/lanes/memory/gates.sh` (TAG=a1873) | once |
| pixel suites, cpu mode (`BOUGH_REPAIR=cpu`) | | **152 passed, 22 failed** | same | once |
| sabotage_copy mutant (scroll gates, gpu mode) | | caught: scenes 5 of 11 and vlist 6 of 8 tests fail | `cargo test -p bough-vlist -p bough-scenes --features bough/sabotage_copy` | once |
| CPU model exactness | | 0 mismatching tiles (fused blend), 2,683–2,710 with two roundings | `examples/cpu_tiles stress` | 4 seeds x 2,772 tiles |
| GPU sqrt vs IEEE | | 1,221,178 / 4 M differ; dot = fma(y,y,x*x) exactly | `examples/mathprobe` | once, 4 M inputs |
| driver block | 75.4 MiB with a live queue | 7.0 within 55 ms of releasing the queue; re-creating costs 3.0–3.3 ms / ~11 M instr | `examples/drvprobe` | 2–6 cycles |

## Not verified / known broken

* **CPU mode fails 22 pixel tests** (heavy correctness 4, heavy gpu_bounds 1, scenes correctness 9, scenes present
  3, vlist 5). Causes known: `gpu_bounds` = the CPU stage indexed a corrupted clip id and panicked (fixed in the WIP
  commit, not re-run). Causes not known for the others: the mismatches are large (e.g. blink step 2 at (80, 81):
  CPU 247 vs GPU 28, a whole-colour difference, not a rounding), so a structural bug: suspects in order: (1) the
  paint-only path (`Pick::Held` / content addressing) assuming a surface's pixels came from a GPU frame of that
  epoch; (2) a surface written by the CPU then used as the scroll copy's source by the GPU while the CPU's IOSurface
  writes are not yet visible to the GPU's texture view (no `IOSurfaceLock` coherence guarantee with a resident
  texture); (3) `write_begin` locking a surface the GPU still reads as a copy source. `BOUGH_CPU_VERIFY=1` on one
  failing test (`cargo test -p bough-scenes --test correctness blink`) will name it in one run.
* The memory batch after region views and after the CPU backend: never run (the before/after per holder the lead
  asked for is missing).
* The cost model's constants (`CPU_TILES_WARM` 16, `CPU_TILES_COLD` 4,096) are guesses; `examples/repair_cost` and
  `examples/interact_cost` are built and never run.
* The self-check (`repair::cpu_exact`) has never run (only reached under `BOUGH_REPAIR=auto`).
* Queue release: code drafted (`drafts/release_queues.py`, a patch script for `Fused::release_queues`: drain the
  armed frames, new queue + aux queue, move the residency set, rebuild Kern and Warm), never applied or built.
* `cargo check --workspace --all-targets` not run on the head; `--features wgpu` not built (the façade's new calls
  are gated off for it).
* `stallprobe` (the pre-armed batch timeout repro) written, never built or run.
* The CPU executor reads glyph coverage from the CPU atlas copy; if the atlas is ever freed after upload (see
  avenues) it needs another source.

## Experiments

| experiment | number | verdict |
|---|---|---|
| a shared MTLBuffer as address space | wired + charged whole once resident (64 MiB -> +64) | killed |
| a no-copy buffer over untouched mmap | footprint unchanged but system wired +1,024 MiB for 1 GiB (hidden, not saved) | killed (accounting trick) |
| no-copy views over an mmap's used prefix | wires only the prefix (64 of 1,024 MiB); overlapping longer views work | kept: region views |
| driver block vs queue depth, storage mode, residency set, threadgroup memory, a second queue | no effect (+0.2 for a second queue's first submission) | killed |
| release the queue | returns all 69 MiB in 55–300 ms | kept (X1, not applied) |
| SDF length with a branch (no root beside edges) | +4.6–6.9 % GPU time | killed |
| explicit fmas in the blend and border | 0.2782–0.2784 vs 0.2787 ms | kept |
| headless GPU-slope A/B at 10x spread | A/A controls spread 0.63–1.91 at load 40–56 | instrument unusable under load; saturated bench used |
| lossless-compressed IOSurface formats | (IDEAS-OPUS K2, agreed) same allocation | killed |
| keep-warm off | counter calm 114.0 -> 53.2 MiB; first frame after 2 s idle 2.4–2.7 ms / 8.5 M | trade measured, policy not changed |
| X1 queue release / X2 never-submitted counter / X3 font file mapping | predictions in `EXPERIMENTS.md` | not run |

## Underexplored and exotic avenues

1. **The signal-write frame through the CPU path** (the lead's order-of-magnitude case). Today 107.5 K instructions
   with render vs 3.7 K without; a 2-tile CPU repair is ~15–31 K estimated (IDEAS-OPUS E2/E4 numbers) -> ~5x per frame,
   and no queue, no driver block. Cost: the 22 CPU-mode failures first. Cheapest test: `micro_bench SCENE=state`
   with `BOUGH_REPAIR=cpu` once the blink failure is fixed.
2. **A process that never submits GPU work** (X2): counter predicted 122.5 -> ~25 MiB active, ~22 idle; then to
   reach the lead's < 10 MiB idle: the font copies (2.2 MiB dirty, X3), frameworks' malloc (~6 MiB, AppKit's), the
   second surface purged at once when calm (2.3), the CPU atlas copy (0.5). Depends on: CPU-mode correctness, a
   first frame that the CPU accepts (full window: 600 tiles for the counter, fine).
3. **Queue release on calm** (X1): −69 MiB on every window ~1 s after its last GPU frame, +0.6 ms / +2.5 M on the next
   GPU frame versus a cold wake; replaces keep-warm's purpose with a speculative wake (one 4-byte blit when input
   starts a burst, IDEAS-OPUS formulation 1). Risk: the pre-armed stream's lifetime (drain before release).
4. **Cost model as a mechanism, not a threshold**: per frame `tiles x cpu_per_tile` vs `gpu_fixed + tiles x
   gpu_per_tile + wake(state)`, with wake from the queue state (live / released / powered down) and, later,
   occlusion, another Space, thermal state, battery. `repair_cost` measures the per-tile terms; one run.
5. **One reference surface plus residual layers** (IDEAS-OPUS 11): ring of 3 x 10.78 MiB -> ~1 x 10.78 + patches
   on AWAGY (−20 MiB), more at 6K (244 -> ~85 MB). Depends on undocumented compositor behaviour; the kill probe is a
   windowed write-under-patch test.
6. **Derived instead of stored** (largest first): (a) the ring's spare surfaces (21.6 MiB active on AWAGY): a spare
   is derivable from the displayed surface + this epoch's repair (copy-forward), so it need not survive between
   frames; (b) the layout engine's per-label state (LeafCtx 392 B + cephal cache box 360 B per text leaf, ~5 MiB at
   7 K rows): a one-line label's measure is O(1) from `line_w`, so its cache box need not exist, and LeafCtx's slow
   path (`run` 88 B, `text`+`starts` 48 B, `user: Option<Spec>` 88 B) can be one boxed cold half (392 -> ~170 B);
   (c) glyph pens (`x: i32` per slot, 4 B of every 6): derivable from the token stream and the advance/kern table
   (prefix sum in the bin pass), leaving 1–2 B per glyph; (d) the CPU copies of the atlases (0.5 / 4 MiB) and the three
   font copies (2.2 MiB).
7. **Bits per record / glyph / tile** (measured inputs, arithmetic): per mounted AWAGY row ~2.5 KiB heap + 320 B GPU
   for 2 records = ~11.5 kbit per record against a core record of 23 B (184 bit); glyph slots 6 B (48 bit) with
   32 slots reserved for a 9-glyph label = ~170 bit per drawn glyph against ~8 bit of information (the token);
   surfaces 3 x 4 KiB per 32 x 32 tile = ~98 kbit per tile of window. Bit-packing candidates: page `handle` u32 and
   `payload` u32 (64 bits per slot) could share one u32 when position is identity (store lane); `NRect` i16 x 4 inks
   as tile-relative u8 x 4 inside a page whose summary holds the origin; run headers (32 B) as 16 B (origin as i16 x 2
   on the 1/256 grid is too narrow; i32 x and i16 y + u16 strike/space + u16 len + i32 base/x0 fits 16 B);
   instances 48 -> 24 B (IDEAS-OPUS 13).
8. **Pre-armed batch timeout under a stall**: 2 of 9 heavy runs died with `kIOGPUCommandBufferCallbackErrorTimeout`
   ~1 s after `footprint`/`vmmap` suspended the process. Repro written (`drafts/stallprobe.rs`; copy into
   `crates/bough-metal/examples/` to build): arm 4 frames in one command buffer, release frame 1, wait D s, release frame 2. If it dies
   for D above some seconds, every window idle mid-batch would die; it does not in practice, so the suspension itself
   (not the wait) is the suspect. Fix candidates: arm batches of 1–2 frames when calm; drain the batch on suspend.
9. **Startup**: the lead measured an empty headless App at 13.2 M instructions, 35 % zero-filling the region (fixed by
   the mmap in both this lane and store); the self-check adds one probe App per machine + OS build (cached). Glyph
   rasterisation at start for strikes the first frame does not draw, and font table parsing, are the next items.
   Shutdown: dropping the queue frees the driver block in ~50 ms; nothing measured beyond that.
10. **Binary size**: the CPU executor adds code (not measured); the probes are examples, not in the binary.
11. **Steady-state allocations** (traced): wheel events clone the hover chain (patch parked in the lane's runtime,
    `input-wheel-noalloc.patch`); re-description allocates a `String` per `&'static str` text (`TextSrc::value`
    returns `String`) and runs `format!` closures per tick in the dashboard; `Rt::place_text`/`lines_point` and
    `Deep::style_id` allocate on text changes; keep-warm boxes a closure per arm. A debug assertion over the counting
    allocator (`bough_mem::count`) per frame would make this a gate.
12. **Exactness as a design rule**: writing the kernel's arithmetic out (explicit fma, precise sqrt) cost nothing
    measurable and makes any executor reproducible; the same could be done for the remaining kinds (images'
    bilinear weights, gradients, the shadow's erf polynomial) to widen the CPU executor, or the lead's relaxation
    (each executor exact against itself) used instead, at the price of a full repaint whenever a surface changes
    executor.

## Where I think the lead or the plan is wrong

* The headless per-frame GPU-slope instrument cannot gate 2 % on this machine under this load (A/A spreads up to
  0.63–1.91); gates on GPU time should use saturated benches (`bench_frames`) and say which scenes lack one.
* Relaxing CPU/GPU exactness is cheaper than it looks only if executors never share a surface; with a ring shared
  by both, mixed executors need full repaints on every switch. The written-out arithmetic made exactness free here;
  I would keep it as the default and relax per kind only where a kind cannot be written out.
* Whole-suite gates at every stage at load 100 cost ~1 hour each; a smaller fixed "gate set" (one test per crate per
  path) between stages and the full suites at merge would have doubled what this lane measured.

## Next (in order)

1. Fix the CPU-mode failures: `BOUGH_CPU_VERIFY=1 cargo test --release -p bough-scenes --test correctness blink`,
   then the rest; predicted: one or two causes (paint-only held surfaces, CPU-written copy sources). Then
   `micro_bench SCENE=state` with `BOUGH_REPAIR=cpu`: predicted ~20 K instructions per signal-write frame vs 107.5 K.
2. Apply `release_queues` and run X1 with the `wake` harness at thresholds 0.1 / 1.25 / 10 s: predicted calm counter
   114 -> ~45 MiB, first frame after release ~3.1 ms / ~11 M.
3. Run the memory batch for region views + CPU backend (`ledger.py` with `BOUGH_REPAIR=gpu` and `auto`): predicted
   counter 122.5 -> ~102 (views) -> ~25 MiB (never submitting), scroll 7 K active 164.6 -> ~146 (views).
