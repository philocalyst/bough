#!/usr/bin/env python3
"""Whole-process instructions of a short run (secondary metric: cold start, which the size levers may slow down):
counter_gate (mount, glyph rasterisation, three clicks, four frames) and heavy_headless at TICKS=1, RENDER=1,
median of REPS (default 5), for each build directory given.
  python3 bench/size/startup.py <dir> [<dir>...]"""
import os, re, statistics, subprocess, sys
FONT = os.environ.get("ZGX_FONT_FILE") or os.path.expanduser("~/projects/_ref/zgx/comparisons/fonts/DejaVuSans.ttf")
def instr(path, env):
    p = subprocess.run(["/usr/bin/time", "-l", path], env=dict(os.environ, ZGX_FONT_FILE=FONT, **env), capture_output=True, text=True)
    return int(re.search(r"(\d+)\s+instructions retired", p.stderr).group(1))
reps = int(os.environ.get("REPS", 5))
rows = []
for d in sys.argv[1:]:
    c = statistics.median(instr(f"{d}/counter_gate", {}) for _ in range(reps))
    h = statistics.median(instr(f"{d}/heavy_headless", {"TICKS": "1", "RENDER": "1", "ZGX_MODE": "both"}) for _ in range(reps))
    rows.append((d, c, h))
b = rows[0]
for d, c, h in rows:
    print(f"{os.path.basename(d.rstrip('/')):24s} counter_gate {c/1e6:9.2f} M ({c/b[1]:.3f})   heavy_headless TICKS=1 {h/1e6:9.2f} M ({h/b[2]:.3f})")
