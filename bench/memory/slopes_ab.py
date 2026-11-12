#!/usr/bin/env python3
"""The instruction gate of a memory card: per-frame instruction slopes (bench/integrate/slope.py's method) of two
bough builds, interleaved per repetition, on the cells the round fixes (heavy stream, heavy scroll, lab both,
animate, vlist scroll) and a few more. A = $BOUGH_A, B = $BOUGH_B: directories holding heavy_headless, lab_headless,
animate_headless and vlist_headless (any profile, the same for both). RENDER=1 (the GPU path: the presenter's CPU
side included) unless $RENDERS says otherwise. Prints every repetition, the medians and B/A.

Usage: slopes_ab.py [reps=5]

The spread between the two frame counts is 10x (lane developer-loop measured the default 2x spread's own noise at
up to 14 % in an A-vs-A control, and 0.4-0.9 % at 10x, median of 5).
"""
import os, statistics, sys
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "integrate"))
import slope

CELLS = [("heavy", "stream"), ("heavy", "scroll"), ("lab", "both"), ("animate", "-"), ("vlist", "scroll"), ("heavy", "both"), ("vlist", "jump")]


def run(dist, scene, mode, render):
    old = slope.BIN[("bough", scene)]
    slope.BIN[("bough", scene)] = os.path.join(dist, os.path.basename(old))
    try:
        a, b = {"animate": (600, 6000), "vlist": (600, 6000)}.get(scene, (300, 3000))
        return (slope.instr("bough", scene, mode, render, b) - slope.instr("bough", scene, mode, render, a)) / (b - a)
    finally:
        slope.BIN[("bough", scene)] = old


def main():
    reps = int(sys.argv[1]) if len(sys.argv) > 1 else 5
    A, B = os.environ["BOUGH_A"], os.environ["BOUGH_B"]
    for render in (int(r) for r in os.environ.get("RENDERS", "1").split(",")):
        for scene, mode in CELLS:
            xa, xb = [], []
            for k in range(reps):
                order = [(A, xa), (B, xb)] if k % 2 == 0 else [(B, xb), (A, xa)]
                for dist, xs in order:
                    xs.append(run(dist, scene, mode, render))
            ma, mb = statistics.median(xa), statistics.median(xb)
            print(f"R{render} {scene:7s} {mode:6s} A {ma:10.0f} {[round(x) for x in xa]}  B {mb:10.0f} {[round(x) for x in xb]}  B/A {mb / ma if ma else float('nan'):.4f}", flush=True)


if __name__ == "__main__":
    main()
