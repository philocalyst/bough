#!/usr/bin/env python3
"""Read a bough frame log (feature `framelog`; format: docs/lanes/pacing/FRAMELOG.md) and report, per workload
phase, the distributions the pacing lane gates on:

* presents: commits per second, and the intervals between commits (AWAGY's rule: from the first commit at or
  after the phase start to the phase end, the trailing gap included in the max);
* input-to-commit: for every input that left something to show, the time from the event's own timestamp to the
  end of the Core Animation commit of the first frame that includes it (a superseded publication counts as shown
  by the next committed one);
* turn time: host callback entry to return, and its phases;
* queueing: event timestamp to host callback entry;
* the largest commit gaps, each with what the main thread and the publisher did inside it.

  framelog.py <log> [--json out.json] [--gaps N] [--scene dashboard|scroll]
"""
import argparse, json, math, statistics, sys, bisect
from collections import defaultdict

TURN, PUB, VSYNC, NOTE, EVENT = 1, 2, 3, 4, 5
KIND = {1: "move", 2: "down", 3: "up", 4: "wheel", 5: "key", 6: "tick", 7: "timer", 8: "resize", 9: "leave", 10: "paced", 11: "other"}
M_EVENT, M_DISPATCH, M_SYNC, M_SETTLE, M_DESCRIBE, M_LAYOUT, M_FRAME, M_PICK, M_SLOT, M_END, M_FID, M_FLAGS, M_CPU, M_FLT = range(2, 16)
NOTES = {1: "superseded", 2: "ring-wait", 3: "trim", 4: "warm-blit", 5: "link", 6: "bins", 7: "posted", 8: "occlusion", 9: "held-back", 10: "phase", 11: "wakeups",
         12: "gpu-wait", 13: "drain", 14: "prepare", 15: "upload", 16: "latch"}
DASH_PHASES = ["typing", "refresh", "toggle", "scroll"]


def pct(xs, q):
    """Ceil-indexed percentile (AWAGY frames.py)."""
    if not xs:
        return None
    s = sorted(xs)
    return s[max(0, math.ceil(len(s) * q) - 1)]


def dist(xs):
    if not xs:
        return {"n": 0}
    return {"n": len(xs), "p50": pct(xs, 0.5), "p95": pct(xs, 0.95), "p99": pct(xs, 0.99), "max": max(xs), "mean": statistics.mean(xs)}


def load(path):
    rows = []
    for line in open(path):
        if line.startswith("#"):
            continue
        rows.append(list(map(int, line.split())))
    return rows


def analyse(rows, scene=None, ngaps=5):
    turns = sorted((r for r in rows if r[0] == TURN), key=lambda r: r[M_DISPATCH])
    pubs = [r for r in rows if r[0] == PUB]
    notes = [r for r in rows if r[0] == NOTE]
    events = [r for r in rows if r[0] == EVENT]
    # the refresh timeline: the measurement recorder's vsyncs (source 1) when present, else the host link's
    vrows = [r for r in rows if r[0] == VSYNC]
    vs = sorted(r[1] for r in vrows if r[4] == 1) or sorted(r[1] for r in vrows if r[4] == 0)
    link_ticks = sorted(r[3] for r in vrows if r[4] == 0)
    posted = [r for r in notes if r[1] == 7]
    phases = sorted((r[3], r[4]) for r in notes if r[1] == 10)
    if scene is None:
        scene = "dashboard" if len(phases) >= 5 else "scroll"
    # committed publications by sequence; commit times (c0: just before setContents, AWAGY's observer point)
    committed = sorted((r[1], r[5], r[6]) for r in pubs if r[7] == 1)
    seqs = [c[0] for c in committed]
    c0s = sorted(c[1] for c in committed)
    # windows
    if scene == "dashboard":
        bounds = [(DASH_PHASES[i], phases[i][1], phases[i + 1][1]) for i in range(min(4, len(phases) - 1))]
        windows = bounds + [("all", phases[0][1], phases[min(4, len(phases) - 1)][1])] if bounds else []
    else:
        start, end = phases[0][1], phases[1][1]
        # AWAGY scroll: the 1 s warm-up is excluded; the window is 3 s from the first commit after it
        anchor_from = start + int(1e9)
        windows = [("scroll", anchor_from, min(end, anchor_from + int(3e9)))]
    out = {"scene": scene, "windows": {}}
    # inputs that left something to show -> the first frame at or after their turn
    frames = [t for t in turns if t[M_FID] > 0]
    fdisp = [t[M_DISPATCH] for t in frames]

    def next_vsync(c):
        i = bisect.bisect_left(vs, c)
        return vs[i] if i < len(vs) else None

    def shown_at(t):
        """Commit end of the first committed publication including turn `t`'s effect."""
        i = bisect.bisect_left(fdisp, t[M_DISPATCH])
        if i >= len(frames):
            return None
        fid = frames[i][M_FID]
        j = bisect.bisect_left(seqs, fid)
        return committed[j][2] if j < len(committed) else None

    for name, a, b in windows:
        w = {}
        cs = [c for c in c0s if a < c <= b]
        anchor = next((c for c in c0s if c >= a), None)
        sel = [c for c in c0s if anchor is not None and anchor < c <= b]
        iv, prev = [], anchor
        for c in sel:
            iv.append((c - prev) / 1e6)
            prev = c
        trailing = (b - prev) / 1e6 if prev is not None else (b - a) / 1e6
        w["presents"] = len(cs)
        w["presents_per_s"] = len(cs) / ((b - a) / 1e9)
        w["refreshes"] = sum(1 for v in vs if a < v <= b)
        w["link_callbacks"] = sum(1 for v in link_ticks if a < v <= b)
        w["commit_interval_ms"] = dist(iv)
        w["worst_gap_ms"] = max([trailing, *iv]) if iv else trailing
        ins = [t for t in turns if a <= t[M_DISPATCH] < b and t[1] <= 5]
        w["inputs"] = len(ins)
        w["posted"] = sum(1 for p in posted if a <= p[2] < b)
        by_kind = defaultdict(list)
        lat_all, lat_vs = [], []
        for t in ins:
            if not (t[M_FLAGS] >> 3) & 1:
                continue
            s = shown_at(t)
            if s is None or not t[M_EVENT]:
                continue
            ms = (s - t[M_EVENT]) / 1e6
            by_kind[KIND[t[1]]].append(ms)
            lat_all.append(ms)
            v = next_vsync(s)
            if v is not None:
                lat_vs.append((v - t[M_EVENT]) / 1e6)
        w["input_to_commit_ms"] = dist(lat_all)
        w["input_to_vsync_after_commit_ms"] = dist(lat_vs)
        # commits per refresh interval (a value above 1 is a present the display never showed)
        per = defaultdict(int)
        for c in cs:
            i = bisect.bisect_left(vs, c)
            per[i] += 1
        w["refreshes_with_2plus_commits"] = sum(1 for k, n in per.items() if n > 1)
        w["input_to_commit_by_kind_ms"] = {k: dist(v) for k, v in sorted(by_kind.items())}
        w["turn_ms"] = dist([(t[M_END] - t[M_DISPATCH]) / 1e6 for t in turns if a <= t[M_DISPATCH] < b])
        w["input_turn_ms"] = dist([(t[M_END] - t[M_DISPATCH]) / 1e6 for t in ins])
        w["queue_ms"] = dist([(t[M_DISPATCH] - t[M_EVENT]) / 1e6 for t in ins if t[M_EVENT]])
        w["turn_cpu_ms"] = dist([t[M_CPU] / 1e6 for t in ins])
        w["faults_per_turn"] = dist([t[M_FLT] for t in ins])
        w["notes"] = {NOTES.get(k, k): sum(1 for n in notes if n[1] == k and a <= n[2] < b) for k in sorted({n[1] for n in notes})}
        out["windows"][name] = w
    # the largest gaps between consecutive commits over the whole workload
    if windows:
        a, b = windows[-1][1], windows[-1][2]
        sel = [c for c in c0s if a <= c <= b]
        gaps = sorted(((sel[i + 1] - sel[i], sel[i], sel[i + 1]) for i in range(len(sel) - 1)), reverse=True)[:ngaps]
        out["gaps"] = [describe_gap(g, turns, pubs, notes, events) for g in gaps]
    return out


def phase_breakdown(t):
    """Per-phase µs of a turn (0 where a phase did not run)."""
    def d(x, y):
        return (t[y] - t[x]) / 1e3 if t[x] and t[y] else None
    return {"kind": KIND.get(t[1], t[1]), "queue": d(M_EVENT, M_DISPATCH) if t[M_EVENT] else None, "sync": d(M_DISPATCH, M_SYNC), "settle": d(M_SYNC, M_SETTLE),
            "describe": d(M_SETTLE, M_DESCRIBE), "layout": d(M_DESCRIBE, M_LAYOUT), "frame": d(M_FRAME, M_END), "pick": d(M_FRAME, M_PICK), "total": d(M_DISPATCH, M_END),
            "cpu": t[M_CPU] / 1e3, "faults": t[M_FLT]}


def describe_gap(g, turns, pubs, notes, events):
    dt, a, b = g
    inside = [t for t in turns if a <= t[M_DISPATCH] <= b or a <= t[M_END] <= b]
    busy = sum(min(t[M_END], b) - max(t[M_DISPATCH], a) for t in inside)
    longest = sorted(inside, key=lambda t: t[M_END] - t[M_DISPATCH], reverse=True)[:3]
    pp = [r for r in pubs if a <= r[2] <= b or a <= r[6] <= b]
    return {"gap_ms": dt / 1e6, "at_ms_from_first": None, "turns": len(inside), "main_busy_ms": busy / 1e6,
            "events_received": sum(1 for e in events if a <= e[3] <= b),
            "longest_turns_us": [phase_breakdown(t) for t in longest],
            "publications": [{"seq": r[1], "post_to_take_us": (r[3] - r[2]) / 1e3 if r[2] else None, "take_to_drawn_us": (r[4] - r[3]) / 1e3, "commit_us": (r[6] - r[5]) / 1e3 if r[7] else None, "committed": r[7]} for r in pp][:6],
            "notes": [NOTES.get(n[1], n[1]) + f"({n[3]},{n[4]})" for n in notes if a <= n[2] <= b and n[1] != 7][:10]}


def fmt(d):
    if not d or d.get("n", 0) == 0:
        return "n=0"
    return f"n={d['n']} p50={d['p50']:.2f} p95={d['p95']:.2f} p99={d['p99']:.2f} max={d['max']:.2f}"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("log")
    ap.add_argument("--json")
    ap.add_argument("--gaps", type=int, default=5)
    ap.add_argument("--scene")
    a = ap.parse_args()
    out = analyse(load(a.log), a.scene, a.gaps)
    for name, w in out["windows"].items():
        print(f"[{name}] presents {w['presents']} ({w['presents_per_s']:.1f}/s), refreshes {w['refreshes']}, link callbacks {w['link_callbacks']}, worst gap {w['worst_gap_ms']:.2f} ms, inputs {w['inputs']} of {w['posted']} posted")
        print(f"   commit interval ms  {fmt(w['commit_interval_ms'])}")
        print(f"   input->commit ms    {fmt(w['input_to_commit_ms'])}")
        print(f"   input->vsync ms     {fmt(w['input_to_vsync_after_commit_ms'])}   (the first vsync at or after the commit; refreshes with 2+ commits: {w['refreshes_with_2plus_commits']})")
        for k, d in w["input_to_commit_by_kind_ms"].items():
            print(f"     {k:6s}            {fmt(d)}")
        print(f"   input turn ms       {fmt(w['input_turn_ms'])}")
        print(f"   queue ms            {fmt(w['queue_ms'])}")
        print(f"   notes {w['notes']}")
    for g in out.get("gaps", []):
        print("gap", json.dumps(g))
    if a.json:
        json.dump(out, open(a.json, "w"), indent=1)


if __name__ == "__main__":
    main()
