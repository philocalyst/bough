#!/usr/bin/env python3
"""Cold first pixel, headless, A/B in alternating fresh processes: exec -> first frame complete on the GPU
(heavy dashboard, scale 2). Usage: first_pixel.py <binA> <binB> [n=7]. Prints per-run splits and medians."""
import os, re, statistics, subprocess, sys, time
FONT = os.path.expanduser(os.environ.get("BOUGH_REF", "~/projects/_ref")) + "/zgx/comparisons/fonts/DejaVuSans.ttf"
def one(b):
    t = time.time()
    out = subprocess.run([b], env=dict(os.environ, ZGX_FONT_FILE=FONT), capture_output=True, text=True).stdout
    m = re.search(r"app_new_ms=([0-9.]+) first_frame_ms=([0-9.]+) total_ms=([0-9.]+) epoch_us=(\d+)", out)
    return float(m.group(1)), float(m.group(2)), float(m.group(3)), int(m.group(4)) / 1e3 - t * 1e3
if __name__ == "__main__":
    a, b = sys.argv[1], sys.argv[2]
    n = int(sys.argv[3]) if len(sys.argv) > 3 else 7
    res = {a: [], b: []}
    for k in range(n):
        for x in ((a, b) if k % 2 == 0 else (b, a)):
            res[x].append(one(x))
    for x in (a, b):
        r = res[x]
        med = lambda i: statistics.median(v[i] for v in r)
        print(f"{x}: exec->first pixel {med(3):.1f} ms (main->first pixel {med(2):.1f}: App::new {med(0):.1f}, first frame {med(1):.1f}); runs {[round(v[3], 1) for v in r]}")
