"""results_doc.py : writes RESULTS.md from dashboard/data.r8.json, results/ and the hand-written text below."""
import os, sys, json, re
sys.path.insert(0, os.path.dirname(__file__))
import results_md as R   # also rewrites results_tables.md
D, table = R.D, R.table
def f(v, nd=1):  # R.f needs nd; the profile table calls f(v) and its cell rows carry NaN for the indirect counters
    return "-" if isinstance(v, float) and v != v else R.f(v, nd)
ROOT = R.ROOT
modes = D["modes"]
def run_row(n, purpose, timing, pinned):
    r = D["runs"][n]; s = r["summary"]; ls = [v for _, v in r["load"]]
    ident = "\n".join(r["identity"])
    start = re.search(r"start=(\S+)", ident).group(1); end = re.search(r"end=(\S+)", ident); rc = re.search(r"runner rc=(\d+)", ident)
    inv = sum(v["inv"] for v in s["reports"].values()); fl = sum(c["err"] for c in s["cells"].values())
    return (f"| {n} | {purpose} | {timing} | {pinned} | {start} to {end.group(1) if end else '?'} | rc {rc.group(1) if rc else '?'} | {len(r['fws'])} | "
            f"{(sum(c['ok'] + c['err'] for c in s['cells'].values()))} ({fl} with errors) | {s['n_reports']} ({inv} invalid) | {sum(ls)/len(ls):.1f} ({min(ls):.1f} to {max(ls):.1f}) |")
runs_tbl = ["| run | what | timing model | pinning | window (UTC) | exit | builds | cells | reports | 1-min load mean (min to max) |", "|---|---|---|---|---|---|---:|---|---|---|"]
for n, p, t, pin in (("cmp-2", "headline matrix, taken over from the previous runner (complete, kept)", "fixed 60 Hz", "none"), ("heavy-1", "heavy scenes (sheet, canvas), 4 frameworks", "fixed 60 Hz", "CPUs 0-7"),
                     ("sweep-60", "refresh sweep: rows, typing, spinner; bough, iced, freya, dioxus-skia", "fixed 60 Hz", "CPUs 0-7"), ("sweep-120", "same", "fixed 120 Hz", "CPUs 0-7"), ("sweep-240", "same", "fixed 240 Hz", "CPUs 0-7"),
                     ("cmp-u1", "headline matrix, patched runner", "unthrottled", "CPUs 0-7"), ("heavy-u1", "heavy scenes", "unthrottled", "CPUs 0-7"),
                     ("cmp-v1", "headline matrix", "vrr 48-144 Hz", "CPUs 0-7"), ("heavy-v1", "heavy scenes", "vrr 48-144 Hz", "CPUs 0-7"),
                     ("smoke-u1", "probe-semantics check (bough, iced), not reported as results", "unthrottled", "CPUs 0-7"), ("smoke-v1", "probe-semantics check (bough, iced), not reported as results", "vrr 48-144 Hz", "CPUs 0-7")):
    if n in D["runs"]: runs_tbl.append(run_row(n, p, t, pin))

def hl(key):
    m = modes[key]; h = m["headline"]
    return table(h["cols"], h["fws"], R.HEAD)

def heavy_tbl(key, sc, n):
    m = modes[key]; hv = m["heavy"]; h = m["headline"]
    ids = [i for i, c in hv["cols"].items() if c["scenario"] == sc and str(c["rows"]) == n]
    return table(hv["cols"], h["fws"], ids)

def spread_tbl():
    rows = ["| cell | bough | iced | freya | dioxus-skia |", "|---|---:|---:|---:|---:|"]
    sw = D["sweep"]["60"]["cells"]; c2 = modes["fixed"]["headline"]["cols"]
    def cmpv(label, col, key):
        out = []
        for fw in ("bough", "iced", "freya", "dioxus-skia"):
            a = c2[col]["vals"][fw]; b = sw[fw][key]
            out.append(f"{f(a['v'], 2)}{' †' if a['st']=='inv' else ''} / {f(b['v'], 2)}{' †' if b['st']=='inv' else ''}")
        rows.append(f"| {label} (cmp-2 / sweep-60) | " + " | ".join(out) + " |")
    cmpv("rows 3,000 hover total p50 ms", "hover3k", "hover_total"); cmpv("rows 3,000 hover input to commit ms", "hover3k_c", "hover_commit")
    cmpv("typing p50 ms", "typing", "typing_p50"); cmpv("spinner 10,000 CPU per frame ms", "spin10k", "spin_cpuf")
    return "\n".join(rows)

def validation_md():
    out = ["| view | hold-out test | tests | stated range held the true value | median error | 90th percentile | worst |", "|---|---|---:|---:|---:|---:|---:|"]
    for key in ("unthrottled", "fixed", "vrr"):
        v = modes[key]["estimate_validation"]
        for nme, lab in (("leave_one_out", "hide one valid cell, estimate it from the rest"), ("floem_like", "keep only the spinner-without-rows cells of a framework (floem's situation), estimate the rest")):
            x = v.get(nme)
            if x: out.append(f"| {modes[key]['title']} | {lab} | {x['n']} | {x['coverage']*100:.0f} % | x{x['median_factor']:.1f} | x{x['p90_factor']:.0f} | x{x['worst_factor']:,.0f} |")
    return "\n".join(out)

pf = json.load(open(os.path.join(ROOT, "results", "profiles", "profiles.json")))
prof_rows = ["| path | instructions / op | cycles / op | IPC | L1d misses / op | L2 misses / op | branches / op | branch misses / op | indirect predictions / op | indirect mispredicts / op | L1d per 1,000 instr | L2 per 1,000 instr | branch misses per 1,000 instr |", "|---|" + "---:|" * 12]
for r in pf["rows"]:
    prof_rows.append(f"| {r['label']} | {f(r['instr'])} | {f(r['cycles'])} | {r['ipc']:.2f} | {f(r['l1'])} | {f(r['l2'])} | {f(r['branches'])} | {f(r['bmiss'])} | {f(r['ind'])} | {f(r['indmiss'])} | {r['l1_per_kinstr']:.1f} | {r['l2_per_kinstr']:.2f} | {r['bmiss_per_kinstr']:.2f} |")
tops = []
for r in pf["rows"]:
    t = r.get("top")
    if not t: continue
    tops.append(f"### {r['label']}" + (f" (flamegraph `profiles/svg/{r['svg']}`)" if r.get("svg") else "") + "\n")
    tops.append("| # | self time | symbol (top ten by self time) | charged to nearest Rust frame | symbol |\n|---:|---:|---|---:|---|")
    for i, (x, y) in enumerate(zip(t["top10"], t["top10_attributed"] + [None] * 10), 1):
        tops.append(f"| {i} | {x['pct']:.1f} % | `{x['symbol'][:100]}` | {y['pct']:.1f} % | `{y['symbol'][:80]}` |" if y else f"| {i} | {x['pct']:.1f} % | `{x['symbol'][:100]}` | | |")
    tops.append(f"\nProcess {t.get('process')}, {t.get('total_cycles_weighted_samples', 0):,} cycles sampled; {t.get('other_processes_pct', 0):.1f} % of the recording in other processes (excluded). Dispatch-like frames (closures, call_once, vtable or shim frames, run_head): {t['dispatch_like_self_pct']:.1f} % of self time.\n")

doc = f"""# bench-r8 results

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

{chr(10).join(runs_tbl)}

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

{hl('unthrottled')}

Values: ms unless noted; † = invalid report; startup columns are unchecked (shown without a mark here, see section 0); `*~x*` = estimate, never ranked.

{R.winloss_md(modes['unthrottled']['headline']['cols'], modes['unthrottled']['headline']['fws'], R.HIGHER)}

Not ranked, startup (unchecked): rows 3,000 bough 56.3 ms against iced 190, egui 179; virtual 100,000 bough 39.1 ms against vizia 104.5, egui 121.8; spinner without rows 38.4 ms.

Failures in cmp-u1: floem failed the layout check in 16 cells (colour at (640,31) or (640,71), `#f2f3f5` for the spec's `#e5e7eb`); **vizia typing** failed the layout check on this run (black pixels at the four points: the typing app painted nothing); nothing else failed. cmp-u1 had 17 failed measurements in all.

### Heavy scenes, unthrottled (heavy-u1; measured: bough, iced, egui, gpui-kit; the others are estimates)

{R.winloss_md(modes['unthrottled']['heavy']['cols'], modes['unthrottled']['heavy']['measured_fws'], R.HIGHER)}

Sheet 10,000 rows:

{heavy_tbl('unthrottled', 'sheet', '10000')}

Canvas 5,000 nodes:

{heavy_tbl('unthrottled', 'canvas', '5000')}

The sheet at 10,000 rows, bough: the resize probe failed ("no matching window", cell `sheet/bough/default/60hz/10000`); canvas and the other sheet probes completed. All four sizes are in `results_tables.md`.

## 4. Fixed 60 Hz view (`cmp-2`, `heavy-1`, sweeps): with the split

Every latency here contains a wait for the next compositor latch, about 10 ms for every framework (9.3 to 11 ms), which hides the app's share. For bough rows-3,000 hover, 11.52 ms total is 0.93 ms in the app (input to commit), 9.58 ms waiting for the latch and 1.00 ms from latch to present. The split segments are medians of separately pooled samples: they do not add up to the total median and the percentiles above p50 compose even less (the table below shows the harness's four parts; a part can exceed the total).

{hl('fixed')}

{R.winloss_md(modes['fixed']['headline']['cols'], modes['fixed']['headline']['fws'], R.HIGHER)}

**Startup in this view is load-contaminated and unchecked.** In cmp-2 bough read 564.9 ms (virtual 100,000) and 681.8 ms (spinner without rows, nothing to mount); in the pinned unthrottled and VRR runs the same cells read 39.1 and 38.4 ms (unthrottled) and 47.5 and 47.2 ms (vrr). Other frameworks moved 2 to 5 times between cmp-2 and cmp-u1 as well. An earlier note that bough was "7th at 564.9 ms" came from this contaminated value and must not be used.

cmp-3 (the repeat of cmp-2 for run-to-run spread) was **not run**: the host was never quiet (1-minute load 2.8 to 21.6 in every window, other tenants building and profiling throughout). Run-to-run spread is therefore not available for the full matrix. The only same-cell pairs are cmp-2 (unpinned, load 9) and sweep-60 (pinned, load 8) for the four frameworks of the sweep, and the unthrottled smoke run against cmp-u1:

{spread_tbl()}

(bough, unthrottled, rows-3,000 hover total p50: smoke-u1 0.98 ms, cmp-u1 0.99 ms; typing 0.30 and 0.25 ms.)

### Heavy scenes, fixed (heavy-1)

{R.winloss_md(modes['fixed']['heavy']['cols'], modes['fixed']['heavy']['measured_fws'], R.HIGHER)}

49 of 86 reports in heavy-1 failed `wake_lateness`, so few cells are valid and the win and loss counts rest on few comparisons. Sheet 10,000 rows:

{heavy_tbl('fixed', 'sheet', '10000')}

Canvas 5,000 nodes:

{heavy_tbl('fixed', 'canvas', '5000')}

**Last time bough lost the canvas, the 10,000-row sheet's memory, startup and resize. Is that still so?** Yes for all four, in every timing mode. Canvas: bough hover 925.8 ms (invalid report) against iced 85 and egui 94 at 60 Hz, 83 ms against iced 17.6 at 1,000 nodes and theme switch 1,151 ms against egui 71 unthrottled. Sheet 10,000 rows idle RSS: bough 331 to 333 MiB against egui 149 to 157 and gpui-kit 187 (iced 550); bough is better than iced and worse than egui and gpui-kit. Startup (unchecked): sheet 10,000 rows 677 to 713 ms against egui 130 to 246; canvas 5,000 about 1.2 s against egui 257 to 265. Resize: bough sheet 10,000 rows failed twice (below). Against the first heavy matrix (r7-scenes-1, bough at d4393497a0, different conditions): sheet 10,000 hover 29.4 ms now against 14.6 then, input to commit 17.9 against 5.1; see the "then and now" table in `results_tables.md` (the two runs differ in bough commit, pinning and load, so it is direction, not a controlled comparison).

### Latency split (fixed 60 Hz, cmp-2)

See `results_tables.md` ("Latency split"); the bough hover split is above, the app's share for typing is 0.43 ms of 8.44, for table update 0.44 ms of 12.1 (sweep and cmp-2 values).

### Refresh sweep (fixed grid at 60, 120 and 240 Hz; bough, iced, freya, dioxus-skia; rows 3,000 hover, typing, spinner)

bough rows-3,000 hover total p50 13.3 / 7.0 / 3.7 ms at 60 / 120 / 240 Hz: the app's input to commit stays 1.09 / 0.98 / 1.03 ms, the wait for the latch falls 11.3 / 5.0 / 1.7 ms with the refresh period, latch to present 1.0 ms. iced's app share is 6.5 / 5.3 / 6.3 ms, freya 5.1 / 6.0 / 5.9, dioxus-skia 6.4 / 7.3 / 7.6 (several of those reports are invalid). Typing p50: bough 11.1 / 4.5 / 3.4 ms with 0.4 ms of handling. Spinner above 10,000 rows: bough stays at 62.5 fps at 120 and 240 Hz (its own pacing); freya 98 fps at 240 Hz; iced about 18 fps. Full table in `results_tables.md`; all 60 reports per rate are flat-tabled in `results/sweep-*/flat`.

## 5. Variable refresh view (`cmp-v1`, `heavy-v1`; vrr 48-144 Hz)

{hl('vrr')}

{R.winloss_md(modes['vrr']['headline']['cols'], modes['vrr']['headline']['fws'], R.HIGHER)}

195 of 587 reports in cmp-v1 and 47 of 86 in heavy-v1 failed `wake_lateness` (VRR wake timing under load). Latency sits between the two other views: bough rows-3,000 hover total 4.1 ms in the smoke run (1.46 ms app share, about 2.6 ms waiting for the shortest VRR period of 6.9 ms).

## 6. Animation

The spinner scenarios are the only animation cells that exist (a turning spinner with no rows and one above 10,000 static rows). **A richer animation benchmark does not exist yet**: no transitions, springs, many simultaneous animations, animated layout or text. The ticker is a data-update load, not an animation. Fixed grid (60 Hz) frame pacing and CPU per frame for all twelve builds, and the unthrottled and VRR rates, are in `results_tables.md` ("Animation"); the dashboard has them per mode. Headline (spinner above 10,000 rows, CPU per frame in ms; fps): unthrottled bough 1.15 ms at 62.5 fps, zgui 12.1 at 60, freya 17.3 at 153, iced 78.9 at 22.9, egui 70.7 at 17.9, gpui-kit 349 at 3.7 (many of those unthrottled values are valid; see tables).

## 7. Estimates, and why they are only estimates

Method (`tools/estimate.py`): per class of quantity (input latency, CPU per frame, frame interval, memory, idle CPU, startup, frame rate) log value = framework level + column level, fitted by median polish on valid values only; the framework level needs two own valid cells in the class (else a cross-class level over all its cells, flagged); the range is the estimate x exp(+-2 s) with s from the median absolute residual of the framework's own cells (floors 0.15, 0.30 cross-class); values are rounded to two significant figures. Estimates fill failed cells (floem in all but one cell, vizia typing in cmp-u1, bough's failed probes) and the heavy scenes for the eight frameworks never ported to them.

**Limits:** the model assumes multiplicative scaling and cannot represent a cliff (fallback path, capacity overflow, layout failure, worker start-up, full re-bin); missing cells are not random (failed or unported cells are often the expensive ones); an estimate for a workload that failed or was never implemented is not evidence that it is supported; the cross-class fallback has no physical basis. Held-out tests of the method (hide valid cells it can see):

{validation_md()}

The stated ranges are meant to hold about 95 % and hold the true value in about three of four tests; the worst errors are the CPU executor's cliffs (bough canvas theme switch measured 1,151 ms, estimated 21 ms from bough's other cells; bough idle CPU, a value near zero nothing else resembles). Treat every estimate as an order of magnitude at best. Estimates appear in the tables as `*~x*`, are drawn dotted below the measured rows on the dashboard, and are excluded from every rank and every win or loss list.

## 8. Profiles of bough's hot paths (the round-7 façade; not timings)

Method: the headless driver of `comparisons/bough-guibench` (`--render`: CPU executor, no window host) for rows hover and the eight fine table operations; for the sheet at 10,000 rows (hover, theme) and the canvas at 5,000 nodes (hover), the gui-bench cell itself (scenes binary `4c9a8a961d4fe29c`, CPU executor, real fakeland window host) with `perf` attached to the app process.
`perf stat` in groups of three events, user mode only, one group per run: cycles + instructions + branches; instructions + branches + branch-misses; instructions + L1-dcache-loads + L1-dcache-load-misses; instructions + cache-references + cache-misses (on this Zen 2 CPU `cache-misses` are **L2 misses** and L1-dcache-load-misses are L2 requests from the data cache; there are no LLC events); instructions + `bp_dyn_ind_pred` + `ex_ret_brn_ind_misp` (indirect branches predicted by the indirect predictor and mispredicted indirect branches; headless paths only: the attached-cell indirect runs produced no counts). Headless paths: slope between 100 and 1,000 repeats (three perf runs each), pinned to CPU 2, per operation (an operation is one input, the hover onto or away from a row, or one key press, with its frame), so mount and start-up cancel. Cells: one cell run per group, `perf stat -I 50` summed over the harness's measured-iteration windows (its own monotonic timestamps), divided by the number of measured repeats (3 iterations x 12 repeats for hover, x 8 for theme; one repeat = pointer onto the target and away, or one theme switch); the windows include the harness's settle time, which adds few instructions. Windows are aligned to within 50 ms.
`perf record` with DWARF call graphs (headless 4,999 Hz on one pinned CPU; cells 999 Hz on the app process), flamegraphs by inferno; per-probe flamegraphs for cells use only the samples inside the measured windows. Sample weights are cycles. The PMU was shared with other sessions' perf runs, so every group ran on the counters about 75 % of the time (counts are scaled; ratios inside one group are exact), and counts under a loaded SMT host carry more noise than the repeat statistics suggest.

{chr(10).join(prof_rows)}

What the profiles show:

* **Sheet 10,000 rows hover (the gui-bench cell) looks like dispatch and pointer chasing.** 76 M instructions per hover at IPC 1.12, 20 L1d and 6.6 L2 misses per 1,000 instructions (the table operations run at IPC 1.8 to 2.6 with 3.5 to 6 L1d misses per 1,000). Self time: `SealCheck::word` 8.5 %, `memmove` 7.9 %, `Rt::run_head` 7.9 %, `split_at_checked` 5.6 %, `i32::clamp` 4.9 %, `Spec::same` 4 %. `run_head` is the wire-chain walk (the linked chain from a signal's head, one indirect call per wire) and `SealCheck::word` a per-word integrity check; frames that are closures, `call_once` or chain walkers hold 28 % of self time. Theme switch at the same size: 139 M instructions at IPC 1.40, same top symbols.
* **Canvas 5,000 nodes hover is arithmetic.** 6.6 G instructions per hover at IPC 1.82 and almost no cache misses (0.1 L1d and 0.04 L2 per 1,000 instructions): `cpu_raster::shadow`, `erf2` and `expf` are about 60 % of self time (blurred shadows by an exp-based error function per pixel on the CPU executor). This is why the canvas loses: it is the executor's shadow math, not the framework's update path.
* **Rows hover through the headless driver is copying and allocation.** 7.6 M instructions per hover at IPC 2.26, 84 K L1d and 17.6 K L2 misses per operation; `memmove` 46 %, `memcmp` 12 %, `free` 11 %, `malloc` 9 %, `memset` 5 %, `realloc` 4 % of cycles; only 119 indirect predictions and 39 indirect mispredicts per operation: dispatch is not the cost on this path, copying a surface and allocating are.
* **Table operations**: remove, insert, replace and clear cost 24 to 28 M instructions (one operation re-rasters many tiles; `CpuRaster::tile`, `blend_span` and `memset` lead), select, update10, swap and append cost 1 to 2.4 M. Branch mispredictions are low everywhere (0.2 to 1.4 per 1,000 instructions).

Top ten symbols by self time per path (and the same time charged to the nearest Rust frame, so that libc leaves such as `memmove` and `malloc` are attributed to their callers where a Rust frame exists):

{chr(10).join(tops)}

Files: flamegraphs `profiles/svg/*.svg` (rows3k-hover, table-select, table-update10, table-swap, table-remove, table-insert, table-replace, table-append, table-clear, sheet10k-hover, sheet10k-theme, canvas5k-hover), collapsed stacks `profiles/collapsed/`, stat tables `profiles/stat/`, per-cell sums `profiles/stat-*.json`, `profiles/params-*.json`, top-ten JSON `profiles/top/`.

## 9. Failures (reported as failures; nothing was rerun to make a cell pass)

* **floem**: fails the harness's layout check in 16 of 17 cells in cmp-2, cmp-u1 and cmp-v1 (a 1 px border colour at (640,31) or (640,71): `#f2f3f5` for the spec's `#e5e7eb`); only the spinner without rows runs. No timing exists for the failed cells; the tables show estimates.
* **vizia typing, unthrottled (cmp-u1)**: layout check failed (black at all four check points: nothing painted). It passed in cmp-2 and cmp-v1.
* **bough, sheet 10,000 rows (heavy-1)**: the scroll probe failed ("no matching window") and the resize probe timed out. The app log ends with the compositor closing the connection: `results/heavy-1/failed-cell/sheet-10000-bough-app.log`:
  ```
  bough-bench: Sheet {{ rows: 10000 }}
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
"""
open(os.path.join(ROOT, "RESULTS.md"), "w").write(doc)
print("RESULTS.md", len(doc), "bytes")
