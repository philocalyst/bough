#!/usr/bin/env python3
"""The size lane's instruction gate (G3): the hot cells of two bough builds, interleaved per cell, by the slope
method of bench/integrate/slope.py and bench/layout/slope.py (TICKS 300 vs 600, animate FRAMES 600 vs 1200, vlist
TICKS 600 vs 3600; process-wide "instructions retired" from `/usr/bin/time -l`).

  BOUGH_A=<dir of the base build's headless binaries> BOUGH_B=<dir of the candidate's> python3 bench/size/hot_ab.py

LONG=k multiplies the tick spread (B - A) by k: at the scripts' own spread, an A-vs-A run of one build measured
up to +-14 % on the small cells (startup variance divided by only 300 ticks), too coarse for a 1.02 gate.
Each directory holds heavy_headless, lab_headless, animate_headless and vlist_headless. Env: RENDERS (default
"0,1"), REPS (3; vlist 5 as layout/slope.py), CELLS (default the five gate cells). Prints one line per cell with
both medians, their ranges and B/A; the last line is the worst ratio. ZGX_FONT_FILE as the slope scripts."""
import os, re, statistics, subprocess, sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
FONT = os.environ.get("ZGX_FONT_FILE") or os.path.expanduser(os.environ.get("BOUGH_REF", "~/projects/_ref")) + "/zgx/comparisons/fonts/DejaVuSans.ttf"
BIN = {"heavy": "heavy_headless", "lab": "lab_headless", "animate": "animate_headless", "vlist": "vlist_headless"}
GATE = [("heavy", "stream"), ("heavy", "scroll"), ("lab", "both"), ("animate", "-"), ("vlist", "scroll")]


def instr(d, scene, mode, render, n):
    env = dict(os.environ, ZGX_MODE=mode, RENDER=str(render), ZGX_FONT_FILE=FONT)
    env["FRAMES" if scene == "animate" else "TICKS"] = str(n)
    p = subprocess.run(["/usr/bin/time", "-l", os.path.join(d, BIN[scene])], env=env, capture_output=True, text=True)
    m = re.search(r"(\d+)\s+instructions retired", p.stderr)
    if p.returncode != 0 or not m:
        sys.exit(f"{d}/{BIN[scene]} {mode} R{render} failed ({p.returncode}): {p.stderr[-400:]}")
    return int(m.group(1))


def main():
    a_dir, b_dir = os.environ["BOUGH_A"], os.environ["BOUGH_B"]
    cells = [tuple(c.split(":")) for c in os.environ["CELLS"].split(",")] if os.environ.get("CELLS") else GATE
    worst = 0.0
    for render in (int(r) for r in os.environ.get("RENDERS", "0,1").split(",")):
        for scene, mode in cells:
            a, b = {"animate": (600, 1200), "vlist": (600, 3600)}.get(scene, (300, 600))
            b = a + int(float(os.environ.get("LONG", "1")) * (b - a))  # LONG=k: k times the scripts' spread
            reps = int(os.environ.get("REPS", 5 if scene == "vlist" else 3))
            xa, xb = [], []
            for _ in range(reps):  # A and B interleaved within the cell
                xa.append((instr(a_dir, scene, mode, render, b) - instr(a_dir, scene, mode, render, a)) / (b - a))
                xb.append((instr(b_dir, scene, mode, render, b) - instr(b_dir, scene, mode, render, a)) / (b - a))
            ma, mb = statistics.median(xa), statistics.median(xb)
            r = mb / ma if ma else float("nan")
            rmin = min(xb) / min(xa) if min(xa) else float("nan")
            worst = max(worst, r)
            print(f"R{render} {scene:7s} {mode:6s} A {ma:10.0f} [{min(xa):.0f}..{max(xa):.0f}]  B {mb:10.0f} [{min(xb):.0f}..{max(xb):.0f}]  B/A {r:.4f}  min/min {rmin:.4f}", flush=True)
    print(f"worst B/A {worst:.4f}")


if __name__ == "__main__":
    main()
