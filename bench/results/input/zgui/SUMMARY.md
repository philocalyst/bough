# zgui on this Mac: times and retired instructions

zgui commit 951a0ea (reference clone ~/projects/_ref/zgui, untouched), measured from an APFS clone at
`scratchpad/zgui-input` (zgui-bench patched, see `zgui-bench-instr.patch`). Apple M3 Pro, 11 cores, 36 GB,
macOS 24.6 (Darwin). Release build, rustc 1.99.1 (`cargo +1.99.1`; the pinned nightly-2026-04-16 is absent),
headless platform, `NullRenderer` (see "What the harness does not measure").

## Read this first

- The machine was heavily loaded by other agents. `uptime` before each batch (also in `raw/uptime.txt`):
  batch 1 load average 124.18 / 112.69 / 97.84, batch 2 100.13 / 107.70 / 97.34, batch 3 84.48 / 103.51 / 96.14;
  the single `cargo xtask perf` run afterwards 75.63 / 96.26 / 93.94. On 11 cores that is 7-11x oversubscribed.
  Wall-clock times are therefore upper bounds with a long noisy tail (run 1 is the worst: e.g. cold.first_frame
  379 ms in run 1, 240 ms in run 3, 164 ms in the xtask run; the repo's own recorded headless figure is 102-111 ms).
  **Retired instructions are the robust figure**: p50 instruction counts agree to well under 1 % across runs
  (3 runs, in separate processes) for almost every measurement.
- Instructions are process-wide (`proc_pid_rusage(getpid(), RUSAGE_INFO_V4).ri_instructions`), so they include
  worker-pool threads (style/layout pools) as well as the main thread, not just the measured thread. The read
  itself costs about 5.4 k instructions; that fixed cost (an empty measured span with the same shape) is
  calibrated at start-up and subtracted. Validated against a loop of known length (exactly 7.00 instr/iteration
  at n = 1e4, 1e5). idle.turn (~1.1 k instr) is the one span small enough for that calibration to matter.
- Each instruction sample brackets exactly the same span as the existing stopwatch sample (the counter read
  sits just outside the stopwatch on both ends, the closing read after `elapsed()`).
- Time columns are the scenario's own `p50` of its distribution (p50 of the 32/60/240/... samples within one
  run); the table shows the median of the 3 runs, and the min-max of the 3 runs' p50s.

## Results (3 runs each, each scenario in its own process)

Time unit is per the `unit` column (us or ms). Instructions are retired instructions of the whole process over
one sample span. `xtask-perf run` is the single extra run from `cargo xtask perf` (time / instr p50).

| scenario | measurement | unit | time p50 (median of runs) | time p50 min-max | xtask-perf run (time / instr) | instr p50 (median of runs) | instr p50 min-max | instr p10..p90 (median of runs) | samples/run | runs |
|---|---|---|---|---|---|---|---|---|---|---|
| idle | idle.turn | us | 0.04 | 0.04 - 0.12 | 0.04 / 1137 | 1137 | 1137 - 1137 | 1137 .. 1224 | 600 | 3 |
| hover-storm | hover.crossing | us | 171.50 | 168.54 - 211.79 | 172.88 / 1,830,072 | 1,831,004 | 1,829,188 - 1,831,590 | 1,824,895 .. 1,888,401 | 240 | 3 |
| scroll | scroll.translation | us | 51.50 | 50.21 - 53.88 | 52.46 / 604,765 | 604,853 | 604,479 - 605,592 | 133,246 .. 743,881 | 142 | 3 |
| scroll | scroll.recycle | us | 277.08 | 246.75 - 469.92 | 266.08 / 2,019,887 | 2,054,098 | 2,040,557 - 2,082,562 | 1,841,296 .. 3,629,758 | 111 | 3 |
| cold-start | cold.first_frame | ms | 376.84 | 239.60 - 379.24 | 164.34 / 1,097,128,190 | 1,146,840,358 | 1,132,940,146 - 1,154,563,648 | 1,146,840,358 .. 1,146,840,358 | 1 | 3 |
| kitchen-sink | kitchen.resize | ms | 8.97 | 6.31 - 23.10 | 5.04 / 29,037,091 | 29,619,198 | 28,390,719 - 29,968,817 | 24,625,484 .. 40,405,720 | 32 | 3 |
| kitchen-sink | kitchen.click | us | 48.54 | 30.62 - 115.29 | 33.17 / 469,205 | 469,414 | 469,089 - 469,418 | 459,899 .. 480,846 | 32 | 3 |
| kitchen-sink | kitchen.keystroke | us | 775.58 | 290.67 - 1564.54 | 324.29 / 4,417,745 | 4,447,493 | 4,403,920 - 4,475,208 | 4,003,245 .. 4,852,238 | 32 | 3 |
| kitchen-sink | kitchen.untouched_turn | us | 97.17 | 96.25 - 99.96 | 96.42 / 897,614 | 898,314 | 892,487 - 916,277 | 878,127 .. 1,432,451 | 240 | 3 |
| kitchen-sink | kitchen.glide_tick | us | 1603.67 | 1084.29 - 4196.96 | 1547.58 / 10,513,082 | 10,501,127 | 10,416,550 - 10,507,271 | 1,550,760 .. 11,244,833 | 120 | 3 |
| kitchen-sink | kitchen.tab_traversal | us | 573.62 | 451.00 - 646.67 | 357.17 / 4,533,197 | 4,534,195 | 4,528,355 - 4,580,194 | 2,025,931 .. 4,650,686 | 22 | 3 |
| thread-resize | thread.resize | ms | 28.00 | 25.27 - 57.52 | 25.97 / 131,194,035 | 130,241,241 | 129,530,510 - 130,991,804 | 125,801,174 .. 132,062,779 | 32 | 3 |
| thread-resize | thread.resize_p95 | ms | 71.72 | 47.02 - 228.83 | 74.17 / - | - | - - - | - .. - | 32 | 3 |
| app-shell | shell.tooltip_crossing | us | 2239.12 | 1868.58 - 8120.75 | 1298.46 / 11,241,512 | 11,312,783 | 11,270,202 - 11,313,477 | 11,092,491 .. 14,873,690 | 37 | 3 |
| app-shell | shell.drag_step | us | 402.12 | 397.58 - 1198.96 | 335.04 / 3,406,602 | 3,439,018 | 3,411,263 - 3,440,158 | 3,022,562 .. 4,207,175 | 60 | 3 |
| app-shell | shell.resize_step | us | 3266.04 | 2809.29 - 12779.29 | 3080.58 / 16,790,984 | 16,369,020 | 15,950,521 - 16,395,432 | 13,629,942 .. 19,399,270 | 40 | 3 |

Count-type measurements (not timed; identical or near-identical across runs):

| scenario | measurement | unit | value (median of runs) | min-max |
|---|---|---|---|---|
| idle | idle.frames | frames | 0.00 | 0.00 - 0.00 |
| idle | reshape.paragraphs_forgotten | paragraphs | 0.00 | 0.00 - 0.00 |
| idle | reshape.boxes_marked_all_dirty | boxes | 0.00 | 0.00 - 0.00 |
| hover-storm | hover.primitives_emitted | prims | 41.56 | 41.56 - 41.56 |
| hover-storm | reshape.paragraphs_forgotten | paragraphs | 0.00 | 0.00 - 0.00 |
| hover-storm | reshape.boxes_marked_all_dirty | boxes | 0.00 | 0.00 - 0.00 |
| scroll | scroll.translation.restyles | elems | 0.00 | 0.00 - 0.00 |
| scroll | scroll.translation.relayouts | nodes | 0.00 | 0.00 - 0.00 |
| scroll | scroll.translation.hit_rebuilds | rebuilds | 0.00 | 0.00 - 0.00 |
| scroll | reshape.paragraphs_forgotten | paragraphs | 0.00 | 0.00 - 0.00 |
| scroll | reshape.boxes_marked_all_dirty | boxes | 0.00 | 0.00 - 0.00 |
| cold-start | reshape.paragraphs_forgotten | paragraphs | 0.00 | 0.00 - 0.00 |
| cold-start | reshape.boxes_marked_all_dirty | boxes | 0.00 | 0.00 - 0.00 |
| kitchen-sink | reshape.paragraphs_forgotten | paragraphs | 0.00 | 0.00 - 0.00 |
| kitchen-sink | reshape.boxes_marked_all_dirty | boxes | 0.00 | 0.00 - 0.00 |
| thread-resize | thread.rebreaks_per_step | paragraphs | 499.00 | 499.00 - 499.00 |
| thread-resize | thread.reshapes_per_step | runs | 0.00 | 0.00 - 0.00 |
| thread-resize | thread.relayouts_per_step | nodes | 1530.00 | 1530.00 - 1530.00 |
| thread-resize | thread.hit_rebuilds_per_step | rebuilds | 0.00 | 0.00 - 0.00 |
| thread-resize | thread.drag.layouts | layouts | 10.00 | 10.00 - 10.00 |
| thread-resize | reshape.paragraphs_forgotten | paragraphs | 0.00 | 0.00 - 0.00 |
| thread-resize | reshape.boxes_marked_all_dirty | boxes | 0.00 | 0.00 - 0.00 |
| app-shell | shell.tooltip_tree_builds | builds | 0.00 | 0.00 - 0.00 |
| app-shell | shell.tooltip_hit_rebuilds | rebuilds | 5.00 | 5.00 - 5.00 |
| app-shell | shell.drag_restyled | elems | 2.55 | 2.55 - 2.55 |
| app-shell | shell.drag_hit_rebuilds | rebuilds | 1.00 | 1.00 - 1.00 |
| app-shell | shell.drag_observation_passes | passes | 2.00 | 2.00 - 2.00 |
| app-shell | shell.drag_primitives_emitted | prims | 864.08 | 864.08 - 864.08 |
| app-shell | shell.resize_truncated | deliveries | 0.00 | 0.00 - 0.00 |
| app-shell | reshape.paragraphs_forgotten | paragraphs | 0.00 | 0.00 - 0.00 |
| app-shell | reshape.boxes_marked_all_dirty | boxes | 0.00 | 0.00 - 0.00 |

## Probes

label-churn (30 labels relabelled per frame): time 1986.9 us/frame (median of round-medians; runs 5043.3, 1487.9, 1986.9), instructions p50 10,914,449 per frame (runs 10,957,377, 10,914,449, 10,150,284; p10..p90 median 8,371,379..11,887,059; n=240 per run)

| resize-slope rows | boxes | configure time us (median of runs; min-max) | content-change time us (median; min-max) | configure instr (median; min-max) | content-change instr (median; min-max) |
|---|---|---|---|---|---|
| 64 | 135 | 985.8 (711.0-1359.8) | 489.7 (474.4-996.7) | 8,149,496 (8,139,985-8,230,120) | 7,287,026 (7,263,388-7,384,881) |
| 128 | 263 | 2644.6 (1740.5-2748.3) | 1386.6 (1216.5-1877.5) | 14,333,810 (14,294,321-14,378,233) | 10,015,064 (9,998,449-10,272,344) |
| 256 | 519 | 3531.4 (2388.1-4608.6) | 2551.7 (1199.4-2908.2) | 26,421,732 (26,419,074-26,563,423) | 15,396,637 (15,392,583-16,130,675) |
| 512 | 1031 | 6884.0 (6734.6-26982.8) | 3849.8 (3417.2-3987.6) | 50,912,924 (50,910,169-50,986,715) | 26,180,952 (26,178,800-27,680,148) |

instruction slopes (least squares over the 4 sizes): configure 47687.34 instr/box, content change 21081.68 instr/box, ratio 2.2620 (runs: 47798.6/20966.9=2.2797, 47687.3/21081.7=2.2620, 47612.6/22782.2=2.0899)
time slopes (median of runs): configure 6.3756 us/box, content change 3.1410 us/box; configure/content ratio per run: 8.1649, 2.0677, 1.8836

(`label-churn` default is 30 labels relabelled per frame, 5 rounds of 48 measured frames after 12 warm-up frames
each; instruction samples are the 240 measured frames per run. `resize-slope`: median of 48 samples per size.
The probe's own gate verdict, `RESIZE REGRESSION ratio=2.07 vs recorded 0.865`, is the existing time-ratio gate
failing on this machine/state; it is advisory and unchanged.)

## Document line each scenario printed

- idle: 13335 boxes over 833 rows, 600 turns with nothing touched
- hover-storm: 1000 rows built, 40 on screen, 240 crossings at 120 Hz
- scroll: 10000 rows, 142 translation frames and 111 recycle frames at 120 Hz
- cold-start: gallery s13, 2695 boxes, first painted frame
- kitchen-sink: gallery s13, 2695 boxes, a text field 22 tab stops in, 32 of each interaction
- thread-resize: 250 messages, 499 wrapped paragraphs, 9517 boxes; 32 width steps and a 120ms drag at 75 Hz
- app-shell: 12 tooltip triggers, 48 rows of library controls, a dock of 5000 virtualised lines in a vertical resizable group; 3 sweeps, 60 drag steps of 5 px, 40 window sizes

Note: the `rationale` strings in the source still say "1 851 boxes" for the gallery; the current s13 document
is 2 695 layout boxes (3 426 fragments).

## Document facts (details in `dump/`)

| document | layout boxes | fragments | DOM rows (el + text) | focusable stops | Tab presses to a text field | first-frame primitives (scene ops) | scene ops after one +8 px resize |
|---|---|---|---|---|---|---|---|
| gallery s13 (1600x1000 CSS px, scale 1) | 2 695 | 3 426 | 1 702 + 678 | 134 | 22 (ends on the 22nd stop, `field.zui-input`, preorder 302) | 1 177 emitted (5 184 culled) | 277 |
| app shell | 824 | 1 103 | 391 + 277 | 61 | never (no text field; the loop gives up after 400 presses) | 1 443 emitted (958 culled) | 695 |
| hover-storm table | 16 007 | 21 007 | 6 007 + 5 000 | 0 | n/a | 22 664 emitted (0 culled) | 43 |

- Swatch centres (s13, CSS px): (41,41) (85,41) (129,41) (173,41). In the shell the "34x34" finder matches the
  icon buttons (duplicates), not meaningful there.
- Shell: tip-0..11 centres at x = 25 + 38 i, y = 21; dock-handle centre (800, 692.5); panels at rest 650 px (page) and 350 px (dock).
- Root scroll extent: s13 root content 1585 x 8066 in a 1585 x 1000 port. Shell and table roots are not scroll
  containers (overflow visible); the table's content is 1600 x 24000 and is painted whole (22 664 primitives, nothing clipped).
- Scroll glide (crates/zgui-scroll `Tween`): fixed 240 ms, ease-out cubic `1-(1-t)^3`, new detents add to the
  destination and restart the ease from the current position. Wheel notch = 3 lines (headless default) x the container's
  computed line height: 351 px for `wheel(6.0)` over the 13 px bench list (19.5 px line), 216 px for `wheel(3.0)` over the 16 px gallery root (24 px line).
- scroll scenario at 120 Hz: a notch glides for 28 / 18 / 28 ticks (first three notches) with 6-7 zero-frame ticks inside each glide
  (animation frames are due on a refresh-interval phase; a tick that does not cross a due moment runs no frame) and 11 / 4 / 11 row-recycle ticks.
- kitchen glide at 60 Hz: every notch (alternating +-3.0) glides exactly 15 ticks of 16.667 ms, 15 frames, moving the page root by 216 px;
  about 840 primitives are re-emitted per glide frame.
- Shell drag: the two panel heights go 650/350 -> 500/500 in 5 px steps (30 steps) and back; one frame per step, 7 relaid-out nodes per step (586 on the first). Tooltip sweep: 7-8 frames per crossing (1-2 from settle + 6 idle turns), the tooltip content element is 116-123 x 28 at y = 48, x = 8 ... 383 following the trigger.

## Files

- `dump/gallery_s13.tsv`, `dump/shell.tsv`, `dump/table.tsv` (+ `.meta.txt`, `.tabs.tsv` tab order), `dump/glide_scroll.tsv`,
  `dump/glide_kitchen.tsv` (+ meta), `dump/shell_drag.tsv`, `dump/tooltip_sweep.tsv`, `dump/shell_motion.meta.txt`.
- `raw/<scenario>.run{1,2,3}.txt|.err`, `raw/label-churn.run*.txt`, `raw/resize-slope.run*.txt`, `raw/uptime.txt`,
  `raw/xtask-perf.log`, `raw/xtask-perf-run.tsv` (the run `cargo xtask perf` stored under docs/perf/runs).
- `zgui-bench-instr.patch`: `diff -ruN` of crates/zgui-bench (+ Cargo.lock) against ~/projects/_ref/zgui.
  Adds `src/instr.rs` (lib; the only unsafe, `deny(unsafe_code)` instead of `forbid` in lib.rs), `src/dump.rs` (the `dump` phase),
  `libc`/`zgui-css`/`zgui-dom`/`zgui-view-dom` deps, instruction records at each timed span, `INSTR` printing in `scenario::print`,
  visibility changes (`pub(crate)`) in scenario/shell.rs and scenario/mod.rs.

## Commands

```
export RUSTC_WRAPPER= PATH=~/.local/share/cargo/bin:$PATH RUSTUP_TOOLCHAIN=1.99.1
cd scratchpad/zgui-input            # cp -c -R of scratchpad/zgui-copy
cargo build --release --offline -p zgui-bench --bins
# per run (1..3), each in its own process, `uptime` logged before the batch:
cargo run --release --offline -q -p zgui-bench --bin zgui-bench -- scenario <idle|hover-storm|scroll|cold-start|kitchen-sink|thread-resize|app-shell>
cargo run --release --offline -q -p zgui-bench --bin label-churn
cargo run --release --offline -q -p zgui-bench --bin resize-slope
cargo xtask perf                    # once, after the batches (exits 1: 8 measurements outside the repo's recorded bands, expected on this machine)
# dumps (one process each):
ZGUI_DUMP_DIR=bench/results/input/zgui/dump ./target/release/zgui-bench dump <s13|shell|table|glide_scroll|glide_kitchen|shell_motion>
```

## What the harness does not measure (surprises)

- `crate::draw::renderer` returns a `NullRenderer` unless `ZGUI_BENCH_GPU` or `ZGUI_BENCH_CAPTURE` is set. Its `draw()` only records the
  display list; there is no GPU work, no rasterisation or composition of pixels, and no texture upload (the sink accepts and drops). It also
  answers `shifts_composed_pixels() = true` and `composed_survives_configure() = true`, i.e. it claims the cheap-scroll/resize paths a real renderer
  would take, so scroll translation frames and resize frames narrow their damage as if pixels were shifted by the GPU. All numbers are CPU
  framework work: cascade, layout, text shaping, emission/culling/draw-order tree, hit index. Glyph rasterisation into the atlas (CPU) is counted.
- Time distributions can be dominated by the load (p95/max of hover.crossing 26-78 ms in run 1); scenario baselines in the source (e.g.
  kitchen.untouched_turn 22 us, kitchen.click 10.8 us, kitchen.glide_tick 610 us) are 2-5x below what this machine produced even in the quiet `xtask perf`
  run (96 us, 33 us, 1 548 us), consistently across all runs, so those are not load artefacts; the s13 gallery has also grown since the baselines were written.
- Process-wide instructions include helper threads. thread-resize and the s13 resize use the layout/style pools, so their instruction counts are the
  total CPU work across threads, not the wall-critical path.
- cold-start is a single sample per process by design; its instruction count (1.10-1.15 G) is stable to about 2 %, its time is not.
- The Tab traversal in the dump uses the same `focus_a_field_timed` as the kitchen scenario: 22 presses on s13; the shell and table have no text field.
- The hover rules cannot be listed per node by any public API; see the meta files for the document-level facts.
