#!/usr/bin/env python3
"""bench/quick: bough's fast end-to-end screen. Run it through `bench/quick/run.sh` (environment, then this).

    run.sh [--out FILE] [--base FILE] [--only GROUPS] [--no-build] [--self-test [small|large]] [--lock-wait S]

Builds what it needs (release profile: the profile `cargo test --release` gates share), measures, prints one JSON
object (to --out, or stdout) and one table; with --base, every cell's ratio to the baseline and a flag:

  instr  instructions (proc_pid_rusage ri_instructions; valid under load, no lock): flagged above 1.02x the base
  time   wall time (median of 5, under the measurement lock): flagged when the median leaves the base's min..max band
  mem    footprint / resident bytes (median of 5): flagged when the median leaves the base's min..max band
  bytes  file size: flagged above 1.02x
  gate   the pixel smoke: flagged when it fails

Groups (--only, comma separated): r0 r1 redescribe mount life size smoke. README.md says what each cell measures.
--self-test runs with QUICK_INJECT / QUICK_INJECT_MS set (bough_metal::perf::inject: a known spin inside every
measured window) and exits nonzero unless the comparison flags every injected cell.
"""
import argparse, ctypes, datetime, json, os, platform, re, statistics, subprocess, sys, time
from concurrent.futures import ThreadPoolExecutor

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
TARGET = os.environ.get("CARGO_TARGET_DIR") or os.path.join(ROOT, "target")
REF = os.path.expanduser(os.environ.get("BOUGH_REF", "~/projects/_ref"))
FONT = os.environ.get("ZGX_FONT_FILE") or os.path.join(REF, "zgx/comparisons/fonts/DejaVuSans.ttf")
JOBS = int(os.environ.get("QUICK_JOBS", "4"))

# ------------------------------------------------------------------------------------------------ what is built
PKGS = ["bough-heavy", "bough-scenes", "bough-vlist", "bough"]
BINS = ["heavy_headless", "heavy_life", "lab_headless", "animate_headless", "vlist_headless"]
EXAMPLES = ["quick_life", "redescribe_cost"]
# The pixel smoke: the fastest gates that still fail on a broken presenter (README.md "Smoke").
SMOKE = [
    ("bough-vlist", "correctness", ["scroll_is_pixel_exact"]),
    ("bough-heavy", "correctness", ["both_is_pixel_exact_at_every_ring_age"]),
    ("bough", "counter", ["three_clicks_count_to_three_and_repaint_exactly"]),
]
SMOKE_TESTS = sorted({t for _, t, _ in SMOKE})

# ------------------------------------------------------------------------------------------------ the workloads
# (cell, binary, env, tick variable, ticks): one process per run, the window is the driver's own
# (`quick_window from=W to=N instr=I thread=T`: ticks W..N, after warm-up, final GPU sync included).
SCENES = [
    ("heavy.stream", "heavy_headless", {"ZGX_MODE": "stream"}, "TICKS", 300),
    ("heavy.scroll", "heavy_headless", {"ZGX_MODE": "scroll"}, "TICKS", 300),
    ("lab.both", "lab_headless", {"ZGX_MODE": "both"}, "TICKS", 300),
    ("animate", "animate_headless", {}, "FRAMES", 600),
    ("vlist.scroll", "vlist_headless", {"ZGX_MODE": "scroll"}, "TICKS", 300),
]
R0_REPS, R1_REPS, LIFE_REPS, MOUNT_REPS, PAR_REPS = 5, 5, 5, 5, 3

libc = ctypes.CDLL("/usr/lib/libSystem.B.dylib", use_errno=True)


class _Rusage(ctypes.Structure):
    _fields_ = [("uuid", ctypes.c_uint8 * 16), ("v", ctypes.c_uint64 * 40)]


class _Timebase(ctypes.Structure):
    _fields_ = [("numer", ctypes.c_uint32), ("denom", ctypes.c_uint32)]


_tb = _Timebase()
libc.mach_timebase_info(ctypes.byref(_tb))
# rusage_info_v4 after the uuid: [7] phys_footprint, [8] proc_start_abstime, [9] proc_exit_abstime,
# [28] lifetime_max_phys_footprint, [29] instructions
RI_FOOTPRINT, RI_START, RI_EXIT, RI_MAXFP, RI_INSTR = 7, 8, 9, 28, 29


def abs_ns(t):
    return t * _tb.numer // _tb.denom


def up_ns():
    return time.clock_gettime_ns(time.CLOCK_UPTIME_RAW)


def loadavg():
    return [round(x, 2) for x in os.getloadavg()]


def env_for(extra):
    e = dict(os.environ, ZGX_FONT_FILE=FONT)
    e.update({k: str(v) for k, v in extra.items()})
    return e


def noise(xs):
    return {"n": len(xs), "min": min(xs), "median": statistics.median(xs), "max": max(xs)}


def cell(kind, unit, xs, method, **kw):
    n = noise(xs)
    return dict(kind=kind, unit=unit, value=n["median"], noise=n, method=method, **kw)


def git(*a):
    return subprocess.run(["git", "-C", ROOT, *a], capture_output=True, text=True).stdout.strip()


# ------------------------------------------------------------------------------------------------ build
PROFILE = os.environ.get("QUICK_PROFILE", "release")


def pflag():
    return ["--release"] if PROFILE == "release" else ["--profile", PROFILE]


def build(paths, sabotage=False):
    """Two cargo invocations: the drivers, then the smoke's test binaries (a package's integration tests make cargo
    build every binary of that package, so the tests' packages are named alone)."""
    drivers = ["cargo", "build", *pflag(), "--message-format=json-render-diagnostics"]
    for p in PKGS:
        drivers += ["-p", p]
    for b in BINS:
        drivers += ["--bin", b]
    for x in EXAMPLES:
        drivers += ["--example", x]
    tests = ["cargo", "build", *pflag(), "--message-format=json-render-diagnostics"]
    if sabotage:
        tests += ["--features", "bough/sabotage_copy"]
    for p in sorted({p for p, _, _ in SMOKE}):
        tests += ["-p", p]
    for t in SMOKE_TESTS:
        tests += ["--test", t]
    t0 = time.time()
    for cmd in ((tests,) if sabotage else (drivers, tests)):
        p = subprocess.Popen(cmd, cwd=ROOT, stdout=subprocess.PIPE, text=True)
        for line in p.stdout:
            try:
                m = json.loads(line)
            except ValueError:
                continue
            if m.get("reason") == "compiler-artifact" and m.get("executable"):
                t = m["target"]
                kind = "test" if m["profile"].get("test") else t["kind"][0]
                pkg = os.path.basename(os.path.dirname(m["manifest_path"]))  # every workspace package is named as its dir
                paths[(kind, t["name"], pkg if kind == "test" else "")] = (m["executable"], os.path.dirname(m["manifest_path"]))
        if p.wait() != 0:
            sys.exit(f"quick: build failed: {' '.join(cmd)}")
    return round(time.time() - t0, 1)


def find_paths(paths):
    """--no-build: the executables of the last build (tests by their newest hash-suffixed file)."""
    rel = os.path.join(TARGET, PROFILE)
    for b in BINS:
        paths[("bin", b, "")] = (os.path.join(rel, b), ROOT)
    for x in EXAMPLES:
        paths[("example", x, "")] = (os.path.join(rel, "examples", x), ROOT)
    import glob
    for pkg, t, _ in SMOKE:
        # cargo's build-dir layout (build/<pkg>/<hash>/out/<test>-<hash>), or the older deps/<test>-<hash>
        c = [f for f in glob.glob(os.path.join(rel, "build", pkg, "*", "out", t + "-*")) + glob.glob(os.path.join(rel, "deps", t + "-*"))
             if "." not in os.path.basename(f) and os.access(f, os.X_OK)]
        man = {"bough": "crates/bough"}.get(pkg, "comparisons/" + pkg)
        if c:
            paths[("test", t, pkg)] = (max(c, key=os.path.getmtime), os.path.join(ROOT, man))


# ------------------------------------------------------------------------------------------------ measurements
def window(exe, extra):
    p = subprocess.run([exe], env=env_for(extra), capture_output=True, text=True)
    m = re.search(r"quick_window from=(\d+) to=(\d+) instr=(\d+) thread=(\d+)", p.stdout)
    if not m:
        sys.exit(f"quick: {exe} {extra}: no quick_window line\n{p.stdout}\n{p.stderr}")
    a, b, i, t = map(int, m.groups())
    return i / (b - a), t / (b - a)


def slopes(paths, render, reps, par):
    jobs = []
    for name, b, env, var, n in SCENES:
        for _ in range(reps):
            jobs.append((name, paths[("bin", b, "")][0], dict(env, RENDER=render, **{var: n})))
    if par:
        with ThreadPoolExecutor(JOBS) as ex:
            res = list(ex.map(lambda j: window(j[1], j[2]), jobs))
    else:
        res = [window(j[1], j[2]) for j in jobs]  # one at a time (parallel R1 runs contend for the GPU)
    out = {}
    for (name, _, _), r in zip(jobs, res):
        out.setdefault(name, []).append(r)
    cells = {}
    for name, b, env, var, n in SCENES:
        how = (f"{b} {' '.join(f'{k}={v}' for k, v in env.items())} RENDER={render} {var}={n}: one process per run, "
               f"instructions of the in-process window after warm-up (process-wide, final GPU sync included) / ticks; "
               f"median of {reps}{' run in parallel' if par else ', runs serial'}")
        cells[f"slope.{name}.r{render}"] = cell("instr", "instr/frame", [x[0] for x in out[name]], how)
        if render:
            cells[f"slope.{name}.r1.main"] = cell("instr", "instr/frame", [x[1] for x in out[name]],
                                                  how.replace("process-wide", "main thread only (thread_selfcounts)"))
    return cells


def redescribe(paths):
    exe = paths[("example", "redescribe_cost", "")][0]

    def one(_):
        p = subprocess.run([exe, "1000"], env=env_for({}), capture_output=True, text=True)
        row = [l for l in p.stdout.splitlines() if l.startswith("1000,")][0].split(",")
        return int(row[1]), int(row[3]), int(row[5])

    res = [one(k) for k in range(MOUNT_REPS)]  # serial (see mount)
    how = ("crates/bough/examples/redescribe_cost.rs 1000: a plain column of 1,000 rows, one plain field changed per "
           "App::turn, no frame; main-thread instructions per turn by slope (48 vs 16 turns); shape {}; 5 serial runs, median")
    return {f"redescribe.{s}.1000": cell("instr", "instr/turn", [r[i] for r in res], how.format(s))
            for i, s in enumerate(["owned", "static", "bare"])}


def mount(paths):
    exe = paths[("example", "quick_life", "")][0]
    jobs = [w for _ in range(MOUNT_REPS) for w in ("empty", "counter", "list1000")]

    def one(w):
        p = subprocess.run([exe, "mount", w], env=env_for({}), capture_output=True, text=True)
        m = dict(re.findall(r"(\w+)=(\d+)", p.stdout))
        return int(m["instr_new"]) + int(m["instr_frame"])

    res = [one(w) for w in jobs]  # serial: run in parallel, App::new's page faults made the count spread 9-14 %
    out = {}
    for w, r in zip(jobs, res):
        out.setdefault(w, []).append(r)
    what = {"empty": "a bare div (the fixed cost: runtime, region, font)", "counter": "examples/counter.rs",
            "list1000": "a column of 1,000 rows (box + two labels) through each(), not virtualised"}
    return {f"mount.{w}": cell("instr", "instr", out[w], f"quick_life mount {w}: {what[w]}; App::new(view, cfg, gpu=false) "
                               "+ the first frame's CPU work, process-wide instructions, a fresh process per run, serial; median of 5")
            for w in out}


def spawn_measured(argv, extra):
    """Run to exit; read the zombie's rusage (waitid WNOWAIT) before reaping. Returns (stdout, rusage, t_reaped)."""
    p = subprocess.Popen(argv, env=env_for(extra), stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
    out = p.stdout.read()
    si = ctypes.create_string_buffer(128)
    while libc.waitid(1, p.pid, si, 0x04 | 0x20) != 0:  # P_PID, WEXITED | WNOWAIT
        if ctypes.get_errno() != 4:
            raise OSError(ctypes.get_errno(), "waitid")
    t_seen = up_ns()
    r = _Rusage()
    if libc.proc_pid_rusage(p.pid, 4, ctypes.byref(r)) != 0:
        raise OSError(ctypes.get_errno(), "proc_pid_rusage on the exited child")
    p.wait()
    return out, list(r.v), t_seen


def life(paths, lock_info):
    apps = {"counter": ([paths[("example", "quick_life", "")][0], "counter"], {}),
            "heavy": ([paths[("bin", "heavy_life", "")][0]], {"ZGX_MODE": "both"})}
    runs = {a: [] for a in apps}
    first = {}
    for a, (argv, env) in apps.items():
        # one unmeasured launch first: a new binary's first launch also fills the Metal shader cache (no prebuilt
        # metallib without Xcode), which is a different event from the startup measured here; its time is kept
        s = up_ns()
        spawn_measured(argv, env)
        first[a] = round((up_ns() - s) / 1e6, 1)
    for k in range(LIFE_REPS):
        for a in (list(apps) if k % 2 == 0 else list(apps)[::-1]):  # interleaved
            argv, env = apps[a]
            out, v, t_seen = spawn_measured(argv, env)
            line = next((l for l in out.splitlines() if l.startswith("quick_life ")), "")
            m = {k2: int(x) for k2, x in re.findall(r"(\w+)=(\d+)", line)}
            if "up_first" not in m:
                sys.exit(f"quick: {argv}: no quick_life line\n{out}")
            start, exit_ = abs_ns(v[RI_START]), abs_ns(v[RI_EXIT])
            runs[a].append(dict(
                startup_ms=(m["up_first"] - start) / 1e6, premain_ms=(m["up_main"] - start) / 1e6,
                new_ms=(m["up_new"] - m["up_main"]) / 1e6, frame_ms=(m["up_first"] - m["up_new"]) / 1e6,
                startup_instr=m["instr_first"], premain_instr=m["instr_main"], new_instr=m["instr_new"] - m["instr_main"],
                frame_instr=m["instr_first"] - m["instr_new"],
                shutdown_ms=(t_seen - m["up_close"]) / 1e6, shutdown_kernel_ms=(exit_ - m["up_close"]) / 1e6,
                gpu_drop_us=(m["up_gpu_drop"] - m["up_close"]) / 1e3, app_drop_us=(m["up_app_drop"] - m["up_gpu_drop"]) / 1e3,
                exit_ms=(t_seen - m["up_app_drop"]) / 1e6,
                shutdown_instr=v[RI_INSTR] - m["instr_close"], gpu_drop_instr=m["instr_gpu_drop"] - m["instr_close"],
                app_drop_instr=m["instr_app_drop"] - m["instr_gpu_drop"], exit_instr=v[RI_INSTR] - m["instr_app_drop"],
                footprint_first=m["footprint_first"], rss_first=m["rss_first"], footprint_100=m["footprint_frames"],
                rss_100=m["rss_frames"], max_footprint=v[RI_MAXFP]))
    cells, details = {}, {}
    locked = lock_info.get("held", False)
    for a, rs in runs.items():
        col = lambda key: [r[key] for r in rs]
        src = "quick_life counter" if a == "counter" else "heavy_life (ZGX_MODE=both)"
        base = (f"{src}, headless, App::new(view, cfg, gpu=true) as Application::run builds it; {LIFE_REPS} cold processes, "
                "interleaved, after one unmeasured launch (the Metal shader cache warm)")
        tw = dict(lock=locked, load=lock_info.get("load"))
        cells[f"startup.{a}.wall"] = cell("time", "ms", col("startup_ms"), base + "; kernel process start (ri_proc_start_abstime) to the first frame complete on the GPU (CLOCK_UPTIME_RAW)", **tw)
        cells[f"startup.{a}.instr"] = cell("instr", "instr", col("startup_instr"), base + "; process-wide instructions at the first frame complete (dyld, runtime init, App::new, frame, sync)")
        cells[f"shutdown.{a}.wall"] = cell("time", "ms", col("shutdown_ms"), base + "; close request (after 100 frames) to the parent's waitid seeing the exit", **tw)
        cells[f"shutdown.{a}.instr"] = cell("instr", "instr", col("shutdown_instr"), base + "; the exited process's total instructions minus those at the close request")
        for key, label in (("footprint_first", "phys_footprint after the first frame"), ("rss_first", "resident size after the first frame"),
                           ("footprint_100", "phys_footprint after 100 more frames"), ("rss_100", "resident size after 100 more frames")):
            cells[f"memory.{a}.{key}"] = cell("mem", "MiB", [x / 1048576 for x in col(key)], base + f"; {label} (proc_pid_rusage)")
        details[a] = {k: noise(col(k)) for k in rs[0]}
        details[a]["warmup_launch_ms"] = first[a]
    return cells, details


def size():
    p = os.path.join(TARGET, "production", "examples", "counter")
    if not os.path.exists(p):
        return {}, {"skipped": f"{os.path.relpath(p, ROOT)} not built (cargo build --profile production -p bough --example counter)"}
    st = os.stat(p)
    age = time.time() - st.st_mtime
    return ({"size.counter.production": dict(kind="bytes", unit="B", value=st.st_size, noise=noise([st.st_size]),
                                             method="target/production/examples/counter, as found (not rebuilt by this run)",
                                             built=datetime.datetime.fromtimestamp(st.st_mtime).isoformat(timespec="seconds"),
                                             age_h=round(age / 3600, 1))}, {})


def smoke(paths):
    t0 = time.time()
    res = []
    for pkg, t, names in SMOKE:
        exe, cwd = paths[("test", t, pkg)]
        s = time.time()
        p = subprocess.run([exe, "--exact", *names, "--test-threads", "4"], cwd=cwd, env=env_for({}), capture_output=True, text=True)
        ok = p.returncode == 0 and re.search(r"test result: ok\. (\d+) passed", p.stdout)
        res.append(dict(pkg=pkg, test=t, names=names, ok=bool(ok), s=round(time.time() - s, 1),
                        tail=None if ok else (p.stdout + p.stderr)[-800:]))
    ok = all(r["ok"] for r in res)
    return {"smoke.pixels": dict(kind="gate", unit="", value=1 if ok else 0, noise=noise([1 if ok else 0]),
                                 method="release test binaries, run directly: " + "; ".join(f"{r['pkg']} --test {r['test']} {' '.join(r['names'])}" for r in res),
                                 seconds=round(time.time() - t0, 1), runs=res)}


# ------------------------------------------------------------------------------------------------ the lock
def lock_dir():
    if os.environ.get("BOUGH_MEASURE_LOCK"):
        return os.environ["BOUGH_MEASURE_LOCK"]
    common = git("rev-parse", "--path-format=absolute", "--git-common-dir")
    return os.path.join(os.path.dirname(common), "runtime", "measure.lock")


def take_lock(wait_s, lane):
    d = lock_dir()
    t0 = time.time()
    while True:
        try:
            os.mkdir(d)
            with open(os.path.join(d, "lane"), "w") as f:
                f.write(lane + "\n")
            return {"held": True, "path": d, "waited_s": round(time.time() - t0, 1)}
        except FileExistsError:
            try:
                owner = open(os.path.join(d, "lane")).read().strip()
                age = time.time() - os.stat(d).st_mtime
            except OSError:
                owner, age = "?", 0
            if age > 900:
                return {"held": False, "note": f"lock held by {owner} for {age / 60:.0f} min (stuck owner, ROUND6): measured without it"}
            if time.time() - t0 > wait_s:
                return {"held": False, "note": f"lock held by {owner} ({age:.0f} s old) after waiting {wait_s} s: measured without it"}
            time.sleep(2)


def drop_lock(info):
    if info.get("held"):
        try:
            os.remove(os.path.join(info["path"], "lane"))
            os.rmdir(info["path"])
        except OSError:
            pass


# ------------------------------------------------------------------------------------------------ comparison
def compare(cur, base):
    rows = []
    for name, c in cur["cells"].items():
        b = base["cells"].get(name) if base else None
        flag, ratio = "", None
        if b is not None and b["value"]:
            ratio = c["value"] / b["value"]
            k = c["kind"]
            if k in ("instr", "bytes"):
                if ratio > 1.02:
                    flag = "REGRESSED"
                elif ratio < 0.98:
                    flag = "better"
                bn = b["noise"]
                spread = (bn["max"] - bn["min"]) / bn["median"] if bn["median"] else 0
                if k == "instr" and spread > 0.02:
                    flag = (flag + " " if flag else "") + f"(base spread {100 * spread:.1f}%)"
            elif k in ("time", "mem"):
                if c["value"] > b["noise"]["max"]:
                    flag = "REGRESSED (above base max)"
                elif c["value"] < b["noise"]["min"]:
                    flag = "better (below base min)"
                if k == "time" and not (c.get("lock") and b.get("lock")):
                    flag = (flag + " " if flag else "") + "(unlocked)"
            elif k == "gate":
                flag = "FAILED" if not c["value"] else ""
        elif c["kind"] == "gate" and not c["value"]:
            flag = "FAILED"
        rows.append((name, c, b, ratio, flag))
    return rows


def fmt(v, unit):
    if unit in ("ms", "MiB"):
        return f"{v:,.1f}"
    if unit == "B":
        return f"{v:,.0f}"
    return f"{v:,.0f}"


def table(cur, rows, base):
    print(f"\nbough quick bench  commit {cur['commit'][:10]}{' (dirty)' if cur['dirty'] else ''}  profile {cur['profile']}  "
          f"load {cur['load']['start']} -> {cur['load']['end']}  elapsed {cur['elapsed_s']} s (build {cur['build_s']} s)")
    if cur.get("inject"):
        print(f"INJECTED: {cur['inject']}")
    if base:
        print(f"base: {base['commit'][:10]} ({base.get('date', '?')}), load {base['load']['start']}")
        if base.get("profile") != cur["profile"]:
            print(f"WARNING: base profile {base.get('profile')} != this run's {cur['profile']}: instruction cells are not comparable")
    print(f"{'cell':34s} {'value':>14s} {'unit':11s} {'noise min..max':>25s}" + (f" {'base':>14s} {'ratio':>7s}  flag" if base else ""))
    for name, c, b, ratio, flag in rows:
        n = c["noise"]
        nz = f"{fmt(n['min'], c['unit'])}..{fmt(n['max'], c['unit'])}" if n["n"] > 1 else ""
        line = f"{name:34s} {fmt(c['value'], c['unit']):>14s} {c['unit']:11s} {nz:>25s}"
        if base:
            line += f" {fmt(b['value'], b['unit']) if b else '-':>14s} {f'{ratio:.3f}' if ratio else '-':>7s}  {flag}"
        elif flag:
            line += f"  {flag}"
        print(line)
    if base:
        missing = [k for k in base["cells"] if k not in cur["cells"]]
        if missing:
            print("not measured in this run (present in base): " + ", ".join(missing))


# ------------------------------------------------------------------------------------------------ main
def main():
    global PROFILE
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--out")
    ap.add_argument("--base")
    ap.add_argument("--only", default="r0,r1,redescribe,mount,life,size,smoke")
    ap.add_argument("--no-build", action="store_true")
    ap.add_argument("--self-test", nargs="?", const="large", choices=["large", "small"])
    ap.add_argument("--lock-wait", type=int, default=int(os.environ.get("QUICK_LOCK_WAIT", "60")))
    ap.add_argument("--lane", default=os.environ.get("BOUGH_LANE", "iteration"))
    ap.add_argument("--profile", default=PROFILE, help="cargo profile of the drivers (release; gate: no LTO, a faster loop, its own baseline)")
    ap.add_argument("--sabotage", action="store_true", help="the smoke's negative control: build it with bough/sabotage_copy, expect it to fail")
    a = ap.parse_args()
    PROFILE = a.profile
    groups = set(a.only.split(","))
    if a.sabotage:
        groups = {"smoke"}
    inject = {}
    if a.self_test:
        # large: every injected cell must flag. small: ~3-4 K instructions per tick/turn/mount, near the 2 % floor of
        # the larger R0 cells (which cells flag shows the detection floor).
        inject = {"large": {"QUICK_INJECT": 1_000_000, "QUICK_INJECT_MS": 40}, "small": {"QUICK_INJECT": 1000}}[a.self_test]
        groups -= {"size", "smoke"}
        os.environ.update({k: str(v) for k, v in inject.items()})
    else:
        os.environ.pop("QUICK_INJECT", None)
        os.environ.pop("QUICK_INJECT_MS", None)
    if not os.path.exists(FONT):
        sys.exit(f"quick: font {FONT} missing (set BOUGH_REF or ZGX_FONT_FILE)")
    t0 = time.time()
    cur = {"schema": "bough-quick/1", "commit": git("rev-parse", "HEAD"), "branch": git("rev-parse", "--abbrev-ref", "HEAD"),
           "dirty": bool(git("status", "--porcelain", "--untracked-files=no")),
           "date": datetime.datetime.now().isoformat(timespec="seconds"),
           "host": f"{platform.node().split('.')[0]} {platform.machine()} {subprocess.run(['sysctl', '-n', 'machdep.cpu.brand_string'], capture_output=True, text=True).stdout.strip()} macOS {platform.mac_ver()[0]}",
           "toolchain": subprocess.run(["rustc", "-V"], capture_output=True, text=True, cwd=ROOT).stdout.strip(),
           "profile": PROFILE, "load": {"start": loadavg()}, "inject": inject, "cells": {}, "details": {}}
    # which source the numbers describe: the merge base with the round's base ref and the measured code changed since
    ref = os.environ.get("QUICK_BASE_REF", "lead/round6-base")
    mb = git("merge-base", "HEAD", ref)
    if mb:
        cur["merge_base"] = {"ref": ref, "commit": mb,
                             "changed": git("diff", "--name-only", mb, "--", "crates", "comparisons", "vendor").split()}
    paths = {}
    if a.no_build:
        find_paths(paths)
        cur["build_s"] = 0
    else:
        cur["build_s"] = build(paths, a.sabotage)
    timing = {}

    def group(name, f):
        if name in groups:
            s = time.time()
            r = f()
            timing[name] = round(time.time() - s, 1)
            return r
        return None

    # serial: parallel runs inflate the kernel's share of the count (page faults, measured: a 3 % R0 outlier)
    r = group("r0", lambda: slopes(paths, 0, R0_REPS, False))
    r and cur["cells"].update(r)
    r = group("r1", lambda: slopes(paths, 1, R1_REPS, False))
    r and cur["cells"].update(r)
    r = group("redescribe", lambda: redescribe(paths))
    r and cur["cells"].update(r)
    r = group("mount", lambda: mount(paths))
    r and cur["cells"].update(r)
    if "life" in groups:
        s = time.time()
        info = take_lock(a.lock_wait, a.lane)
        info["load"] = loadavg()
        try:
            c, d = life(paths, info)
        finally:
            drop_lock(info)
        cur["cells"].update(c)
        cur["details"]["life"] = d
        cur["lock"] = {k: v for k, v in info.items() if k != "path"}
        timing["life"] = round(time.time() - s, 1)
    if "size" in groups:
        c, d = size()
        cur["cells"].update(c)
        d and cur["details"].update(size=d)
    r = group("smoke", lambda: smoke(paths))
    if r and a.sabotage:
        c = r.pop("smoke.pixels")
        caught = [x["test"] + " " + " ".join(x["names"]) for x in c["runs"] if not x["ok"]]
        r["smoke.sabotage"] = dict(kind="gate", unit="", value=1 if caught else 0, noise=noise([1 if caught else 0]),
                                   method="the smoke built with --features bough/sabotage_copy (the scroll copy one pixel off) "
                                          "must fail; value 1 = it failed. Failing: " + ("; ".join(caught) or "none"),
                                   runs=c["runs"])
    r and cur["cells"].update(r)
    cur["load"]["end"] = loadavg()
    cur["group_s"] = timing
    cur["elapsed_s"] = round(time.time() - t0, 1)
    base = json.load(open(a.base)) if a.base else None
    rows = compare(cur, base)
    text = json.dumps(cur, indent=1)
    if a.out:
        with open(a.out, "w") as f:
            f.write(text + "\n")
    else:
        print(text)
    table(cur, rows, base)
    if "size" in cur["details"]:
        print("size: " + cur["details"]["size"]["skipped"])
    print("group seconds: " + ", ".join(f"{k} {v}" for k, v in timing.items()))
    bad = [n for n, c, b, ratio, flag in rows if flag.startswith(("REGRESSED", "FAILED"))]
    if a.self_test:
        injected = [n for n, c, b, ratio, flag in rows if b is not None and n.split(".")[0] in ("slope", "redescribe", "mount", "startup", "shutdown")]
        missed = [n for n in injected if n not in bad]
        print(f"self-test ({a.self_test}, {inject}): {len(injected) - len(missed)}/{len(injected)} injected cells flagged"
              + (f"; NOT flagged: {', '.join(missed)}" if missed else ""))
        return 1 if (a.self_test == "large" and missed) else 0
    if bad:
        print(f"quick: {len(bad)} cell(s) flagged: {', '.join(bad)}")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
