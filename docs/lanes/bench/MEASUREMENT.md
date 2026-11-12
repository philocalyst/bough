# Measurement in bough: what each tool is and is not good for

Bench lane, round 7 (2026-11-12). Every claim here was checked on the two hosts we use: the Mac (Apple M3 Pro,
11 cores, macOS 24.6, shared by every lane, load 15-50 during this work) and ilo-prod (AMD Ryzen 7 3700X, Zen 2,
16 threads, Linux 6.18.54, NixOS, shared and busy: load 13-30). Commit and numbers in brackets.

## Rules that apply to every tool

* **Every result file starts with an identity record**: host, OS, CPU, frequency policy, load, commit (+dirty),
  toolchain, and the binary's profile, hash and size. `bench/perf/identity.sh [BINARY]` prints it; `pstat.sh` and
  `bench/scenes/run.sh` include it. A number without one is an anecdote.
* **Release numbers come from the `dist` profile** (fat LTO, one codegen unit, as zgx's `dist`). `release` moves
  untouched cells by up to 4 % (composition lane, round 6).
* **Instrumented builds are never release results.** A binary built with `hotpath`, `hotpath-alloc`, or run as
  `workload_alloc` (counting allocator) measures something else; its numbers go in a separate file that says so.
* **Simulated cache counters and profiler overhead are never wins.**
* **A number from a busy host says so.** Instruction counts barely move with load; wall time, cycles and cache
  misses do. On the Mac only instructions are trustworthy under load; on ilo-prod pin to CCX0 (CPUs 0-3, 8-11)
  away from the build cores (4-7, 12-15) and still expect cycles to drift a few percent between runs.
* **Instructions on aarch64 and on x86-64 never share an axis.** Same source, different ISA, different SIMD paths
  (bough-core has NEON paths and scalar fallbacks): `set_ink` at 1e5 records is 588 instructions on the M3 and
  905 on the 3700X [eab3927e34].

## The tools

| tool | good for | not good for |
|---|---|---|
| In-process thread counters (`bough_mem::pmu`; `bough_metal::perf::tinstr`; slopes as in `signal_cost`) | Instructions (both hosts) and cache/TLB events (Linux) per operation, bracketed precisely around the work, many cells per run | Work on other threads (GPU completion, workers); comparing across binaries whose harness code differs (a harness edit moved `pmu_cells`' per-op counts by 2-5 %); cells whose state drifts between batches (see "signal_cost", below) |
| `perf stat` on ilo-prod (`bench/perf/pstat.sh`) | Whole-process or slope-per-repetition counts of named Zen 2 events (L1d, L2, refills from L3/DRAM, dTLB, branch misses) with variance; a cross-check of the in-process counters | Attribution to functions (use `perf record`); anything macOS-only (the façade, Metal); more than three events in one group on this host |
| `hotpath` 0.30.3 (feature `hotpath` / `hotpath-alloc` of `bough`) | Which phase of a turn takes the time on a slow scene (≥ 10 µs per call); exclusive allocation bytes per function; quick "where do the allocations come from" | Any cell under ~10 µs (each measured call costs ~310 instructions on the calling thread); any release number; binaries that declare their own `#[global_allocator]` unless it is passed to `hotpath::main(allocator = ..)` |
| `hyperfine` (Mac: 2.0.0 on PATH; ilo-prod: `/root/work/r7/bench/gcroots/tools/bin`) | Process-level wall time distributions: cold start (exec to first frame), whole-run time; peak RSS per run (its `Memory` row) | Anything under a few ms; any within-process cell; precise results on the loaded Mac (σ was 30 % of the mean for a 126 ms cold start) |
| `/usr/bin/time -l` (macOS), GNU `time -v` (ilo-prod: `/root/work/r7/bench/gcroots/tools-1/bin/time`, not the shell keyword) | Peak RSS, page faults, context switches of one process; on macOS also whole-process instructions retired and peak memory footprint | Per-phase memory (use the harness's `footprint_mib` / `peak_mib` columns or the memory lane's ledger) |
| Allocation counts: `workload_alloc` (bough-workloads) or `hotpath-alloc` | Allocations per step (a frame should allocate nothing), allocation bytes by function | Instruction or time numbers (both add atomics per allocation) |
| The standing workloads (`bough-workloads`, `bench/scenes/run.sh`) | The same five application scenes, the same phases, per-step instructions, wall time and memory | Micro-cells (use `signal_cost`, `pmu_cells`, the core benches) |

## Recipes

Environment as ROUND7.md (`PATH=$HOME/.local/share/cargo/bin:$PATH RUSTC_WRAPPER= CARGO_TARGET_DIR=<worktree>/target`).

**Instructions per operation, Mac.** Build at `dist` and run the cell's binary; read `tinstr` / `pmu::sample`
deltas. Example: `cargo build --profile dist -p bough-core --example pmu_cells && target/dist/examples/pmu_cells`.
`signal_cost` with `SC_STABLE=1` (below).

**Cache events per operation, ilo-prod.** Get the source there (below), then in `/root/work/r7/<lane>`:
```
source env.sh                                       # own RUSTUP_HOME/CARGO_HOME/CARGO_TARGET_DIR, perf, hyperfine
build cargo build --profile dist -p bough-core --example pmu_cells        # build = cores 4-7,12-15 at nice 10
BOUGH_PMU=l1d-misses,cache-misses,dc-fill-dram taskset -c 2 $CARGO_TARGET_DIR/dist/examples/pmu_cells
OP=set_ink bench/perf/pstat.sh -g core,l1,l2,fill,tlb -s REPS=2000,22000 -o out.tsv -- $CARGO_TARGET_DIR/dist/examples/pmu_cells 100000
```
In-process names (`bough_mem::pmu::Ev::parse`): `instructions`, `cycles`, `branch-misses`, `l1d-loads`,
`l1d-misses`, `cache-refs`, `cache-misses`, `dtlb-misses`, `dc-fill-l2`, `dc-fill-ccx`, `dc-fill-dram`,
`dc-fill-remote`, `raw:0x<config>`; at most three beyond instructions and cycles. Any bough-core example that uses
`bough_damage_lab::measure` now counts the calling thread on Linux (user mode; `BOUGH_PMU_KERNEL=1` adds the
kernel), so `trace_bench`, `mark_probe`, `batch_probe`, `cull_stats` and `r6_check` run there unchanged, and
`core_bench` (examples/bench.rs with `cost_r6`) counts the calling thread on Linux (first full run:
`bench/results/r7/core_bench.ilo-prod.txt`). bough-text's `pipeline`, `parley_bench` and `parley_probe` run on
Linux too (`gpu_shape` says it needs Metal; `bench` calls the NEON-only `shape_simd`, so it is aarch64 only). The
text examples need the font and fontconfig paths the round-6 environment set (`BOUGH_DEJAVU_DIR`, `BOUGH_WORDS`,
`PKG_CONFIG_PATH`): `/root/work/r7/bench/env.sh` sets them.
Cross-check [eab3927e34, ilo-prod, load ~20]: `set_ink` at 1e5 records, perf stat slope vs in-process: instructions
905.3 vs 908.6, cycles 881 vs 878, L1d misses 11.7 vs 10.5-12.7, L2 misses 5.7 vs 6.8-7.4, DRAM refills 0.1 vs
0.04; run% 100; relative stddev of instructions 0.00 %. Result: `bench/results/r7/pstat-set_ink-1e5.ilo-prod.tsv`.

**Per-function profile (instrumented).** `cargo build --profile dist -p bough --example signal_cost --features hotpath`
(or `hotpath-alloc`); the report prints at exit. For the scenes: `-p bough-workloads --features hotpath`. Add a
measure site with `#[cfg_attr(feature = "hotpath", hotpath::measure)]` on a function of `bough`; sites today:
`App::new`, `turn`, `effects`, `redescribe`, `frame_as`, `draw`, `Rt::settle`, `lay_flush`, `pointer_move`, `resize`,
`key_down`, `wheel`.

**Cold start and peak RSS.** `PHASES=cold REPS=1 SCENE_SCALE=1 hyperfine -N --warmup 2 --runs 10 'target/dist/workload sheet'`;
`/usr/bin/time -l target/dist/workload sheet` (Mac) or `$B/gcroots/tools-1/bin/time -v ...` (ilo-prod). The
harness's own `cold` rows split the same start into model build, mount and first frame, and print `since_exec_ms`.

**The scenes.** `cargo build --profile dist -p bough-workloads --bin workload --bin workload_alloc`, then
`bench/scenes/run.sh out.tsv sheet` (environment: `SCENE_SCALE`, `REPS`, `MODE=once|redescribe`, `RENDER=0|1`,
`SCALE`, `PHASES`, `BOUGH_PMU`). See `SCENES.md`.

**Getting source to ilo-prod.** `git bundle create x.bundle <branch> --not <commit the host has>` (main
`bd9978cba8` is in `/root/work/bough-r6/bough`), `bench/remote/xfer.sh x.bundle /root/work/r7/<lane>/xfer/x.bundle`
(base64 in 63,000-byte chunks: rex refuses a control message over 64 KiB; ~16 KB/s; sha256 checked), then
`git fetch ../xfer/x.bundle <branch>:<branch>` in a clone of that checkout. Between commits, `bench/remote/push.sh
<remote checkout> FILE...` writes up to ~40 KiB of working-tree files. Long jobs: `bench/remote/bg.sh LOG SCRIPT`
(its own rex block; a `nohup`/`setsid` job started from an `rx.sh` script died with the block). `rx.sh` closes
its block when done: `rex kill` takes sessions, not blocks, so the old helper's `rex kill -b` left every block
open until the session hit rex's limit of 64 windows. Toolchain: hard-link copies (`cp -al`) of
`/root/work/bough-r6/{rustup,cargo/registry}`; a plain copy crawled at ~1 MB/s on that disk (ext4 journal waits).

## Host facts

**ilo-prod PMU (Zen 2).** PMUs exposed: `cpu`, `ibs_op`, `ibs_fetch` (no `amd_l3`, no `amd_df`: no uncore L3 or
memory-controller counters). Six core counters per thread; the NMI watchdog holds one (`kernel.nmi_watchdog=1`;
we do not change host settings). Generic events and what they are here (`perf stat -vv`):
`cache-references` = `0xff60` (all L2 requests), `cache-misses` = `0x0964` (L2 misses of data and instruction
fetches), `L1-dcache-loads` = `ls_dc_accesses`, `L1-dcache-load-misses` = L2 requests from the data cache (stores'
misses included). `LLC-loads`, `LLC-load-misses`, `stalled-cycles-backend`: not supported. Where an L1 miss was
served from: `ls_refills_from_sys.ls_mabresp_{lcl_l2,lcl_cache,lcl_dram,rmt_cache}` (raw `0x43` with unit masks
1, 2, 8, 16; `lcl_cache` = L3 or another core's L2 in the CCX). Multiplexing: a `perf stat` group of four or more
events was on the PMU 49-52 % of the time, three events 100 %, pinned to CPU 2 and unpinned alike, with no other
process holding perf events; cause not found (an in-kernel user of the counters is the likeliest). An in-process group of five (`bough_mem::pmu`) read complete in every batch. So
`pstat.sh` groups are three events, and `run%` must read 100. Frequency: governor `powersave` with boost on:
cycles vary run to run (in-process `set_ink` at 1e5: 593-878 cycles in three runs); instructions do not.

**Mac.** `thread_selfcounts` gives instructions and cycles of the calling thread (~1.4 K instructions per call;
the harness subtracts the median of 64 empty brackets). Cache events need kperf (root and an entitlement): not
available. Under load, wall time spread is large: a 126 ms cold start had σ 38 ms over 10 runs.

## What this round found with these tools

* **`signal_cost` mismeasured its number and string cells.** It warms with 8 turns and takes one slope between
  64 and 192 turns writing `k % 1000`. The first write to each row's label then lands inside the measured
  batches: at 64 rows `hi < lo` and the unsigned division printed 144115188075854459 (that is -1,412 and -4,236);
  at 256 rows the first-touch work is the slope. And a label's digit count changes at different rates in the two
  batches. `SC_STABLE=1` (warm `3n + 8` turns, values `1000 + k % 9000`, median of five slopes): number write
  5,990-6,090 at 1, 16, 64 and 256 rows (flat in rows: the "grows with rows" reading came from the method);
  string write 6,500-6,880 at 1-64 rows and 9,960-10,130 at 256 (grows; not explained yet); colour and idle
  unchanged (512-557, 199) [M3, dist, 3 runs, 2169ab5e34].
* **hotpath overhead** [M3, dist, 5 runs each, medians, 2931c0fe50]: +1,513-1,812 instructions per turn with the
  five measure sites a turn passes (`turn`, `settle`, `effects`, `redescribe`, `lay_flush`), i.e. ~310 per measured
  call; with `hotpath-alloc` +1,921-2,783 per turn mount-once and +6,339-7,654 at 256 rows re-describing (that
  mode allocates every turn: `lay_flush` 240 B, `settle` 70 B, `redescribe` 43 B on average per call). An idle
  turn reads 199 without and 1,756 with: 8.8x. Unusable for cells of that size; fine for re-describing turns of
  150 K (1 %) and for scene phases. hotpath's background thread does not show in thread counters but does in
  process counters (`proc_pid_rusage`, `/usr/bin/time`).
* **hotpath and our allocators**: compatible when nested. `hotpath::main(allocator = bough_mem::count::Counting)`
  wraps `Counting` in hotpath's counting allocator; both saw every allocation (4,267 allocations, 4,033 frees,
  peak 4.0 MB under hotpath at 16 rows). A binary that declares its own `#[global_allocator]` (`workload_alloc`,
  `widget_bench`, `editor_streaming`, awagy's `ledger`) cannot also enable `hotpath-alloc` without moving its
  allocator into that argument: two global allocators do not compile.
