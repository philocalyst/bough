#!/usr/bin/env python3
"""Per-frame instructions by the slope method for the vlist scene (zgx's heavy_headless method): run TICKS=A
and TICKS=B, (instructions retired B - A) / (B - A) via `/usr/bin/time -l` ("instructions retired",
process wide), median of REPS. zgx has no headless vlist driver upstream: its side is `vlist_headless`
(the windowed vlist_workload's loop, built in a copy of the zgx tree, same sources/profile as the measured
06debe9d binaries). Usage: slope.py <bough|bough-deep|zgx> <RENDER 0|1> [modes...]"""
import os, re, statistics, subprocess, sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
SCRATCH = os.environ.get("ZGX_COPY", "/private/tmp/claude-501/-Users-mileswirht-Downloads-designs-reactivity/16fd4c0d-6057-492e-b9c5-0daf75a4051a/scratchpad/zgx-copy")
BIN = {"bough": f"{ROOT}/target/dist/vlist_headless",
       "bough-deep": f"{ROOT}/target/dist-deep/vlist_headless",
       "zgx": f"{SCRATCH}/target/dist/examples/vlist_headless"}
FONT = "/Users/mileswirht/projects/_ref/zgx/comparisons/fonts/DejaVuSans.ttf"

def instr(fw, mode, render, ticks):
    env = dict(os.environ, ZGX_MODE=mode, RENDER=str(render), TICKS=str(ticks), ZGX_FONT_FILE=FONT)
    p = subprocess.run(["/usr/bin/time", "-l", BIN[fw]], env=env, capture_output=True, text=True)
    m = re.search(r"(\d+)\s+instructions retired", p.stderr)
    return int(m.group(1))

def slope(fw, mode, render, a=300, b=600, reps=3):
    xs = [(instr(fw, mode, render, b) - instr(fw, mode, render, a)) / (b - a) for _ in range(reps)]
    return statistics.median(xs), min(xs), max(xs)

if __name__ == "__main__":
    fw, render = sys.argv[1], int(sys.argv[2])
    modes = sys.argv[3:] or ["scroll", "fling", "jump"]
    for mode in modes:
        med, lo, hi = slope(fw, mode, render)
        print(f"{fw} RENDER={render} {mode:7s} {med:12.0f} instr/frame  [{lo:.0f} .. {hi:.0f}]", flush=True)
