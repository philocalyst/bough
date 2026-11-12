# bench lane, round 7: journal

Branch `r7/bench` in `wt/r7-bench`, from `round6` `3b0b00c698`. Brief: `bough/runtime/lead/ROUND7.md`, section
"bench", plus Amendment 1. Entry points: `MEASUREMENT.md` (what each tool is and is not good for), this file
(what was done, in order, with numbers and what is next).

## Remote (ilo-prod)

* Work dir `/root/work/r7/bench` (moved there from `/root/work/bough-r7-bench` after Amendment 1): `bough/` a
  clone of `/root/work/bough-r6/bough` with `r7/bench` fetched from a bundle, `env.sh` (own `RUSTUP_HOME`,
  `CARGO_HOME`, `CARGO_TARGET_DIR`; copies of the round-6 toolchain and registry; `build` = cores 4-7+12-15 at
  nice 10, `pin` = core 2), `xfer/` bundles, `logs/`.
* Source goes there with `bench/remote/xfer.sh` (base64 chunks of 63,000 bytes through `rex run --wait`; rex
  refuses a control message over 64 KiB; ~16 KB/s) and is verified by sha256. Bundles are incremental against
  `main` `bd9978cba8`, which the host has.

## Log

### 2026-11-12

* ilo-prod probed: kernel 6.18.54, `perf_event_paranoid` 2, PMUs `cpu ibs_op ibs_fetch` (no `amd_l3`, no
  `amd_df`), NMI watchdog on (one of the six core counters taken), governor powersave with boost, load 25-30 on
  16 threads (gui-bench runs and rustc from other lanes). perf 7.3.1 from the nix store.
* Zen 2 event facts (verified with `perf stat -vv`): `cache-references` = `0xff60` (all L2 requests),
  `cache-misses` = `0x0964` (L2 misses), so on this host "cache misses" means L2 misses. `LLC-loads`,
  `LLC-load-misses` and `stalled-cycles-backend` are not supported. 17 events in one run multiplexed to 33-44 %
  each.
* hotpath 0.30.3 added to `bough` as an optional dependency (features `hotpath`, `hotpath-alloc`); measure sites
  on the turn's phases; `signal_cost` opts in with `hotpath::main(allocator = bough_mem::count::Counting)`.
* `bough_mem::pmu`: the calling thread's counters on both hosts (Linux perf_event_open group, macOS
  thread_selfcounts); `bough_damage_lab::measure` uses it on Linux, so bough-core's examples build there.
* Finding: `signal_cost` at 64 rows prints 144115188075854459 for the mount-once number and string cells
  (deterministic). That is `(hi - lo) / 128` wrapping. Cause: the first write to each row's label lands in the
  measured batches (and value-dependent digit counts). `SC_STABLE=1` fixes the method; numbers in
  MEASUREMENT.md. Told the lead.
* hotpath overhead measured on signal_cost (5 runs, medians): ~310 instructions per measured call, +1,550 per turn;
  +2,000-7,600 with alloc tracking. Nested allocators verified. Details in MEASUREMENT.md.
* perf recipe works (pstat.sh, three-event groups after finding 4+ multiplex to ~50 %); in-process and perf stat
  agree on set_ink at 1e5 within 1 %. Told the lead (with the transfer recipe and the rxf.sh window leak).
* bough-core examples run on x86-64 (`trace_bench` checked). Cache matrix `bench/results/r7/pmu_cells-matrix.ilo-prod.tsv`:
  at 1e6 records `set_ink` is 969 instructions, ~2,100 cycles, 18 L1d misses, 17 L2 misses, 3.4 lines from DRAM;
  at 1e5: 909 instructions, 8.6 L1d, 3.9 L2, 0.04 DRAM. `translate_all` at 1e6: 2.2 M instructions, 22.9 K L1d misses.
* Scenes: crate `comparisons/bough-workloads` (harness + sheet by me; ide, dash, canvas, chat by four Sonnet
  agents from `SCENE-BRIEF.md` in the scratchpad, each scene behind its own cargo feature so they build apart).
  Sheet at full size (100,000 cells) mounts with RENDER=1: 3.51 G instructions, 480 ms, 240,055 records,
  footprint 365 MiB; theme switch 57 M instructions / 8.7 ms; hover 15 M / 9.9 ms; resize 127 M / 52 ms per step
  [M3, busy, 1eca7b5dc9 tree]. RENDER=0 needed a larger glyph capacity (fixed: ~21 glyph slots per numeric cell).
* hotpath on the sheet (scale 0.2): resize 37 %, lay_flush 29 %, frame 26 %, mount 17 % of the run.
* Dashboard (merged `lane/research/round6` for its files): zortax quick-1 section, labelled rough baseline;
  workloads section waits for the baselines.
* Scenes landed (all five, each reviewed; agents' reports summarised in SCENES.md and each module doc):
  ide (30,166 records), dash (8,188), canvas (10,856), chat (23,593), sheet (240,055). All mount at scale 1. The
  pixel gate `tests/pixels.rs` (retained == full redraw through every phase; rings 1-4, windowed, re-describing)
  passes for all five at scale 0.05.
* Defects found by the scenes: nested `clip_by` stale after a write (repro in tests/defects.rs, fails: retained !=
  fresh after the third pan); a list row's transition outliving the row (panic in resize, chat agent's report;
  repro written, run pending); `clip_by` inside `anchor` mounts without the anchor's origin (canvas agent, not
  reproduced); scroll offset not re-clamped when content height shrinks (chat; behaviour, blank view); image atlas
  one 1024x1024 texture, panics at the 9th distinct ~200x130 pt image (chat; media lane input).
* hotpath per scene (instrumented, scale 1, RENDER=1, REPS=16; share of the run's wall): ide lay_flush 52 %,
  resize 38 %; dash frame 22 %, resize 20 %; canvas frame 10 %, resize 10 % (the rest is GPU waits); sheet
  lay_flush 67 %, resize 66 %. Layout dominates the text-heavy scenes, the GPU the canvas.
* Running `workload all` in one process grows the footprint scene after scene (124 -> 386 MiB): an `App` drop
  does not return its memory (shutdown is the memory lane's item). The baseline runs each scene in its own process.
* Baseline (Mac, `bench/results/r7/workloads/mac-6deab7e4e7/`, M3 Pro under load 17-27, dist, REPS=32, scale 1,
  MODE=once, RENDER=1; instructions p50 on the calling thread / wall p50):

  | scene | mount | first frame | steady | hover | theme | resize | resize+anim | anim | idle |
  |---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
  | sheet | 3.67 G / 389 ms | 10.7 M / 10.7 ms | 27 K / 0.33 ms | 15.7 M / 8.2 ms | 57.5 M / 6.0 ms | 126 M / 30 ms (p95 2.1 G) | 79 M / 27 ms | 277 | 534 |
  | ide | 410 M / 83 ms | 6.6 M / 9.5 ms | 405 K / 0.61 ms | 804 K / 0.94 ms | 6.6 M / 3.5 ms | 63 M / 22 ms | 105 M / 24 ms | 42 M / 6.3 ms | 535 |
  | dash | 133 M / 63 ms | 5.6 M / 13 ms | 1.56 M / 1.4 ms | 734 K / 0.99 ms | 1.32 M / 2.7 ms | 15 M / 7.1 ms | 16 M / 7.5 ms | 876 K / 2.1 ms | 543 |
  | canvas | 127 M / 63 ms | 9.4 M / 48 ms | 46 K / 0.85 ms | 226 K / 7.7 ms | 579 K / 12.5 ms | 8.9 M / 16 ms | 8.8 M / 16 ms | 109 K / 3.8 ms | 541 |
  | chat | 964 M / 140 ms | 11.7 M / 12 ms | 780 K / 1.6 ms | 145 K / 0.28 ms | 3.1 M / 2.9 ms | 187 M / 27 ms | 170 M / 24 ms | 787 K / 1.5 ms | 538 |

  Cold start (hyperfine, 10 runs, exec to first frame; mean +- sd, peak RSS): sheet 666 +- 200 ms, 341 MiB; ide
  117 +- 13 ms, 75 MiB; dash 94 +- 15 ms, 31 MiB; canvas 128 +- 9 ms, 38 MiB; chat 218 +- 18 ms, 88 MiB.
  Allocations per step (`workload_alloc`): 0 on hover, theme and idle in every scene; resize 37 K (sheet), 33 K
  (chat), 1.3 K (ide), 1.2 K (dash), 86 (canvas); dash steady 213; mount 1.04 M (sheet), 140 K (chat), 130 K (ide).
  CPU share (RENDER=0): canvas resize 3.1 M / 0.20 ms against 8.9 M / 16 ms with the GPU; dash hover 41 us against
  0.99 ms: the GPU path dominates wall time in the graphics scenes, layout in the text scenes.
  Re-describing mode: idle 8-12 K instructions (106 K in the canvas) against ~540 once; other phases within a few %.
* Dashboard version 4 published (same URL as round 6's page): workloads first, signal_cost's two mismeasured cells
  annotated with stable-method series beside them, zortax quick run 1 as a labelled rough baseline.
* The sheet's selection outline now glides (translate transition; the amendment wants transitions in every heavy
  scene). Its baseline needs a rerun on that commit (the first rerun read a binary another build had replaced:
  integration tests rebuild the package's binaries, so a `--no-default-features` test run leaves a scene-less
  `target/dist/workload`; copy binaries out before measuring, as the agents did).
