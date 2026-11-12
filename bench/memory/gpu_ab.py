#!/usr/bin/env python3
"""GPU time A/B of two bough builds (a kernel change's gate), interleaved pairs, at ten times the default frame
spread, median of PAIRS (5): per pair the GPU Kcycles per frame of A and B by the slope method (gputime.dylib from
bench/floor; FLOOR_TOOLS points at its directory), and B/A. A = $BOUGH_A, B = $BOUGH_B: directories holding
heavy_headless, lab_headless, animate_headless, vlist_headless (the same directory for both is fine); $A_ENV /
$B_ENV: `NAME=value` pairs (space separated) set for that side only, e.g. `BOUGH_METAL_SRC=<old shader>` to A/B a
kernel change on one binary. Hold the measurement lock while it runs.

Usage: gpu_ab.py <scene> [modes...]"""
import os, statistics, sys
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "floor"))
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "integrate"))
import gpu_slope
from slope import BIN

SPREAD = {"animate": (600, 6000), "vlist": (600, 6000)}


def one(dist, scene, mode, extra):
    old = BIN[("bough", scene)]
    BIN[("bough", scene)] = os.path.join(dist, os.path.basename(old))
    saved = {k: os.environ.get(k) for k in extra}
    os.environ.update(extra)
    try:
        a, b = SPREAD.get(scene, (300, 3000))
        (sa, ca), (sb, cb) = gpu_slope.gpu_s("bough", scene, mode, a), gpu_slope.gpu_s("bough", scene, mode, b)
    finally:
        BIN[("bough", scene)] = old
        for k, v in saved.items():
            if v is None:
                os.environ.pop(k, None)
            else:
                os.environ[k] = v
    return (sb - sa) / (b - a) * 1e3, (cb - ca) / (b - a) * 1e3


def main():
    scene = sys.argv[1]
    modes = sys.argv[2:] or ["-"]
    A, B = os.environ["BOUGH_A"], os.environ["BOUGH_B"]
    env = lambda v: dict(x.split("=", 1) for x in os.environ.get(v, "").split())
    EA, EB = env("A_ENV"), env("B_ENV")
    n = int(os.environ.get("PAIRS", "5"))
    for mode in modes:
        ratios = []
        for k in range(n):
            order = [("A", A, EA), ("B", B, EB)] if k % 2 == 0 else [("B", B, EB), ("A", A, EA)]
            r = {tag: one(d, scene, mode, e) for tag, d, e in order}
            ratio = r["B"][1] / r["A"][1] if r["A"][1] > 0 else float("nan")
            ratios.append(ratio)
            print(f"  {scene} {mode:6s} pair {k}: A {r['A'][1]:8.1f} Kc {r['A'][0]:6.3f} ms | B {r['B'][1]:8.1f} Kc {r['B'][0]:6.3f} ms | B/A {ratio:.3f}", flush=True)
        print(f"{scene} {mode:6s} median B/A {statistics.median(ratios):.3f}  spread [{min(ratios):.3f} .. {max(ratios):.3f}]", flush=True)


if __name__ == "__main__":
    main()
