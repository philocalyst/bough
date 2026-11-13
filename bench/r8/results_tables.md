# VIEW Unthrottled (cmp-u1, heavy-u1; timing unthrottled)

## Where bough wins and loses, headline cells

Lowest valid value in 35 metrics, behind another framework in 3.

Behind (valid measurements only):

- spinner (no rows) fps (fps): bough 62.5, best valid egui 1,248
- spinner 10,000 fps (fps): bough 62.5, best valid freya 153.1
- ticker fine 100 updates fps (fps): bough 62.5, best valid iced 62.5

## Where bough wins and loses, heavy scenes (four measured frameworks)

Lowest valid value in 14 metrics, behind another framework in 11.

Behind (valid measurements only):

- sheet 10,000 rows hover total p50 (ms): bough 19.40, best valid egui 9.95
- sheet 10,000 rows hover input to commit p50 (ms): bough 19.41, best valid egui 9.97
- sheet 10,000 rows theme switch total p50 (ms): bough 27.91, best valid egui 11.19
- sheet 10,000 rows edit total p50 (ms): bough 15.70, best valid egui 9.55
- sheet 10,000 rows idle RSS (MiB): bough 331.3, best valid egui 149.0
- canvas 1,000 nodes hover total p50 (ms): bough 83.29, best valid iced 17.56
- canvas 1,000 nodes hover input to commit p50 (ms): bough 83.30, best valid iced 17.57
- canvas 1,000 nodes theme switch total p50 (ms): bough 285.6, best valid iced 19.92
- canvas 1,000 nodes resize configure to present p50 (ms): bough 87.7, best valid egui 18.3
- canvas 5,000 nodes theme switch total p50 (ms): bough 1,151, best valid egui 71.46
- canvas 5,000 nodes resize configure to present p50 (ms): bough 385.5, best valid egui 70.2

## Headline table (the cmp-1 columns)

| framework | rows 3,000 hover total p50 (ms) | rows 3,000 hover input to commit p50 (ms) | rows 3,000 startup to first present (ms) | rows 3,000 idle RSS (MiB) | rows 3,000 idle CPU (ms/s) | rows 3,000 scroll CPU per frame (ms) | typing p50 (ms) | resize configure to present p50 (ms) | spinner 10,000 CPU per frame (ms) | table fine remove total p50 (ms) | table fine remove CPU per frame (ms) |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| zgui | 7.25 | 7.27 | - | 316.9 | 115.0 | 17.61 † | 4.06 | 34.64 | 12.07 | 32.74 | 181.0 |
| gpui-kit | 117.1 | 117.1 | - | 252.0 | 145.2 | 144.5 | 46.56 | 84.22 | 349.4 | 51.98 | 141.5 |
| gpui-kit-fast | 55.84 | 55.86 | - | 269.6 | 114.8 | 167.8 | 38.29 | 70.09 | 241.6 | 44.12 | 179.1 |
| freya | 6.17 | 6.18 | - | 250.2 | 11.46 | 25.90 | 7.55 | 46.21 | 17.34 | 8.14 | 19.27 |
| freya-056a5b4 | 5.43 | 5.45 | - | 251.1 | 10.88 | 18.25 | 7.60 | 46.85 | 17.53 | 7.75 | 18.54 |
| vizia | 96.72 | 96.73 | - | 273.6 | 21.55 | 167.6 | *~33.00* (failed cell) | 57.59 | 20.61 | 430.8 | 324.0 |
| floem | *~2.00* (failed cell) | *~2.00* (failed cell) | *~71.0* | *~190.0* (failed cell) | *~0.87* (failed cell) | *~4.00* (failed cell) | *~2.30* (failed cell) | *~5.30* (failed cell) | *~4.60* (failed cell) | *~3.40* (failed cell) | *~3.80* (failed cell) |
| iced | 5.35 | 5.36 | - | 168.5 | 11.55 | 15.69 | 6.37 | 8.20 | 78.85 | 11.47 | 24.68 |
| egui | 19.13 | 19.15 | - | 168.9 | 24.18 | 35.51 | 10.50 | 11.77 | 70.66 | 9.65 | 25.01 |
| dioxus | 16.89 | 16.90 | - | 219.5 | 60.59 | 69.64 | 20.42 | 45.07 | 77.15 | 25.06 | 71.32 |
| dioxus-skia | 7.29 | 7.30 | - | 191.3 | 11.68 | 20.14 | 5.63 | 13.16 | 28.74 | 9.11 | 18.06 |
| bough | 0.99 | 1.00 | - | 34.8 | 0.03 | 1.73 | 0.25 | 5.80 | 1.15 | 3.73 | 3.85 |

## Startup, memory, idle

| framework | rows 3,000 startup to first present (ms) | virtual 100,000 startup to first present (ms) | spinner (no rows) startup to first present (ms) | rows 3,000 idle RSS (MiB) | virtual 100,000 idle RSS (MiB) | spinner (no rows) RSS (MiB) | rows 3,000 idle CPU (ms/s) | spinner (no rows) idle CPU (ms/s) |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| zgui | - | - | - | 316.9 | 176.8 | 178.3 | 115.0 | 551.9 |
| gpui-kit | - | - | - | 252.0 | 178.9 | 190.7 | 145.2 | 5,674 |
| gpui-kit-fast | - | - | - | 269.6 | 199.6 | 206.6 | 114.8 | 6,002 |
| freya | - | - | - | 250.2 | 140.3 | 139.9 | 11.46 | 541.5 |
| freya-056a5b4 | - | - | - | 251.1 | 140.3 | 139.8 | 10.88 | 547.2 |
| vizia | - | - | - | 273.6 | 145.6 | 151.3 | 21.55 | 464.7 |
| floem | *~71.0* | *~35.0* | - | *~190.0* (failed cell) | *~120.0* (failed cell) | 127.1 | *~0.87* (failed cell) | 98.22 |
| iced | - | - | - | 168.5 | 142.8 | 167.2 | 11.55 | 2,042 |
| egui | - | - | - | 168.9 | 143.1 | 142.6 | 24.18 | 603.1 |
| dioxus | - | - | - | 219.5 | 176.2 | 178.8 | 60.59 | 2,804 |
| dioxus-skia | - | - | - | 191.3 | 147.2 | 159.1 | 11.68 | 326.9 |
| bough | - | - | - | 34.8 | 18.8 | 19.4 | 0.03 | 5.37 |

## Hover (rows)

| framework | rows 3,000 hover total p50 (ms) | rows 3,000 hover input to commit p50 (ms) | rows 100 hover total p50 (ms) | rows 100 hover input to commit p50 (ms) | rows 3,000 hover CPU per frame (ms) | rows 100 hover CPU per frame (ms) |
|---|---:|---:|---:|---:|---:|---:|
| zgui | 7.25 | 7.27 | 5.34 | 5.35 | 26.50 | 24.69 |
| gpui-kit | 117.1 | 117.1 | 22.96 | 22.97 | 229.2 | 100.3 |
| gpui-kit-fast | 55.84 | 55.86 | 25.61 | 25.63 | 169.7 | 153.8 |
| freya | 6.17 | 6.18 | 4.51 | 4.52 | 18.51 | 17.42 |
| freya-056a5b4 | 5.43 | 5.45 | 5.07 | 5.09 | 16.30 | 16.79 |
| vizia | 96.72 | 96.73 | 5.60 | 5.61 | 119.7 | 13.23 |
| floem | *~2.00* (failed cell) | *~2.00* (failed cell) | *~1.40* (failed cell) | *~1.40* (failed cell) | *~3.50* (failed cell) | *~3.00* (failed cell) |
| iced | 5.35 | 5.36 | 4.01 | 4.02 | 16.52 | 20.91 |
| egui | 19.13 | 19.15 | 4.61 | 4.62 | 34.82 | 20.26 |
| dioxus | 16.89 | 16.90 | 14.38 | 14.39 | 67.05 | 61.11 |
| dioxus-skia | 7.29 | 7.30 | 3.72 | 3.73 | 15.67 | 13.27 |
| bough | 0.99 | 1.00 | 0.13 | 0.14 | 0.99 | 0.12 |

## Scrolling

| framework | rows 3,000 scroll CPU per frame (ms) | rows 3,000 scroll fps (fps) | virtual 100,000 scroll CPU per frame (ms) |
|---|---:|---:|---:|
| zgui | 17.61 † | 144.8 † | 16.97 |
| gpui-kit | 144.5 | 8.9 | 83.45 |
| gpui-kit-fast | 167.8 | 15.6 | 118.9 |
| freya | 25.90 | 78.1 | 15.28 |
| freya-056a5b4 | 18.25 | 127.2 | 15.81 |
| vizia | 167.6 | 5.8 | 22.16 |
| floem | *~4.00* (failed cell) | *~13.0* (failed cell) | *~3.00* (failed cell) |
| iced | 15.69 | 199.0 | 19.63 † |
| egui | 35.51 | 49.8 | 21.49 |
| dioxus | 69.64 | 52.6 | 66.10 |
| dioxus-skia | 20.14 | 99.0 | 17.04 |
| bough | 1.73 | 240.0 | 0.90 |

## Typing, search, resize

| framework | typing p50 (ms) | typing handling p50 (app share) (ms) | typing CPU per frame (ms) | search filter total p50 (ms) | search filter input to commit p50 (ms) | search filter CPU per frame (ms) | resize configure to present p50 (ms) | resize configure to commit p50 (app share) (ms) | resize CPU per frame (ms) |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| zgui | 4.06 | 4.07 | 13.42 | 71.60 | 71.61 | 231.1 | 34.64 | 34.65 | 52.49 |
| gpui-kit | 46.56 | 46.57 | 130.4 | 36.32 | 36.33 | 111.2 | 84.22 | 84.23 | 142.6 |
| gpui-kit-fast | 38.29 | 38.30 | 172.9 | 40.13 | 40.14 | 152.9 | 70.09 | 70.10 | 159.5 |
| freya | 7.55 | 7.56 | 18.72 | 39.27 | 39.29 | 47.29 | 46.21 | 46.22 | 37.08 |
| freya-056a5b4 | 7.60 | 7.61 | 19.88 | 37.51 | 37.53 | 57.02 | 46.85 | 46.86 | 37.48 |
| vizia | *~33.00* (failed cell) | *~33.00* (failed cell) | *~34.00* (failed cell) | 119.7 | 119.7 | 11.05 | 57.59 | 57.60 | 84.14 |
| floem | *~2.30* (failed cell) | *~2.30* (failed cell) | *~3.60* (failed cell) | *~8.50* (failed cell) | *~8.50* (failed cell) | *~3.50* (failed cell) | *~5.30* (failed cell) | *~5.30* (failed cell) | *~4.00* (failed cell) |
| iced | 6.37 | 6.38 | 17.03 | 9.86 | 9.87 | 15.68 | 8.20 | 8.21 | 23.72 |
| egui | 10.50 | 10.51 | 24.86 | 12.93 | 12.94 | 11.49 | 11.77 | 11.78 | 27.29 |
| dioxus | 20.42 | 20.43 | 72.49 | 48.65 | 48.66 | 66.91 | 45.07 | 45.09 | 62.45 |
| dioxus-skia | 5.63 | 5.64 | 18.45 | 23.93 | 23.94 | 18.55 | 13.16 | 13.17 | 20.24 |
| bough | 0.25 | 0.26 | 0.26 | 6.99 | 7.00 | 5.74 | 5.80 | 5.81 | 6.01 |

## Table operations (1,000 rows, per-row state; egui has its one variant)

| framework | table fine remove total p50 (ms) | table fine remove input to commit p50 (ms) | table fine remove CPU per frame (ms) | table fine update10 total p50 (ms) | table fine update10 input to commit p50 (ms) | table fine update10 CPU per frame (ms) | table fine swap total p50 (ms) | table fine swap input to commit p50 (ms) | table fine swap CPU per frame (ms) |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| zgui | 32.74 | 32.75 | 181.0 | 11.55 | 11.56 | 42.51 | 30.72 | 30.73 | 175.7 |
| gpui-kit | 51.98 | 51.99 | 141.5 | 34.05 | 34.07 | 119.1 | 30.16 | 30.17 | 130.7 |
| gpui-kit-fast | 44.12 | 44.14 | 179.1 | 41.13 | 41.15 | 180.8 | 36.82 | 36.84 | 192.6 |
| freya | 8.14 | 8.15 | 19.27 | 7.99 | 8.00 | 17.27 | 8.13 | 8.14 | 19.26 |
| freya-056a5b4 | 7.75 | 7.77 | 18.54 | 8.38 | 8.40 | 18.69 | 9.17 | 9.18 | 17.01 |
| vizia | 430.8 | 430.8 | 324.0 | 31.63 | 31.64 | 46.56 | 279.7 | 279.8 | 405.8 |
| floem | *~3.40* (failed cell) | *~3.40* (failed cell) | *~3.80* (failed cell) | *~2.50* (failed cell) | *~2.50* (failed cell) | *~3.70* (failed cell) | *~2.70* (failed cell) | *~2.80* (failed cell) | *~3.70* (failed cell) |
| iced | 11.47 | 11.48 | 24.68 | 7.16 | 7.18 | 18.44 | 5.95 | 5.96 | 19.91 |
| egui | 9.65 | 9.66 | 25.01 | 11.51 | 11.52 | 25.94 | 9.39 | 9.40 | 24.77 |
| dioxus | 25.06 | 25.07 | 71.32 | 21.71 | 21.72 | 78.74 | 23.03 | 23.04 | 76.61 |
| dioxus-skia | 9.11 | 9.12 | 18.06 | 7.40 | 7.41 | 17.89 | 9.72 | 9.74 | 21.75 |
| bough | 3.73 | 3.74 | 3.85 | 0.37 | 0.38 | 0.38 | 0.52 | 0.53 | 0.53 |

## Ticker (100 row updates per tick)

| framework | ticker fine 100 updates CPU per frame (ms) | ticker fine 100 updates fps (fps) |
|---|---:|---:|
| zgui | 23.95 | 62.0 |
| gpui-kit | 116.1 | 31.5 |
| gpui-kit-fast | 160.9 | 27.7 |
| freya | 19.24 | 62.2 |
| freya-056a5b4 | 19.04 | 62.2 |
| vizia | 42.75 | 35.2 |
| floem | *~3.50* (failed cell) | *~17.0* (failed cell) |
| iced | 21.87 | 62.5 |
| egui | 25.18 | 62.5 |
| dioxus | 67.31 | 55.6 |
| dioxus-skia | 17.54 | 58.0 |
| bough | 0.11 | 62.5 |

## Spinner

| framework | spinner (no rows) CPU per frame (ms) | spinner 10,000 CPU per frame (ms) | spinner (no rows) fps (fps) | spinner 10,000 fps (fps) |
|---|---:|---:|---:|---:|
| zgui | 12.02 | 12.07 | 60.0 | 60.0 |
| gpui-kit | 59.98 | 349.4 | 102.9 | 3.7 |
| gpui-kit-fast | 92.65 | 241.6 | 72.7 | 8.8 |
| freya | 3.17 | 17.34 | 894.5 | 153.1 |
| freya-056a5b4 | 3.14 | 17.53 | 855.1 | 150.7 |
| vizia | 7.15 | 20.61 | 62.5 | 62.5 |
| floem | 1.34 | *~4.60* (failed cell) | 59.9 | *~13.0* (failed cell) |
| iced | 39.65 | 78.85 | 139.4 | 22.9 |
| egui | 3.02 | 70.66 | 1,248 | 17.9 |
| dioxus | 53.62 | 77.15 | 85.6 | 33.3 |
| dioxus-skia | 6.83 | 28.74 | 380.0 | 59.0 |
| bough | 0.09 | 1.15 | 62.5 | 62.5 |

## Estimates, with method per value

| column | framework | estimate | range | basis |
|---|---|---:|---|---|
| rows 3,000 hover total p50 (ms) | floem | ~2.00 | 0.07 to 58.00 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 11 frameworks |
| rows 3,000 hover input to commit p50 (ms) | floem | ~2.00 | 0.07 to 58.00 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 11 frameworks |
| rows 100 hover total p50 (ms) | floem | ~1.40 | 0.05 to 41.00 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 11 frameworks |
| rows 3,000 idle RSS (MiB) | floem | ~190.0 | 140.0 to 260.0 | own: 2 own valid cells in class 'resident memory (MiB)' (log-level -0.20 against the column medians); column level from 11 frameworks |
| rows 3,000 idle CPU (ms/s) | floem | ~0.87 | 0.24 to 3.10 | own: 2 own valid cells in class 'idle CPU (ms/s)' (log-level -3.34 against the column medians); column level from 11 frameworks |
| rows 3,000 scroll CPU per frame (ms) | floem | ~4.00 | 3.00 to 5.40 | own: 2 own valid cells in class 'cpu per frame (ms)' (log-level -1.91 against the column medians); column level from 10 frameworks |
| rows 3,000 scroll fps (fps) | floem | ~13.0 | 9.7 to 18.0 | own: 2 own valid cells in class 'frame rate (fps)' (log-level -1.33 against the column medians); column level from 10 frameworks |
| virtual 100,000 scroll CPU per frame (ms) | floem | ~3.00 | 2.30 to 4.10 | own: 2 own valid cells in class 'cpu per frame (ms)' (log-level -1.91 against the column medians); column level from 10 frameworks |
| virtual 100,000 idle RSS (MiB) | floem | ~120.0 | 89.0 to 160.0 | own: 2 own valid cells in class 'resident memory (MiB)' (log-level -0.20 against the column medians); column level from 11 frameworks |
| typing p50 (ms) | floem | ~2.30 | 0.08 to 67.00 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 10 frameworks |
| typing p50 (ms) | vizia | ~33.00 | 0.83 to 1,300 | own: 14 own valid cells in class 'latency (ms)' (log-level +1.32 against the column medians); column level from 10 frameworks |
| search filter total p50 (ms) | floem | ~8.50 | 0.30 to 250.0 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 11 frameworks |
| resize configure to present p50 (ms) | floem | ~5.30 | 0.18 to 150.0 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 11 frameworks |
| spinner 10,000 CPU per frame (ms) | floem | ~4.60 | 3.40 to 6.20 | own: 2 own valid cells in class 'cpu per frame (ms)' (log-level -1.91 against the column medians); column level from 11 frameworks |
| spinner 10,000 fps (fps) | floem | ~13.0 | 9.3 to 17.0 | own: 2 own valid cells in class 'frame rate (fps)' (log-level -1.33 against the column medians); column level from 11 frameworks |
| table fine remove total p50 (ms) | floem | ~3.40 | 0.12 to 98.00 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 11 frameworks |
| table fine remove CPU per frame (ms) | floem | ~3.80 | 2.80 to 5.20 | own: 2 own valid cells in class 'cpu per frame (ms)' (log-level -1.91 against the column medians); column level from 11 frameworks |
| table fine update10 total p50 (ms) | floem | ~2.50 | 0.09 to 72.00 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 11 frameworks |
| table fine swap total p50 (ms) | floem | ~2.70 | 0.10 to 79.00 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 11 frameworks |
| ticker fine 100 updates CPU per frame (ms) | floem | ~3.50 | 2.60 to 4.70 | own: 2 own valid cells in class 'cpu per frame (ms)' (log-level -1.91 against the column medians); column level from 11 frameworks |
| ticker fine 100 updates fps (fps) | floem | ~17.0 | 12.0 to 23.0 | own: 2 own valid cells in class 'frame rate (fps)' (log-level -1.33 against the column medians); column level from 11 frameworks |
| rows 100 hover input to commit p50 (ms) | floem | ~1.40 | 0.05 to 41.00 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 11 frameworks |
| table fine remove input to commit p50 (ms) | floem | ~3.40 | 0.12 to 98.00 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 11 frameworks |
| table fine update10 input to commit p50 (ms) | floem | ~2.50 | 0.09 to 73.00 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 11 frameworks |
| table fine swap input to commit p50 (ms) | floem | ~2.80 | 0.10 to 79.00 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 11 frameworks |
| search filter input to commit p50 (ms) | floem | ~8.50 | 0.30 to 250.0 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 11 frameworks |
| typing handling p50 (app share) (ms) | floem | ~2.30 | 0.08 to 67.00 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 10 frameworks |
| typing handling p50 (app share) (ms) | vizia | ~33.00 | 0.83 to 1,300 | own: 14 own valid cells in class 'latency (ms)' (log-level +1.32 against the column medians); column level from 10 frameworks |
| resize configure to commit p50 (app share) (ms) | floem | ~5.30 | 0.18 to 150.0 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 11 frameworks |
| rows 3,000 hover CPU per frame (ms) | floem | ~3.50 | 2.60 to 4.70 | own: 2 own valid cells in class 'cpu per frame (ms)' (log-level -1.91 against the column medians); column level from 11 frameworks |
| rows 100 hover CPU per frame (ms) | floem | ~3.00 | 2.20 to 4.00 | own: 2 own valid cells in class 'cpu per frame (ms)' (log-level -1.91 against the column medians); column level from 11 frameworks |
| table fine update10 CPU per frame (ms) | floem | ~3.70 | 2.80 to 5.10 | own: 2 own valid cells in class 'cpu per frame (ms)' (log-level -1.91 against the column medians); column level from 11 frameworks |
| table fine swap CPU per frame (ms) | floem | ~3.70 | 2.80 to 5.00 | own: 2 own valid cells in class 'cpu per frame (ms)' (log-level -1.91 against the column medians); column level from 11 frameworks |
| typing CPU per frame (ms) | floem | ~3.60 | 2.70 to 4.90 | own: 2 own valid cells in class 'cpu per frame (ms)' (log-level -1.91 against the column medians); column level from 10 frameworks |
| typing CPU per frame (ms) | vizia | ~34.00 | 3.80 to 310.0 | own: 14 own valid cells in class 'cpu per frame (ms)' (log-level +0.34 against the column medians); column level from 10 frameworks |
| search filter CPU per frame (ms) | floem | ~3.50 | 2.60 to 4.70 | own: 2 own valid cells in class 'cpu per frame (ms)' (log-level -1.91 against the column medians); column level from 11 frameworks |
| resize CPU per frame (ms) | floem | ~4.00 | 2.90 to 5.40 | own: 2 own valid cells in class 'cpu per frame (ms)' (log-level -1.91 against the column medians); column level from 11 frameworks |
| rows 3,000 startup to first present (ms) | floem | ~71.0 | 2.5 to 2,100 | cross-class: only 1 own valid cell(s) in class 'startup (ms)': level taken across all classes (cross-class, low confidence); column level from 11 frameworks |
| virtual 100,000 startup to first present (ms) | floem | ~35.0 | 1.2 to 1,000 | cross-class: only 1 own valid cell(s) in class 'startup (ms)': level taken across all classes (cross-class, low confidence); column level from 11 frameworks |
| sheet 1,000 rows hover total p50 (ms) | dioxus | ~28.00 | 21.00 to 38.00 | own: 16 own valid cells in class 'latency (ms)' (log-level +0.81 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows hover total p50 (ms) | dioxus-skia | ~9.10 | 6.70 to 12.00 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.32 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows hover total p50 (ms) | floem | ~3.30 | 0.12 to 96.00 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 4 frameworks |
| sheet 1,000 rows hover total p50 (ms) | freya | ~11.00 | 7.80 to 14.00 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.17 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows hover total p50 (ms) | freya-056a5b4 | ~11.00 | 7.40 to 17.00 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.12 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows hover total p50 (ms) | gpui-kit-fast | ~49.00 | 34.00 to 72.00 | own: 16 own valid cells in class 'latency (ms)' (log-level +1.37 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows hover total p50 (ms) | vizia | ~47.00 | 1.20 to 1,800 | own: 14 own valid cells in class 'latency (ms)' (log-level +1.32 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows hover total p50 (ms) | zgui | ~18.00 | 5.30 to 63.00 | own: 16 own valid cells in class 'latency (ms)' (log-level +0.37 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows hover input to commit p50 (ms) | dioxus | ~28.00 | 21.00 to 38.00 | own: 16 own valid cells in class 'latency (ms)' (log-level +0.81 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows hover input to commit p50 (ms) | dioxus-skia | ~9.10 | 6.80 to 12.00 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.32 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows hover input to commit p50 (ms) | floem | ~3.30 | 0.12 to 96.00 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 4 frameworks |
| sheet 1,000 rows hover input to commit p50 (ms) | freya | ~11.00 | 7.80 to 14.00 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.17 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows hover input to commit p50 (ms) | freya-056a5b4 | ~11.00 | 7.40 to 17.00 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.12 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows hover input to commit p50 (ms) | gpui-kit-fast | ~49.00 | 34.00 to 72.00 | own: 16 own valid cells in class 'latency (ms)' (log-level +1.37 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows hover input to commit p50 (ms) | vizia | ~47.00 | 1.20 to 1,800 | own: 14 own valid cells in class 'latency (ms)' (log-level +1.32 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows hover input to commit p50 (ms) | zgui | ~18.00 | 5.30 to 63.00 | own: 16 own valid cells in class 'latency (ms)' (log-level +0.37 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows theme switch total p50 (ms) | dioxus | ~49.00 | 36.00 to 66.00 | own: 16 own valid cells in class 'latency (ms)' (log-level +0.81 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows theme switch total p50 (ms) | dioxus-skia | ~16.00 | 12.00 to 21.00 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.32 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows theme switch total p50 (ms) | floem | ~5.70 | 0.20 to 170.0 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 4 frameworks |
| sheet 1,000 rows theme switch total p50 (ms) | freya | ~18.00 | 14.00 to 25.00 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.17 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows theme switch total p50 (ms) | freya-056a5b4 | ~19.00 | 13.00 to 29.00 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.12 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows theme switch total p50 (ms) | gpui-kit-fast | ~85.00 | 59.00 to 120.0 | own: 16 own valid cells in class 'latency (ms)' (log-level +1.37 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows theme switch total p50 (ms) | vizia | ~81.00 | 2.00 to 3,200 | own: 14 own valid cells in class 'latency (ms)' (log-level +1.32 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows theme switch total p50 (ms) | zgui | ~31.00 | 9.10 to 110.0 | own: 16 own valid cells in class 'latency (ms)' (log-level +0.37 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows edit total p50 (ms) | dioxus | ~30.00 | 22.00 to 40.00 | own: 16 own valid cells in class 'latency (ms)' (log-level +0.81 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows edit total p50 (ms) | dioxus-skia | ~9.70 | 7.20 to 13.00 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.32 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows edit total p50 (ms) | floem | ~3.50 | 0.12 to 100.0 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 4 frameworks |
| sheet 1,000 rows edit total p50 (ms) | freya | ~11.00 | 8.30 to 15.00 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.17 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows edit total p50 (ms) | freya-056a5b4 | ~12.00 | 7.90 to 18.00 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.12 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows edit total p50 (ms) | gpui-kit-fast | ~52.00 | 36.00 to 76.00 | own: 16 own valid cells in class 'latency (ms)' (log-level +1.37 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows edit total p50 (ms) | vizia | ~49.00 | 1.30 to 2,000 | own: 14 own valid cells in class 'latency (ms)' (log-level +1.32 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows edit total p50 (ms) | zgui | ~19.00 | 5.60 to 67.00 | own: 16 own valid cells in class 'latency (ms)' (log-level +0.37 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows resize configure to present p50 (ms) | dioxus | ~43.0 | 32.0 to 58.0 | own: 16 own valid cells in class 'latency (ms)' (log-level +0.81 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows resize configure to present p50 (ms) | dioxus-skia | ~14.0 | 10.0 to 19.0 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.32 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows resize configure to present p50 (ms) | floem | ~5.1 | 0.2 to 150.0 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 4 frameworks |
| sheet 1,000 rows resize configure to present p50 (ms) | freya | ~16.0 | 12.0 to 22.0 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.17 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows resize configure to present p50 (ms) | freya-056a5b4 | ~17.0 | 11.0 to 25.0 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.12 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows resize configure to present p50 (ms) | gpui-kit-fast | ~75.0 | 52.0 to 110.0 | own: 16 own valid cells in class 'latency (ms)' (log-level +1.37 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows resize configure to present p50 (ms) | vizia | ~71.0 | 1.8 to 2,800 | own: 14 own valid cells in class 'latency (ms)' (log-level +1.32 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows resize configure to present p50 (ms) | zgui | ~28.0 | 8.0 to 96.0 | own: 16 own valid cells in class 'latency (ms)' (log-level +0.37 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows idle RSS (MiB) | dioxus | ~200.0 | 150.0 to 270.0 | own: 5 own valid cells in class 'resident memory (MiB)' (log-level +0.14 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows idle RSS (MiB) | dioxus-skia | ~180.0 | 130.0 to 240.0 | own: 5 own valid cells in class 'resident memory (MiB)' (log-level +0.01 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows idle RSS (MiB) | floem | ~140.0 | 110.0 to 190.0 | own: 2 own valid cells in class 'resident memory (MiB)' (log-level -0.20 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows idle RSS (MiB) | freya | ~170.0 | 120.0 to 230.0 | own: 5 own valid cells in class 'resident memory (MiB)' (log-level -0.04 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows idle RSS (MiB) | freya-056a5b4 | ~170.0 | 120.0 to 230.0 | own: 5 own valid cells in class 'resident memory (MiB)' (log-level -0.04 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows idle RSS (MiB) | gpui-kit-fast | ~230.0 | 170.0 to 310.0 | own: 5 own valid cells in class 'resident memory (MiB)' (log-level +0.28 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows idle RSS (MiB) | vizia | ~170.0 | 130.0 to 240.0 | own: 5 own valid cells in class 'resident memory (MiB)' (log-level +0.00 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows idle RSS (MiB) | zgui | ~210.0 | 160.0 to 290.0 | own: 5 own valid cells in class 'resident memory (MiB)' (log-level +0.19 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows idle CPU (ms/s) | dioxus | ~33.00 | 19.00 to 56.00 | own: 4 own valid cells in class 'idle CPU (ms/s)' (log-level +0.36 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows idle CPU (ms/s) | dioxus-skia | ~13.00 | 7.00 to 26.00 | own: 4 own valid cells in class 'idle CPU (ms/s)' (log-level -0.53 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows idle CPU (ms/s) | floem | ~0.81 | 0.22 to 2.90 | own: 2 own valid cells in class 'idle CPU (ms/s)' (log-level -3.34 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows idle CPU (ms/s) | freya | ~14.00 | 3.90 to 52.00 | own: 4 own valid cells in class 'idle CPU (ms/s)' (log-level -0.48 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows idle CPU (ms/s) | freya-056a5b4 | ~13.00 | 3.40 to 52.00 | own: 4 own valid cells in class 'idle CPU (ms/s)' (log-level -0.53 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows idle CPU (ms/s) | gpui-kit-fast | ~58.00 | 15.00 to 220.0 | own: 4 own valid cells in class 'idle CPU (ms/s)' (log-level +0.93 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows idle CPU (ms/s) | vizia | ~8.60 | 1.40 to 53.00 | own: 4 own valid cells in class 'idle CPU (ms/s)' (log-level -0.98 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows idle CPU (ms/s) | zgui | ~7.00 | 3.90 to 13.00 | own: 4 own valid cells in class 'idle CPU (ms/s)' (log-level -1.18 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows hover total p50 (ms) | dioxus | ~77.00 | 57.00 to 100.0 | own: 16 own valid cells in class 'latency (ms)' (log-level +0.81 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows hover total p50 (ms) | dioxus-skia | ~25.00 | 19.00 to 34.00 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.32 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows hover total p50 (ms) | floem | ~9.10 | 0.32 to 260.0 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 4 frameworks |
| sheet 10,000 rows hover total p50 (ms) | freya | ~29.00 | 22.00 to 39.00 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.17 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows hover total p50 (ms) | freya-056a5b4 | ~30.00 | 20.00 to 46.00 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.12 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows hover total p50 (ms) | gpui-kit-fast | ~140.0 | 93.00 to 200.0 | own: 16 own valid cells in class 'latency (ms)' (log-level +1.37 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows hover total p50 (ms) | vizia | ~130.0 | 3.20 to 5,100 | own: 14 own valid cells in class 'latency (ms)' (log-level +1.32 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows hover total p50 (ms) | zgui | ~50.00 | 14.00 to 170.0 | own: 16 own valid cells in class 'latency (ms)' (log-level +0.37 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows hover input to commit p50 (ms) | dioxus | ~77.00 | 57.00 to 100.0 | own: 16 own valid cells in class 'latency (ms)' (log-level +0.81 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows hover input to commit p50 (ms) | dioxus-skia | ~25.00 | 19.00 to 34.00 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.32 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows hover input to commit p50 (ms) | floem | ~9.10 | 0.32 to 260.0 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 4 frameworks |
| sheet 10,000 rows hover input to commit p50 (ms) | freya | ~29.00 | 22.00 to 39.00 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.17 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows hover input to commit p50 (ms) | freya-056a5b4 | ~30.00 | 20.00 to 46.00 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.12 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows hover input to commit p50 (ms) | gpui-kit-fast | ~140.0 | 93.00 to 200.0 | own: 16 own valid cells in class 'latency (ms)' (log-level +1.37 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows hover input to commit p50 (ms) | vizia | ~130.0 | 3.20 to 5,100 | own: 14 own valid cells in class 'latency (ms)' (log-level +1.32 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows hover input to commit p50 (ms) | zgui | ~50.00 | 14.00 to 170.0 | own: 16 own valid cells in class 'latency (ms)' (log-level +0.37 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows theme switch total p50 (ms) | dioxus | ~98.00 | 73.00 to 130.0 | own: 16 own valid cells in class 'latency (ms)' (log-level +0.81 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows theme switch total p50 (ms) | dioxus-skia | ~32.00 | 24.00 to 43.00 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.32 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows theme switch total p50 (ms) | floem | ~12.00 | 0.40 to 330.0 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 4 frameworks |
| sheet 10,000 rows theme switch total p50 (ms) | freya | ~37.00 | 27.00 to 50.00 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.17 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows theme switch total p50 (ms) | freya-056a5b4 | ~39.00 | 26.00 to 58.00 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.12 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows theme switch total p50 (ms) | gpui-kit-fast | ~170.0 | 120.0 to 250.0 | own: 16 own valid cells in class 'latency (ms)' (log-level +1.37 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows theme switch total p50 (ms) | vizia | ~160.0 | 4.10 to 6,400 | own: 14 own valid cells in class 'latency (ms)' (log-level +1.32 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows theme switch total p50 (ms) | zgui | ~63.00 | 18.00 to 220.0 | own: 16 own valid cells in class 'latency (ms)' (log-level +0.37 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows edit total p50 (ms) | dioxus | ~160.0 | 120.0 to 220.0 | own: 16 own valid cells in class 'latency (ms)' (log-level +0.81 against the column medians); column level from 3 frameworks |
| sheet 10,000 rows edit total p50 (ms) | dioxus-skia | ~52.00 | 39.00 to 71.00 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.32 against the column medians); column level from 3 frameworks |
| sheet 10,000 rows edit total p50 (ms) | floem | ~19.00 | 0.66 to 550.0 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 3 frameworks |
| sheet 10,000 rows edit total p50 (ms) | freya | ~61.00 | 45.00 to 82.00 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.17 against the column medians); column level from 3 frameworks |
| sheet 10,000 rows edit total p50 (ms) | freya-056a5b4 | ~64.00 | 43.00 to 96.00 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.12 against the column medians); column level from 3 frameworks |
| sheet 10,000 rows edit total p50 (ms) | gpui-kit-fast | ~280.0 | 200.0 to 410.0 | own: 16 own valid cells in class 'latency (ms)' (log-level +1.37 against the column medians); column level from 3 frameworks |
| sheet 10,000 rows edit total p50 (ms) | vizia | ~270.0 | 6.80 to 11,000 | own: 14 own valid cells in class 'latency (ms)' (log-level +1.32 against the column medians); column level from 3 frameworks |
| sheet 10,000 rows edit total p50 (ms) | zgui | ~100.0 | 30.00 to 360.0 | own: 16 own valid cells in class 'latency (ms)' (log-level +0.37 against the column medians); column level from 3 frameworks |
| sheet 10,000 rows resize configure to present p50 (ms) | bough | ~3.8 | 0.2 to 64.0 | own: 33 own valid cells in class 'latency (ms)' (log-level -1.52 against the column medians); column level from 3 frameworks |
| sheet 10,000 rows resize configure to present p50 (ms) | dioxus | ~39.0 | 29.0 to 53.0 | own: 16 own valid cells in class 'latency (ms)' (log-level +0.81 against the column medians); column level from 3 frameworks |
| sheet 10,000 rows resize configure to present p50 (ms) | dioxus-skia | ~13.0 | 9.4 to 17.0 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.32 against the column medians); column level from 3 frameworks |
| sheet 10,000 rows resize configure to present p50 (ms) | floem | ~4.6 | 0.2 to 130.0 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 3 frameworks |
| sheet 10,000 rows resize configure to present p50 (ms) | freya | ~15.0 | 11.0 to 20.0 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.17 against the column medians); column level from 3 frameworks |
| sheet 10,000 rows resize configure to present p50 (ms) | freya-056a5b4 | ~15.0 | 10.0 to 23.0 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.12 against the column medians); column level from 3 frameworks |
| sheet 10,000 rows resize configure to present p50 (ms) | gpui-kit-fast | ~69.0 | 47.0 to 100.0 | own: 16 own valid cells in class 'latency (ms)' (log-level +1.37 against the column medians); column level from 3 frameworks |
| sheet 10,000 rows resize configure to present p50 (ms) | vizia | ~65.0 | 1.6 to 2,600 | own: 14 own valid cells in class 'latency (ms)' (log-level +1.32 against the column medians); column level from 3 frameworks |
| sheet 10,000 rows resize configure to present p50 (ms) | zgui | ~25.0 | 7.3 to 87.0 | own: 16 own valid cells in class 'latency (ms)' (log-level +0.37 against the column medians); column level from 3 frameworks |
| sheet 10,000 rows idle RSS (MiB) | dioxus | ~340.0 | 260.0 to 470.0 | own: 5 own valid cells in class 'resident memory (MiB)' (log-level +0.14 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows idle RSS (MiB) | dioxus-skia | ~300.0 | 220.0 to 410.0 | own: 5 own valid cells in class 'resident memory (MiB)' (log-level +0.01 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows idle RSS (MiB) | floem | ~250.0 | 180.0 to 330.0 | own: 2 own valid cells in class 'resident memory (MiB)' (log-level -0.20 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows idle RSS (MiB) | freya | ~290.0 | 210.0 to 390.0 | own: 5 own valid cells in class 'resident memory (MiB)' (log-level -0.04 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows idle RSS (MiB) | freya-056a5b4 | ~290.0 | 210.0 to 390.0 | own: 5 own valid cells in class 'resident memory (MiB)' (log-level -0.04 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows idle RSS (MiB) | gpui-kit-fast | ~400.0 | 290.0 to 540.0 | own: 5 own valid cells in class 'resident memory (MiB)' (log-level +0.28 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows idle RSS (MiB) | vizia | ~300.0 | 220.0 to 400.0 | own: 5 own valid cells in class 'resident memory (MiB)' (log-level +0.00 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows idle RSS (MiB) | zgui | ~360.0 | 270.0 to 490.0 | own: 5 own valid cells in class 'resident memory (MiB)' (log-level +0.19 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows idle CPU (ms/s) | dioxus | ~40.00 | 23.00 to 68.00 | own: 4 own valid cells in class 'idle CPU (ms/s)' (log-level +0.36 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows idle CPU (ms/s) | dioxus-skia | ~16.00 | 8.40 to 31.00 | own: 4 own valid cells in class 'idle CPU (ms/s)' (log-level -0.53 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows idle CPU (ms/s) | floem | ~0.98 | 0.27 to 3.50 | own: 2 own valid cells in class 'idle CPU (ms/s)' (log-level -3.34 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows idle CPU (ms/s) | freya | ~17.00 | 4.70 to 62.00 | own: 4 own valid cells in class 'idle CPU (ms/s)' (log-level -0.48 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows idle CPU (ms/s) | freya-056a5b4 | ~16.00 | 4.10 to 63.00 | own: 4 own valid cells in class 'idle CPU (ms/s)' (log-level -0.53 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows idle CPU (ms/s) | gpui-kit-fast | ~70.00 | 19.00 to 260.0 | own: 4 own valid cells in class 'idle CPU (ms/s)' (log-level +0.93 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows idle CPU (ms/s) | vizia | ~10.00 | 1.70 to 64.00 | own: 4 own valid cells in class 'idle CPU (ms/s)' (log-level -0.98 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows idle CPU (ms/s) | zgui | ~8.50 | 4.70 to 15.00 | own: 4 own valid cells in class 'idle CPU (ms/s)' (log-level -1.18 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes hover total p50 (ms) | dioxus | ~56.00 | 42.00 to 76.00 | own: 16 own valid cells in class 'latency (ms)' (log-level +0.81 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes hover total p50 (ms) | dioxus-skia | ~18.00 | 13.00 to 25.00 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.32 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes hover total p50 (ms) | floem | ~6.70 | 0.23 to 190.0 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 4 frameworks |
| canvas 1,000 nodes hover total p50 (ms) | freya | ~21.00 | 16.00 to 29.00 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.17 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes hover total p50 (ms) | freya-056a5b4 | ~22.00 | 15.00 to 33.00 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.12 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes hover total p50 (ms) | gpui-kit-fast | ~99.00 | 68.00 to 140.0 | own: 16 own valid cells in class 'latency (ms)' (log-level +1.37 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes hover total p50 (ms) | vizia | ~93.00 | 2.40 to 3,700 | own: 14 own valid cells in class 'latency (ms)' (log-level +1.32 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes hover total p50 (ms) | zgui | ~36.00 | 11.00 to 130.0 | own: 16 own valid cells in class 'latency (ms)' (log-level +0.37 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes hover input to commit p50 (ms) | dioxus | ~56.00 | 42.00 to 76.00 | own: 16 own valid cells in class 'latency (ms)' (log-level +0.81 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes hover input to commit p50 (ms) | dioxus-skia | ~18.00 | 14.00 to 25.00 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.32 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes hover input to commit p50 (ms) | floem | ~6.70 | 0.23 to 190.0 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 4 frameworks |
| canvas 1,000 nodes hover input to commit p50 (ms) | freya | ~21.00 | 16.00 to 29.00 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.17 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes hover input to commit p50 (ms) | freya-056a5b4 | ~22.00 | 15.00 to 33.00 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.12 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes hover input to commit p50 (ms) | gpui-kit-fast | ~99.00 | 68.00 to 140.0 | own: 16 own valid cells in class 'latency (ms)' (log-level +1.37 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes hover input to commit p50 (ms) | vizia | ~93.00 | 2.40 to 3,700 | own: 14 own valid cells in class 'latency (ms)' (log-level +1.32 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes hover input to commit p50 (ms) | zgui | ~36.00 | 11.00 to 130.0 | own: 16 own valid cells in class 'latency (ms)' (log-level +0.37 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes theme switch total p50 (ms) | dioxus | ~62.00 | 46.00 to 83.00 | own: 16 own valid cells in class 'latency (ms)' (log-level +0.81 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes theme switch total p50 (ms) | dioxus-skia | ~20.00 | 15.00 to 27.00 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.32 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes theme switch total p50 (ms) | floem | ~7.30 | 0.25 to 210.0 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 4 frameworks |
| canvas 1,000 nodes theme switch total p50 (ms) | freya | ~23.00 | 17.00 to 31.00 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.17 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes theme switch total p50 (ms) | freya-056a5b4 | ~24.00 | 16.00 to 36.00 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.12 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes theme switch total p50 (ms) | gpui-kit-fast | ~110.0 | 74.00 to 160.0 | own: 16 own valid cells in class 'latency (ms)' (log-level +1.37 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes theme switch total p50 (ms) | vizia | ~100.0 | 2.60 to 4,000 | own: 14 own valid cells in class 'latency (ms)' (log-level +1.32 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes theme switch total p50 (ms) | zgui | ~40.00 | 12.00 to 140.0 | own: 16 own valid cells in class 'latency (ms)' (log-level +0.37 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes edit total p50 (ms) | dioxus | ~58.00 | 43.00 to 79.00 | own: 16 own valid cells in class 'latency (ms)' (log-level +0.81 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes edit total p50 (ms) | dioxus-skia | ~19.00 | 14.00 to 25.00 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.32 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes edit total p50 (ms) | floem | ~6.90 | 0.24 to 200.0 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 4 frameworks |
| canvas 1,000 nodes edit total p50 (ms) | freya | ~22.00 | 16.00 to 30.00 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.17 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes edit total p50 (ms) | freya-056a5b4 | ~23.00 | 15.00 to 34.00 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.12 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes edit total p50 (ms) | gpui-kit-fast | ~100.0 | 70.00 to 150.0 | own: 16 own valid cells in class 'latency (ms)' (log-level +1.37 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes edit total p50 (ms) | vizia | ~97.00 | 2.40 to 3,800 | own: 14 own valid cells in class 'latency (ms)' (log-level +1.32 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes edit total p50 (ms) | zgui | ~38.00 | 11.00 to 130.0 | own: 16 own valid cells in class 'latency (ms)' (log-level +0.37 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes resize configure to present p50 (ms) | dioxus | ~55.0 | 41.0 to 74.0 | own: 16 own valid cells in class 'latency (ms)' (log-level +0.81 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes resize configure to present p50 (ms) | dioxus-skia | ~18.0 | 13.0 to 24.0 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.32 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes resize configure to present p50 (ms) | floem | ~6.5 | 0.2 to 190.0 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 4 frameworks |
| canvas 1,000 nodes resize configure to present p50 (ms) | freya | ~21.0 | 15.0 to 28.0 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.17 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes resize configure to present p50 (ms) | freya-056a5b4 | ~22.0 | 15.0 to 32.0 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.12 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes resize configure to present p50 (ms) | gpui-kit-fast | ~96.0 | 66.0 to 140.0 | own: 16 own valid cells in class 'latency (ms)' (log-level +1.37 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes resize configure to present p50 (ms) | vizia | ~91.0 | 2.3 to 3,600 | own: 14 own valid cells in class 'latency (ms)' (log-level +1.32 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes resize configure to present p50 (ms) | zgui | ~36.0 | 10.0 to 120.0 | own: 16 own valid cells in class 'latency (ms)' (log-level +0.37 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes idle RSS (MiB) | dioxus | ~210.0 | 160.0 to 290.0 | own: 5 own valid cells in class 'resident memory (MiB)' (log-level +0.14 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes idle RSS (MiB) | dioxus-skia | ~190.0 | 140.0 to 250.0 | own: 5 own valid cells in class 'resident memory (MiB)' (log-level +0.01 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes idle RSS (MiB) | floem | ~150.0 | 110.0 to 210.0 | own: 2 own valid cells in class 'resident memory (MiB)' (log-level -0.20 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes idle RSS (MiB) | freya | ~180.0 | 130.0 to 240.0 | own: 5 own valid cells in class 'resident memory (MiB)' (log-level -0.04 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes idle RSS (MiB) | freya-056a5b4 | ~180.0 | 130.0 to 240.0 | own: 5 own valid cells in class 'resident memory (MiB)' (log-level -0.04 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes idle RSS (MiB) | gpui-kit-fast | ~250.0 | 180.0 to 330.0 | own: 5 own valid cells in class 'resident memory (MiB)' (log-level +0.28 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes idle RSS (MiB) | vizia | ~190.0 | 140.0 to 250.0 | own: 5 own valid cells in class 'resident memory (MiB)' (log-level +0.00 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes idle RSS (MiB) | zgui | ~230.0 | 170.0 to 310.0 | own: 5 own valid cells in class 'resident memory (MiB)' (log-level +0.19 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes idle CPU (ms/s) | dioxus | ~65.00 | 38.00 to 110.0 | own: 4 own valid cells in class 'idle CPU (ms/s)' (log-level +0.36 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes idle CPU (ms/s) | dioxus-skia | ~27.00 | 14.00 to 51.00 | own: 4 own valid cells in class 'idle CPU (ms/s)' (log-level -0.53 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes idle CPU (ms/s) | floem | ~1.60 | 0.44 to 5.80 | own: 2 own valid cells in class 'idle CPU (ms/s)' (log-level -3.34 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes idle CPU (ms/s) | freya | ~28.00 | 7.60 to 100.0 | own: 4 own valid cells in class 'idle CPU (ms/s)' (log-level -0.48 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes idle CPU (ms/s) | freya-056a5b4 | ~26.00 | 6.70 to 100.0 | own: 4 own valid cells in class 'idle CPU (ms/s)' (log-level -0.53 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes idle CPU (ms/s) | gpui-kit-fast | ~110.0 | 31.00 to 430.0 | own: 4 own valid cells in class 'idle CPU (ms/s)' (log-level +0.93 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes idle CPU (ms/s) | vizia | ~17.00 | 2.80 to 100.0 | own: 4 own valid cells in class 'idle CPU (ms/s)' (log-level -0.98 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes idle CPU (ms/s) | zgui | ~14.00 | 7.70 to 25.00 | own: 4 own valid cells in class 'idle CPU (ms/s)' (log-level -1.18 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes hover total p50 (ms) | dioxus | ~190.0 | 140.0 to 260.0 | own: 16 own valid cells in class 'latency (ms)' (log-level +0.81 against the column medians); column level from 3 frameworks |
| canvas 5,000 nodes hover total p50 (ms) | dioxus-skia | ~62.00 | 46.00 to 83.00 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.32 against the column medians); column level from 3 frameworks |
| canvas 5,000 nodes hover total p50 (ms) | floem | ~23.00 | 0.78 to 650.0 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 3 frameworks |
| canvas 5,000 nodes hover total p50 (ms) | freya | ~72.00 | 53.00 to 97.00 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.17 against the column medians); column level from 3 frameworks |
| canvas 5,000 nodes hover total p50 (ms) | freya-056a5b4 | ~75.00 | 50.00 to 110.0 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.12 against the column medians); column level from 3 frameworks |
| canvas 5,000 nodes hover total p50 (ms) | gpui-kit-fast | ~330.0 | 230.0 to 490.0 | own: 16 own valid cells in class 'latency (ms)' (log-level +1.37 against the column medians); column level from 3 frameworks |
| canvas 5,000 nodes hover total p50 (ms) | vizia | ~320.0 | 8.00 to 12,000 | own: 14 own valid cells in class 'latency (ms)' (log-level +1.32 against the column medians); column level from 3 frameworks |
| canvas 5,000 nodes hover total p50 (ms) | zgui | ~120.0 | 36.00 to 430.0 | own: 16 own valid cells in class 'latency (ms)' (log-level +0.37 against the column medians); column level from 3 frameworks |
| canvas 5,000 nodes hover input to commit p50 (ms) | dioxus | ~190.0 | 140.0 to 260.0 | own: 16 own valid cells in class 'latency (ms)' (log-level +0.81 against the column medians); column level from 3 frameworks |
| canvas 5,000 nodes hover input to commit p50 (ms) | dioxus-skia | ~62.00 | 46.00 to 83.00 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.32 against the column medians); column level from 3 frameworks |
| canvas 5,000 nodes hover input to commit p50 (ms) | floem | ~23.00 | 0.78 to 650.0 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 3 frameworks |
| canvas 5,000 nodes hover input to commit p50 (ms) | freya | ~72.00 | 53.00 to 97.00 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.17 against the column medians); column level from 3 frameworks |
| canvas 5,000 nodes hover input to commit p50 (ms) | freya-056a5b4 | ~75.00 | 50.00 to 110.0 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.12 against the column medians); column level from 3 frameworks |
| canvas 5,000 nodes hover input to commit p50 (ms) | gpui-kit-fast | ~330.0 | 230.0 to 490.0 | own: 16 own valid cells in class 'latency (ms)' (log-level +1.37 against the column medians); column level from 3 frameworks |
| canvas 5,000 nodes hover input to commit p50 (ms) | vizia | ~320.0 | 8.00 to 12,000 | own: 14 own valid cells in class 'latency (ms)' (log-level +1.32 against the column medians); column level from 3 frameworks |
| canvas 5,000 nodes hover input to commit p50 (ms) | zgui | ~120.0 | 36.00 to 430.0 | own: 16 own valid cells in class 'latency (ms)' (log-level +0.37 against the column medians); column level from 3 frameworks |
| canvas 5,000 nodes theme switch total p50 (ms) | dioxus | ~210.0 | 160.0 to 290.0 | own: 16 own valid cells in class 'latency (ms)' (log-level +0.81 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes theme switch total p50 (ms) | dioxus-skia | ~69.00 | 51.00 to 93.00 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.32 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes theme switch total p50 (ms) | floem | ~25.00 | 0.88 to 730.0 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 4 frameworks |
| canvas 5,000 nodes theme switch total p50 (ms) | freya | ~80.00 | 60.00 to 110.0 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.17 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes theme switch total p50 (ms) | freya-056a5b4 | ~84.00 | 56.00 to 130.0 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.12 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes theme switch total p50 (ms) | gpui-kit-fast | ~370.0 | 260.0 to 540.0 | own: 16 own valid cells in class 'latency (ms)' (log-level +1.37 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes theme switch total p50 (ms) | vizia | ~350.0 | 9.00 to 14,000 | own: 14 own valid cells in class 'latency (ms)' (log-level +1.32 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes theme switch total p50 (ms) | zgui | ~140.0 | 40.00 to 480.0 | own: 16 own valid cells in class 'latency (ms)' (log-level +0.37 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes edit total p50 (ms) | dioxus | ~190.0 | 140.0 to 260.0 | own: 16 own valid cells in class 'latency (ms)' (log-level +0.81 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes edit total p50 (ms) | dioxus-skia | ~63.00 | 46.00 to 85.00 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.32 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes edit total p50 (ms) | floem | ~23.00 | 0.79 to 660.0 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 4 frameworks |
| canvas 5,000 nodes edit total p50 (ms) | freya | ~73.00 | 54.00 to 98.00 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.17 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes edit total p50 (ms) | freya-056a5b4 | ~76.00 | 51.00 to 110.0 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.12 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes edit total p50 (ms) | gpui-kit-fast | ~340.0 | 230.0 to 490.0 | own: 16 own valid cells in class 'latency (ms)' (log-level +1.37 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes edit total p50 (ms) | vizia | ~320.0 | 8.10 to 13,000 | own: 14 own valid cells in class 'latency (ms)' (log-level +1.32 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes edit total p50 (ms) | zgui | ~130.0 | 36.00 to 430.0 | own: 16 own valid cells in class 'latency (ms)' (log-level +0.37 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes resize configure to present p50 (ms) | dioxus | ~210.0 | 160.0 to 280.0 | own: 16 own valid cells in class 'latency (ms)' (log-level +0.81 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes resize configure to present p50 (ms) | dioxus-skia | ~68.0 | 50.0 to 92.0 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.32 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes resize configure to present p50 (ms) | floem | ~25.0 | 0.9 to 720.0 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 4 frameworks |
| canvas 5,000 nodes resize configure to present p50 (ms) | freya | ~79.0 | 59.0 to 110.0 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.17 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes resize configure to present p50 (ms) | freya-056a5b4 | ~83.0 | 56.0 to 120.0 | own: 16 own valid cells in class 'latency (ms)' (log-level -0.12 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes resize configure to present p50 (ms) | gpui-kit-fast | ~370.0 | 250.0 to 540.0 | own: 16 own valid cells in class 'latency (ms)' (log-level +1.37 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes resize configure to present p50 (ms) | vizia | ~350.0 | 8.8 to 14,000 | own: 14 own valid cells in class 'latency (ms)' (log-level +1.32 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes resize configure to present p50 (ms) | zgui | ~140.0 | 39.0 to 470.0 | own: 16 own valid cells in class 'latency (ms)' (log-level +0.37 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes idle RSS (MiB) | dioxus | ~320.0 | 240.0 to 440.0 | own: 5 own valid cells in class 'resident memory (MiB)' (log-level +0.14 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes idle RSS (MiB) | dioxus-skia | ~280.0 | 210.0 to 380.0 | own: 5 own valid cells in class 'resident memory (MiB)' (log-level +0.01 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes idle RSS (MiB) | floem | ~230.0 | 170.0 to 310.0 | own: 2 own valid cells in class 'resident memory (MiB)' (log-level -0.20 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes idle RSS (MiB) | freya | ~270.0 | 200.0 to 370.0 | own: 5 own valid cells in class 'resident memory (MiB)' (log-level -0.04 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes idle RSS (MiB) | freya-056a5b4 | ~270.0 | 200.0 to 370.0 | own: 5 own valid cells in class 'resident memory (MiB)' (log-level -0.04 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes idle RSS (MiB) | gpui-kit-fast | ~370.0 | 280.0 to 500.0 | own: 5 own valid cells in class 'resident memory (MiB)' (log-level +0.28 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes idle RSS (MiB) | vizia | ~280.0 | 210.0 to 380.0 | own: 5 own valid cells in class 'resident memory (MiB)' (log-level +0.00 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes idle RSS (MiB) | zgui | ~340.0 | 250.0 to 460.0 | own: 5 own valid cells in class 'resident memory (MiB)' (log-level +0.19 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes idle CPU (ms/s) | dioxus | ~260.0 | 150.0 to 450.0 | own: 4 own valid cells in class 'idle CPU (ms/s)' (log-level +0.36 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes idle CPU (ms/s) | dioxus-skia | ~110.0 | 56.00 to 210.0 | own: 4 own valid cells in class 'idle CPU (ms/s)' (log-level -0.53 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes idle CPU (ms/s) | floem | ~6.50 | 1.80 to 23.00 | own: 2 own valid cells in class 'idle CPU (ms/s)' (log-level -3.34 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes idle CPU (ms/s) | freya | ~110.0 | 31.00 to 410.0 | own: 4 own valid cells in class 'idle CPU (ms/s)' (log-level -0.48 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes idle CPU (ms/s) | freya-056a5b4 | ~110.0 | 27.00 to 420.0 | own: 4 own valid cells in class 'idle CPU (ms/s)' (log-level -0.53 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes idle CPU (ms/s) | gpui-kit-fast | ~460.0 | 120.0 to 1,700 | own: 4 own valid cells in class 'idle CPU (ms/s)' (log-level +0.93 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes idle CPU (ms/s) | vizia | ~69.00 | 11.00 to 420.0 | own: 4 own valid cells in class 'idle CPU (ms/s)' (log-level -0.98 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes idle CPU (ms/s) | zgui | ~56.00 | 31.00 to 100.0 | own: 4 own valid cells in class 'idle CPU (ms/s)' (log-level -1.18 against the column medians); column level from 4 frameworks |
| sheet 1,000 startup to first present (ms) | dioxus | ~270.0 | 190.0 to 390.0 | own: 3 own valid cells in class 'startup (ms)' (log-level +0.47 against the column medians); column level from 4 frameworks |
| sheet 1,000 startup to first present (ms) | dioxus-skia | ~180.0 | 140.0 to 250.0 | own: 3 own valid cells in class 'startup (ms)' (log-level +0.08 against the column medians); column level from 4 frameworks |
| sheet 1,000 startup to first present (ms) | floem | ~45.0 | 1.6 to 1,300 | cross-class: only 1 own valid cell(s) in class 'startup (ms)': level taken across all classes (cross-class, low confidence); column level from 4 frameworks |
| sheet 1,000 startup to first present (ms) | freya | ~160.0 | 120.0 to 220.0 | own: 3 own valid cells in class 'startup (ms)' (log-level -0.05 against the column medians); column level from 4 frameworks |
| sheet 1,000 startup to first present (ms) | freya-056a5b4 | ~160.0 | 120.0 to 220.0 | own: 3 own valid cells in class 'startup (ms)' (log-level -0.05 against the column medians); column level from 4 frameworks |
| sheet 1,000 startup to first present (ms) | gpui-kit-fast | ~420.0 | 310.0 to 560.0 | own: 3 own valid cells in class 'startup (ms)' (log-level +0.90 against the column medians); column level from 4 frameworks |
| sheet 1,000 startup to first present (ms) | vizia | ~140.0 | 83.0 to 220.0 | own: 3 own valid cells in class 'startup (ms)' (log-level -0.23 against the column medians); column level from 4 frameworks |
| sheet 1,000 startup to first present (ms) | zgui | ~320.0 | 200.0 to 510.0 | own: 3 own valid cells in class 'startup (ms)' (log-level +0.63 against the column medians); column level from 4 frameworks |
| sheet 10,000 startup to first present (ms) | dioxus | ~600.0 | 410.0 to 880.0 | own: 3 own valid cells in class 'startup (ms)' (log-level +0.47 against the column medians); column level from 4 frameworks |
| sheet 10,000 startup to first present (ms) | dioxus-skia | ~410.0 | 300.0 to 550.0 | own: 3 own valid cells in class 'startup (ms)' (log-level +0.08 against the column medians); column level from 4 frameworks |
| sheet 10,000 startup to first present (ms) | floem | ~100.0 | 3.5 to 2,900 | cross-class: only 1 own valid cell(s) in class 'startup (ms)': level taken across all classes (cross-class, low confidence); column level from 4 frameworks |
| sheet 10,000 startup to first present (ms) | freya | ~360.0 | 270.0 to 490.0 | own: 3 own valid cells in class 'startup (ms)' (log-level -0.05 against the column medians); column level from 4 frameworks |
| sheet 10,000 startup to first present (ms) | freya-056a5b4 | ~360.0 | 260.0 to 480.0 | own: 3 own valid cells in class 'startup (ms)' (log-level -0.05 against the column medians); column level from 4 frameworks |
| sheet 10,000 startup to first present (ms) | gpui-kit-fast | ~930.0 | 690.0 to 1,300 | own: 3 own valid cells in class 'startup (ms)' (log-level +0.90 against the column medians); column level from 4 frameworks |
| sheet 10,000 startup to first present (ms) | vizia | ~300.0 | 180.0 to 490.0 | own: 3 own valid cells in class 'startup (ms)' (log-level -0.23 against the column medians); column level from 4 frameworks |
| sheet 10,000 startup to first present (ms) | zgui | ~710.0 | 440.0 to 1,100 | own: 3 own valid cells in class 'startup (ms)' (log-level +0.63 against the column medians); column level from 4 frameworks |
| canvas 1,000 startup to first present (ms) | dioxus | ~980.0 | 670.0 to 1,400 | own: 3 own valid cells in class 'startup (ms)' (log-level +0.47 against the column medians); column level from 4 frameworks |
| canvas 1,000 startup to first present (ms) | dioxus-skia | ~660.0 | 490.0 to 900.0 | own: 3 own valid cells in class 'startup (ms)' (log-level +0.08 against the column medians); column level from 4 frameworks |
| canvas 1,000 startup to first present (ms) | floem | ~160.0 | 5.7 to 4,700 | cross-class: only 1 own valid cell(s) in class 'startup (ms)': level taken across all classes (cross-class, low confidence); column level from 4 frameworks |
| canvas 1,000 startup to first present (ms) | freya | ~590.0 | 430.0 to 790.0 | own: 3 own valid cells in class 'startup (ms)' (log-level -0.05 against the column medians); column level from 4 frameworks |
| canvas 1,000 startup to first present (ms) | freya-056a5b4 | ~580.0 | 430.0 to 790.0 | own: 3 own valid cells in class 'startup (ms)' (log-level -0.05 against the column medians); column level from 4 frameworks |
| canvas 1,000 startup to first present (ms) | gpui-kit-fast | ~1,500 | 1,100 to 2,000 | own: 3 own valid cells in class 'startup (ms)' (log-level +0.90 against the column medians); column level from 4 frameworks |
| canvas 1,000 startup to first present (ms) | vizia | ~490.0 | 300.0 to 800.0 | own: 3 own valid cells in class 'startup (ms)' (log-level -0.23 against the column medians); column level from 4 frameworks |
| canvas 1,000 startup to first present (ms) | zgui | ~1,200 | 720.0 to 1,900 | own: 3 own valid cells in class 'startup (ms)' (log-level +0.63 against the column medians); column level from 4 frameworks |
| canvas 5,000 startup to first present (ms) | dioxus | ~1,800 | 1,200 to 2,600 | own: 3 own valid cells in class 'startup (ms)' (log-level +0.47 against the column medians); column level from 4 frameworks |
| canvas 5,000 startup to first present (ms) | dioxus-skia | ~1,200 | 890.0 to 1,600 | own: 3 own valid cells in class 'startup (ms)' (log-level +0.08 against the column medians); column level from 4 frameworks |
| canvas 5,000 startup to first present (ms) | floem | ~300.0 | 10.0 to 8,500 | cross-class: only 1 own valid cell(s) in class 'startup (ms)': level taken across all classes (cross-class, low confidence); column level from 4 frameworks |
| canvas 5,000 startup to first present (ms) | freya | ~1,100 | 790.0 to 1,400 | own: 3 own valid cells in class 'startup (ms)' (log-level -0.05 against the column medians); column level from 4 frameworks |
| canvas 5,000 startup to first present (ms) | freya-056a5b4 | ~1,100 | 780.0 to 1,400 | own: 3 own valid cells in class 'startup (ms)' (log-level -0.05 against the column medians); column level from 4 frameworks |
| canvas 5,000 startup to first present (ms) | gpui-kit-fast | ~2,700 | 2,000 to 3,700 | own: 3 own valid cells in class 'startup (ms)' (log-level +0.90 against the column medians); column level from 4 frameworks |
| canvas 5,000 startup to first present (ms) | vizia | ~890.0 | 540.0 to 1,500 | own: 3 own valid cells in class 'startup (ms)' (log-level -0.23 against the column medians); column level from 4 frameworks |
| canvas 5,000 startup to first present (ms) | zgui | ~2,100 | 1,300 to 3,400 | own: 3 own valid cells in class 'startup (ms)' (log-level +0.63 against the column medians); column level from 4 frameworks |
| spinner above 10,000 rows: presented frames per second (fps) | floem | ~13.0 | 9.3 to 17.0 | own: 2 own valid cells in class 'frame rate (fps)' (log-level -1.33 against the column medians); column level from 11 frameworks |
| spinner above 10,000 rows: frame interval p50 (ms) | floem | ~73.00 | 26.00 to 210.0 | own: 2 own valid cells in class 'frame interval (ms)' (log-level +0.78 against the column medians); column level from 11 frameworks |
| spinner above 10,000 rows: frame interval p99 (ms) | floem | ~91.00 | 32.00 to 260.0 | own: 2 own valid cells in class 'frame interval (ms)' (log-level +0.78 against the column medians); column level from 11 frameworks |
| spinner above 10,000 rows: CPU per frame (ms) | floem | ~4.60 | 3.40 to 6.20 | own: 2 own valid cells in class 'cpu per frame (ms)' (log-level -1.91 against the column medians); column level from 11 frameworks |
| spinner above 10,000 rows: CPU per second (ms/s) | floem | ~84.0 | 23.0 to 300.0 | own: 2 own valid cells in class 'idle CPU (ms/s)' (log-level -3.34 against the column medians); column level from 11 frameworks |
| spinner above 10,000 rows: RSS (MiB) | floem | ~320.0 | 230.0 to 430.0 | own: 2 own valid cells in class 'resident memory (MiB)' (log-level -0.20 against the column medians); column level from 11 frameworks |

## Heavy scenes: Sheet, 1,000 rows

| framework | sheet 1,000 rows hover total p50 (ms) | sheet 1,000 rows hover input to commit p50 (ms) | sheet 1,000 rows theme switch total p50 (ms) | sheet 1,000 rows edit total p50 (ms) | sheet 1,000 rows resize configure to present p50 (ms) | sheet 1,000 rows idle RSS (MiB) | sheet 1,000 rows idle CPU (ms/s) | sheet 1,000 startup to first present (ms) |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| zgui | *~18.00* | *~18.00* | *~31.00* | *~19.00* | *~28.0* | *~210.0* | *~7.00* | *~320.0* |
| gpui-kit | 24.71 | 24.72 | 25.35 | 27.91 | 39.3 | 187.2 | 62.46 | - |
| gpui-kit-fast | *~49.00* | *~49.00* | *~85.00* | *~52.00* | *~75.0* | *~230.0* | *~58.00* | *~420.0* |
| freya | *~11.00* | *~11.00* | *~18.00* | *~11.00* | *~16.0* | *~170.0* | *~14.00* | *~160.0* |
| freya-056a5b4 | *~11.00* | *~11.00* | *~19.00* | *~12.00* | *~17.0* | *~170.0* | *~13.00* | *~160.0* |
| vizia | *~47.00* | *~47.00* | *~81.00* | *~49.00* | *~71.0* | *~170.0* | *~8.60* | *~140.0* |
| floem | *~3.30* | *~3.30* | *~5.70* | *~3.50* | *~5.1* | *~140.0* | *~0.81* | *~45.0* |
| iced | 18.93 | 18.95 | 18.29 | 17.94 | 14.7 | 185.5 | 22.95 | - |
| egui | 10.01 | 10.02 | 14.57 | 9.96 | 8.5 | 148.4 | 22.15 | - |
| dioxus | *~28.00* | *~28.00* | *~49.00* | *~30.00* | *~43.0* | *~200.0* | *~33.00* | *~270.0* |
| dioxus-skia | *~9.10* | *~9.10* | *~16.00* | *~9.70* | *~14.0* | *~180.0* | *~13.00* | *~180.0* |
| bough | 2.45 | 2.46 | 6.20 | 1.53 | 7.9 | 49.8 | 0.04 | - |

## Heavy scenes: Sheet, 10,000 rows

| framework | sheet 10,000 rows hover total p50 (ms) | sheet 10,000 rows hover input to commit p50 (ms) | sheet 10,000 rows theme switch total p50 (ms) | sheet 10,000 rows edit total p50 (ms) | sheet 10,000 rows resize configure to present p50 (ms) | sheet 10,000 rows idle RSS (MiB) | sheet 10,000 rows idle CPU (ms/s) | sheet 10,000 startup to first present (ms) |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| zgui | *~50.00* | *~50.00* | *~63.00* | *~100.0* | *~25.0* | *~360.0* | *~8.50* | *~710.0* |
| gpui-kit | 20.81 | 20.83 | 27.12 | 30.67 † | 36.6 | 186.4 | 73.51 | - |
| gpui-kit-fast | *~140.0* | *~140.0* | *~170.0* | *~280.0* | *~69.0* | *~400.0* | *~70.00* | *~930.0* |
| freya | *~29.00* | *~29.00* | *~37.00* | *~61.00* | *~15.0* | *~290.0* | *~17.00* | *~360.0* |
| freya-056a5b4 | *~30.00* | *~30.00* | *~39.00* | *~64.00* | *~15.0* | *~290.0* | *~16.00* | *~360.0* |
| vizia | *~130.0* | *~130.0* | *~160.0* | *~270.0* | *~65.0* | *~300.0* | *~10.00* | *~300.0* |
| floem | *~9.10* | *~9.10* | *~12.00* | *~19.00* | *~4.6* | *~250.0* | *~0.98* | *~100.0* |
| iced | 175.3 | 175.3 | 191.3 | 223.9 | 155.5 | 550.9 | 31.95 | - |
| egui | 9.95 | 9.97 | 11.19 | 9.55 | 9.0 | 149.0 | 27.37 | - |
| dioxus | *~77.00* | *~77.00* | *~98.00* | *~160.0* | *~39.0* | *~340.0* | *~40.00* | *~600.0* |
| dioxus-skia | *~25.00* | *~25.00* | *~32.00* | *~52.00* | *~13.0* | *~300.0* | *~16.00* | *~410.0* |
| bough | 19.40 | 19.41 | 27.91 | 15.70 | *~3.8* (failed cell) | 331.3 | 0.03 | - |

## Heavy scenes: Canvas, 1,000 nodes

| framework | canvas 1,000 nodes hover total p50 (ms) | canvas 1,000 nodes hover input to commit p50 (ms) | canvas 1,000 nodes theme switch total p50 (ms) | canvas 1,000 nodes edit total p50 (ms) | canvas 1,000 nodes resize configure to present p50 (ms) | canvas 1,000 nodes idle RSS (MiB) | canvas 1,000 nodes idle CPU (ms/s) | canvas 1,000 startup to first present (ms) |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| zgui | *~36.00* | *~36.00* | *~40.00* | *~38.00* | *~36.0* | *~230.0* | *~14.00* | *~1,200* |
| gpui-kit | 39.81 | 39.82 | 40.62 | 42.13 | 45.0 | 195.5 | 121.2 | - |
| gpui-kit-fast | *~99.00* | *~99.00* | *~110.0* | *~100.0* | *~96.0* | *~250.0* | *~110.0* | *~1,500* |
| freya | *~21.00* | *~21.00* | *~23.00* | *~22.00* | *~21.0* | *~180.0* | *~28.00* | *~590.0* |
| freya-056a5b4 | *~22.00* | *~22.00* | *~24.00* | *~23.00* | *~22.0* | *~180.0* | *~26.00* | *~580.0* |
| vizia | *~93.00* | *~93.00* | *~100.0* | *~97.00* | *~91.0* | *~190.0* | *~17.00* | *~490.0* |
| floem | *~6.70* | *~6.70* | *~7.30* | *~6.90* | *~6.5* | *~150.0* | *~1.60* | *~160.0* |
| iced | 17.56 | 17.57 | 19.92 | 19.18 | 18.6 | 426.8 | 50.97 | - |
| egui | 20.37 | 20.39 | 21.52 | 20.14 | 18.3 | 172.8 | 55.21 | - |
| dioxus | *~56.00* | *~56.00* | *~62.00* | *~58.00* | *~55.0* | *~210.0* | *~65.00* | *~980.0* |
| dioxus-skia | *~18.00* | *~18.00* | *~20.00* | *~19.00* | *~18.0* | *~190.0* | *~27.00* | *~660.0* |
| bough | 83.29 | 83.30 | 285.6 | 5.79 | 87.7 | 19.4 | 0.03 | - |

## Heavy scenes: Canvas, 5,000 nodes

| framework | canvas 5,000 nodes hover total p50 (ms) | canvas 5,000 nodes hover input to commit p50 (ms) | canvas 5,000 nodes theme switch total p50 (ms) | canvas 5,000 nodes edit total p50 (ms) | canvas 5,000 nodes resize configure to present p50 (ms) | canvas 5,000 nodes idle RSS (MiB) | canvas 5,000 nodes idle CPU (ms/s) | canvas 5,000 startup to first present (ms) |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| zgui | *~120.0* | *~120.0* | *~140.0* | *~130.0* | *~140.0* | *~340.0* | *~56.00* | *~2,100* |
| gpui-kit | 178.0 | 178.0 | 184.7 | 181.2 | 155.6 | 262.5 | 476.1 | - |
| gpui-kit-fast | *~330.0* | *~330.0* | *~370.0* | *~340.0* | *~370.0* | *~370.0* | *~460.0* | *~2,700* |
| freya | *~72.00* | *~72.00* | *~80.00* | *~73.00* | *~79.0* | *~270.0* | *~110.0* | *~1,100* |
| freya-056a5b4 | *~75.00* | *~75.00* | *~84.00* | *~76.00* | *~83.0* | *~270.0* | *~110.0* | *~1,100* |
| vizia | *~320.0* | *~320.0* | *~350.0* | *~320.0* | *~350.0* | *~280.0* | *~69.00* | *~890.0* |
| floem | *~23.00* | *~23.00* | *~25.00* | *~23.00* | *~25.0* | *~230.0* | *~6.50* | *~300.0* |
| iced | 68.36 | 68.38 | 72.17 | 76.06 | 71.2 | 1,428 | 211.9 | - |
| egui | 63.08 | 63.10 | 71.46 | 59.46 | 70.2 | 293.8 | 193.6 | - |
| dioxus | *~190.0* | *~190.0* | *~210.0* | *~190.0* | *~210.0* | *~320.0* | *~260.0* | *~1,800* |
| dioxus-skia | *~62.00* | *~62.00* | *~69.00* | *~63.00* | *~68.0* | *~280.0* | *~110.0* | *~1,200* |
| bough | 917.4 † | 917.4 † | 1,151 | 18.70 | 385.5 | 25.8 | 0.04 | - |

## Animation: the spinner scenarios (the only animation cells)

| framework | spinner, no rows: presented frames per second (fps) | spinner, no rows: frame interval p50 (ms) | spinner, no rows: frame interval p99 (ms) | spinner, no rows: jank fraction (fraction) | spinner, no rows: missed vblanks (count) | spinner, no rows: CPU per frame (ms) | spinner, no rows: CPU per second (ms/s) | spinner above 10,000 rows: presented frames per second (fps) | spinner above 10,000 rows: frame interval p50 (ms) | spinner above 10,000 rows: frame interval p99 (ms) | spinner above 10,000 rows: jank fraction (fraction) | spinner above 10,000 rows: missed vblanks (count) | spinner above 10,000 rows: CPU per frame (ms) | spinner above 10,000 rows: CPU per second (ms/s) |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| zgui | 60.0 | 16.68 | 17.49 | 0.000 | 0 | 12.02 | 721.1 | 60.0 | 16.57 | 29.51 | 0.335 | 0 | 12.07 | 726.4 |
| gpui-kit | 102.9 | 9.13 | 16.12 | 0.055 | 0 | 59.98 | 6,185 | 3.7 | 270.6 | 288.4 | 0.000 | 0 | 349.4 | 1,303 |
| gpui-kit-fast | 72.7 | 13.29 | 19.00 | 0.014 | 0 | 92.65 | 6,726 | 8.8 | 113.1 | 121.8 | 0.000 | 0 | 241.6 | 2,130 |
| freya | 894.5 | 1.06 | 1.91 | 0.021 | 0 | 3.17 | 2,837 | 153.1 | 6.45 | 7.98 | 0.000 | 0 | 17.34 | 2,652 |
| freya-056a5b4 | 855.1 | 1.08 | 2.60 | 0.037 | 0 | 3.14 | 2,676 | 150.7 | 6.53 | 8.71 | 0.004 | 0 | 17.53 | 2,642 |
| vizia | 62.5 | 15.99 | 16.48 | 0.000 | 0 | 7.15 | 448.2 | 62.5 | 15.43 | 18.48 | 0.000 | 0 | 20.61 | 1,285 |
| floem | 59.9 | 16.69 | 17.03 | 0.000 | 0 | 1.34 | 80.3 | *~13.0* (failed cell) | *~73.00* (failed cell) | *~91.00* (failed cell) | - | - | *~4.60* (failed cell) | *~84.0* (failed cell) |
| iced | 139.4 | 6.47 | 12.94 | 0.083 | 0 | 39.65 | 5,527 | 22.9 | 42.83 | 52.31 | 0.000 | 0 | 78.85 | 1,805 |
| egui | 1,248 | 0.75 | 1.82 | 0.025 | 0 | 3.02 | 3,769 | 17.9 | 55.75 | 60.83 | 0.000 | 0 | 70.66 | 1,269 |
| dioxus | 85.6 | 11.40 | 14.95 | 0.000 | 0 | 53.62 | 4,592 | 33.3 | 29.53 | 35.96 | 0.000 | 0 | 77.15 | 2,556 |
| dioxus-skia | 380.0 | 2.52 | 5.84 | 0.073 | 0 | 6.83 | 2,598 | 59.0 | 16.78 | 20.50 | 0.000 | 0 | 28.74 | 1,700 |
| bough | 62.5 | 16.22 | 16.27 | 0.000 | 0 | 0.09 | 5.6 | 62.5 | 16.28 | 16.44 | 0.000 | 0 | 1.15 | 71.9 |

## Latency split (input to commit, wait, present)

| framework | probe | total p50 | input to commit | commit to ready | ready to latch | latch to present |
|---|---|---:|---:|---:|---:|---:|
| zgui | rows 3,000 hover | 7.25 | 7.27 | 0.00 | 0.05 | -0.06 |
| gpui-kit | rows 3,000 hover | 117.1 | 117.1 | 0.00 | 0.04 | -0.05 |
| gpui-kit-fast | rows 3,000 hover | 55.84 | 55.86 | 0.00 | 0.04 | -0.05 |
| freya | rows 3,000 hover | 6.17 | 6.18 | 0.00 | 0.04 | -0.05 |
| freya-056a5b4 | rows 3,000 hover | 5.43 | 5.45 | 0.00 | 0.04 | -0.05 |
| vizia | rows 3,000 hover | 96.72 | 96.73 | 0.00 | 0.04 | -0.05 |
| iced | rows 3,000 hover | 5.35 | 5.36 | 0.00 | 0.03 | -0.05 |
| egui | rows 3,000 hover | 19.13 | 19.15 | 0.00 | 0.04 | -0.05 |
| dioxus | rows 3,000 hover | 16.89 | 16.90 | 0.00 | 0.04 | -0.05 |
| dioxus-skia | rows 3,000 hover | 7.29 | 7.30 | 0.00 | 0.04 | -0.05 |
| bough | rows 3,000 hover | 0.99 | 1.00 | 0.00 | 0.03 | -0.04 |
| zgui | table fine update10 | 11.55 | 11.56 | 0.00 | 0.04 | -0.05 |
| gpui-kit | table fine update10 | 34.05 | 34.07 | 0.00 | 0.04 | -0.05 |
| gpui-kit-fast | table fine update10 | 41.13 | 41.15 | 0.00 | 0.04 | -0.06 |
| freya | table fine update10 | 7.99 | 8.00 | 0.00 | 0.04 | -0.05 |
| freya-056a5b4 | table fine update10 | 8.38 | 8.40 | 0.00 | 0.04 | -0.06 |
| vizia | table fine update10 | 31.63 | 31.64 | 0.00 | 0.04 | -0.05 |
| iced | table fine update10 | 7.16 | 7.18 | 0.00 | 0.04 | -0.05 |
| egui | table fine update10 | 11.51 | 11.52 | 0.00 | 0.04 | -0.05 |
| dioxus | table fine update10 | 21.71 | 21.72 | 0.00 | 0.04 | -0.05 |
| dioxus-skia | table fine update10 | 7.40 | 7.41 | 0.00 | 0.03 | -0.05 |
| bough | table fine update10 | 0.37 | 0.38 | 0.00 | 0.03 | -0.04 |
| zgui | table fine remove | 32.74 | 32.75 | 0.00 | 0.05 | -0.06 |
| gpui-kit | table fine remove | 51.98 | 51.99 | 0.00 | 0.04 | -0.06 |
| gpui-kit-fast | table fine remove | 44.12 | 44.14 | 0.00 | 0.03 | -0.04 |
| freya | table fine remove | 8.14 | 8.15 | 0.00 | 0.04 | -0.05 |
| freya-056a5b4 | table fine remove | 7.75 | 7.77 | 0.00 | 0.04 | -0.05 |
| vizia | table fine remove | 430.8 | 430.8 | 0.00 | 0.04 | -0.05 |
| iced | table fine remove | 11.47 | 11.48 | 0.00 | 0.04 | -0.05 |
| egui | table fine remove | 9.65 | 9.66 | 0.00 | 0.03 | -0.04 |
| dioxus | table fine remove | 25.06 | 25.07 | 0.00 | 0.04 | -0.05 |
| dioxus-skia | table fine remove | 9.11 | 9.12 | 0.00 | 0.03 | -0.05 |
| bough | table fine remove | 3.73 | 3.74 | 0.00 | 0.03 | -0.04 |
| zgui | table fine swap | 30.72 | 30.73 | 0.00 | 0.05 | -0.06 |
| gpui-kit | table fine swap | 30.16 | 30.17 | 0.00 | 0.04 | -0.05 |
| gpui-kit-fast | table fine swap | 36.82 | 36.84 | 0.00 | 0.04 | -0.06 |
| freya | table fine swap | 8.13 | 8.14 | 0.00 | 0.04 | -0.05 |
| freya-056a5b4 | table fine swap | 9.17 | 9.18 | 0.00 | 0.04 | -0.05 |
| vizia | table fine swap | 279.7 | 279.8 | 0.00 | 0.04 | -0.05 |
| iced | table fine swap | 5.95 | 5.96 | 0.00 | 0.04 | -0.05 |
| egui | table fine swap | 9.39 | 9.40 | 0.00 | 0.04 | -0.05 |
| dioxus | table fine swap | 23.03 | 23.04 | 0.00 | 0.04 | -0.05 |
| dioxus-skia | table fine swap | 9.72 | 9.74 | 0.00 | 0.04 | -0.05 |
| bough | table fine swap | 0.52 | 0.53 | 0.00 | 0.03 | -0.04 |
| zgui | search filter | 71.60 | 71.61 | 0.00 | 0.05 | -0.06 |
| gpui-kit | search filter | 36.32 | 36.33 | 0.00 | 0.04 | -0.05 |
| gpui-kit-fast | search filter | 40.13 | 40.14 | 0.00 | 0.04 | -0.05 |
| freya | search filter | 39.27 | 39.29 | 0.00 | 0.04 | -0.06 |
| freya-056a5b4 | search filter | 37.51 | 37.53 | 0.00 | 0.04 | -0.05 |
| vizia | search filter | 119.7 | 119.7 | 0.00 | 0.04 | -0.05 |
| iced | search filter | 9.86 | 9.87 | 0.00 | 0.03 | -0.04 |
| egui | search filter | 12.93 | 12.94 | 0.00 | 0.03 | -0.05 |
| dioxus | search filter | 48.65 | 48.66 | 0.00 | 0.04 | -0.05 |
| dioxus-skia | search filter | 23.93 | 23.94 | 0.00 | 0.03 | -0.04 |
| bough | search filter | 6.99 | 7.00 | 0.00 | 0.03 | -0.04 |

# VIEW Fixed 60 Hz grid (cmp-2, heavy-1; timing fixed 60 Hz)

## Where bough wins and loses, headline cells

Lowest valid value in 35 metrics, behind another framework in 2.

Behind (valid measurements only):

- resize configure to present p50 (ms): bough 17.22, best valid iced 14.41
- resize configure to commit p50 (app share) (ms): bough 9.14, best valid iced 9.07

## Where bough wins and loses, heavy scenes (four measured frameworks)

Lowest valid value in 9 metrics, behind another framework in 3.

Behind (valid measurements only):

- sheet 1,000 rows resize configure to present p50 (ms): bough 20.4, best valid egui 16.2
- sheet 10,000 rows edit total p50 (ms): bough 26.32, best valid egui 14.95
- sheet 10,000 rows idle RSS (MiB): bough 332.6, best valid egui 156.9

## Headline table (the cmp-1 columns)

| framework | rows 3,000 hover total p50 (ms) | rows 3,000 hover input to commit p50 (ms) | rows 3,000 startup to first present (ms) | rows 3,000 idle RSS (MiB) | rows 3,000 idle CPU (ms/s) | rows 3,000 scroll CPU per frame (ms) | typing p50 (ms) | resize configure to present p50 (ms) | spinner 10,000 CPU per frame (ms) | table fine remove total p50 (ms) | table fine remove CPU per frame (ms) |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| zgui | 16.36 | 5.37 | - | 316.7 | 136.1 | 31.33 | 13.71 | 45.77 † | 19.09 | 31.36 † | 287.3 † |
| gpui-kit | 98.46 | 89.38 | - | 251.1 | 165.9 | 169.6 † | 54.23 † | 108.9 † | 438.2 † | 49.26 † | 193.7 † |
| gpui-kit-fast | 57.59 † | 46.40 † | - | 269.6 | 154.2 | 201.8 † | 45.74 † | 91.30 † | 328.7 † | 47.28 | 296.7 |
| freya | 15.12 | 4.47 | - | 250.0 | 14.98 | 27.46 | 18.00 † | 60.04 † | 24.93 | 14.69 | 25.65 |
| freya-056a5b4 | 15.12 | 4.49 | - | 250.8 | 14.71 | 25.66 | 19.41 † | 68.05 † | 25.88 † | 14.69 | 26.11 |
| vizia | 89.34 | 79.78 | - | 273.2 | 27.26 | 166.4 | 56.10 † | 67.10 † | 26.26 | 343.9 | 270.7 |
| floem | *~13.00* (failed cell) | *~3.80* (failed cell) | *~320.0* | *~190.0* (failed cell) | *~3.70* (failed cell) | *~5.70* (failed cell) | *~9.00* (failed cell) | *~18.00* (failed cell) | *~5.50* (failed cell) | *~12.00* (failed cell) | *~5.60* (failed cell) |
| iced | 14.02 | 3.74 | - | 169.4 | 22.82 | 23.18 | 18.74 † | 14.41 | 133.0 † | 20.12 | 27.77 |
| egui | 28.19 | 15.59 | - | 168.8 | 28.62 | 42.29 † | 18.96 † | 22.97 | 86.33 | 14.68 | 37.21 |
| dioxus | 24.25 † | 13.85 † | - | 220.5 | 74.94 | 97.02 † | 31.97 † | 50.96 † | 114.1 † | 28.80 | 112.0 |
| dioxus-skia | 15.12 | 5.81 | - | 191.4 | 14.86 | 27.68 | 18.19 † | 33.51 † | 39.34 | 15.92 | 25.86 |
| bough | 11.52 | 0.93 | - | 35.1 | 0.03 | 5.74 | 8.44 | 17.22 | 1.13 | 14.67 | 4.18 |

## Startup, memory, idle

| framework | rows 3,000 startup to first present (ms) | virtual 100,000 startup to first present (ms) | spinner (no rows) startup to first present (ms) | rows 3,000 idle RSS (MiB) | virtual 100,000 idle RSS (MiB) | spinner (no rows) RSS (MiB) | rows 3,000 idle CPU (ms/s) | spinner (no rows) idle CPU (ms/s) |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| zgui | - | - | - | 316.7 | 176.8 | 179.1 | 136.1 | 784.1 |
| gpui-kit | - | - | - | 251.1 | 178.8 | 180.6 | 165.9 | 3,059 |
| gpui-kit-fast | - | - | - | 269.6 | 200.8 | 201.1 | 154.2 | 4,247 |
| freya | - | - | - | 250.0 | 140.7 | 139.4 | 14.98 | 336.9 |
| freya-056a5b4 | - | - | - | 250.8 | 140.5 | 139.5 | 14.71 | 347.0 |
| vizia | - | - | - | 273.2 | 150.0 | 150.8 | 27.26 | 769.5 |
| floem | *~320.0* | *~480.0* | - | *~190.0* (failed cell) | *~130.0* (failed cell) | 127.7 | *~3.70* (failed cell) | 123.6 |
| iced | - | - | - | 169.4 | 143.1 | 167.7 | 22.82 | 2,112 |
| egui | - | - | - | 168.8 | 143.8 | 143.2 | 28.62 | 323.6 |
| dioxus | - | - | - | 220.5 | 176.8 | 174.9 | 74.94 | 3,284 |
| dioxus-skia | - | - | - | 191.4 | 147.7 | 159.5 | 14.86 | 309.6 |
| bough | - | - | - | 35.1 | 18.7 | 19.3 | 0.03 | 7.43 |

## Hover (rows)

| framework | rows 3,000 hover total p50 (ms) | rows 3,000 hover input to commit p50 (ms) | rows 100 hover total p50 (ms) | rows 100 hover input to commit p50 (ms) | rows 3,000 hover CPU per frame (ms) | rows 100 hover CPU per frame (ms) |
|---|---:|---:|---:|---:|---:|---:|
| zgui | 16.36 | 5.37 | 14.03 | 3.99 | 40.45 | 38.84 |
| gpui-kit | 98.46 | 89.38 | 20.01 † | 11.95 † | 231.4 | 139.5 † |
| gpui-kit-fast | 57.59 † | 46.40 † | 24.26 † | 13.93 † | 223.9 † | 182.6 † |
| freya | 15.12 | 4.47 | 14.01 | 3.27 | 21.92 | 23.35 |
| freya-056a5b4 | 15.12 | 4.49 | 14.02 | 3.29 | 22.62 | 21.50 |
| vizia | 89.34 | 79.78 | 15.13 | 4.65 | 106.5 | 18.06 |
| floem | *~13.00* (failed cell) | *~3.80* (failed cell) | *~12.00* (failed cell) | *~2.60* (failed cell) | *~4.90* (failed cell) | *~4.40* (failed cell) |
| iced | 14.02 | 3.74 | 13.32 | 2.13 | 25.14 | 20.94 |
| egui | 28.19 | 15.59 | 14.03 | 3.19 | 42.06 | 30.57 |
| dioxus | 24.25 † | 13.85 † | 17.82 | 11.55 | 92.11 † | 91.08 |
| dioxus-skia | 15.12 | 5.81 | 14.03 | 2.91 | 23.77 | 18.51 |
| bough | 11.52 | 0.93 | 11.54 | 0.12 | 0.99 | 0.11 |

## Scrolling

| framework | rows 3,000 scroll CPU per frame (ms) | rows 3,000 scroll fps (fps) | virtual 100,000 scroll CPU per frame (ms) |
|---|---:|---:|---:|
| zgui | 31.33 | 60.0 | 32.35 † |
| gpui-kit | 169.6 † | 10.0 † | 122.5 † |
| gpui-kit-fast | 201.8 † | 20.0 † | 176.7 † |
| freya | 27.46 | 60.0 | 23.23 † |
| freya-056a5b4 | 25.66 | 60.0 | 22.94 † |
| vizia | 166.4 | 6.5 | 34.94 † |
| floem | *~5.70* (failed cell) | *~60.0* (failed cell) | *~20.00* (failed cell) |
| iced | 23.18 | 60.0 | 22.07 † |
| egui | 42.29 † | 38.0 † | 31.57 † |
| dioxus | 97.02 † | 60.0 † | 97.27 † |
| dioxus-skia | 27.68 | 60.0 | 24.90 † |
| bough | 5.74 | 60.0 | 4.13 |

## Typing, search, resize

| framework | typing p50 (ms) | typing handling p50 (app share) (ms) | typing CPU per frame (ms) | search filter total p50 (ms) | search filter input to commit p50 (ms) | search filter CPU per frame (ms) | resize configure to present p50 (ms) | resize configure to commit p50 (app share) (ms) | resize CPU per frame (ms) |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| zgui | 13.71 | 3.16 | 21.15 | 85.96 † | 75.79 † | 410.2 † | 45.77 † | 43.46 † | 92.88 † |
| gpui-kit | 54.23 † | 45.57 † | 190.3 † | 58.36 † | 44.79 † | 161.9 † | 108.9 † | 95.02 † | 202.4 † |
| gpui-kit-fast | 45.74 † | 37.77 † | 255.8 † | 57.57 † | 49.04 † | 212.3 † | 91.30 † | 82.35 † | 212.5 † |
| freya | 18.00 † | 7.86 † | 26.45 † | 54.20 | 42.39 | 64.20 | 60.04 † | 50.33 † | 44.95 † |
| freya-056a5b4 | 19.41 † | 10.33 † | 26.46 † | 55.72 | 46.61 | 81.34 | 68.05 † | 51.99 † | 44.21 † |
| vizia | 56.10 † | 46.72 † | 69.81 † | 148.0 | 139.8 | 29.17 | 67.10 † | 61.51 † | 102.9 † |
| floem | *~9.00* (failed cell) | *~0.97* (failed cell) | *~2.40* (failed cell) | *~21.00* (failed cell) | *~20.00* (failed cell) | *~14.00* (failed cell) | *~18.00* (failed cell) | *~9.40* (failed cell) | *~7.80* (failed cell) |
| iced | 18.74 † | 8.52 † | 25.31 † | 23.45 | 12.90 | 18.70 | 14.41 | 9.07 | 37.01 |
| egui | 18.96 † | 12.56 † | 37.30 † | 25.79 † | 15.24 † | 18.04 † | 22.97 | 14.44 | 38.11 |
| dioxus | 31.97 † | 22.33 † | 104.9 † | 71.61 † | 61.79 † | 111.3 † | 50.96 † | 39.91 † | 77.30 † |
| dioxus-skia | 18.19 † | 8.96 † | 25.38 † | 40.18 † | 28.32 † | 23.76 † | 33.51 † | 20.68 † | 22.52 † |
| bough | 8.44 | 0.43 | 0.47 | 15.93 | 6.38 | 5.01 | 17.22 | 9.14 | 8.75 |

## Table operations (1,000 rows, per-row state; egui has its one variant)

| framework | table fine remove total p50 (ms) | table fine remove input to commit p50 (ms) | table fine remove CPU per frame (ms) | table fine update10 total p50 (ms) | table fine update10 input to commit p50 (ms) | table fine update10 CPU per frame (ms) | table fine swap total p50 (ms) | table fine swap input to commit p50 (ms) | table fine swap CPU per frame (ms) |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| zgui | 31.36 † | 25.02 † | 287.3 † | 15.95 | 8.72 | 63.70 | 31.36 † | 25.45 † | 293.9 † |
| gpui-kit | 49.26 † | 38.93 † | 193.7 † | 31.36 † | 26.15 † | 206.1 † | 31.37 | 20.80 | 206.1 |
| gpui-kit-fast | 47.28 | 34.83 | 296.7 | 34.43 † | 26.93 † | 276.1 † | 32.59 † | 24.95 † | 239.0 † |
| freya | 14.69 | 6.86 | 25.65 | 14.69 | 6.50 | 25.38 | 14.69 | 7.13 | 25.98 |
| freya-056a5b4 | 14.69 | 6.74 | 26.11 | 14.70 | 6.65 | 25.63 | 15.93 | 7.32 | 25.50 |
| vizia | 343.9 | 332.6 | 270.7 | 32.60 | 27.23 | 55.65 | 241.6 | 231.6 | 359.4 |
| floem | *~12.00* (failed cell) | *~7.50* (failed cell) | *~5.60* (failed cell) | *~12.00* (failed cell) | *~5.50* (failed cell) | *~5.50* (failed cell) | *~13.00* (failed cell) | *~6.10* (failed cell) | *~5.50* (failed cell) |
| iced | 20.12 | 9.56 | 27.77 | 14.68 | 5.60 | 24.94 | 14.68 | 4.84 | 22.86 |
| egui | 14.68 | 7.64 | 37.21 | 15.93 | 8.73 | 36.16 | 15.93 | 7.64 | 36.02 |
| dioxus | 28.80 | 18.28 | 112.0 | 28.80 † | 16.38 † | 99.43 † | 30.60 | 18.36 | 114.5 |
| dioxus-skia | 15.92 | 8.94 | 25.86 | 14.69 | 6.54 | 25.31 | 15.92 | 8.88 | 28.25 |
| bough | 14.67 | 4.22 | 4.18 | 12.10 | 0.44 | 0.45 | 12.10 | 0.59 | 0.59 |

## Ticker (100 row updates per tick)

| framework | ticker fine 100 updates CPU per frame (ms) | ticker fine 100 updates fps (fps) |
|---|---:|---:|
| zgui | 37.72 | 58.8 |
| gpui-kit | 175.6 † | 28.4 † |
| gpui-kit-fast | 244.8 † | 26.0 † |
| freya | 26.81 | 60.0 |
| freya-056a5b4 | 27.11 | 60.0 |
| vizia | 56.83 | 30.8 |
| floem | *~5.50* (failed cell) | *~60.0* (failed cell) |
| iced | 23.56 | 60.0 |
| egui | 37.48 | 60.0 |
| dioxus | 104.7 † | 55.6 † |
| dioxus-skia | 26.08 | 57.8 |
| bough | 0.19 | 60.0 |

## Spinner

| framework | spinner (no rows) CPU per frame (ms) | spinner 10,000 CPU per frame (ms) | spinner (no rows) fps (fps) | spinner 10,000 fps (fps) |
|---|---:|---:|---:|---:|
| zgui | 19.23 | 19.09 | 59.8 | 60.0 |
| gpui-kit | 103.2 | 438.2 † | 57.6 | 3.4 † |
| gpui-kit-fast | 150.0 † | 328.7 † | 56.1 † | 8.3 † |
| freya | 5.53 | 24.93 | 60.0 | 60.0 |
| freya-056a5b4 | 5.63 | 25.88 † | 60.0 | 59.0 † |
| vizia | 11.96 | 26.26 | 60.0 | 57.2 |
| floem | 2.07 | *~5.50* (failed cell) | 59.7 | *~60.0* (failed cell) |
| iced | 53.49 | 133.0 † | 59.8 | 18.9 † |
| egui | 5.23 | 86.33 | 60.0 | 14.9 |
| dioxus | 86.48 | 114.1 † | 60.0 | 37.1 † |
| dioxus-skia | 10.12 | 39.34 | 60.0 | 53.0 |
| bough | 0.17 | 1.13 | 60.0 | 60.0 |

## Estimates, with method per value

| column | framework | estimate | range | basis |
|---|---|---:|---|---|
| rows 3,000 hover total p50 (ms) | floem | ~13.00 | 7.00 to 23.00 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 9 frameworks |
| rows 3,000 hover input to commit p50 (ms) | floem | ~3.80 | 2.10 to 6.80 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 9 frameworks |
| rows 100 hover total p50 (ms) | floem | ~12.00 | 6.40 to 21.00 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 9 frameworks |
| rows 3,000 idle RSS (MiB) | floem | ~190.0 | 140.0 to 260.0 | own: 2 own valid cells in class 'resident memory (MiB)' (log-level -0.16 against the column medians); column level from 11 frameworks |
| rows 3,000 idle CPU (ms/s) | floem | ~3.70 | 2.10 to 6.50 | own: 2 own valid cells in class 'idle CPU (ms/s)' (log-level -1.85 against the column medians); column level from 11 frameworks |
| rows 3,000 scroll CPU per frame (ms) | floem | ~5.70 | 4.20 to 7.60 | own: 2 own valid cells in class 'cpu per frame (ms)' (log-level -1.59 against the column medians); column level from 7 frameworks |
| rows 3,000 scroll fps (fps) | floem | ~60.0 | 44.0 to 60.0 | own: 2 own valid cells in class 'frame rate (fps)' (log-level -0.01 against the column medians); column level from 7 frameworks |
| virtual 100,000 scroll CPU per frame (ms) | floem | ~20.00 | 15.00 to 27.00 | own: 2 own valid cells in class 'cpu per frame (ms)' (log-level -1.59 against the column medians); column level from 1 frameworks |
| virtual 100,000 idle RSS (MiB) | floem | ~130.0 | 95.0 to 170.0 | own: 2 own valid cells in class 'resident memory (MiB)' (log-level -0.16 against the column medians); column level from 11 frameworks |
| typing p50 (ms) | floem | ~9.00 | 4.90 to 16.00 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 2 frameworks |
| search filter total p50 (ms) | floem | ~21.00 | 12.00 to 38.00 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 5 frameworks |
| resize configure to present p50 (ms) | floem | ~18.00 | 9.70 to 32.00 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 3 frameworks |
| spinner 10,000 CPU per frame (ms) | floem | ~5.50 | 4.10 to 7.40 | own: 2 own valid cells in class 'cpu per frame (ms)' (log-level -1.59 against the column medians); column level from 6 frameworks |
| spinner 10,000 fps (fps) | floem | ~60.0 | 44.0 to 60.0 | own: 2 own valid cells in class 'frame rate (fps)' (log-level -0.01 against the column medians); column level from 6 frameworks |
| table fine remove total p50 (ms) | floem | ~12.00 | 6.80 to 22.00 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 9 frameworks |
| table fine remove CPU per frame (ms) | floem | ~5.60 | 4.10 to 7.50 | own: 2 own valid cells in class 'cpu per frame (ms)' (log-level -1.59 against the column medians); column level from 9 frameworks |
| table fine update10 total p50 (ms) | floem | ~12.00 | 6.80 to 22.00 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 8 frameworks |
| table fine swap total p50 (ms) | floem | ~13.00 | 7.20 to 24.00 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 9 frameworks |
| ticker fine 100 updates CPU per frame (ms) | floem | ~5.50 | 4.10 to 7.40 | own: 2 own valid cells in class 'cpu per frame (ms)' (log-level -1.59 against the column medians); column level from 8 frameworks |
| ticker fine 100 updates fps (fps) | floem | ~60.0 | 44.0 to 60.0 | own: 2 own valid cells in class 'frame rate (fps)' (log-level -0.01 against the column medians); column level from 8 frameworks |
| rows 100 hover input to commit p50 (ms) | floem | ~2.60 | 1.40 to 4.80 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 9 frameworks |
| table fine remove input to commit p50 (ms) | floem | ~7.50 | 4.10 to 14.00 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 9 frameworks |
| table fine update10 input to commit p50 (ms) | floem | ~5.50 | 3.00 to 10.00 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 8 frameworks |
| table fine swap input to commit p50 (ms) | floem | ~6.10 | 3.40 to 11.00 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 9 frameworks |
| search filter input to commit p50 (ms) | floem | ~20.00 | 11.00 to 36.00 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 5 frameworks |
| typing handling p50 (app share) (ms) | floem | ~0.97 | 0.53 to 1.80 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 2 frameworks |
| resize configure to commit p50 (app share) (ms) | floem | ~9.40 | 5.20 to 17.00 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 3 frameworks |
| rows 3,000 hover CPU per frame (ms) | floem | ~4.90 | 3.60 to 6.60 | own: 2 own valid cells in class 'cpu per frame (ms)' (log-level -1.59 against the column medians); column level from 9 frameworks |
| rows 100 hover CPU per frame (ms) | floem | ~4.40 | 3.30 to 5.90 | own: 2 own valid cells in class 'cpu per frame (ms)' (log-level -1.59 against the column medians); column level from 9 frameworks |
| table fine update10 CPU per frame (ms) | floem | ~5.50 | 4.00 to 7.40 | own: 2 own valid cells in class 'cpu per frame (ms)' (log-level -1.59 against the column medians); column level from 8 frameworks |
| table fine swap CPU per frame (ms) | floem | ~5.50 | 4.00 to 7.40 | own: 2 own valid cells in class 'cpu per frame (ms)' (log-level -1.59 against the column medians); column level from 9 frameworks |
| typing CPU per frame (ms) | floem | ~2.40 | 1.80 to 3.20 | own: 2 own valid cells in class 'cpu per frame (ms)' (log-level -1.59 against the column medians); column level from 2 frameworks |
| search filter CPU per frame (ms) | floem | ~14.00 | 10.00 to 19.00 | own: 2 own valid cells in class 'cpu per frame (ms)' (log-level -1.59 against the column medians); column level from 5 frameworks |
| resize CPU per frame (ms) | floem | ~7.80 | 5.80 to 11.00 | own: 2 own valid cells in class 'cpu per frame (ms)' (log-level -1.59 against the column medians); column level from 3 frameworks |
| rows 3,000 startup to first present (ms) | floem | ~320.0 | 170.0 to 580.0 | cross-class: only 1 own valid cell(s) in class 'startup (ms)': level taken across all classes (cross-class, low confidence); column level from 11 frameworks |
| virtual 100,000 startup to first present (ms) | floem | ~480.0 | 260.0 to 870.0 | cross-class: only 1 own valid cell(s) in class 'startup (ms)': level taken across all classes (cross-class, low confidence); column level from 11 frameworks |
| sheet 1,000 rows hover total p50 (ms) | dioxus | ~33.00 | 23.00 to 47.00 | own: 6 own valid cells in class 'latency (ms)' (log-level +0.71 against the column medians); column level from 1 frameworks |
| sheet 1,000 rows hover total p50 (ms) | dioxus-skia | ~16.00 | 12.00 to 22.00 | own: 10 own valid cells in class 'latency (ms)' (log-level +0.01 against the column medians); column level from 1 frameworks |
| sheet 1,000 rows hover total p50 (ms) | floem | ~14.00 | 7.50 to 25.00 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 1 frameworks |
| sheet 1,000 rows hover total p50 (ms) | freya | ~16.00 | 12.00 to 22.00 | own: 12 own valid cells in class 'latency (ms)' (log-level +0.01 against the column medians); column level from 1 frameworks |
| sheet 1,000 rows hover total p50 (ms) | freya-056a5b4 | ~16.00 | 12.00 to 22.00 | own: 12 own valid cells in class 'latency (ms)' (log-level +0.01 against the column medians); column level from 1 frameworks |
| sheet 1,000 rows hover total p50 (ms) | gpui-kit-fast | ~58.00 | 43.00 to 78.00 | own: 2 own valid cells in class 'latency (ms)' (log-level +1.27 against the column medians); column level from 1 frameworks |
| sheet 1,000 rows hover total p50 (ms) | vizia | ~96.00 | 4.40 to 2,100 | own: 12 own valid cells in class 'latency (ms)' (log-level +1.79 against the column medians); column level from 1 frameworks |
| sheet 1,000 rows hover total p50 (ms) | zgui | ~20.00 | 15.00 to 27.00 | own: 8 own valid cells in class 'latency (ms)' (log-level +0.22 against the column medians); column level from 1 frameworks |
| sheet 1,000 rows hover input to commit p50 (ms) | dioxus | ~6.00 | 4.20 to 8.70 | own: 6 own valid cells in class 'latency (ms)' (log-level +0.71 against the column medians); column level from 1 frameworks |
| sheet 1,000 rows hover input to commit p50 (ms) | dioxus-skia | ~3.00 | 2.20 to 4.10 | own: 10 own valid cells in class 'latency (ms)' (log-level +0.01 against the column medians); column level from 1 frameworks |
| sheet 1,000 rows hover input to commit p50 (ms) | floem | ~2.50 | 1.40 to 4.60 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 1 frameworks |
| sheet 1,000 rows hover input to commit p50 (ms) | freya | ~3.00 | 2.20 to 4.00 | own: 12 own valid cells in class 'latency (ms)' (log-level +0.01 against the column medians); column level from 1 frameworks |
| sheet 1,000 rows hover input to commit p50 (ms) | freya-056a5b4 | ~3.00 | 2.20 to 4.00 | own: 12 own valid cells in class 'latency (ms)' (log-level +0.01 against the column medians); column level from 1 frameworks |
| sheet 1,000 rows hover input to commit p50 (ms) | gpui-kit-fast | ~11.00 | 7.90 to 14.00 | own: 2 own valid cells in class 'latency (ms)' (log-level +1.27 against the column medians); column level from 1 frameworks |
| sheet 1,000 rows hover input to commit p50 (ms) | vizia | ~18.00 | 0.81 to 390.0 | own: 12 own valid cells in class 'latency (ms)' (log-level +1.79 against the column medians); column level from 1 frameworks |
| sheet 1,000 rows hover input to commit p50 (ms) | zgui | ~3.70 | 2.70 to 5.10 | own: 8 own valid cells in class 'latency (ms)' (log-level +0.22 against the column medians); column level from 1 frameworks |
| sheet 1,000 rows theme switch total p50 (ms) | dioxus | ~29.00 | 20.00 to 42.00 | own: 6 own valid cells in class 'latency (ms)' (log-level +0.71 against the column medians); column level from 1 frameworks |
| sheet 1,000 rows theme switch total p50 (ms) | dioxus-skia | ~15.00 | 11.00 to 20.00 | own: 10 own valid cells in class 'latency (ms)' (log-level +0.01 against the column medians); column level from 1 frameworks |
| sheet 1,000 rows theme switch total p50 (ms) | floem | ~12.00 | 6.70 to 22.00 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 1 frameworks |
| sheet 1,000 rows theme switch total p50 (ms) | freya | ~15.00 | 11.00 to 20.00 | own: 12 own valid cells in class 'latency (ms)' (log-level +0.01 against the column medians); column level from 1 frameworks |
| sheet 1,000 rows theme switch total p50 (ms) | freya-056a5b4 | ~15.00 | 11.00 to 20.00 | own: 12 own valid cells in class 'latency (ms)' (log-level +0.01 against the column medians); column level from 1 frameworks |
| sheet 1,000 rows theme switch total p50 (ms) | gpui-kit-fast | ~52.00 | 38.00 to 70.00 | own: 2 own valid cells in class 'latency (ms)' (log-level +1.27 against the column medians); column level from 1 frameworks |
| sheet 1,000 rows theme switch total p50 (ms) | vizia | ~86.00 | 3.90 to 1,900 | own: 12 own valid cells in class 'latency (ms)' (log-level +1.79 against the column medians); column level from 1 frameworks |
| sheet 1,000 rows theme switch total p50 (ms) | zgui | ~18.00 | 13.00 to 25.00 | own: 8 own valid cells in class 'latency (ms)' (log-level +0.22 against the column medians); column level from 1 frameworks |
| sheet 1,000 rows edit total p50 (ms) | dioxus | ~31.00 | 21.00 to 45.00 | own: 6 own valid cells in class 'latency (ms)' (log-level +0.71 against the column medians); column level from 1 frameworks |
| sheet 1,000 rows edit total p50 (ms) | dioxus-skia | ~16.00 | 12.00 to 21.00 | own: 10 own valid cells in class 'latency (ms)' (log-level +0.01 against the column medians); column level from 1 frameworks |
| sheet 1,000 rows edit total p50 (ms) | floem | ~13.00 | 7.10 to 24.00 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 1 frameworks |
| sheet 1,000 rows edit total p50 (ms) | freya | ~15.00 | 11.00 to 21.00 | own: 12 own valid cells in class 'latency (ms)' (log-level +0.01 against the column medians); column level from 1 frameworks |
| sheet 1,000 rows edit total p50 (ms) | freya-056a5b4 | ~16.00 | 11.00 to 21.00 | own: 12 own valid cells in class 'latency (ms)' (log-level +0.01 against the column medians); column level from 1 frameworks |
| sheet 1,000 rows edit total p50 (ms) | gpui-kit-fast | ~55.00 | 41.00 to 74.00 | own: 2 own valid cells in class 'latency (ms)' (log-level +1.27 against the column medians); column level from 1 frameworks |
| sheet 1,000 rows edit total p50 (ms) | vizia | ~92.00 | 4.20 to 2,000 | own: 12 own valid cells in class 'latency (ms)' (log-level +1.79 against the column medians); column level from 1 frameworks |
| sheet 1,000 rows edit total p50 (ms) | zgui | ~19.00 | 14.00 to 26.00 | own: 8 own valid cells in class 'latency (ms)' (log-level +0.22 against the column medians); column level from 1 frameworks |
| sheet 1,000 rows resize configure to present p50 (ms) | dioxus | ~50.0 | 35.0 to 73.0 | own: 6 own valid cells in class 'latency (ms)' (log-level +0.71 against the column medians); column level from 3 frameworks |
| sheet 1,000 rows resize configure to present p50 (ms) | dioxus-skia | ~25.0 | 19.0 to 34.0 | own: 10 own valid cells in class 'latency (ms)' (log-level +0.01 against the column medians); column level from 3 frameworks |
| sheet 1,000 rows resize configure to present p50 (ms) | floem | ~21.0 | 12.0 to 38.0 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 3 frameworks |
| sheet 1,000 rows resize configure to present p50 (ms) | freya | ~25.0 | 19.0 to 34.0 | own: 12 own valid cells in class 'latency (ms)' (log-level +0.01 against the column medians); column level from 3 frameworks |
| sheet 1,000 rows resize configure to present p50 (ms) | freya-056a5b4 | ~25.0 | 19.0 to 34.0 | own: 12 own valid cells in class 'latency (ms)' (log-level +0.01 against the column medians); column level from 3 frameworks |
| sheet 1,000 rows resize configure to present p50 (ms) | gpui-kit-fast | ~89.0 | 66.0 to 120.0 | own: 2 own valid cells in class 'latency (ms)' (log-level +1.27 against the column medians); column level from 3 frameworks |
| sheet 1,000 rows resize configure to present p50 (ms) | vizia | ~150.0 | 6.7 to 3,200 | own: 12 own valid cells in class 'latency (ms)' (log-level +1.79 against the column medians); column level from 3 frameworks |
| sheet 1,000 rows resize configure to present p50 (ms) | zgui | ~31.0 | 23.0 to 42.0 | own: 8 own valid cells in class 'latency (ms)' (log-level +0.22 against the column medians); column level from 3 frameworks |
| sheet 1,000 rows idle RSS (MiB) | dioxus | ~210.0 | 150.0 to 280.0 | own: 4 own valid cells in class 'resident memory (MiB)' (log-level +0.15 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows idle RSS (MiB) | dioxus-skia | ~180.0 | 130.0 to 240.0 | own: 5 own valid cells in class 'resident memory (MiB)' (log-level -0.02 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows idle RSS (MiB) | floem | ~150.0 | 110.0 to 210.0 | own: 2 own valid cells in class 'resident memory (MiB)' (log-level -0.16 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows idle RSS (MiB) | freya | ~170.0 | 120.0 to 220.0 | own: 5 own valid cells in class 'resident memory (MiB)' (log-level -0.08 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows idle RSS (MiB) | freya-056a5b4 | ~170.0 | 120.0 to 220.0 | own: 4 own valid cells in class 'resident memory (MiB)' (log-level -0.07 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows idle RSS (MiB) | gpui-kit-fast | ~240.0 | 180.0 to 320.0 | own: 3 own valid cells in class 'resident memory (MiB)' (log-level +0.29 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows idle RSS (MiB) | vizia | ~180.0 | 130.0 to 240.0 | own: 5 own valid cells in class 'resident memory (MiB)' (log-level -0.00 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows idle RSS (MiB) | zgui | ~210.0 | 160.0 to 290.0 | own: 5 own valid cells in class 'resident memory (MiB)' (log-level +0.17 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows idle CPU (ms/s) | dioxus | ~72.00 | 53.00 to 97.00 | own: 3 own valid cells in class 'idle CPU (ms/s)' (log-level +1.62 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows idle CPU (ms/s) | dioxus-skia | ~9.00 | 5.80 to 14.00 | own: 4 own valid cells in class 'idle CPU (ms/s)' (log-level -0.46 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows idle CPU (ms/s) | floem | ~2.20 | 1.30 to 3.90 | own: 2 own valid cells in class 'idle CPU (ms/s)' (log-level -1.85 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows idle CPU (ms/s) | freya | ~8.10 | 4.90 to 14.00 | own: 4 own valid cells in class 'idle CPU (ms/s)' (log-level -0.55 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows idle CPU (ms/s) | freya-056a5b4 | ~7.60 | 4.80 to 12.00 | own: 3 own valid cells in class 'idle CPU (ms/s)' (log-level -0.63 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows idle CPU (ms/s) | gpui-kit-fast | ~93.00 | 69.00 to 120.0 | own: 2 own valid cells in class 'idle CPU (ms/s)' (log-level +1.88 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows idle CPU (ms/s) | vizia | ~13.00 | 6.70 to 26.00 | own: 4 own valid cells in class 'idle CPU (ms/s)' (log-level -0.07 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows idle CPU (ms/s) | zgui | ~17.00 | 5.50 to 53.00 | own: 4 own valid cells in class 'idle CPU (ms/s)' (log-level +0.19 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows hover total p50 (ms) | dioxus | ~160.0 | 110.0 to 230.0 | own: 6 own valid cells in class 'latency (ms)' (log-level +0.71 against the column medians); column level from 2 frameworks |
| sheet 10,000 rows hover total p50 (ms) | dioxus-skia | ~79.00 | 59.00 to 110.0 | own: 10 own valid cells in class 'latency (ms)' (log-level +0.01 against the column medians); column level from 2 frameworks |
| sheet 10,000 rows hover total p50 (ms) | floem | ~66.00 | 36.00 to 120.0 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 2 frameworks |
| sheet 10,000 rows hover total p50 (ms) | freya | ~79.00 | 59.00 to 110.0 | own: 12 own valid cells in class 'latency (ms)' (log-level +0.01 against the column medians); column level from 2 frameworks |
| sheet 10,000 rows hover total p50 (ms) | freya-056a5b4 | ~79.00 | 59.00 to 110.0 | own: 12 own valid cells in class 'latency (ms)' (log-level +0.01 against the column medians); column level from 2 frameworks |
| sheet 10,000 rows hover total p50 (ms) | gpui-kit-fast | ~280.0 | 210.0 to 380.0 | own: 2 own valid cells in class 'latency (ms)' (log-level +1.27 against the column medians); column level from 2 frameworks |
| sheet 10,000 rows hover total p50 (ms) | vizia | ~470.0 | 21.00 to 10,000 | own: 12 own valid cells in class 'latency (ms)' (log-level +1.79 against the column medians); column level from 2 frameworks |
| sheet 10,000 rows hover total p50 (ms) | zgui | ~98.00 | 72.00 to 130.0 | own: 8 own valid cells in class 'latency (ms)' (log-level +0.22 against the column medians); column level from 2 frameworks |
| sheet 10,000 rows hover input to commit p50 (ms) | dioxus | ~120.0 | 82.00 to 170.0 | own: 6 own valid cells in class 'latency (ms)' (log-level +0.71 against the column medians); column level from 2 frameworks |
| sheet 10,000 rows hover input to commit p50 (ms) | dioxus-skia | ~60.00 | 44.00 to 81.00 | own: 10 own valid cells in class 'latency (ms)' (log-level +0.01 against the column medians); column level from 2 frameworks |
| sheet 10,000 rows hover input to commit p50 (ms) | floem | ~50.00 | 27.00 to 91.00 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 2 frameworks |
| sheet 10,000 rows hover input to commit p50 (ms) | freya | ~59.00 | 44.00 to 80.00 | own: 12 own valid cells in class 'latency (ms)' (log-level +0.01 against the column medians); column level from 2 frameworks |
| sheet 10,000 rows hover input to commit p50 (ms) | freya-056a5b4 | ~60.00 | 44.00 to 80.00 | own: 12 own valid cells in class 'latency (ms)' (log-level +0.01 against the column medians); column level from 2 frameworks |
| sheet 10,000 rows hover input to commit p50 (ms) | gpui-kit-fast | ~210.0 | 160.0 to 280.0 | own: 2 own valid cells in class 'latency (ms)' (log-level +1.27 against the column medians); column level from 2 frameworks |
| sheet 10,000 rows hover input to commit p50 (ms) | vizia | ~350.0 | 16.00 to 7,700 | own: 12 own valid cells in class 'latency (ms)' (log-level +1.79 against the column medians); column level from 2 frameworks |
| sheet 10,000 rows hover input to commit p50 (ms) | zgui | ~74.00 | 54.00 to 100.0 | own: 8 own valid cells in class 'latency (ms)' (log-level +0.22 against the column medians); column level from 2 frameworks |
| sheet 10,000 rows theme switch total p50 (ms) | dioxus | ~380.0 | 260.0 to 550.0 | own: 6 own valid cells in class 'latency (ms)' (log-level +0.71 against the column medians); column level from 1 frameworks |
| sheet 10,000 rows theme switch total p50 (ms) | dioxus-skia | ~190.0 | 140.0 to 260.0 | own: 10 own valid cells in class 'latency (ms)' (log-level +0.01 against the column medians); column level from 1 frameworks |
| sheet 10,000 rows theme switch total p50 (ms) | floem | ~160.0 | 88.00 to 290.0 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 1 frameworks |
| sheet 10,000 rows theme switch total p50 (ms) | freya | ~190.0 | 140.0 to 260.0 | own: 12 own valid cells in class 'latency (ms)' (log-level +0.01 against the column medians); column level from 1 frameworks |
| sheet 10,000 rows theme switch total p50 (ms) | freya-056a5b4 | ~190.0 | 140.0 to 260.0 | own: 12 own valid cells in class 'latency (ms)' (log-level +0.01 against the column medians); column level from 1 frameworks |
| sheet 10,000 rows theme switch total p50 (ms) | gpui-kit-fast | ~670.0 | 500.0 to 910.0 | own: 2 own valid cells in class 'latency (ms)' (log-level +1.27 against the column medians); column level from 1 frameworks |
| sheet 10,000 rows theme switch total p50 (ms) | vizia | ~1,100 | 51.00 to 25,000 | own: 12 own valid cells in class 'latency (ms)' (log-level +1.79 against the column medians); column level from 1 frameworks |
| sheet 10,000 rows theme switch total p50 (ms) | zgui | ~240.0 | 170.0 to 320.0 | own: 8 own valid cells in class 'latency (ms)' (log-level +0.22 against the column medians); column level from 1 frameworks |
| sheet 10,000 rows edit total p50 (ms) | dioxus | ~65.00 | 45.00 to 94.00 | own: 6 own valid cells in class 'latency (ms)' (log-level +0.71 against the column medians); column level from 3 frameworks |
| sheet 10,000 rows edit total p50 (ms) | dioxus-skia | ~32.00 | 24.00 to 44.00 | own: 10 own valid cells in class 'latency (ms)' (log-level +0.01 against the column medians); column level from 3 frameworks |
| sheet 10,000 rows edit total p50 (ms) | floem | ~27.00 | 15.00 to 49.00 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 3 frameworks |
| sheet 10,000 rows edit total p50 (ms) | freya | ~32.00 | 24.00 to 43.00 | own: 12 own valid cells in class 'latency (ms)' (log-level +0.01 against the column medians); column level from 3 frameworks |
| sheet 10,000 rows edit total p50 (ms) | freya-056a5b4 | ~32.00 | 24.00 to 44.00 | own: 12 own valid cells in class 'latency (ms)' (log-level +0.01 against the column medians); column level from 3 frameworks |
| sheet 10,000 rows edit total p50 (ms) | gpui-kit-fast | ~110.0 | 85.00 to 150.0 | own: 2 own valid cells in class 'latency (ms)' (log-level +1.27 against the column medians); column level from 3 frameworks |
| sheet 10,000 rows edit total p50 (ms) | vizia | ~190.0 | 8.70 to 4,200 | own: 12 own valid cells in class 'latency (ms)' (log-level +1.79 against the column medians); column level from 3 frameworks |
| sheet 10,000 rows edit total p50 (ms) | zgui | ~40.00 | 29.00 to 54.00 | own: 8 own valid cells in class 'latency (ms)' (log-level +0.22 against the column medians); column level from 3 frameworks |
| sheet 10,000 rows resize configure to present p50 (ms) | bough | ~46.0 | 34.0 to 62.0 | own: 29 own valid cells in class 'latency (ms)' (log-level -0.19 against the column medians); column level from 2 frameworks |
| sheet 10,000 rows resize configure to present p50 (ms) | dioxus | ~110.0 | 78.0 to 160.0 | own: 6 own valid cells in class 'latency (ms)' (log-level +0.71 against the column medians); column level from 2 frameworks |
| sheet 10,000 rows resize configure to present p50 (ms) | dioxus-skia | ~57.0 | 42.0 to 76.0 | own: 10 own valid cells in class 'latency (ms)' (log-level +0.01 against the column medians); column level from 2 frameworks |
| sheet 10,000 rows resize configure to present p50 (ms) | floem | ~47.0 | 26.0 to 86.0 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 2 frameworks |
| sheet 10,000 rows resize configure to present p50 (ms) | freya | ~56.0 | 42.0 to 76.0 | own: 12 own valid cells in class 'latency (ms)' (log-level +0.01 against the column medians); column level from 2 frameworks |
| sheet 10,000 rows resize configure to present p50 (ms) | freya-056a5b4 | ~57.0 | 42.0 to 76.0 | own: 12 own valid cells in class 'latency (ms)' (log-level +0.01 against the column medians); column level from 2 frameworks |
| sheet 10,000 rows resize configure to present p50 (ms) | gpui-kit-fast | ~200.0 | 150.0 to 270.0 | own: 2 own valid cells in class 'latency (ms)' (log-level +1.27 against the column medians); column level from 2 frameworks |
| sheet 10,000 rows resize configure to present p50 (ms) | vizia | ~330.0 | 15.0 to 7,300 | own: 12 own valid cells in class 'latency (ms)' (log-level +1.79 against the column medians); column level from 2 frameworks |
| sheet 10,000 rows resize configure to present p50 (ms) | zgui | ~70.0 | 51.0 to 95.0 | own: 8 own valid cells in class 'latency (ms)' (log-level +0.22 against the column medians); column level from 2 frameworks |
| sheet 10,000 rows idle RSS (MiB) | dioxus | ~330.0 | 250.0 to 450.0 | own: 4 own valid cells in class 'resident memory (MiB)' (log-level +0.15 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows idle RSS (MiB) | dioxus-skia | ~280.0 | 210.0 to 380.0 | own: 5 own valid cells in class 'resident memory (MiB)' (log-level -0.02 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows idle RSS (MiB) | floem | ~250.0 | 180.0 to 330.0 | own: 2 own valid cells in class 'resident memory (MiB)' (log-level -0.16 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows idle RSS (MiB) | freya | ~270.0 | 200.0 to 360.0 | own: 5 own valid cells in class 'resident memory (MiB)' (log-level -0.08 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows idle RSS (MiB) | freya-056a5b4 | ~270.0 | 200.0 to 360.0 | own: 4 own valid cells in class 'resident memory (MiB)' (log-level -0.07 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows idle RSS (MiB) | gpui-kit-fast | ~380.0 | 280.0 to 520.0 | own: 3 own valid cells in class 'resident memory (MiB)' (log-level +0.29 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows idle RSS (MiB) | vizia | ~290.0 | 210.0 to 390.0 | own: 5 own valid cells in class 'resident memory (MiB)' (log-level -0.00 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows idle RSS (MiB) | zgui | ~340.0 | 250.0 to 460.0 | own: 5 own valid cells in class 'resident memory (MiB)' (log-level +0.17 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows idle CPU (ms/s) | dioxus | ~70.00 | 52.00 to 95.00 | own: 3 own valid cells in class 'idle CPU (ms/s)' (log-level +1.62 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows idle CPU (ms/s) | dioxus-skia | ~8.80 | 5.70 to 14.00 | own: 4 own valid cells in class 'idle CPU (ms/s)' (log-level -0.46 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows idle CPU (ms/s) | floem | ~2.20 | 1.20 to 3.80 | own: 2 own valid cells in class 'idle CPU (ms/s)' (log-level -1.85 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows idle CPU (ms/s) | freya | ~8.00 | 4.80 to 13.00 | own: 4 own valid cells in class 'idle CPU (ms/s)' (log-level -0.55 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows idle CPU (ms/s) | freya-056a5b4 | ~7.40 | 4.70 to 12.00 | own: 3 own valid cells in class 'idle CPU (ms/s)' (log-level -0.63 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows idle CPU (ms/s) | gpui-kit-fast | ~91.00 | 67.00 to 120.0 | own: 2 own valid cells in class 'idle CPU (ms/s)' (log-level +1.88 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows idle CPU (ms/s) | vizia | ~13.00 | 6.60 to 25.00 | own: 4 own valid cells in class 'idle CPU (ms/s)' (log-level -0.07 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows idle CPU (ms/s) | zgui | ~17.00 | 5.40 to 52.00 | own: 4 own valid cells in class 'idle CPU (ms/s)' (log-level +0.19 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes hover total p50 (ms) | dioxus | ~240.0 | 170.0 to 350.0 | own: 6 own valid cells in class 'latency (ms)' (log-level +0.71 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes hover total p50 (ms) | dioxus-skia | ~120.0 | 90.00 to 160.0 | own: 10 own valid cells in class 'latency (ms)' (log-level +0.01 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes hover total p50 (ms) | floem | ~100.0 | 56.00 to 190.0 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 1 frameworks |
| canvas 1,000 nodes hover total p50 (ms) | freya | ~120.0 | 90.00 to 160.0 | own: 12 own valid cells in class 'latency (ms)' (log-level +0.01 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes hover total p50 (ms) | freya-056a5b4 | ~120.0 | 90.00 to 160.0 | own: 12 own valid cells in class 'latency (ms)' (log-level +0.01 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes hover total p50 (ms) | gpui-kit-fast | ~430.0 | 320.0 to 580.0 | own: 2 own valid cells in class 'latency (ms)' (log-level +1.27 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes hover total p50 (ms) | vizia | ~720.0 | 33.00 to 16,000 | own: 12 own valid cells in class 'latency (ms)' (log-level +1.79 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes hover total p50 (ms) | zgui | ~150.0 | 110.0 to 200.0 | own: 8 own valid cells in class 'latency (ms)' (log-level +0.22 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes hover input to commit p50 (ms) | dioxus | ~230.0 | 160.0 to 330.0 | own: 6 own valid cells in class 'latency (ms)' (log-level +0.71 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes hover input to commit p50 (ms) | dioxus-skia | ~110.0 | 84.00 to 150.0 | own: 10 own valid cells in class 'latency (ms)' (log-level +0.01 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes hover input to commit p50 (ms) | floem | ~94.00 | 52.00 to 170.0 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 1 frameworks |
| canvas 1,000 nodes hover input to commit p50 (ms) | freya | ~110.0 | 83.00 to 150.0 | own: 12 own valid cells in class 'latency (ms)' (log-level +0.01 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes hover input to commit p50 (ms) | freya-056a5b4 | ~110.0 | 84.00 to 150.0 | own: 12 own valid cells in class 'latency (ms)' (log-level +0.01 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes hover input to commit p50 (ms) | gpui-kit-fast | ~400.0 | 300.0 to 540.0 | own: 2 own valid cells in class 'latency (ms)' (log-level +1.27 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes hover input to commit p50 (ms) | vizia | ~670.0 | 30.00 to 15,000 | own: 12 own valid cells in class 'latency (ms)' (log-level +1.79 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes hover input to commit p50 (ms) | zgui | ~140.0 | 100.0 to 190.0 | own: 8 own valid cells in class 'latency (ms)' (log-level +0.22 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes edit total p50 (ms) | dioxus | ~32.00 | 22.00 to 47.00 | own: 6 own valid cells in class 'latency (ms)' (log-level +0.71 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes edit total p50 (ms) | dioxus-skia | ~16.00 | 12.00 to 22.00 | own: 10 own valid cells in class 'latency (ms)' (log-level +0.01 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes edit total p50 (ms) | floem | ~14.00 | 7.40 to 25.00 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 1 frameworks |
| canvas 1,000 nodes edit total p50 (ms) | freya | ~16.00 | 12.00 to 22.00 | own: 12 own valid cells in class 'latency (ms)' (log-level +0.01 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes edit total p50 (ms) | freya-056a5b4 | ~16.00 | 12.00 to 22.00 | own: 12 own valid cells in class 'latency (ms)' (log-level +0.01 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes edit total p50 (ms) | gpui-kit-fast | ~57.00 | 42.00 to 77.00 | own: 2 own valid cells in class 'latency (ms)' (log-level +1.27 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes edit total p50 (ms) | vizia | ~96.00 | 4.40 to 2,100 | own: 12 own valid cells in class 'latency (ms)' (log-level +1.79 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes edit total p50 (ms) | zgui | ~20.00 | 15.00 to 27.00 | own: 8 own valid cells in class 'latency (ms)' (log-level +0.22 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes resize configure to present p50 (ms) | dioxus | ~270.0 | 190.0 to 390.0 | own: 6 own valid cells in class 'latency (ms)' (log-level +0.71 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes resize configure to present p50 (ms) | dioxus-skia | ~140.0 | 100.0 to 180.0 | own: 10 own valid cells in class 'latency (ms)' (log-level +0.01 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes resize configure to present p50 (ms) | floem | ~110.0 | 62.0 to 210.0 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 1 frameworks |
| canvas 1,000 nodes resize configure to present p50 (ms) | freya | ~130.0 | 100.0 to 180.0 | own: 12 own valid cells in class 'latency (ms)' (log-level +0.01 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes resize configure to present p50 (ms) | freya-056a5b4 | ~140.0 | 100.0 to 180.0 | own: 12 own valid cells in class 'latency (ms)' (log-level +0.01 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes resize configure to present p50 (ms) | gpui-kit-fast | ~480.0 | 350.0 to 650.0 | own: 2 own valid cells in class 'latency (ms)' (log-level +1.27 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes resize configure to present p50 (ms) | vizia | ~800.0 | 36.0 to 18,000 | own: 12 own valid cells in class 'latency (ms)' (log-level +1.79 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes resize configure to present p50 (ms) | zgui | ~170.0 | 120.0 to 230.0 | own: 8 own valid cells in class 'latency (ms)' (log-level +0.22 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes idle RSS (MiB) | dioxus | ~210.0 | 150.0 to 280.0 | own: 4 own valid cells in class 'resident memory (MiB)' (log-level +0.15 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes idle RSS (MiB) | dioxus-skia | ~170.0 | 130.0 to 240.0 | own: 5 own valid cells in class 'resident memory (MiB)' (log-level -0.02 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes idle RSS (MiB) | floem | ~150.0 | 110.0 to 210.0 | own: 2 own valid cells in class 'resident memory (MiB)' (log-level -0.16 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes idle RSS (MiB) | freya | ~170.0 | 120.0 to 220.0 | own: 5 own valid cells in class 'resident memory (MiB)' (log-level -0.08 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes idle RSS (MiB) | freya-056a5b4 | ~170.0 | 120.0 to 220.0 | own: 4 own valid cells in class 'resident memory (MiB)' (log-level -0.07 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes idle RSS (MiB) | gpui-kit-fast | ~240.0 | 180.0 to 320.0 | own: 3 own valid cells in class 'resident memory (MiB)' (log-level +0.29 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes idle RSS (MiB) | vizia | ~180.0 | 130.0 to 240.0 | own: 5 own valid cells in class 'resident memory (MiB)' (log-level -0.00 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes idle RSS (MiB) | zgui | ~210.0 | 160.0 to 290.0 | own: 5 own valid cells in class 'resident memory (MiB)' (log-level +0.17 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes idle CPU (ms/s) | dioxus | ~130.0 | 100.0 to 180.0 | own: 3 own valid cells in class 'idle CPU (ms/s)' (log-level +1.62 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes idle CPU (ms/s) | dioxus-skia | ~17.00 | 11.00 to 26.00 | own: 4 own valid cells in class 'idle CPU (ms/s)' (log-level -0.46 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes idle CPU (ms/s) | floem | ~4.20 | 2.40 to 7.40 | own: 2 own valid cells in class 'idle CPU (ms/s)' (log-level -1.85 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes idle CPU (ms/s) | freya | ~15.00 | 9.20 to 25.00 | own: 4 own valid cells in class 'idle CPU (ms/s)' (log-level -0.55 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes idle CPU (ms/s) | freya-056a5b4 | ~14.00 | 9.00 to 22.00 | own: 3 own valid cells in class 'idle CPU (ms/s)' (log-level -0.63 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes idle CPU (ms/s) | gpui-kit-fast | ~170.0 | 130.0 to 230.0 | own: 2 own valid cells in class 'idle CPU (ms/s)' (log-level +1.88 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes idle CPU (ms/s) | vizia | ~25.00 | 13.00 to 49.00 | own: 4 own valid cells in class 'idle CPU (ms/s)' (log-level -0.07 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes idle CPU (ms/s) | zgui | ~32.00 | 10.00 to 100.0 | own: 4 own valid cells in class 'idle CPU (ms/s)' (log-level +0.19 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes edit total p50 (ms) | dioxus | ~74.00 | 51.00 to 110.0 | own: 6 own valid cells in class 'latency (ms)' (log-level +0.71 against the column medians); column level from 1 frameworks |
| canvas 5,000 nodes edit total p50 (ms) | dioxus-skia | ~37.00 | 27.00 to 50.00 | own: 10 own valid cells in class 'latency (ms)' (log-level +0.01 against the column medians); column level from 1 frameworks |
| canvas 5,000 nodes edit total p50 (ms) | floem | ~31.00 | 17.00 to 56.00 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 1 frameworks |
| canvas 5,000 nodes edit total p50 (ms) | freya | ~37.00 | 27.00 to 49.00 | own: 12 own valid cells in class 'latency (ms)' (log-level +0.01 against the column medians); column level from 1 frameworks |
| canvas 5,000 nodes edit total p50 (ms) | freya-056a5b4 | ~37.00 | 27.00 to 50.00 | own: 12 own valid cells in class 'latency (ms)' (log-level +0.01 against the column medians); column level from 1 frameworks |
| canvas 5,000 nodes edit total p50 (ms) | gpui-kit-fast | ~130.0 | 96.00 to 180.0 | own: 2 own valid cells in class 'latency (ms)' (log-level +1.27 against the column medians); column level from 1 frameworks |
| canvas 5,000 nodes edit total p50 (ms) | vizia | ~220.0 | 9.90 to 4,700 | own: 12 own valid cells in class 'latency (ms)' (log-level +1.79 against the column medians); column level from 1 frameworks |
| canvas 5,000 nodes edit total p50 (ms) | zgui | ~45.00 | 33.00 to 62.00 | own: 8 own valid cells in class 'latency (ms)' (log-level +0.22 against the column medians); column level from 1 frameworks |
| canvas 5,000 nodes resize configure to present p50 (ms) | dioxus | ~1,300 | 860.0 to 1,800 | own: 6 own valid cells in class 'latency (ms)' (log-level +0.71 against the column medians); column level from 1 frameworks |
| canvas 5,000 nodes resize configure to present p50 (ms) | dioxus-skia | ~620.0 | 460.0 to 840.0 | own: 10 own valid cells in class 'latency (ms)' (log-level +0.01 against the column medians); column level from 1 frameworks |
| canvas 5,000 nodes resize configure to present p50 (ms) | floem | ~520.0 | 290.0 to 950.0 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 1 frameworks |
| canvas 5,000 nodes resize configure to present p50 (ms) | freya | ~620.0 | 460.0 to 840.0 | own: 12 own valid cells in class 'latency (ms)' (log-level +0.01 against the column medians); column level from 1 frameworks |
| canvas 5,000 nodes resize configure to present p50 (ms) | freya-056a5b4 | ~620.0 | 460.0 to 840.0 | own: 12 own valid cells in class 'latency (ms)' (log-level +0.01 against the column medians); column level from 1 frameworks |
| canvas 5,000 nodes resize configure to present p50 (ms) | gpui-kit-fast | ~2,200 | 1,600 to 3,000 | own: 2 own valid cells in class 'latency (ms)' (log-level +1.27 against the column medians); column level from 1 frameworks |
| canvas 5,000 nodes resize configure to present p50 (ms) | vizia | ~3,700 | 170.0 to 81,000 | own: 12 own valid cells in class 'latency (ms)' (log-level +1.79 against the column medians); column level from 1 frameworks |
| canvas 5,000 nodes resize configure to present p50 (ms) | zgui | ~770.0 | 570.0 to 1,100 | own: 8 own valid cells in class 'latency (ms)' (log-level +0.22 against the column medians); column level from 1 frameworks |
| canvas 5,000 nodes idle RSS (MiB) | dioxus | ~310.0 | 230.0 to 410.0 | own: 4 own valid cells in class 'resident memory (MiB)' (log-level +0.15 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes idle RSS (MiB) | dioxus-skia | ~260.0 | 190.0 to 350.0 | own: 5 own valid cells in class 'resident memory (MiB)' (log-level -0.02 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes idle RSS (MiB) | floem | ~220.0 | 170.0 to 300.0 | own: 2 own valid cells in class 'resident memory (MiB)' (log-level -0.16 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes idle RSS (MiB) | freya | ~240.0 | 180.0 to 330.0 | own: 5 own valid cells in class 'resident memory (MiB)' (log-level -0.08 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes idle RSS (MiB) | freya-056a5b4 | ~240.0 | 180.0 to 330.0 | own: 4 own valid cells in class 'resident memory (MiB)' (log-level -0.07 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes idle RSS (MiB) | gpui-kit-fast | ~350.0 | 260.0 to 470.0 | own: 3 own valid cells in class 'resident memory (MiB)' (log-level +0.29 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes idle RSS (MiB) | vizia | ~260.0 | 190.0 to 360.0 | own: 5 own valid cells in class 'resident memory (MiB)' (log-level -0.00 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes idle RSS (MiB) | zgui | ~310.0 | 230.0 to 420.0 | own: 5 own valid cells in class 'resident memory (MiB)' (log-level +0.17 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes idle CPU (ms/s) | dioxus | ~490.0 | 360.0 to 660.0 | own: 3 own valid cells in class 'idle CPU (ms/s)' (log-level +1.62 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes idle CPU (ms/s) | dioxus-skia | ~61.00 | 40.00 to 94.00 | own: 4 own valid cells in class 'idle CPU (ms/s)' (log-level -0.46 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes idle CPU (ms/s) | floem | ~15.00 | 8.60 to 27.00 | own: 2 own valid cells in class 'idle CPU (ms/s)' (log-level -1.85 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes idle CPU (ms/s) | freya | ~55.00 | 33.00 to 92.00 | own: 4 own valid cells in class 'idle CPU (ms/s)' (log-level -0.55 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes idle CPU (ms/s) | freya-056a5b4 | ~52.00 | 33.00 to 82.00 | own: 3 own valid cells in class 'idle CPU (ms/s)' (log-level -0.63 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes idle CPU (ms/s) | gpui-kit-fast | ~630.0 | 470.0 to 850.0 | own: 2 own valid cells in class 'idle CPU (ms/s)' (log-level +1.88 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes idle CPU (ms/s) | vizia | ~90.00 | 46.00 to 180.0 | own: 4 own valid cells in class 'idle CPU (ms/s)' (log-level -0.07 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes idle CPU (ms/s) | zgui | ~120.0 | 37.00 to 360.0 | own: 4 own valid cells in class 'idle CPU (ms/s)' (log-level +0.19 against the column medians); column level from 4 frameworks |
| sheet 1,000 startup to first present (ms) | dioxus | ~9,000 | 6,600 to 12,000 | own: 3 own valid cells in class 'startup (ms)' (log-level +0.45 against the column medians); column level from 4 frameworks |
| sheet 1,000 startup to first present (ms) | dioxus-skia | ~5,700 | 1,400 to 24,000 | own: 3 own valid cells in class 'startup (ms)' (log-level +0.00 against the column medians); column level from 4 frameworks |
| sheet 1,000 startup to first present (ms) | floem | ~4,900 | 2,700 to 8,800 | cross-class: only 1 own valid cell(s) in class 'startup (ms)': level taken across all classes (cross-class, low confidence); column level from 4 frameworks |
| sheet 1,000 startup to first present (ms) | freya | ~4,900 | 3,200 to 7,400 | own: 3 own valid cells in class 'startup (ms)' (log-level -0.17 against the column medians); column level from 4 frameworks |
| sheet 1,000 startup to first present (ms) | freya-056a5b4 | ~5,600 | 3,100 to 9,900 | own: 3 own valid cells in class 'startup (ms)' (log-level -0.03 against the column medians); column level from 4 frameworks |
| sheet 1,000 startup to first present (ms) | gpui-kit-fast | ~7,700 | 5,700 to 10,000 | own: 3 own valid cells in class 'startup (ms)' (log-level +0.29 against the column medians); column level from 4 frameworks |
| sheet 1,000 startup to first present (ms) | vizia | ~2,200 | 1,600 to 2,900 | own: 3 own valid cells in class 'startup (ms)' (log-level -0.97 against the column medians); column level from 4 frameworks |
| sheet 1,000 startup to first present (ms) | zgui | ~6,400 | 3,800 to 11,000 | own: 3 own valid cells in class 'startup (ms)' (log-level +0.11 against the column medians); column level from 4 frameworks |
| sheet 10,000 startup to first present (ms) | dioxus | ~870.0 | 650.0 to 1,200 | own: 3 own valid cells in class 'startup (ms)' (log-level +0.45 against the column medians); column level from 4 frameworks |
| sheet 10,000 startup to first present (ms) | dioxus-skia | ~560.0 | 130.0 to 2,300 | own: 3 own valid cells in class 'startup (ms)' (log-level +0.00 against the column medians); column level from 4 frameworks |
| sheet 10,000 startup to first present (ms) | floem | ~470.0 | 260.0 to 860.0 | cross-class: only 1 own valid cell(s) in class 'startup (ms)': level taken across all classes (cross-class, low confidence); column level from 4 frameworks |
| sheet 10,000 startup to first present (ms) | freya | ~470.0 | 310.0 to 720.0 | own: 3 own valid cells in class 'startup (ms)' (log-level -0.17 against the column medians); column level from 4 frameworks |
| sheet 10,000 startup to first present (ms) | freya-056a5b4 | ~540.0 | 300.0 to 970.0 | own: 3 own valid cells in class 'startup (ms)' (log-level -0.03 against the column medians); column level from 4 frameworks |
| sheet 10,000 startup to first present (ms) | gpui-kit-fast | ~750.0 | 550.0 to 1,000 | own: 3 own valid cells in class 'startup (ms)' (log-level +0.29 against the column medians); column level from 4 frameworks |
| sheet 10,000 startup to first present (ms) | vizia | ~210.0 | 160.0 to 290.0 | own: 3 own valid cells in class 'startup (ms)' (log-level -0.97 against the column medians); column level from 4 frameworks |
| sheet 10,000 startup to first present (ms) | zgui | ~620.0 | 370.0 to 1,100 | own: 3 own valid cells in class 'startup (ms)' (log-level +0.11 against the column medians); column level from 4 frameworks |
| canvas 1,000 startup to first present (ms) | dioxus | ~1,300 | 940.0 to 1,700 | own: 3 own valid cells in class 'startup (ms)' (log-level +0.45 against the column medians); column level from 4 frameworks |
| canvas 1,000 startup to first present (ms) | dioxus-skia | ~810.0 | 200.0 to 3,400 | own: 3 own valid cells in class 'startup (ms)' (log-level +0.00 against the column medians); column level from 4 frameworks |
| canvas 1,000 startup to first present (ms) | floem | ~690.0 | 380.0 to 1,300 | cross-class: only 1 own valid cell(s) in class 'startup (ms)': level taken across all classes (cross-class, low confidence); column level from 4 frameworks |
| canvas 1,000 startup to first present (ms) | freya | ~690.0 | 450.0 to 1,000 | own: 3 own valid cells in class 'startup (ms)' (log-level -0.17 against the column medians); column level from 4 frameworks |
| canvas 1,000 startup to first present (ms) | freya-056a5b4 | ~790.0 | 440.0 to 1,400 | own: 3 own valid cells in class 'startup (ms)' (log-level -0.03 against the column medians); column level from 4 frameworks |
| canvas 1,000 startup to first present (ms) | gpui-kit-fast | ~1,100 | 800.0 to 1,500 | own: 3 own valid cells in class 'startup (ms)' (log-level +0.29 against the column medians); column level from 4 frameworks |
| canvas 1,000 startup to first present (ms) | vizia | ~310.0 | 230.0 to 420.0 | own: 3 own valid cells in class 'startup (ms)' (log-level -0.97 against the column medians); column level from 4 frameworks |
| canvas 1,000 startup to first present (ms) | zgui | ~910.0 | 530.0 to 1,500 | own: 3 own valid cells in class 'startup (ms)' (log-level +0.11 against the column medians); column level from 4 frameworks |
| canvas 5,000 startup to first present (ms) | dioxus | ~1,400 | 1,000 to 1,900 | own: 3 own valid cells in class 'startup (ms)' (log-level +0.45 against the column medians); column level from 4 frameworks |
| canvas 5,000 startup to first present (ms) | dioxus-skia | ~900.0 | 220.0 to 3,700 | own: 3 own valid cells in class 'startup (ms)' (log-level +0.00 against the column medians); column level from 4 frameworks |
| canvas 5,000 startup to first present (ms) | floem | ~760.0 | 420.0 to 1,400 | cross-class: only 1 own valid cell(s) in class 'startup (ms)': level taken across all classes (cross-class, low confidence); column level from 4 frameworks |
| canvas 5,000 startup to first present (ms) | freya | ~760.0 | 500.0 to 1,100 | own: 3 own valid cells in class 'startup (ms)' (log-level -0.17 against the column medians); column level from 4 frameworks |
| canvas 5,000 startup to first present (ms) | freya-056a5b4 | ~870.0 | 490.0 to 1,600 | own: 3 own valid cells in class 'startup (ms)' (log-level -0.03 against the column medians); column level from 4 frameworks |
| canvas 5,000 startup to first present (ms) | gpui-kit-fast | ~1,200 | 890.0 to 1,600 | own: 3 own valid cells in class 'startup (ms)' (log-level +0.29 against the column medians); column level from 4 frameworks |
| canvas 5,000 startup to first present (ms) | vizia | ~340.0 | 250.0 to 460.0 | own: 3 own valid cells in class 'startup (ms)' (log-level -0.97 against the column medians); column level from 4 frameworks |
| canvas 5,000 startup to first present (ms) | zgui | ~1,000 | 590.0 to 1,700 | own: 3 own valid cells in class 'startup (ms)' (log-level +0.11 against the column medians); column level from 4 frameworks |
| spinner above 10,000 rows: presented frames per second (fps) | floem | ~60.0 | 44.0 to 60.0 | own: 2 own valid cells in class 'frame rate (fps)' (log-level -0.01 against the column medians); column level from 6 frameworks |
| spinner above 10,000 rows: frame interval p50 (ms) | floem | ~17.00 | 12.00 to 22.00 | own: 2 own valid cells in class 'frame interval (ms)' (log-level +0.00 against the column medians); column level from 6 frameworks |
| spinner above 10,000 rows: frame interval p99 (ms) | floem | ~24.00 | 17.00 to 32.00 | own: 2 own valid cells in class 'frame interval (ms)' (log-level +0.00 against the column medians); column level from 6 frameworks |
| spinner above 10,000 rows: CPU per frame (ms) | floem | ~5.50 | 4.10 to 7.40 | own: 2 own valid cells in class 'cpu per frame (ms)' (log-level -1.59 against the column medians); column level from 6 frameworks |
| spinner above 10,000 rows: CPU per second (ms/s) | floem | ~320.0 | 180.0 to 570.0 | own: 2 own valid cells in class 'idle CPU (ms/s)' (log-level -1.85 against the column medians); column level from 6 frameworks |
| spinner above 10,000 rows: RSS (MiB) | floem | ~440.0 | 320.0 to 590.0 | own: 2 own valid cells in class 'resident memory (MiB)' (log-level -0.16 against the column medians); column level from 6 frameworks |

## Heavy scenes: Sheet, 1,000 rows

| framework | sheet 1,000 rows hover total p50 (ms) | sheet 1,000 rows hover input to commit p50 (ms) | sheet 1,000 rows theme switch total p50 (ms) | sheet 1,000 rows edit total p50 (ms) | sheet 1,000 rows resize configure to present p50 (ms) | sheet 1,000 rows idle RSS (MiB) | sheet 1,000 rows idle CPU (ms/s) | sheet 1,000 startup to first present (ms) |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| zgui | *~20.00* | *~3.70* | *~18.00* | *~19.00* | *~31.0* | *~210.0* | *~17.00* | *~6,400* |
| gpui-kit | 29.64 † | 17.30 † | 29.32 † | 29.34 † | 46.0 † | 199.5 | 63.73 | - |
| gpui-kit-fast | *~58.00* | *~11.00* | *~52.00* | *~55.00* | *~89.0* | *~240.0* | *~93.00* | *~7,700* |
| freya | *~16.00* | *~3.00* | *~15.00* | *~15.00* | *~25.0* | *~170.0* | *~8.10* | *~4,900* |
| freya-056a5b4 | *~16.00* | *~3.00* | *~15.00* | *~16.00* | *~25.0* | *~170.0* | *~7.60* | *~5,600* |
| vizia | *~96.00* | *~18.00* | *~86.00* | *~92.00* | *~150.0* | *~180.0* | *~13.00* | *~2,200* |
| floem | *~14.00* | *~2.50* | *~12.00* | *~13.00* | *~21.0* | *~150.0* | *~2.20* | *~4,900* |
| iced | 27.48 † | 15.25 † | 27.69 † | 26.34 † | 27.6 | 198.9 | 15.55 | - |
| egui | 15.47 † | 7.86 † | 14.95 | 15.19 † | 16.2 | 157.8 | 24.83 | - |
| dioxus | *~33.00* | *~6.00* | *~29.00* | *~31.00* | *~50.0* | *~210.0* | *~72.00* | *~9,000* |
| dioxus-skia | *~16.00* | *~3.00* | *~15.00* | *~16.00* | *~25.0* | *~180.0* | *~9.00* | *~5,700* |
| bough | 13.26 | 2.44 | 14.94 † | 12.64 | 20.4 | 51.2 | 0.03 | - |

## Heavy scenes: Sheet, 10,000 rows

| framework | sheet 10,000 rows hover total p50 (ms) | sheet 10,000 rows hover input to commit p50 (ms) | sheet 10,000 rows theme switch total p50 (ms) | sheet 10,000 rows edit total p50 (ms) | sheet 10,000 rows resize configure to present p50 (ms) | sheet 10,000 rows idle RSS (MiB) | sheet 10,000 rows idle CPU (ms/s) | sheet 10,000 startup to first present (ms) |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| zgui | *~98.00* | *~74.00* | *~240.0* | *~40.00* | *~70.0* | *~340.0* | *~17.00* | *~620.0* |
| gpui-kit | 29.92 † | 19.26 † | 29.30 † | 30.47 † | 50.2 † | 187.3 | 65.15 | - |
| gpui-kit-fast | *~280.0* | *~210.0* | *~670.0* | *~110.0* | *~200.0* | *~380.0* | *~91.00* | *~750.0* |
| freya | *~79.00* | *~59.00* | *~190.0* | *~32.00* | *~56.0* | *~270.0* | *~8.00* | *~470.0* |
| freya-056a5b4 | *~79.00* | *~60.00* | *~190.0* | *~32.00* | *~57.0* | *~270.0* | *~7.40* | *~540.0* |
| vizia | *~470.0* | *~350.0* | *~1,100* | *~190.0* | *~330.0* | *~290.0* | *~13.00* | *~210.0* |
| floem | *~66.00* | *~50.00* | *~160.0* | *~27.00* | *~47.0* | *~250.0* | *~2.20* | *~470.0* |
| iced | 164.4 | 152.4 | 179.3 | 179.9 | 139.8 | 550.4 | 27.58 | - |
| egui | 16.19 † | 8.06 † | 15.21 † | 14.95 | 22.0 | 156.9 | 24.39 | - |
| dioxus | *~160.0* | *~120.0* | *~380.0* | *~65.00* | *~110.0* | *~330.0* | *~70.00* | *~870.0* |
| dioxus-skia | *~79.00* | *~60.00* | *~190.0* | *~32.00* | *~57.0* | *~280.0* | *~8.80* | *~560.0* |
| bough | 29.37 | 17.85 | 29.88 † | 26.32 | *~46.0* (failed cell) | 332.6 | 0.03 | - |

## Heavy scenes: Canvas, 1,000 nodes

| framework | canvas 1,000 nodes hover total p50 (ms) | canvas 1,000 nodes hover input to commit p50 (ms) | canvas 1,000 nodes theme switch total p50 (ms) | canvas 1,000 nodes edit total p50 (ms) | canvas 1,000 nodes resize configure to present p50 (ms) | canvas 1,000 nodes idle RSS (MiB) | canvas 1,000 nodes idle CPU (ms/s) | canvas 1,000 startup to first present (ms) |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| zgui | *~150.0* | *~140.0* | - | *~20.00* | *~170.0* | *~210.0* | *~32.00* | *~910.0* |
| gpui-kit | 59.70 † | 49.83 † | 61.03 † | 62.71 † | 72.4 † | 195.2 | 159.0 | - |
| gpui-kit-fast | *~430.0* | *~400.0* | - | *~57.00* | *~480.0* | *~240.0* | *~170.0* | *~1,100* |
| freya | *~120.0* | *~110.0* | - | *~16.00* | *~130.0* | *~170.0* | *~15.00* | *~690.0* |
| freya-056a5b4 | *~120.0* | *~110.0* | - | *~16.00* | *~140.0* | *~170.0* | *~14.00* | *~790.0* |
| vizia | *~720.0* | *~670.0* | - | *~96.00* | *~800.0* | *~180.0* | *~25.00* | *~310.0* |
| floem | *~100.0* | *~94.00* | - | *~14.00* | *~110.0* | *~150.0* | *~4.20* | *~690.0* |
| iced | 34.60 † | 25.78 † | 31.86 † | 33.92 † | 38.3 † | 428.0 | 41.48 | - |
| egui | 32.81 † | 23.16 † | 31.58 † | 31.60 † | 41.1 † | 172.8 | 55.41 | - |
| dioxus | *~240.0* | *~230.0* | - | *~32.00* | *~270.0* | *~210.0* | *~130.0* | *~1,300* |
| dioxus-skia | *~120.0* | *~110.0* | - | *~16.00* | *~140.0* | *~170.0* | *~17.00* | *~810.0* |
| bough | 98.79 | 91.94 | 293.0 † | 13.20 | 110.3 | 19.9 | 0.04 | - |

## Heavy scenes: Canvas, 5,000 nodes

| framework | canvas 5,000 nodes hover total p50 (ms) | canvas 5,000 nodes hover input to commit p50 (ms) | canvas 5,000 nodes theme switch total p50 (ms) | canvas 5,000 nodes edit total p50 (ms) | canvas 5,000 nodes resize configure to present p50 (ms) | canvas 5,000 nodes idle RSS (MiB) | canvas 5,000 nodes idle CPU (ms/s) | canvas 5,000 startup to first present (ms) |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| zgui | - | - | - | *~45.00* | *~770.0* | *~310.0* | *~120.0* | *~1,000* |
| gpui-kit | 210.1 † | 201.0 † | 186.3 † | 196.0 † | 183.3 † | 263.3 | 454.9 | - |
| gpui-kit-fast | - | - | - | *~130.0* | *~2,200* | *~350.0* | *~630.0* | *~1,200* |
| freya | - | - | - | *~37.00* | *~620.0* | *~240.0* | *~55.00* | *~760.0* |
| freya-056a5b4 | - | - | - | *~37.00* | *~620.0* | *~240.0* | *~52.00* | *~870.0* |
| vizia | - | - | - | *~220.0* | *~3,700* | *~260.0* | *~90.00* | *~340.0* |
| floem | - | - | - | *~31.00* | *~520.0* | *~220.0* | *~15.00* | *~760.0* |
| iced | 85.31 † | 77.51 † | 83.97 † | 84.08 † | 92.1 † | 1,379 | 191.4 | - |
| egui | 94.13 † | 83.71 † | 90.16 † | 81.58 † | 89.6 † | 278.4 | 236.1 | - |
| dioxus | - | - | - | *~74.00* | *~1,300* | *~310.0* | *~490.0* | *~1,400* |
| dioxus-skia | - | - | - | *~37.00* | *~620.0* | *~260.0* | *~61.00* | *~900.0* |
| bough | 925.8 † | 917.6 † | 1,132 † | 29.90 | 508.3 | 25.9 | 0.03 | - |

## Heavy scenes, then (r7-scenes-1) and now (heavy-1), measured frameworks

| metric | bough now / then | iced now / then | egui now / then | gpui-kit now / then |
|---|---:|---:|---:|---:|
| sheet 1,000 rows hover total p50 (ms) | 13.26 / 12.69 | 27.48 † / 29.82 † | 15.47 † / 14.57 | 29.64 † / 29.36 † |
| sheet 1,000 rows hover input to commit p50 (ms) | 2.44 / 1.02 | 15.25 † / 18.02 † | 7.86 † / 6.91 | 17.30 † / 16.93 † |
| sheet 1,000 rows theme switch total p50 (ms) | 14.94 † / 13.20 | 27.69 † / 29.31 | 14.95 / 15.20 † | 29.32 † / 27.69 † |
| sheet 1,000 rows edit total p50 (ms) | 12.64 / 12.68 † | 26.34 † / 29.32 † | 15.19 † / 13.22 † | 29.34 † / 29.31 † |
| sheet 1,000 rows resize configure to present p50 (ms) | 20.4 / 12.6 | 27.6 / 18.7 | 16.2 / 18.5 | 46.0 † / 47.8 † |
| sheet 1,000 rows idle RSS (MiB) | 51.2 / 48.3 | 198.9 / 187.0 | 157.8 / 149.0 | 199.5 / 187.7 |
| sheet 1,000 rows idle CPU (ms/s) | 0.03 / 0.03 | 15.55 / 41.53 | 24.83 / 34.86 | 63.73 / 110.2 |
| sheet 10,000 rows hover total p50 (ms) | 29.37 / 14.57 | 164.4 / 174.1 | 16.19 † / 15.46 † | 29.92 † / 29.36 † |
| sheet 10,000 rows hover input to commit p50 (ms) | 17.85 / 5.12 | 152.4 / 162.4 | 8.06 † / 7.50 † | 19.26 † / 17.79 † |
| sheet 10,000 rows theme switch total p50 (ms) | 29.88 † / 17.30 | 179.3 / 180.5 | 15.21 † / 14.94 † | 29.30 † / 29.31 † |
| sheet 10,000 rows edit total p50 (ms) | 26.32 / 12.63 † | 179.9 / 179.9 | 14.95 / 13.22 † | 30.47 † / 29.31 † |
| sheet 10,000 rows resize configure to present p50 (ms) | failed / 1,044 † | 139.8 / 150.9 | 22.0 / 25.2 † | 50.2 † / 37.7 † |
| sheet 10,000 rows idle RSS (MiB) | 332.6 / 330.1 | 550.4 / 551.3 | 156.9 / 149.5 | 187.3 / 188.5 |
| sheet 10,000 rows idle CPU (ms/s) | 0.03 / 0.03 | 27.58 / 44.85 | 24.39 / 37.69 | 65.15 / 135.2 |
| canvas 1,000 nodes hover total p50 (ms) | 98.79 / 94.16 † | 34.60 † / 29.91 † | 32.81 † / 29.46 † | 59.70 † / 47.88 † |
| canvas 1,000 nodes hover input to commit p50 (ms) | 91.94 / 80.65 † | 25.78 † / 19.27 † | 23.16 † / 18.78 † | 49.83 † / 39.53 † |
| canvas 1,000 nodes theme switch total p50 (ms) | 293.0 † / 266.0 † | 31.86 † / 29.87 † | 31.58 † / 29.88 † | 61.03 † / 46.02 † |
| canvas 1,000 nodes edit total p50 (ms) | 13.20 / 13.22 † | 33.92 † / 30.53 † | 31.60 † / 29.59 † | 62.71 † / 45.91 † |
| canvas 1,000 nodes resize configure to present p50 (ms) | 110.3 / 91.5 † | 38.3 † / 25.0 † | 41.1 † / 24.1 † | 72.4 † / 56.5 † |
| canvas 1,000 nodes idle RSS (MiB) | 19.9 / 17.8 | 428.0 / 421.8 | 172.8 / 173.6 | 195.2 / 196.5 |
| canvas 1,000 nodes idle CPU (ms/s) | 0.04 / 0.03 | 41.48 / 119.0 | 55.41 / 83.04 | 159.0 / 184.9 |
| canvas 5,000 nodes hover total p50 (ms) | 925.8 † / 889.0 † | 85.31 † / 77.47 † | 94.13 † / 106.4 † | 210.1 † / 188.5 † |
| canvas 5,000 nodes hover input to commit p50 (ms) | 917.6 † / 880.8 † | 77.51 † / 67.48 † | 83.71 † / 100.2 † | 201.0 † / 178.0 † |
| canvas 5,000 nodes theme switch total p50 (ms) | 1,132 † / 1,056 † | 83.97 † / 78.70 † | 90.16 † / 211.6 † | 186.3 † / 181.6 † |
| canvas 5,000 nodes edit total p50 (ms) | 29.90 / 29.89 † | 84.08 † / 79.25 † | 81.58 † / 226.6 † | 196.0 † / 192.9 † |
| canvas 5,000 nodes resize configure to present p50 (ms) | 508.3 / 386.2 † | 92.1 † / 89.7 † | 89.6 † / 119.2 † | 183.3 † / 180.2 † |
| canvas 5,000 nodes idle RSS (MiB) | 25.9 / 24.5 | 1,379 / 1,384 | 278.4 / 279.4 | 263.3 / 264.4 |
| canvas 5,000 nodes idle CPU (ms/s) | 0.03 / 0.03 | 191.4 / 249.9 | 236.1 / 337.5 | 454.9 / 643.5 |
| sheet 1,000 startup to first present (ms) | 2,565 / 112.5 | 17,312 / 199.0 | 1,246 / 132.1 | 19,434 / 265.6 |
| sheet 10,000 startup to first present (ms) | 712.7 / 665.6 | 829.0 / 915.6 | 246.0 / 132.3 | 312.5 / 249.0 |
| canvas 1,000 startup to first present (ms) | 776.1 / 382.3 | 3,200 / 365.7 | 374.8 / 149.0 | 414.8 / 348.3 |
| canvas 5,000 startup to first present (ms) | 1,263 / 1,066 | 1,332 / 1,382 | 265.3 / 265.6 | 648.3 / 631.8 |

## Animation: the spinner scenarios (the only animation cells)

| framework | spinner, no rows: presented frames per second (fps) | spinner, no rows: frame interval p50 (ms) | spinner, no rows: frame interval p99 (ms) | spinner, no rows: jank fraction (fraction) | spinner, no rows: missed vblanks (count) | spinner, no rows: CPU per frame (ms) | spinner, no rows: CPU per second (ms/s) | spinner above 10,000 rows: presented frames per second (fps) | spinner above 10,000 rows: frame interval p50 (ms) | spinner above 10,000 rows: frame interval p99 (ms) | spinner above 10,000 rows: jank fraction (fraction) | spinner above 10,000 rows: missed vblanks (count) | spinner above 10,000 rows: CPU per frame (ms) | spinner above 10,000 rows: CPU per second (ms/s) |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| zgui | 59.8 | 16.67 | 16.67 | 0.003 | 0 | 19.23 | 1,150 | 60.0 | 16.67 | 16.67 | 0.000 | 0 | 19.09 | 1,144 |
| gpui-kit | 57.6 | 16.67 | 33.33 | 0.041 | 7 | 103.2 | 5,951 | 3.4 † | 300.0 † | 316.0 † | 0.000 † | 150.5 † | 438.2 † | 1,486 † |
| gpui-kit-fast | 56.1 † | 16.67 † | 27.08 † | 0.072 † | 12 † | 150.0 † | 8,445 † | 8.3 † | 116.7 † | 139.8 † | 0.000 † | 149.0 † | 328.7 † | 2,738 † |
| freya | 60.0 | 16.67 | 16.67 | 0.000 | 0 | 5.53 | 331.7 | 60.0 | 16.67 | 16.67 | 0.000 | 0 | 24.93 | 1,495 |
| freya-056a5b4 | 60.0 | 16.67 | 16.67 | 0.000 | 0 | 5.63 | 337.9 | 59.0 † | 16.67 † | 16.67 † | 0.003 † | 3 † | 25.88 † | 1,525 † |
| vizia | 60.0 | 16.67 | 16.67 | 0.000 | 0 | 11.96 | 717.4 | 57.2 | 16.67 | 33.33 | 0.050 | 8 | 26.26 | 1,500 |
| floem | 59.7 | 16.67 | 16.67 | 0.006 | 1 | 2.07 | 123.5 | *~60.0* (failed cell) | *~17.00* (failed cell) | *~24.00* (failed cell) | - | - | *~5.50* (failed cell) | *~320.0* (failed cell) |
| iced | 59.8 | 16.67 | 16.67 | 0.003 | 0 | 53.49 | 3,193 | 18.9 † | 58.33 † | 71.00 † | 0.000 † | 121.0 † | 133.0 † | 2,497 † |
| egui | 60.0 | 16.67 | 16.67 | 0.000 | 0 | 5.23 | 313.6 | 14.9 | 66.67 | 76.25 | 0.000 | 131.5 | 86.33 | 1,288 |
| dioxus | 60.0 | 16.67 | 16.67 | 0.000 | 0 | 86.48 | 5,189 | 37.1 † | 33.33 † | 33.33 † | 0.000 † | 68 † | 114.1 † | 4,235 † |
| dioxus-skia | 60.0 | 16.67 | 16.67 | 0.000 | 0 | 10.12 | 607.1 | 53.0 | 16.67 | 33.33 | 0.137 | 21 | 39.34 | 2,077 |
| bough | 60.0 | 16.67 | 16.67 | 0.000 | 0 | 0.17 | 10.2 | 60.0 | 16.67 | 16.67 | 0.000 | 0 | 1.13 | 67.8 |

## Latency split (input to commit, wait, present)

| framework | probe | total p50 | input to commit | commit to ready | ready to latch | latch to present |
|---|---|---:|---:|---:|---:|---:|
| zgui | rows 3,000 hover | 16.36 | 5.37 | 0.00 | 9.91 | 1.00 |
| gpui-kit | rows 3,000 hover | 98.46 | 89.38 | 0.00 | 8.16 | 1.00 |
| gpui-kit-fast | rows 3,000 hover | 57.59 † | 46.40 † | 0.00 † | 10.26 † | 1.00 † |
| freya | rows 3,000 hover | 15.12 | 4.47 | 0.00 | 9.66 | 1.00 |
| freya-056a5b4 | rows 3,000 hover | 15.12 | 4.49 | 0.00 | 9.63 | 1.00 |
| vizia | rows 3,000 hover | 89.34 | 79.78 | 0.00 | 8.15 | 1.00 |
| iced | rows 3,000 hover | 14.02 | 3.74 | 0.00 | 9.31 | 1.00 |
| egui | rows 3,000 hover | 28.19 | 15.59 | 0.00 | 11.05 | 1.00 |
| dioxus | rows 3,000 hover | 24.25 † | 13.85 † | 0.00 † | 9.42 † | 1.00 † |
| dioxus-skia | rows 3,000 hover | 15.12 | 5.81 | 0.00 | 8.28 | 1.00 |
| bough | rows 3,000 hover | 11.52 | 0.93 | 0.00 | 9.58 | 1.00 |
| zgui | table fine update10 | 15.95 | 8.72 | 0.00 | 6.11 | 1.00 |
| gpui-kit | table fine update10 | 31.36 † | 26.15 † | 0.00 † | 4.23 † | 1.00 † |
| gpui-kit-fast | table fine update10 | 34.43 † | 26.93 † | 0.00 † | 6.68 † | 1.00 † |
| freya | table fine update10 | 14.69 | 6.50 | 0.00 | 7.20 | 1.00 |
| freya-056a5b4 | table fine update10 | 14.70 | 6.65 | 0.00 | 6.93 | 1.00 |
| vizia | table fine update10 | 32.60 | 27.23 | 0.00 | 4.37 | 1.00 |
| iced | table fine update10 | 14.68 | 5.60 | 0.00 | 8.10 | 1.00 |
| egui | table fine update10 | 15.93 | 8.73 | 0.00 | 6.03 | 1.00 |
| dioxus | table fine update10 | 28.80 † | 16.38 † | 0.00 † | 10.99 † | 1.00 † |
| dioxus-skia | table fine update10 | 14.69 | 6.54 | 0.00 | 7.36 | 1.00 |
| bough | table fine update10 | 12.10 | 0.44 | 0.00 | 10.66 | 1.00 |
| zgui | table fine remove | 31.36 † | 25.02 † | 0.00 † | 5.12 † | 1.00 † |
| gpui-kit | table fine remove | 49.26 † | 38.93 † | 0.00 † | 9.40 † | 1.00 † |
| gpui-kit-fast | table fine remove | 47.28 | 34.83 | 0.00 | 11.47 | 1.00 |
| freya | table fine remove | 14.69 | 6.86 | 0.00 | 6.71 | 1.00 |
| freya-056a5b4 | table fine remove | 14.69 | 6.74 | 0.00 | 6.95 | 1.00 |
| vizia | table fine remove | 343.9 | 332.6 | 0.00 | 9.33 | 1.00 |
| iced | table fine remove | 20.12 | 9.56 | 0.00 | 11.17 | 1.00 |
| egui | table fine remove | 14.68 | 7.64 | 0.00 | 6.01 | 1.00 |
| dioxus | table fine remove | 28.80 | 18.28 | 0.00 | 8.33 | 1.00 |
| dioxus-skia | table fine remove | 15.92 | 8.94 | 0.00 | 6.04 | 1.00 |
| bough | table fine remove | 14.67 | 4.22 | 0.00 | 9.45 | 1.00 |
| zgui | table fine swap | 31.36 † | 25.45 † | 0.00 † | 4.30 † | 1.00 † |
| gpui-kit | table fine swap | 31.37 | 20.80 | 0.00 | 8.12 | 1.00 |
| gpui-kit-fast | table fine swap | 32.59 † | 24.95 † | 0.00 † | 6.78 † | 1.00 † |
| freya | table fine swap | 14.69 | 7.13 | 0.00 | 6.64 | 1.00 |
| freya-056a5b4 | table fine swap | 15.93 | 7.32 | 0.00 | 7.61 | 1.00 |
| vizia | table fine swap | 241.6 | 231.6 | 0.00 | 6.73 | 1.00 |
| iced | table fine swap | 14.68 | 4.84 | 0.00 | 8.80 | 1.00 |
| egui | table fine swap | 15.93 | 7.64 | 0.00 | 7.32 | 1.00 |
| dioxus | table fine swap | 30.60 | 18.36 | 0.00 | 11.19 | 1.00 |
| dioxus-skia | table fine swap | 15.92 | 8.88 | 0.00 | 6.38 | 1.00 |
| bough | table fine swap | 12.10 | 0.59 | 0.00 | 10.54 | 1.00 |
| zgui | search filter | 85.96 † | 75.79 † | 0.00 † | 10.64 † | 1.00 † |
| gpui-kit | search filter | 58.36 † | 44.79 † | 0.00 † | 11.56 † | 1.00 † |
| gpui-kit-fast | search filter | 57.57 † | 49.04 † | 0.00 † | 7.17 † | 1.00 † |
| freya | search filter | 54.20 | 42.39 | 0.00 | 8.74 | 1.00 |
| freya-056a5b4 | search filter | 55.72 | 46.61 | 0.00 | 6.90 | 1.00 |
| vizia | search filter | 148.0 | 139.8 | 0.00 | 9.75 | 1.00 |
| iced | search filter | 23.45 | 12.90 | 0.00 | 8.43 | 1.00 |
| egui | search filter | 25.79 † | 15.24 † | 0.00 † | 8.70 † | 1.00 † |
| dioxus | search filter | 71.61 † | 61.79 † | 0.00 † | 9.08 † | 1.00 † |
| dioxus-skia | search filter | 40.18 † | 28.32 † | 0.00 † | 9.86 † | 1.00 † |
| bough | search filter | 15.93 | 6.38 | 0.00 | 10.01 | 1.00 |

# VIEW Variable refresh (cmp-v1, heavy-v1; timing vrr 48-144 Hz)

## Where bough wins and loses, headline cells

Lowest valid value in 30 metrics, behind another framework in 3.

Behind (valid measurements only):

- spinner (no rows) fps (fps): bough 62.5, best valid freya 144.0
- spinner 10,000 fps (fps): bough 62.5, best valid freya-056a5b4 140.6
- ticker fine 100 updates fps (fps): bough 62.5, best valid egui 62.5

## Where bough wins and loses, heavy scenes (four measured frameworks)

Lowest valid value in 9 metrics, behind another framework in 3.

Behind (valid measurements only):

- sheet 10,000 rows theme switch total p50 (ms): bough 23.73, best valid egui 10.74
- sheet 10,000 rows edit total p50 (ms): bough 14.28, best valid egui 9.59
- sheet 10,000 rows idle RSS (MiB): bough 331.3, best valid egui 149.1

## Headline table (the cmp-1 columns)

| framework | rows 3,000 hover total p50 (ms) | rows 3,000 hover input to commit p50 (ms) | rows 3,000 startup to first present (ms) | rows 3,000 idle RSS (MiB) | rows 3,000 idle CPU (ms/s) | rows 3,000 scroll CPU per frame (ms) | typing p50 (ms) | resize configure to present p50 (ms) | spinner 10,000 CPU per frame (ms) | table fine remove total p50 (ms) | table fine remove CPU per frame (ms) |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| zgui | 7.43 | 6.69 | - | 315.7 | 98.55 | 19.29 † | 3.82 | 25.84 † | 12.69 † | 43.33 † | 254.4 † |
| gpui-kit | 110.6 † | 109.3 † | - | 249.8 | 119.3 | 116.8 † | 45.31 † | 68.07 † | 346.4 † | 57.89 | 175.3 |
| gpui-kit-fast | 68.01 † | 66.38 † | - | 268.4 | 112.1 | 149.3 † | 38.71 † | 55.82 † | 244.6 † | 59.24 † | 195.6 † |
| freya | 5.74 | 5.53 | - | 250.1 | 12.42 | 28.19 | 7.56 † | 44.01 | 17.87 | 11.09 † | 19.42 † |
| freya-056a5b4 | 6.96 | 6.70 | - | 249.9 | 10.67 | 22.52 | 7.70 † | 42.72 | 17.22 | 13.90 | 24.39 |
| vizia | 87.29 | 85.76 | - | 273.2 | 21.69 | 177.9 | 30.89 † | 54.16 † | 20.77 | 388.2 † | 296.2 † |
| floem | *~3.60* (failed cell) | *~3.30* (failed cell) | *~140.0* | *~200.0* (failed cell) | *~1.90* (failed cell) | *~9.90* (failed cell) | *~2.10* (failed cell) | *~5.00* (failed cell) | *~6.80* (failed cell) | *~6.20* (failed cell) | *~13.00* (failed cell) |
| iced | 6.72 | 6.35 | - | 168.7 | 10.67 | 20.88 | 6.47 † | 7.21 | 85.37 † | 16.28 | 28.85 |
| egui | 20.24 | 19.75 | - | 168.6 | 20.99 | 37.11 † | 10.51 † | 9.48 | 79.49 † | 14.04 † | 30.69 † |
| dioxus | 25.54 † | 23.19 † | - | 219.7 | 48.83 | 85.30 † | 20.29 † | 25.48 | 79.08 † | 27.53 † | 82.69 † |
| dioxus-skia | 9.19 | 8.98 | - | 191.5 | 9.77 | 22.61 † | 5.55 | 10.77 † | 30.04 † | 15.57 | 23.39 |
| bough | 4.10 | 1.11 | - | 34.9 | 0.03 | 3.98 † | 0.34 † | 6.75 | 1.18 | 4.58 | 3.94 |

## Startup, memory, idle

| framework | rows 3,000 startup to first present (ms) | virtual 100,000 startup to first present (ms) | spinner (no rows) startup to first present (ms) | rows 3,000 idle RSS (MiB) | virtual 100,000 idle RSS (MiB) | spinner (no rows) RSS (MiB) | rows 3,000 idle CPU (ms/s) | spinner (no rows) idle CPU (ms/s) |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| zgui | - | - | - | 315.7 | 176.7 | 178.6 | 98.55 | 506.6 |
| gpui-kit | - | - | - | 249.8 | 178.7 | 179.9 | 119.3 | 4,573 |
| gpui-kit-fast | - | - | - | 268.4 | 199.7 | 200.1 | 112.1 | 5,720 |
| freya | - | - | - | 250.1 | 140.7 | 139.5 | 12.42 | 500.4 |
| freya-056a5b4 | - | - | - | 249.9 | 140.1 | 139.9 | 10.67 | 465.6 |
| vizia | - | - | - | 273.2 | 146.0 | 150.6 | 21.69 | 440.9 |
| floem | *~140.0* | *~57.0* | - | *~200.0* (failed cell) | *~130.0* (failed cell) | 127.0 | *~1.90* (failed cell) | 105.5 |
| iced | - | - | - | 168.7 | 142.5 | 167.0 | 10.67 | 3,985 |
| egui | - | - | - | 168.6 | 143.5 | 142.5 | 20.99 | 605.6 |
| dioxus | - | - | - | 219.7 | 176.6 | 174.9 | 48.83 | 2,704 |
| dioxus-skia | - | - | - | 191.5 | 147.6 | 158.9 | 9.77 | 370.8 |
| bough | - | - | - | 34.9 | 18.8 | 19.3 | 0.03 | 7.11 |

## Hover (rows)

| framework | rows 3,000 hover total p50 (ms) | rows 3,000 hover input to commit p50 (ms) | rows 100 hover total p50 (ms) | rows 100 hover input to commit p50 (ms) | rows 3,000 hover CPU per frame (ms) | rows 100 hover CPU per frame (ms) |
|---|---:|---:|---:|---:|---:|---:|
| zgui | 7.43 | 6.69 | 5.52 | 4.73 | 25.22 | 23.60 |
| gpui-kit | 110.6 † | 109.3 † | 20.43 † | 17.69 † | 202.3 † | 89.23 † |
| gpui-kit-fast | 68.01 † | 66.38 † | 25.94 † | 24.90 † | 174.8 † | 123.3 † |
| freya | 5.74 | 5.53 | 6.32 † | 5.57 † | 18.38 | 14.81 † |
| freya-056a5b4 | 6.96 | 6.70 | 6.37 † | 6.04 † | 18.78 | 13.26 † |
| vizia | 87.29 | 85.76 | 7.12 | 7.02 | 106.2 | 14.54 |
| floem | *~3.60* (failed cell) | *~3.30* (failed cell) | *~2.80* (failed cell) | *~2.30* (failed cell) | *~7.30* (failed cell) | *~5.60* (failed cell) |
| iced | 6.72 | 6.35 | 4.96 | 3.79 | 15.48 | 12.99 |
| egui | 20.24 | 19.75 | 8.54 | 8.55 | 36.49 | 25.16 |
| dioxus | 25.54 † | 23.19 † | 16.83 | 16.68 | 80.85 † | 65.81 |
| dioxus-skia | 9.19 | 8.98 | 6.19 | 5.87 | 17.92 | 13.41 |
| bough | 4.10 | 1.11 | 4.10 | 0.16 | 1.12 | 0.15 |

## Scrolling

| framework | rows 3,000 scroll CPU per frame (ms) | rows 3,000 scroll fps (fps) | virtual 100,000 scroll CPU per frame (ms) |
|---|---:|---:|---:|
| zgui | 19.29 † | 120.3 † | 17.60 |
| gpui-kit | 116.8 † | 9.4 † | 84.52 |
| gpui-kit-fast | 149.3 † | 15.6 † | 124.4 † |
| freya | 28.19 | 68.3 | 16.06 |
| freya-056a5b4 | 22.52 | 101.0 | 15.71 |
| vizia | 177.9 | 5.9 | 21.61 |
| floem | *~9.90* (failed cell) | *~34.0* (failed cell) | *~6.20* (failed cell) |
| iced | 20.88 | 130.4 | 20.25 |
| egui | 37.11 † | 42.5 † | 20.69 † |
| dioxus | 85.30 † | 41.9 † | 61.70 |
| dioxus-skia | 22.61 † | 87.9 † | 17.09 |
| bough | 3.98 † | 144.0 † | 2.11 |

## Typing, search, resize

| framework | typing p50 (ms) | typing handling p50 (app share) (ms) | typing CPU per frame (ms) | search filter total p50 (ms) | search filter input to commit p50 (ms) | search filter CPU per frame (ms) | resize configure to present p50 (ms) | resize configure to commit p50 (app share) (ms) | resize CPU per frame (ms) |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| zgui | 3.82 | 3.72 | 15.01 | 69.30 † | 69.31 † | 253.7 † | 25.84 † | 25.66 † | 42.35 † |
| gpui-kit | 45.31 † | 44.24 † | 132.1 † | 41.41 | 40.37 | 111.0 | 68.07 † | 68.09 † | 137.2 † |
| gpui-kit-fast | 38.71 † | 37.52 † | 168.0 † | 44.50 † | 42.70 † | 141.9 † | 55.82 † | 54.93 † | 148.8 † |
| freya | 7.56 † | 7.39 † | 17.52 † | 41.26 | 40.64 | 52.69 | 44.01 | 43.77 | 35.24 |
| freya-056a5b4 | 7.70 † | 7.58 † | 17.54 † | 39.14 | 38.49 | 53.98 | 42.72 | 42.31 | 34.38 |
| vizia | 30.89 † | 29.75 † | 43.32 † | 114.3 † | 114.3 † | 13.88 † | 54.16 † | 54.17 † | 82.15 † |
| floem | *~2.10* (failed cell) | *~2.00* (failed cell) | *~5.70* (failed cell) | *~10.00* (failed cell) | *~9.80* (failed cell) | *~7.70* (failed cell) | *~5.00* (failed cell) | *~4.90* (failed cell) | *~11.00* (failed cell) |
| iced | 6.47 † | 6.27 † | 17.02 † | 11.63 | 10.78 | 15.80 | 7.21 | 7.22 | 22.74 |
| egui | 10.51 † | 10.18 † | 26.26 † | 12.80 | 11.13 | 11.55 | 9.48 | 9.48 | 24.59 |
| dioxus | 20.29 † | 19.43 † | 70.34 † | 51.42 | 49.91 | 72.16 | 25.48 | 24.73 | 51.31 |
| dioxus-skia | 5.55 | 5.18 | 17.12 | 25.34 | 24.73 | 17.24 | 10.77 † | 10.78 † | 17.48 † |
| bough | 0.34 † | 0.33 † | 0.36 † | 7.68 | 7.11 | 7.65 | 6.75 | 6.76 | 6.80 |

## Table operations (1,000 rows, per-row state; egui has its one variant)

| framework | table fine remove total p50 (ms) | table fine remove input to commit p50 (ms) | table fine remove CPU per frame (ms) | table fine update10 total p50 (ms) | table fine update10 input to commit p50 (ms) | table fine update10 CPU per frame (ms) | table fine swap total p50 (ms) | table fine swap input to commit p50 (ms) | table fine swap CPU per frame (ms) |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| zgui | 43.33 † | 42.09 † | 254.4 † | 19.16 † | 16.82 † | 48.93 † | 49.28 † | 45.41 † | 254.2 † |
| gpui-kit | 57.89 | 57.31 | 175.3 | 42.50 † | 41.25 † | 155.2 † | 36.67 † | 36.18 † | 139.1 † |
| gpui-kit-fast | 59.24 † | 59.13 † | 195.6 † | 38.30 † | 36.96 † | 175.9 † | 40.53 | 40.55 | 181.3 |
| freya | 11.09 † | 10.51 † | 19.42 † | 13.23 | 11.67 | 20.60 | 11.39 | 10.13 | 24.35 |
| freya-056a5b4 | 13.90 | 12.99 | 24.39 | 13.22 | 11.59 | 23.96 | 15.86 † | 13.75 † | 24.55 † |
| vizia | 388.2 † | 387.1 † | 296.2 † | 33.52 | 33.53 | 54.07 | 289.1 | 286.9 | 423.5 |
| floem | *~6.20* (failed cell) | *~5.60* (failed cell) | *~13.00* (failed cell) | *~4.70* (failed cell) | *~4.10* (failed cell) | *~9.30* (failed cell) | *~6.80* (failed cell) | *~6.60* (failed cell) | *~9.40* (failed cell) |
| iced | 16.28 | 13.76 | 28.85 | 9.41 † | 8.20 † | 20.96 † | 9.63 † | 8.04 † | 22.19 † |
| egui | 14.04 † | 14.02 † | 30.69 † | 12.01 | 12.02 | 24.63 | 14.38 | 14.39 | 28.55 |
| dioxus | 27.53 † | 27.51 † | 82.69 † | 26.03 † | 26.05 † | 88.81 † | 28.12 † | 27.16 † | 90.06 † |
| dioxus-skia | 15.57 | 13.98 | 23.39 | 11.73 | 10.38 | 24.09 | 17.48 | 16.36 | 24.25 |
| bough | 4.58 | 3.98 | 3.94 | 0.44 | 0.44 | 0.45 | 0.58 | 0.55 | 0.58 |

## Ticker (100 row updates per tick)

| framework | ticker fine 100 updates CPU per frame (ms) | ticker fine 100 updates fps (fps) |
|---|---:|---:|
| zgui | 30.00 † | 57.9 † |
| gpui-kit | 116.7 † | 30.6 † |
| gpui-kit-fast | 167.2 † | 22.3 † |
| freya | 21.44 † | 59.6 † |
| freya-056a5b4 | 20.87 † | 60.0 † |
| vizia | 48.41 † | 29.4 † |
| floem | *~7.10* (failed cell) | *~26.0* (failed cell) |
| iced | 23.91 † | 60.6 † |
| egui | 26.29 | 62.5 |
| dioxus | 69.58 † | 52.9 † |
| dioxus-skia | 18.19 | 57.9 |
| bough | 0.19 | 62.5 |

## Spinner

| framework | spinner (no rows) CPU per frame (ms) | spinner 10,000 CPU per frame (ms) | spinner (no rows) fps (fps) | spinner 10,000 fps (fps) |
|---|---:|---:|---:|---:|
| zgui | 13.74 † | 12.69 † | 59.7 † | 59.9 † |
| gpui-kit | 59.72 † | 346.4 † | 102.4 † | 3.7 † |
| gpui-kit-fast | 91.36 | 244.6 † | 73.3 | 8.5 † |
| freya | 3.81 | 17.87 | 144.0 | 139.2 |
| freya-056a5b4 | 3.82 | 17.22 | 144.0 | 140.6 |
| vizia | 7.21 | 20.77 | 62.5 | 61.1 |
| floem | 1.48 | *~6.80* (failed cell) | 59.9 | *~58.0* (failed cell) |
| iced | 34.39 | 85.37 † | 138.2 | 20.3 † |
| egui | 3.46 | 79.49 † | 144.0 | 15.4 † |
| dioxus | 52.35 † | 79.08 † | 80.2 † | 34.8 † |
| dioxus-skia | 7.07 | 30.04 † | 144.0 | 52.7 † |
| bough | 0.10 | 1.18 | 62.5 | 62.5 |

## Estimates, with method per value

| column | framework | estimate | range | basis |
|---|---|---:|---|---|
| rows 3,000 hover total p50 (ms) | floem | ~3.60 | 0.46 to 28.00 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 8 frameworks |
| rows 3,000 hover input to commit p50 (ms) | floem | ~3.30 | 0.43 to 26.00 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 8 frameworks |
| rows 100 hover total p50 (ms) | floem | ~2.80 | 0.35 to 21.00 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 7 frameworks |
| rows 3,000 idle RSS (MiB) | floem | ~200.0 | 150.0 to 270.0 | own: 2 own valid cells in class 'resident memory (MiB)' (log-level -0.14 against the column medians); column level from 11 frameworks |
| rows 3,000 idle CPU (ms/s) | floem | ~1.90 | 1.40 to 2.60 | own: 2 own valid cells in class 'idle CPU (ms/s)' (log-level -2.09 against the column medians); column level from 11 frameworks |
| rows 3,000 scroll CPU per frame (ms) | floem | ~9.90 | 7.40 to 13.00 | own: 2 own valid cells in class 'cpu per frame (ms)' (log-level -0.95 against the column medians); column level from 4 frameworks |
| rows 3,000 scroll fps (fps) | floem | ~34.0 | 25.0 to 46.0 | own: 2 own valid cells in class 'frame rate (fps)' (log-level -0.84 against the column medians); column level from 4 frameworks |
| virtual 100,000 scroll CPU per frame (ms) | floem | ~6.20 | 4.60 to 8.40 | own: 2 own valid cells in class 'cpu per frame (ms)' (log-level -0.95 against the column medians); column level from 9 frameworks |
| virtual 100,000 idle RSS (MiB) | floem | ~130.0 | 94.0 to 170.0 | own: 2 own valid cells in class 'resident memory (MiB)' (log-level -0.14 against the column medians); column level from 11 frameworks |
| typing p50 (ms) | floem | ~2.10 | 0.26 to 16.00 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 2 frameworks |
| search filter total p50 (ms) | floem | ~10.00 | 1.30 to 79.00 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 8 frameworks |
| resize configure to present p50 (ms) | floem | ~5.00 | 0.64 to 39.00 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 6 frameworks |
| spinner 10,000 CPU per frame (ms) | floem | ~6.80 | 5.00 to 9.20 | own: 2 own valid cells in class 'cpu per frame (ms)' (log-level -0.95 against the column medians); column level from 4 frameworks |
| spinner 10,000 fps (fps) | floem | ~58.0 | 43.0 to 78.0 | own: 2 own valid cells in class 'frame rate (fps)' (log-level -0.84 against the column medians); column level from 4 frameworks |
| table fine remove total p50 (ms) | floem | ~6.20 | 0.80 to 48.00 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 5 frameworks |
| table fine remove CPU per frame (ms) | floem | ~13.00 | 9.30 to 17.00 | own: 2 own valid cells in class 'cpu per frame (ms)' (log-level -0.95 against the column medians); column level from 5 frameworks |
| table fine update10 total p50 (ms) | floem | ~4.70 | 0.60 to 37.00 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 6 frameworks |
| table fine swap total p50 (ms) | floem | ~6.80 | 0.87 to 53.00 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 6 frameworks |
| ticker fine 100 updates CPU per frame (ms) | floem | ~7.10 | 5.20 to 9.60 | own: 2 own valid cells in class 'cpu per frame (ms)' (log-level -0.95 against the column medians); column level from 3 frameworks |
| ticker fine 100 updates fps (fps) | floem | ~26.0 | 19.0 to 35.0 | own: 2 own valid cells in class 'frame rate (fps)' (log-level -0.84 against the column medians); column level from 3 frameworks |
| rows 100 hover input to commit p50 (ms) | floem | ~2.30 | 0.30 to 18.00 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 7 frameworks |
| table fine remove input to commit p50 (ms) | floem | ~5.60 | 0.71 to 43.00 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 5 frameworks |
| table fine update10 input to commit p50 (ms) | floem | ~4.10 | 0.53 to 32.00 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 6 frameworks |
| table fine swap input to commit p50 (ms) | floem | ~6.60 | 0.85 to 52.00 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 6 frameworks |
| search filter input to commit p50 (ms) | floem | ~9.80 | 1.30 to 77.00 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 8 frameworks |
| typing handling p50 (app share) (ms) | floem | ~2.00 | 0.25 to 15.00 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 2 frameworks |
| resize configure to commit p50 (app share) (ms) | floem | ~4.90 | 0.63 to 39.00 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 6 frameworks |
| rows 3,000 hover CPU per frame (ms) | floem | ~7.30 | 5.40 to 9.80 | own: 2 own valid cells in class 'cpu per frame (ms)' (log-level -0.95 against the column medians); column level from 8 frameworks |
| rows 100 hover CPU per frame (ms) | floem | ~5.60 | 4.20 to 7.60 | own: 2 own valid cells in class 'cpu per frame (ms)' (log-level -0.95 against the column medians); column level from 7 frameworks |
| table fine update10 CPU per frame (ms) | floem | ~9.30 | 6.90 to 13.00 | own: 2 own valid cells in class 'cpu per frame (ms)' (log-level -0.95 against the column medians); column level from 6 frameworks |
| table fine swap CPU per frame (ms) | floem | ~9.40 | 7.00 to 13.00 | own: 2 own valid cells in class 'cpu per frame (ms)' (log-level -0.95 against the column medians); column level from 6 frameworks |
| typing CPU per frame (ms) | floem | ~5.70 | 4.20 to 7.60 | own: 2 own valid cells in class 'cpu per frame (ms)' (log-level -0.95 against the column medians); column level from 2 frameworks |
| search filter CPU per frame (ms) | floem | ~7.70 | 5.70 to 10.00 | own: 2 own valid cells in class 'cpu per frame (ms)' (log-level -0.95 against the column medians); column level from 8 frameworks |
| resize CPU per frame (ms) | floem | ~11.00 | 8.50 to 15.00 | own: 2 own valid cells in class 'cpu per frame (ms)' (log-level -0.95 against the column medians); column level from 6 frameworks |
| rows 3,000 startup to first present (ms) | floem | ~140.0 | 18.0 to 1,100 | cross-class: only 1 own valid cell(s) in class 'startup (ms)': level taken across all classes (cross-class, low confidence); column level from 11 frameworks |
| virtual 100,000 startup to first present (ms) | floem | ~57.0 | 7.3 to 450.0 | cross-class: only 1 own valid cell(s) in class 'startup (ms)': level taken across all classes (cross-class, low confidence); column level from 11 frameworks |
| sheet 1,000 rows hover total p50 (ms) | dioxus | ~19.00 | 14.00 to 26.00 | own: 6 own valid cells in class 'latency (ms)' (log-level +0.79 against the column medians); column level from 1 frameworks |
| sheet 1,000 rows hover total p50 (ms) | dioxus-skia | ~9.40 | 6.90 to 13.00 | own: 14 own valid cells in class 'latency (ms)' (log-level +0.08 against the column medians); column level from 1 frameworks |
| sheet 1,000 rows hover total p50 (ms) | floem | ~3.70 | 0.48 to 29.00 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 1 frameworks |
| sheet 1,000 rows hover total p50 (ms) | freya | ~11.00 | 2.20 to 50.00 | own: 10 own valid cells in class 'latency (ms)' (log-level +0.20 against the column medians); column level from 1 frameworks |
| sheet 1,000 rows hover total p50 (ms) | freya-056a5b4 | ~10.00 | 4.00 to 27.00 | own: 10 own valid cells in class 'latency (ms)' (log-level +0.20 against the column medians); column level from 1 frameworks |
| sheet 1,000 rows hover total p50 (ms) | gpui-kit-fast | ~23.00 | 17.00 to 30.00 | own: 2 own valid cells in class 'latency (ms)' (log-level +0.96 against the column medians); column level from 1 frameworks |
| sheet 1,000 rows hover total p50 (ms) | vizia | ~52.00 | 3.70 to 730.0 | own: 8 own valid cells in class 'latency (ms)' (log-level +1.81 against the column medians); column level from 1 frameworks |
| sheet 1,000 rows hover total p50 (ms) | zgui | ~7.50 | 5.50 to 10.00 | own: 6 own valid cells in class 'latency (ms)' (log-level -0.14 against the column medians); column level from 1 frameworks |
| sheet 1,000 rows hover input to commit p50 (ms) | dioxus | ~9.50 | 7.00 to 13.00 | own: 6 own valid cells in class 'latency (ms)' (log-level +0.79 against the column medians); column level from 1 frameworks |
| sheet 1,000 rows hover input to commit p50 (ms) | dioxus-skia | ~4.70 | 3.50 to 6.30 | own: 14 own valid cells in class 'latency (ms)' (log-level +0.08 against the column medians); column level from 1 frameworks |
| sheet 1,000 rows hover input to commit p50 (ms) | floem | ~1.90 | 0.24 to 15.00 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 1 frameworks |
| sheet 1,000 rows hover input to commit p50 (ms) | freya | ~5.30 | 1.10 to 25.00 | own: 10 own valid cells in class 'latency (ms)' (log-level +0.20 against the column medians); column level from 1 frameworks |
| sheet 1,000 rows hover input to commit p50 (ms) | freya-056a5b4 | ~5.30 | 2.00 to 14.00 | own: 10 own valid cells in class 'latency (ms)' (log-level +0.20 against the column medians); column level from 1 frameworks |
| sheet 1,000 rows hover input to commit p50 (ms) | gpui-kit-fast | ~11.00 | 8.40 to 15.00 | own: 2 own valid cells in class 'latency (ms)' (log-level +0.96 against the column medians); column level from 1 frameworks |
| sheet 1,000 rows hover input to commit p50 (ms) | vizia | ~26.00 | 1.90 to 370.0 | own: 8 own valid cells in class 'latency (ms)' (log-level +1.81 against the column medians); column level from 1 frameworks |
| sheet 1,000 rows hover input to commit p50 (ms) | zgui | ~3.70 | 2.80 to 5.10 | own: 6 own valid cells in class 'latency (ms)' (log-level -0.14 against the column medians); column level from 1 frameworks |
| sheet 1,000 rows theme switch total p50 (ms) | dioxus | ~26.00 | 19.00 to 35.00 | own: 6 own valid cells in class 'latency (ms)' (log-level +0.79 against the column medians); column level from 3 frameworks |
| sheet 1,000 rows theme switch total p50 (ms) | dioxus-skia | ~13.00 | 9.40 to 17.00 | own: 14 own valid cells in class 'latency (ms)' (log-level +0.08 against the column medians); column level from 3 frameworks |
| sheet 1,000 rows theme switch total p50 (ms) | floem | ~5.10 | 0.65 to 39.00 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 3 frameworks |
| sheet 1,000 rows theme switch total p50 (ms) | freya | ~14.00 | 3.00 to 69.00 | own: 10 own valid cells in class 'latency (ms)' (log-level +0.20 against the column medians); column level from 3 frameworks |
| sheet 1,000 rows theme switch total p50 (ms) | freya-056a5b4 | ~14.00 | 5.40 to 37.00 | own: 10 own valid cells in class 'latency (ms)' (log-level +0.20 against the column medians); column level from 3 frameworks |
| sheet 1,000 rows theme switch total p50 (ms) | gpui-kit-fast | ~31.00 | 23.00 to 41.00 | own: 2 own valid cells in class 'latency (ms)' (log-level +0.96 against the column medians); column level from 3 frameworks |
| sheet 1,000 rows theme switch total p50 (ms) | vizia | ~71.00 | 5.10 to 1,000 | own: 8 own valid cells in class 'latency (ms)' (log-level +1.81 against the column medians); column level from 3 frameworks |
| sheet 1,000 rows theme switch total p50 (ms) | zgui | ~10.00 | 7.50 to 14.00 | own: 6 own valid cells in class 'latency (ms)' (log-level -0.14 against the column medians); column level from 3 frameworks |
| sheet 1,000 rows edit total p50 (ms) | dioxus | ~34.00 | 26.00 to 47.00 | own: 6 own valid cells in class 'latency (ms)' (log-level +0.79 against the column medians); column level from 2 frameworks |
| sheet 1,000 rows edit total p50 (ms) | dioxus-skia | ~17.00 | 13.00 to 23.00 | own: 14 own valid cells in class 'latency (ms)' (log-level +0.08 against the column medians); column level from 2 frameworks |
| sheet 1,000 rows edit total p50 (ms) | floem | ~6.80 | 0.87 to 53.00 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 2 frameworks |
| sheet 1,000 rows edit total p50 (ms) | freya | ~19.00 | 4.00 to 92.00 | own: 10 own valid cells in class 'latency (ms)' (log-level +0.20 against the column medians); column level from 2 frameworks |
| sheet 1,000 rows edit total p50 (ms) | freya-056a5b4 | ~19.00 | 7.30 to 50.00 | own: 10 own valid cells in class 'latency (ms)' (log-level +0.20 against the column medians); column level from 2 frameworks |
| sheet 1,000 rows edit total p50 (ms) | gpui-kit-fast | ~41.00 | 30.00 to 55.00 | own: 2 own valid cells in class 'latency (ms)' (log-level +0.96 against the column medians); column level from 2 frameworks |
| sheet 1,000 rows edit total p50 (ms) | vizia | ~95.00 | 6.80 to 1,300 | own: 8 own valid cells in class 'latency (ms)' (log-level +1.81 against the column medians); column level from 2 frameworks |
| sheet 1,000 rows edit total p50 (ms) | zgui | ~14.00 | 10.00 to 18.00 | own: 6 own valid cells in class 'latency (ms)' (log-level -0.14 against the column medians); column level from 2 frameworks |
| sheet 1,000 rows resize configure to present p50 (ms) | dioxus | ~30.0 | 22.0 to 41.0 | own: 6 own valid cells in class 'latency (ms)' (log-level +0.79 against the column medians); column level from 2 frameworks |
| sheet 1,000 rows resize configure to present p50 (ms) | dioxus-skia | ~15.0 | 11.0 to 20.0 | own: 14 own valid cells in class 'latency (ms)' (log-level +0.08 against the column medians); column level from 2 frameworks |
| sheet 1,000 rows resize configure to present p50 (ms) | floem | ~5.9 | 0.8 to 46.0 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 2 frameworks |
| sheet 1,000 rows resize configure to present p50 (ms) | freya | ~17.0 | 3.5 to 81.0 | own: 10 own valid cells in class 'latency (ms)' (log-level +0.20 against the column medians); column level from 2 frameworks |
| sheet 1,000 rows resize configure to present p50 (ms) | freya-056a5b4 | ~17.0 | 6.4 to 44.0 | own: 10 own valid cells in class 'latency (ms)' (log-level +0.20 against the column medians); column level from 2 frameworks |
| sheet 1,000 rows resize configure to present p50 (ms) | gpui-kit-fast | ~36.0 | 27.0 to 49.0 | own: 2 own valid cells in class 'latency (ms)' (log-level +0.96 against the column medians); column level from 2 frameworks |
| sheet 1,000 rows resize configure to present p50 (ms) | vizia | ~83.0 | 6.0 to 1,200 | own: 8 own valid cells in class 'latency (ms)' (log-level +1.81 against the column medians); column level from 2 frameworks |
| sheet 1,000 rows resize configure to present p50 (ms) | zgui | ~12.0 | 8.8 to 16.0 | own: 6 own valid cells in class 'latency (ms)' (log-level -0.14 against the column medians); column level from 2 frameworks |
| sheet 1,000 rows idle RSS (MiB) | dioxus | ~200.0 | 150.0 to 270.0 | own: 3 own valid cells in class 'resident memory (MiB)' (log-level +0.18 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows idle RSS (MiB) | dioxus-skia | ~170.0 | 130.0 to 230.0 | own: 4 own valid cells in class 'resident memory (MiB)' (log-level +0.04 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows idle RSS (MiB) | floem | ~140.0 | 110.0 to 200.0 | own: 2 own valid cells in class 'resident memory (MiB)' (log-level -0.14 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows idle RSS (MiB) | freya | ~160.0 | 120.0 to 210.0 | own: 5 own valid cells in class 'resident memory (MiB)' (log-level -0.05 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows idle RSS (MiB) | freya-056a5b4 | ~160.0 | 120.0 to 210.0 | own: 5 own valid cells in class 'resident memory (MiB)' (log-level -0.05 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows idle RSS (MiB) | gpui-kit-fast | ~230.0 | 170.0 to 310.0 | own: 4 own valid cells in class 'resident memory (MiB)' (log-level +0.31 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows idle RSS (MiB) | vizia | ~170.0 | 130.0 to 230.0 | own: 5 own valid cells in class 'resident memory (MiB)' (log-level +0.03 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows idle RSS (MiB) | zgui | ~200.0 | 150.0 to 270.0 | own: 3 own valid cells in class 'resident memory (MiB)' (log-level +0.20 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows idle CPU (ms/s) | dioxus | ~42.00 | 31.00 to 57.00 | own: 2 own valid cells in class 'idle CPU (ms/s)' (log-level +1.22 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows idle CPU (ms/s) | dioxus-skia | ~8.00 | 3.60 to 18.00 | own: 3 own valid cells in class 'idle CPU (ms/s)' (log-level -0.45 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows idle CPU (ms/s) | floem | ~1.50 | 1.10 to 2.10 | own: 2 own valid cells in class 'idle CPU (ms/s)' (log-level -2.09 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows idle CPU (ms/s) | freya | ~8.70 | 6.40 to 12.00 | own: 4 own valid cells in class 'idle CPU (ms/s)' (log-level -0.36 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows idle CPU (ms/s) | freya-056a5b4 | ~8.60 | 6.40 to 12.00 | own: 4 own valid cells in class 'idle CPU (ms/s)' (log-level -0.37 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows idle CPU (ms/s) | gpui-kit-fast | ~94.00 | 69.00 to 130.0 | own: 3 own valid cells in class 'idle CPU (ms/s)' (log-level +2.02 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows idle CPU (ms/s) | vizia | ~7.10 | 3.50 to 14.00 | own: 4 own valid cells in class 'idle CPU (ms/s)' (log-level -0.56 against the column medians); column level from 4 frameworks |
| sheet 1,000 rows idle CPU (ms/s) | zgui | ~26.00 | 0.89 to 750.0 | own: 2 own valid cells in class 'idle CPU (ms/s)' (log-level +0.73 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows hover total p50 (ms) | dioxus | ~73.00 | 54.00 to 99.00 | own: 6 own valid cells in class 'latency (ms)' (log-level +0.79 against the column medians); column level from 1 frameworks |
| sheet 10,000 rows hover total p50 (ms) | dioxus-skia | ~36.00 | 27.00 to 49.00 | own: 14 own valid cells in class 'latency (ms)' (log-level +0.08 against the column medians); column level from 1 frameworks |
| sheet 10,000 rows hover total p50 (ms) | floem | ~14.00 | 1.90 to 110.0 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 1 frameworks |
| sheet 10,000 rows hover total p50 (ms) | freya | ~41.00 | 8.50 to 200.0 | own: 10 own valid cells in class 'latency (ms)' (log-level +0.20 against the column medians); column level from 1 frameworks |
| sheet 10,000 rows hover total p50 (ms) | freya-056a5b4 | ~41.00 | 16.00 to 110.0 | own: 10 own valid cells in class 'latency (ms)' (log-level +0.20 against the column medians); column level from 1 frameworks |
| sheet 10,000 rows hover total p50 (ms) | gpui-kit-fast | ~87.00 | 65.00 to 120.0 | own: 2 own valid cells in class 'latency (ms)' (log-level +0.96 against the column medians); column level from 1 frameworks |
| sheet 10,000 rows hover total p50 (ms) | vizia | ~200.0 | 14.00 to 2,800 | own: 8 own valid cells in class 'latency (ms)' (log-level +1.81 against the column medians); column level from 1 frameworks |
| sheet 10,000 rows hover total p50 (ms) | zgui | ~29.00 | 21.00 to 39.00 | own: 6 own valid cells in class 'latency (ms)' (log-level -0.14 against the column medians); column level from 1 frameworks |
| sheet 10,000 rows hover input to commit p50 (ms) | dioxus | ~71.00 | 53.00 to 96.00 | own: 6 own valid cells in class 'latency (ms)' (log-level +0.79 against the column medians); column level from 1 frameworks |
| sheet 10,000 rows hover input to commit p50 (ms) | dioxus-skia | ~35.00 | 26.00 to 47.00 | own: 14 own valid cells in class 'latency (ms)' (log-level +0.08 against the column medians); column level from 1 frameworks |
| sheet 10,000 rows hover input to commit p50 (ms) | floem | ~14.00 | 1.80 to 110.0 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 1 frameworks |
| sheet 10,000 rows hover input to commit p50 (ms) | freya | ~40.00 | 8.20 to 190.0 | own: 10 own valid cells in class 'latency (ms)' (log-level +0.20 against the column medians); column level from 1 frameworks |
| sheet 10,000 rows hover input to commit p50 (ms) | freya-056a5b4 | ~39.00 | 15.00 to 100.0 | own: 10 own valid cells in class 'latency (ms)' (log-level +0.20 against the column medians); column level from 1 frameworks |
| sheet 10,000 rows hover input to commit p50 (ms) | gpui-kit-fast | ~84.00 | 63.00 to 110.0 | own: 2 own valid cells in class 'latency (ms)' (log-level +0.96 against the column medians); column level from 1 frameworks |
| sheet 10,000 rows hover input to commit p50 (ms) | vizia | ~200.0 | 14.00 to 2,800 | own: 8 own valid cells in class 'latency (ms)' (log-level +1.81 against the column medians); column level from 1 frameworks |
| sheet 10,000 rows hover input to commit p50 (ms) | zgui | ~28.00 | 21.00 to 38.00 | own: 6 own valid cells in class 'latency (ms)' (log-level -0.14 against the column medians); column level from 1 frameworks |
| sheet 10,000 rows theme switch total p50 (ms) | dioxus | ~48.00 | 36.00 to 65.00 | own: 6 own valid cells in class 'latency (ms)' (log-level +0.79 against the column medians); column level from 2 frameworks |
| sheet 10,000 rows theme switch total p50 (ms) | dioxus-skia | ~24.00 | 18.00 to 32.00 | own: 14 own valid cells in class 'latency (ms)' (log-level +0.08 against the column medians); column level from 2 frameworks |
| sheet 10,000 rows theme switch total p50 (ms) | floem | ~9.50 | 1.20 to 74.00 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 2 frameworks |
| sheet 10,000 rows theme switch total p50 (ms) | freya | ~27.00 | 5.60 to 130.0 | own: 10 own valid cells in class 'latency (ms)' (log-level +0.20 against the column medians); column level from 2 frameworks |
| sheet 10,000 rows theme switch total p50 (ms) | freya-056a5b4 | ~27.00 | 10.00 to 70.00 | own: 10 own valid cells in class 'latency (ms)' (log-level +0.20 against the column medians); column level from 2 frameworks |
| sheet 10,000 rows theme switch total p50 (ms) | gpui-kit-fast | ~58.00 | 43.00 to 78.00 | own: 2 own valid cells in class 'latency (ms)' (log-level +0.96 against the column medians); column level from 2 frameworks |
| sheet 10,000 rows theme switch total p50 (ms) | vizia | ~130.0 | 9.50 to 1,900 | own: 8 own valid cells in class 'latency (ms)' (log-level +1.81 against the column medians); column level from 2 frameworks |
| sheet 10,000 rows theme switch total p50 (ms) | zgui | ~19.00 | 14.00 to 26.00 | own: 6 own valid cells in class 'latency (ms)' (log-level -0.14 against the column medians); column level from 2 frameworks |
| sheet 10,000 rows edit total p50 (ms) | dioxus | ~38.00 | 28.00 to 52.00 | own: 6 own valid cells in class 'latency (ms)' (log-level +0.79 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows edit total p50 (ms) | dioxus-skia | ~19.00 | 14.00 to 25.00 | own: 14 own valid cells in class 'latency (ms)' (log-level +0.08 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows edit total p50 (ms) | floem | ~7.50 | 0.96 to 59.00 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 4 frameworks |
| sheet 10,000 rows edit total p50 (ms) | freya | ~21.00 | 4.40 to 100.0 | own: 10 own valid cells in class 'latency (ms)' (log-level +0.20 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows edit total p50 (ms) | freya-056a5b4 | ~21.00 | 8.10 to 55.00 | own: 10 own valid cells in class 'latency (ms)' (log-level +0.20 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows edit total p50 (ms) | gpui-kit-fast | ~45.00 | 34.00 to 61.00 | own: 2 own valid cells in class 'latency (ms)' (log-level +0.96 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows edit total p50 (ms) | vizia | ~110.0 | 7.50 to 1,500 | own: 8 own valid cells in class 'latency (ms)' (log-level +1.81 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows edit total p50 (ms) | zgui | ~15.00 | 11.00 to 20.00 | own: 6 own valid cells in class 'latency (ms)' (log-level -0.14 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows resize configure to present p50 (ms) | bough | ~26.0 | 19.0 to 36.0 | own: 27 own valid cells in class 'latency (ms)' (log-level -0.54 against the column medians); column level from 2 frameworks |
| sheet 10,000 rows resize configure to present p50 (ms) | dioxus | ~99.0 | 74.0 to 130.0 | own: 6 own valid cells in class 'latency (ms)' (log-level +0.79 against the column medians); column level from 2 frameworks |
| sheet 10,000 rows resize configure to present p50 (ms) | dioxus-skia | ~49.0 | 36.0 to 66.0 | own: 14 own valid cells in class 'latency (ms)' (log-level +0.08 against the column medians); column level from 2 frameworks |
| sheet 10,000 rows resize configure to present p50 (ms) | floem | ~20.0 | 2.5 to 150.0 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 2 frameworks |
| sheet 10,000 rows resize configure to present p50 (ms) | freya | ~55.0 | 12.0 to 260.0 | own: 10 own valid cells in class 'latency (ms)' (log-level +0.20 against the column medians); column level from 2 frameworks |
| sheet 10,000 rows resize configure to present p50 (ms) | freya-056a5b4 | ~55.0 | 21.0 to 140.0 | own: 10 own valid cells in class 'latency (ms)' (log-level +0.20 against the column medians); column level from 2 frameworks |
| sheet 10,000 rows resize configure to present p50 (ms) | gpui-kit-fast | ~120.0 | 87.0 to 160.0 | own: 2 own valid cells in class 'latency (ms)' (log-level +0.96 against the column medians); column level from 2 frameworks |
| sheet 10,000 rows resize configure to present p50 (ms) | vizia | ~270.0 | 20.0 to 3,800 | own: 8 own valid cells in class 'latency (ms)' (log-level +1.81 against the column medians); column level from 2 frameworks |
| sheet 10,000 rows resize configure to present p50 (ms) | zgui | ~39.0 | 29.0 to 53.0 | own: 6 own valid cells in class 'latency (ms)' (log-level -0.14 against the column medians); column level from 2 frameworks |
| sheet 10,000 rows idle RSS (MiB) | dioxus | ~340.0 | 250.0 to 460.0 | own: 3 own valid cells in class 'resident memory (MiB)' (log-level +0.18 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows idle RSS (MiB) | dioxus-skia | ~300.0 | 220.0 to 400.0 | own: 4 own valid cells in class 'resident memory (MiB)' (log-level +0.04 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows idle RSS (MiB) | floem | ~250.0 | 180.0 to 340.0 | own: 2 own valid cells in class 'resident memory (MiB)' (log-level -0.14 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows idle RSS (MiB) | freya | ~270.0 | 200.0 to 370.0 | own: 5 own valid cells in class 'resident memory (MiB)' (log-level -0.05 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows idle RSS (MiB) | freya-056a5b4 | ~270.0 | 200.0 to 370.0 | own: 5 own valid cells in class 'resident memory (MiB)' (log-level -0.05 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows idle RSS (MiB) | gpui-kit-fast | ~390.0 | 290.0 to 530.0 | own: 4 own valid cells in class 'resident memory (MiB)' (log-level +0.31 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows idle RSS (MiB) | vizia | ~290.0 | 220.0 to 400.0 | own: 5 own valid cells in class 'resident memory (MiB)' (log-level +0.03 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows idle RSS (MiB) | zgui | ~350.0 | 260.0 to 470.0 | own: 3 own valid cells in class 'resident memory (MiB)' (log-level +0.20 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows idle CPU (ms/s) | dioxus | ~45.00 | 34.00 to 61.00 | own: 2 own valid cells in class 'idle CPU (ms/s)' (log-level +1.22 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows idle CPU (ms/s) | dioxus-skia | ~8.60 | 3.80 to 19.00 | own: 3 own valid cells in class 'idle CPU (ms/s)' (log-level -0.45 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows idle CPU (ms/s) | floem | ~1.70 | 1.20 to 2.30 | own: 2 own valid cells in class 'idle CPU (ms/s)' (log-level -2.09 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows idle CPU (ms/s) | freya | ~9.40 | 6.90 to 13.00 | own: 4 own valid cells in class 'idle CPU (ms/s)' (log-level -0.36 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows idle CPU (ms/s) | freya-056a5b4 | ~9.20 | 6.80 to 12.00 | own: 4 own valid cells in class 'idle CPU (ms/s)' (log-level -0.37 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows idle CPU (ms/s) | gpui-kit-fast | ~100.0 | 75.00 to 140.0 | own: 3 own valid cells in class 'idle CPU (ms/s)' (log-level +2.02 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows idle CPU (ms/s) | vizia | ~7.70 | 3.80 to 16.00 | own: 4 own valid cells in class 'idle CPU (ms/s)' (log-level -0.56 against the column medians); column level from 4 frameworks |
| sheet 10,000 rows idle CPU (ms/s) | zgui | ~28.00 | 0.96 to 810.0 | own: 2 own valid cells in class 'idle CPU (ms/s)' (log-level +0.73 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes hover total p50 (ms) | dioxus | ~340.0 | 250.0 to 460.0 | own: 6 own valid cells in class 'latency (ms)' (log-level +0.79 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes hover total p50 (ms) | dioxus-skia | ~170.0 | 120.0 to 230.0 | own: 14 own valid cells in class 'latency (ms)' (log-level +0.08 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes hover total p50 (ms) | floem | ~67.00 | 8.60 to 520.0 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 1 frameworks |
| canvas 1,000 nodes hover total p50 (ms) | freya | ~190.0 | 40.00 to 910.0 | own: 10 own valid cells in class 'latency (ms)' (log-level +0.20 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes hover total p50 (ms) | freya-056a5b4 | ~190.0 | 72.00 to 490.0 | own: 10 own valid cells in class 'latency (ms)' (log-level +0.20 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes hover total p50 (ms) | gpui-kit-fast | ~410.0 | 300.0 to 550.0 | own: 2 own valid cells in class 'latency (ms)' (log-level +0.96 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes hover total p50 (ms) | vizia | ~940.0 | 67.00 to 13,000 | own: 8 own valid cells in class 'latency (ms)' (log-level +1.81 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes hover total p50 (ms) | zgui | ~130.0 | 100.0 to 180.0 | own: 6 own valid cells in class 'latency (ms)' (log-level -0.14 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes hover input to commit p50 (ms) | dioxus | ~340.0 | 250.0 to 460.0 | own: 6 own valid cells in class 'latency (ms)' (log-level +0.79 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes hover input to commit p50 (ms) | dioxus-skia | ~170.0 | 120.0 to 230.0 | own: 14 own valid cells in class 'latency (ms)' (log-level +0.08 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes hover input to commit p50 (ms) | floem | ~67.00 | 8.60 to 520.0 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 1 frameworks |
| canvas 1,000 nodes hover input to commit p50 (ms) | freya | ~190.0 | 40.00 to 910.0 | own: 10 own valid cells in class 'latency (ms)' (log-level +0.20 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes hover input to commit p50 (ms) | freya-056a5b4 | ~190.0 | 72.00 to 490.0 | own: 10 own valid cells in class 'latency (ms)' (log-level +0.20 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes hover input to commit p50 (ms) | gpui-kit-fast | ~410.0 | 300.0 to 550.0 | own: 2 own valid cells in class 'latency (ms)' (log-level +0.96 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes hover input to commit p50 (ms) | vizia | ~940.0 | 67.00 to 13,000 | own: 8 own valid cells in class 'latency (ms)' (log-level +1.81 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes hover input to commit p50 (ms) | zgui | ~130.0 | 100.0 to 180.0 | own: 6 own valid cells in class 'latency (ms)' (log-level -0.14 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes theme switch total p50 (ms) | dioxus | ~1,100 | 790.0 to 1,400 | own: 6 own valid cells in class 'latency (ms)' (log-level +0.79 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes theme switch total p50 (ms) | dioxus-skia | ~520.0 | 390.0 to 710.0 | own: 14 own valid cells in class 'latency (ms)' (log-level +0.08 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes theme switch total p50 (ms) | floem | ~210.0 | 27.00 to 1,600 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 1 frameworks |
| canvas 1,000 nodes theme switch total p50 (ms) | freya | ~590.0 | 120.0 to 2,800 | own: 10 own valid cells in class 'latency (ms)' (log-level +0.20 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes theme switch total p50 (ms) | freya-056a5b4 | ~590.0 | 220.0 to 1,500 | own: 10 own valid cells in class 'latency (ms)' (log-level +0.20 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes theme switch total p50 (ms) | gpui-kit-fast | ~1,300 | 930.0 to 1,700 | own: 2 own valid cells in class 'latency (ms)' (log-level +0.96 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes theme switch total p50 (ms) | vizia | ~2,900 | 210.0 to 41,000 | own: 8 own valid cells in class 'latency (ms)' (log-level +1.81 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes theme switch total p50 (ms) | zgui | ~420.0 | 310.0 to 560.0 | own: 6 own valid cells in class 'latency (ms)' (log-level -0.14 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes edit total p50 (ms) | dioxus | ~19.00 | 14.00 to 26.00 | own: 6 own valid cells in class 'latency (ms)' (log-level +0.79 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes edit total p50 (ms) | dioxus-skia | ~9.40 | 7.00 to 13.00 | own: 14 own valid cells in class 'latency (ms)' (log-level +0.08 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes edit total p50 (ms) | floem | ~3.80 | 0.48 to 29.00 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 1 frameworks |
| canvas 1,000 nodes edit total p50 (ms) | freya | ~11.00 | 2.20 to 51.00 | own: 10 own valid cells in class 'latency (ms)' (log-level +0.20 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes edit total p50 (ms) | freya-056a5b4 | ~11.00 | 4.00 to 28.00 | own: 10 own valid cells in class 'latency (ms)' (log-level +0.20 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes edit total p50 (ms) | gpui-kit-fast | ~23.00 | 17.00 to 31.00 | own: 2 own valid cells in class 'latency (ms)' (log-level +0.96 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes edit total p50 (ms) | vizia | ~53.00 | 3.80 to 740.0 | own: 8 own valid cells in class 'latency (ms)' (log-level +1.81 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes edit total p50 (ms) | zgui | ~7.50 | 5.60 to 10.00 | own: 6 own valid cells in class 'latency (ms)' (log-level -0.14 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes resize configure to present p50 (ms) | dioxus | ~390.0 | 290.0 to 520.0 | own: 6 own valid cells in class 'latency (ms)' (log-level +0.79 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes resize configure to present p50 (ms) | dioxus-skia | ~190.0 | 140.0 to 260.0 | own: 14 own valid cells in class 'latency (ms)' (log-level +0.08 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes resize configure to present p50 (ms) | floem | ~76.0 | 9.7 to 590.0 | cross-class: only 0 own valid cell(s) in class 'latency (ms)': level taken across all classes (cross-class, low confidence); column level from 1 frameworks |
| canvas 1,000 nodes resize configure to present p50 (ms) | freya | ~210.0 | 45.0 to 1,000 | own: 10 own valid cells in class 'latency (ms)' (log-level +0.20 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes resize configure to present p50 (ms) | freya-056a5b4 | ~210.0 | 82.0 to 560.0 | own: 10 own valid cells in class 'latency (ms)' (log-level +0.20 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes resize configure to present p50 (ms) | gpui-kit-fast | ~460.0 | 340.0 to 620.0 | own: 2 own valid cells in class 'latency (ms)' (log-level +0.96 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes resize configure to present p50 (ms) | vizia | ~1,100 | 76.0 to 15,000 | own: 8 own valid cells in class 'latency (ms)' (log-level +1.81 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes resize configure to present p50 (ms) | zgui | ~150.0 | 110.0 to 210.0 | own: 6 own valid cells in class 'latency (ms)' (log-level -0.14 against the column medians); column level from 1 frameworks |
| canvas 1,000 nodes idle RSS (MiB) | dioxus | ~210.0 | 160.0 to 290.0 | own: 3 own valid cells in class 'resident memory (MiB)' (log-level +0.18 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes idle RSS (MiB) | dioxus-skia | ~190.0 | 140.0 to 250.0 | own: 4 own valid cells in class 'resident memory (MiB)' (log-level +0.04 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes idle RSS (MiB) | floem | ~150.0 | 110.0 to 210.0 | own: 2 own valid cells in class 'resident memory (MiB)' (log-level -0.14 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes idle RSS (MiB) | freya | ~170.0 | 130.0 to 230.0 | own: 5 own valid cells in class 'resident memory (MiB)' (log-level -0.05 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes idle RSS (MiB) | freya-056a5b4 | ~170.0 | 130.0 to 230.0 | own: 5 own valid cells in class 'resident memory (MiB)' (log-level -0.05 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes idle RSS (MiB) | gpui-kit-fast | ~240.0 | 180.0 to 330.0 | own: 4 own valid cells in class 'resident memory (MiB)' (log-level +0.31 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes idle RSS (MiB) | vizia | ~180.0 | 140.0 to 250.0 | own: 5 own valid cells in class 'resident memory (MiB)' (log-level +0.03 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes idle RSS (MiB) | zgui | ~220.0 | 160.0 to 290.0 | own: 3 own valid cells in class 'resident memory (MiB)' (log-level +0.20 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes idle CPU (ms/s) | dioxus | ~72.00 | 53.00 to 97.00 | own: 2 own valid cells in class 'idle CPU (ms/s)' (log-level +1.22 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes idle CPU (ms/s) | dioxus-skia | ~14.00 | 6.10 to 31.00 | own: 3 own valid cells in class 'idle CPU (ms/s)' (log-level -0.45 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes idle CPU (ms/s) | floem | ~2.70 | 1.90 to 3.70 | own: 2 own valid cells in class 'idle CPU (ms/s)' (log-level -2.09 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes idle CPU (ms/s) | freya | ~15.00 | 11.00 to 20.00 | own: 4 own valid cells in class 'idle CPU (ms/s)' (log-level -0.36 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes idle CPU (ms/s) | freya-056a5b4 | ~15.00 | 11.00 to 20.00 | own: 4 own valid cells in class 'idle CPU (ms/s)' (log-level -0.37 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes idle CPU (ms/s) | gpui-kit-fast | ~160.0 | 120.0 to 220.0 | own: 3 own valid cells in class 'idle CPU (ms/s)' (log-level +2.02 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes idle CPU (ms/s) | vizia | ~12.00 | 6.00 to 25.00 | own: 4 own valid cells in class 'idle CPU (ms/s)' (log-level -0.56 against the column medians); column level from 4 frameworks |
| canvas 1,000 nodes idle CPU (ms/s) | zgui | ~44.00 | 1.50 to 1,300 | own: 2 own valid cells in class 'idle CPU (ms/s)' (log-level +0.73 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes idle RSS (MiB) | dioxus | ~310.0 | 230.0 to 420.0 | own: 3 own valid cells in class 'resident memory (MiB)' (log-level +0.18 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes idle RSS (MiB) | dioxus-skia | ~270.0 | 200.0 to 370.0 | own: 4 own valid cells in class 'resident memory (MiB)' (log-level +0.04 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes idle RSS (MiB) | floem | ~230.0 | 170.0 to 310.0 | own: 2 own valid cells in class 'resident memory (MiB)' (log-level -0.14 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes idle RSS (MiB) | freya | ~250.0 | 180.0 to 340.0 | own: 5 own valid cells in class 'resident memory (MiB)' (log-level -0.05 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes idle RSS (MiB) | freya-056a5b4 | ~250.0 | 180.0 to 340.0 | own: 5 own valid cells in class 'resident memory (MiB)' (log-level -0.05 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes idle RSS (MiB) | gpui-kit-fast | ~360.0 | 260.0 to 480.0 | own: 4 own valid cells in class 'resident memory (MiB)' (log-level +0.31 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes idle RSS (MiB) | vizia | ~270.0 | 200.0 to 360.0 | own: 5 own valid cells in class 'resident memory (MiB)' (log-level +0.03 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes idle RSS (MiB) | zgui | ~320.0 | 240.0 to 430.0 | own: 3 own valid cells in class 'resident memory (MiB)' (log-level +0.20 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes idle CPU (ms/s) | dioxus | ~280.0 | 210.0 to 370.0 | own: 2 own valid cells in class 'idle CPU (ms/s)' (log-level +1.22 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes idle CPU (ms/s) | dioxus-skia | ~53.00 | 24.00 to 120.0 | own: 3 own valid cells in class 'idle CPU (ms/s)' (log-level -0.45 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes idle CPU (ms/s) | floem | ~10.00 | 7.30 to 14.00 | own: 2 own valid cells in class 'idle CPU (ms/s)' (log-level -2.09 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes idle CPU (ms/s) | freya | ~57.00 | 43.00 to 77.00 | own: 4 own valid cells in class 'idle CPU (ms/s)' (log-level -0.36 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes idle CPU (ms/s) | freya-056a5b4 | ~57.00 | 42.00 to 76.00 | own: 4 own valid cells in class 'idle CPU (ms/s)' (log-level -0.37 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes idle CPU (ms/s) | gpui-kit-fast | ~620.0 | 460.0 to 840.0 | own: 3 own valid cells in class 'idle CPU (ms/s)' (log-level +2.02 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes idle CPU (ms/s) | vizia | ~47.00 | 23.00 to 96.00 | own: 4 own valid cells in class 'idle CPU (ms/s)' (log-level -0.56 against the column medians); column level from 4 frameworks |
| canvas 5,000 nodes idle CPU (ms/s) | zgui | ~170.0 | 5.90 to 4,900 | own: 2 own valid cells in class 'idle CPU (ms/s)' (log-level +0.73 against the column medians); column level from 4 frameworks |
| sheet 1,000 startup to first present (ms) | dioxus | ~320.0 | 240.0 to 430.0 | own: 3 own valid cells in class 'startup (ms)' (log-level +0.50 against the column medians); column level from 4 frameworks |
| sheet 1,000 startup to first present (ms) | dioxus-skia | ~190.0 | 140.0 to 260.0 | own: 3 own valid cells in class 'startup (ms)' (log-level +0.00 against the column medians); column level from 4 frameworks |
| sheet 1,000 startup to first present (ms) | floem | ~83.0 | 11.0 to 650.0 | cross-class: only 1 own valid cell(s) in class 'startup (ms)': level taken across all classes (cross-class, low confidence); column level from 4 frameworks |
| sheet 1,000 startup to first present (ms) | freya | ~190.0 | 140.0 to 260.0 | own: 3 own valid cells in class 'startup (ms)' (log-level +0.01 against the column medians); column level from 4 frameworks |
| sheet 1,000 startup to first present (ms) | freya-056a5b4 | ~190.0 | 130.0 to 280.0 | own: 3 own valid cells in class 'startup (ms)' (log-level -0.01 against the column medians); column level from 4 frameworks |
| sheet 1,000 startup to first present (ms) | gpui-kit-fast | ~540.0 | 380.0 to 770.0 | own: 3 own valid cells in class 'startup (ms)' (log-level +1.03 against the column medians); column level from 4 frameworks |
| sheet 1,000 startup to first present (ms) | vizia | ~140.0 | 100.0 to 190.0 | own: 3 own valid cells in class 'startup (ms)' (log-level -0.32 against the column medians); column level from 4 frameworks |
| sheet 1,000 startup to first present (ms) | zgui | ~350.0 | 260.0 to 470.0 | own: 3 own valid cells in class 'startup (ms)' (log-level +0.59 against the column medians); column level from 4 frameworks |
| sheet 10,000 startup to first present (ms) | dioxus | ~690.0 | 510.0 to 930.0 | own: 3 own valid cells in class 'startup (ms)' (log-level +0.50 against the column medians); column level from 4 frameworks |
| sheet 10,000 startup to first present (ms) | dioxus-skia | ~420.0 | 310.0 to 560.0 | own: 3 own valid cells in class 'startup (ms)' (log-level +0.00 against the column medians); column level from 4 frameworks |
| sheet 10,000 startup to first present (ms) | floem | ~180.0 | 23.0 to 1,400 | cross-class: only 1 own valid cell(s) in class 'startup (ms)': level taken across all classes (cross-class, low confidence); column level from 4 frameworks |
| sheet 10,000 startup to first present (ms) | freya | ~420.0 | 310.0 to 570.0 | own: 3 own valid cells in class 'startup (ms)' (log-level +0.01 against the column medians); column level from 4 frameworks |
| sheet 10,000 startup to first present (ms) | freya-056a5b4 | ~410.0 | 280.0 to 600.0 | own: 3 own valid cells in class 'startup (ms)' (log-level -0.01 against the column medians); column level from 4 frameworks |
| sheet 10,000 startup to first present (ms) | gpui-kit-fast | ~1,200 | 810.0 to 1,700 | own: 3 own valid cells in class 'startup (ms)' (log-level +1.03 against the column medians); column level from 4 frameworks |
| sheet 10,000 startup to first present (ms) | vizia | ~300.0 | 220.0 to 410.0 | own: 3 own valid cells in class 'startup (ms)' (log-level -0.32 against the column medians); column level from 4 frameworks |
| sheet 10,000 startup to first present (ms) | zgui | ~750.0 | 560.0 to 1,000 | own: 3 own valid cells in class 'startup (ms)' (log-level +0.59 against the column medians); column level from 4 frameworks |
| canvas 1,000 startup to first present (ms) | dioxus | ~600.0 | 440.0 to 810.0 | own: 3 own valid cells in class 'startup (ms)' (log-level +0.50 against the column medians); column level from 4 frameworks |
| canvas 1,000 startup to first present (ms) | dioxus-skia | ~360.0 | 270.0 to 490.0 | own: 3 own valid cells in class 'startup (ms)' (log-level +0.00 against the column medians); column level from 4 frameworks |
| canvas 1,000 startup to first present (ms) | floem | ~160.0 | 20.0 to 1,200 | cross-class: only 1 own valid cell(s) in class 'startup (ms)': level taken across all classes (cross-class, low confidence); column level from 4 frameworks |
| canvas 1,000 startup to first present (ms) | freya | ~360.0 | 270.0 to 490.0 | own: 3 own valid cells in class 'startup (ms)' (log-level +0.01 against the column medians); column level from 4 frameworks |
| canvas 1,000 startup to first present (ms) | freya-056a5b4 | ~360.0 | 250.0 to 520.0 | own: 3 own valid cells in class 'startup (ms)' (log-level -0.01 against the column medians); column level from 4 frameworks |
| canvas 1,000 startup to first present (ms) | gpui-kit-fast | ~1,000 | 710.0 to 1,500 | own: 3 own valid cells in class 'startup (ms)' (log-level +1.03 against the column medians); column level from 4 frameworks |
| canvas 1,000 startup to first present (ms) | vizia | ~260.0 | 190.0 to 350.0 | own: 3 own valid cells in class 'startup (ms)' (log-level -0.32 against the column medians); column level from 4 frameworks |
| canvas 1,000 startup to first present (ms) | zgui | ~650.0 | 480.0 to 880.0 | own: 3 own valid cells in class 'startup (ms)' (log-level +0.59 against the column medians); column level from 4 frameworks |
| canvas 5,000 startup to first present (ms) | dioxus | ~1,500 | 1,100 to 2,000 | own: 3 own valid cells in class 'startup (ms)' (log-level +0.50 against the column medians); column level from 4 frameworks |
| canvas 5,000 startup to first present (ms) | dioxus-skia | ~900.0 | 670.0 to 1,200 | own: 3 own valid cells in class 'startup (ms)' (log-level +0.00 against the column medians); column level from 4 frameworks |
| canvas 5,000 startup to first present (ms) | floem | ~390.0 | 50.0 to 3,000 | cross-class: only 1 own valid cell(s) in class 'startup (ms)': level taken across all classes (cross-class, low confidence); column level from 4 frameworks |
| canvas 5,000 startup to first present (ms) | freya | ~900.0 | 670.0 to 1,200 | own: 3 own valid cells in class 'startup (ms)' (log-level +0.01 against the column medians); column level from 4 frameworks |
| canvas 5,000 startup to first present (ms) | freya-056a5b4 | ~890.0 | 610.0 to 1,300 | own: 3 own valid cells in class 'startup (ms)' (log-level -0.01 against the column medians); column level from 4 frameworks |
| canvas 5,000 startup to first present (ms) | gpui-kit-fast | ~2,500 | 1,800 to 3,600 | own: 3 own valid cells in class 'startup (ms)' (log-level +1.03 against the column medians); column level from 4 frameworks |
| canvas 5,000 startup to first present (ms) | vizia | ~650.0 | 480.0 to 880.0 | own: 3 own valid cells in class 'startup (ms)' (log-level -0.32 against the column medians); column level from 4 frameworks |
| canvas 5,000 startup to first present (ms) | zgui | ~1,600 | 1,200 to 2,200 | own: 3 own valid cells in class 'startup (ms)' (log-level +0.59 against the column medians); column level from 4 frameworks |
| spinner above 10,000 rows: presented frames per second (fps) | floem | ~58.0 | 43.0 to 78.0 | own: 2 own valid cells in class 'frame rate (fps)' (log-level -0.84 against the column medians); column level from 4 frameworks |
| spinner above 10,000 rows: frame interval p50 (ms) | floem | ~17.00 | 13.00 to 23.00 | own: 2 own valid cells in class 'frame interval (ms)' (log-level +0.57 against the column medians); column level from 4 frameworks |
| spinner above 10,000 rows: frame interval p99 (ms) | floem | ~21.00 | 16.00 to 29.00 | own: 2 own valid cells in class 'frame interval (ms)' (log-level +0.57 against the column medians); column level from 4 frameworks |
| spinner above 10,000 rows: CPU per frame (ms) | floem | ~6.80 | 5.00 to 9.20 | own: 2 own valid cells in class 'cpu per frame (ms)' (log-level -0.95 against the column medians); column level from 4 frameworks |
| spinner above 10,000 rows: CPU per second (ms/s) | floem | ~440.0 | 320.0 to 610.0 | own: 2 own valid cells in class 'idle CPU (ms/s)' (log-level -2.09 against the column medians); column level from 4 frameworks |
| spinner above 10,000 rows: RSS (MiB) | floem | ~450.0 | 330.0 to 610.0 | own: 2 own valid cells in class 'resident memory (MiB)' (log-level -0.14 against the column medians); column level from 4 frameworks |

## Heavy scenes: Sheet, 1,000 rows

| framework | sheet 1,000 rows hover total p50 (ms) | sheet 1,000 rows hover input to commit p50 (ms) | sheet 1,000 rows theme switch total p50 (ms) | sheet 1,000 rows edit total p50 (ms) | sheet 1,000 rows resize configure to present p50 (ms) | sheet 1,000 rows idle RSS (MiB) | sheet 1,000 rows idle CPU (ms/s) | sheet 1,000 startup to first present (ms) |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| zgui | *~7.50* | *~3.70* | *~10.00* | *~14.00* | *~12.0* | *~200.0* | *~26.00* | *~350.0* |
| gpui-kit | 22.99 † | 20.44 † | 21.39 † | 20.91 † | 39.4 † | 186.9 | 60.26 | - |
| gpui-kit-fast | *~23.00* | *~11.00* | *~31.00* | *~41.00* | *~36.0* | *~230.0* | *~94.00* | *~540.0* |
| freya | *~11.00* | *~5.30* | *~14.00* | *~19.00* | *~17.0* | *~160.0* | *~8.70* | *~190.0* |
| freya-056a5b4 | *~10.00* | *~5.30* | *~14.00* | *~19.00* | *~17.0* | *~160.0* | *~8.60* | *~190.0* |
| vizia | *~52.00* | *~26.00* | *~71.00* | *~95.00* | *~83.0* | *~170.0* | *~7.10* | *~140.0* |
| floem | *~3.70* | *~1.90* | *~5.10* | *~6.80* | *~5.9* | *~140.0* | *~1.50* | *~83.0* |
| iced | 17.96 † | 17.50 † | 19.16 | 19.51 | 11.5 | 185.6 | 26.82 | - |
| egui | 9.81 † | 9.82 † | 10.55 | 9.37 | 9.6 † | 148.4 | 24.81 | - |
| dioxus | *~19.00* | *~9.50* | *~26.00* | *~34.00* | *~30.0* | *~200.0* | *~42.00* | *~320.0* |
| dioxus-skia | *~9.40* | *~4.70* | *~13.00* | *~17.00* | *~15.0* | *~170.0* | *~8.00* | *~190.0* |
| bough | 5.01 | 2.51 | 6.80 | 1.66 † | 7.9 | 49.8 | 0.03 | - |

## Heavy scenes: Sheet, 10,000 rows

| framework | sheet 10,000 rows hover total p50 (ms) | sheet 10,000 rows hover input to commit p50 (ms) | sheet 10,000 rows theme switch total p50 (ms) | sheet 10,000 rows edit total p50 (ms) | sheet 10,000 rows resize configure to present p50 (ms) | sheet 10,000 rows idle RSS (MiB) | sheet 10,000 rows idle CPU (ms/s) | sheet 10,000 startup to first present (ms) |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| zgui | *~29.00* | *~28.00* | *~19.00* | *~15.00* | *~39.0* | *~350.0* | *~28.00* | *~750.0* |
| gpui-kit | 24.25 † | 20.06 † | 21.71 † | 21.76 | 36.6 † | 186.2 | 65.99 | - |
| gpui-kit-fast | *~87.00* | *~84.00* | *~58.00* | *~45.00* | *~120.0* | *~390.0* | *~100.0* | *~1,200* |
| freya | *~41.00* | *~40.00* | *~27.00* | *~21.00* | *~55.0* | *~270.0* | *~9.40* | *~420.0* |
| freya-056a5b4 | *~41.00* | *~39.00* | *~27.00* | *~21.00* | *~55.0* | *~270.0* | *~9.20* | *~410.0* |
| vizia | *~200.0* | *~200.0* | *~130.0* | *~110.0* | *~270.0* | *~290.0* | *~7.70* | *~300.0* |
| floem | *~14.00* | *~14.00* | *~9.50* | *~7.50* | *~20.0* | *~250.0* | *~1.70* | *~180.0* |
| iced | 175.2 † | 174.7 † | 191.7 † | 188.3 | 153.3 | 550.9 | 38.22 | - |
| egui | 8.91 † | 8.92 † | 10.74 | 9.59 | 9.9 | 149.1 | 23.32 | - |
| dioxus | *~73.00* | *~71.00* | *~48.00* | *~38.00* | *~99.0* | *~340.0* | *~45.00* | *~690.0* |
| dioxus-skia | *~36.00* | *~35.00* | *~24.00* | *~19.00* | *~49.0* | *~300.0* | *~8.60* | *~420.0* |
| bough | 19.43 | 18.78 | 23.73 | 14.28 | *~26.0* (failed cell) | 331.3 | 0.03 | - |

## Heavy scenes: Canvas, 1,000 nodes

| framework | canvas 1,000 nodes hover total p50 (ms) | canvas 1,000 nodes hover input to commit p50 (ms) | canvas 1,000 nodes theme switch total p50 (ms) | canvas 1,000 nodes edit total p50 (ms) | canvas 1,000 nodes resize configure to present p50 (ms) | canvas 1,000 nodes idle RSS (MiB) | canvas 1,000 nodes idle CPU (ms/s) | canvas 1,000 startup to first present (ms) |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| zgui | *~130.0* | *~130.0* | *~420.0* | *~7.50* | *~150.0* | *~220.0* | *~44.00* | *~650.0* |
| gpui-kit | 35.59 † | 35.60 † | 39.85 † | 39.16 † | 41.1 † | 196.0 | 116.7 | - |
| gpui-kit-fast | *~410.0* | *~410.0* | *~1,300* | *~23.00* | *~460.0* | *~240.0* | *~160.0* | *~1,000* |
| freya | *~190.0* | *~190.0* | *~590.0* | *~11.00* | *~210.0* | *~170.0* | *~15.00* | *~360.0* |
| freya-056a5b4 | *~190.0* | *~190.0* | *~590.0* | *~11.00* | *~210.0* | *~170.0* | *~15.00* | *~360.0* |
| vizia | *~940.0* | *~940.0* | *~2,900* | *~53.00* | *~1,100* | *~180.0* | *~12.00* | *~260.0* |
| floem | *~67.00* | *~67.00* | *~210.0* | *~3.80* | *~76.0* | *~150.0* | *~2.70* | *~160.0* |
| iced | 22.66 † | 19.93 † | 20.67 † | 20.44 † | 20.1 † | 421.2 | 44.62 | - |
| egui | 26.63 † | 24.42 † | 25.42 † | 24.31 † | 26.6 † | 172.8 | 57.26 | - |
| dioxus | *~340.0* | *~340.0* | *~1,100* | *~19.00* | *~390.0* | *~210.0* | *~72.00* | *~600.0* |
| dioxus-skia | *~170.0* | *~170.0* | *~520.0* | *~9.40* | *~190.0* | *~190.0* | *~14.00* | *~360.0* |
| bough | 90.19 | 90.20 | 280.4 | 5.05 | 101.9 | 19.2 | 0.04 | - |

## Heavy scenes: Canvas, 5,000 nodes

| framework | canvas 5,000 nodes hover total p50 (ms) | canvas 5,000 nodes hover input to commit p50 (ms) | canvas 5,000 nodes theme switch total p50 (ms) | canvas 5,000 nodes edit total p50 (ms) | canvas 5,000 nodes resize configure to present p50 (ms) | canvas 5,000 nodes idle RSS (MiB) | canvas 5,000 nodes idle CPU (ms/s) | canvas 5,000 startup to first present (ms) |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| zgui | - | - | - | - | - | *~320.0* | *~170.0* | *~1,600* |
| gpui-kit | 157.0 † | 157.0 † | 159.4 † | 157.5 † | 141.6 † | 262.6 | 404.6 | - |
| gpui-kit-fast | - | - | - | - | - | *~360.0* | *~620.0* | *~2,500* |
| freya | - | - | - | - | - | *~250.0* | *~57.00* | *~900.0* |
| freya-056a5b4 | - | - | - | - | - | *~250.0* | *~57.00* | *~890.0* |
| vizia | - | - | - | - | - | *~270.0* | *~47.00* | *~650.0* |
| floem | - | - | - | - | - | *~230.0* | *~10.00* | *~390.0* |
| iced | 90.03 † | 88.85 † | 91.36 † | 92.55 † | 79.6 † | 1,400 | 191.0 | - |
| egui | 113.6 † | 110.4 † | 62.47 † | 62.57 † | 68.7 † | 278.6 | 226.1 | - |
| dioxus | - | - | - | - | - | *~310.0* | *~280.0* | *~1,500* |
| dioxus-skia | - | - | - | - | - | *~270.0* | *~53.00* | *~900.0* |
| bough | 898.0 † | 894.2 † | 1,110 † | 21.99 † | 612.1 † | 25.8 | 0.03 | - |

## Animation: the spinner scenarios (the only animation cells)

| framework | spinner, no rows: presented frames per second (fps) | spinner, no rows: frame interval p50 (ms) | spinner, no rows: frame interval p99 (ms) | spinner, no rows: jank fraction (fraction) | spinner, no rows: missed vblanks (count) | spinner, no rows: CPU per frame (ms) | spinner, no rows: CPU per second (ms/s) | spinner above 10,000 rows: presented frames per second (fps) | spinner above 10,000 rows: frame interval p50 (ms) | spinner above 10,000 rows: frame interval p99 (ms) | spinner above 10,000 rows: jank fraction (fraction) | spinner above 10,000 rows: missed vblanks (count) | spinner above 10,000 rows: CPU per frame (ms) | spinner above 10,000 rows: CPU per second (ms/s) |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| zgui | 59.7 † | 16.64 † | 24.10 † | 0.028 † | 5 † | 13.74 † | 821.7 † | 59.9 † | 15.22 † | 28.27 † | 0.325 † | 58 † | 12.69 † | 760.9 † |
| gpui-kit | 102.4 † | 9.36 † | 15.69 † | 0.030 † | 0 † | 59.72 † | 6,127 † | 3.7 † | 272.9 † | 281.5 † | 0.000 † | 137.0 † | 346.4 † | 1,293 † |
| gpui-kit-fast | 73.3 | 13.41 | 19.24 | 0.005 | 0 | 91.36 | 6,699 | 8.5 † | 116.6 † | 127.7 † | 0.000 † | 126.0 † | 244.6 † | 2,074 † |
| freya | 144.0 | 6.94 | 6.94 | 0.000 | 0 | 3.81 | 549.4 | 139.2 | 7.00 | 9.20 | 0.005 | 0 | 17.87 | 2,490 |
| freya-056a5b4 | 144.0 | 6.94 | 6.94 | 0.000 | 0 | 3.82 | 550.4 | 140.6 | 6.94 | 9.32 | 0.002 | 0 | 17.22 | 2,422 |
| vizia | 62.5 | 16.00 | 17.57 | 0.000 | 0 | 7.21 | 450.8 | 61.1 | 15.98 | 19.63 | 0.000 | 0 | 20.77 | 1,271 |
| floem | 59.9 | 16.71 | 17.20 | 0.000 | 0 | 1.48 | 88.5 | *~58.0* (failed cell) | *~17.00* (failed cell) | *~21.00* (failed cell) | - | - | *~6.80* (failed cell) | *~440.0* (failed cell) |
| iced | 138.2 | 6.94 | 10.94 | 0.016 | 0 | 34.39 | 4,749 | 20.3 † | 48.61 † | 64.83 † | 0.000 † | 106.5 † | 85.37 † | 1,722 † |
| egui | 144.0 | 6.94 | 6.94 | 0.000 | 0 | 3.46 | 498.7 | 15.4 † | 65.62 † | 71.99 † | 0.000 † | 116.5 † | 79.49 † | 1,226 † |
| dioxus | 80.2 † | 12.10 † | 18.89 † | 0.016 † | 0 † | 52.35 † | 4,187 † | 34.8 † | 28.47 † | 33.97 † | 0.000 † | 101.0 † | 79.08 † | 2,757 † |
| dioxus-skia | 144.0 | 6.94 | 6.94 | 0.000 | 0 | 7.07 | 1,018 | 52.7 † | 18.68 † | 27.78 † | 0.003 † | 14 † | 30.04 † | 1,583 † |
| bough | 62.5 | 16.22 | 16.39 | 0.000 | 0 | 0.10 | 6.5 | 62.5 | 16.26 | 16.56 | 0.000 | 0 | 1.18 | 73.7 |

## Latency split (input to commit, wait, present)

| framework | probe | total p50 | input to commit | commit to ready | ready to latch | latch to present |
|---|---|---:|---:|---:|---:|---:|
| zgui | rows 3,000 hover | 7.43 | 6.69 | 0.00 | 0.05 | -0.06 |
| gpui-kit | rows 3,000 hover | 110.6 † | 109.3 † | 0.00 † | 2.49 † | -0.02 † |
| gpui-kit-fast | rows 3,000 hover | 68.01 † | 66.38 † | 0.00 † | 1.26 † | -0.04 † |
| freya | rows 3,000 hover | 5.74 | 5.53 | 0.00 | 0.12 | -0.04 |
| freya-056a5b4 | rows 3,000 hover | 6.96 | 6.70 | 0.00 | 0.04 | -0.05 |
| vizia | rows 3,000 hover | 87.29 | 85.76 | 0.00 | 0.58 | -0.03 |
| iced | rows 3,000 hover | 6.72 | 6.35 | 0.00 | 0.06 | -0.04 |
| egui | rows 3,000 hover | 20.24 | 19.75 | 0.00 | 0.05 | -0.05 |
| dioxus | rows 3,000 hover | 25.54 † | 23.19 † | 0.00 † | 1.90 † | -0.03 † |
| dioxus-skia | rows 3,000 hover | 9.19 | 8.98 | 0.00 | 0.04 | -0.05 |
| bough | rows 3,000 hover | 4.10 | 1.11 | 0.00 | 2.95 | -0.02 |
| zgui | table fine update10 | 19.16 † | 16.82 † | 0.00 † | 1.22 † | -0.03 † |
| gpui-kit | table fine update10 | 42.50 † | 41.25 † | 0.00 † | 0.76 † | -0.05 † |
| gpui-kit-fast | table fine update10 | 38.30 † | 36.96 † | 0.00 † | 1.42 † | -0.04 † |
| freya | table fine update10 | 13.23 | 11.67 | 0.00 | 1.25 | -0.06 |
| freya-056a5b4 | table fine update10 | 13.22 | 11.59 | 0.00 | 0.48 | -0.06 |
| vizia | table fine update10 | 33.52 | 33.53 | 0.00 | 0.04 | -0.06 |
| iced | table fine update10 | 9.41 † | 8.20 † | 0.00 † | 1.00 † | -0.03 † |
| egui | table fine update10 | 12.01 | 12.02 | 0.00 | 0.04 | -0.05 |
| dioxus | table fine update10 | 26.03 † | 26.05 † | 0.00 † | 0.45 † | -0.05 † |
| dioxus-skia | table fine update10 | 11.73 | 10.38 | 0.00 | 0.97 | -0.04 |
| bough | table fine update10 | 0.44 | 0.44 | 0.00 | 0.04 | -0.05 |
| zgui | table fine remove | 43.33 † | 42.09 † | 0.00 † | 0.58 † | -0.04 † |
| gpui-kit | table fine remove | 57.89 | 57.31 | 0.00 | 0.51 | -0.06 |
| gpui-kit-fast | table fine remove | 59.24 † | 59.13 † | 0.00 † | 0.26 † | -0.05 † |
| freya | table fine remove | 11.09 † | 10.51 † | 0.00 † | 0.11 † | -0.05 † |
| freya-056a5b4 | table fine remove | 13.90 | 12.99 | 0.00 | 0.24 | -0.06 |
| vizia | table fine remove | 388.2 † | 387.1 † | 0.00 † | 0.10 † | -0.05 † |
| iced | table fine remove | 16.28 | 13.76 | 0.00 | 0.05 | -0.05 |
| egui | table fine remove | 14.04 † | 14.02 † | 0.00 † | 0.05 † | -0.06 † |
| dioxus | table fine remove | 27.53 † | 27.51 † | 0.00 † | 0.06 † | -0.07 † |
| dioxus-skia | table fine remove | 15.57 | 13.98 | 0.00 | 0.28 | -0.06 |
| bough | table fine remove | 4.58 | 3.98 | 0.00 | 0.04 | -0.05 |
| zgui | table fine swap | 49.28 † | 45.41 † | 0.00 † | 2.31 † | -0.04 † |
| gpui-kit | table fine swap | 36.67 † | 36.18 † | 0.00 † | 0.04 † | -0.05 † |
| gpui-kit-fast | table fine swap | 40.53 | 40.55 | 0.00 | 0.04 | -0.05 |
| freya | table fine swap | 11.39 | 10.13 | 0.00 | 1.37 | -0.04 |
| freya-056a5b4 | table fine swap | 15.86 † | 13.75 † | 0.00 † | 1.92 † | -0.05 † |
| vizia | table fine swap | 289.1 | 286.9 | 0.00 | 2.23 | -0.04 |
| iced | table fine swap | 9.63 † | 8.04 † | 0.00 † | 1.06 † | -0.05 † |
| egui | table fine swap | 14.38 | 14.39 | 0.00 | 0.04 | -0.06 |
| dioxus | table fine swap | 28.12 † | 27.16 † | 0.00 † | 0.64 † | -0.05 † |
| dioxus-skia | table fine swap | 17.48 | 16.36 | 0.00 | 0.87 | -0.04 |
| bough | table fine swap | 0.58 | 0.55 | 0.00 | 0.04 | -0.05 |
| zgui | search filter | 69.30 † | 69.31 † | 0.00 † | 0.06 † | -0.07 † |
| gpui-kit | search filter | 41.41 | 40.37 | 0.00 | 0.04 | -0.05 |
| gpui-kit-fast | search filter | 44.50 † | 42.70 † | 0.00 † | 2.24 † | -0.03 † |
| freya | search filter | 41.26 | 40.64 | 0.00 | 0.04 | -0.05 |
| freya-056a5b4 | search filter | 39.14 | 38.49 | 0.00 | 0.19 | -0.04 |
| vizia | search filter | 114.3 † | 114.3 † | 0.00 † | 0.60 † | -0.04 † |
| iced | search filter | 11.63 | 10.78 | 0.00 | 0.91 | -0.05 |
| egui | search filter | 12.80 | 11.13 | 0.00 | 1.00 | -0.04 |
| dioxus | search filter | 51.42 | 49.91 | 0.00 | 0.09 | -0.05 |
| dioxus-skia | search filter | 25.34 | 24.73 | 0.00 | 0.04 | -0.05 |
| bough | search filter | 7.68 | 7.11 | 0.00 | 0.04 | -0.05 |

# Refresh sweep (rows 3,000 hover, typing 1,000, spinner), fixed grid, measured, † = invalid by the harness

| metric (ms unless noted) | bough 60 Hz | bough 120 Hz | bough 240 Hz | iced 60 Hz | iced 120 Hz | iced 240 Hz | freya 60 Hz | freya 120 Hz | freya 240 Hz | dioxus-skia 60 Hz | dioxus-skia 120 Hz | dioxus-skia 240 Hz |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| hover total p50 | 13.31 | 7.00 | 3.74 | 15.12 † | 9.41 † | 8.98 † | 15.12 † | 12.10 † | 8.39 † | 15.12 | 14.07 | 11.51 |
| hover input to commit | 1.09 | 0.98 | 1.03 | 6.49 † | 5.33 † | 6.25 † | 5.11 † | 5.98 † | 5.86 † | 6.39 | 7.30 | 7.59 |
| hover commit to ready | 0.00 | 0.00 | 0.00 | 0.00 † | 0.00 † | 0.00 † | 0.00 † | 0.00 † | 0.00 † | 0.00 | 0.00 | 0.00 |
| hover ready to latch | 11.27 | 5.02 | 1.67 | 7.62 † | 2.75 † | 1.94 † | 9.31 † | 4.83 † | 1.57 † | 7.74 | 5.30 | 2.82 |
| hover latch to present | 1.00 | 1.00 | 1.00 | 1.00 † | 1.00 † | 1.00 † | 1.00 † | 1.00 † | 1.00 † | 1.00 | 1.00 | 1.00 |
| typing p50 | 11.09 | 4.46 | 3.38 | 14.34 † | 11.59 † | 9.60 † | 17.24 † | 12.32 † | 10.37 † | 14.08 † | 11.23 † | 9.10 † |
| typing handling p50 | 0.31 | 0.26 | 0.29 | 6.77 † | 6.56 † | 6.67 † | 7.25 † | 7.55 † | 7.41 † | 5.61 † | 5.68 † | 5.71 † |
| spinner 10,000 rows fps | 60.0 | 62.5 | 62.5 | 17.5 † | 17.8 † | 18.1 † | 59.3 | 67.1 † | 98.3 † | 49.1 † | 55.5 † | 53.7 † |
| spinner 10,000 interval p99 | 16.67 | 16.67 | 16.67 | 66.67 † | 72.87 † | 61.42 † | 25.00 | 16.67 † | 12.50 † | 33.33 † | 25.00 † | 23.75 † |
| spinner 10,000 CPU per frame | 1.15 | 1.15 | 1.12 | 91.83 † | 87.91 † | 100.9 † | 23.61 | 16.46 † | 17.65 † | 30.43 † | 29.32 † | 28.95 † |
| spinner no rows fps | 60.0 | 62.5 † | 62.5 | 58.7 | 57.0 † | 86.8 † | 60.0 | 120.0 | 234.3 | 60.0 | 120.0 | 229.5 † |
| spinner no rows CPU per frame | 0.09 | 0.11 † | 0.13 | 35.01 | 40.82 † | 41.63 † | 5.07 | 2.63 | 3.34 | 7.34 | 6.30 | 6.94 † |
