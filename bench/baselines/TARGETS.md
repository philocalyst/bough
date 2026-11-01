# Targets: zgx baselines to beat

zgx 06debe9d. Full raw data lands in `linux/` (ilo-prod) and `macos/` (this Mac) with each run's SUMMARY.md.

## Linux, CPU-only headless (RENDER=0), Ryzen 7 3700X, rustc 1.97.1, `perf stat -e instructions:u`

Per-frame = slope between 300- and 600-tick runs (animate: 600/1200 frames), which cancels startup.
RENDER=0 measures text as a renderer would (shaping included) and draws nothing.

| scene | instructions/frame | cycles/frame |
|---|---:|---:|
| heavy idle | 523 | 112 |
| heavy stream | 3,175,664 | 1,537,978 |
| heavy scroll | 147,093 | 79,161 |
| heavy both | 3,346,541 | 1,603,384 |
| lab (workload) idle | 716 | 453 |
| lab stream | 482,127 | 289,468 |
| lab scroll | 54,909 | 28,845 |
| lab both | 534,115 | 325,803 |
| animate (update + flush) | 236,224 | 105,444 |

micro_bench, RENDER=0, medians of process CPU time per operation:

| scene | write | frame |
|---|---:|---:|
| state (one counter text in a 40×25 text grid) | 3,139 ns | 5,518 ns |
| vscroll-fixed (10k rows, 12 pt step) | 2,332 ns | 8,654 ns |
| vscroll-variable | 12,000 ns | 13,760 ns (p10 1,684, p90 25,526) |

zgx's GPU headless scenes could not run on ilo-prod (no GL/Vulkan ICD); they run on the Mac.

## macOS (zgx's own published figures, M-series, `dist`, system allocator)

From zgx README (2026-10-25, `282bfc0`) and `docs/performance/floor-plan.md`:

| heavy mode | CPU % of a core | GPU Mcycles/s | memory MB |
|---|---:|---:|---:|
| idle | 0.00 | 0 | 59 |
| scroll | 3.2 | 11.5 | 255 |
| stream | 8.5 | 16.8 | 259 |
| both | 9.1 | 16.7 | 259 |

Frame instructions (macOS, incl. render): heavy both 9.28 M, stream 8.30 M, scroll 0.67 M; one colour write
121 K (zgx's own ~24 K + Metal). zgx's hand-written floor for heavy stream: 2.72 M (text 1.42 M, submit 0.99 M).
p99 frame interval 17.6 ms.

## macOS, measured here (M3 Pro, zgx 06debe9d `dist`; details in `macos/SUMMARY.md`)

micro_bench medians (CPU ns per op): state write 1,020 / frame 2,200 (RENDER=0), frame **141,600** with the
renderer; vscroll-fixed frame 4,000 / **201,000**; vscroll-variable frame 7,100 / **213,800**.
GPUI Fast frames (no renderer): state 957,800, vscroll-fixed 37,600, vscroll-variable 162,400.

Heavy per-phase (K instructions per frame): stream update 1,458 · layout 750 · damage 1,227 · paint 1,150 ·
encode 634 · submit 16 (≈ 5.24 M); scroll paint 452 + encode 108 dominate (≈ 0.73 M); idle ≈ 125 K.

## Linux, zgx `scripts/perf/suite.py` non-GPU scenes (least of 3, instructions per whole scene)

| scene | instructions | spread |
|---|---:|---:|
| mount | 296.8 M | 0.05% |
| script-update | 3,843.8 M | 0.01% |
| anime | 3,733.5 M | 0.00% |
| script-layout | 2,974.1 M | 0.01% |
| list-state | 830.8 M | 0.02% |
| style | 1,332.1 M | 0.00% |
| widgets | 146.8 M | 0.00% |

## Linux, zgui `cargo xtask perf` (951a0ea, timed headless scenarios; its "REGRESSED" verdicts only mean this
## Ryzen is slower than the machine its bands were set on)

| measurement | time |
|---|---:|
| hover.crossing | 274.09 µs |
| scroll.translation | 98.78 µs |
| kitchen.click | 68.87 µs |
| kitchen.keystroke | 765.03 µs |
| kitchen.untouched_turn | 130.98 µs |
| kitchen.glide_tick | 1,451.89 µs |
| kitchen.tab_traversal | 774.06 µs |
| shell.tooltip_crossing | 2,234.34 µs |
| shell.drag_step | 517.15 µs |
