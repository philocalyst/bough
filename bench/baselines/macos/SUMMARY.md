# macOS baselines — zgx 06debe9d, zgui 951a0ea (2026-11-01)

Apple M3 Pro, 11 CPUs, 36 GiB, macOS 15.7.4 (24G517). rustc 1.99.1. Font: zgx `comparisons/fonts/DejaVuSans.ttf`
(`ZGX_FONT_FILE`). Builds `--profile dist --locked` (the suite builds `--release`). Binary hashes:
`logs/binaries_sha256.txt`. Raw data: `raw/`, `logs/`, `micro_summary.txt`, `zgui/`, scripts in `scripts/`.
Collected by a benchmark agent; no windows opened; no source in `_ref/` modified. Transcribed from its report.

## micro_bench (CPU ns per op, median of 3 repeats; [min–max])

zgx: `SCENE=<s> RENDER=<0|1> target/dist/examples/micro_bench`. GPUI Fast: `gpui-fast-micro` (no renderer, ≈ RENDER=0).

| framework | scene | RENDER | write | frame |
|---|---|---|---:|---:|
| zgx | state | 0 | 1,020 [980–1,040] | 2,200 [2,200–2,300] |
| zgx | state | 1 | 1,580 [1,540–1,800] | 141,600 [133,600–168,100] |
| GPUI Fast | state | – | 40 | 957,800 |
| zgx | vscroll-fixed | 0 | 1,200 | 4,000 [3,800–4,000] |
| zgx | vscroll-fixed | 1 | 2,100 | 201,000 [187,300–235,400] |
| GPUI Fast | vscroll-fixed | – | 80 | 37,600 |
| zgx | vscroll-variable | 0 | 7,140 | 7,100 (p10 900, p90 13,000) |
| zgx | vscroll-variable | 1 | 8,520 | 213,800 [206,600–222,200] |
| GPUI Fast | vscroll-variable | – | 160 | 162,400 |

## zgx headless instruction suite (`python3 scripts/perf/suite.py --all`, millions of instructions per scene)

| scene | M instr | spread | | scene | M instr | spread |
|---|---:|---:|---|---|---:|---:|
| heavy-both | 4,411.9 | 0.79% | | script-update | 3,922.8 | 0.62% |
| heavy-stream | 3,945.6 | 0.68% | | anime | 3,024.1 | 0.68% |
| heavy-scroll | 1,065.0 | 0.90% | | script-layout | 2,908.2 | 0.83% |
| lab-stream | 1,140.5 | 1.33% | | list-state | 843.2 | 1.95% |
| lab-scroll | 787.6 | 0.61% | | layout | 5,180.3 | 0.39% |
| animate | 1,133.7 | 2.66% | | frame | 530.9 | 2.99% |
| micro-state | 6,546.6 | 1.25% | | text | 4,488.6 | 0.87% |
| micro-vscroll-fixed | 6,526.6 | 0.49% | | style | 1,246.8 | 0.32% |
| micro-vscroll-variable | 6,896.0 | 1.16% | | widgets | 182.9 | 3.01% |
| mount | 386.2 | 2.88% | | app-mail | 1,035.9 | 3.61% |
| state (slow) | 65,870.2 | 0.12% | | app-dashboard | 3,441.0 | 0.49% |
| core (slow) | 87,602.5 | 0.12% | | app-sketch / app-code | 678.3 / 2,143.7 | 1.54% / 0.44% |

Wall times overlapped a zgui compile; instruction counts are unaffected.

## zgx per-phase profile (`ZGX_PROFILE=1 heavy_headless`, dist, TICKS=4000; medians of 36 lines; K instr / µs per frame)

| mode | update | layout | damage | paint | encode | submit |
|---|---|---|---|---|---|---|
| idle | 53.9 / 3.9 | 62.1 / 6.5 | 0.6 / 0.1 | 0.5 / 0.2 | 7.9 / 1.4 | 0.2 / 0.1 |
| stream | 1,457.6 / 229.8 | 750.3 / 69.1 | 1,226.8 / 178.6 | 1,149.6 / 682.0 | 633.7 / 82.1 | 15.8 / 11.1 |
| scroll | 79.7 / 30.9 | 67.3 / 22.8 | 6.5 / 1.6 | 452.4 / 735.8 | 108.2 / 35.7 | 15.7 / 9.9 |
| both | 1,576.6 / 590.2 | 817.3 / 158.5 | 1,283.4 / 340.5 | 1,663.5 / 1,849.2 | 616.2 / 136.1 | 16.5 / 35.6 |

Heavy stream ≈ 5.24 M instructions/frame across these phases (excluding presentation/driver work outside them).

## zgui

`cargo xtask perf` (run in a copy, stable 1.99.1 since its pinned nightly-2026-04-16 is absent): 7 scenarios,
47 measurements, gate exit 1 with 5 out of band (bands are the author's machine): cold.first_frame 190.33 ms,
kitchen.click 31.08 µs, kitchen.untouched_turn 95.58 µs, kitchen.glide_tick 941.46 µs, shell.drag_hit_rebuilds 1.
Full table: `zgui/docs/perf/runs/1793520287.tsv`. Probes: label-churn 1,348.3 µs/frame (30 labels);
resize-slope ratio 1.1145 vs recorded 0.8650 (exit 1); five probes capped at 60 s. Criterion `paged_vec`: ~1 ns.

## Not run

The windowed suite (CPU %, GPU, memory, pacing). Commands, for a quiet desktop with Screen Recording permission:

```sh
cd ~/projects/_ref/zgx
python3 scripts/perf/preflight.py --state && python3 scripts/perf/preflight.py
O=~/projects/bough/bench/windowed/zgx-vs-gpui-fast-$(date -u +%F)
for s in heavy lab animate vlist; do
  python3 scripts/run_macos_comparison.py --suite --scene $s --frameworks zgx,gpui-fast --hide Zeron --out "$O/$s"
done
```
