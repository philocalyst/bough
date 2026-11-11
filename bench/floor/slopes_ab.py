#!/usr/bin/env python3
"""Instruction slopes (bench/integrate/slope.py method) for two bough builds side by side, interleaved per cell:
A = $BOUGH_A (direct Metal dist), B = $BOUGH_B (wgpu-hal dist). Every scene and mode, RENDER 0 and 1."""
import os, sys, statistics
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "integrate"))
import slope
A, B = os.environ["BOUGH_A"], os.environ["BOUGH_B"]
cells = [("heavy", m) for m in ("idle", "stream", "scroll", "both")] + [("lab", m) for m in ("idle", "stream", "scroll", "both")] + [("animate", "-")] + [("vlist", m) for m in ("scroll", "fling", "jump")]
def run(dist, scene, mode, render):
    old = slope.BIN[("bough", scene)]
    slope.BIN[("bough", scene)] = os.path.join(dist, os.path.basename(old))
    try:
        return slope.slope("bough", scene, mode, render)
    finally:
        slope.BIN[("bough", scene)] = old
for render in (int(r) for r in os.environ.get("RENDERS", "1,0").split(",")):
    for scene, mode in cells:
        a = run(A, scene, mode, render)
        b = run(B, scene, mode, render)
        print(f"R{render} {scene:7s} {mode:6s} direct {a[0]:10.0f} [{a[1]:.0f}..{a[2]:.0f}]   wgpu {b[0]:10.0f} [{b[1]:.0f}..{b[2]:.0f}]   wgpu/direct {b[0]/a[0] if a[0] else float('nan'):.3f}", flush=True)
