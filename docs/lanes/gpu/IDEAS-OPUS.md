# IDEAS-OPUS: the tile kernels, the frame path, and fourteen formulations (lane gpu, round 6)

Opus collaborator, 2026-11-11, branch `opus/gpu` (from `lead/core-r6` 59e26e2f4c). I own no code; this file is the
deliverable. It answers lane `gpu` (Fable, branch `lane/gpu/round6`, including its **uncommitted** working tree as of
17:20: the certificate / inline-shading changes to `shader.metal` and `fused.rs` and card
`2026-11-11-r6-certificates-and-shading.md`) and lane `memory` (`lane/memory/round6` up to 389d37ba1e).

Every number carries its provenance: **measured** (by me, this session, command in the appendix), **measured, gpu
lane / memory lane / RESULTS §n** (someone else's run, quoted), or **estimated** (arithmetic from measured inputs,
stated). Machine: M3 Pro, macOS 15.7.4; load average 86 to 160 during my runs (other lanes building and testing), so
my wall-time and GPU-time figures are noisy; instruction counts are not. GPU and wall timings were taken under the
round's measurement lock (four short holds, each released within minutes).

Experiments I ran (sources are not committed; the essential code is in the appendix):

| id | what | where |
|---|---|---|
| E1 | a bounded persistent kernel polling a word of a shared buffer vs today's event-released stream: round trip, instructions, lifetime, effect on a second queue | `crates/bough-metal/examples/opus_probe.rs` (`poll`, `event`, `life`) |
| E2 | one 32x32 tile shaded on one P-core with NEON half arithmetic in the kernel's arithmetic (fused half blend, `RTNE(v*255)` store) | scratchpad `neon_tile.rs` |
| E3 | codec-style redundancy of real frames, per tile: shaded vs actually changed, constant, 2-colour, equal to a tile of the previous frame / of the same frame, distinct tiles per frame | `comparisons/bough-scenes/examples/opus_tiles.rs`, `comparisons/bough-heavy/examples/opus_tiles_heavy.rs` |
| E4 | IOSurface lock cost, a CPU tile write into an IOSurface, IOSurfaces with compressed-format fourccs | `opus_probe io` |
| E5 | `replaceRegion` of the 1024x512 R8 glyph atlas (whole vs a 16-row band); a linear texture over a shared buffer | `opus_probe atlas` |
| E6 | bough-core's CPU cull (`Bough::cull`) of one 32x32 tile on heavy | `opus_tiles_heavy` with `CULL=1` |
| E7 | a window wider than 4096 px | `scene_shot` with `SCENE=lab SCALE=5.4` |

---

# Part 1. Reading

## 1.1 `fs_tile`, line by line (base: `lead/core-r6` shader.metal 1468-1559)

**Finding the tile (once per threadgroup, every thread).**

```metal
uint lo = 0, hi = fs.nrows;
while (hi - lo > 1) { uint mid = (lo + hi) >> 1; if (frows[mid].prefix <= i) lo = mid; else hi = mid; }
...
for (; w < 3u; w++) { uint c = popcount(fr.bits[w]); if (k < c) break; k -= c; }
uint v = fr.bits[w];
for (uint j = 0; j < k; j++) v &= v - 1u;
```

`log2(nrows)` dependent loads of `FRow.prefix` (up to 6 on heavy's ~35 damaged rows), then a serial loop of up to
31 iterations to select the k-th set bit. Constant per frame, recomputed by all 256 threads of every tile. Small (tens
of cycles per tile, estimated) but free to remove: the bin pass, which runs one threadgroup per damaged row and has
`prefix` and `bits` in hand, can write the row's tile coordinates into a list as a side effect, and the tile pass
reads `tiles[i]` with one load (the CPU side stays O(rows), which was the reason for this design).

**The scan (per 128-entry step).** Threads 0..127 load one 16-byte bin entry each and test its ink:

```metal
uint4 en = bin[bi];
short4 ink = short4(as_type<short2>(en.y), as_type<short2>(en.z));
if (all(ink < th)) { float4 cl = f_clip(pool, h, en.w >> 20); ... f_stage_glyph / f_stage_rec ... }
```

One independent 16 B load per entry; nothing in the scan depends on the previous entry. Every tile of a row scans the
whole row bin. Measured by lane gpu's counters (decomp-stats, repair tiles, pooled over draws 60/120/180):

| row type | entries scanned per tile, median (p90) | hits per tile, median | hit/scan | executed layers per pixel |
|---|---:|---:|---:|---:|
| glyph-dense (lab stream) | 173 (208) | 10 | 0.063 | 1.34 |
| glyph-dense (heavy stream) | 124 (197) | 5 | 0.040 | 2.32 |
| flat (animate) | 21 (39) | 3 | 0.122 | 1.18 |
| flat (heavy scroll strip) | 50 (51) | 6 | 0.126 | 3.10 |

So a glyph-dense tile reads ~2.8 KB of bin entries to keep 10, a flat tile ~0.3-0.8 KB to keep 3-6. Since one step
holds 128 entries, both are one or two steps; the scan's cost is the step's barriers, not the bytes.

**Staging a hit: three serial memory levels, recomputed per tile.** A glyph hit (`f_stage_glyph` -> `f_glyph` ->
`f_glyph_rh`, 1015-1028, 1443-1466):

```metal
device const RunHeader& rh = ((device const RunHeader*)(pool + h.runs_off))[run & h.m_run];       // 32 B, needs en.x
int x = ((device const int*)(pool + h.gx_off))[slot & h.m_glyph];                                  //  4 B, needs en.w
uint e = ((device const ushort*)(pool + h.gent_off))[slot & h.m_glyph] & 255u;                     //  2 B, needs en.w
device const GEntry& en = *(... h.toks_off + (uint(rh.strike) & h.m_strike) * h.tok_stride + e * 32u); // 32 B, needs rh AND e
float px = floor(fma(float(dx), rh.scale, rh.origin.x + 0.5)) + en.geo.x;
p0 = float2(px, rh.origin.y + en.geo.y) + f_space(pool, h, rh.space);                             //  8 B, needs rh
...
if (f_xf(pool, h, f_run_space(pool, h, run), xm, xk, xt)) ...                                      // 32 B FXf, needs rh.space
```

plus the clip (`f_clip`, 8 B, needs `en.w`). Seven loads, about 118 B, dependency depth three after the entry load
(entry -> header/slot/clip -> strike entry/space/transform). An instance hit (`f_stage_rec`, 1408-1441) is entry ->
`FInst` (48 B) and clip -> space (8 B) and `FXf` (32 B): four loads, ~96 B, depth three. Everything staged is a pure
function of the record **except `cover`**, which compares the clipped quad with the tile origin `o`. So a record that
touches k tiles of a row is staged k times (a row-wide background quad: once per tile of the row), and the glyph pen
formula, already evaluated by `fs_bin` to compute the glyph's ink, is evaluated again by every tile that hits it:
`fs_bin` keeps only the floored ink (`f_qink`) and discards the exact quad.

The transform probe is unconditional: `f_xf` loads 32 bytes and does six compares for every staged record and every
glyph, in every tile, when the frame has no transformed space at all (a per-frame constant).

**Compaction and barriers.** Each 128-entry step runs `compact` (simd prefix sum, one threadgroup store, a barrier,
eight threadgroup loads) and three `threadgroup_barrier`s in total, then writes the staged `FRec`s. `FRec` is 144
bytes (computed from its layout: float4 q, core, b; half4 cv, pm, pm2; radius, bw, uv, kind; float2 gc; float4 xi;
float2 xk, xt; float xs; padded to 16), so `threadgroup FRec rec[128]` is **18,432 bytes of threadgroup memory per
tile threadgroup**, a third of it fields that only transformed records, images, gradients and shadows use (`gc`,
`xi`, `xk`, `xt`, `xs`, `pm2`: 52 of 144 bytes, plus 12 bytes of padding).
Its effect on occupancy is not measured; it is the first thing I would check (formulation 9).

**Shading: the loop-carried chain.**

```metal
for (uint k = k0; k < kend; k++) {
    threadgroup const FRec& rr = rec[k];
    if (rr.q.x > blk.z || rr.q.z <= blk.x || rr.q.y > blk.w || rr.q.w <= blk.y) continue;
    if (blk.x >= rr.core.x && ...) { half4 cv = rr.cv; for (j) acc[j] = cv + acc[j] * (1.0h - cv.a); continue; }
    for (uint j = 0; j < 4; j++) { half4 sh = f_shade_x(rr, c00 + ..., atlas, img); acc[j] = sh + acc[j] * (1.0h - sh.a); }
}
```

The dependency carried from one entry to the next is `acc[j]`: one fused half multiply-add per channel per record per
pixel (the memory lane's CPU model reproduces the output bit for bit only with this blend as one half FMA). Each
thread carries four independent chains (its 2x2 block); chain length is the executed layers per pixel, 1.2-3.1
(table above), so the arithmetic chain is short. What the chain serialises is **latency**: a glyph record's four
`atlas.read`s must return before its four FMAs, and the next record's reads cannot be issued earlier because the loop
body is data dependent (`continue`s), so every glyph layer of a block pays one texture-read latency in series. Lane
gpu's uncommitted version replaces this loop with a per-thread bit mask of records meeting the block and walks set
bits, with glyphs and the base kinds shaded inline instead of through `f_shade_x` (measured by them: lab stream 71 ->
43 us from the inline glyph path, 54 -> 44 us from the bit walk).

**The opaque floor and what it misses.**

```metal
cover = !xf && in.kind == 0 && (in.color >> 24) == 255u && qa.x <= float(o.x) + 0.5 && ...;   // f_stage_rec
uint fl = simd_max((rh && cover) ? rs + 1u : 0u);  ...  k0 = k0 > 0 ? k0 - 1u : 0u;              // per step
```

1. It is computed **per 128-entry step after staging**: a cover in step 2 cannot cancel step 1, which was already
   shaded into `acc`, and everything below the cover inside its own step was loaded and staged before the floor was
   known.
2. Only kind 0 qualifies. A rounded card (kind 1) whose `core` box (coverage provably 1) holds the whole tile, or a
   border (kind 2) whose opaque fill `pm2` does, is an exact cover too and is not used.
3. It is per tile, never per 2x2 block, though the shading loop already tests `core` against the block: a record
   whose core holds the block and whose `cv.a` is 1 is an exact floor **for that block** (start `acc` at `cv`, skip
   every record below it for this thread).
4. Any transformed space disables it, including a pure translation carried in `FXf.t`.

Lane gpu measured the payoff of a *global* floor on its scenes at 0.00-0.01 layers per pixel, so items 1 and 4 do not
pay on heavy/lab/vlist. Items 2 and 3 are about interiors of cards and panels, and every one of them matters on UIs
with stacked opaque surfaces (sheets, popovers, sidebars), which the benchmark scenes do not have (formulation 10).

## 1.2 `fs_bin` (1052-1222)

L1 tests every directory entry against the row band (`dink[di] < th`, 8 B each, 256 per step), L2 the eight group
inks of each hit page, L3 the eight record inks of each hit group. Two things cost more than they need to:

* **`f_pg_xf` reloads the page affine (24 B) and tests it in every group thread and every record thread**
  (`gh = f_pg_xf(pg) ? ... : ...;` in L2, 8 times per page; again `if (f_pg_xf(pg))` in L3, once per record thread
  of every hit group, up to 64 more): a per-page constant.
* **A text record's runs are expanded with no per-run row test.** A multi-line text node is one record with
  `pay_runs(first, cap)`, up to 2,048 runs (one per line; `bough/src/text.rs:570, 628, 766`). The bin threadgroup of
  every damaged row that the record's ink union touches does

  ```metal
  for (uint run0 = r0; run0 < r0 + rn; run0++) {
      ... uint rlen = min(rhd.len, h.m_glyph + 1u) ...
      for (uint g0 = 0; g0 < rlen; g0 += 256) { ... f_glyph(...) ... threadgroup_barrier(...); compact(...); ... }
  ```

  i.e. every glyph of every line of the paragraph is loaded (RunHeader 32 B, slot 6 B, GEntry 32 B, space 8 B, FXf 32 B
  and the space again inside `f_xf`) and tested against the row, with two barriers per run, although only the one or
  two lines whose baseline band meets the row can hit. Per damaged row the work is O(glyphs in the record), not
  O(glyphs in the row). For lab's streamed paragraph (920 x 160 pt box, ~11 lines) that is ~11 cooperative steps per
  row where 1-2 would do (estimated); for an editor whose visible text is one record of 60 lines it is ~30-60x
  (estimated). A line's vertical extent is `origin.y` plus the strike's ascent/descent bound: one compare per run,
  uniform across the threadgroup, exact as a conservative superset.

## 1.3 `fs_copy` (1566-1590)

One threadgroup per tile of the destination span; a binary search over `frows` to skip repaired tiles; 32 x 8 threads
x 4 rows moving one pixel each (read 4 B, write 4 B). Measured by lane gpu: the copy is 75-85 % of a vlist scroll frame
(e.g. 201 of 232 us); its certificates (SKIP / FILL / MOVE per 32x4 band) cut vlist scroll whole-frame time from ~205
to ~86 us (ratio 0.41-0.47, measured, gpu lane, uncommitted). RESULTS §15 already bounded the idea: a copy reads and
writes every pixel, so moving pixels costs about what drawing simple content does.

## 1.4 One damaged tile, side by side

| | glyph-dense row (lab stream) | flat row (animate) |
|---|---|---|
| bin entries scanned | 173 (one or two 128-entry steps; 2.8 KB) | 21 (one step; 0.3 KB) |
| reads per scanned entry | one 16 B load, independent | same |
| hits staged | 10, each 4-7 loads (~96-118 B), depth 3, recomputed in every tile the record touches | 3 |
| barriers | 3 per step (base); the gpu lane's version 3-4 per step | same |
| threadgroup memory | `rec[128]` = 18.4 KB | same |
| per pixel | 1.34 layers: block test, core test, glyph = atlas read then FMA, serialised per record | 1.18 layers, mostly `core` fills |
| GPU floor (gpu lane's floor columns) | a tile pass doing no work over the same 156 tiles: 13.3 us | the same over ~122 tiles: 6.4-8.2 us |

The floor scales with the tile count (13.3 us at 156 tiles, 31-35 us at ~1,100 on heavy: roughly 30-85 ns of launch
per tile threadgroup), and an encoder with empty dispatches costs 1-4 us at its minimum (its medians are noisy:
"empty" column, measured, gpu lane). Lab stream's whole frame is 38-39 us after lane gpu's changes (measured, gpu
lane). For a one-tile frame the GPU's own time is therefore small; what a one-tile frame pays is on the CPU and in
latency (1.5).

## 1.5 The CPU frame path of a one-tile change (`App::frame` -> `pick` -> `App::draw` -> `Fused::draw`)

Input thread, in order:

| # | call | where | cost | avoidable? |
|---|---|---|---|---|
| 1 | `IOSurfaceIsInUse` per candidate surface (up to 3) | `pick` -> `in_use` -> `Fused::is_in_use` | small (not measured) | mostly: the publisher already knows what it holds |
| 2 | `sync_atlas`: `replaceRegion` of the **whole** atlas if any byte is dirty | `App::draw`, fused.rs 1030-1057 | **1,144,989 instructions, 220.6 us** per call (measured, E5); a 16-row band: 36,776 instr, 1.3 us | yes: dirty rows only (31x), or no upload at all (formulation 6) |
| 3 | `sync()`: `ev.signaledValue()` | fused.rs 872 | ~72 instr (RESULTS §14.2) | it is the wait that guards region writes; cheap when the GPU is done |
| 4 | `check_bins`: `self.counts.contents()` (an Objective-C message per frame) | fused.rs 890 | small | yes: the pointer is constant for the buffer's life |
| 5 | header `n_dir` store, `write_slot` | | O(damaged rows) | no (it is the frame) |
| 6 | `release`: `kern.ensure(f)` (one atomic), **`ev.setSignaledValue(2f-1)`** | fused.rs 1249-1269 | ~2.8 K (RESULTS §14.2); the GPU's wait on it is resolved in the kernel on the driver's submission thread, ~50 K a frame | only by not using the GPU for the frame (E1 kills the shared-memory alternative) |
| 7 | `after_gpu_frame`: `CACurrentMediaTime`, `warm.arm` = `dispatch_after` with a boxed closure when unarmed | fused.rs 1195-1209 | one allocation per arm (memory lane's trace) | yes: one standing timer, or no keep-warm (formulation 1) |
| 8 | `post`: `counts.contents()` again, `CFRetain` of the IOSurface, mutex, condvar notify (a syscall when the publisher sleeps) | fused.rs 1187-1193 | not measured | the wake is the price of publishing off the input thread |

Publisher thread: `signaledValue`, then **`waitUntilSignaledValue`** (blocks until the GPU signals), the overflow word,
then `CATransaction begin / setDisableActions / setContents / commit` (~80-84 K instructions, RESULTS §12.3 and §12.10),
then `kern.ensure`: every 8 frames it encodes 16 frames (`commandBufferWithUnretainedReferences`, per frame
`encodeWaitForEvent`, `computeCommandEncoder`, `setBuffer`, `setBytes`, three `setComputePipelineState`, three indirect
dispatches, `memoryBarrierWithScope`, `endEncoding`, `encodeSignalEvent`, then one `commit`): about 13 messages a
frame, amortised. Driver threads: the submission thread blocks inside `IOGPUCommandQueueSubmitCommandBuffers` until
the event value arrives and submits the next segment (~50 K a frame), plus a completion/event thread.

Measured totals for this path without Core Animation: **101 K process instructions per frame** (`fprobe`, RESULTS §14.2:
45 K main, 52 K driver threads) and **86.4 K per release** in my E1 event probe (one trivial kernel per release, arming
on the same thread). Measured latency of a release through the stream to a GPU-written acknowledgement seen by the
CPU (E1): **p50 76-101 us back to back, 396-444 us when frames are 1-8 ms apart** (the realistic UI cadence: the GPU
has clocked down between frames), p99 up to 3.3 ms. After the GPU has slept (no work for ~1.25 s): 2.4-3.3 ms and
8.4-11 M instructions for the first submission (measured, memory lane `drvprobe`; RESULTS §13.3 9.9-10 M, 3.1-4.1 ms).

What a one-tile frame cannot avoid on this design is the event release plus its kernel-side submission and the CA
transaction. Everything else in the table is small or a defect (2, 4, 7). What a one-tile frame could avoid entirely
is the GPU: shading the tile on the CPU costs 7.5-23 K instructions (E2) plus 2.1 K for the cull (E6) plus 5.3 K for
the IOSurface lock (E4).

## 1.6 The five findings from the reading that would save the most

1. **Small frames pay the GPU's fixed costs, and the presenter has no other executor.** One damaged tile costs
   86-101 K process instructions and a ~400 us round trip at UI cadence (2.4-3.3 ms and ~10 M instructions after the
   GPU sleeps, and the 68 MiB driver block kept charged by keep-warm); shading that tile on a CPU core costs 15-31 K
   instructions including cull and lock, and microseconds (measured inputs, estimated sum). Formulation 1.
2. **Nothing asks whether a damaged tile's inputs changed.** Measured (E3, ring 1, so no ring-age inflation): lab
   stream shades 448 tiles a frame and the pixels of 43 change (90 % of shaded tiles come out identical); heavy stream
   992 vs 657 (34 %); animate 116 vs 85 (27 %). Formulations 2 and 3.
3. **`fs_bin` expands every line of a text record for every damaged row, and `fs_tile` re-derives every glyph's quad in
   every tile.** No per-run row test (records carry up to 2,048 runs); the bin keeps only the floored ink, so each tile
   re-runs the three-deep load chain and the pen formula. Formulations 7 and 9.
4. **The glyph atlas is re-uploaded whole on any new glyph**: 1.14 M instructions and 220 us per frame that rasterises
   a glyph (typing a new character, CJK input, a first paint), about ten times the whole GPU frame path. Formulation 6.
5. **Windows wider or taller than 4096 px panic** (`window up to 4096x4096`, bough-core damage.rs:335; measured with
   lab at 5184 px, E7). A full-screen window on a 5K Studio Display (5120 px) or a 6K Pro Display XDR (6016 px) is
   that case. The 128-bit tile row runs through the damage plane, `FRow.bits`, `next_set`/`span_mask` and `fs_copy`.
   Formulation 5.

---

# Part 2. Formulations

Ranked by expected gain over the ten situations the lead named. A mark means the formulation changes that situation
materially; it is a judgement from the measurements below, not a measurement.

| # | formulation | caret | hover | typing | text-dense scroll | resize | theme | one animated element | video / webview | static | 6K |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 1 | two executors, one arithmetic (CPU NEON tiles / GPU), chosen per frame with the GPU's wake state | x | x | x | | | | x | | x | |
| 2 | damage from bytes: the write diffs the GPU-visible column | | | x | x | | | x | | | |
| 3 | per-surface tile fingerprints: an unchanged tile is never shaded or written | x | x | x | | | x | x | | | x |
| 4 | the window as layers: compositor scrolling, sprite overlays, foreign surfaces | x | x | | x | | | x | x | | x |
| 5 | no window-size limit: two-level tile rows | | | | | x | | | | | x |
| 6 | zero-copy glyph and image atlases | | | x | x | | | | | | |
| 7 | text binned by line, glyph ranges from the pen column | | | x | x | x | x | | | | x |
| 8 | per-tile mode decision (SKIP / FILL / MOVE / INTRA / PAINT) | | | | x | x | x | | | | x |
| 9 | stage once per row; a 64-byte staged record | | | x | x | x | x | x | | | x |
| 10 | the opaque floor done right | | | | x | x | x | | x | | |
| 11 | one reference surface plus residual layers | x | x | x | | | | x | | x | x |
| 12 | the small-frame GPU floor | x | x | x | | | | x | | | |
| 13 | compact encodings: line header + residual glyphs, 24-byte quads | | | x | x | x | x | | | | x |
| 14 | retained raster caches as tile pools | | | | x | x | | | | | x |

Killed or not established by my experiments, recorded at the end: **K1** zero Metal calls through a GPU that polls
shared memory (killed, measured); **K2** lossless-compressed IOSurfaces as a memory lever (not established); **K3**
render passes / tile memory for the raster (rejected on the memory lane's measured +30 MiB driver block).

---

### 1. Two executors, one arithmetic: NEON tiles on the CPU for small damage, the GPU for large, chosen per frame

* **Rule bent:** "the renderer is the GPU". The renderer is the arithmetic (premultiplied half colour, the blend as
  one half FMA, `RTNE(v*255)`), and any executor that reproduces it bit for bit may run a frame.
* **Representation:** a `Repair` choice per frame over the same tile list and records: (a) CPU: cull per tile with
  bough-core's existing `Bough::cull` (SIMD, page summaries), expand text lines that meet the tile, shade record-major
  into an 8 KB L1-resident planar half accumulator, store BGRA8 into the target IOSurface under `IOSurfaceLock`;
  (b) the GPU stream as today. The cost model: `tiles x cpu_per_tile` against `gpu_fixed + tiles x gpu_per_tile +
  wake(state)`, where `wake` is ~0 while warm and 2.4-3.3 ms after ~1.25 s idle. The lane memory card
  `2026-11-11-repair-backends` is this mechanism; this entry adds numbers and the wake-aware policy.
* **Wake-aware, not keep-warm:** with a CPU executor, keep-warm (a blit every 0.8 s for 10 s after each interaction,
  0.54 M instr/s, the 68 MiB driver block charged throughout; RESULTS §13.3, memory ledger) can go. Small frames run
  on the CPU; the first input event of a burst issues one asynchronous 4-byte blit to wake the GPU *speculatively*
  while the CPU serves the first frames; the queue is released after an idle threshold (the memory lane found
  releasing the queue returns the 69 MiB within 55-300 ms).
* **Saves (measured inputs, estimated totals):**
  * per one-tile frame: GPU path 86-101 K process instructions + CA; CPU path cull 2.1 K (E6) + shade 7.5-23 K (E2) +
    lock 5.3 K (E4) = 15-31 K + CA: **3-6x fewer instructions** before the CA commit (estimated);
  * latency to pixels written: GPU round trip p50 396-444 us at 1-8 ms frame gaps (E1) vs 0.5-6 us per tile on one
    core (E2, wall time under load 90) plus 0.4-0.6 us lock: **~20-100x lower** (estimated); after a GPU sleep,
    2.4-3.3 ms vs the same microseconds;
  * footprint of an app whose frames stay small: a process that only writes an IOSurface from the CPU sits at 6.5
    MiB vs the counter's 122.5 MiB (memory lane, measured): **~10-20x** for the counter, the caret-blink window, an
    idle chat window.
* **Crossover (estimated from E1/E2/E6):** on instructions, single thread, `(86 K - 5.3 K) / (2.1 K + 7.5..23 K)` = 3-8
  tiles; on latency against a GPU at UI cadence (~400 us), ~70-200 tiles on one core at 2-6 us per text tile, a
  multiple of that on several cores. A caret blink is 1-2 tiles, a hover 2-6, a
  typed character 2-4 (glyph, caret, reflow of the line's tail), a counter click 1-4. Scroll, resize and theme change
  stay on the GPU.
* **Cost when unused:** one comparison per frame; the CPU rasteriser's code (~10-20 KB, estimated).
* **Overfit risk:** the modelled kinds. The memory lane's CPU model is bit-exact for solids and glyphs (0 mismatching
  tiles on 3,000 random translucent quads + 400 labels at quarter-pixel x, 4 seeds; the two-rounding blend control
  fails 2,658-2,669 tiles). Rounded boxes (`f_sd`'s `length` = sqrt of an f32 dot), borders, gradients, shadows
  (`exp`, the erf polynomial), images (bilinear) and transforms are not yet modelled: a tile with an unmodelled kind
  forces the GPU for the frame, so apps whose small frames involve shadows (a hover that lifts a card) would never
  use the CPU path until those kinds are modelled. Each kind added needs its own oracle.
* **Exactness:** unchanged gate: the presented frame equals the GPU full redraw. Gate it with `BOUGH_REPAIR=cpu`
  forced on every pixel test, a start-up self-check (a fixed probe scene both ways, cached per device + OS build,
  disabling the CPU path on mismatch), and the two-rounding blend as the mutant. The risk to name: Metal does not
  promise `sqrt`/`exp` bit patterns across OS releases, so modelled transcendental kinds need the self-check; solids,
  glyphs and the store need none (IEEE operations only).
* **Kill experiment:** caret blink and one typed character on the counter window, windowed (borderless, never key):
  input-to-surface-written latency and process instructions per frame, CPU path vs GPU path, at 2 Hz and at 10 Hz,
  plus footprint at t = 3 s and 9 s. It dies if the CPU path is not at least 3x lower in latency at 2 Hz or if the
  IOSurface written by the CPU shows torn or stale content in the published ring (it should not: the target is never
  the surface on screen).

### 2. Damage from bytes: the write diffs the GPU-visible column

* **Rule bent:** "a write damages the ink of the record it wrote". The pixels depend on the bytes the kernels read
  (glyph slots `x: i32, ent: u16`, line headers, 48-byte instances), so the damage of a write is the ink of the slots
  whose bytes changed, not the ink of the node whose description changed.
* **Representation:** the text and instance writers compare the new bytes with the old ones before storing
  (NEON, 16-64 B per compare; a 1,000-glyph paragraph is 6 KB, ~400 compares, ~1-2 K instructions, estimated); a line
  header that changed (origin, scale, colour, clip, space) damages the line; a slot that changed damages old ∪ new ink
  of that glyph; equal bytes damage nothing. bough-text already detects equal and shifted *lines*
  (`lines.rs::set_text`); this goes one level down and, for shifted lines, damages the line's old and new extents only
  where the glyph bytes relative to the line differ.
* **Saves (measured opportunity, E3, ring 1):** tiles shaded vs tiles whose pixels changed, per frame:

  | scene | shaded | changed | shaded but identical |
  |---|---:|---:|---:|
  | lab stream (streamed paragraph) | 447.6 | 43.4 | 90.3 % |
  | heavy stream | 992.5 | 657.1 | 33.8 % |
  | heavy both | 1,856.5 | 1,508.6 | 18.7 % |
  | animate | 116.0 | 84.6 | 27.1 % |
  | micro vscroll (ring 1, no copy) | 2,250 | 1,992.6 | 11.4 % |
  | blink | 2.0 | 2.0 | 0 % |

  On lab stream that is up to ~10x fewer tiles, bins and rows; whether glyph diffs recover most of it is the
  experiment below (the gap could also come from layout re-placing equal glyphs, which the diff would catch, or from
  records other than text).
* **Cost when unused:** a compare before each column store, ~1 instruction per 4-16 bytes written (estimated); writes
  that always change (an animated colour) pay it for nothing.
* **Overfit risk:** low for text (every editor, chat, log and label update); moderate for instances (a moved quad
  changes its bytes whatever happens, so no gain for motion).
* **Exactness:** exact by construction if every input a pixel reads is either diffed or forces damage: glyph slot,
  line header, strike entry (a re-rasterised glyph changes its atlas texels, so the strike token's generation must be
  part of the compared bytes), clip, space, transform. The existing `edges.rs`-style oracle applies: random edits,
  every pixel whose value changes must lie in a damaged tile. Mutant: drop the line-header compare.
* **Kill experiment:** in lab stream, log per epoch (a) today's damage, (b) the tiles of the glyph-slot byte diff,
  (c) the tiles whose pixels change (E3 already measures (a) and (c)). If (b) is not below 2x (c), the over-damage is
  elsewhere and this formulation is wrong for lab.

### 3. Per-surface tile fingerprints: an unchanged tile is never shaded or written

* **Rule bent:** "a damaged tile is repainted". A damaged tile is a tile whose inputs *may* have changed; whether they
  did is decidable from the inputs themselves, after the cull and before shading.
* **Representation:** each surface keeps a 64-bit fingerprint per tile (8 B x 2,700 tiles = 21 KB per surface at lab's
  size): a hash of the staged inputs of the tile, in paint order (kind, edges, colours, uv, clip, the strike token's
  atlas generation). The tile pass computes it from what it has just staged (a few hundred ALU per tile, estimated);
  equal to the target surface's stored fingerprint -> skip shading and the write; otherwise shade and store the new
  fingerprint. This is Forma's `tile_unchanged` pass (research gpu-binning-simd §2) on the GPU, and the general form of
  lane gpu's constant-colour certificates (a certificate is a fingerprint whose value is "one colour").
* **Saves:** shading (44-62 % of the frame on lab and heavy before lane gpu's changes, measured, gpu lane) and the
  tile's 4 KB write for the "shaded but identical" fraction above (34-90 % on stream scenes, measured E3); it catches
  every source of over-damage at once (ring age, conservative marks, re-descriptions that write equal values, a
  toggled colour that comes back). It does not save the bin pass or the scan.
* **Cost when unused:** one 8-byte load and compare per tile, and the hashing ALU; a write of 8 B per shaded tile.
* **Overfit risk:** low; the gain is zero where damage is already exact (blink, scroll strips).
* **Exactness:** the kernel is deterministic, so equal inputs give equal pixels; the residual risk is a 64-bit
  collision (2^-64 per comparison), the standard RESULTS §12.7 (Zobrist tile hashes) already accepted. Fingerprints
  must be per surface (each surface holds a different age) and invalidated with the surface (resize, purge, trim).
  Gates: every ring age; mutants: omit the atlas generation from the hash (must fail on a re-rasterised glyph), keep a
  surface's fingerprints after a purge.
* **Kill experiment:** add the fingerprint and a counter only (no skip) to lane gpu's A/B harness on lab stream and
  heavy stream: if fewer than 25 % of shaded tiles match, drop it.

### 4. The window as layers: compositor-side scrolling, sprite overlays, foreign surfaces

* **Rule bent:** "the window is one surface". Core Animation already composites layers at integral device-pixel
  positions by copy; motion that is a pure translation of retained pixels can be a layer position, not a pixel move.
  This is the codec's motion vector handed to the decoder (WindowServer) instead of applied by us.
* **Representation:** a scroll container is its own `CALayer` whose IOSurface is the container's content plus an
  overscan margin, laid out toroidally in y (two sublayers for the wrap); a scroll is a change of the sublayers'
  positions in the same transaction as the repaint of the exposed strip. Small oscillating marks (the caret, a hover
  highlight, a focus ring) are sprite layers whose `hidden` flag or position changes; video and webview content are
  foreign IOSurfaces as sublayers, never copied (WebRender's compositor surfaces, research web-engines).
* **Saves:** the scroll copy, which is 75-85 % of a vlist frame (measured, gpu lane), and its bandwidth (8 bytes per
  pixel moved: a full-viewport scroll at 6K moves 163 MB, ~1.6 ms at ~100 GB/s, estimated); a caret blink becomes one
  CA transaction with no pixel written at all.
* **Cost when unused:** none (one layer). When used: one layer per scroll container in WindowServer's composite,
  overscan memory, and the static content above or beside a scroll area must live in its own layer.
* **Overfit risk:** sticky headers, translucent overlays above scrolled content, and nested scrolls each add layers;
  a scroll that is not integral in device pixels (smooth scrolling at fractional offsets) is not a copy and must be
  repainted (as today).
* **Exactness:** the presented frame is the composite. It equals a full redraw iff every layer is opaque where it
  covers, placed at integral device pixels, `contentsScale` equal to the window's, no filtering. The gate needs a new
  oracle: compose the layers in software (rectangle copies) and compare with the full redraw, plus one windowed
  capture check per OS build that CA's composite equals the software composite. That is a change of what the gate
  compares (from "the surface" to "the composite"), legitimate because the composite is what the user sees.
* **Kill experiment:** vlist scroll as one container layer, windowed (borderless, never key, off the active Space):
  GPU time per frame and WindowServer GPU time (macsample) vs today's copy path. Dies if WindowServer's added time is
  more than the copy it removes.

### 5. No window-size limit: two-level tile rows

* **Rule bent:** "a tile row is one `u128`" (damage.rs: "windows up to 4096 x 4096 px ... so a tile row is one
  `u128`").
* **Representation:** a row is a summary word plus 64-bit words (occupancy summary bit w set iff word w is non-zero),
  type-stable at the window's maximum (6K: 188 columns = 3 words + summary; 8K: 4); `FRow` carries the summary and a
  word offset; `next_set`/`span_mask` walk the summary first. Marks stay "two shifts" for spans inside one word.
* **Saves:** a crash. Measured (E7): lab at 5184 px wide panics `window up to 4096x4096`; a full-screen window on a
  5K Studio Display (5120 px) or a 6K XDR (6016 x 3384 px = 20.4 Mpx, 188 x 106 = 19,928 tiles) is that case.
* **Cost when unused:** one summary test per row operation (estimated a few instructions per marked row).
* **Overfit risk:** none; it removes a special domain.
* **Exactness:** representation only; the existing plane oracles (`shift_within`, marks) extend with random wide
  windows. Mutant: a summary bit not set for a non-empty word must fail the scroll-copy oracle.
* **Kill experiment:** none needed for the crash; to price it, the R0 slope cells before/after (must stay within
  1.02x).

### 6. Zero-copy glyph and image atlases

* **Rule bent:** "the atlas is a texture we upload to". On Apple silicon a linear texture can sit on a shared buffer
  (E5: an R8 1024x512 linear texture over a `storageModeShared` buffer was created, row alignment 16); the CPU
  rasteriser writes glyph masks in place.
* **Representation:** the atlas becomes a region column (or a separate shared buffer in the residency set), append-
  only per cell: a new glyph is written into a cell no pending frame references, before the frame that references it
  is released; a cell is reused only after the frames that referenced it are done (the stream already tells when). No
  `replaceRegion`, no CPU-side copy kept beside the texture.
* **Saves (measured, E5):** `replaceRegion` of the whole atlas: 1,144,989 instructions and 220.6 us per frame that
  dirties it (today's behaviour on any dirty byte); a 16-row band: 36,776 instructions, 1.3 us; in place: 0. Memory:
  the atlas's CPU copy (0.5 MiB) and the image atlas's (4 MiB on the dashboard; memory ledger: "image atlas CPU copy
  + texture 8 MiB") are halved. (The atlas texture is created with the default storage mode, which is Managed here,
  E5.)
* **Cost when unused:** none. The kernels' `atlas.read` on a linear texture may be slower than on a twiddled one
  (2D locality); not measured.
* **Overfit risk:** none for text; it matters most for CJK input, emoji, first paint and zoom (new strikes).
* **Exactness:** the texels read are the same bytes; the hazard is a cell rewritten while a pending frame reads it,
  closed by the append-only rule. Mutant: reuse a cell one frame early (must fail a gate that alternates two glyph
  sets in one cell).
* **Kill experiment:** lab stream's tile pass with the atlas as a linear texture vs twiddled (lane gpu's A/B
  harness); dies if the tile pass loses more than the upload saved at the observed rate of dirty frames.

### 7. Text binned by line, glyph ranges from the pen column

* **Rule bent:** "the bin holds glyphs". A line's glyphs are already sorted by pen x (monotone for each run's
  direction), so the column is a 1D spatial index; binning per glyph throws that away.
* **Representation:** (a) in `fs_bin`, a per-run row test before expanding (`origin.y` against the row with the
  strike's ascent/descent bound: one compare per run); (b) for each run that meets the row, the bin pass writes one
  entry for the run plus, per tile column the run spans, the first glyph index of that column (found in parallel: the
  glyph thread whose pen crosses a column boundary writes its index there: one store, no atomics, no sort); the tile
  pass reads its glyph range in O(1) and stages only those glyphs. Paint order holds: glyphs keep slot order within a
  line, lines keep record order.
* **Saves:** (a) alone turns the bin pass's run loop from O(glyphs in the record) to O(glyphs in the row) per
  damaged row (~5-10x on lab's paragraph, ~30-60x on an editor-sized record, estimated; lane gpu measured the bin pass
  at 12-16 % of lab and heavy frames); (b) shrinks a text row's bin from one 16 B entry per glyph to one per line plus
  2 B per column, and the per-tile scan from ~173 entries to ~10 (estimated).
* **Cost when unused:** none for non-text rows.
* **Overfit risk:** right-to-left and mixed-direction runs (pen x decreasing or not monotone): a per-run flag selects
  the reverse index or the existing per-glyph path. Glyphs whose ink overhangs the pen interval (italics, kerning):
  search with the strike's maximum bearing as a margin (a conservative superset; the staging test culls the extra).
* **Exactness:** the set of glyphs staged per tile is a superset of today's, in the same order; the existing
  `runs_check` (runs vs instances byte for byte) and lab/heavy gates apply; mutant: margin 0 on an italic run.
* **Kill experiment:** (a) alone, on lab stream and heavy stream in lane gpu's A/B harness: bin-pass time. Dies if the
  bin pass does not drop by at least 30 % on lab.

### 8. A per-tile mode decision (the codec's mode table)

* **Rule bent:** "a damaged tile is shaded or copied". Per tile the presenter can choose, by cost, SKIP (the target
  holds it: formulation 3), FILL (one word: constant colour), MOVE (copy from another surface at a vector: today's
  scroll copy), INTRA (copy a tile already shaded this frame whose tile-relative inputs are equal), or PAINT.
* **Representation:** fingerprints computed in tile-relative coordinates (the edge-form SDF on the 1/256 px grid is
  translation-invariant for integral translations, RESULTS §14.3), a per-frame hash table of fingerprints -> shaded
  tile, and the constant-colour fold of Forma (`skip_fully_covered_layers`: when every remaining record is a solid
  full cover, fold their colours once and FILL).
* **Saves (measured opportunity, E3):** whole frames are 25-93 % uniform tiles (heavy 24.9 %, lab 36.4 %, lab scroll
  54.6 %, animate 92.8 %) and have only 7-25 % distinct tiles (heavy 23.1-25.1 %, lab 24.4 %, vscroll 9.4 %, animate
  7.0 %); among *changed* tiles, constant ones are 0.1-2 % on stream scenes and 32-39 % on scroll workloads, and tiles
  equal to another tile of the same frame are 0.3 % (lab stream), 18.8 % (heavy stream), 76-96 % (heavy, lab and micro
  scroll). A theme change is a full redraw in which a quarter to all tiles are FILLs (a FILL is 237 instructions per
  tile on a CPU, E2, against 7.5-23 K to shade).
* **Cost when unused:** the hash-table probe per painted tile (INTRA) and the fold check (FILL): small, but INTRA is a
  serialising lookup across threadgroups within one frame (the table is written by tiles shaded earlier in the
  dispatch); a two-pass form (fingerprint all, then dedupe, then shade the uniques) avoids races at the cost of a pass.
* **Overfit risk: high for INTRA.** The 76-96 % duplicate rates come from synthetic scenes built of identical rows on
  a 32-px pitch; real lists have varying text. FILL and SKIP are not at risk.
* **Exactness:** FILL is exact when the fold uses the kernel's own arithmetic (the fold is one record's `cv` or a
  chain of the same half FMAs); INTRA is exact when the fingerprint covers every input and the arithmetic is
  translation-invariant (true for integral tile offsets with grid geometry; false under transforms, which must not
  INTRA). Mutant: INTRA across a transformed space.
* **Kill experiment:** E3 on the AWAGY dashboard and scroll windows and on gui-bench's ported scenes: if changed tiles
  equal to another tile of the frame stay under 10 %, drop INTRA and keep FILL/SKIP.

### 9. Stage once per row; a 64-byte staged record

* **Rule bent:** "the tile stages what it hits". Staging is a per-record constant except `cover`; the bin pass, which
  already evaluates the glyph pen formula and the clip, can emit the staged geometry.
* **Representation:** a bin entry carries the record's exact edges (4 x f32, or i32 on the 1/256 grid), its clip
  window, the premultiplied colour (half4), uv and kind: 32-48 B instead of 16 B plus three dependent levels of loads
  per tile. The threadgroup record for the base kinds shrinks from 144 B to ~64 B (`q`, `b`, `pm`, `uv`, `kind`, `core`
  as i16x4), with the transformed/image/gradient/shadow fields in a side array indexed only when `kind >= 4 || xs`.
* **Saves:** per hit per tile, 4-7 dependent loads (~96-118 B, depth 3) become one independent 32-48 B load; the
  threadgroup memory per tile threadgroup falls from ~18.4 KB to ~8 KB (estimated), which raises occupancy if
  threadgroup memory is what bounds it on M3 (not measured); a row-wide background record is staged once per row, not
  once per tile.
* **Cost when unused:** bin memory doubles or triples (today ~16 B x entries x rows); for full redraws of glyph-dense
  windows that is ~2-3 MB (estimated).
* **Overfit risk:** low.
* **Exactness:** the staged values are computed by the same expressions in the bin pass; equal by construction
  (gate: lane gpu's original-kernel raster oracle `oracle.metal`); mutant: stage from the floored ink.
* **Kill experiment:** occupancy first: shrink `FRec` to 64 B (moving the rare fields to a side array) in lane gpu's
  harness and measure lab stream and heavy stream. If occupancy is not the limiter, keep only the bin-carried staging
  and measure it alone.

### 10. The opaque floor done right

* **Rule bent:** "a floor is a solid quad covering the tile".
* **Representation:** (a) per 2x2 block: while building the per-thread record mask (lane gpu's bit walk), also mark
  records whose `core` holds the block and whose `cv.a == 1`; clear every bit below the highest such record and start
  `acc` at its `cv` (exact: blending over alpha 1 is a copy, RESULTS §13.1 v4); (b) rounded and bordered records'
  cores count as covers; (c) the floor index known before the scan, from the core's per-tile floor plane
  (FOUNDATION-R6 §2), so entries below it are never loaded; (d) translation-only spaces stay eligible.
* **Saves:** layers below the floor: 0.00-0.01 layers per pixel on lane gpu's scenes (measured, gpu lane: these scenes
  have no stacked opaque panels), up to whole content layers under sheets, popovers, sidebars and modal dialogs.
* **Cost when unused:** one more compare per record per block in the mask loop (estimated 2-3 % of the mask loop).
* **Overfit risk:** none; its value is invisible on the current benchmarks, which is itself a sign of overfitting in
  the benchmark set.
* **Exactness:** exact by the alpha-1 identity; mutant: count a record with `cv.a = 254/255` as a floor.
* **Kill experiment:** a gate scene with a sheet over a dense list; layers per pixel and tile time with and without.

### 11. One reference surface plus residual layers

* **Rule bent:** "a frame is a full surface". A codec keeps one reference frame and sends residual blocks.
* **Representation:** the window shows a base IOSurface plus small patch layers (opaque, tile-aligned) carrying the
  tiles that changed since the base was last written. Patches are flattened into the base lazily **under their own
  cover**: the base's pixels beneath an opaque patch are invisible, so they can be written in place while the base is
  on screen; the patch is removed in a later transaction once the write is complete. Memory is one full surface plus
  the patches in flight, instead of a ring of 2-4 full surfaces.
* **Saves:** at 6K one surface is 81.4 MB (6016 x 3384 x 4 B); the ring of three is 244 MB (estimated arithmetic);
  on the memory ledger's AWAGY windows, 3 x 10.78 MiB drawn surfaces become ~1 x 10.78 + patches (measured surface
  sizes, estimated saving ~20 MiB).
* **Cost when unused:** none.
* **Overfit risk:** large-change frames (scroll, resize) degenerate to "patch everything", so the base must still be
  double-buffered for those (fallback to today's ring, made volatile when idle as today).
* **Exactness:** depends on two compositor behaviours that are not documented: that CA re-reads the base's pixels when
  a covering patch is removed (it may need `setContentsChanged`, RESULTS §12.3 measured that as no cheaper than a
  swap), and that WindowServer never scans out the base directly while patches cover it. The gate is the layer
  composite of formulation 4.
* **Kill experiment:** a windowed probe: write a base region under an opaque patch while the base is on screen, remove
  the patch, capture: the probe dies on any frame where the old pixels show.

### 12. The small-frame GPU floor

* **Rule bent:** "a frame is three dispatches and a barrier".
* **Representation:** when the slot names one damaged row, encode bin and tile as one kernel (the row's threadgroup
  bins, then shades its tiles itself); `fs_copy`'s dispatch removed from frames without a copy (today it is dispatched
  with zero threadgroups every frame); the tile list written by the bin pass (1.1).
* **Saves:** a dispatch, a buffer barrier and the tile-locating search per small frame: a few microseconds of GPU time
  (the empty-encoder floor is 1-4 us at its minimum and the per-tile launch 30-85 ns, measured, gpu lane; the
  barrier's drain is not measured separately). Small, which is why it ranks here.
* **Cost when unused:** a second pre-encoded command shape (the batch would hold both shapes, selected by an indirect
  count of zero), so a little more arming work.
* **Overfit risk:** low; less valuable once formulation 1 takes small frames off the GPU.
* **Exactness:** same kernels' arithmetic.
* **Kill experiment:** lane gpu's A/B on animate and a one-tile frame; dies if the frame drops by less than 3 us.

### 13. Compact encodings: line header plus residual glyphs; 24-byte quads

* **Rule bent:** "a primitive is a 48-byte instance".
* **Representation:** glyph slots are already 6 B (`x: i32`, `ent: u16`); a line header plus 4 B residuals (`dx: u16`
  from the line's pen origin in 1/64 px, `ent: u8`, flags) makes it 4 B, and bin entries per line instead of per glyph
  (formulation 7) remove the 16 B per glyph per row. Quads: edges as 4 x i32 on the 1/256 grid (16 B) + colour (4 B) +
  space and clip (4 B) = 24 B; radius, border width, `color2` in a side column only for kinds that use them.
* **Saves:** memory and staging bandwidth. The ledger says what the GPU draws a row from is ~320 B (glyph slots
  192 B, pages 46 B, run header 32 B, instance 48 B, measured, memory lane): perhaps 320 -> 200 B per row (estimated),
  small next to the 2.5 KiB of Rust heap per row. Ranked low for that reason.
* **Cost when unused:** decode ALU per staged record.
* **Overfit risk:** none.
* **Exactness:** representation only; the pen formula is applied to the reconstructed `x` (exact integer arithmetic).
* **Kill experiment:** none needed; do it when formulation 7 lands.

### 14. Retained raster caches as tile pools

* **Rule bent:** "a cache of pixels is a rectangle of pixels".
* **Representation:** retained rasters that are not presented directly (a scroll-back cache, a quiet subtree's raster
  from FOUNDATION-R6 §4, an overscan margin) are stored as a pool of 32x32 tiles plus a 4-byte index per tile slot,
  with FILL tiles as one word and duplicates shared; a sparse texture or a heap of tile-sized allocations backs it.
* **Saves (measured opportunity, E3):** 7-25 % distinct tiles per frame and 25-93 % uniform ones, i.e. 4-14x less
  memory for such caches on these scenes (estimated; synthetic content inflates duplicates).
* **Cost when unused:** none. Presented surfaces cannot use it: CA needs a linear IOSurface.
* **Overfit risk:** the duplicate rates (formulation 8).
* **Exactness:** pixels are copied verbatim.
* **Kill experiment:** E3 on real application frames (AWAGY, gui-bench): distinct-tile ratio.

---

## Killed or not established

### K1. Zero Metal calls on the frame path: the GPU polls a word in shared memory (KILLED, measured)

The idea: a bounded persistent kernel spins on a sequence word in a `storageModeShared` buffer; a frame is one CPU
store; no `setSignaledValue`, no kernel-side submission. Measured (E1, `opus_probe poll|life`):

* A running kernel essentially never observes the CPU's store. Plain relaxed atomic loads: 1 acknowledgement in 240
  releases (at 22.9 ms), and in a first, unbounded run 5 in 600 (at 41-95 ms); with the buffer mapped write-combined
  on the CPU: 0 of 120; with read-modify-write atomics
  and write-combined: 0 of 120. In every run the kernel ran to its full iteration cap (300 M iterations in 41.9 s;
  150 M in 18.4 s; 100 M in 17.7 s) without seeing the stop sentinel the CPU had stored. Coherence between a CPU
  store and a running kernel's device atomics is not provided on this GPU through public Metal (MSL has no system
  scope); it is established at command-buffer boundaries, which is exactly what the event path pays for.
* A spinning poll iteration costs 140 ns (relaxed load) or 195 ns (RMW). Inference, not measured: the loads leave the
  core's L1 but are served inside the GPU's own cache hierarchy, which the CPU's stores do not invalidate.
* A running kernel blocks the process's other GPU work: a one-thread nop committed on a **second** queue completed
  only after the poller ended (p50 359 ms and 572 ms vs 216-227 us alone).
* No watchdog ended a 41.9 s kernel (macOS 15, no window).

So the frame path keeps one event release per GPU frame; the only way to take the ~50 K driver instructions and the
~400 us round trip off a frame is not to use the GPU for it (formulation 1).

### K2. Lossless-compressed IOSurfaces as a memory lever (NOT ESTABLISHED)

IOSurfaces created with the fourccs `&BGA`, `-BGA` and `&8v0` and plain BGRA properties were accepted, allocated
11,534,336 bytes each (the same as plain BGRA at 2048 x 1408), took a BGRA8 shader-write Metal texture and a compute
write without error (E4). Whatever these fourccs do, they did not reduce the allocation; real AGX framebuffer
compression saves bandwidth, not allocation, and is applied to private textures. I did not try CoreVideo's
attribute set for compressed pixel buffers. As a memory lever: not supported by anything I measured.

### K3. Render passes and tile memory for the raster (REJECTED)

A render pass adds ~30 MiB to the driver's block on first use (91.4 vs 60.8 MiB, measured, memory lane `floorprobe`),
and the compute kernel already accumulates in registers, which is what tile memory would give. Memoryless targets
exist only for render attachments.

---

# Appendix: experiments

All commands from the worktree root with the round's environment
(`PATH=$HOME/.local/share/cargo/bin:$PATH RUSTC_WRAPPER= CARGO_BUILD_JOBS=4 CARGO_TARGET_DIR=$PWD/target`) and
`ZGX_FONT_FILE=.../ref/zgx/comparisons/fonts/DejaVuSans.ttf`. Built with
`cargo build --release -p bough-metal --example opus_probe -p bough-scenes --example opus_tiles -p bough-heavy --example opus_tiles_heavy`.

## E1: GPU polling vs the event stream (`opus_probe poll event life`, under the lock)

The poll kernel (the RMW variant replaces the loads with `atomic_fetch_or_explicit(&ctl[0], 0u, ...)`):

```metal
kernel void poll(device atomic_uint* ctl [[buffer(0)]], uint tid [[thread_position_in_grid]]) {
    if (tid != 0) return;
    uint cap = atomic_load_explicit(&ctl[1], memory_order_relaxed);
    uint last = atomic_load_explicit(&ctl[0], memory_order_relaxed);
    uint i = 0;
    for (; i < cap; i++) {
        uint s = atomic_load_explicit(&ctl[0], memory_order_relaxed);
        if (s != last) { last = s; atomic_store_explicit(&ctl[32], s, memory_order_relaxed); if (s == 0xffffffffu) break; }
    }
    atomic_store_explicit(&ctl[64], i, memory_order_relaxed);
}
```

The CPU stores `seq` with `Release` and spins (200 ms timeout) on `ack == seq` with `Acquire`, after gaps of 0, 100,
1,000 and 8,000 us. The event variant pre-arms 16 frames per command buffer, each `wait(ev >= 2f-1)` -> a one-thread
kernel copying `seq` to `ack` -> `signal(2f)`, and releases with `setSignaledValue(2f-1)` (as `Kern::arm_locked`).

```
event  gap     0 us: round trip p50   101.2 us  p90   372.7  p99  3315.8  max   5824.3  fails 0/300
event  gap   100 us: round trip p50    76.3 us  p90   104.8  p99   438.5  max  95775.2  fails 0/300
event  gap  1000 us: round trip p50   395.7 us  p90   427.4  p99   743.0  max   1348.5  fails 0/300
event  gap  8000 us: round trip p50   444.1 us  p90   460.5  p99  1233.9  max   2124.7  fails 0/300
event  process instr per release (minus a 1-ms sleep loop's 18046): 86410; acks missing after 1 ms: 1/300
poll   gap 0/100/1000/8000 us: 1 ack in 240 tries (22.9 ms); kernel ended: gpu 41.894 s iterations 300000000 (= cap)
poll   (WC=1)          0 acks in 120; gpu 18.379 s iterations 150000000 (= cap)
poll   (RMW=1 WC=1)    0 acks in 120; gpu 17.688 s iterations 100000000 (= cap)
life   cap 4194304: gpu 586.605 ms, 139.9 ns per poll iteration;  RMW: 817.615 ms, 194.9 ns
life   nop on a second queue commit->completed: alone p50 227 us p90 630; beside the poller p50 359360 us
life   (RMW) alone p50 216 us p90 242; beside the poller p50 572176 us
```

## E2: one tile on one P-core (scratchpad `neon_tile.rs`, `rustc -C opt-level=3 -C target-cpu=native`)

Record-major spans over an 8 KB planar half accumulator (`[[f16; 1024]; 4]`), starting at the last opaque full-tile
solid; per 8-pixel group: coverage (solid: lane mask; glyph: one 8-byte load of the atlas row, widened, `* (1/255)`
in f32, narrowed to half; rounded box: the edge-form SDF in f32 with `vsqrtq_f32`), `sh = pm * cov`
(`fmul.8h`), `acc = fma(sh, acc, 1 - sh.a)` (`fmla.8h`, one rounding, as the GPU's); the store converts each half to
f32, multiplies by 255, rounds to nearest even (`fcvtnu.4s`), narrows and interleaves (`st4.8b`). Assembly checked:
4 `fmla.8h`, 4 `fmul.8h`, 32 `fcvtnu.4s`, 4 `st4.8b` in the function; the only calls are two cold bounds-check panics.
Instructions per tile are `thread_selfcounts` over 20,000 tiles, best of 5; wall time is noisy (load average ~90).

```
tile (32x32 px)                                   instr        ns
background only (1 solid)                          7543       523
caret: bg + 2x20 solid                             9104       867
text: bg + 5 glyphs (heavy median hits)           15482      4763
text: bg + 10 glyphs (lab median hits)            23409      5541
button: bg + rrect r=6 + 4 glyphs                 21522      5959
translucent: bg + 3 overlapping 50% solids        15988      3447
constant tile: store only                           237       225
```

A naive group-major version (every record tested for every 8-pixel group, scalar coverage gathers) cost 15-55 K
instructions per tile: the record-major form is what a CPU backend should be. About 4.5 K of every tile is the store
conversion (estimated from the instruction mix), which is why a constant tile (237) is 30x cheaper than a shaded one.

## E3: tile redundancy of real frames (`opus_tiles`, `opus_tiles_heavy`; `RING=1|2|4`, 120 ticks, 90 for heavy)

Per frame after the first: the presented frame is read back, each 32x32 tile hashed (FNV-1a over its bytes; distinct
colours counted up to 17); "shaded" is the tile set of `App::debug_rows()` for that frame; "changed" means the tile's
bytes differ from the previous presented frame. Ring 1 (one surface, age 1) is the "single reference" case and shows
the epoch's own damage without ring-age inflation.

```
RING=1
lab stream    tiles 2700 distinct 24.4% uniform 36.4% | shaded 447.6 changed   43.4 shaded-but-unchanged 90.3%
              changed: constant 0.1% 2-colour 0.0% <=16 colours 5.0% in prev frame 3.1% elsewhere in frame 0.3%
lab scroll    tiles 2700 distinct  8.7% uniform 54.6% | shaded 1508.0 changed 1458.5 shaded-but-unchanged 3.3%
              changed: constant 32.9% 2-colour 39.2% <=16 colours 78.1% in prev frame 33.7% elsewhere in frame 95.7%
lab both      tiles 2700 distinct 24.1% uniform 39.3% | shaded 1955.6 changed 1501.9 shaded-but-unchanged 23.2%
animate       tiles 2700 distinct  7.0% uniform 92.8% | shaded  116.0 changed   84.6 shaded-but-unchanged 27.1%
              changed: constant 0.0% <=16 colours 69.0% in prev frame 1.8% elsewhere in frame 1.4%
blink         tiles 2700 distinct  0.1% uniform 100%  | shaded    2.0 changed    2.0 shaded-but-unchanged 0.0%
micro vscroll tiles 2250 distinct  9.4% uniform 45.7% | shaded 2250.0 changed 1992.6 shaded-but-unchanged 11.4%
              changed: constant 38.7% 2-colour 41.9% in prev frame 38.7% elsewhere in frame 92.1%
heavy stream  tiles 4000 distinct 25.1% uniform 24.9% | shaded  992.5 changed  657.1 shaded-but-unchanged 33.8%
              changed: constant 1.1% 2-colour 29.9% <=16 colours 46.9% in prev frame 29.8% elsewhere in frame 18.8%
heavy scroll  tiles 4000 distinct 23.1% uniform 25.0% | shaded  990.0 changed  990.0 shaded-but-unchanged 0.0%
              changed: constant 2.1% in prev frame 3.6% elsewhere in frame 76.2%
heavy both    tiles 4000 distinct 24.8% uniform 24.9% | shaded 1856.5 changed 1508.6 shaded-but-unchanged 18.7%
RING=2 (copy active): lab stream shaded 453.3; lab scroll 172.6 (+1334.6 copied); animate 121.6; micro vscroll 89.2
RING=4 (headless default): lab stream shaded 458.1; heavy stream 1080.6 (41.0% unchanged); heavy scroll 87.0
```

## E4, E5: IOSurface and atlas (`opus_probe io atlas`)

```
io     IOSurfaceLock+Unlock (read-write): 5330 instr, 0.59 us per pair
io     IOSurfaceLock+Unlock (AvoidSync): 5329 instr, 0.39 us per pair
io     lock + one 32x32 tile memcpy + unlock: 5789 instr, 0.98 us
io     &BGA / -BGA / &8v0: alloc 11534336 bytes (= plain BGRA 2048x1408), BGRA8 ShaderWrite texture created, compute write completed
atlas  default storage mode MTLStorageMode(1)  (Managed)
atlas  replaceRegion whole atlas 1024x512: 1144989 instr, 220.6 us per call
atlas  replaceRegion one 16-row band: 36776 instr, 1.3 us per call
atlas  linear R8 texture over a shared buffer: created (alignment 16)
```

## E6: CPU cull of one tile (`CULL=1 opus_tiles_heavy`)

`Bough::cull(tile, None, out)` on heavy (mode both, 30 ticks, headless, 10 pages): **2,148 instructions per tile**,
3.4 records per tile (max 9), 0.58 text records per tile, over all 4,000 tiles.

## E7: a window wider than 4096 px

`SCENE=lab SCALE=5.4 TICKS=2 scene_shot` (960 pt x 5.4 = 5184 px): `panicked at crates/bough-core/src/damage.rs:335:9:
window up to 4096x4096`.

## Not verified

* No formulation above was built; none has a gate. Every "saves" figure that is not tagged measured is arithmetic.
* E2's records are synthetic (integral geometry, a random atlas), and its CPU timings were taken at load ~90.
* E3 counts pixels, not inputs: "shaded but identical" bounds what formulations 2 and 3 can save; it does not show
  that either recovers it.
* I did not read the Metal compiler's output for the kernels (no `metal` tool on this machine; the library is compiled
  at run time).
* The 5K/6K panic was reproduced headless at 5184 px, not with a real display.
