#!/usr/bin/env python3
"""areweactuallyguiyet's scroll and dashboard measurements, reproduced on macOS.

Same workloads, input schedule and window/statistics rules as the site's harness (apps/bench.py, scroll.py,
dashboard.py, frames.py); MangoHud's present hook is replaced by present_observer.dylib (DYLD_INSERT_LIBRARIES)
and xdotool by awagy_input (CGEventPostToPid). CPU and memory come from proc_pid_rusage.

  run.py <bins dir> <out.json> <app>... [--rows 1000 3000 5000 7000] [--reps 3] [--warmup 1] [--duration 3]
"""
import argparse, ctypes, json, mmap, os, statistics, struct, subprocess, sys, threading, time, math

TOOLS = os.path.dirname(os.path.abspath(__file__))
DYLIB = os.path.join(TOOLS, "present_observer.dylib")
INPUT = os.path.join(TOOLS, "awagy_input")
CAP = 1 << 20
FONT = os.path.expanduser("~/projects/_ref/zgx/comparisons/fonts/DejaVuSans.ttf")


class RusageInfoV4(ctypes.Structure):
    _fields_ = [("uuid", ctypes.c_uint8 * 16)] + [(n, ctypes.c_uint64) for n in (
        "user_time", "system_time", "pkg_idle_wkups", "interrupt_wkups", "pageins", "wired_size", "resident_size",
        "phys_footprint", "proc_start_abstime", "proc_exit_abstime", "child_user_time", "child_system_time",
        "child_pkg_idle_wkups", "child_interrupt_wkups", "child_pageins", "child_elapsed_abstime",
        "diskio_bytesread", "diskio_byteswritten", "cpu_time_qos_default", "cpu_time_qos_maintenance",
        "cpu_time_qos_background", "cpu_time_qos_utility", "cpu_time_qos_legacy", "cpu_time_qos_user_initiated",
        "cpu_time_qos_user_interactive", "billed_system_time", "serviced_system_time", "logical_writes",
        "lifetime_max_phys_footprint", "instructions", "cycles", "billed_energy", "serviced_energy",
        "interval_max_phys_footprint", "runnable_time")]


libproc = ctypes.CDLL("/usr/lib/libproc.dylib")
libc = ctypes.CDLL(None)
libc.mach_timebase_info.argtypes = [ctypes.c_void_p]


def timebase():
    tb = (ctypes.c_uint32 * 2)()
    libc.mach_timebase_info(tb)
    return tb[0] / tb[1]


TB = timebase()


def rusage(pid):
    r = RusageInfoV4()
    if libproc.proc_pid_rusage(pid, 4, ctypes.byref(r)) != 0:
        return None
    return {"cpu_ns": (r.user_time + r.system_time) * TB, "rss": r.resident_size, "footprint": r.phys_footprint,
            "instructions": r.instructions}


def now_ns():
    return time.clock_gettime_ns(time.CLOCK_UPTIME_RAW)


def read_presents(path):
    with open(path, "rb") as f:
        m = mmap.mmap(f.fileno(), 0, access=mmap.ACCESS_READ)
        n = min(struct.unpack_from("<Q", m, 0)[0], CAP)
        stamps = list(struct.unpack_from(f"<{n}Q", m, 8))
        tags = list(m[8 + CAP * 8: 8 + CAP * 8 + n])
        m.close()
    return sorted(zip(stamps, tags))


def summarize(frames, start_ns, end_ns):
    """apps/frames.py `summarize`: frames in (start, end], intervals from the previous frame (the anchor
    first), the trailing gap to the endpoint, ceil-indexed percentiles, max including the trailing gap."""
    sel = [t for t in frames if start_ns < t <= end_ns]
    prev, iv = start_ns, []
    for t in sel:
        iv.append((t - prev) / 1e6)
        prev = t
    trailing = (end_ns - prev) / 1e6
    iv.sort()
    pct = lambda q: iv[math.ceil(len(iv) * q) - 1] if iv else None
    return {"frames": len(sel), "fps": len(sel) * 1e9 / (end_ns - start_ns), "p95_ms": pct(0.95), "p99_ms": pct(0.99),
            "median_ms": statistics.median(iv) if iv else None, "max_gap_ms": max([trailing, *iv])}


def launch(binary, env_extra, log):
    env = dict(os.environ, AWAGY_PRESENT_LOG=log, DYLD_INSERT_LIBRARIES=DYLIB, ZGX_FONT_FILE=FONT, BOUGH_FONT=FONT, **env_extra)
    p = subprocess.Popen([binary], env=env, stdout=subprocess.DEVNULL, stderr=open(log + ".stderr", "w"))
    out = subprocess.run([INPUT, "window", str(p.pid)], capture_output=True, text=True, timeout=30).stdout.split()
    if len(out) != 4:
        p.kill()
        raise RuntimeError(f"{binary}: no window")
    x, y, w, h = map(float, out)
    # as the harness's xdo_activate_window, verified: the app is active and its window uncovered
    state = subprocess.run([INPUT, "activate", str(p.pid)], capture_output=True, text=True).stdout.strip()
    if state != "front":
        p.kill()
        raise RuntimeError(f"{os.path.basename(binary)}: window covered after activation")
    # the content area is the bottom 700 points of the window (the title bar is above it)
    return p, (x, y + h - 700.0, w, 700.0)


def front(pid):
    return subprocess.run([INPUT, "front", str(pid)], capture_output=True, text=True).stdout.strip() == "front"


SHOTS = None


def shot(rect, name):
    if SHOTS:
        x, y, w, h = rect
        subprocess.run(["screencapture", "-x", f"-R{x},{y},{w},{h}", os.path.join(SHOTS, name + ".png")])


def sampler(pid, stop, out):
    while not stop.is_set():
        r = rusage(pid)
        if r:
            out.append((now_ns(), r))
        time.sleep(0.05)


def run_scroll(binary, rows, warmup, duration, rate=144, tag=None):
    log = f"/tmp/awagy-presents-{os.getpid()}.bin"
    p, (x, y, w, h) = launch(binary, {"SCROLL_ROWS": str(rows)}, log)
    try:
        time.sleep(1.0)  # first frame and mount
        samples, stop = [], threading.Event()
        th = threading.Thread(target=sampler, args=(p.pid, stop, samples)); th.start()
        drv = subprocess.Popen([INPUT, "scroll", str(p.pid), str(x + w / 2), str(y + h / 2), str(rate), str(warmup + duration + 1.5)],
                               stdout=subprocess.PIPE, text=True)
        time.sleep(warmup + duration / 2)
        mid_front = front(p.pid)
        if tag:
            shot((x, y, w, h), tag)
        out = drv.communicate()[0]
        stop.set(); th.join()
        if not mid_front:
            raise RuntimeError("window covered during the measurement")
        drv = type("D", (), {"stdout": out})
        d = json.loads(drv.stdout)
        frames = [t for t, _ in read_presents(log)]
        warm_end = d["start_ns"] + int(warmup * 1e9)
        anchor = next((t for t in frames if t >= warm_end), None)
        if anchor is None or anchor + int(duration * 1e9) > d["end_ns"]:
            res = {"frames": 0, "fps": 0.0, "p95_ms": None, "p99_ms": None, "median_ms": None, "max_gap_ms": duration * 1e3, "note": "no bracketed window"}
            anchor = warm_end
        else:
            res = summarize(frames, anchor, anchor + int(duration * 1e9))
        end = anchor + int(duration * 1e9)
        inwin = [r for t, r in samples if anchor <= t <= end]
        if len(inwin) >= 2:
            res["cpu_ms"] = (inwin[-1]["cpu_ns"] - inwin[0]["cpu_ns"]) / 1e6 * duration * 1e9 / max(1, (end - anchor))
            res["rss_mib"] = inwin[-1]["rss"] / 2**20
            res["footprint_mib"] = inwin[-1]["footprint"] / 2**20
        res["events"] = d["events"]
        return res
    finally:
        p.kill(); p.wait()
        for f in (log, log + ".stderr"):
            try: os.remove(f)
            except OSError: pass


def run_dashboard(binary, warmup, rate=240, phase=2.0, tag=None):
    log = f"/tmp/awagy-presents-{os.getpid()}.bin"
    p, (x, y, w, h) = launch(binary, {}, log)
    try:
        time.sleep(1.0)
        samples, stop = [], threading.Event()
        th = threading.Thread(target=sampler, args=(p.pid, stop, samples)); th.start()
        drv = subprocess.Popen([INPUT, "dashboard", str(p.pid), str(x), str(y), str(w), str(h), str(rate), str(phase), str(warmup)],
                               stdout=subprocess.PIPE, text=True)
        time.sleep(warmup + 0.4 + 3.2 * phase)  # in the scroll phase
        mid_front = front(p.pid)
        if tag:
            shot((x, y, w, h), tag)
        out = drv.communicate()[0]
        time.sleep(0.3)  # capture past the endpoint
        stop.set(); th.join()
        if not mid_front:
            raise RuntimeError("window covered during the measurement")
        drv = type("D", (), {"stdout": out})
        d = json.loads(drv.stdout)
        frames = [t for t, _ in read_presents(log)]
        start, end = d["start_ns"], d["start_ns"] + int(4 * phase * 1e9)
        # the anchor: the first frame at or after the workload start (pre-workload idle excluded); the window
        # runs to the workload's end, its trailing silence included
        anchor = next((t for t in frames if t >= start), end)
        s = summarize(frames, anchor, end)
        inwin = [r for t, r in samples if start <= t <= end]
        per_phase = [sum(1 for t in frames if start + i * int(phase * 1e9) < t <= start + (i + 1) * int(phase * 1e9)) for i in range(4)]
        return {"worst_gap_ms": s["max_gap_ms"], "presents_per_phase": per_phase, "p95_ms": s["p95_ms"], "median_ms": s["median_ms"], "frames": s["frames"],
                "rss_mean_mib": statistics.mean(r["rss"] for r in inwin) / 2**20 if inwin else None,
                "footprint_mean_mib": statistics.mean(r["footprint"] for r in inwin) / 2**20 if inwin else None,
                "cpu_ms": (inwin[-1]["cpu_ns"] - inwin[0]["cpu_ns"]) / 1e6 if len(inwin) >= 2 else None,
                "phases": d["phases"]}
    finally:
        p.kill(); p.wait()
        for f in (log, log + ".stderr"):
            try: os.remove(f)
            except OSError: pass


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("bins"); ap.add_argument("out"); ap.add_argument("apps", nargs="+")
    ap.add_argument("--rows", type=int, nargs="+", default=[1000, 3000, 5000, 7000])
    ap.add_argument("--reps", type=int, default=3); ap.add_argument("--warmup", type=float, default=1.0)
    ap.add_argument("--duration", type=float, default=3.0); ap.add_argument("--dashboard-reps", type=int, default=1)
    ap.add_argument("--skip-dashboard", action="store_true"); ap.add_argument("--skip-scroll", action="store_true")
    ap.add_argument("--shots")
    a = ap.parse_args()
    global SHOTS
    SHOTS = a.shots
    if SHOTS:
        os.makedirs(SHOTS, exist_ok=True)
    results = json.load(open(a.out)) if os.path.exists(a.out) else {}
    for app in a.apps:
        r = results.setdefault(app, {})
        if not a.skip_scroll and os.path.exists(os.path.join(a.bins, f"{app}-scroll")):
            for rows in a.rows:
                reps = []
                for i in range(a.reps):
                    try:
                        reps.append(run_scroll(os.path.join(a.bins, f"{app}-scroll"), rows, a.warmup, a.duration, tag=f"{app}-scroll-{rows}" if i == 0 else None))
                    except Exception as e:
                        reps.append({"error": str(e)})
                    print(app, "scroll", rows, i, {k: (round(v, 2) if isinstance(v, float) else v) for k, v in reps[-1].items()}, flush=True)
                r[f"scroll-{rows}"] = reps
        if not a.skip_dashboard and os.path.exists(os.path.join(a.bins, f"{app}-dashboard")):
            reps = []
            for i in range(a.dashboard_reps):
                try:
                    reps.append(run_dashboard(os.path.join(a.bins, f"{app}-dashboard"), a.warmup, tag=f"{app}-dashboard" if i == 0 else None))
                except Exception as e:
                    reps.append({"error": str(e)})
                print(app, "dashboard", i, {k: (round(v, 2) if isinstance(v, float) else v) for k, v in reps[-1].items() if k != "phases"}, flush=True)
            r["dashboard"] = reps
        json.dump(results, open(a.out, "w"), indent=1)


if __name__ == "__main__":
    main()
