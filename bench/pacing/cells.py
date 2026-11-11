#!/usr/bin/env python3
"""Gate G5 of the X4 card: the per-frame instruction cells (bench/integrate/slope.py's slope method, process
instructions retired, median of 3) for two dist builds, interleaved per cell: A = $BOUGH_A, B = $BOUGH_B
(directories holding heavy_headless, lab_headless, animate_headless, vlist_headless).

  BOUGH_A=<dir> BOUGH_B=<dir> cells.py [RENDERS=1,0]
"""
import os, sys
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "integrate"))
import slope  # noqa: E402

A, B = os.environ["BOUGH_A"], os.environ["BOUGH_B"]
CELLS = [("heavy", "stream"), ("heavy", "scroll"), ("lab", "both"), ("animate", "-"), ("vlist", "scroll")]


def run(dist, scene, mode, render):
    old = slope.BIN[("bough", scene)]
    slope.BIN[("bough", scene)] = os.path.join(dist, os.path.basename(old))
    try:
        return slope.slope("bough", scene, mode, render)
    finally:
        slope.BIN[("bough", scene)] = old


worst = 0.0
for render in (int(r) for r in os.environ.get("RENDERS", "1,0").split(",")):
    for scene, mode in CELLS:
        a = run(A, scene, mode, render)
        b = run(B, scene, mode, render)
        ratio = b[0] / a[0] if a[0] else float("nan")
        worst = max(worst, ratio)
        print(f"R{render} {scene:7s} {mode:6s} A {a[0]:10.0f} [{a[1]:.0f}..{a[2]:.0f}]   B {b[0]:10.0f} [{b[1]:.0f}..{b[2]:.0f}]   B/A {ratio:.4f}", flush=True)
print(f"worst B/A {worst:.4f}: G5 {'ok' if worst <= 1.02 else 'FAIL'} (threshold 1.02)")
