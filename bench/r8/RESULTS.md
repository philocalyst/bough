# bench-r8 results

Host `ilo-prod` (AMD Ryzen 7 3700X, 16 threads, Linux 6.18.54, governor powersave, boost on, no GPU), 2026-11-12 and 13. Everything here was run by one runner
(`wt/lead/bench-r8`) on top of zortax's gui-bench at 0fd45107 with fakeland 71695e7c. Nothing was optimised and no framework's code was changed.
The dashboard (`dashboard/`, published at https://claude.ai/artifact/2564xjff1QJabBAk9623Fj) draws the same data; `results_tables.md` holds every table in full.

## 0. Read this first

* **This is the round-7 façade, not the new core.** "bough" in every table is bough at commit `74e665786c` (branch `r7/platform`), the gui-bench app `comparisons/bough-guibench`
  on its CPU executor. It says nothing about the round-8 core. The e2e lane's new-core binary (`bough-x`) is a separate entry that will sit beside `bough`
  once it has a run directory that passed the runner's layout check; it is not in these tables.
* **One deployment configuration.** ilo-prod has no GPU. bough draws on one thread with its CPU executor into shared-memory buffers (1 thread in the idle process); iced, egui, gpui-kit, zgui,
  dioxus, freya and vizia render with wgpu or Skia on Mesa lavapipe and run 21 to 53 threads; the compositor is fakeland's pixman. This says which build works best on this software-rendered host. It is not a GPU
  ranking and not an equal-work or single-thread comparison.
* **CPU pinning is not isolation.** cmp-2 was not pinned and the 1-minute load went from 0.8 to 21.6 (mean 9.1). Every later run was pinned (runner, in-process compositor and apps) to CPUs 0-7 while other tenants were
  confined to CPUs 8-15 by the lead from about 21:30 UTC on 2026-11-12; CPU n and n+8 are SMT siblings of one core, so the pinned runs still shared cores, caches and memory bandwidth with other people's builds and profiling.
  Load during each run is in the table below. No randomised or ABBA block design was run.
* **Startup is unchecked.** The harness produces no validity report for startup, so every startup value here is "unchecked", never "valid", and is not ranked. cmp-2's startup is load-contaminated (below).
* **Estimates are heuristic.** Cells with no measurement carry an estimate (marked `*~x*`). The method crosses architectural cliffs and its ranges are not confidence intervals (section 8). Estimates are not ranked and not counted in any win or loss.
* **What a "valid" value is.** A value counts as valid only if the harness's own checks passed for its report (`validity.tsv`); a value without a validity record would be "unchecked". Values from reports that failed a check are shown with †. The only check that failed in any run is `wake_lateness`
  (compositor output wakes later than 2 ms). Each rankable cell is bound to the executable that produced it (sha256 in the manifest and in the dashboard tooltip) and to the runner's layout check (colours at fixed points), which every probe of a cell follows; there is no independent pixel or behaviour check beyond that.

## 1. State at takeover

The previous runner was stopped after `cmp-2` had completed: runner exit 0, a `results:` line, 186 of 186 cells, flat tables written (16 failed measurements, all floem's layout check). It was kept and not rerun. Nothing was running on ilo-prod under `/root/work/r8/bench`,
no half-run directory existed, and nothing was discarded. The host runs other people's builds (rustc, cargo, perf); cmp-2 saw that load rise from 0.8 to 21.6. Three `rx` rex windows (41cbc08f, f60708b0, 479c9a29) and the windows `zx-status`, `q1sum`, `q1b` in the shared rex session were left by earlier work and are not mine; every window opened by this runner was closed.

## 2. Runs

| run | what | timing model | pinning | window (UTC) | exit | builds | cells | reports | 1-min load mean (min to max) |
|---|---|---|---|---|---|---:|---|---|---|
| cmp-2 | headline matrix, taken over from the previous runner (complete, kept) | fixed 60 Hz | none | 2026-11-12T20:22:11Z to 2026-11-12T21:18:39Z | rc 0 | 12 | 186 (16 with errors) | 587 (130 invalid) | 9.1 (0.2 to 21.6) |
| heavy-1 | heavy scenes (sheet, canvas), 4 frameworks | fixed 60 Hz | CPUs 0-7 | 2026-11-12T21:40:20Z to 2026-11-12T21:55:21Z | rc 0 | 4 | 16 (1 with errors) | 86 (49 invalid) | 10.7 (8.5 to 14.9) |
| sweep-60 | refresh sweep: rows, typing, spinner; bough, iced, freya, dioxus-skia | fixed 60 Hz | CPUs 0-7 | 2026-11-12T21:55:54Z to 2026-11-12T22:02:12Z | rc 0 | 4 | 20 (0 with errors) | 48 (11 invalid) | 8.1 (5.9 to 12.0) |
| sweep-120 | same | fixed 120 Hz | CPUs 0-7 | 2026-11-12T22:02:12Z to 2026-11-12T22:07:38Z | rc 0 | 4 | 20 (0 with errors) | 48 (18 invalid) | 5.8 (4.7 to 8.3) |
| sweep-240 | same | fixed 240 Hz | CPUs 0-7 | 2026-11-12T22:07:39Z to 2026-11-12T22:13:05Z | rc 0 | 4 | 20 (0 with errors) | 48 (21 invalid) | 3.8 (2.8 to 6.3) |
| cmp-u1 | headline matrix, patched runner | unthrottled | CPUs 0-7 | 2026-11-12T22:58:04Z to 2026-11-12T23:52:57Z | rc 0 | 12 | 186 (17 with errors) | 585 (3 invalid) | 4.7 (1.0 to 17.6) |
| heavy-u1 | heavy scenes | unthrottled | CPUs 0-7 | 2026-11-12T23:52:58Z to 2026-11-13T00:05:41Z | rc 0 | 4 | 16 (1 with errors) | 87 (2 invalid) | 12.7 (7.4 to 16.6) |
| cmp-v1 | headline matrix | vrr 48-144 Hz | CPUs 0-7 | 2026-11-13T00:05:43Z to 2026-11-13T01:01:49Z | rc 0 | 12 | 186 (16 with errors) | 587 (195 invalid) | 10.2 (3.7 to 17.4) |
| heavy-v1 | heavy scenes | vrr 48-144 Hz | CPUs 0-7 | 2026-11-13T01:01:59Z to 2026-11-13T01:14:49Z | rc 0 | 4 | 16 (1 with errors) | 86 (47 invalid) | 5.4 (4.2 to 7.2) |
| smoke-u1 | probe-semantics check (bough, iced), not reported as results | unthrottled | CPUs 0-7 | 2026-11-12T22:50:41Z to 2026-11-12T22:54:08Z | rc 0 | 2 | 12 (0 with errors) | 30 (2 invalid) | 7.3 (5.2 to 8.8) |
| smoke-v1 | probe-semantics check (bough, iced), not reported as results | vrr 48-144 Hz | CPUs 0-7 | 2026-11-12T22:54:53Z to 2026-11-12T22:58:03Z | rc 0 | 2 | 10 (0 with errors) | 24 (5 invalid) | 10.6 (7.9 to 13.8) |

Every run directory was re-checked after the fact with `tools/run_status.py` (`results/RUN-STATUS.tsv`): runner exit code 0 in the identity record, a `results:` line, the announced number of cells, matching cell tables, non-empty flat tables. All eleven are COMPLETE.
(The review's B09 finding is right that `run-gb.sh` ended with an `echo`, so its own exit status would have been 0 whatever the runner did; `run-gb.sh` and `run-gb2.sh` now exit with the runner's code, write `STATUS.json`, and `unthrottled.sh` stops at the first failed run. None of the runs above hit that path.)

### Identity and method

* Commits: gui-bench 0fd45107d64b (dirty overlay: the bench lane's scene kinds, bough registered; `runner/src` and `apps/shared` hashes in `results/EFFECTIVE-SOURCE.txt`), fakeland 71695e7c5315 (clean), bough 74e665786cbd (plain app, clean tree), bough scenes tree 74e665786cbd + overlay (bough-guibench-scenes, bough-workloads, `bough-mem/pmu.rs` from r7/bench 580bcd2981). Toolchain: bough's pinned nightly in `/root/work/r8/bench/rustup`; bough-guibench `[profile.release] debug=1, lto=thin`.
* Binaries (sha256[:16]): bough plain `52e30d0a171c0672` (55,506,840 bytes), bough scenes `4c9a8a961d4fe29c` (70,127,584), original runner `e7fb33095c6f86d3`, patched runner `0a347d1bc50ca5b5`, headless driver (profiles) `b0c050824ea6db3b`. The other eleven builds are zortax's, read-only at `/root/work/zortax/gui-bench/apps/*/target`, hashes in each run's `IDENTITY.txt`; iced, egui and gpui-kit in the heavy runs are the bench lane's scene builds.
* Fixed runs: `run-gb.sh NAME zortax|scenes ARGS` (cmp-2 used the same command with `--quick --scenarios rows,table,ticker,spinner,typing,search,virtual,resize`). The matrix is all twelve builds in one runner invocation; heavy runs use `--scenarios sheet,canvas --frameworks bough,iced,egui,gpui-kit` (default effort: sheet 1,000 and 10,000 rows, canvas 1,000 and 5,000 nodes); sweeps use `--quick --scenarios rows,typing,spinner --frameworks bough,iced,freya,dioxus-skia --hz R`.
* Unthrottled and VRR (amendment 6): the runner had no timing option (it builds `OutputConfig::new("DP-1").mode(1280,800,Hz(60))` in `runner/src/cell.rs`). Smallest patch, `scripts/patch-timing.py` (diff in `patches/gb-timing.patch`, sha256 93f7ab8f5c68): environment variable `GB_TIMING` = `fixed` (default, unchanged) | `unthrottled` (`TimingConfig::unthrottled()`) | `vrr:MIN-MAX` (`TimingConfig::vrr`) passed to `OutputConfig::timing`. Built into a separate target (`target-runner-timing`, runner sha256 0a347d1bc50c); `GUIBENCH=` selects it in `run-gb2.sh`, which records the patch hash and `GB_TIMING` in each identity record. The advertised output stays 1280x800 at 60 Hz; only the scheduling model changes.
* Pinning: `taskset -c 0-7 nice -n 10` on the runner for every run from heavy-1 on; fakeland is in the runner process and the apps inherit the mask (checked: affinity 0-7 on runner and app processes).
* Profiles: `perf` 7.3.1 at `/nix/store/bw8frr0nk26m740a2mwaw5yi06mgxakb-perf-linux-7.3.1`, flamegraphs by nixpkgs `inferno` 0.12.8 (store path `/nix/store/qxrk44bap2vnldsmni7882ym9iyl8mvy-inferno-0.12.8`, built earlier by another session's `nix build`; nothing installed). No system setting was changed: governor powersave, boost on, `perf_event_paranoid` 2, NMI watchdog on.

## 3. Unthrottled view (`cmp-u1`, `heavy-u1`): the app's latency with no compositor wait

fakeland presents a frame when the app has it ready; there is no refresh grid. Checked with `smoke-u1` and in cmp-u1 itself what each probe measures:

* **Meaningful, as fast as the app can go:** hover, typing, search, table-operation and resize latency; startup to first present; idle CPU and memory; spinner and ticker frame rate and CPU per frame (nothing paces them but the app).
* **Lose their meaning:** missed vblanks, pacing error, jank fraction, presents or commits per refresh (all against a nominal 60 Hz that no longer exists); the ready-to-latch and latch-to-present parts of the split (about 0.03 ms, noise, sometimes slightly negative); fps against a refresh. Scroll fps follows the harness's 240 Hz input, not the app.
* **Still applied:** the harness's validity checks (they concern the compositor's wakes and the host). 3 of 585 reports in cmp-u1 failed `wake_lateness`, against 130 of 587 in cmp-2; most of that difference is the lower, pinned load and fewer output wakes, not a different standard.
* An app that paces itself shows its own ceiling: **bough draws the spinner and ticker at about 62.5 fps even unthrottled** (16 ms interval). freya reaches 153 fps and egui 1,248 fps (no rows).

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

Values: ms unless noted; † = invalid report; startup columns are unchecked (shown without a mark here, see section 0); `*~x*` = estimate, never ranked.

Lowest valid value in 35 metrics, behind another framework in 3.

Behind (valid measurements only):

- spinner (no rows) fps (fps): bough 62.5, best valid egui 1,248
- spinner 10,000 fps (fps): bough 62.5, best valid freya 153.1
- ticker fine 100 updates fps (fps): bough 62.5, best valid iced 62.5

Not ranked, startup (unchecked): rows 3,000 bough 56.3 ms against iced 190, egui 179; virtual 100,000 bough 39.1 ms against vizia 104.5, egui 121.8; spinner without rows 38.4 ms.

Failures in cmp-u1: floem failed the layout check in 16 cells (colour at (640,31) or (640,71), `#f2f3f5` for the spec's `#e5e7eb`); **vizia typing** failed the layout check on this run (black pixels at the four points: the typing app painted nothing); nothing else failed. cmp-u1 had 17 failed measurements in all.

### Heavy scenes, unthrottled (heavy-u1; measured: bough, iced, egui, gpui-kit; the others are estimates)

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

Sheet 10,000 rows:

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

Canvas 5,000 nodes:

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

The sheet at 10,000 rows, bough: the resize probe failed ("no matching window", cell `sheet/bough/default/60hz/10000`); canvas and the other sheet probes completed. All four sizes are in `results_tables.md`.

## 4. Fixed 60 Hz view (`cmp-2`, `heavy-1`, sweeps): with the split

Every latency here contains a wait for the next compositor latch, about 10 ms for every framework (9.3 to 11 ms), which hides the app's share. For bough rows-3,000 hover, 11.52 ms total is 0.93 ms in the app (input to commit), 9.58 ms waiting for the latch and 1.00 ms from latch to present. The split segments are medians of separately pooled samples: they do not add up to the total median and the percentiles above p50 compose even less (the table below shows the harness's four parts; a part can exceed the total).

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

Lowest valid value in 35 metrics, behind another framework in 2.

Behind (valid measurements only):

- resize configure to present p50 (ms): bough 17.22, best valid iced 14.41
- resize configure to commit p50 (app share) (ms): bough 9.14, best valid iced 9.07

**Startup in this view is load-contaminated and unchecked.** In cmp-2 bough read 564.9 ms (virtual 100,000) and 681.8 ms (spinner without rows, nothing to mount); in the pinned unthrottled and VRR runs the same cells read 39.1 and 38.4 ms (unthrottled) and 47.5 and 47.2 ms (vrr). Other frameworks moved 2 to 5 times between cmp-2 and cmp-u1 as well. An earlier note that bough was "7th at 564.9 ms" came from this contaminated value and must not be used.

cmp-3 (the repeat of cmp-2 for run-to-run spread) was **not run**: the host was never quiet (1-minute load 2.8 to 21.6 in every window, other tenants building and profiling throughout). Run-to-run spread is therefore not available for the full matrix. The only same-cell pairs are cmp-2 (unpinned, load 9) and sweep-60 (pinned, load 8) for the four frameworks of the sweep, and the unthrottled smoke run against cmp-u1:

| cell | bough | iced | freya | dioxus-skia |
|---|---:|---:|---:|---:|
| rows 3,000 hover total p50 ms (cmp-2 / sweep-60) | 11.52 / 13.31 | 14.02 / 15.12 † | 15.12 / 15.12 † | 15.12 / 15.12 |
| rows 3,000 hover input to commit ms (cmp-2 / sweep-60) | 0.93 / 1.09 | 3.74 / 6.49 † | 4.47 / 5.11 † | 5.81 / 6.39 |
| typing p50 ms (cmp-2 / sweep-60) | 8.44 / 11.09 | 18.74 † / 14.34 † | 18.00 † / 17.24 † | 18.19 † / 14.08 † |
| spinner 10,000 CPU per frame ms (cmp-2 / sweep-60) | 1.13 / 1.15 | 133.0 † / 91.83 † | 24.93 / 23.61 | 39.34 / 30.43 † |

(bough, unthrottled, rows-3,000 hover total p50: smoke-u1 0.98 ms, cmp-u1 0.99 ms; typing 0.30 and 0.25 ms.)

### Heavy scenes, fixed (heavy-1)

Lowest valid value in 9 metrics, behind another framework in 3.

Behind (valid measurements only):

- sheet 1,000 rows resize configure to present p50 (ms): bough 20.4, best valid egui 16.2
- sheet 10,000 rows edit total p50 (ms): bough 26.32, best valid egui 14.95
- sheet 10,000 rows idle RSS (MiB): bough 332.6, best valid egui 156.9

49 of 86 reports in heavy-1 failed `wake_lateness`, so few cells are valid and the win and loss counts rest on few comparisons. Sheet 10,000 rows:

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

Canvas 5,000 nodes:

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

**Last time bough lost the canvas, the 10,000-row sheet's memory, startup and resize. Is that still so?** Yes for all four, in every timing mode. Canvas: bough hover 925.8 ms (invalid report) against iced 85 and egui 94 at 60 Hz, 83 ms against iced 17.6 at 1,000 nodes and theme switch 1,151 ms against egui 71 unthrottled. Sheet 10,000 rows idle RSS: bough 331 to 333 MiB against egui 149 to 157 and gpui-kit 187 (iced 550); bough is better than iced and worse than egui and gpui-kit. Startup (unchecked): sheet 10,000 rows 677 to 713 ms against egui 130 to 246; canvas 5,000 about 1.2 s against egui 257 to 265. Resize: bough sheet 10,000 rows failed twice (below). Against the first heavy matrix (r7-scenes-1, bough at d4393497a0, different conditions): sheet 10,000 hover 29.4 ms now against 14.6 then, input to commit 17.9 against 5.1; see the "then and now" table in `results_tables.md` (the two runs differ in bough commit, pinning and load, so it is direction, not a controlled comparison).

### Latency split (fixed 60 Hz, cmp-2)

See `results_tables.md` ("Latency split"); the bough hover split is above, the app's share for typing is 0.43 ms of 8.44, for table update 0.44 ms of 12.1 (sweep and cmp-2 values).

### Refresh sweep (fixed grid at 60, 120 and 240 Hz; bough, iced, freya, dioxus-skia; rows 3,000 hover, typing, spinner)

bough rows-3,000 hover total p50 13.3 / 7.0 / 3.7 ms at 60 / 120 / 240 Hz: the app's input to commit stays 1.09 / 0.98 / 1.03 ms, the wait for the latch falls 11.3 / 5.0 / 1.7 ms with the refresh period, latch to present 1.0 ms. iced's app share is 6.5 / 5.3 / 6.3 ms, freya 5.1 / 6.0 / 5.9, dioxus-skia 6.4 / 7.3 / 7.6 (several of those reports are invalid). Typing p50: bough 11.1 / 4.5 / 3.4 ms with 0.4 ms of handling. Spinner above 10,000 rows: bough stays at 62.5 fps at 120 and 240 Hz (its own pacing); freya 98 fps at 240 Hz; iced about 18 fps. Full table in `results_tables.md`; all 60 reports per rate are flat-tabled in `results/sweep-*/flat`.

## 5. Variable refresh view (`cmp-v1`, `heavy-v1`; vrr 48-144 Hz)

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

Lowest valid value in 30 metrics, behind another framework in 3.

Behind (valid measurements only):

- spinner (no rows) fps (fps): bough 62.5, best valid freya 144.0
- spinner 10,000 fps (fps): bough 62.5, best valid freya-056a5b4 140.6
- ticker fine 100 updates fps (fps): bough 62.5, best valid egui 62.5

195 of 587 reports in cmp-v1 and 47 of 86 in heavy-v1 failed `wake_lateness` (VRR wake timing under load). Latency sits between the two other views: bough rows-3,000 hover total 4.1 ms in the smoke run (1.46 ms app share, about 2.6 ms waiting for the shortest VRR period of 6.9 ms).

## 6. Animation

The spinner scenarios are the only animation cells that exist (a turning spinner with no rows and one above 10,000 static rows). **A richer animation benchmark does not exist yet**: no transitions, springs, many simultaneous animations, animated layout or text. The ticker is a data-update load, not an animation. Fixed grid (60 Hz) frame pacing and CPU per frame for all twelve builds, and the unthrottled and VRR rates, are in `results_tables.md` ("Animation"); the dashboard has them per mode. Headline (spinner above 10,000 rows, CPU per frame in ms; fps): unthrottled bough 1.15 ms at 62.5 fps, zgui 12.1 at 60, freya 17.3 at 153, iced 78.9 at 22.9, egui 70.7 at 17.9, gpui-kit 349 at 3.7 (many of those unthrottled values are valid; see tables).

## 7. Estimates, and why they are only estimates

Method (`tools/estimate.py`): per class of quantity (input latency, CPU per frame, frame interval, memory, idle CPU, startup, frame rate) log value = framework level + column level, fitted by median polish on valid values only; the framework level needs two own valid cells in the class (else a cross-class level over all its cells, flagged); the range is the estimate x exp(+-2 s) with s from the median absolute residual of the framework's own cells (floors 0.15, 0.30 cross-class); values are rounded to two significant figures. Estimates fill failed cells (floem in all but one cell, vizia typing in cmp-u1, bough's failed probes) and the heavy scenes for the eight frameworks never ported to them.

**Limits:** the model assumes multiplicative scaling and cannot represent a cliff (fallback path, capacity overflow, layout failure, worker start-up, full re-bin); missing cells are not random (failed or unported cells are often the expensive ones); an estimate for a workload that failed or was never implemented is not evidence that it is supported; the cross-class fallback has no physical basis. Held-out tests of the method (hide valid cells it can see):

| view | hold-out test | tests | stated range held the true value | median error | 90th percentile | worst |
|---|---|---:|---:|---:|---:|---:|
| Unthrottled | hide one valid cell, estimate it from the rest | 662 | 71 % | x1.4 | x6 | x56 |
| Unthrottled | keep only the spinner-without-rows cells of a framework (floem's situation), estimate the rest | 608 | 74 % | x1.8 | x7 | x1,689 |
| Fixed 60 Hz grid | hide one valid cell, estimate it from the rest | 429 | 73 % | x1.3 | x5 | x35 |
| Fixed 60 Hz grid | keep only the spinner-without-rows cells of a framework (floem's situation), estimate the rest | 371 | 75 % | x1.5 | x8 | x6,406 |
| Variable refresh | hide one valid cell, estimate it from the rest | 375 | 76 % | x1.3 | x4 | x20 |
| Variable refresh | keep only the spinner-without-rows cells of a framework (floem's situation), estimate the rest | 327 | 77 % | x1.6 | x6 | x2,183 |

The stated ranges are meant to hold about 95 % and hold the true value in about three of four tests; the worst errors are the CPU executor's cliffs (bough canvas theme switch measured 1,151 ms, estimated 21 ms from bough's other cells; bough idle CPU, a value near zero nothing else resembles). Treat every estimate as an order of magnitude at best. Estimates appear in the tables as `*~x*`, are drawn dotted below the measured rows on the dashboard, and are excluded from every rank and every win or loss list.

## 8. Profiles of bough's hot paths (the round-7 façade; not timings)

Method: the headless driver of `comparisons/bough-guibench` (`--render`: CPU executor, no window host) for rows hover and the eight fine table operations; for the sheet at 10,000 rows (hover, theme) and the canvas at 5,000 nodes (hover), the gui-bench cell itself (scenes binary `4c9a8a961d4fe29c`, CPU executor, real fakeland window host) with `perf` attached to the app process.
`perf stat` in groups of three events, user mode only, one group per run: cycles + instructions + branches; instructions + branches + branch-misses; instructions + L1-dcache-loads + L1-dcache-load-misses; instructions + cache-references + cache-misses (on this Zen 2 CPU `cache-misses` are **L2 misses** and L1-dcache-load-misses are L2 requests from the data cache; there are no LLC events); instructions + `bp_dyn_ind_pred` + `ex_ret_brn_ind_misp` (indirect branches predicted by the indirect predictor and mispredicted indirect branches; headless paths only: the attached-cell indirect runs produced no counts). Headless paths: slope between 100 and 1,000 repeats (three perf runs each), pinned to CPU 2, per operation (an operation is one input, the hover onto or away from a row, or one key press, with its frame), so mount and start-up cancel. Cells: one cell run per group, `perf stat -I 50` summed over the harness's measured-iteration windows (its own monotonic timestamps), divided by the number of measured repeats (3 iterations x 12 repeats for hover, x 8 for theme; one repeat = pointer onto the target and away, or one theme switch); the windows include the harness's settle time, which adds few instructions. Windows are aligned to within 50 ms.
`perf record` with DWARF call graphs (headless 4,999 Hz on one pinned CPU; cells 999 Hz on the app process), flamegraphs by inferno; per-probe flamegraphs for cells use only the samples inside the measured windows. Sample weights are cycles. The PMU was shared with other sessions' perf runs, so every group ran on the counters about 75 % of the time (counts are scaled; ratios inside one group are exact), and counts under a loaded SMT host carry more noise than the repeat statistics suggest.

| path | instructions / op | cycles / op | IPC | L1d misses / op | L2 misses / op | branches / op | branch misses / op | indirect predictions / op | indirect mispredicts / op | L1d per 1,000 instr | L2 per 1,000 instr | branch misses per 1,000 instr |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| rows 3,000 hover | 7,585,701 | 3,353,948 | 2.26 | 84,078 | 17,596 | 1,090,532 | 1,842 | 118.8 | 39.1 | 11.1 | 2.32 | 0.24 |
| table fine select | 2,255,771 | 1,100,660 | 2.05 | 8,019 | 5,339 | 275,780 | 933.6 | 11.1 | 5.3 | 3.6 | 2.37 | 0.41 |
| table fine update 10th rows | 1,616,231 | 917,099 | 1.76 | 7,635 | 6,732 | 225,432 | 810.4 | 27.1 | 21.7 | 4.7 | 4.17 | 0.50 |
| table fine swap | 2,429,759 | 1,342,583 | 1.81 | 10,297 | 10,290 | 301,999 | 1,520 | 5.7 | 122.1 | 4.2 | 4.23 | 0.63 |
| table fine remove | 25,344,039 | 12,563,324 | 2.02 | 89,311 | 59,725 | 3,038,764 | 8,256 | 23.6 | 75.9 | 3.5 | 2.36 | 0.33 |
| table fine insert | 25,349,712 | 11,533,898 | 2.20 | 88,089 | 58,436 | 3,039,445 | 8,257 | 9.5 | 72.8 | 3.5 | 2.31 | 0.33 |
| table fine replace | 27,788,469 | 14,110,495 | 1.97 | 112,565 | 65,719 | 3,463,450 | 10,569 | 133.3 | 82.8 | 4.1 | 2.36 | 0.38 |
| table fine append | 1,057,897 | 403,035 | 2.62 | 5,991 | 1,867 | 201,445 | 1,473 | 22.4 | 8.6 | 5.7 | 1.76 | 1.39 |
| table fine clear | 24,435,309 | 12,204,678 | 2.00 | 102,270 | 64,979 | 2,911,608 | 7,987 | 64.7 | 86.5 | 4.2 | 2.66 | 0.33 |
| sheet 10,000 rows hover (gui-bench cell) | 76,423,601 | 68,395,006 | 1.12 | 1,448,544 | 485,646 | 11,229,310 | 46,722 | - | - | 20.4 | 6.58 | 0.60 |
| sheet 10,000 rows theme switch (gui-bench cell) | 138,978,645 | 99,262,187 | 1.40 | 1,885,776 | 549,469 | 18,895,191 | 53,597 | - | - | 14.6 | 3.88 | 0.37 |
| canvas 5,000 nodes hover (gui-bench cell) | 6,602,828,311 | 3,632,033,897 | 1.82 | 645,782 | 239,397 | 505,603,884 | 1,295,808 | - | - | 0.1 | 0.04 | 0.20 |

What the profiles show:

* **Sheet 10,000 rows hover (the gui-bench cell) looks like dispatch and pointer chasing.** 76 M instructions per hover at IPC 1.12, 20 L1d and 6.6 L2 misses per 1,000 instructions (the table operations run at IPC 1.8 to 2.6 with 3.5 to 6 L1d misses per 1,000). Self time: `SealCheck::word` 8.5 %, `memmove` 7.9 %, `Rt::run_head` 7.9 %, `split_at_checked` 5.6 %, `i32::clamp` 4.9 %, `Spec::same` 4 %. `run_head` is the wire-chain walk (the linked chain from a signal's head, one indirect call per wire) and `SealCheck::word` a per-word integrity check; frames that are closures, `call_once` or chain walkers hold 28 % of self time. Theme switch at the same size: 139 M instructions at IPC 1.40, same top symbols.
* **Canvas 5,000 nodes hover is arithmetic.** 6.6 G instructions per hover at IPC 1.82 and almost no cache misses (0.1 L1d and 0.04 L2 per 1,000 instructions): `cpu_raster::shadow`, `erf2` and `expf` are about 60 % of self time (blurred shadows by an exp-based error function per pixel on the CPU executor). This is why the canvas loses: it is the executor's shadow math, not the framework's update path.
* **Rows hover through the headless driver is copying and allocation.** 7.6 M instructions per hover at IPC 2.26, 84 K L1d and 17.6 K L2 misses per operation; `memmove` 46 %, `memcmp` 12 %, `free` 11 %, `malloc` 9 %, `memset` 5 %, `realloc` 4 % of cycles; only 119 indirect predictions and 39 indirect mispredicts per operation: dispatch is not the cost on this path, copying a surface and allocating are.
* **Table operations**: remove, insert, replace and clear cost 24 to 28 M instructions (one operation re-rasters many tiles; `CpuRaster::tile`, `blend_span` and `memset` lead), select, update10, swap and append cost 1 to 2.4 M. Branch mispredictions are low everywhere (0.2 to 1.4 per 1,000 instructions).

Top ten symbols by self time per path (and the same time charged to the nearest Rust frame, so that libc leaves such as `memmove` and `malloc` are attributed to their callers where a Rust frame exists):

### rows 3,000 hover (flamegraph `profiles/svg/rows3k-hover.svg`)

| # | self time | symbol (top ten by self time) | charged to nearest Rust frame | symbol |
|---:|---:|---|---:|---|
| 1 | 46.2 % | `__memmove_avx_unaligned_erms` | 46.1 % | `__memmove_avx_unaligned_erms` |
| 2 | 11.9 % | `__memcmp_avx2_movbe` | 11.9 % | `__memcmp_avx2_movbe` |
| 3 | 10.7 % | `cfree@GLIBC_2.2.5` | 10.7 % | `cfree@GLIBC_2.2.5` |
| 4 | 9.2 % | `malloc` | 9.2 % | `malloc` |
| 5 | 5.1 % | `__memset_avx2_unaligned_erms` | 5.1 % | `__memset_avx2_unaligned_erms` |
| 6 | 4.2 % | `realloc` | 4.1 % | `realloc` |
| 7 | 3.8 % | `__memmove_avx_unaligned` | 3.8 % | `__memmove_avx_unaligned` |
| 8 | 0.3 % | `clear_page_rep` | 0.4 % | `core::ptr::write::<bough::layout::LeafCtx>` |
| 9 | 0.3 % | `__pgalloc_tag_sub` | 0.3 % | `__pgalloc_tag_sub` |
| 10 | 0.2 % | `asm_sysvec_apic_timer_interrupt` | 0.3 % | `std::sys::alloc::unix::alloc` |

Process headless, 1,741,195,978 cycles sampled; 1.8 % of the recording in other processes (excluded). Dispatch-like frames (closures, call_once, vtable or shim frames, run_head): 0.4 % of self time.

### table fine select (flamegraph `profiles/svg/table-select.svg`)

| # | self time | symbol (top ten by self time) | charged to nearest Rust frame | symbol |
|---:|---:|---|---:|---|
| 1 | 25.5 % | `core::ptr::write::<core::ptr::Unaligned<core::core_arch::x86::__m128i>>` | 25.5 % | `core::ptr::write::<core::ptr::Unaligned<core::core_arch::x86::__m128i>>` |
| 2 | 16.1 % | `bough::cpu_raster::x86::blend_span` | 16.6 % | `bough::cpu_raster::x86::blend_span` |
| 3 | 12.1 % | `<bough::cpu_raster::CpuRaster>::tile::<true>` | 12.2 % | `<bough::cpu_raster::CpuRaster>::tile::<true>` |
| 4 | 7.5 % | `core::core_arch::x86::f16c::_mm256_cvtph_ps` | 7.5 % | `core::core_arch::x86::f16c::_mm256_cvtph_ps` |
| 5 | 5.9 % | `core::core_arch::x86::fma::_mm256_fmadd_ps` | 5.9 % | `core::core_arch::x86::fma::_mm256_fmadd_ps` |
| 6 | 3.4 % | `__memset_avx2_unaligned_erms` | 3.3 % | `__memset_avx2_unaligned_erms` |
| 7 | 2.9 % | `bough::cpu_raster::x86::store_row` | 3.0 % | `bough::cpu_raster::x86::store_row` |
| 8 | 1.9 % | `<core::ops::range::Range<i32> as core::iter::range::RangeIteratorImpl>::spec_next` | 2.0 % | `<core::ops::range::Range<i32> as core::iter::range::RangeIteratorImpl>::spec_nex` |
| 9 | 1.8 % | `core::core_arch::x86::avx::_mm256_cvtps_epi32` | 1.8 % | `core::core_arch::x86::avx::_mm256_cvtps_epi32` |
| 10 | 1.5 % | `core::core_arch::x86::avx::_mm256_min_ps` | 1.5 % | `core::core_arch::x86::avx::_mm256_min_ps` |

Process headless, 1,839,381,301 cycles sampled; 1.7 % of the recording in other processes (excluded). Dispatch-like frames (closures, call_once, vtable or shim frames, run_head): 1.1 % of self time.

### table fine update 10th rows (flamegraph `profiles/svg/table-update10.svg`)

| # | self time | symbol (top ten by self time) | charged to nearest Rust frame | symbol |
|---:|---:|---|---:|---|
| 1 | 15.1 % | `<bough::cpu_raster::CpuRaster>::tile::<true>` | 15.2 % | `<bough::cpu_raster::CpuRaster>::tile::<true>` |
| 2 | 13.2 % | `core::ptr::write::<core::ptr::Unaligned<core::core_arch::x86::__m128i>>` | 13.2 % | `core::ptr::write::<core::ptr::Unaligned<core::core_arch::x86::__m128i>>` |
| 3 | 8.1 % | `__memset_avx2_unaligned_erms` | 7.6 % | `bough::cpu_raster::x86::blend_span` |
| 4 | 7.5 % | `bough::cpu_raster::x86::blend_span` | 7.6 % | `__memset_avx2_unaligned_erms` |
| 5 | 3.5 % | `_int_malloc` | 5.3 % | `core::ptr::write_bytes::<u8>` |
| 6 | 3.5 % | `core::core_arch::x86::f16c::_mm256_cvtph_ps` | 3.5 % | `core::core_arch::x86::f16c::_mm256_cvtph_ps` |
| 7 | 1.9 % | `<core::ops::range::Range<usize> as core::slice::index::SliceIndex<[f16]>>::index_mut` | 3.0 % | `_int_malloc` |
| 8 | 1.9 % | `core::core_arch::x86::fma::_mm256_fmadd_ps` | 1.9 % | `<core::ops::range::Range<usize> as core::slice::index::SliceIndex<[f16]>>::index` |
| 9 | 1.6 % | `_int_free_merge_chunk` | 1.9 % | `core::core_arch::x86::fma::_mm256_fmadd_ps` |
| 10 | 1.6 % | `<bough::cpu_raster::CpuRaster>::tile::<true>::{closure#0}` | 1.6 % | `<bough::cpu_raster::CpuRaster>::tile::<true>::{closure#0}` |

Process headless, 477,159,480 cycles sampled; 6.3 % of the recording in other processes (excluded). Dispatch-like frames (closures, call_once, vtable or shim frames, run_head): 2.7 % of self time.

### table fine swap (flamegraph `profiles/svg/table-swap.svg`)

| # | self time | symbol (top ten by self time) | charged to nearest Rust frame | symbol |
|---:|---:|---|---:|---|
| 1 | 23.0 % | `core::ptr::write::<core::ptr::Unaligned<core::core_arch::x86::__m128i>>` | 23.1 % | `core::ptr::write::<core::ptr::Unaligned<core::core_arch::x86::__m128i>>` |
| 2 | 14.1 % | `bough::cpu_raster::x86::blend_span` | 14.3 % | `bough::cpu_raster::x86::blend_span` |
| 3 | 12.2 % | `<bough::cpu_raster::CpuRaster>::tile::<true>` | 12.2 % | `<bough::cpu_raster::CpuRaster>::tile::<true>` |
| 4 | 6.5 % | `core::core_arch::x86::f16c::_mm256_cvtph_ps` | 6.5 % | `core::core_arch::x86::f16c::_mm256_cvtph_ps` |
| 5 | 6.1 % | `__memset_avx2_unaligned_erms` | 5.6 % | `core::core_arch::x86::fma::_mm256_fmadd_ps` |
| 6 | 5.5 % | `core::core_arch::x86::fma::_mm256_fmadd_ps` | 3.7 % | `__memset_avx2_unaligned_erms` |
| 7 | 2.7 % | `bough::cpu_raster::x86::store_row` | 2.7 % | `bough::cpu_raster::x86::store_row` |
| 8 | 1.6 % | `core::core_arch::x86::avx::_mm256_mul_ps` | 2.2 % | `<bough::cpu_raster::CpuRaster>::repair` |
| 9 | 1.5 % | `<core::core_arch::x86::__m128i>::as_i32x4` | 1.6 % | `core::core_arch::x86::avx::_mm256_mul_ps` |
| 10 | 1.3 % | `<core::ops::range::Range<i32> as core::iter::range::RangeIteratorImpl>::spec_next` | 1.5 % | `core::ptr::write_bytes::<u8>` |

Process headless, 1,519,570,690 cycles sampled; 2.1 % of the recording in other processes (excluded). Dispatch-like frames (closures, call_once, vtable or shim frames, run_head): 1.1 % of self time.

### table fine remove (flamegraph `profiles/svg/table-remove.svg`)

| # | self time | symbol (top ten by self time) | charged to nearest Rust frame | symbol |
|---:|---:|---|---:|---|
| 1 | 22.4 % | `core::ptr::write::<core::ptr::Unaligned<core::core_arch::x86::__m128i>>` | 22.4 % | `core::ptr::write::<core::ptr::Unaligned<core::core_arch::x86::__m128i>>` |
| 2 | 13.9 % | `<bough::cpu_raster::CpuRaster>::tile::<true>` | 13.9 % | `<bough::cpu_raster::CpuRaster>::tile::<true>` |
| 3 | 13.1 % | `bough::cpu_raster::x86::blend_span` | 13.4 % | `bough::cpu_raster::x86::blend_span` |
| 4 | 6.9 % | `core::core_arch::x86::f16c::_mm256_cvtph_ps` | 6.9 % | `core::core_arch::x86::f16c::_mm256_cvtph_ps` |
| 5 | 5.4 % | `core::core_arch::x86::fma::_mm256_fmadd_ps` | 5.4 % | `core::core_arch::x86::fma::_mm256_fmadd_ps` |
| 6 | 5.3 % | `core::core_arch::x86::sse2::_mm_packs_epi32` | 5.3 % | `core::core_arch::x86::sse2::_mm_packs_epi32` |
| 7 | 4.5 % | `<core::core_arch::x86::__m128i>::as_i32x4` | 4.5 % | `<core::core_arch::x86::__m128i>::as_i32x4` |
| 8 | 4.5 % | `bough::cpu_raster::x86::store_row` | 4.5 % | `bough::cpu_raster::x86::store_row` |
| 9 | 1.9 % | `__memset_avx2_unaligned_erms` | 1.9 % | `<bough::cpu_raster::CpuRaster>::repair` |
| 10 | 1.8 % | `core::core_arch::x86::avx::_mm256_min_ps` | 1.8 % | `core::core_arch::x86::avx::_mm256_min_ps` |

Process headless, 8,097,285,666 cycles sampled; 0.4 % of the recording in other processes (excluded). Dispatch-like frames (closures, call_once, vtable or shim frames, run_head): 0.9 % of self time.

### table fine insert (flamegraph `profiles/svg/table-insert.svg`)

| # | self time | symbol (top ten by self time) | charged to nearest Rust frame | symbol |
|---:|---:|---|---:|---|
| 1 | 74.2 % | `__memset_avx2_unaligned_erms` | 74.2 % | `__memset_avx2_unaligned_erms` |
| 2 | 1.1 % | `__memmove_avx_unaligned_erms` | 1.1 % | `__memmove_avx_unaligned_erms` |
| 3 | 0.8 % | `update_load_avg` | 0.8 % | `update_load_avg` |
| 4 | 0.7 % | `asm_sysvec_apic_timer_interrupt` | 0.7 % | `asm_sysvec_apic_timer_interrupt` |
| 5 | 0.7 % | `_int_malloc` | 0.7 % | `_int_malloc` |
| 6 | 0.6 % | `update_curr` | 0.6 % | `update_curr` |
| 7 | 0.6 % | `_raw_spin_lock` | 0.6 % | `_raw_spin_lock` |
| 8 | 0.5 % | `native_write_msr` | 0.5 % | `native_write_msr` |
| 9 | 0.4 % | `perf_adjust_freq_unthr_context` | 0.4 % | `perf_adjust_freq_unthr_context` |
| 10 | 0.4 % | `tick_nohz_handler` | 0.4 % | `tick_nohz_handler` |

Process headless, 1,535,884,938 cycles sampled; 2.7 % of the recording in other processes (excluded). Dispatch-like frames (closures, call_once, vtable or shim frames, run_head): 0.0 % of self time.

### table fine replace (flamegraph `profiles/svg/table-replace.svg`)

| # | self time | symbol (top ten by self time) | charged to nearest Rust frame | symbol |
|---:|---:|---|---:|---|
| 1 | 8.3 % | `core::ptr::write::<core::ptr::Unaligned<core::core_arch::x86::__m128i>>` | 8.3 % | `core::ptr::write::<core::ptr::Unaligned<core::core_arch::x86::__m128i>>` |
| 2 | 7.0 % | `bough::cpu_raster::x86::blend_span` | 7.3 % | `bough::cpu_raster::x86::blend_span` |
| 3 | 5.3 % | `<bough::cpu_raster::CpuRaster>::tile::<true>` | 5.3 % | `<bough::cpu_raster::CpuRaster>::tile::<true>` |
| 4 | 3.9 % | `core::core_arch::x86::f16c::_mm256_cvtph_ps` | 3.9 % | `core::core_arch::x86::f16c::_mm256_cvtph_ps` |
| 5 | 3.4 % | `bough::cpu_raster::x86::store_row` | 3.4 % | `bough::cpu_raster::x86::store_row` |
| 6 | 3.4 % | `clear_page_rep` | 3.3 % | `clear_page_rep` |
| 7 | 3.1 % | `core::core_arch::x86::fma::_mm256_fmadd_ps` | 3.1 % | `core::core_arch::x86::fma::_mm256_fmadd_ps` |
| 8 | 2.7 % | `__list_del_entry_valid_or_report` | 2.6 % | `__list_del_entry_valid_or_report` |
| 9 | 2.0 % | `unmap_page_range` | 2.0 % | `unmap_page_range` |
| 10 | 1.8 % | `__handle_mm_fault` | 1.8 % | `__handle_mm_fault` |

Process headless, 956,726,131 cycles sampled; 3.3 % of the recording in other processes (excluded). Dispatch-like frames (closures, call_once, vtable or shim frames, run_head): 1.4 % of self time.

### table fine append (flamegraph `profiles/svg/table-append.svg`)

| # | self time | symbol (top ten by self time) | charged to nearest Rust frame | symbol |
|---:|---:|---|---:|---|
| 1 | 13.4 % | `_int_malloc` | 5.1 % | `<core::fmt::rt::Argument>::fmt` |
| 2 | 4.1 % | `_int_free_create_chunk` | 4.7 % | `core::fmt::write` |
| 3 | 4.0 % | `core::fmt::write` | 3.8 % | `std::sys::alloc::unix::alloc` |
| 4 | 3.7 % | `_int_free_merge_chunk` | 3.7 % | `std::sys::alloc::unix::realloc` |
| 5 | 3.3 % | `<bough::cpu_raster::CpuRaster>::tile::<true>` | 3.6 % | `alloc::fmt::format::{closure#0}` |
| 6 | 3.2 % | `__memmove_avx_unaligned_erms` | 3.4 % | `<&mut alloc::string::String as core::fmt::Write::write_fmt::SpecWriteFmt>::spec_` |
| 7 | 3.1 % | `realloc` | 3.3 % | `_int_malloc` |
| 8 | 2.8 % | `<alloc::string::String as core::fmt::Write>::write_str` | 3.3 % | `<bough::cpu_raster::CpuRaster>::tile::<true>` |
| 9 | 2.8 % | `unlink_chunk.isra.0` | 3.2 % | `core::ptr::copy_nonoverlapping::<u8>` |
| 10 | 2.7 % | `core::ptr::copy_nonoverlapping::<u8>` | 2.8 % | `<alloc::raw_vec::RawVecInner>::grow_amortized` |

Process headless, 1,050,053,951 cycles sampled; 3.0 % of the recording in other processes (excluded). Dispatch-like frames (closures, call_once, vtable or shim frames, run_head): 2.5 % of self time.

### table fine clear (flamegraph `profiles/svg/table-clear.svg`)

| # | self time | symbol (top ten by self time) | charged to nearest Rust frame | symbol |
|---:|---:|---|---:|---|
| 1 | 33.1 % | `__memset_avx2_unaligned_erms` | 32.1 % | `__memset_avx2_unaligned_erms` |
| 2 | 7.5 % | `core::ptr::write::<core::ptr::Unaligned<core::core_arch::x86::__m128i>>` | 7.5 % | `core::ptr::write::<core::ptr::Unaligned<core::core_arch::x86::__m128i>>` |
| 3 | 4.7 % | `bough::cpu_raster::x86::blend_span` | 4.8 % | `bough::cpu_raster::x86::blend_span` |
| 4 | 4.1 % | `_int_malloc` | 3.9 % | `_int_malloc` |
| 5 | 3.7 % | `<bough::cpu_raster::CpuRaster>::tile::<true>` | 3.8 % | `<bough::cpu_raster::CpuRaster>::tile::<true>` |
| 6 | 2.6 % | `_int_free_create_chunk` | 2.6 % | `_int_free_create_chunk` |
| 7 | 2.5 % | `__memmove_avx_unaligned_erms` | 2.5 % | `core::core_arch::x86::f16c::_mm256_cvtph_ps` |
| 8 | 2.5 % | `core::core_arch::x86::f16c::_mm256_cvtph_ps` | 2.4 % | `__memmove_avx_unaligned_erms` |
| 9 | 1.9 % | `<core::core_arch::x86::__m128i>::as_i32x4` | 1.9 % | `<core::core_arch::x86::__m128i>::as_i32x4` |
| 10 | 1.7 % | `core::core_arch::x86::sse2::_mm_packs_epi32` | 1.7 % | `core::core_arch::x86::sse2::_mm_packs_epi32` |

Process headless, 3,441,547,881 cycles sampled; 1.0 % of the recording in other processes (excluded). Dispatch-like frames (closures, call_once, vtable or shim frames, run_head): 0.3 % of self time.

### sheet 10,000 rows hover (gui-bench cell) (flamegraph `profiles/svg/sheet10k-hover.svg`)

| # | self time | symbol (top ten by self time) | charged to nearest Rust frame | symbol |
|---:|---:|---|---:|---|
| 1 | 8.5 % | `<bough::seal::SealCheck as bough::seal::Sealer>::word` | 8.5 % | `<bough::seal::SealCheck as bough::seal::Sealer>::word` |
| 2 | 7.9 % | `__memmove_avx_unaligned_erms` | 7.9 % | `<bough::rt::Rt>::run_head` |
| 3 | 7.9 % | `<bough::rt::Rt>::run_head` | 7.0 % | `__memmove_avx_unaligned_erms` |
| 4 | 5.6 % | `<[u8]>::split_at_checked` | 5.6 % | `<[u8]>::split_at_checked` |
| 5 | 4.9 % | `<i32 as core::cmp::Ord>::clamp` | 4.9 % | `<i32 as core::cmp::Ord>::clamp` |
| 6 | 4.0 % | `<bough::layout::Spec>::same` | 4.0 % | `<bough::layout::Spec>::same` |
| 7 | 3.7 % | `<core::option::Option<usize>>::map::<bough::layout::LBox<bough::signal::WMap<u32, bough_workloads::s` | 3.7 % | `<core::option::Option<usize>>::map::<bough::layout::LBox<bough::signal::WMap<u32` |
| 8 | 2.8 % | `<bough::elem::ApplyBg as bough::elem::Apply>::apply` | 3.3 % | `bough::describe::update_dyn::<bough_workloads::sheet::Sheet, bough::layout::LBox` |
| 9 | 2.8 % | `bough_workloads::sheet::fill` | 3.0 % | `<bough::elem::ApplyBg as bough::elem::Apply>::apply` |
| 10 | 2.7 % | `bough::describe::update_dyn::<bough_workloads::sheet::Sheet, bough::layout::LBox<bough::signal::WMap` | 2.8 % | `bough_workloads::sheet::fill` |

Process bough-bench, 2,969,579,362 cycles sampled; 0.0 % of the recording in other processes (excluded). Dispatch-like frames (closures, call_once, vtable or shim frames, run_head): 28.0 % of self time.

### sheet 10,000 rows theme switch (gui-bench cell) (flamegraph `profiles/svg/sheet10k-theme.svg`)

| # | self time | symbol (top ten by self time) | charged to nearest Rust frame | symbol |
|---:|---:|---|---:|---|
| 1 | 8.8 % | `<bough::seal::SealCheck as bough::seal::Sealer>::word` | 8.8 % | `<bough::seal::SealCheck as bough::seal::Sealer>::word` |
| 2 | 6.2 % | `__memmove_avx_unaligned_erms` | 5.8 % | `<i32 as core::cmp::Ord>::clamp` |
| 3 | 5.8 % | `<i32 as core::cmp::Ord>::clamp` | 5.7 % | `__memmove_avx_unaligned_erms` |
| 4 | 5.0 % | `<bough::cpu_raster::CpuRaster>::tile::<true>` | 5.0 % | `<bough::cpu_raster::CpuRaster>::tile::<true>` |
| 5 | 4.8 % | `<bough_core::nrect::NRect>::is_empty` | 4.8 % | `<bough_core::nrect::NRect>::is_empty` |
| 6 | 4.3 % | `<[u8]>::split_at_checked` | 4.5 % | `bough::describe::update_dyn::<bough_workloads::sheet::Sheet, bough::layout::LBox` |
| 7 | 4.3 % | `bough::describe::update_dyn::<bough_workloads::sheet::Sheet, bough::layout::LBox<bough::signal::WMap` | 4.3 % | `<[u8]>::split_at_checked` |
| 8 | 4.1 % | `<bough::layout::LBox<bough::signal::WMap<u32, bough_workloads::sheet::Sheet, <bough_workloads::sheet` | 4.1 % | `<bough::layout::LBox<bough::signal::WMap<u32, bough_workloads::sheet::Sheet, <bo` |
| 9 | 3.9 % | `<bough_core::page::Page>::abs` | 3.9 % | `<bough_core::page::Page>::abs` |
| 10 | 3.3 % | `<usize as core::slice::index::SliceIndex<[bough::layout::Spec]>>::get` | 3.3 % | `<usize as core::slice::index::SliceIndex<[bough::layout::Spec]>>::get` |

Process bough-bench, 1,649,539,102 cycles sampled; 0.0 % of the recording in other processes (excluded). Dispatch-like frames (closures, call_once, vtable or shim frames, run_head): 17.5 % of self time.

### canvas 5,000 nodes hover (gui-bench cell) (flamegraph `profiles/svg/canvas5k-hover.svg`)

| # | self time | symbol (top ten by self time) | charged to nearest Rust frame | symbol |
|---:|---:|---|---:|---|
| 1 | 20.1 % | `bough::cpu_raster::erf2::{closure#0}` | 20.2 % | `bough::cpu_raster::erf2::{closure#0}` |
| 2 | 19.9 % | `bough::cpu_raster::shadow` | 20.0 % | `bough::cpu_raster::shadow` |
| 3 | 14.2 % | `__expf_fma` | 9.0 % | `<bough::cpu_raster::CpuRaster>::repair` |
| 4 | 7.5 % | `bough::cpu_raster::shadow_x` | 7.5 % | `bough::cpu_raster::shadow_x` |
| 5 | 6.0 % | `bough::cpu_raster::shade_x` | 6.4 % | `<f32>::exp` |
| 6 | 4.6 % | `<f32>::exp` | 6.0 % | `bough::cpu_raster::shade_x` |
| 7 | 3.2 % | `<f32>::abs` | 4.1 % | `__expf_fma` |
| 8 | 2.3 % | `bough::cpu_raster::blend_px` | 3.2 % | `<f32>::abs` |
| 9 | 1.8 % | `<bough::cpu_raster::CpuRaster>::tile::<true>` | 2.3 % | `bough::cpu_raster::blend_px` |
| 10 | 1.8 % | `<f32>::clamp` | 1.8 % | `<bough::cpu_raster::CpuRaster>::tile::<true>` |

Process bough-bench, 95,865,579,139 cycles sampled; 0.0 % of the recording in other processes (excluded). Dispatch-like frames (closures, call_once, vtable or shim frames, run_head): 22.9 % of self time.


Files: flamegraphs `profiles/svg/*.svg` (rows3k-hover, table-select, table-update10, table-swap, table-remove, table-insert, table-replace, table-append, table-clear, sheet10k-hover, sheet10k-theme, canvas5k-hover), collapsed stacks `profiles/collapsed/`, stat tables `profiles/stat/`, per-cell sums `profiles/stat-*.json`, `profiles/params-*.json`, top-ten JSON `profiles/top/`.

## 9. Failures (reported as failures; nothing was rerun to make a cell pass)

* **floem**: fails the harness's layout check in 16 of 17 cells in cmp-2, cmp-u1 and cmp-v1 (a 1 px border colour at (640,31) or (640,71): `#f2f3f5` for the spec's `#e5e7eb`); only the spinner without rows runs. No timing exists for the failed cells; the tables show estimates.
* **vizia typing, unthrottled (cmp-u1)**: layout check failed (black at all four check points: nothing painted). It passed in cmp-2 and cmp-v1.
* **bough, sheet 10,000 rows (heavy-1)**: the scroll probe failed ("no matching window") and the resize probe timed out. The app log ends with the compositor closing the connection: `results/heavy-1/failed-cell/sheet-10000-bough-app.log`:
  ```
  bough-bench: Sheet { rows: 10000 }
  Io error: Connection reset by peer (os error 104)
  bough: the compositor connection closed: Io error: Connection reset by peer (os error 104)
  ```
  The startup, idle, hover, theme and edit probes of the cell completed (`cell.json` in the same directory). Not investigated (instruction from the lead).
* **bough, sheet 10,000 rows, heavy-u1**: resize probe failed ("no matching window"); **heavy-v1**: scroll ("no matching window") and resize (timed out), as in heavy-1.
* `wake_lateness` failures: cmp-2 130 of 587 reports, heavy-1 49 of 86, sweeps 11 / 18 / 21 of 48, cmp-u1 3 of 585, heavy-u1 2 of 87, cmp-v1 195 of 587, heavy-v1 47 of 86. They are reports the harness marked invalid; they are not framework failure rates (different denominators from the failed cells above).
* The first cell of each binary in an invocation (sheet 1,000) shows cold-start startup (bough 2.6 s, iced 17 s, gpui-kit 19 s in heavy-1); flagged on the dashboard, startup is unchecked in any case.
* cmp-3 not run (host never quiet). The indirect-branch counters did not run on the attached cells (no counts in `perf stat -p` for that group). A first attempt at the indirect group used event names that do not exist on Zen 2 and was redone with `bp_dyn_ind_pred`.

## 10. Caveats

* Software rendering for everyone; bough's CPU executor on one thread against many-thread lavapipe builds (thread counts in the manifest); the compositor is pixman; no GPU. Per-process CPU per frame includes every worker thread of the process.
* bough has no runtime scale change; the scale probe is unsupported and the resize probe covers size only.
* Cells are comparable only within a column; egui has one default variant for table and ticker (the table ops are not two implementations for it; marked ‡ in the dashboard value tables); freya and dioxus run their own idioms; shared-state variants exist for seven builds.
* Memory is process RSS at the end of the probe (includes shared pages, libraries and, for the lavapipe builds, the driver), not steady state after scrolling a whole dataset. Executable sizes are for symbol-bearing benchmark apps built with different profiles; the 55.5 MB bough app is not a binary-size result and must not be compared with the e2e lane's 1.07 MB app.
* The latency split is not an exact decomposition (section 4); an unthrottled run still contains protocol and buffer scheduling and is neither pure app cost nor input-to-photon. fakeland has no display.
* Quick mode: smallest sizes, fewer repeats, two iterations; the heavy runs use default effort. The heavy scenes are a sheet and a canvas in four frameworks, not all five standing scenes; Latin text only (no fallback shaping, bidi, IME, image churn, nested scrollers, multiple windows).
* Instruction and miss counts come from a PMU shared with other sessions (75 % duty cycle) on SMT siblings of busy cores; the headless paths end at the CPU executor without the window host.

## 11. Independent review (Batch 11) and what was done

| finding | what was done |
|---|---|
| B01 headline is the round-7 binary | stated at the top of this file and of the page; manifest per executable (hash, size, source, threads); `bough-x` reserved as a separate entry |
| B02 renderers and thread counts differ | stated at the top; threads per framework in the manifest (bough 1, others 21 to 53); no GPU or equal-CPU claim |
| B03 affinity is not isolation | stated; SMT siblings and other tenants named; cmp-2 unpinned; no randomised blocks (not done) |
| B04, B05 estimates | labelled heuristic everywhere; kept out of all rankings and counts; limits stated; hold-out tests run and reported (coverage about 3 of 4, long tails); values rounded to two significant figures; reason (failed cell, never ported) in every tooltip |
| B06 fail-open validity | missing validity is now "unchecked", never valid (`collect.py`); startup marked unchecked and unranked; cmp-2 startup marked load-contaminated; the variant actually run is shown (egui ‡) |
| B07 phase medians | stated beside every split; unthrottled not pure app cost |
| B09 workflow success | scripts propagate the runner's exit code and write `STATUS.json`; every run directory re-checked (`RUN-STATUS.tsv`); effective source hashed (`EFFECTIVE-SOURCE.txt`, `patches/gb-timing.patch`) |
| B12 binary and memory scope | scope stated (symbols, profile, RSS definition); no size ratio is reported |
| B14 score bound to semantics | rankable only with a valid report, a passed layout check and a recorded executable hash; the lack of an independent pixel check, trace replay and failure mutants is stated; not built |
| B08 counter scopes | stated in section 8 (user mode, process scope, group duty cycle, slope method; not randomised) |

## 12. Files

`RESULTS.md` (this), `results_tables.md` (every table), `dashboard/` (source: `build.py`, `r8.py`, `data.r8.json`, `data.json` from r7/bench; built page `artifact.html`), `tools/` (`collect.py`, `estimate.py`, `headline.py`, `gb.py`, `flatten`-side `startup.py`, `run_status.py`, `results_md.py`, `results_doc.py`, `prof_collect.py`), `scripts/` (every script run on ilo-prod or the Mac), `results/<run>/` (identity, load log, runner log, `flat/*.tsv`, `run/run.json`; the 50 MB of per-cell reports stay on ilo-prod in `/root/work/r8/bench/results/<run>/run`), `results/profiles/`, `results/RUN-STATUS.tsv`, `results/EFFECTIVE-SOURCE.txt`, `patches/gb-timing.patch`. Every file brought back from ilo-prod was verified by sha256 against the remote (`scripts/fetch.sh`).
