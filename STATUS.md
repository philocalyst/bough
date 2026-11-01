# bough: status of the fused system (branch `fusion`, 2026-11-01)

One system instead of four pieces. bough-core, bough-text, bough-metal round 2 and cephal now share one
GPU-visible region and one damage model, and the façade has one path for each job. Every number below was measured
on this M3 Pro (macOS 15.7, 120 Hz) against zgx 06debe9d's `dist` binaries, by the methods the scene agents used.
The machine had other users' load (load average 18–42), so instruction counts are the reliable columns and CPU %
is indicative.

```
export CARGO_TARGET_DIR=$PWD/target/fusion RUSTC_WRAPPER= PATH=~/.local/share/cargo/bin:$PATH
export ZGX_FONT_FILE=/Users/mileswirht/projects/_ref/zgx/comparisons/fonts/DejaVuSans.ttf
cargo check --workspace --all-targets          # zero warnings
cargo test --workspace --release                # every gate (see "Correctness"); debug: cargo test --workspace
cargo build --profile dist -p bough-heavy -p bough-scenes -p bough-vlist
bench/zgx/build.sh                              # zgx's headless drivers (workload/animation/vlist_headless) into target/zgx-headless
bench/results/fusion/run_slopes.sh              # per-frame instructions, every scene, both frameworks, RENDER 0/1
bench/results/fusion/run_windowed.sh            # 10 s windows, CPU %, instr/s, footprint, ticks, ring stats
python3 bench/integrate/micro.py 3              # micro_bench: state, vscroll-fixed, vscroll-variable
BOUGH_CHECK=1 BOUGH_STATS=1 ZGX_SECONDS=5 target/fusion/dist/animate_workload   # the real window vs a full redraw
```

Raw output: `bench/results/fusion/{slopes,windowed,micro}.txt`.

## 1. bough vs zgx, scene by scene

### 1.1 Per-frame instructions (slope method: TICKS 300 vs 600; animate FRAMES 600 vs 1200; vlist TICKS 600 vs 3600; process-wide, median)

`RENDER=0`: for bough, everything up to the GPU's input (the update, wires, text patches, layout, damage, and the frame
slot's damaged-row records). The GPU culls bough-core's pages and expands bough-text's runs itself, so there is no CPU
cull and no draw list. For zgx, layout plus damage with no renderer. `RENDER=1`: plus the offscreen GPU frame (bough:
the binned compute repair, one command buffer; animate uses the pre-armed stream). zgx: wgpu `GpuRenderer`. The
"piecewise best" column is the best bough number before fusion (the integrate and layout branches' RESULTS, RENDER 0 / 1).

| scene / mode | zgx R=0 | **bough R=0** | × | zgx R=1 | **bough R=1** | × | piecewise best (R=0 / R=1) |
|---|---:|---:|---:|---:|---:|---:|---|
| heavy idle | 3,235 | **~0** (noise ±10 K) | – | 554,155 | **~0** (no frame) | – | ~0 / ~0 |
| heavy stream | 2,982,643 | **175,303** | 17.0 | 6,190,927 | **383,397** | 16.1 | 404,008 / 652,788 |
| heavy scroll | 161,932 | **24,468** | 6.6 | 1,443,768 | **220,466** | 6.5 | 32,452 / 277,723 |
| heavy both | 3,153,118 | **210,611** | 15.0 | 6,896,602 | **414,768** | 16.6 | 439,000 / 687,068 |
| lab idle | 2,708 | **~0** | – | 113,510 | **~0** | – | ~0 / ~0 |
| lab stream | 754,172 | **26,505** | 28.5 | 1,061,953 | **217,129** | 4.9 | 69,587 / 309,022 |
| lab scroll | 84,489 | **11,205** | 7.5 | 649,136 | **198,033** | 3.3 | 11,208 / 262,335 |
| lab both | 813,004 | **34,808** | 23.4 | 1,408,099 | **237,779** | 5.9 | 79,390 / 338,415 |
| animate | 226,715 | **13,877** | 16.3 | 816,343 | **141,530** (stream; 214 K without) | 5.8 | 14,270 / 259,914 |
| vlist scroll | 108,843 | **12,989** | 8.4 | 1,140,929 | **211,804** | 5.4 | 20,288 / 256,311 |
| vlist fling | 178,601 | **19,573** | 9.1 | 1,308,185 | **220,321** | 5.9 | 28,135 / 279,368 |
| vlist jump | 335,715 | **35,662** | 9.4 | 1,333,209 | **236,249** | 5.6 | 47,243 / 291,453 |

The fused system is at or below the best piecewise number in every cell (lab scroll R=0 ties). With a renderer, the
remaining ~200 K per frame is Metal's per-command-buffer floor (encoder, commit, two driver threads), not bough's code.
The throughput mode removes the per-frame command buffer: animate falls from 214 K to 142 K.

### 1.2 Windowed, 10 s (`proc_pid_rusage` from outside at t = 1.5 s and 9 s; two repetitions, idle one; bough / zgx interleaved)

`frames` are the adapter's own report (ticks; `animation_frames` and fps for animate; blink frames). `skipped` is
bough's ring counter: frames whose damage was not presented in their turn. zgx's equivalent is not observable from
outside, so its column shows the frames it reported. Footprint is `phys_footprint`.

| scene / mode | CPU % zgx | **CPU % bough** | instr/s zgx | **instr/s bough** | footprint zgx MB | **footprint bough MB** | frames zgx / bough | skipped (bough) | surfaces drawn (bough) |
|---|---:|---:|---:|---:|---:|---:|---|---:|---|
| heavy idle | 0.09 | **0.01** | 4.9 M | **0.09 M** | 49.9 | **45.1** | 0 / 0 (mount frame only) | 0 | 1 |
| heavy stream | 5.89, 5.84 | **1.48, 1.42** | 381 M, 380 M | **37.4 M, 36.5 M** | 166.1, 164.5 | **121.8, 120.1** | 599 / 600 | 0 | 2 |
| heavy scroll | 2.63 † | **2.35, 1.32** | 66.6 M † | **29.6 M, 27.2 M** | 161.7 † | **137.2, 120.5** | 598 † / 597, 600 | 0 | 3, 2 |
| heavy both | 7.33, 6.97 | **1.62, 1.57** | 445 M, 445 M | **38.8 M, 38.3 M** | 167.4, 166.8 | **121.2, 121.9** | 599 / 600 | 0 | 2 |
| lab idle | 0.08 | **0.00** | 4.9 M | **0.07 M** | 40.7 | **38.9** | 0 / 0 | 0 | 1 |
| lab stream | 2.75, 2.49 | **1.33, 1.24** | 85.7 M, 78.4 M | **27.1 M, 27.2 M** | 146.4, 146.4 | **111.5, 111.1** | 599 / 600 | 0 | 2 |
| lab scroll | 2.16, 2.32 | **1.39, 1.34** | 45.8 M, 56.3 M | **26.4 M, 26.3 M** | 145.7, 145.7 | **110.3, 111.3** | 599 / 600 | 0 | 2 |
| lab both | 2.82, 2.73 | **1.38, 1.39** | 90.4 M, 83.7 M | **27.8 M, 27.9 M** | 146.9, 146.1 | **110.0, 110.4** | 599 / 600 | 0 | 2 |
| animate (120 Hz) | 3.53, 3.61 | **1.91, 1.74** | 101 M, 101 M | **34.4 M, 34.0 M** | 145.8, 145.5 | **132.2, 133.6** | 119.1, 118.9 fps / **120.1, 120.0 fps** (1200 / 1201) | **0** | 4 (stream) |
| blink (530 ms) | 0.16, 0.14 | **0.03, 0.03** | 6.3 M, 6.1 M | **0.55 M, 0.52 M** | 141.5, 141.6 | **49.4, 50.0** | 18 / 18 | 0 | 2 (17 of 19 frames: no GPU) |
| vlist scroll | 2.21, 2.14 | **1.27, 1.24** | 61.8 M, 58.9 M | **26.4 M, 26.3 M** | 148.9, 149.2 | **108.4, 107.8** | 599 / 600 | 0 | 2 |
| vlist fling | 2.50, 2.46 | **1.42, 1.32** | 79.6 M, 74.9 M | **28.2 M, 26.8 M** | 148.7, 148.5 | **107.2, 107.6** | 599, 592 / 600 | 0 | 2 |
| vlist jump | 2.24, 2.40 | **1.32, 1.35** | 74.6 M, 78.6 M | **27.6 M, 27.7 M** | 151.4, 151.7 | **108.1, 109.1** | 599 / 600 | 0 | 2 |

† zgx's first heavy-scroll repetition delivered only 84 ticks in 10 s (throttled, most likely an occluded window on the
user's desktop), so it is not counted; the valid repetition is shown. bough's ring never waited for the compositor
(`waits 0`, `max_wait_us 0`) and never skipped a frame in any run (24 windows).

bough beats zgx on CPU, instructions per second and footprint in every cell. It is 26–37 % below zgx's footprint on
every active scene, 65 % below on blink, and at or below zgx at idle. Interrupt wakeups per second are 60 against
zgx's 180–317 on active scenes, and 0.8 against 1.2 at idle.

### 1.3 micro_bench (zgx's `micro` semantics: process CPU ns per op, median of 3 repeats' medians [min–max])

bough's `write` is the signal store alone (`App::write`); its `frame` is settle + text patches + layout + damage +
frame slot (+ GPU with RENDER=1). zgx does part of the turn inside its write, so compare write + frame.

| scene | RENDER | zgx write | **bough write** | zgx frame | **bough frame** |
|---|---|---:|---:|---:|---:|
| state | 0 | 1,040 [1,000–1,160] | **0** [0–0] | 2,200 [2,100–2,200] | **100** [100–100] |
| state | 1 | 1,840 [1,640–2,900] | **20** | 150,300 [143,500–197,300] | **63,600** [53,800–70,100] |
| vscroll-fixed | 0 | 2,700 [1,240–3,860] | **0** | 4,300 [4,200–6,000] | **600** [600–600] |
| vscroll-fixed | 1 | 2,160 | **0** | 230,500 [151,000–232,300] | **122,800** [116,700–144,300] |
| vscroll-variable | 0 | 7,180 [6,960–7,360] | **540** | 8,100 [7,500–8,200] | **800** [800–1,000] |
| vscroll-variable | 1 | 8,540 | **600** | 212,300 [174,500–241,000] | **124,600** [81,100–143,300] |

(bough's vscroll-variable runs from `micro_bench_vlist`. Its "write" is the whole turn: list update, row
mount/measure/place, and layout write-back.)

## 2. What was fused (one mechanism each)

* **One region.** `bough_mem::Region` is the fused presenter's `storageModeShared` buffer (heap memory headless, same
  layout). Every GPU-visible column is a `Col<T>` carved from it once, at a fixed offset and capacity. The columns are:
  bough-core's pages (`PagePool` arena: page id = element), directory page ids and union inks, and the clip table;
  bough-text's glyph slots `x: i32` / `ent: u16`, run headers, integral space translations, and strike token tables;
  and the façade's 48-byte quad instances. The GPU reads them where the CPU writes them, with no mirror pass. Measured
  first (`bough-metal/examples/resprobe.rs`): untouched capacity of a shared buffer costs nothing, but a residency set
  makes an allocation wholly resident, and binding a surface to an encoder makes all its pages resident.
* **Core round 2.** Pages with page-relative inks and offset lanes (the kernels fold `offl` into the threshold with
  `subsat`), clip ids on records (`new_clip` / `clip_rect`; the GPU clamps to the record's clip), and `scroll_run`
  for lists, which damages only the viewport ∩ old ∪ viewport ∩ new, with no per-record rewrite. There are no stamps.
  New core API: `set_payload_handle`, `set_ink_handle_nodamage`, `visible_ink_handle`, `Plane::or_rows`
  (bough-text's mark sink), and `Bough::with_columns`.
* **Text round 2.** A one-line text is a `Label`: digit patches (a `u16` per changed glyph, a cached tile span into
  the plane), windows from the first changed byte, and the token table for ligatures and Latin-1. Numeric operands
  (`sig!(..).num(f, NumFmt)`) render the value into the label's 16-byte vector with no `String`; heavy's 6 tile,
  12 card and per-row values are checked equal to zgx's `format!` strings over 3,000 ticks. Multi-line and wrapped
  text is shaped once into a pool block, and each line is a run header (a window); the record's payload names the
  header range. Layout's measured leaves shape straight into the pool, and the word table reads the pool's pens, so
  text is shaped once for measuring and drawing. Tile marks are produced at update time into bough-core's current
  plane. rustybuzz is left only beyond Latin-1 and for per-glyph colour (glyph instances).
* **Metal round 2** (`bough_metal::fused`). Binned 32×32 compute repair over the core pages and text runs at runtime
  window size, with no render pass, which also shrinks the driver's block. **The damage is the dispatch:** the frame
  slot is the damaged tile rows themselves (row, span, prefix count, 128 bits), O(damaged rows) on the CPU. Each
  tile threadgroup finds its tile by a binary search and a k-th-set-bit select. The rest:
  * Row bins are sized by need: an overflow records the needed size, and the CPU grows the bins and redraws that frame
    before it is published, including inside the pre-armed stream.
  * The residency set holds the region, atlas, bins, and the surfaces actually drawn.
  * Keep-warm is bounded to 10 s, armed only by a GPU frame within the window of the previous one, and cancelled by a
    frame served from a held surface, so a lone mount frame or a blinking caret lets the GPU sleep.
  * The display link pauses on a refresh with no damage, and timer scenes never run it.
  * The pre-armed command stream is the throughput mode for animate (frames released by a shared event, published
    at the next turn).
  * Publish happens on the main thread after the frame completes.
* **Layout.** cephal over the Bough with the child-id side column (LAYOUT.md's "deep-kids") is the only path. The
  mirror and the `deep` features are deleted. Absolute positioning below static ancestors is implemented through
  stored `AbsoluteContext`s. `tests/layout.rs::absolute_box_below_static_ancestors_uses_its_containing_block` checks
  it, and fails with the contexts disabled.
* **Ring / pacing: zero skipped frames, footprint below zgx.** Each frame draws one surface and never skips:
  1. A surface that already holds the current state is published with no GPU work, or nothing is done at all if it is
     the one on screen (content addressing).
  2. Otherwise the most recently drawn free surface is repaired: free means not on screen and not `IOSurfaceIsInUse`.
  3. Otherwise a fresh surface is used (unbacked until written, at most 4).
  4. Only if all 4 are held does the frame wait for a release.

  Content addressing is O(1) per paint write: each epoch logs its paint writes as (record, old state, new state). A
  surface holds the current state iff no epoch since it was drawn had non-paint damage and the logged writes compose
  to the identity. The 2-surface skip-and-retry of the integrate branch is gone. The surfaces actually drawn per
  scene are in the table: 2 at 60 Hz, 3 once in heavy scroll, and 4 in animate's stream. Because compute-only frames
  shrink the driver's block, even 4 surfaces stay 12 MB under zgx.

## 3. Correctness (all on the fused system; `cargo test --workspace --release`, and in debug)

| gate | where | result |
|---|---|---|
| presented == full redraw (same kernels), **every ring age 1..4 + the windowed content-addressed ring** | `comparisons/*/tests/correctness.rs`: heavy stream/scroll/both/jumps, lab stream/scroll/both/jumps, animate, blink, micro state, micro vscroll, vlist scroll/fling/jump/back-up | 0 differing bytes |
| retained == fresh mount at the same signal values | same files (every scene), vlist with the same row heights | 0 differing bytes |
| throughput mode (pre-armed stream) at rings 2/3/4/windowed, incl. a bin overflow inside the stream | `animate_in_throughput_mode_is_pixel_exact` | exact |
| bin overflow → grow + redraw before publish | `bin_overflow_grows_the_bins_and_redraws_exactly` (heavy, bins of 8) | exact |
| content addressing | `blink_is_served_by_held_surfaces_after_two_frames`: 2 GPU frames, 39 held publishes, each read back == full redraw | exact |
| the real window | `BOUGH_CHECK=1` on heavy / animate / blink: last presented surface vs a full redraw | 0 differing bytes |
| layout == cold cephal | `tests/layout.rs` (60 random edits: incremental == cold, retained == fresh mount), vlist `row_layouts_equal_cold_layout`, absolute at depth | equal |
| vlist row heights == zgx | `heights_follow_the_zgx_formula` (rows 0..300), `ZGX_MODE=probe` | equal |
| text exactness | bough-text's own gates (exhaustive ≤ 3 chars, corpora, digit patches vs fresh shape, windows == lines shaped alone, `NumFmt` == `format!` over 402 M f32); façade: line windows on the pool block == rebased-run glyph quads (3,000 random strings × 4 widths); heavy's numeric operands == zgx's strings | equal |
| core | `differential.rs` incl. a carved-storage (arena) profile on every 3rd sequence | equal to the oracle |
| `sig!` soundness | compile-fail doctests (union field, path through `Box`) + a positive nested path | as designed |

## 4. Soundness audit (`crates/bough`, plus what it relies on)

* **Wires (`sig!`).** `sig!(self.a.b)` / `sig!(v: T => a.b)` expand to `offset_of!(Owner, a.b)` plus a never-called
  accessor `|o: &Owner| &o.a.b` (`fn(&Owner) -> &Signal<T>` pins the type). The field access is *outside* the macro's
  `unsafe` block, so:
  * a union or packed field needs the caller's own `unsafe`;
  * a path through `Box`/`Deref` fails `offset_of!`;
  * an enum field cannot be named as a path.

  `Wired<'_, T, V>` carries the owner type; only `Mounter<'_, V>` of the same `V` lowers it. `Wired::proven`, which
  takes the offset, is `unsafe` with that contract. The runtime offset inference is gone: it survives only as
  `debug_assert_eq!(addr(sig), owner + off)` in `Mounter::wire` and the measured list.
* **The wire arena is crate-private.** That covers `Rt::wires`, `Rt::wired`, `Rt::add_wire` (`unsafe`, with a
  contract), `Signal::head`, the region row functions, and `MRegion::{mount_row, row_fn}`. Safe code could otherwise
  copy a head into a signal of another type, or forge a recipe or row-function pointer. `Rt::settle` is
  `unsafe fn(owner)`. `App` calls it with its boxed owner, which never moves.
* **Patches** (`elem.rs`, `mlist.rs`). Each `unsafe fn patch_*` reads `Signal<T>` at `owner + off`, which the wire
  contract guarantees. Recipes are written by `recipe_of::<F>` (`F: Copy + 'static`, `size_of::<F>() <= 16`
  const-asserted) and read back as the same `F`. `mount_row`/`mount_mrow` transmute the stored `usize` back to the
  `fn(&V, usize, &mut Mounter<V>)` stored by the same `V`'s mount, and that pair is private.
* **Region** (`bough_mem`).
  * A carved `Col` holds an `Rc` on its region, so the memory outlives every column.
  * Columns never overlap (the carve cursor only advances) and never move.
  * A carved `Col` cannot grow past its capacity (it panics with the column's name).
  * `Region::external` (`App::new`) is the MTLBuffer's contents, kept alive by a retained buffer in the region,
    page-aligned and zero-filled.
  * `inst.set_len(cap)` relies on all-zero being a valid `Instance`.
* **CPU/GPU protocol (not Rust memory safety; visual correctness).** The CPU writes the region only after
  `Fused::sync()`: `App::turn`/`flush`/`frame` sync first, and in the stream a frame's slot is written before its
  event release. The GPU never writes CPU-visible memory except `counts`, which is read after a wait. `App::rt` is
  public, so a caller that mutates it between `frame()` and the next `turn()` in stream mode races the GPU's reads.
  That yields torn pixels, not CPU UB.
* **Not bounds-checked on the GPU.** The kernels trust payloads, run ranges and page ids; a Bough corrupted through
  the public `rt` could make the GPU read out of range within the buffer, or fault the command buffer. That is a
  GPU-side robustness gap (§5), not CPU memory unsafety.
* **Host** (`backend/host.rs`). objc2's `unsafe` timer and run-loop calls happen on the main thread with retained
  blocks.
* **bough-metal `fused.rs`.** `write_slot`, header field writes and `check_bins` use raw pointers into the region at
  layout-reserved offsets after a wait (documented `# Safety` / `// SAFETY:` at each site).

## 5. Remaining gaps

1. **zgx's own windowed suite was not run.** Its trials keep windows open 23–39 s, but the user's limit here is
   12 s. It also needs Screen Recording (frame pacing captures), Automation permission for System Events (the
   preflight fails with -1743) and an idle desktop. The runner is ready and its dry run passes (§6). So GPU
   Mcycles/s, WindowServer CPU, p99 frame interval and blank rows are not measured for either framework. Pacing
   evidence is only bough's ring counters (0 skipped, 0 waits) and the adapters' frame counts.
2. **Visual parity with zgx was re-checked for animate only.** The compute kernels accumulate in half precision. The
   animate frame at t = 120 (`scene_shot` vs zgx's `render_parity` frame `crates/bough/results/zgx_animate_t120.png`)
   differs in 0.76 % of pixels at all, 0.29 % by more than 16/255 and 0.163 % by more than 64. That matches the
   render-pass shader's 0.73 / 0.29 / 0.16 %, which come from glyph anti-aliasing and the dots' fractional edges.
   Heavy and lab were not re-diffed; lab has no DejaVu reference from zgx.
3. **Text.**
   * Multi-line and wrapped text use the ASCII slot kernel. A line with a ligature or Latin-1 that is not a
     one-line label falls back to rustybuzz glyph instances, because the token path is not windowed.
   * Beyond Latin-1, chunks are shaped by rustybuzz and cached; kerning across a segment boundary is not applied.
   * The log pane re-shapes its block on every change: the stream append and ring-of-lines-by-spaces forms are not
     used.
   * Bars damage the union of old and new on every tick, with no per-bar strips, so heavy repaints about 1,200 of
     4,000 tiles per frame.
4. **GPU.**
   * There are no bounds checks in the kernels (§4).
   * Glyph-dense partial damage is still the binned kernel's losing regime (metal RESULTS 13.2, 2D binning not built).
   * The residency set makes the region's whole capacity resident (≈2 MB).
5. **The RENDER=1 floor.** About 200 K instructions per frame is Metal's per-command-buffer cost. Only animate uses
   the throughput mode. Heavy and lab streams are also non-input-driven and would gain from it at +1 frame of
   latency, but the task scoped the mode to animation.
6. **Measurement conditions.** Load average 18–42 from other activity during every run, so CPU % is noisy (one zgx
   repetition was invalid, †). Instruction counts are stable to a few percent.
7. **Carried over from the pieces, unchanged:**
   * core: `parent`/`ancestors` lose to pointer chasing, and `freeze_into` costs O(pages);
   * layout: style is fixed at mount, a measured leaf edit costs cephal O(siblings), there is no grid builder, and a
     reactive text edit in a list row does not update the prefix index;
   * façade: input, hit testing, focus, accessibility, resize, `List`/`Entity`/`Reader`/async and re-description
     (`FIXED = false`) are not built.
8. **bough-metal still carries its round-1/2 research presenters** (`v2.rs`, `gpu.rs`, `present.rs`, `scene.rs`) as the
   harness that reproduces its RESULTS. The façade uses only `fused`, `app`, `perf` and `instance`.

## 6. zgx's windowed suite with bough (`bench/windowed/`)

The runner is a copy of zgx's `scripts/run_macos_comparison.py`, with its preflight, staging and Swift samplers.
`ROOT` stays the zgx checkout. Every scene bough implements has a `bough` entry pointing at
`target/fusion/dist/*_workload`: heavy, lab, animate, vlist, and field (blink). bough is never built by the runner,
which stops with the build command if a bough binary is missing. Results go to `bench/windowed/results/`. See
`bench/windowed/README.md`.

```
# 1. build bough first (the runner never does)
cd /Users/mileswirht/projects/bough && RUSTC_WRAPPER= PATH=~/.local/share/cargo/bin:$PATH \
  CARGO_TARGET_DIR=/Users/mileswirht/projects/bough/target/fusion cargo build --profile dist -p bough-heavy -p bough-scenes -p bough-vlist
# 2. check the plan (no windows): binaries, order, env
python3 bench/windowed/run_macos_comparison.py --suite --scene heavy --dry-run
# 3. the full suite, one scene per command (needs Screen Recording + Automation for System Events, an idle desktop;
#    about 1 h for heavy; add --powermetrics after `sudo -v`; --frameworks zgx,bough,gpui-fast leaves out MyGo/Rune)
python3 bench/windowed/run_macos_comparison.py --suite --scene heavy
python3 bench/windowed/run_macos_comparison.py --suite --scene lab
python3 bench/windowed/run_macos_comparison.py --suite --scene animate
python3 bench/windowed/run_macos_comparison.py --suite --scene vlist
python3 bench/windowed/run_macos_comparison.py --suite --scene field
# 4. a quick heavy cell (about 9 min of windows)
python3 bench/windowed/run_macos_comparison.py --quick --scene heavy --frameworks zgx,bough
```

Verified here: `--help`, and `--dry-run` for every scene with `--suite` and with `--quick --frameworks zgx,bough`.
With the bough binaries built, the quick heavy dry run plans 18 launches with both binaries staged. The missing-binary
check exits 1 with the build command. The `--quick` heavy cell itself was not run, for three reasons: its windows
stay open 23–39 s (over the 12 s limit), its preflight needs System Events automation (`-1743` here), and its pacing
capture needs Screen Recording. The preflight copy calls `/bin/ps`, because the first `ps` on this Mac's PATH is
nix's, which prints no time column and crashes zgx's own preflight.
