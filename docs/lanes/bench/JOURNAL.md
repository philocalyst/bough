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
  (deterministic). That is `(hi - lo) / 128` wrapping: 64 repetitions cost ~181 K instructions more than 192.
  Cause under investigation (first write to each row's label paid inside the 64-rep batch).
