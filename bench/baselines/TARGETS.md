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
