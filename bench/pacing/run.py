#!/usr/bin/env python3
"""The pacing lane's measurement batch: pace_probe builds run interleaved (ABAB...) under the round's
measurement lock, the load average recorded before and after every run, every frame log kept and summarised.

  run.py <out dir> <label>=<pace_probe binary>[:ENV=V,ENV=V] ... [--dash N] [--scroll N] [--rows 1000 3000 5000 7000]
         [--idle SECS] [--tail SECS] [--lock DIR]

The lock (default $BOUGH_MEASURE_LOCK) is a directory: created with this lane's name inside, removed at the end;
if it exists the batch waits. Fonts: BOUGH_FONT (default: the zgx reference's DejaVuSans, as AWAGY's runs).
"""
import argparse, json, os, subprocess, sys, time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import framelog  # noqa: E402

FONT = os.environ.get("BOUGH_FONT", os.path.expanduser(os.environ.get("BOUGH_REF", "~/projects/_ref")) + "/zgx/comparisons/fonts/DejaVuSans.ttf")


def load():
    return os.getloadavg()


def take_lock(lock):
    while True:
        try:
            os.mkdir(lock)
            open(os.path.join(lock, "lane"), "w").write("pacing\n")
            return
        except FileExistsError:
            who = open(os.path.join(lock, "lane")).read().strip() if os.path.exists(os.path.join(lock, "lane")) else "?"
            print(f"lock held by {who}; waiting", flush=True)
            time.sleep(30)


def drop_lock(lock):
    try:
        os.remove(os.path.join(lock, "lane"))
    except OSError:
        pass
    try:
        os.rmdir(lock)
    except OSError:
        pass


def run(binary, env_extra, args, log, timeout):
    env = dict(os.environ, BOUGH_FONT=FONT, BOUGH_FRAMELOG=log, BOUGH_CHECK="1", **env_extra)
    l0 = load()
    t0 = time.time()
    p = subprocess.run([binary, *args], env=env, capture_output=True, text=True, timeout=timeout)
    return {"args": args, "env": env_extra, "rc": p.returncode, "secs": time.time() - t0, "load_before": l0, "load_after": load(), "stderr": p.stderr[-2000:]}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("out")
    ap.add_argument("builds", nargs="+")
    ap.add_argument("--dash", type=int, default=10)
    ap.add_argument("--scroll", type=int, default=3)
    ap.add_argument("--rows", type=int, nargs="+", default=[1000, 3000, 5000, 7000])
    ap.add_argument("--hover", type=int, default=0)
    ap.add_argument("--idle", type=float, default=0)
    ap.add_argument("--tail", type=float, default=0)
    ap.add_argument("--lock", default=os.environ.get("BOUGH_MEASURE_LOCK"))
    a = ap.parse_args()
    os.makedirs(a.out, exist_ok=True)
    builds = []
    for b in a.builds:
        label, rest = b.split("=", 1)
        binary, _, envs = rest.partition(":")
        env = dict(kv.split("=", 1) for kv in envs.split(",") if kv)
        builds.append((label, binary, env))
    if a.lock:
        take_lock(a.lock)
    results = {"started": time.strftime("%Y-%m-%d %H:%M:%S"), "load_start": load(), "runs": []}
    save = lambda: json.dump(results, open(os.path.join(a.out, "batch.json"), "w"), indent=1)
    try:
        jobs = []
        for i in range(a.dash):
            for label, binary, env in builds:
                jobs.append((label, binary, env, ["dashboard", "240", "2", "1", str(a.tail)], f"dash-{label}-{i}", "dashboard"))
        for rows in a.rows:
            for i in range(a.scroll):
                for label, binary, env in builds:
                    jobs.append((label, binary, env, ["scroll", str(rows), "144", "5.5", "0"], f"scroll{rows}-{label}-{i}", "scroll"))
        for i in range(a.hover):
            for label, binary, env in builds:
                jobs.append((label, binary, env, ["hover", "240", "4"], f"hover-{label}-{i}", "hover"))
        if a.idle:
            for label, binary, env in builds:
                jobs.append((label, binary, env, ["idle", str(a.idle)], f"idle-{label}", "idle"))
        for label, binary, env, args, name, scene in jobs:
            log = os.path.join(a.out, name + ".log")
            r = run(binary, env, args, log, timeout=120)
            r.update(label=label, name=name)
            if scene and os.path.exists(log):
                try:
                    r["summary"] = framelog.analyse(framelog.load(log), scene, 5)
                except Exception as e:  # keep the run, record why it could not be summarised
                    r["summary_error"] = repr(e)
            results["runs"].append(r)
            w = r.get("summary", {}).get("windows", {})
            brief = {k: (round(v["presents_per_s"], 1), round(v["worst_gap_ms"], 2), v["input_to_commit_ms"].get("p99")) for k, v in w.items()}
            print(name, "rc", r["rc"], "load", round(r["load_before"][0], 1), brief, flush=True)
            save()
    finally:
        results["load_end"] = load()
        results["ended"] = time.strftime("%Y-%m-%d %H:%M:%S")
        save()
        if a.lock:
            drop_lock(a.lock)


if __name__ == "__main__":
    main()
