# zortax gui-bench, quick matrix, run 1 (ilo-prod, 2026-11-12)

Status: **finished, not a pass.** 172 cells; 154 completed all their measurements, 18 stopped after `startup`
because the layout check failed. By the harness's own validity checks 280 of 535 reports are invalid
(`wake_lateness` fail: the host was busy, other processes used about 70 % of 16 CPUs). Values marked † below
come from an invalid report. Treat everything here as a rough baseline, not a result to quote.

Identity: see `IDENTITY.txt` (gui-bench 0fd45107, fakeland 71695e7c, Ryzen 7 3700X, governor powersave,
pixman compositor, lavapipe software Vulkan, `--quick`, nice 19, load average about 20-30). bough is not in this
matrix (it has no Linux backend), so none of these numbers compare with bough's macOS numbers directly.

## Failures
- floem, 16 of 17 cells: the 1 px row border reads `#f2f3f5`, the spec says `#e5e7eb`. `#f2f3f5` is the border
  at half coverage over white, so floem draws the line across a pixel boundary. Deterministic, not load.
- vizia, typing and search: the frame is black at the layout check (nothing painted). Cause not established.
- dioxus (wgpu edition) spinner: the log shows "no frames after the first: timed out", but the cells record
  no error and finished their measurements. dioxus-skia is fine.

## Headline medians (rows scenario at 3000 rows unless named)
| framework | hover p50 ms | hover in→commit ms | idle RSS MiB | idle cpu ms/s | scroll cpu ms/frame | typing p50 ms | resize cfg→present ms | spinner10k cpu ms/frame |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| zgui | 16.16 † | 7.73 † | 317.16 | 144.42 | 32.15 † | 22.70 † | 73.47 † | 20.12 † |
| gpui-kit | 111.06 † | 98.47 † | 251.64 | 154.07 | 194.04 † | 80.05 † | 120.02 † | 473.70 † |
| gpui-kit-fast | 61.96 † | 49.48 † | 269.82 | 174.91 | 207.83 † | 78.39 † | 134.81 † | 323.81 † |
| freya | 15.11 † | 5.94 † | 250.74 | 14.89 | 32.25 † | 21.15 † | 59.97 † | 26.36 |
| freya-056a5b4 | 15.11 † | 5.39 † | 250.55 | 14.75 | 29.06 † | 24.31 † | 72.96 † | 26.61 † |
| vizia | 113.16 † | 104.15 † | 273.66 | 30.11 | 168.21 | – | 91.66 † | 28.24 † |
| floem | – | – | – | – | – | – | – | – |
| iced | 15.10 | 5.49 | 169.94 | 22.38 | 25.88 † | 23.15 † | 14.69 | 145.56 † |
| egui | 29.04 † | 17.03 † | 169.77 | 31.82 | 45.51 † | 21.96 † | 27.35 † | 99.26 † |
| dioxus | 28.19 † | 15.11 † | 220.82 | 74.94 | 101.31 † | 37.02 † | 65.01 † | 115.50 † |
| dioxus-skia | 15.12 | 6.50 | 192.20 | 14.29 | 27.76 | 19.95 † | 26.80 † | 44.77 † |

Files: `cells.tsv` (per cell: done, errors), `metrics.tsv` (every metric), `validity.tsv` (every check).
Raw results stay on ilo-prod at `/root/work/zortax/results/quick-1`; flattened by `/root/work/zortax/tools/flatten.py`.
