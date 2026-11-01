#!/usr/bin/env python3
"""GPU time per frame, headless (RENDER=1), either framework, by the slope method: the process's GPU time
(I/O Registry, per GPU client: gputime.dylib injected with DYLD_INSERT_LIBRARIES; no root) at TICKS=A and
TICKS=B (animate: FRAMES 600/1200; vlist: 600/3600), (B - A) / (B - A) frames, median of REPS (3).
Back-to-back frames run at whatever clock the GPU picks under load (DVFS), so compare columns of one run.
Usage: gpu_slope.py <bough|zgx> <heavy|lab|animate|vlist> [modes...]. Never read instruction counts from
these runs (the injected poller costs CPU)."""
import os, re, statistics, subprocess, sys
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "integrate"))
from slope import BIN, FONT  # same binaries as the instruction slopes

TOOLS = os.environ.get("FLOOR_TOOLS", os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "..", "bough", "target", "floor-tools"))
DYLIB = os.path.join(TOOLS, "gputime.dylib")

def gpu_s(fw, scene, mode, n):
    env = dict(os.environ, ZGX_MODE=mode, RENDER="1", ZGX_FONT_FILE=FONT, DYLD_INSERT_LIBRARIES=DYLIB)
    env["FRAMES" if scene == "animate" else "TICKS"] = str(n)
    p = subprocess.run([BIN[(fw, scene)]], env=env, capture_output=True, text=True)
    m = re.search(r"gpu_seconds=([0-9.]+)", p.stderr)
    return float(m.group(1))

def slope(fw, scene, mode, reps):
    a, b = {"animate": (600, 1200), "vlist": (600, 3600)}.get(scene, (300, 600))
    xs = [(gpu_s(fw, scene, mode, b) - gpu_s(fw, scene, mode, a)) / (b - a) * 1e3 for _ in range(reps)]
    return statistics.median(xs), min(xs), max(xs)

if __name__ == "__main__":
    fw, scene = sys.argv[1], sys.argv[2]
    modes = sys.argv[3:] or {"animate": ["-"], "vlist": ["scroll", "fling", "jump"]}.get(scene, ["stream", "scroll", "both"])
    reps = int(os.environ.get("REPS", "3"))
    for mode in modes:
        med, lo, hi = slope(fw, scene, mode, reps)
        print(f"{fw} {scene} {mode:7s} GPU {med:7.3f} ms/frame  [{lo:.3f} .. {hi:.3f}]", flush=True)
