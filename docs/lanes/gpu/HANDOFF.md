# GPU lane handoff, round 6 (2026-11-12, Claude Fable session; replaces the Astra handoff of 2026-11-11)

## State

* Branch `lane/gpu/round6` in the lane's worktree. Head: the commit that adds this file (on top of `82b83dd072`).
  Nothing of this lane is merged into `round6`; nothing was pushed.
* `82b83dd072` is GATED on this machine by the session that built it (not replayed by anyone else): row
  certificates for the scroll copy, the survivor-stepped tile loop, 256-entry windows, prefix-expanded bin runs,
  the original-kernel raster oracle, the kernel A/B probe.
* The head commit adds measurement tooling only (`examples/frameprobe.rs`, `BOUGH_LAT` in `fused.rs`): the four
  headless drivers and `micro_bench` build and run with it; **the pixel gates were not rerun after it**.
* All kernel work is hand-written MSL (`crates/bough-metal/src/shader.metal`). The wgsl-rs kernels
  (`crates/bough-gpu`) are untouched and still have the round-4 structure. This is against the direction received
  after the work was done; see "Next".
* Private state: `runtime/lanes/gpu/STATE.md`; every raw result is under `runtime/lanes/gpu/r/`.

## Numbers

Machine: M3 Pro, macOS 15.7.4, runtime-compiled MSL, other lanes building throughout. Loads are 1-minute
averages. Scripts named below live in the ignored `runtime/lanes/gpu/` (they are 30-60 lines each; `ab.py`,
`e2e.py`, `instr.py`, `lat.py`, `locked.py`); the instruments they drive are committed (`BOUGH_AB`, `BOUGH_LAT`,
`BOUGH_MSL`, `bench/floor/gputime.c`, `docs/lanes/gpu/tools/instrument.py`).

### Decomposition before any change (µs per frame, saturated GPU; overlay `tools/instrument.py timing`, 2 runs x 5 alternating cycles, load 26-115; verified by two runs)

| cell (tiles repaired) | whole | bin | tile dispatch | pixel writes | scan + stage | shading | scroll copy (write-only / dispatch-only) |
|---|---:|---:|---:|---:|---:|---:|---:|
| heavy stream 2560x1600 (1120) | 197-204 | 24 | 8 | 41 | 38 | 86 | none |
| heavy scroll (90) | 75-87 | 15 | 1.5 | 3.5 | 5 | 20-25 | 26-27 (22 / 5) |
| heavy both (1196) | 272-333 | 25-28 | 7 | 46-60 | 41-43 | 92-112 | 31 (28 / 5) |
| lab stream 960x720 (156) | 74 | 11.6 | 1.7 | 6.5 | 7.9 | 46 | none |
| lab scroll (60) | 35.5 | 5.8 | 1.2 | 3.2 | 2.9 | 11.7 | 11.7 (10.3 / 3.1) |
| lab both (216) | 89-101 | 13-14 | 2.2 | 8.6 | 9.7 | 44-59 | 12.4 (11.0 / 3.1) |
| animate 1920x1440 (122) | 32-40 | 5 | 2 | 7-13 | 2-5 | 13-28 | none |
| vlist scroll 1440x1600 (90) | 191-244 | 6-11 | 1.4 | 4-5 | 3-4 | 1-17 | 167-201 (67-77 / 9-11) |
| vlist fling (105) | 188-213 | 7-12 | 1.4 | 4-6 | 4-5 | 3-14 | 161-167 (66-69 / 8.7) |
| vlist jump (90) | 197-201 | 8-11 | 1.5 | 4 | 4-5 | 8-15 | 161-166 (68-71 / 8.7) |

The inherited "bin scanning is 47-64 % of GPU time" is false on this tree: scan + stage 11-19 %, bin pass 12-16 %,
shading 44-62 %, and on vlist the scroll copy is 75-85 %. Per-tile counters (overlay `stats`, one run, retained ==
full redraw asserted): 4-12 % of scanned entries hit (lab stream repair: 173 scanned / 10 hit, median); a global
last-opaque-cover floor removes 0.00-0.01 layers per pixel beyond the per-batch floor in every cell; whole tiles
certified uniform in the repair set 0-0.6 % (heavy, lab), 10-54 % (vlist); of the 32x4 px blocks the vlist copy
moves, 67-79 % are one colour.

### Kernels after / before (`BOUGH_AB=6 <scene>_headless`: kernels vs `oracle.metal` in one process, 100 back-to-back frames, alternating; 3 runs x draws 60/120/180, n = 9, load ~50; pixels identical in every run)

| cell | after µs | before µs | ratio median [min..max] |
|---|---:|---:|---|
| heavy stream | 143.6 | 186.5 | 0.77 [0.77..0.79] |
| heavy scroll | 53.9 | 66.1 | 0.82 [0.81..0.82] |
| heavy both | 191.0 | 236.6 | 0.81 [0.80..0.82] |
| lab stream | 36.1 | 70.8 | 0.51 [0.51..0.51] |
| lab scroll | 27.6 | 33.2 | 0.83 [0.83..0.88] |
| lab both | 55.6 | 85.2 | 0.65 [0.65..0.65] |
| animate | 25.4 | 27.9 | 0.89 [0.86..0.92] |
| vlist scroll | 82.9 | 188.0 | 0.44 [0.42..0.46] |
| vlist fling | 81.4 | 185.1 | 0.42 [0.38..0.47] |
| vlist jump | 79.6 | 183.9 | 0.43 [0.41..0.44] |
| signal write (`micro_bench` SCENE=state, 2 tiles; 3 runs, load 35) | 16.0 | 17.6 | 0.91 |

Losses inside it: the repair part of some vlist frames is up to 1.17 (short lists pay the 256-window's extra
barriers); the copy itself is slower on small rects (lab ~9.4 -> 10.4 µs, heavy ~23 -> 29): the new `fs_copy` has
a floor of ~26-30 ns per tile whatever it skips. lab scroll (0.83) and lab both (0.65) miss the 0.6 target.

### Process GPU time by slope (`e2e.py`: gputime.dylib, 600 vs 3000 ticks, vlist 3600; 5 interleaved pairs, one run, load 41-44)

After/before in Kcycles: heavy 0.78 / 0.87 / 0.78, lab 0.60 / 0.91 / 0.65, animate 0.93, vlist 0.65 / 0.67 / 0.80
(spreads up to [0.45..0.97]). **In milliseconds: heavy stream 1.02, heavy both 0.96, lab 0.75 / 0.91 / 0.90, animate
0.93, vlist 1.03 / 1.03 / 1.10**: the GPU clocks down when it has less to do, so removed cycles do not become
removed milliseconds end to end.

### Latency, release -> pixels exist (`lat.py`: `BOUGH_LAT=<gap_us>`, spin on the event; before = `BOUGH_MSL=oracle.metal`; 3 interleaved runs, one session, load 70-100)

Back to back (1,500 frames): p50 after/before 0.69-0.94 on nine cells (heavy scroll 296 -> 203 µs, vlist jump 352
-> 259, lab stream 206 -> 169, signal 152 -> 143); heavy stream 1.03-1.24 (two sessions disagree: not resolved);
p99 is 3-20 ms in every cell, before and after: scheduler noise at this load, not kernels.
At a 4 ms cadence (300 frames): p50 is 460-870 µs for every cell including the 2-tile signal frame (463 before, 470
after); ratios 0.90-1.02 except vlist 1.07-1.17 (n = 300 x 3, not resolved). **At a display's cadence the round
trip is the GPU's wake and the driver thread's scheduling; the kernels' cycles are a minor part of it.**

### CPU (`instr.py`: 600 vs 3600 ticks, median of 5, interleaved, one run)

R0 after/before 0.993-1.007 and R1 0.947-1.007 on every non-idle cell (base vs `82b83dd072`'s Rust). The idle cells
are 200-1,000 instructions per frame with spreads from -22 to 2,767: not resolved. Signal-write frame
(`micro_bench` SCENE=state, slope OPS 2000 -> 8000, 5 reps): 3.7 K instructions per op at RENDER=0, 107.5 K at
RENDER=1 (committed build only).

### The frame path, call by call (`cargo build --profile dist -p bough-metal --example frameprobe`; `frameprobe 1500 2000`: one nop-kernel frame every 2 ms; one run per configuration, load 53-77)

| mechanism | process instr / frame (the 2 ms sleep's 18 K included) | calling thread, per call | other threads |
|---|---:|---|---|
| command buffer + encoder + commit per frame | 189 K | new encoder 17.7 K, commit 11.5 K, dispatch 3.9 K, command buffer 2.8 K, wait 2.6 K | two driver threads, 70 K + 62 K |
| the presenter's stream (16 armed ahead, event-released, signal per frame) | 109 K | **new compute encoder 16.6 K**, `setSignaledValue` 5.9 K, dispatch 3.0 K, encodeSignal 2.5 K, encodeWait 0.6 K, endEncoding 0.55 K, setPSO 0.14 K, setBuffer 0.13 K, command buffer + commit 0.3 K amortised | one driver thread, 56 K |
| the same without the per-frame signal (completion read from a word the kernel writes) | 91 K | as above without encodeSignal | 38.7 K |

So an on-demand frame costs ~72-90 K instructions whatever it draws: ~39 K is the driver's segment submission when
the event is released, ~17 K the driver's handling of the completion signal, 16.6 K creating the compute encoder
(one per frame, because an event wait is a command-buffer operation and cannot sit inside an encoder), ~6 K the
release. The real signal frame (104 K above RENDER=0) is ~14 K above that floor. Back to back, awaiting the
previous frame by yielding on the memory word instead of the event cost 700 K per frame (rejected as a wait).

### Gates and negative controls of `82b83dd072` (one run each)

`cargo test --release -p bough-heavy -p bough-scenes -p bough-vlist`: 31 passed, 1 ignored. With
`--features bough/sabotage_copy`: 18 failed (all 6 vlist scroll gates). `bough/mutant_cert_skip`: 18 failed;
`bough/mutant_cert_stale`: 16; `bough/mutant_cert_tile`: 18. Bin mutants as shader variants through the oracle
probe: items appended in reverse caught on lab, heavy, vlist; one item dropped caught on lab, heavy, animate, not on
vlist's three sampled frames. `cargo check --workspace --all-targets` passes.

## Not verified / known broken

* Pixel gates not rerun on the head commit (tooling only: a cold `BOUGH_LAT` branch in `Fused::draw`).
* wgsl-rs / wgpu-hal presenter: not touched, not run. Windowed GPU time and windowed latency: not measured.
* **Lane memory's CPU repair backend will break the certificates when merged**: anything that writes a surface's
  pixels without the kernels must zero that surface's `FCert` entries for the tiles it wrote (or the whole plane).
  No hook exists yet; `Fused::prepare_target` shows the zeroing.
* Horizontal scroll: a dx that is not a multiple of 32 keeps no certificates (all MOVE, plus the floor). Correct
  by the existing gates only; no scene scrolls horizontally.
* The certificate plane is 72 B per tile per touched surface (~160 KB at 1440x1600, in the residency set):
  resident-memory effect not measured.
* `reference_pixels` panics if the kernels' full redraw differs from `oracle.metal`. The first pixel-changing
  experiment (bit-exactness against the old shader is no longer required) must turn that into a report.
* My first "dropped bin entry" mutant left the slot unwritten and was NOT caught: the stale entry from the previous
  render was the right one. A probe that re-renders the same frame cannot see writes that are skipped.
* The pre-armed stream is untouched: the `kIOGPUCommandBufferCallbackErrorTimeout` after a process suspend
  (lane memory) is still there.
* No generated shader code was read (no Metal compiler on this machine to dump it).
* Not replayed by `independent-review`.

## Experiments (all same-bytes unless marked "floor")

| tried | result | verdict |
|---|---|---|
| copy without its per-thread binary search; through half textures; through sample-access textures | vlist whole 0.99-1.01 each | the copy is memory-bound; dropped |
| band (32x4) certificates without row masks | vlist scroll/jump 0.46-0.49 but fling 0.90, lab/heavy scroll 1.06-1.07 | certificates erode under any dy % 4 != 0; replaced by row masks |
| copy decision by every thread | heavy scroll 1.17, vlist 0.66 | rejected |
| ... by each band's first thread, shared through simd reductions | heavy scroll 1.20, vlist 0.66 | rejected (a block run by one lane costs the simdgroup) |
| ... by one thread for all 8 bands + barrier | heavy scroll 1.31-1.43, vlist 0.73-0.79 | rejected |
| ... by 8 lanes of one simdgroup + barrier | vlist 0.39-0.46, lab scroll 0.83, heavy scroll 0.91 | kept |
| f_shade through a generic inline call | lab stream 0.87 | superseded |
| glyph loop inline, `sh = pm * cov` | lab stream 0.60 but 35 pixels of animate differ (contracted to an fma) | kept with `sh` assigned in the branch: exact |
| per-thread survivor bit sets walked with ctz | lab stream 0.76 -> 0.62 | kept |
| survivor sets as AND of per-row and per-column sets built by 32 threads | heavy stream 0.90 -> 0.98, lab 0.62 -> 0.67 | rejected |
| 256-entry windows, hits compacted before staging | lab stream 0.62 -> 0.55 | kept (short lists pay: vlist repair up to 1.17) |
| one scalar mask word per 32 records | heavy stream 0.90 -> 0.86 | kept |
| 64-entry batches | heavy stream 1.07, lab 0.75 | rejected: steps cost |
| 8 KB more threadgroup memory | no change | occupancy is not bound by threadgroup memory |
| one barrier fewer per step (alternating count arrays), bin and tile | no change | rejected |
| bin: hits -> segments -> items by prefix sums | heavy stream 0.86 -> 0.77, lab 0.54 -> 0.51, animate 0.86 -> 0.89 | kept |
| floors: no atlas read; no glyph records; reject test only | lab stream 0.79; 0.47; 0.29 | located the cost |
| floors of the final tile kernel on heavy stream (160 µs): empty list 86, ink test + compaction 15, staging 17, masks 12, walk 29 | | the fixed part is now more than half |
| bin pass alone: fixed 5.1 / 2.4 / 3.0 µs (heavy / lab / animate), cull 1.2 / 1.9 / 1.2, instance append 4.8 / 0.4 / 1.1, run expansion 16.8 / 7.5 / 0 | | led to the prefix expansion |
| gputime slopes at 300 / 600 ticks | negative slopes in some pairs | use 600 / 3000 and the in-process A/B |
| a command buffer with no work | reports 1-170 µs | unusable as a zero |

Working cost model that predicted the kept results and none of the rejected ones in advance: these kernels are
bound by instructions per simdgroup execution (a block one lane needs is paid by the whole simdgroup, per
simdgroup), not by loads, barriers or threadgroup memory; large copies are bound by memory traffic.

## Underexplored and exotic avenues

1. **The signal-write frame without the GPU (two executors).** Mechanism: lane memory's CPU repair backend draws
   the damaged tiles when there are few; the GPU is not kicked. Buys: the 72-90 K-instruction floor above and
   the ~0.5 ms round trip at cadence, for 15-46 K (Opus: 7.5-23 K per tile on a CPU) and microseconds. Cost: two
   rasters that must not show a seam; certificates must be zeroed or written by the CPU side. Cheapest test:
   `BOUGH_REPAIR=auto` on lane memory's branch with `micro_bench` SCENE=state, instructions and `BOUGH_LAT`.
   Depends on: lane memory (raster, cost model), core (damaged-tile count is known before the frame).
2. **Fewer encoders: the frame as data in an encoder that already exists.** An encoder per frame exists only to
   put an event wait between frames. If K frames are released together (a frame's indirect arguments are zero
   until its slot is written, so unreleased frames in the same segment are no-ops), one release serves the
   frames that arrive within a window; the cost is a wasted no-op dispatch per unused frame. Buys at most the
   16.6 K encoder + part of the 39 K submission per coalesced frame; nothing for a lone frame. Kill: frameprobe
   mode with N encoders per wait.
3. **Completion without the event signal.** The tile pass's last threadgroup writes the frame number to a
   shared word; `sync` reads it (free at cadence) and falls back to short sleeps when it must block. Measured
   potential: 109 K -> 91 K per frame; the windowed publisher still needs a wake (poll with a 50 µs sleep, or keep
   the signal only when a publisher is waiting). Kill: windowed p99 gets worse.
4. **Skip the copy dispatch's and plan's fixed cost with a dense plan pass.** One thread per band, 32 bands
   per simdgroup, writes a plan byte per band (in `FCert`); `fs_copy` threads read one byte and return. Indirect
   arguments fit in the 24 spare bytes of the header. Predicted: vlist 83 -> ~50 µs, lab and heavy scroll under
   0.65; kill at vlist > 65. Then compact the tile list so all-SKIP tiles launch no threadgroup.
5. **Tile fingerprints (IDEAS-OPUS 3).** Hash the staged records of a tile in paint order; equal to the target
   surface's stored hash -> no shading, no write. Opus measured lab stream shading 448 tiles for 43 changed
   (ring 1). Predicted lab stream 36 -> ~15 µs. The hash must cover the atlas content a glyph reads. Kill: add
   hash + counter only to the A/B probe; drop if under 25 % match at ring 4.
6. **Per-run row test in the bin's segment step.** One compare where a segment's item count is computed (run
   origin y +- the strike's ascent/descent) makes a paragraph record cost its lines in the row, not all its lines.
   Needs the strike's vertical bound in GPU-visible data (text lane). Kill: bin pass alone on lab does not drop 30 %.
7. **Text as one entry per run with a per-column glyph index** (IDEAS-OPUS 7b): the tile reads its glyph range in
   O(1); the scan of 170-210 entries per tile becomes ~10. Now worth less than it looked: ink test + compaction is
   15 of 160 µs on heavy, 3 of 38 on lab.
8. **Glyph coverage resolved per pixel, not per record.** A band's pixels are covered by at most 2-3 glyph boxes;
   the floor is one atlas read per pixel (~800 simd instructions per tile against ~3,000 now). Shape: per run, a
   pen-sorted column index, each pixel block binary-searches its glyph. Predicted lab stream shading 7 -> ~3 µs.
9. **SDF experiments (none started).** (a) Analytic merged coverage: stage a tile's solid rects into one
   coverage evaluation per pixel by sorting edges per tile row (predict: heavy stream walk 29 -> 15 µs; kill if
   staging grows more than it saves). (b) Glyphs as distance bands: store per glyph a small set of line
   segments or a 1-D distance per scanline instead of atlas texels; coverage from distance needs no texture read
   and scales under transforms (predict: glyph step -30 %; kill on visible difference above one 8-bit step at
   1x, images required). (c) One distance evaluation for rounded rect + border + shadow of the same box (a card is
   three records over the same geometry): predict heavy stream staged records -30 %; kill if the merged record's
   path is paid by plain rects.
10. **GPU-side damage.** The tile pass already knows, per band, whether it wrote one colour; with fingerprints it
    knows whether a tile changed at all. A per-frame "changed tiles" bitmap written by the GPU would let the
    presenter hand the compositor a dirty rect (CALayer `setNeedsDisplayInRect:` has no IOSurface equivalent, but a
    second small surface could carry only changed bands), and would give core an oracle for over-damage (lab
    stream: 90 % of shaded tiles unchanged) without CPU cost.
11. **Phase-split encode on workers.** Encoding is 16.6 K + ~10 K per frame on whichever thread arms the stream;
    it already runs off the input thread when windowed (publisher). Splitting disjoint tile bands across encoders
    would add an encoder (16.6 K) per band: negative under this cost model. What can be split is the CPU work that
    feeds the slot (row records) and, with a CPU executor, the tiles themselves (disjoint by construction).
12. **Metal-specific paths worth keeping reachable from wgsl-rs** (each behind a capability type, none measured
    here): bindless resource ids and GPU addresses in a buffer (the bind table: the reason a pre-armed frame names
    no surface); shared events; residency sets; `simd_*` reductions and `thread_index_in_simdgroup`; indirect
    dispatch from a buffer the CPU writes; IOSurface-backed textures. Tile shaders / imageblocks, framebuffer
    fetch and memoryless targets belong to render passes, which the presenter does not use (RESULTS: a render pass
    costs a tiler parameter buffer; Opus K3 rejected them); they would matter only for a render-pass executor.
    Indirect command buffers cover draws and concurrent dispatch, executed through an encoder: they do not remove
    the encoder.
13. **wgsl-rs gaps known from reading, not from porting**: the vendored wgsl-rs has `subgroup_exclusive_add`,
    `subgroup_add`, `subgroup_max`, `subgroup_ballot`; this lane's kernels also need `subgroup_min`, count
    trailing zeros, `as_type` reinterpretation between half4 and u32x2 (WGSL has no f16 without the extension:
    the accumulator is half), byte-addressed stores into a struct array (`FCert.m[8]` as u8: WGSL has no u8), 64-bit
    GPU addresses and resource ids in a buffer (the bind table), `texture.write` to one of four textures chosen at
    run time, preprocessor defines for mutants, and control over contraction (the fma that changed 35 pixels).
14. **Windows over 4096 px** (Opus E7: panic at 5,184 px): the 128-bit tile row runs through bough-core's damage
    plane, `FRow.bits`, the tile pass's k-th-set-bit select and `fs_copy`. Shape: rows of 64-bit words with a
    summary word; the slot's row record becomes variable length. Depends on core.
15. **Atlas dirty-band upload** (Opus: 1.14 M instructions per new glyph vs 36.8 K for a 16-row band):
    `Fused::sync_atlas` takes a `dirty` flag; it needs a row range from bough-text.
16. **Certificates beyond the copy.** A band certified one colour needs no shading when the tile is repainted and
    its inputs for that band are unchanged (a fingerprint per band); a full redraw of a certified-constant window
    (theme change) is fills. And the compositor could be told a surface is opaque-constant in a region.
17. **Heavy stream's real lever is outside the kernels**: 1,120 tiles a frame (28 % of the window) are repaired
    and 41 µs of the frame is writing their pixels at memory bandwidth; Opus measured 34 % of them unchanged.

## Where I think the lead or the plan is wrong

* "Under 10 K instructions for a GPU frame" is not reachable in Metal by encoder or call tuning: the driver's
  submission on release (39 K) and the encoder (16.6 K) are not ours to remove, and earlier rounds already removed
  what was (RESULTS 14.2). The order of magnitude is in not kicking the GPU for small frames; that is lane
  memory's backend, and it should be the default the moment its cost model exists.
* GPU time in Kcycles is the wrong headline for anything a user sees. At a display's cadence the round trip is
  ~0.5 ms whatever the kernels do, and in milliseconds the end-to-end cells barely moved. Kernel cycles matter for
  energy and for frames that are large; latency is decided by the wake and the driver thread.
* The brief's candidates (2D binning, floors, retained index) were written from a decomposition nobody had
  measured. An hour of floors first would have saved the previous session's whole plan.
* Porting to wgsl-rs before the kernels settle doubles every experiment's cost; the experiments above took
  seconds each only because MSL compiles at run time. Port once, after fingerprints and the plan pass, unless
  wgsl-rs gets the same edit-and-run loop.
* Bit-exactness against the old shader was dropped, but it is what caught the fma contraction, a wrong barrier
  and both bin-order mutants in seconds. Keep an oracle per executor, with a stated tolerance, not none.

## Next (in order, with predictions)

1. Certificate hook for CPU-written tiles + `BOUGH_REPAIR=auto` measured on the signal frame with `frameprobe`
   and `BOUGH_LAT`: predicted 107 K -> 20-40 K instructions and p50 470 -> under 50 µs at cadence.
2. Dense plan pass + short-list path (a list of at most 128 entries keeps the old two-barrier step): predicted
   vlist 83 -> ~50 µs, lab scroll 0.83 -> ~0.65, vlist repair back under 1.0; with a rect-area x dy sweep.
3. Tile fingerprints: predicted lab stream 36 -> ~15 µs (0.2 of the original), heavy stream 144 -> ~110.
   Then the wgsl-rs port of the settled kernels, listing every gap as it is hit.
