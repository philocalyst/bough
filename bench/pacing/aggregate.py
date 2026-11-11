#!/usr/bin/env python3
"""Aggregate a run.py batch: per build and workload window, the median over runs of each run's percentiles
(and the max of maxima), presents and refreshes per second, the gates G1 (presents <= refreshes, refreshes
with 2+ commits <= 1 %) and G2a (every posted event received), and the load averages.

  aggregate.py <batch dir> [--md]   (markdown tables with --md)
"""
import json, os, statistics, sys
from collections import defaultdict

METRICS = [("commit_interval_ms", "commit interval"), ("input_to_commit_ms", "input→commit"), ("input_to_vsync_after_commit_ms", "input→vsync after commit"),
           ("input_turn_ms", "input turn"), ("queue_ms", "queueing")]


def med(xs):
    xs = [x for x in xs if x is not None]
    return statistics.median(xs) if xs else None


def f(x, d=2):
    return "–" if x is None else f"{x:.{d}f}"


def main():
    d = sys.argv[1]
    md = "--md" in sys.argv
    b = json.load(open(os.path.join(d, "batch.json")))
    groups = defaultdict(list)  # (workload, label) -> runs
    for r in b["runs"]:
        if "summary" not in r:
            continue
        wl = r["name"].split("-")[0]
        groups[(wl, r["label"])].append(r)
    loads = [r["load_before"][0] for r in b["runs"]] + [r["load_after"][0] for r in b["runs"]]
    print(f"batch {d}: {len(b['runs'])} runs, {b['started']} .. {b.get('ended')}, 1-min load average min {min(loads):.1f} median {statistics.median(loads):.1f} max {max(loads):.1f}")
    for (wl, label), runs in sorted(groups.items()):
        wins = sorted(runs[0]["summary"]["windows"].keys(), key=lambda k: ["typing", "refresh", "toggle", "scroll", "all"].index(k) if k in ["typing", "refresh", "toggle", "scroll", "all"] else 9)
        g2 = all(r["summary"]["posted_by_kind"] == r["summary"]["received_by_kind"] for r in runs)
        rc = sorted({r["rc"] for r in runs})
        print(f"\n## {wl} / {label}: {len(runs)} runs, exit codes {rc}, every posted event received in every run: {g2}")
        if md:
            print("\n| window | presents/s | refreshes/s | 2+ commits / refreshes | worst gap ms (median, max) | " + " | ".join(f"{n} p50 / p95 / p99 / max" for _, n in METRICS) + " |")
            print("|---|---|---|---|---|" + "---|" * len(METRICS))
        for w in wins:
            ws = [r["summary"]["windows"][w] for r in runs if w in r["summary"]["windows"]]
            secs = None
            pps = med([x["presents_per_s"] for x in ws])
            # refreshes per second in the same window length
            rps = med([x["refreshes"] / (x["presents"] / x["presents_per_s"]) if x["presents"] else None for x in ws])
            dbl = sum(x["refreshes_with_2plus_commits"] for x in ws)
            refs = sum(x["refreshes"] for x in ws)
            g1 = all(x["presents"] <= x["refreshes"] for x in ws) and (refs == 0 or dbl / refs <= 0.01)
            gaps = [x["worst_gap_ms"] for x in ws]
            cells = []
            for k, _ in METRICS:
                ds = [x[k] for x in ws if x[k].get("n")]
                cells.append(" / ".join(f(med([dd[q] for dd in ds])) for q in ("p50", "p95", "p99")) + " / " + f(max([dd["max"] for dd in ds]) if ds else None))
            if md:
                print(f"| {w} | {f(pps,1)} | {f(rps,1)} | {dbl} / {refs} {'(G1 ok)' if g1 else '(G1 FAIL)'} | {f(med(gaps))}, {f(max(gaps))} | " + " | ".join(cells) + " |")
            else:
                print(f"  [{w}] presents/s {f(pps,1)} refreshes/s {f(rps,1)} 2+commit refreshes {dbl}/{refs} G1 {'ok' if g1 else 'FAIL'} worst gap median {f(med(gaps))} max {f(max(gaps))}")
                for (k, n), c in zip(METRICS, cells):
                    print(f"      {n:26s} {c}")
        idle = [i for r in runs for i in r["summary"].get("idle", [])]
        if idle:
            print("  idle windows:", json.dumps(idle))
    for r in b["runs"]:
        if r["name"].startswith("idle"):
            print("\n", r["name"], r["stderr"].strip().splitlines()[-1:] if r["stderr"] else "")


if __name__ == "__main__":
    main()
