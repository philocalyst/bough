#!/usr/bin/env python3
"""to_dashboard.py RUN_DIR [MACREF_DIR] : a cross-framework run (bench/results/r7/guibench/<run>/: IDENTITY.txt,
metrics.tsv from flatten.py) into the dashboard's data.json as its "frameworks_scenes" section, then rebuild the page.
MACREF_DIR (bench/results/r7/workloads/macref-guibench) adds bough's Mac reference (Metal, bough's own harness).
"""
import csv
import json
import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
DATA = os.path.join(ROOT, "bench", "r6", "dashboard", "data.json")

FRAMEWORKS = ["bough", "egui", "iced", "gpui-kit"]
RENDERER = {
    "bough": "bough's CPU executor into wl_shm buffers, one thread",
    "egui": "eframe 0.37, wgpu on Mesa lavapipe (software Vulkan, many threads)",
    "iced": "iced 0.14, wgpu on Mesa lavapipe (software Vulkan, many threads)",
    "gpui-kit": "gpui-kit 0.9.1 (GPUI), wgpu on Mesa lavapipe (software Vulkan, many threads)",
}
# what the panels plot: (probe, metric, label)
METRICS = [
    ("hover", "latency.total_p50", "hover fan-out"),
    ("theme", "latency.total_p50", "theme switch"),
    ("edit", "latency.total_p50", "edit"),
    ("resize", "resize.configure_to_present_p50", "resize (configure to present)"),
    ("startup", "first_present_ms", "startup (spawn to first frame)"),
]
EXTRA = [
    ("hover", "latency.input_to_commit_p50", "hover: input to the app's commit"),
    ("theme", "latency.input_to_commit_p50", "theme: input to the app's commit"),
    ("edit", "latency.input_to_commit_p50", "edit: input to the app's commit"),
    ("idle", "resources.rss_max", "idle resident memory"),
    ("idle", "resources.cpu_ms_per_s", "idle CPU"),
    ("scroll", "scroll_smoothness.smoothness_score", "scroll smoothness score"),
]


def macref(d):
    """{(scene, size): {phase: (instr_p50, wall_us_p50)}} from the Mac reference's run.sh files."""
    out = {}
    if not d or not os.path.isdir(d):
        return out, ""
    ident = ""
    for f in sorted(os.listdir(d)):
        if not f.endswith(".tsv"):
            continue
        scene, scale = f[:-4].split("-")
        size = {"sheet": lambda s: str(round(10000 * float(s))), "gbcanvas": lambda s: str(round(5000 * float(s)))}[scene](scale)
        name = "canvas" if scene == "gbcanvas" else scene
        phases = {}
        lines = open(os.path.join(d, f)).read().splitlines()
        ident = " · ".join(l.lstrip("# ") for l in lines if l.startswith("# host") or l.startswith("# cpu") or l.startswith("# commit") or l.startswith("# env"))
        for l in lines:
            p = l.split("\t")
            if len(p) > 8 and p[0] == scene:
                phases[p[1]] = (int(p[3]), float(p[6]))
        out[f"{name}|{size}"] = phases
    return out, ident


def main():
    run = sys.argv[1]
    ref_dir = sys.argv[2] if len(sys.argv) > 2 else None
    identity = open(os.path.join(run, "IDENTITY.txt")).read().strip().splitlines()
    rows = list(csv.DictReader(open(os.path.join(run, "metrics.tsv")), delimiter="\t"))
    cells = {}
    for r in rows:
        if r["probe"] == "cell":
            continue
        key = f"{r['scene']}|{r['size']}"
        cells.setdefault(key, {}).setdefault(r["framework"], {})[f"{r['probe']}.{r['metric']}"] = {
            "value": float(r["value"]), "unit": r["unit"], "valid": r["valid"] == "True", "failed": r["failed_checks"]}
    ref, ref_ident = macref(ref_dir)
    with open(DATA, encoding="utf-8") as f:
        data = json.load(f)
    data["frameworks_scenes"] = {
        "title": "The heavy scenes in four frameworks",
        "identity": identity,
        "source": os.path.relpath(run, ROOT),
        "frameworks": FRAMEWORKS,
        "renderer": RENDERER,
        "metrics": [{"probe": p, "metric": m, "label": l} for p, m, l in METRICS],
        "extra": [{"probe": p, "metric": m, "label": l} for p, m, l in EXTRA],
        "cells": cells,
        "macref": ref,
        "macref_identity": ref_ident,
    }
    with open(DATA, "w", encoding="utf-8") as f:
        json.dump(data, f, indent=1, ensure_ascii=False)
        f.write("\n")
    subprocess.run([sys.executable, os.path.join(os.path.dirname(DATA), "build.py")], check=True)


if __name__ == "__main__":
    main()
