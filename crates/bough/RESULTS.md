# bough façade: heavy, lab, animate, blink and micro_bench against zgx

`crates/bough` (the public façade) + two comparison crates on the `integrate` worktree: `comparisons/bough-heavy` (zgx's *heavy* dashboard) and
`comparisons/bough-scenes` (the rest of zgx's comparison scenes that need no general layout engine: **lab** = `component_workload`, **animate**,
**blink**, and `micro_bench`'s **state** and **vscroll-fixed**; windowed adapters, headless adapters, correctness suite). Apple M3 Pro, macOS 15.7,
120 Hz panel, `nightly-2026-10-17`, `dist` profile (fat LTO, cgu 1) for every number below. zgx = 06debe9d, its `dist` binaries (the headless ones,
`workload_headless` and `animation_headless`, which the repo did not have built, were built from the same tree with the same profile into a scratch target dir).
Everything for zgx was measured here, today, by the identical method; nothing is taken from zgx's published tables except where a column says so.

**Measurement conditions, read this first.** The machine was shared with other agents compiling for the whole session (load average 40-80 on 11 cores).
Instruction counts do not care; CPU %, CPU ns and footprint do, a little (they are reported with their spread, and bough and zgx runs are interleaved).
Windowed runs are 10 s, Accessory activation, no focus stealing, sampled at t = 1.5 s and t = 9 s. Blink was sampled in the same 1.5-9 s window,
not 20-30 s after launch (windowed runs were capped at 12 s).

```
export CARGO_TARGET_DIR=$PWD/target; RUSTC_WRAPPER= PATH=~/.local/share/cargo/bin:$PATH
export ZGX_FONT_FILE=/Users/mileswirht/projects/_ref/zgx/comparisons/fonts/DejaVuSans.ttf
cargo test --release -p bough -p bough-heavy -p bough-scenes     # 15 tests: oracle, pixel-exactness per scene, trim/resume
cargo build --profile dist -p bough-heavy -p bough-scenes
python3 bench/integrate/slope.py bough 0 lab                      # per-frame instructions, slope TICKS 300 vs 600 (animate: FRAMES 600 vs 1200)
python3 bench/integrate/slope.py zgx 1 animate                    #   <fw> <RENDER 0|1> <heavy|lab|animate> [modes]
python3 bench/integrate/windowed.py bough lab both                # 10 s window: CPU %, instr/s, footprint, wakeups (<fw> <scene> <mode>)
python3 bench/integrate/micro.py 3                                # micro_bench, zgx's exact semantics, 3 repeats each
bench/integrate/vmbreak.sh target/dist/lab_workload 5 ZGX_MODE=stream ZGX_SECONDS=7   # vmmap footprint breakdown
BOUGH_CHECK=1 BOUGH_STATS=1 ZGX_SECONDS=5 target/dist/lab_workload   # the real window: presented surface vs full redraw, ring statistics
target/release/examples/memprobe 2                                 # where the GPU driver's first-submission footprint comes from
```

## 1. The comparison, scene by scene

### 1.1 Per-frame instructions (slope method: TICKS=600 minus TICKS=300, `/usr/bin/time -l` "instructions retired", process-wide, median of 3)

`RENDER=0`: everything up to a ready draw list + instance writes (bough) / layout + damage, no renderer (zgx). `RENDER=1`: plus the offscreen Metal render
of every damaged frame (bough: IOSurface ring, stencil repair, one draw, commit; zgx: wgpu `GpuRenderer`). Lab runs at SCALE 1 (zgx's `workload_headless`
default), animate at SCALE 2 (zgx's `animation_headless`), heavy at SCALE 2. Spreads are in `results/raw/slopes_*.txt` (RENDER=1 rows: 1-10 % from Metal driver threads).

| scene / mode | zgx RENDER=0 | **bough RENDER=0** | ratio | zgx RENDER=1 | **bough RENDER=1** | ratio |
|---|---:|---:|---:|---:|---:|---:|
| heavy idle   | 2,609 | **~0** (1.7 K = startup noise) | - | 537,961 | **~0** (no frame is drawn) | - |
| heavy stream | 2,988,404 | **404,008** | 7.4x | 6,190,664 | **652,788** | 9.5x |
| heavy scroll | 164,565 | **32,452** | 5.1x | 1,463,470 | **277,723** | 5.3x |
| heavy both   | 3,170,725 | **439,000** | 7.2x | 6,970,157 | **687,068** | 10.1x |
| lab idle     | 2,928 | **~0** (2.4 K = noise) | - | 117,402 | **~0** | - |
| lab stream   | 768,937 | **69,587** | 11.0x | 1,085,308 | **309,022** | 3.5x |
| lab scroll   | 86,496 | **11,208** | 7.7x | 686,063 | **262,335** | 2.6x |
| lab both     | 811,835 | **79,390** | 10.2x | 1,442,231 | **338,415** | 4.3x |
| animate (update + flush + draw list) | 225,140 | **14,270** | 15.8x | 820,256 | **259,914** | 3.2x |

With a renderer the floor is the Metal encode + commit (~240-250 K instructions per drawn frame: `renderCommandEncoder` creation, `commit`, driver
bookkeeping, measured with `sample`: nothing bough-side). That floor is why the RENDER=1 ratios are smaller than the RENDER=0 ones, and it is
the same floor for any direct-Metal presenter (tried: untracked hazard tracking on pool/atlas/surfaces, `maxCommandBufferCount` 2..64: no change).

heavy per-phase (bough, instructions per frame, first slice, `PHASES=1`): stream update 207 K, text 140 K, damage 27 K, cull+list 23 K; scroll 16 K / 0 / 8 K / 12 K.

### 1.2 Windowed, 10 s, from outside (`proc_pid_rusage` at t = 1.5 s and 9 s; CPU = user+system of the process, all threads; GPU and WindowServer excluded)

Two repetitions each (idle: one); the cell shows both, or the one. `ticks` = ticks the adapter reported in 10 s (zgx / bough); animate: `animation_frames`.

| scene / mode | zgx CPU % of a core | **bough CPU %** | zgx instr/s | **bough instr/s** | zgx footprint | **bough footprint** | ticks / fps (zgx / bough) |
|---|---:|---:|---:|---:|---:|---:|---|
| heavy idle   | 0.08 | **0.00** | 4.8 M | **0.08 M** | 51.0 MB | **46.0 MB** | 0 / 0 |
| heavy stream | 7.0, 5.6 | **1.9, 1.8** | 360 M, 358 M | **52.3 M, 52.4 M** | 166.1, 165.1 | **156.9, 156.4** | 599 / 600 |
| heavy scroll | 3.5, 2.9 | **1.9, 1.6** | 67.5 M, 65.1 M | **30.3 M, 30.2 M** | 161.5, 161.1 | **156.3, 156.2** | 599 / 600 |
| heavy both   | 7.3, 7.5 | **2.3, 1.9** | 418 M, 452 M | **54.4 M, 54.4 M** | 167.5, 165.9 | **158.0, 156.0** | 599 / 600 |
| lab idle     | 0.18, 0.09 | **0.01** | 9.9 M, 4.5 M | **0.1 M** | 41.0, 41.1 | **40.8, 39.8** | 0 / 0 |
| lab stream   | 2.8, 2.9 | **1.7, 1.9** | 74.0 M, 74.6 M | **32.2 M, 32.3 M** | 146.1, 146.6 | **144.9, 144.3** | 599 / 600 |
| lab scroll   | 2.5, 2.9 | **1.6, 1.9** | 46.3 M, 51.2 M | **29.4 M, 29.1 M** | 145.6, 145.8 | **145.2, 145.0** | 599 / 600 |
| lab both     | 3.6, 2.7 | **1.9, 1.5** | 92.5 M, 87.3 M | **33.4 M, 32.9 M** | 148.1, 147.1 | **144.1, 143.3** | 599 / 600 |
| animate (120 Hz refresh) | 4.8, 3.7 | **3.7, 2.9** | 101 M, 100 M | **60.6 M, 58.5 M** | 145.7, 144.5 | **145.2, 143.7** | 119.0, 119.1 fps / 120.2, 120.2 fps |
| blink (530 ms) | 0.26, 0.15 | **0.07, 0.05** | 10.8 M, 6.0 M | **1.3 M, 1.2 M** | 141.8, 141.6 | **143.5, 142.8** | 18 / 18 blink frames, workload_ticks=0 |

Interrupt wakeups per second (blink, two runs each): zgx 4.1 and 4.7, **bough 3.6 and 3.2**; lab idle: zgx 1.2, bough 0.8 (`ri_pkg_idle_wkups` reads 0 for every
run). A skipped animate frame (compositor still holding both surfaces after ~7 ms) happens in about 1.5 % of frames under this load (`BOUGH_STATS`); the
adapter's `animation_frames` counts display refreshes it was called for, as zgx's does.

Honest reading. bough wins every instruction column by a wide margin and the CPU % column everywhere, but the CPU gap is much smaller than the instruction gap
outside heavy: at 60 or 120 frames a second the Metal commit + compositor handshake (~250 us of CPU per presented frame, none of it bough-side) is most of
what is left, and zgx pays it too. Memory is a clear win only in heavy and at idle; lab, animate and blink are within +-2 MB of zgx (section 2).

### 1.3 micro_bench (zgx's `zgx_workload::micro`, exact semantics: process CPU clock, all threads, batches of 50 writes / 10 frames, 200 warm-up ops, 2,000 ops)

`write` = the scene's signal write alone (`App::write`), `frame` = write + everything to a frame ready to present (`App::flush` + `App::frame`; RENDER=1: + encode and commit).
CPU ns per operation, median of the 3 repeats' medians [min-max over repeats]; zgx re-run here with its own binary (`micro_bench`, same load). `vscroll-variable` needs the
layout engine (bough-layout) and is not built.

| scene | RENDER | figure | zgx (re-run today) | **bough** | zgx published baseline (`SUMMARY.md`, quiet machine) |
|---|---|---|---:|---:|---:|
| state | 0 | write | 1,060 [1,040-1,140] | **0** [0-0] (p90 20) | 1,020 |
| state | 0 | frame | 2,600 [2,300-5,000] | **400** [200-400] | 2,200 |
| state | 1 | write | 2,080 | **20** | 1,580 |
| state | 1 | frame | 253,400 [220,900-287,500] | **188,400** [187,500-192,100] | 141,600 |
| vscroll-fixed | 0 | write | 1,500 [1,260-1,820] | **20** | 1,200 |
| vscroll-fixed | 0 | frame | 4,300 [4,300-4,700] | **1,700** [900-2,000] | 4,000 |
| vscroll-fixed | 1 | write | 2,560 | **20** | 2,100 |
| vscroll-fixed | 1 | frame | 386,000 [384,100-394,700] | **176,500** [164,100-227,000] | 201,000 |

p10 / p90 of every row are in `results/raw/micro.txt`. bough's write is a field store plus one OR (the QUEUED bit); the work zgx does inside its write happens in bough's
frame, so compare write + frame: 400 ns against 3,660 ns (state, RENDER=0), 1,720 against 5,800 (vscroll-fixed). With the renderer both are dominated by the Metal floor
(bough p10 165 K, zgx p10 178 K ns for state); the machine was loaded, which inflated zgx's re-run well above its published quiet-machine figure and bough's less.

## 2. Memory

### 2.1 Where the footprint goes (windowed, sampled at t = 5 s with `vmmap --summary`; `bench/integrate/vmbreak.sh`; MB, dirty + swapped by region type)

| scene | footprint | IOSurface | GPU driver (owned unmapped + IOAccelerator*) | malloc | image / data (__DATA, __AUTH, mapped file, stack, misc) |
|---|---:|---:|---:|---:|---:|
| heavy both, bough (before: ring of 4, round-robin) | 187.7 | 62.5 | 92 + 7 | ~12 | ~16 |
| **heavy both, bough (now)** | **155.7** | 31.2 | 107.2 | 11.9 | 17.0 |
| heavy both, zgx | 165.2 | 31.2 | 108.0 | 19.7 | 25.7 |
| lab stream, bough / zgx | 145.0 / 146.6 | 21.1 / 21.1 | 105.5 / 98.0 | 12.9 / 13.2 | 18.1 / 19.2 |
| animate, bough / zgx | 144.4 / 145.3 | 21.1 / 21.1 | 97.6 / 105.8 | 12.4 / 12.2 | 17.5 / 18.2 |
| blink, bough / zgx | 143.0 / 141.7 | 21.1 / 21.1 | 96.6 / 103.9 | 12.0 / 10.3 | 17.0 / 16.4 |
| heavy idle, bough / zgx | 46.5 / 50.4 | 15.6 / 15.6 | 13.0 / 13.1 | 12.6 / 15.4 | 17.7 / 21.3 |

(The columns sum to a few MB more than `phys_footprint`: some GPU regions are dirty but not footprint-counted. The GPU column moves by +-8 MB between runs depending on whether the
driver books one 8 MB heap as `IOAccelerator` or `owned unmapped`; the footprint does not.)

**The ~95-105 MB "GPU driver" block is the Metal driver's first-submission allocation, it is identical in both frameworks, and no Metal API choice changes it.**
`crates/bough/examples/memprobe.rs` isolates it. Process footprint before / after the first command buffer that *runs anything*:

| first GPU work | footprint after | notes |
|---|---:|---|
| device + queue + texture + pipelines created, nothing submitted | 6-9 MB | |
| an empty command buffer committed | 6.3 MB | the driver allocates nothing until there is work |
| **one render pass, clear only, 64x64 target** | **128 MB** | same at 640x480, 2560x1600, 5120x3200 (independent of target size) |
| one render pass with a draw, blend, vertex buffer | 129 MB | |
| compute dispatch only | 92 MB | measured with a temporary kernel, since removed from the probe |
| blit only | 92 MB | |
| 1-2 s after the GPU goes idle | **34 MB** | the driver gives it all back (a windowed idle bough is 40-46 MB for this reason) |
| next pass | 124 MB | and takes it again |

So any process that submits GPU work holds ~90-105 MB for as long as it keeps submitting (every 530 ms is often enough: blink never lets go), and none of the knobs
in reach move it: `maxCommandBufferCount` (2..64), hazard tracking untracked, storage modes, PLAIN texture instead of IOSurface-backed (+2 MB). The only way below it
is to not use the GPU for a frame (section 2.4).

### 2.2 What changed (the surface ring)

What was left to win was the IOSurface block and the rest of the process: heavy at the old default (a 4-surface ring, drawn round-robin) was 187.7 MB against zgx's 169.
Measured facts that drive the design:
* An IOSurface costs nothing until its pages are first written (windowed idle with four allocated: 46 MB, one touched).
* Round-robin touches every slot. A ring of 4 therefore costs 62.5 MB however little contention there is.
* The compositor holds the displayed surface and, at most, one more; the CPU is already synchronised with the GPU at the start of every turn, so a window never needs
  more than two surfaces *unless the compositor is late*.
* `IOSurfaceSetPurgeable(volatile)` takes a surface's pages out of the footprint, but **only once the GPU's mapping is gone** (after a GPU-written surface is volatile the
  footprint does not move until the GPU context idles ~1.5 s later; probe: `PURGE2=1 memprobe 2`). So it is a calm-window tool, not an active-window one.

Policy (`App::pick_surface`, `App::trim`, `backend/host.rs`):
1. **Windowed ring = 2 slots, drawn oldest-free-first, never a held surface.** The slot the compositor releases next is polled first (one `IOSurfaceIsInUse` call per poll
   in steady state). If it holds both, the frame sleeps most of the *learned* usual release time (running average), polls at 150 us, and gives up after ~7 ms: the frame is
   **skipped, its damage stays pending** (the tile planes are not retired), and the host retries on a one-shot timer (4, 8 .. 128 ms back-off). Growing the ring on contention was
   tried first (3rd slot after 8 ms, 4th after 16): under this machine's load it touched a third surface in about a third of the 120 Hz runs (166 MB), and a surface the
   compositor cannot take yet is better drawn later from the latest state than drawn now into more memory. Offscreen rings (`Config.ring` default 4, `RENDER=1` headless, tests) are
   unchanged: round-robin, ages 1..4, so every repair age stays exercised.
2. **Calm trim (purgeable).** One one-shot timer, pushed back at most four times a second while frames keep leaving a written, non-displayed surface (so it never fires, and adds no
   wakeup, under 60 Hz or 530 ms-blink activity); after ~1 s without a frame it makes every non-displayed surface purgeable-volatile. The next draw into a trimmed slot makes it
   non-volatile first and, if the system purged it meanwhile, repaints it whole (`age = None`). A burst of lab activity (3 s) followed by calm: **145.2 MB while ticking, 40.1 MB 2.5 s
   after the last tick** (one surface resident, driver block returned), `BOUGH_BURST=3` on `lab_workload`.
3. Publishing moved from the Metal completion handler (a second thread, its own `CATransaction` begin/commit per frame) to the main thread right after the frame's completion
   (`waitUntilCompleted`, then `setContents` inside the turn's implicit CA transaction). That is no thread hop and one commit per turn. Measured: no change in instr/s (the CA commit
   itself is the cost); kept because it removes a thread and a block allocation per frame (`BOUGH_PUBLISH=async` restores the old path for A/B).
4. Compositor polling cost: waiting for the release at 120 Hz was 14 `IOSurfaceIsInUse` polls per frame (200 us sleeps); the learned sleep + oldest-first poll made it 4 (`polls` / frames in
   `BOUGH_STATS`). The animate window's instr/s was 64-98 M in runs before (a large spread with load) and is 56-61 M in every run after; the rest of the gap to its 31 M headless figure is the CA commit
   and the compositor handshake of each presented frame.

Result: heavy 187.7 -> **156.2 MB** (zgx 165.2, a 9 MB / 5 % win); lab 144-145 vs 146-148 (1-4 MB win); animate and blink within +-1.5 MB of zgx (blink is **1 MB worse**:
bough's GPU bucket is not smaller there); idle 40.8-46.0 vs 41.0-51.0.

### 2.3 Tried, no effect
`newCommandQueueWithMaxCommandBufferCount(2..64)`; `MTLHazardTrackingModeUntracked` on pool/atlas/surface textures (also no instruction change); a plain (non-IOSurface) render target;
making surfaces volatile while the GPU is active (no footprint change until it idles); a single-surface "ring" (would draw into the surface the compositor is reading).

### 2.4 Where the rest is, and the rule that would have to bend
Of the 145 MB of a lab window, 105 MB is the driver block, 21 MB the two surfaces, 12 MB malloc (bough's own heap is about 3-4 MB of it: `lab_headless RENDER=0` is 8 MB in total,
including the font at least twice: the shaper's leaked copy and the glyph cache's copy), the rest frameworks' data. bough's remaining non-GPU memory is at most 3 MB from
being shaved (dedupe the font copies, shrink the 1024x512 CPU atlas copy); not done, below the noise of the measurement.

The rule bent here is "every damaged frame is rasterised by the GPU". The representation change that follows from the probe is to rasterise *small* repairs on the CPU into the IOSurface
(solid, rounded, border and glyph instances from the draw list the cull already produces, ~300 lines) and to submit GPU work only for large repairs: a window whose frames are all small
(blink, a caret, a clock) would never create the GPU context at all (~45 MB instead of ~143), and the context is returned 1-2 s after the last large frame. It is **not implemented**: the
CPU path must be bit-compatible with the shader (or the full-redraw oracle must run the same path) or the byte-exact correctness check degrades to a +-1 LSB tolerance, and a CPU frame is estimated (not measured) to cost
more than a GPU frame above a few hundred thousand repaired pixels (heavy scroll repairs 1.3 M).

## 3. Correctness

* **Per scene, offscreen:** `comparisons/bough-scenes/tests/correctness.rs` (9 tests, release) and `comparisons/bough-heavy/tests/correctness.rs` (5, release and debug in the first slice): after every 5th-9th
  presented frame the surface the ring drew into is read back (IOSurface memory) and compared **byte for byte with a from-scratch full redraw of the same retained state**; at the end the
  retained frame equals **a fresh mount of the view at the same signal values**. 0 differing bytes in every test.

  | scene | what is driven | frames compared |
  |---|---|---|
  | lab stream / scroll / both | 240 / 240 / 400 workload ticks at scale 2 | every 7th / 7th / 9th |
  | lab scroll jumps | 0 -> 14 -> 200 -> 1000 -> 5000 -> 123,456 -> ... -> the last row (2,799,600) -> back to 0 | every jump |
  | animate | 240 refreshes at 120 Hz, then irregular clocks (2.5, 7.3, 100, 0, 3.4) | every 5th |
  | blink | 31 flips; 50 turns that change nothing draw nothing and shape nothing | every flip |
  | micro state | 300 counter writes | every 11th |
  | micro vscroll-fixed | 120 steps of 12 pt, then 5,000, the last rows, the end, wrap to 0, +12, +24 | every 3rd |
  | trim / resume | two calm trims (all but the displayed slot made volatile) in a 160-tick lab run | every 5th after; 0 purged |
  | heavy stream / scroll / both, jumps, idle | as in the first slice | |

* **The real window path** (host timer, `Metal::publish`, the 2-slot ring with the compositor holding surfaces): `BOUGH_CHECK=1` compares, at exit, the surface the window last drew into
  with a full redraw of the retained state. Heavy both, heavy scroll, lab stream, lab scroll, lab both, animate (594 frames, 9 skipped and retried), blink (10 frames): **0 differing bytes in each**.
* **Visual parity with zgx** (`results/`). zgx's window cannot be captured here (no Screen Recording permission), so the reference frames are zgx's own `render_parity` (offscreen, same
  renderer, scale 2, 1920x1440): `zgx_animate_t120.png` vs `bough_animate_t120.png`, `diff_animate_t120.png`: **0.73 % of pixels differ at all, 0.29 % by more than 16/255, 0.16 % by more than 64**
  (glyph anti-aliasing and the fractional edge of the dots). That frame carries the shimmer text (44 per-glyph alphas) and 48 fading dots; it also pins the "normal" line pitch
  (`ceil(1.4 * size)`) and the baseline rule. **Lab**: `render_parity`'s lab view is not the windowed `component_workload` (it sets no font family, so zgx shows its default sans instead of DejaVu; its
  buttons use another alignment and it has one fewer label), so a pixel diff of text is meaningless (`diff_lab_t120_font_differs.png`, 7.1 % > 16, all of it glyph outlines). In that diff every
  rectangle - background, the three buttons, stream panel, the 17 striped rows, the clip edges - is identical to the pixel, and the text boxes sit on the same lines; the glyph-level check of the
  text rules is the animate frame. A DejaVu lab reference needs a patched copy of zgx's example and was not built.
* heavy visual parity (first slice, unchanged): 2.69 % of pixels > 16 / 0.77 % > 64 at tick 0, 2.68 / 0.76 % at tick 120 (`diff_t0.png`, `diff_t120.png`).
* No test failed in this round. Two defects were found by measuring instead: `bough-heavy`'s config hard-coded `ring: 4` (so the windowed heavy still used four surfaces after the policy change; found
  because `BOUGH_STATS` showed all four slots in use), and an `IOSurfaceIsInUse` poll storm at 120 Hz (14 polls per frame, 2.2). The release suite was run; the debug build of the new tests was not.
* The frozen crates' own suites were not re-run (their default features are unchanged).

## 4. What was built

| file | role |
|---|---|
| `crates/bough/src/signal.rs` | `Signal<T>`, `Map` |
| `src/elem.rs` | `Element<V>`, `Mounter`, `IntoProp`/`Apply`, `quad`, `text` (+ **`glyph_alpha`**, new), `at`, `clip`, `each`, `bars`, `virtual_list` |
| `src/rt.rs` | pool layout + size-classed instance-run allocator, nodes, wire arena + settle, text phase, spaces/clips, keyed virtual-list regions |
| `src/text.rs` | bough-text SimpleScript shaper + atlas + fused shape-to-instances; **segmented shaping** for non-ASCII (new); rustybuzz fallback for trigger strings |
| `src/app.rs` | `View`, `App<V>`: mount, `turn`, **`write` / `flush`** (new), `frame`, **ring policy, trim, retry** (new), phase profiler |
| `src/repair.rs` | tile plane -> rect list |
| `src/backend/metal.rs`, `shader.metal` | presenter (see 5.2); **main-thread publish, purgeable slots** (new) |
| `src/backend/host.rs` | AppKit window host on bough-metal's `app::run_sized`: `run_window` (timer driven), **`run_window_linked`** (display-link driven, new), trim and retry timers, `BOUGH_STATS` / `BOUGH_CHECK` |
| `crates/bough/examples/memprobe.rs` | the GPU-footprint probe of 2.1 |
| `comparisons/bough-heavy` | heavy: `Dash` view, `heavy_workload`, `heavy_headless`, `heavy_shot`, `frame_diff`, tests |
| `comparisons/bough-scenes` | `Lab`, `Anim`, `Blink`, `MicroState`, `MicroScroll` views (`lib.rs`); bins `lab_workload`, `lab_headless`, `animate_workload`, `animate_headless`, `blink_workload`, `micro_bench`, `scene_shot`, `scene_diff`; `tests/correctness.rs` |
| `bench/integrate/` | `slope.py` (heavy/lab/animate), `windowed.py` (heavy/lab/animate/blink), `micro.py`, `vm.sh` / `vm2.sh` / `vmbreak.sh`, `rgba2png.py` |

Adapter contract, as zgx's: `ZGX_MODE`, `ZGX_SECONDS`, `ZGX_INITIAL_TICKS`, `ZGX_FONT_FILE`; lab ends with `{"framework":"bough","scene":"lab","ticks":N}`, animate with
`animation_frames=N fps=F` on stderr, blink with `blink_frames=N workload_ticks=0` on stderr; windows 960x720 pt titled per scene, not resizable; lab ticks by an NSTimer at `PERIOD`, animate by the
display link at each refresh with the clock at the frame's time, blink by an NSTimer every 530 ms.

### Scene notes
* **lab**: zgx's `component_workload` geometry/colours/text exactly (title, three 50x32 buttons with 12,6 labels, "0" / "stable" / "0" labels, the 920x160 stream panel clipped at 8,8 with the 7-line
  `visible_text`, the 100,000-row virtual list at 28 pt, overscan 2, 17-19 mounted rows). Row labels contain an em dash: shaped by the segmented path (below), no rustybuzz per row.
* **animate**: 48 dots = 48 quads with one wire each (`bg(clock.map(..))`, an 8-bit alpha write); the shimmer line = **one text node, one wire** (`.glyph_alpha(&clock, |i, n, t| shimmer_alpha(i, n, *t))`):
  the 44 characters' alpha written into the 39 glyph instances' colour words, no reshape, no geometry change (spaces own no instance and are skipped in the mapping).
* **blink**: the caret is one quad whose colour wire flips alpha 255 / 0 (a 2x20 pt repair, one tile column); nothing at all happens between flips.
* **micro state**: 1,000 static texts + one counter text, laid out as zgx's column (p 8, gap 6, cell 36x18 + gap 2); **vscroll-fixed**: 10,000 rows of 28 pt, label `Row {i}: user-NNNN`, wrap to the top past the end.

## 5. Deviations, with reasons

### 5.1 From API.md
* **Signal layout**: `{head, value}` `#[repr(C)]` rather than `{value, head}`, so the runtime can find the head of any `Signal<_>` without knowing `T`.
* **`FIXED = false` re-description + diff is not implemented**; `view` is mounted once. `changed`, `List`, `Entity`, `Reader`/async, `Source` routes, typed routes, dynamic projections/height heap and `FIXED` re-describe are **not built**; the only turn
  forms are the arbitrary handler (`app.turn(|v| ..)`) and its two halves (`app.write(..)`, `app.flush()`, added for micro_bench's write/frame split). Layout is absolute positioning only (no cephal).
* **`Element` is public**, not sealed, and `IntoProp::lower` is generic over the target property (`Apply`); the `&Signal<T>` operand clones `T` for the initial value (`T: Clone`) and patches by reference.
* **`List<T>` + row fn became `virtual_list(&scroll, ..., visible fn, row fn pointer)`**: rows keyed by index, the row function a non-capturing `fn(&V, usize, &mut Mounter)`.
* **`bars` and `glyph_alpha` are additions** (a series run and a glyph-colour run, 5.3): a record may own a run of instances and a single operand patches the run.
* **Mount does not memcpy const templates**: records and instances are written element by element.
* **No display link for timer scenes**: ticks come from a wall-clock NSTimer (absent in idle) and the frame is committed straight from the turn; the link is paused after the first tick. The *animate* host (`run_window_linked`)
  keeps the link running because the contract is a draw per refresh.

### 5.2 Backend (bough-metal re-implemented at runtime size)
bough-metal's presenter has 1920x1440 and 4,096 instances compiled in; here the presenter is re-implemented in `backend/metal.rs` over the façade's pool (IOSurface ring, stencil repair from the rect table, one instanced
draw, AOT metallib, `IsInUse` checks), with the shader's instance layout changed to bough-text's 48-byte `atlas::Instance`. Reused as is: `bough_metal::app` (AppKit host), `bough_metal::perf`, `instance::Hdr`.
New in this round: slot policy / skip-and-retry / purgeable slots (2.2), main-thread publish.

### 5.3 Reformulations, where something lost (rule bent -> representation change)
* **Non-ASCII text (lab rows lost: 58 K instr per scroll frame against zgx's 79 K).** Rule bent: "a string is shaped as a unit by the general shaper". Representation: a line is a *sequence of segments*; maximal ASCII
  stretches go through the table kernel (`Shaper::shape_into`), maximal non-ASCII stretches through a per-chunk cache of rustybuzz runs (the em dash is shaped once per process); kerning across a segment boundary is not
  applied (the pair table has no entry for a non-ASCII neighbour). lab scroll 57.9 K -> **11.2 K**, lab both 123 K -> **79 K**; the pixels are identical to the rustybuzz path (0 of 2.76 M pixels differ).
* **Per-glyph animation (animate).** Rule bent: "a colour change is a property of an element". Representation: colour is a field of the instance, so a *run* owns it: one wire writes 39 colour words; 15.8x fewer
  instructions than zgx (RENDER=0).
* **Surface memory.** Rule bent: "the ring has N surfaces". Representation: a surface exists only when contention needs it, a held one is never drawn into, and a calm window keeps one (2.2).
* **Not reformulated, still behind or level:** the GPU driver block (2.4), the Metal commit floor in windowed CPU % (1.2), blink's footprint (+1 MB).

### 5.4 The `bars` series run
Unchanged from the first slice: one record owning a run of N solid quads, one wire, one ink write, memoised samples when the key advances by one.

## 6. Changes to the frozen crates (all of them)
1. `bough-core/src/damage.rs` + `Cargo.toml`: `TILE_SHIFT` is `5` (32 px tiles) under the cargo feature `tile32`, `4` otherwise (default behaviour and tests unchanged). The façade enables it for itself only.
2. `bough-metal/src/app.rs`: `run_sized(title, w, h, f)`; `run` calls it with 960x720. Nothing else in bough-metal or bough-text was touched in this round either.

Façade changes for the merge with the layout branch are additive and localised: `Text<S, G = ()>` gained a type parameter with a default (`GlyphOp`), `App` gained `write`, `flush`, `trim`, `needs_trim`,
`retry`, `retry_delay_ms`, `surf_stats`, `host.rs` gained `run_window_linked` and the two timers, `text.rs` gained `segment`/`chunk`, `Config.ring` gained the meaning 0 = default. The new scenes live in their own crate and
`Cargo.toml`'s `members` gained one line.

## 7. Known gaps
* No input, hit testing, focus, accessibility; no resize; no p99 frame-interval, GPU-time or latency measurements (only CPU / instructions / footprint).
* `vlist` and `vscroll-variable` are not built (they need the layout engine in `bough-layout`).
* Memory: the GPU driver block (95-105 MB, identical to zgx) is untouched; lab, animate and blink are a tie with zgx (+-2 MB), blink 1 MB worse. The idea that would remove it (CPU rasterisation of small repairs) is described in 2.4 and not built.
* Windowed ring of 2: a compositor that holds both surfaces for more than ~7 ms (observed in ~1.5 % of 120 Hz frames on this loaded machine) skips the frame and retries (4 ms and back-off); the window
  shows the previous frame for that long. An occluded window can keep both held: the retry then backs off to 128 ms.
* The purgeable trim only returns memory once the GPU context has idled (~1.5 s after the last submission); its "purged by the system under pressure" branch (repaint whole) is straight-line code that no test reaches
  (no way to force `kIOSurfacePurgeableEmpty` under test).
* Lab text cannot be pixel-compared with zgx (the available zgx reference renders the lab in another font, 3); animate can (0.29 % > 16).
* Damage granularity is 32 px (tile32). Rows' unclipped overscan ink makes a scroll tick also repaint ~1-2 tile rows above and below the table; `translate_run` damages old-union-new per 64-record page chunk, so a heavy scroll frame
  repaints ~ the whole table viewport (1.33 M px of 4.1 M).
* `update` is half `format!` (the workloads' functions) in heavy; text is re-shaped for every changed label although digit-slot patching would turn most of it into a handful of `uv` writes.
* Text: ASCII fast path, segmented non-ASCII, rustybuzz fallback for `ff/fi/fl` lines (keeps glyph ids and x advances only, not y offsets); no wrapping/bidi/line breaking in the façade; baseline = CSS half-leading from hhea
  metrics, which lands on zgx's glyph rows (animate: 0.29 % of pixels > 16).
* The row clip is a rectangle (vertex-stage clamp); at most 32 clips, 512 spaces (about 500 mounted rows), 16,384 instances. `glyph_alpha` is for constant ASCII text.
* `ZGX_PARITY` is honoured by heavy (`Heavy::turn()`); lab, animate and blink have no parity switch in the contract.
* Slope noise: a single slope sample has +-10 K of startup variance (visible on the idle rows); stream/both medians are stable to 1 %. Windowed CPU % is single-core-relative and was measured on a machine with load 40-80.
* GPU Mcycles/s and WindowServer CPU are not measured for either framework.
