# Measuring a hot path: cache lines, misses, branches, flamegraphs (round 8, for every lane)

Threads lane, 2026-11-12. One script, `bench/r8/perf/hot.sh`, run unchanged on any binary that follows a small
convention. It builds on the round-7 bench lane's `pstat.sh` and `identity.sh` (copied here) and its
`bough_mem::pmu`. What was run where is in the last section; what has not run is said there too.

## What it answers

For each hot path (a *cell*): how many instructions, cycles, branch misses, L1 misses, L2 misses, and lines
from L3 and from DRAM one repetition costs, **warm** and **with each cache level cold**.

Why cold: an input arrives after other code has run (the application, the compositor, other processes), so
the lines an interaction touches are not all in L1 when it starts. The warm numbers say what the steady state
costs; the cold ones say what an interaction costs, and they differ by 4x to 10x for our turns (a hover on the
sheet: 3,300 cycles warm, 13,000 with L3 cold, on the quiet host).

How a level is made cold: between repetitions the harness walks a buffer twice the size of that level
(`EVICT_KB`), outside the counted bracket. Then:

* **L1 misses at level `l1`** count the distinct data lines the path touches (plus what the hardware prefetcher
  brings).
* **`fill_dram` at level `l3`** counts the lines that had to come from memory.
* **`chain_lb`** = (cycles with L3 cold - cycles warm) / `FILL_CYCLES` (300): if every miss were serial it
  would equal the DRAM fills; the ratio of the two says how much the path's misses overlap. It is a bound,
  not a count: the count of *dependent* lines is read in the code (see the journal, section 5).
* The eviction walk also evicts the path's **code** from L2 and L3 (not from L1i). That is part of what an
  interaction pays and it is in the cold numbers; it means a structure change that removes data lines can
  disappear in them (it did: journal, C4).

Every other cell has the `floor` cell (an empty repetition) subtracted at the same level.

## The convention a binary follows

As `crates/bough-x/examples/hot_x.rs` does (copy its `main`):

| environment | meaning |
|---|---|
| `CELLS=1` | print `name<TAB>what` per cell and exit; one cell is `floor` |
| `OP=<cell>` | run that cell only |
| `REPS=<n>` | repetitions after set-up (the perf stat slope uses two values) |
| `EVICT_KB=<k>` | walk a `k` KiB buffer between repetitions |
| `BOUGH_PMU=a,b,c` | up to three more in-process events (`bough_mem::pmu::Ev::parse`) |

and prints one TSV row per cell: `scene cell evict_kb reps <event>_p50.. <event>_max.. tiles_p50 wall_ns_p50
wall_ns_max` (anything after the event columns is carried through). A repetition must leave the state such
that the next does the same work; the harness runs 16 unmeasured repetitions first.

## Running it

```
bench/r8/perf/hot.sh [-o OUT] [-r REPS] [-c CPU] [-x] [-f] [-R ROWS] BIN [CELL...]
```

* default: the in-process table, `OUT/hot.tsv` (and `OUT/raw.tsv`, the unfolded rows);
* `-x`: also `perf stat` slopes per cell with `floor` subtracted (`OUT/pstat.tsv`), the cross-check;
* `-f`: also a flamegraph per cell (`OUT/<cell>.svg`, `OUT/<cell>.folded`): `perf record -e cycles:u -F 4999
  --call-graph dwarf,8192`, folded and drawn by inferno.

The first lines of every output are the identity record (host, load, commit, binary hash). Build at `dist`.

### mothership (preferred for counters and micro-timing: quiet, Intel, last-level-cache events)

Xeon E-2236 (Coffee Lake, 6 cores / 12 threads, L1d 32 KiB, L2 256 KiB, L3 12 MiB), load about 1, user `miles`
(not root), `perf_event_paranoid` 2: user-mode counting of one's own process works, kernel-mode does not. No
checkout, no `cc`, no `python3` on `PATH`; nothing on the host is changed. What exists and is used:

* toolchain: `~/.rustup/toolchains/nightly-2026-10-17-x86_64-unknown-linux-gnu/bin`;
* linker: `/nix/store/*-clang-wrapper-21.1.8/bin/cc` (`CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER`);
* perf 7.2.10: `/nix/store/r9yzbjkiz71ig7g26jdd9cc4pp8ymdph-perf-linux-7.2.10/bin`;
* a python3 that runs: `/nix/store/9r02ykx9y35lf4gk6gc30gnag1y8ncp7-python3-3.13.15/bin` (another python3 in
  the store is a foreign-architecture build).

`/home/miles/work/r8/threads/env.sh` sets all of it; copy it to your lane's directory and change `L`.

Source goes over as a small workspace of only the crates needed (they must depend on nothing else):

```
bench/r8/remote/mini.sh /tmp/mini.tgz bough-x bough-expr bough-mem          # 85 KB
RX_CWD=/tmp REX_HOST=mothership REX_SESSION_ID=bough-probe wt/r7-bench/bench/remote/xfer.sh /tmp/mini.tgz /home/miles/work/r8/<lane>/xfer/mini.tgz
# then, in a script run by <scratchpad>/mrx.sh:
cd /home/miles/work/r8/<lane>; source env.sh; rm -rf mini; tar -xzf xfer/mini.tgz; cd mini; rm -f rust-toolchain.toml
cargo build --profile dist -p bough-x --example hot_x
bash bench/r8/perf/hot.sh -c 3 -o ../out/hot $CARGO_TARGET_DIR/dist/examples/hot_x
```

`RX_CWD=/tmp` matters: the helpers default to `/root`, which `miles` cannot enter. Results come back through
the terminal capture, which turns tabs into spaces: print them with `tr '\t' ','`. The build takes five
seconds. Keep the lane under 4 GB and delete `target` when done.

Intel event groups (three events each, chosen by the script when `/proc/cpuinfo` says GenuineIntel):
`branch-misses, l1d-misses, raw:0x10d1` (L2 misses of retired loads) and `cache-refs, cache-misses,
dtlb-misses` (the generic cache events are requests to and misses of the last-level cache here). In the table:
`l2_miss` = L2 load misses, `fill_l3` = LLC requests that hit, `fill_dram` = LLC misses.

### ilo-prod (Zen 2; busy; root)

`/root/work/r8/threads/env.sh` (perf 7.3.1 on `PATH`; `lo` = `taskset -c 8-15 nice -n 19`). Source as a git
bundle (ROUND7.md, remote recipe). Pin the run to one of CPUs 8 to 15 (`-c 9`). Zen 2: `cache-misses` are L2
misses; no last-level-cache events; refills from L3 and DRAM are `dc-fill-ccx` and `dc-fill-dram`; groups of
three. Cycles on this host move with the load (a hover read 2,600 to 3,800 cycles warm in three runs); use it
for instructions, miss counts and flamegraphs, and mothership for cycles and wall time.

Long jobs go through `bench/remote/bg.sh` (its own rex block): a `setsid nohup` started from an `rx.sh`
script did not survive the block closing.

### The Mac

Instructions and cycles of the calling thread only (they include the kernel's), no miss counters; the cold
levels still show as cycles. Under load the cold cycles are noise (the `floor` cell read 350 and 1,781 cycles
at the L2 level in two runs ten minutes apart).

## Which lines, by what they are: `lines.sh`

`hot.sh` counts; `bench/r8/perf/lines.sh [-o OUT] [-c CPU] BIN CELL...` says which. Intel only (PEBS data
addresses), an ordinary user suffices. For each cell it samples, with every event a sample:

* `mem_load_retired.l1_miss` with L1 cold: the address of every load that missed L1 (every data line touched);
* `mem_load_retired.l3_miss` with L3 cold: the data lines that came from memory;
* `frontend_retired.l2_miss` with L3 cold: the code lines fetched from L3 or memory, by function;

and folds addresses by the regions the binary prints with `REGIONS=1` (`World::regions`: block, column table,
store tree, each column's pages split into rect lanes and records, live words, damage bitmap, model, stack).
The binary enables the sampler for its measured repetitions only (perf's control FIFO: `PERF_CTL`, `PERF_ACK`;
copy `PerfCtl` from `hot_x.rs`), and evicts with stores only, so neither set-up nor the eviction walk is in
the samples. The `floor` cell's counts are subtracted per kind. Output: `OUT/<cell>.lines.tsv`.

## Allocations and page faults: `alloc_x`

`cargo run --profile dist -p bough-x --example alloc_x`: the same cells under a counting global allocator and
`getrusage`: allocations and minor faults per repetition, and for each step of a start (model, world, mount,
publish, first frame). Not a release binary: it prints no instruction counts. A turn is expected to print 0
and 0.

## What has run, and what has not (2026-11-12)

* The in-process table (`hot.sh`): Mac, mothership (the tree walk, the first plan, the port, the final tree),
  ilo-prod. `lines.sh`: mothership. `alloc_x`, `bump_x`, `pixels_x`: Mac and mothership. Files in
  `bench/results/r8/threads/`.
* `-f` (flamegraphs): drawn on ilo-prod for seven cells (`/root/work/r8/threads/out/flame`). Usable for cells
  of tens of microseconds and up. **For a cell of a few microseconds the graph is the harness**: the two
  counter reads around each repetition are system calls and were 93 % of a hover's samples. A switch that
  drops the bracket for `-f` runs is the fix; not added.
* `-x` (perf stat slopes): run on ilo-prod (`pstat-ilo-prod-81da84ea75.tsv`). Branch and L1 misses agree with
  the in-process counters; instructions are 600 to 1,000 per repetition higher for every cell, because the
  loop reads and clears the damage outside the in-process bracket and the `floor` cell has none. So: **use
  the in-process table for a cell's cost**; a slope is only comparable with a floor that leaves the same
  damage.
* Not available: kernel-mode counts on mothership (not root), so a cell that is mostly system calls (mount,
  mapping an image) shows its user part in the counters and its whole in `wall_ns`.
* Confirmed runnable at the final tree: `mini.sh` -> `xfer.sh` -> build -> `hot.sh` and `lines.sh` on
  mothership, as written above (the last run is `hot-mothership-final.tsv`).
