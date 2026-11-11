#!/usr/bin/env python3
"""Per-frame instructions by the slope method (zgx's headless method): run TICKS=A and TICKS=B (animate: FRAMES),
(instructions retired B - A) / (B - A), via `/usr/bin/time -l` ("instructions retired", process wide),
median of REPS repetitions. Usage: slope.py <bough|zgx> <RENDER 0|1> [scene=heavy|lab|animate] [modes...]
(animate has no modes; its slope is 600 vs 1200 frames; vlist: scroll fling jump, 600 vs 3600 ticks)."""
import os, re, statistics, subprocess, sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
# bough's dist binaries (built with CARGO_TARGET_DIR=target/fusion) and zgx's headless drivers (bench/zgx/build.sh)
BOUGH = os.environ.get("BOUGH_DIST", f"{ROOT}/target/fusion/dist")
ZGXH = os.environ.get("ZGX_HEADLESS", f"{ROOT}/target/zgx-headless")
ZGXBIN = os.environ.get("ZGX_DIST", os.path.expanduser(os.environ.get('BOUGH_REF', '~/projects/_ref')) + "/zgx/target/dist/examples")
BIN = {("bough", "heavy"): f"{BOUGH}/heavy_headless", ("zgx", "heavy"): f"{ZGXBIN}/heavy_headless",
       ("bough", "lab"): f"{BOUGH}/lab_headless", ("zgx", "lab"): f"{ZGXH}/workload_headless",
       ("bough", "animate"): f"{BOUGH}/animate_headless", ("zgx", "animate"): f"{ZGXH}/animation_headless",
       ("bough", "vlist"): f"{BOUGH}/vlist_headless", ("zgx", "vlist"): f"{ZGXH}/vlist_headless"}
FONT = os.path.expanduser(os.environ.get('BOUGH_REF', '~/projects/_ref')) + "/zgx/comparisons/fonts/DejaVuSans.ttf"

def instr(fw, scene, mode, render, n):
    env = dict(os.environ, ZGX_MODE=mode, RENDER=str(render), ZGX_FONT_FILE=FONT)
    env["FRAMES" if scene == "animate" else "TICKS"] = str(n)
    p = subprocess.run(["/usr/bin/time", "-l", BIN[(fw, scene)]], env=env, capture_output=True, text=True)
    m = re.search(r"(\d+)\s+instructions retired", p.stderr)
    return int(m.group(1))

def slope(fw, scene, mode, render, reps=3):
    a, b = {"animate": (600, 1200), "vlist": (600, 3600)}.get(scene, (300, 600))
    xs = [(instr(fw, scene, mode, render, b) - instr(fw, scene, mode, render, a)) / (b - a) for _ in range(reps)]
    return statistics.median(xs), min(xs), max(xs)

if __name__ == "__main__":
    fw, render = sys.argv[1], int(sys.argv[2])
    scene = sys.argv[3] if len(sys.argv) > 3 else "heavy"
    modes = sys.argv[4:] or {"animate": ["-"], "vlist": ["scroll", "fling", "jump"]}.get(scene, ["idle", "stream", "scroll", "both"])
    for mode in modes:
        med, lo, hi = slope(fw, scene, mode, render)
        print(f"{fw} {scene} RENDER={render} {mode:7s} {med:12.0f} instr/frame  [{lo:.0f} .. {hi:.0f}]", flush=True)
