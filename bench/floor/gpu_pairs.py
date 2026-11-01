#!/usr/bin/env python3
"""GPU time per frame, A/B in PAIRS inside one run (round 3c measurement protocol).

The GPU clock under other agents' load swings 338-1,143 MHz between runs, so separate runs of two frameworks are
not comparable. This script interleaves them: for each pair k it measures A's slope and B's slope back to back
(order alternating A,B / B,A to cancel drift), each slope = (GPU at TICKS=b - GPU at TICKS=a) / (b - a) from
gputime.dylib (I/O Registry per-client GPU time, mean active MHz of the run). It prints every pair's Kcycles and
GPU ms for both sides, the pair's ratio B/A, then the median ratio and the pair spread [min .. max].
A cell is a win for B only when the whole spread is below 1 (and for A when it is above 1).

Usage: gpu_pairs.py <scene> [modes...]   (scene: heavy | lab | animate | vlist)
Env:   A=zgx B=bough (default); a side may be `bough=<dist dir>` to A/B two bough builds; PAIRS (5).
"""
import os, statistics, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import gpu_slope
from slope import BIN

def side_bin(spec, scene):
    if "=" in spec:
        fw, dist = spec.split("=", 1)
        return fw, os.path.join(dist, os.path.basename(BIN[(fw, scene)]))
    return spec, BIN[(spec, scene)]

def one(spec, scene, mode):
    """(ms/frame, Kcycles/frame, mean MHz) of one slope."""
    fw, path = side_bin(spec, scene)
    BIN[(fw, scene)], old = path, BIN[(fw, scene)]
    try:
        a, b = {"animate": (600, 1200), "vlist": (600, 3600)}.get(scene, (300, 600))
        (sa, ca), (sb, cb) = gpu_slope.gpu_s(fw, scene, mode, a), gpu_slope.gpu_s(fw, scene, mode, b)
    finally:
        BIN[(fw, scene)] = old
    ms = (sb - sa) / (b - a) * 1e3
    kc = (cb - ca) / (b - a) * 1e3
    mhz = (cb - ca) / (sb - sa) if sb > sa else 0.0
    return ms, kc, mhz

if __name__ == "__main__":
    scene = sys.argv[1]
    modes = sys.argv[2:] or {"animate": ["-"], "vlist": ["scroll", "fling", "jump"]}.get(scene, ["stream", "scroll", "both"])
    A, B = os.environ.get("A", "zgx"), os.environ.get("B", "bough")
    n = int(os.environ.get("PAIRS", "5"))
    for mode in modes:
        ratios = []
        for k in range(n):
            order = (A, B) if k % 2 == 0 else (B, A)
            r = {s: one(s, scene, mode) for s in order}
            (ma, ka, fa), (mb, kb, fb) = r[A], r[B]
            ratio = kb / ka if ka > 0 else float("nan")
            ratios.append(ratio)
            print(f"  {scene} {mode:6s} pair {k}: A {ka:7.1f} Kc {ma:6.3f} ms @{fa:5.0f} MHz | B {kb:7.1f} Kc {mb:6.3f} ms @{fb:5.0f} MHz | B/A {ratio:5.2f}", flush=True)
        med = statistics.median(ratios)
        verdict = "B wins" if max(ratios) < 1 else ("A wins" if min(ratios) > 1 else "not clear")
        print(f"{scene} {mode:6s} A={A} B={B}  median B/A {med:5.2f}  spread [{min(ratios):.2f} .. {max(ratios):.2f}]  {verdict}", flush=True)
