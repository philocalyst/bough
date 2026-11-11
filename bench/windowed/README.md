# Windowed comparison (macOS): zgx's suite runner, with bough

`run_macos_comparison.py` is zgx's `scripts/run_macos_comparison.py` (zgx
commit `06debe9d`) with a `bough` framework added. It opens real windows,
samples each app's CPU, energy, memory and GPU time, and the window server's,
with `macsample`, and times the frames each window shows with `framepace`. It
measures bough's dist apps beside zgx, GPUI Fast, MyGo and Rune, under the same
protocol.

`ROOT` is still the zgx checkout (`/Users/mileswirht/projects/_ref/zgx`,
override with `ZGX_ROOT`). zgx's and the other frameworks' binaries, their
builds, and the bundled font (`comparisons/fonts/DejaVuSans.ttf`) all come
from there. bough's apps come from `$BOUGH_TARGET`, which defaults to
`$BOUGH_ROOT/target/fusion/dist` with `BOUGH_ROOT=/Users/mileswirht/projects/bough`.

## 1. Build bough first

The runner never builds bough. A run that selects bough stops before
building or launching anything if one of bough's binaries is missing.

```sh
cd /Users/mileswirht/projects/bough && RUSTC_WRAPPER= PATH=~/.local/share/cargo/bin:$PATH \
  CARGO_TARGET_DIR=/Users/mileswirht/projects/bough/target/fusion \
  cargo build --profile dist -p bough-heavy -p bough-scenes -p bough-vlist
```

This builds `heavy_workload` (heavy), `lab_workload` (lab), `animate_workload`
(animate), `blink_workload` (the `field` scene, a blinking caret) and
`vlist_workload` (vlist). There is no bough entry for `viewport`.

Without `--skip-build`, the runner builds the zgx side itself: cargo in the
zgx checkout and in `comparisons/gpui-fast`, plus `comparisons/{rune,mygo}/build.sh`.
MyGo needs `go` on PATH. Wait for every build to finish before measuring,
because the preflight waits until no `cargo` or `rustc` is running.

## 2. Check the plan without opening windows

```sh
python3 bench/windowed/run_macos_comparison.py --suite --scene heavy --dry-run
python3 bench/windowed/run_macos_comparison.py --quick --scene heavy --frameworks zgx,bough --dry-run
```

`--dry-run` prints:

- the frameworks, modes and binary paths, including which are MISSING
- the build commands it would run
- the output directory
- the environment every app gets
- every launch in order: probe, trials, blank captures and pacing, each with
  its `ZGX_MODE`/`ZGX_SECONDS` and binary
- the number of launches and their total minutes

It builds, stages and launches nothing.

## 3. Run

Run these from the bough checkout, in a terminal that has the permissions in
section 4, on an idle desktop. Keep the Mac awake and the windows uncovered,
and don't use the Mac while a run is going.

Full suite protocol, one scene at a time. Each scene writes its own result
directory. The approximate time is the windows alone, from `--dry-run`:

```sh
cd /Users/mileswirht/projects/bough
python3 bench/windowed/run_macos_comparison.py --suite --scene heavy     # ~64 min
python3 bench/windowed/run_macos_comparison.py --suite --scene lab       # ~46 min
python3 bench/windowed/run_macos_comparison.py --suite --scene animate   # ~17 min
python3 bench/windowed/run_macos_comparison.py --suite --scene vlist     # ~62 min, height probe first
python3 bench/windowed/run_macos_comparison.py --suite --scene field     # ~17 min, idle caret (bough: blink_workload)
```

The suite's frameworks are zgx (and zgx-luau in vlist), bough, gpui-fast,
mygo and rune.

- **Pick the frameworks:** add `--frameworks zgx,bough,gpui-fast` to limit a
  suite run, for example when `go` or Rune's build is unavailable.
- **Skip the build:** add `--skip-build` once everything is built.
- **Package power:** add `--powermetrics` for package, CPU and GPU power.

Quick heavy cell. It's for iterating, not publishing: 2 repeats, with the
pacing captured during the trials.

```sh
python3 bench/windowed/run_macos_comparison.py --quick --scene heavy --frameworks zgx,bough   # ~9 min
python3 bench/windowed/run_macos_comparison.py --quick --scene heavy                          # zgx, gpui-fast, bough, ~12 min
```

Before a vlist run, every framework's `ZGX_MODE=probe` row heights, bough's
included, must agree within 2 pt in the bundled font. If they don't, the run
stops.

## 4. Permissions (for the terminal app you run from)

- **Screen Recording** (Privacy & Security): needed by `framepace`, which
  takes the pacing captures, and by `blankprobe`, which takes vlist's blank-row
  captures. Without it the pacing and blank results fail.
- **Automation → System Events, and Accessibility:** the preflight reads system
  dialogs through System Events. If it can't, it counts a dialog as being on
  screen and waits, then stops. `--hide Zeron` and the readiness fallback
  (raising the app's window to the front) use it too. When this was set up,
  the agent's terminal was denied:
  `Not authorized to send Apple events to System Events (-1743)`.
- **sudo, optional:** `--powermetrics` runs `sudo -n /usr/bin/powermetrics`.
  Run `sudo -v` first in the same terminal, or add a sudoers rule for
  `/usr/bin/powermetrics`. Without it the power columns stay empty.

Check the machine before a run:

```sh
python3 bench/windowed/preflight.py          # quiet? heavy processes, CPU of the rest, dialogs
python3 bench/windowed/preflight.py --state  # displays, power mode, macOS, uptime
```

## Results

Runs go to `bench/windowed/results/latest-macos[-<scene>][-quick]-<date>-<zgx commit>/`,
never into zgx's `docs/results`. Each run directory holds `current.csv`,
`pacing.csv`, `summary.json`, `audit.json` and per-trial logs.

`audit.json` has a `bough` entry with:

- bough's commit
- whether bough's source was dirty
- `BOUGH_TARGET`
- the build command

Its `binaries` entry holds the sha256 of each binary measured. `--out`
chooses a different directory.

## What changed from zgx's runner

- **Paths:** `BENCH` is this directory. `ROOT` is fixed to the zgx checkout
  (`ZGX_ROOT`). The new constants are `BOUGH_ROOT`, `BOUGH_TARGET`,
  `BOUGH_BUILD` and `BUILT_ELSEWHERE = {'bough'}`.
- **Helpers:** `preflight` and `staging` are imported from this directory.
- **Scenes:** each of lab, heavy, animate, vlist and field has a `bough`
  binary. `bough` is in `SUITE_FRAMEWORKS`, and `--quick` keeps zgx,
  gpui-fast and bough (`QUICK_FRAMEWORKS`).
- **Build:** `build()` never builds bough. `main()` stops with the build
  command above if a bough binary this run needs is missing.
- **Tools:** the Swift tools are compiled from this directory's copies into
  `/tmp/bough-windowed/` (override with `BOUGH_TOOLS_DIR`), under zgx's names
  (`zgx-macsample`, …), so the preflight still leaves them out of its CPU
  count. zgx's own `/tmp/zgx-*` tools are left alone.
- **Staging:** apps are still staged under `/tmp/zgx-staged`, keyed by
  content (`ZGX_STAGE_DIR`).
- **Output:** the default output is `bench/windowed/results/`, and
  `audit.json` gains a `bough` entry.
- **`--dry-run`:** new; `build()` and `tools()` take `dry=` to print their
  commands.
- **`preflight.py`:** runs `/bin/ps` instead of the first `ps` on PATH. On
  this Mac that is nix's `ps`, which lacks the task-ports entitlement and
  prints no time column. zgx's own preflight crashes on it with
  `ValueError: could not convert string to float`.
- **Unchanged:** every other file here is a byte-identical copy of zgx's:
  `staging.py`, `macsample.swift`, `framepace.swift`, `blankprobe.swift`
  and `window_ready.swift`.

bough's apps follow zgx's adapter contract, so they launch exactly as zgx's
do, with the same readiness wait, sampling, validity checks and output
parsing:

- **Environment:** `ZGX_MODE`, `ZGX_SECONDS` and `ZGX_FONT_FILE`, plus the
  `ZGUI_*` aliases.
- **Font report:** `font_file_fnv64=` on stderr.
- **Ticks:**
  - lab and heavy report `{"framework":"bough",...,"ticks":N}`
  - animate reports `animation_frames=N fps=F`
  - field reports `blink_frames=N workload_ticks=0`
  - vlist reports `workload_ticks=N`, plus the probe's `row_heights=…`

Windows are found by process id, not by title, so bough's titles don't
matter: "bough heavy", "bough lab", "bough animation", "bough blink" and
"bough vlist". `--pgo` doesn't cover bough because there is no PGO build of
it.
