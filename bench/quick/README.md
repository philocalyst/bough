# bench/quick: the fast end-to-end screen

One command builds what it needs and measures the current tree in well under two minutes once the drivers are built:

```sh
bench/quick/run.sh --base bench/quick/baselines/aa9943e7ae.json      # table + JSON on stdout, every cell's ratio
bench/quick/run.sh --out q.json                                       # JSON to a file
bench/quick/run.sh --self-test --base bench/quick/baselines/aa9943e7ae.json   # the negative control
bench/quick/run.sh --only r0,redescribe --no-build                    # a subset
```

`run.sh` sets the ROUND6 environment (rustup's proxies first, `RUSTC_WRAPPER` cleared, `CARGO_BUILD_JOBS=4`, the
checkout's own `CARGO_TARGET_DIR`) and runs `quick.py`. The DejaVu face comes from `$BOUGH_REF` (default
`~/projects/_ref`) like every other bough benchmark. Build profile: `release`, the one `cargo test --release` gates
use, so a gate run and a quick run share their library builds.

## What each cell is

Every cell in the JSON carries `kind`, `unit`, `value` (the median), `noise` (n, min, median, max) and `method` (the
exact workload and how it was counted). The top level carries the commit, whether the tree was dirty, the merge base
with `lead/round6-base` and which measured files differ from it, the toolchain, the load average at start and end, the
lock state and the seconds per group.

| group | cells | how |
|---|---|---|
| `r0` | `slope.{heavy.stream, heavy.scroll, lab.both, animate, vlist.scroll}.r0` | the headless drivers at `RENDER=0`, one process per run, `TICKS=300` (animate `FRAMES=600`); instructions of the driver's in-process window (ticks 60..300, animate 120..600; process-wide `ri_instructions`) / ticks; 5 serial runs, median |
| `r1` | the same `.r1`, and `.r1.main` | `RENDER=1` (an offscreen Metal frame per damaged tick); `.r1` process-wide, `.r1.main` the main thread only (`thread_selfcounts`): the GPU driver's own threads are the noisy part of R1; 5 serial runs |
| `redescribe` | `redescribe.{owned,static,bare}.1000` | `crates/bough/examples/redescribe_cost.rs 1000`: one plain field changed per `App::turn` on a 1,000-row column, main-thread instructions per turn by slope (48 vs 16 turns), 3 runs |
| `mount` | `mount.{empty,counter,list1000}` | `quick_life mount <what>`: `App::new(view, cfg, gpu=false)` + the first frame's CPU work in a fresh process; `empty` is the fixed cost (runtime, region, font tables); `list1000` is 1,000 rows (box + two labels) through `each` (not virtualised); 5 serial runs |
| `life` | `startup.{counter,heavy}.{wall,instr}`, `shutdown.{counter,heavy}.{wall,instr}`, `memory.{counter,heavy}.{footprint,rss}_{first,100}` | `quick_life counter` and `heavy_life` (bench/quick/life.rs): headless, built as `Application::run` builds an app (`App::new(view, cfg, true)`, no `prepare`). Startup: the kernel's process start stamp to the first frame complete on the GPU. Shutdown: the close request (after 100 frames) to the parent's `waitid` seeing the exit; instructions = the zombie's total minus those at the close request. One unmeasured launch first, then 5 interleaved cold processes under the measurement lock |
| `size` | `size.counter.production` | `target/production/examples/counter` as found (never built by this run; skipped when absent) |
| `smoke` | `smoke.pixels` | release test binaries run directly: `bough-vlist --test correctness scroll_is_pixel_exact` (scroll by copy at ring ages 1..4 and the windowed ring: fails with `--features bough/sabotage_copy`), `bough-heavy --test correctness both_is_pixel_exact_at_every_ring_age` (text, numbers, scroll, damage at every ring age), `bough --test counter three_clicks_count_to_three_and_repaint_exactly` (the facade's input, hit test and a repaired frame against a full redraw) |

`details.life` holds every split: pre-`main` (dyld and framework initialisers), `App::new`, the first frame; the
presenter's drop, the rest of the app's drop and the exit; the kernel's own exit stamp; the lifetime maximum
footprint; the warm-up launch's time.

## Comparison and flags

`--base FILE` prints each cell's base value and ratio. Nothing is averaged across cells.

* `instr` and `bytes`: flagged `REGRESSED` above 1.02x (`better` below 0.98x). When the base's own spread exceeds 2 %
  the flag says so (`base spread x %`): R1 cells are the ones that do.
* `time` and `mem`: flagged when the median leaves the base's min..max band; `(unlocked)` when either run measured
  without the lock.
* `gate`: `FAILED` when the smoke fails.

The exit status is nonzero when any cell is flagged `REGRESSED` or `FAILED`.

## The negative control

`bough_metal::perf::inject()` spins `QUICK_INJECT` loop iterations (about 5 instructions each) and
`inject_ms()` spins `QUICK_INJECT_MS` milliseconds; every driver calls them inside its measured window (per tick,
per turn, per mount, in the startup and shutdown windows). `--self-test` (= `large`: 1,000,000 iterations and 40 ms)
must flag every slope, re-description, mount, startup and shutdown cell, or it exits 1. `--self-test small`
(1,000 iterations, about 5 K instructions) shows the detection floor: which cells a 5 K-instruction regression per
unit reaches.
