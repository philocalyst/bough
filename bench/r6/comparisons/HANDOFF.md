# Comparisons lane: handoff (stopped early at the coordinator's request)

State in one line: the measurement kit and the bough side of the signal case exist; **no competitor framework has been
measured yet**, so no row of the matrix is complete. Everything below is what I verified and what is left.

## What exists

| path | state |
|---|---|
| `cmpkit/` | Shared measurement crate (own workspace, only `libc`). `instr()` = retired user-space instructions of the calling thread (Linux: `perf_event_open`, exclude_kernel; macOS: `thread_selfcounts`, the counter `bough_metal::perf::tinstr` uses). `run_signal::<A: SignalApp>` runs bough's signal case (colour/number/string/idle at n sizes: slope `(run(192)-run(64))/128` for instructions, 1,500-3,000 turns for wall p50/p99, mount instructions, rss). `run_ops::<A: TableApp>` runs the gui-bench table ops the way `wt/integ/comparisons/bough-guibench/src/driver.rs` does (op and undo per step, block of repeats / (2*repeats), median of blocks). Rows are emitted as JSON lines. `src/spec.rs` is gui-bench's `apps/shared/spec.rs` verbatim. **Verified on the Mac only**: `cargo run --release --bin selftest` (a 1e7-iteration loop counted 7.01 instr/iteration, call overhead 1,455). **Not yet run on ilo-prod**: the `perf_event_open` path is untested there (kernel.perf_event_paranoid=2, bare metal Ryzen 7 3700X, so user-only counting of one's own thread should work). Two unused-`mut` warnings. |
| `bough/` | `cmp-bough`: bough's signal case on cmpkit (`src/bin/signal.rs`, variants `once` and `redescribe`, copied from `signal_cost.rs`), path-depends on `wt/lead`. **Written, never built** (a bough build on the loaded Mac was about to start; disk had 13 GiB free). It exists so bough and the competitors share one slope/percentile implementation. |
| `bough/raw/signal_cost-prebuilt-2runs.txt` | The only bough numbers produced: the prebuilt `wt/lead/target/release/examples/signal_cost` (built 2026-11-11 22:25, wt/lead 4f4c469ef5) run twice, Mac, unchanged source. |
| `matrix.json` | Partial: 24 bough rows (signal case, `once` and `redescribe`, n=16/64/256, 4 cells), flat fields as asked plus `variant`, `n`, `run_values`, `confirmed`, `extrapolated`. No competitor rows. |
| `tools/` | `rx.sh` (run a command on ilo-prod through rex, print output), `bg.sh` (background tab), `cap.sh` (capture a tab), `push.sh localdir remotedir` (tar+base64 over rex; plain ssh is unusable). They assume the rex session `cmp-r6` on `-S ilo-prod`; `du` over the big target dirs exceeds rex's 120 s foreground limit, avoid it. |

## Inventory matrix (case x framework)

Legend: M = measured by me (see matrix.json), - = missing (port to write and run), X = impossible / no counterpart (reason). Frameworks are gui-bench's
(ref/gui-bench/apps): zgui, gpui-kit (and gpui-kit-fast), freya (and freya-056a5b4), vizia, floem, iced, egui. Ripple and Solid are excluded by instruction.
Dioxus exists in the newer gui-bench checkout on ilo-prod but not in `ref/gui-bench`; I left it out.

| case (dashboard series) | bough | zgui | gpui-kit(+fast) | freya(+056) | vizia | floem | iced | egui |
|---|---|---|---|---|---|---|---|---|
| signal colour write, n=16/64/256 (`sig_colour_write_*`) | M (2 runs; see below) | - | - | - | - | - | - | - |
| signal numeric label write (`sig_numeric_label_256`) | M | - | - | - | - | - | - | - |
| signal string label write (`sig_string_label_256`) | M | - | - | - | - | - | - | - |
| idle turn (`sig_idle_256`) | M (247) | - | - | - | - | - | - | - (egui skips idle frames; define as forced frame and say so) |
| re-describing owner idle / re-description per row (`sig_redescribe_owner_idle_256`, `redescribe_*`, `store_redescribe_*`) | M | X fine-only, use coarse variant | - | - | - | X (signals; coarse variant rebuilds) | - | - |
| gui-bench table select/update10/swap/remove, coarse and fine (`table_*`, `gb6-table-*`) | existing, `wt/integ/docs/lanes/generalization/TABLE-CELLS.json`, not re-measured by me | - | - | - | - | - | - | - (immediate mode: one variant) |
| rows hover n=10/100/1000 (`gb6-rows-fine-hover-*`) | existing (same file) | - | - | - | - | - | - | - |
| binary size (`size_counter`) | Mac, existing | - | - | - | - | - | - | - |
| startup (`startup_empty`) | Mac, existing (headless instructions) | - | - | - | - | - | - | - |
| resident memory / footprint (`memory_*`) | Mac, existing | - | - | - | - | - | - | - |
| pacing: presents/s typing, scroll p95 (`pace_*`) | Mac, existing | - | - | - | - | - | - | - |
| record tree, store, page translate, flag flip (`core_*`, `store_*`) | M by their lanes | X no public counterpart in any framework (bough-internal structure operations); the nearest cases are table select/swap/append, which are covered above | | | | | | |
| tests failing (`tests_failing`) | n/a | X not a performance case | | | | | | |

The "good at, bough may be bad at" case per framework is not chosen yet. Candidates from gui-bench's own scenario list: egui, replace-all and a 100-row ticker (no
diff to pay for); iced, `lazy` keyed single-row update; gpui, `virtual` 1M rows; floem, `virtual_list`; freya and vizia, text input typing/search; zgui, `fine` ticker. bough's
numbers for these ops are already in TABLE-CELLS.json (replace is 2.9 M at 1,000 rows, a visible weakness; append 1.0 M).

## Numbers produced (all bough, Mac M3 Pro aarch64, loaded, `signal_cost` prebuilt from wt/lead 4f4c469ef5, 2 runs)

| n | mode | colour | number | string | idle | second run confirms? |
|---|---|---|---|---|---|---|
| 16 | once | 649 / 765 | 2,766 / 2,764 | 3,523 / 3,652 | 247 / 247 | colour **no** (16 % apart), number yes, string ~3 %, idle yes |
| 64 | once | 618 / 618 | INVALID | INVALID | 247 / 247 | colour, idle yes |
| 256 | once | 606 / 606 | 8,724 / 8,754 | 20,851 / 20,852 | 247 / 247 | yes |
| 16 | redescribe | 9,825 / 9,825 | 11,942 / 11,942 | 12,698 / 12,699 | 9,423 / 9,423 | yes |
| 64 | redescribe | 35,954 / 36,095 | 33,895 / 33,894 | 31,032 / 31,160 | 35,583 / 35,614 | yes |
| 256 | redescribe | 140,625 / 140,582 | 148,672 / 148,786 | 160,767 / 160,754 | 140,223 / 140,223 | yes |

Defect in the bough harness, reproduced in 5 of 5 runs: at 64 rows the `once` number and string cells print about 1.44e17, an unsigned `hi - lo` underflow in
`signal_cost.rs::slope` (the 192-repeat run counted fewer instructions than the 64-repeat run, so the cost is not linear in repeats there: something periodic, e.g. a
reclaim, lands inside the short run). The dashboard only charts 256 rows, so it is not visible. matrix.json marks those two cells `value: null`. Use cmpkit's slope
(same formula) but report the raw lo/hi too, or fit over more repeat counts, before charting n=64.

## Environment facts for whoever continues (verified)

* ilo-prod: Ryzen 7 3700X, 16 threads, no virtualisation, governor `powersave`, load average 25 to 45 (shared), `perf_event_paranoid=2`, no `perf` binary. Linux/x86-64, so
  instruction counts are x86 and **never share an axis with the Mac's aarch64 counts** (bough has no Linux build: `bough::app` is macOS-gated, the Vulkan presenter is the
  platform-host lane's work). The honest options are (a) label the machine and chart separately, (b) also run the light headless harnesses (egui, iced, floem) on the Mac
  (disk was 13 GiB free: a gpui or skia build will not fit), (c) bough on Linux once the host lane lands it.
* Toolchain without touching zortax's: `/root/work/cmp/env.sh` (written; uses `/root/work/zortax/env/result` and the toolchain dir read-only, own `CARGO_HOME=/root/work/cmp/cargo`).
  I started `cp -a /root/work/zortax/cargo/{registry,git} /root/work/cmp/cargo/` (about 1.2 GB, very slow on that disk); I closed that tab when stopped, so the copy is
  **partial**: delete `/root/work/cmp/cargo/registry` and redo it, or let cargo download. Nothing under `/root/work/zortax` or `/root/work/gui-bench-r6` was modified.
  `/root/work/cmp/cmpkit` holds a pushed copy of cmpkit (untested there).
* Other agent's built apps (read-only, `debug=1` so 150 to 470 MB; strip a copy before quoting size): `/root/work/zortax/gui-bench/apps/<fw>/target/release/<fw>-bench`; sources at `/root/work/zortax/gui-bench/apps/`. vizia renders black under lavapipe+wl_shm (layout check fails) but a headless harness does not need pixels.
* Headless drivers found in the framework sources (all in `/root/work/zortax/cargo/{registry/src,git/checkouts}`):
  floem `floem::headless::HeadlessHarness` (`new_with_size`, `pointer_*`, `click`, `process_update_no_paint`, `paint`; built for tests and benchmarks);
  freya `freya-testing::launch_test` (`TestingRunner`: `click_cursor`, `press_key`, `sync_and_update`, `process_events_and_layout`, `render_to_surface` optional);
  gpui `test-support` feature (gpui-pre 0.3.12 / gpui-base 0.9.1 / gpui-kit 0.9.1 forward it): `TestAppContext`, `VisualTestContext::draw`;
  zgui `zgui-platform-headless`, `zgui-testkit-view`, `zgui-bench` crates under `ref/zgui/crates`;
  egui: `Context::run` + `tessellate` needs no window; iced: `iced_test::Simulator` or a manual `UserInterface` with the tiny-skia renderer;
  vizia: no harness; `vizia_core::Context` plus the `BackendContext` update/style/layout passes the winit loop calls is the route (unverified).

## Next steps, per empty cell (in the order the lead asked)

1. Run cmpkit's selftest on ilo-prod (`push.sh`, then `cargo run --release --bin selftest` with `/root/work/cmp/env.sh`, `CARGO_BUILD_JOBS=2`, `nice -n 19`). If `perf_event_open` panics, fall back to the per-process `/proc` schedstat (CPU ns) and say so on every row.
2. One crate per framework in `comparisons/<fw>/` with `Cargo.toml` pinned to the gui-bench versions (egui/eframe 0.37.0, iced 0.14.0, floem git 1351ffb, freya 0.5.1 and git 056a5b4, vizia 0.4.0, gpui-kit 0.9.1 and `gpui-fast` feature, zgui path `zgui-fakeland-marks` ab5fe9f), each a `SignalApp` impl calling `run_signal` first. Idiomatic fine path per framework (as gui-bench's README table): zgui `RwSignal` per field + reactive holes; floem `RwSignal` per field + `dyn_stack`; vizia `Signal` + bindings; freya per-row `State`; gpui an `Entity` per row; iced `lazy` rows keyed by version; egui plain data. Also the coarse variant of each that has one. A turn = one write, then everything up to the point the framework would hand the frame to the GPU (no raster, no present). Write down in each RUN.md exactly where the turn ends.
3. Then the table ops with `run_ops` by reusing each gui-bench app's own `table/{coarse,fine}.rs` view and update code (n=10/100/1000/5000, select/update10/swap/remove first), then rows hover.
4. Run each binary at least twice as separate processes; the merger sets `confirmed` when instructions agree within 3 % and wall within 15 %.
5. Memory, startup, binary size: strip a copy of each existing release binary for size; startup and idle rss from fakeland via `/root/work/zortax/gui-bench/runner/target/release/gui-bench --out /root/work/cmp/fk/<fw> --scenarios rows --rows 1000 ...` niced (the other agent is running the same runner: check `/root/work/zortax/logs` first and do not overlap). These are Linux/lavapipe numbers and carry that machine label; do not put them on the Mac axis. A same-profile size build (bough `production`: lto, codegen-units 1, strip) of a minimal counter app per framework is the fair size comparison.
6. Anything that cannot be built or driven headless: extrapolate only with `extrapolated: true` and a `basis` naming the measured points (for example vizia: from its egui-class retained peers). Nothing is extrapolated now.
7. Merge to `matrix.json` (write a small `merge.py`: read every `raw/*.jsonl`, group by case/framework/variant/n/metric, median across runs, fill `runs`, `run_values`, `confirmed`) and add the series to `bench/r6/dashboard/data.json` with `add.py --kind measured`.

## Fairness risks in our existing charts (for the lead)

1. **Counter scope differs.** bough's gb6 cells are R0 "up to the GPU's input" on aarch64; zortax's published numbers are `cpu_ms` of the whole process under fakeland, including software rasterisation on lavapipe. Those must not share an axis with R0 instructions. Our competitor ports use R0-style scope, which still charges frameworks that paint straight into a canvas (vizia, freya/Skia) for recording work bough does later on the GPU.
2. **Mixed provenance in one series.** Many dashboard `base aa9943e` points are kind `reported`, `round6` points `measured`; the dumbbell connects them. Mark which are re-runs.
3. **Pixel gates.** `TABLE-CELLS.md` lists coarse-variant cells with `retained-fresh: N differing bytes` and a failed assertion (coarse select/update10/swap at n=10, replace-select). Its own text says "a failed pixel cell is not a speed win". The dashboard group title says "gates pass" for the `gb6-table-coarse-plain-*` series: check that the cited cells are the passing ones.
4. **Slope artefact** at 64 rows (above), and the colour n=16 `once` cell moved 649 to 765 between two runs.
5. **Only favourable shapes are charted for the signal path.** `Signal<T>` arrays are capped at 256 rows (`MAX`), one write per turn, writes spread by `(k*7) % n`. Frameworks whose cost is per write are fine; frameworks whose cost scales with rows (all coarse paths) look worst here. The good-for-them cases (replace, append, ticker with 100 updates) must be charted too; bough's replace is 2.9 M at 1,000 rows and 11 M at 5,000.
6. **Binary size and memory.** bough's 1.5 to 2.9 MB is a stripped production-profile counter on Arm; gui-bench's apps are `debug=1` multi-hundred-MB ELF files with a different feature set. Comparing without stripping and without a counter-equivalent app flatters bough. Mac resident includes the Metal driver; lavapipe's includes LLVM JIT.
7. **Wall time** on the Mac and ilo-prod is taken under load averages of 25 to 45; only instruction counts are load-robust. Say so on every wall figure.
