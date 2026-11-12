#!/usr/bin/env python3
"""Classify every commit gap longer than a threshold (default 16.7 ms: two refreshes) in frame logs, by the
mechanism that filled it (the F2 trace):

* gpu-sync: a turn waited for the GPU's previous frame before writing the region (note gpu-wait)
* ring: a frame waited for a free surface (note ring-wait)
* drain: a residency drain (a surface's first use, after a trim)
* main-busy: a turn ran long for another reason (its own phases)
* main-late: events waited in the queue while no turn ran (the main thread was not scheduled)
* publisher: the publisher took long to wake for a posted frame
* gpu: the GPU took long to finish a posted frame
* no-input: nothing was posted (the workload paused)

  gaps.py <log>... [--min-ms 16.7] [--window dashboard|scroll|all]
"""
import argparse, sys, os
from collections import Counter
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import framelog as F


def classify(rows, min_ms):
    turns = sorted((r for r in rows if r[0] == F.TURN), key=lambda r: r[F.M_DISPATCH])
    pubs = [r for r in rows if r[0] == F.PUB]
    notes = [r for r in rows if r[0] == F.NOTE]
    events = [r for r in rows if r[0] == F.EVENT]
    ph = sorted((r[3], r[4]) for r in notes if r[1] == 10)
    if len(ph) < 2:
        return []
    a, b = ph[0][1], ph[-1][1]
    c0 = sorted(r[5] for r in pubs if r[7] == 1 and a <= r[5] <= b)
    out = []
    for i in range(len(c0) - 1):
        g = (c0[i + 1] - c0[i]) / 1e6
        if g < min_ms:
            continue
        lo, hi = c0[i], c0[i + 1]
        causes = Counter()
        for n in notes:
            if lo <= n[2] <= hi:
                if n[1] == 12:
                    causes["gpu-sync"] += n[3] / 1e3
                elif n[1] == 2:
                    causes["ring"] = max(causes["ring"], n[3] / 1e3)
                elif n[1] == 13:
                    causes["drain"] += n[3] / 1e3
        inside = [t for t in turns if lo <= t[F.M_DISPATCH] <= hi]
        if inside:
            longest = max(inside, key=lambda t: t[F.M_END] - t[F.M_DISPATCH])
            causes["main-busy"] = (longest[F.M_END] - longest[F.M_DISPATCH]) / 1e6
        evs = [e for e in events if lo <= e[3] <= hi]
        qd = [(t[F.M_DISPATCH] - t[F.M_EVENT]) / 1e6 for t in inside if t[F.M_EVENT]]
        if qd:
            causes["main-late"] = max(qd)
        pp = [r for r in pubs if lo <= r[3] <= hi]
        if pp:
            causes["publisher"] = max((r[3] - r[2]) / 1e6 for r in pp if r[2])
            causes["gpu"] = max((r[4] - r[3]) / 1e6 for r in pp)
        posted = [n for n in notes if n[1] == 7 and lo <= n[2] <= hi]
        if not posted and not evs:
            causes["no-input"] = g
        top = max(causes.items(), key=lambda kv: kv[1]) if causes else ("?", 0)
        out.append((g, top[0], {k: round(v, 2) for k, v in causes.items()}))
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("logs", nargs="+")
    ap.add_argument("--min-ms", type=float, default=16.7)
    a = ap.parse_args()
    tally = Counter()
    worst = []
    for p in a.logs:
        for g, top, causes in classify(F.load(p), a.min_ms):
            tally[top] += 1
            worst.append((g, os.path.basename(p), top, causes))
    worst.sort(reverse=True)
    print(f"{len(worst)} gaps >= {a.min_ms} ms in {len(a.logs)} logs; dominant mechanism: {dict(tally)}")
    for w in worst[:12]:
        print(f"  {w[0]:7.2f} ms  {w[1]}  {w[2]}  {w[3]}")


if __name__ == "__main__":
    main()
