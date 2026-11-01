#!/usr/bin/env python3
"""micro_bench (zgx_workload::micro semantics) for bough and zgx, N repeats each, same env for both
(ZGX_FONT_FILE = DejaVu): prints the median-of-repeats of the median / p10 / p90 CPU ns per op
(and the [min-max] over repeats) for write and frame. Usage: micro.py [reps=3] [scenes...]"""
import json, os, statistics, subprocess, sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
BIN = {"bough": f"{ROOT}/target/dist/micro_bench",
       "zgx": "/Users/mileswirht/projects/_ref/zgx/target/dist/examples/micro_bench"}
FONT = "/Users/mileswirht/projects/_ref/zgx/comparisons/fonts/DejaVuSans.ttf"

def run(fw, scene, render):
    env = dict(os.environ, SCENE=scene, RENDER=str(render), ZGX_FONT_FILE=FONT)
    out = subprocess.run([BIN[fw]], env=env, capture_output=True, text=True).stdout
    return {j["figure"]: j for j in (json.loads(l) for l in out.splitlines() if l.startswith("{"))}

def fmt(xs):
    return f"{statistics.median(xs):9,.0f} [{min(xs):,.0f}-{max(xs):,.0f}]"

if __name__ == "__main__":
    reps = int(sys.argv[1]) if len(sys.argv) > 1 else 3
    scenes = sys.argv[2:] or ["state", "vscroll-fixed"]
    for scene in scenes:
        for render in (0, 1):
            for fw in ("bough", "zgx"):
                rs = [run(fw, scene, render) for _ in range(reps)]
                for fig in ("write", "frame"):
                    print(f"{fw:5s} {scene:14s} RENDER={render} {fig:5s} median {fmt([r[fig]['median_ns'] for r in rs])}  p10 {fmt([r[fig]['p10_ns'] for r in rs])}  p90 {fmt([r[fig]['p90_ns'] for r in rs])}", flush=True)
