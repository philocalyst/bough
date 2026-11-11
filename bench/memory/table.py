#!/usr/bin/env python3
"""Tables from ledger.py's raw runs: the footprint by category bucket (the system's view), the in-process
ledger by holder (bough's view), how the two meet, and the per-row slope between two scroll sizes.

Usage: table.py <outdir> [--sample K] [--slope scroll-100,scroll-7000]

Medians over the repetitions of each case; K is the ledger sample (1 = the first `--at` time, active; 2 = the
second, calm). Bytes are printed in MiB (KiB for the per-row slope).
"""
import glob, json, os, statistics, sys

MiB = 1 << 20


def bucket(name):
    n = name
    if n.startswith("IOSurface"):
        return "IOSurface"
    if n == "IOAccelerator (graphics)":
        return "IOAccelerator (graphics)"
    if n.startswith("IOAccelerator") or n.startswith("IOKit") or "unmapped" in n.lower() or n.startswith("Owned"):
        return "driver (IOAccelerator, IOKit, owned unmapped)"
    if n.startswith("MALLOC") or n == "malloc":
        return "malloc zones"
    if n.startswith(("__DATA", "__AUTH", "__OBJC", "__TPRO", "__IMPORT", "__TEXT", "__LINKEDIT")) or n in ("unused dyld shared cache area", "dyld private memory"):
        return "images (__DATA, __AUTH, ...)"
    if n in ("page table",):
        return "page table"
    if n.startswith("stack") or n.startswith("Stack"):
        return "stacks"
    if n in ("CoreUI image data", "ColorSync", "CoreAnimation", "CG image", "CoreGraphics", "Foundation", "Accelerate image backing stores", "Activity Tracing", "os_alloc_once", "Kernel Alloc Once", "CoreServices", "AppKit"):
        return "framework data (CoreUI, ColorSync, CA, ...)"
    return "other (" + n + ")"


def load(outdir, k):
    cases = {}
    for d in sorted(glob.glob(os.path.join(outdir, "*-*"))):
        case = os.path.basename(d).rsplit("-", 1)[0]
        led = os.path.join(d, "ledger.jsonl")
        fp = os.path.join(d, f"footprint-{k}.json")
        if not (os.path.exists(led) and os.path.exists(fp)):
            continue
        lines = open(led).read().splitlines()
        if len(lines) < k:
            continue
        L = json.loads(lines[k - 1])
        F = json.load(open(fp))["processes"][0]
        cases.setdefault(case, []).append((L, F))
    return cases


def summarize(L, F):
    led, proc = L["ledger"]["lines"], L["ledger"]["proc"]
    s = {}
    s["phys_footprint"] = proc["phys_footprint"]
    s["resident (RSS)"] = proc["resident"]
    s["footprint tool total"] = F["footprint"]
    for cat, v in F["categories"].items():
        b = "fp: " + bucket(cat)
        s[b] = s.get(b, 0) + v["dirty"] + v.get("swapped", 0)
        if v.get("reclaimable", 0):
            s["fp reclaimable (not in footprint): " + bucket(cat)] = s.get("fp reclaimable (not in footprint): " + bucket(cat), 0) + v["reclaimable"]
    cost = lambda x: x["resident"] if x["resident"] is not None else x["reserved"]
    reg = [x for x in led if x["space"] == "Region" and x["group"] != "region"]
    whole = [x for x in led if x["group"] == "region" and x["name"].startswith("whole")]
    s["ledger: region resident (whole)"] = whole[0]["resident"] if whole else 0
    s["ledger: region used bytes (columns' len)"] = sum(x["used"] for x in reg)
    s["ledger: region reserved (capacity)"] = whole[0]["reserved"] if whole else 0
    for x in reg:
        s[f"ledger: region col {x['name']} resident"] = cost(x)
    surf = [x for x in led if x["space"] == "Gpu" and x["name"].startswith("surface")]
    s["ledger: surfaces drawn"] = sum(1 for x in surf if (x["resident"] or 0) > 0)
    s["ledger: surfaces resident"] = sum(x["resident"] or 0 for x in surf)
    s["ledger: surfaces allocated (virtual)"] = sum(x["reserved"] for x in surf)
    s["ledger: gpu other (atlas, bins, counts, images)"] = sum(x["reserved"] for x in led if x["space"] == "Gpu" and not x["name"].startswith(("surface", "(region")))
    heap = [x for x in led if x["space"] == "Heap"]
    s["ledger: heap lines total"] = sum(cost(x) for x in heap)
    for g in sorted({x["group"] for x in heap}):
        s[f"ledger: heap {g}"] = sum(cost(x) for x in heap if x["group"] == g)
    s["proc: rust live"] = proc["rust_live"]
    s["proc: rust live - ledger heap lines (unattributed)"] = proc["rust_live"] - s["ledger: heap lines total"] if proc["rust_live"] else None
    s["proc: malloc in use"] = proc["malloc_in_use"]
    s["proc: malloc in use - rust live (frameworks' malloc)"] = proc["malloc_in_use"] - proc["rust_live"] if proc["rust_live"] else None
    s["proc: metal currentAllocatedSize"] = proc["metal_allocated"]
    return s


def med(vals):
    vals = [v for v in vals if v is not None]
    return statistics.median(vals) if vals else None


def main():
    a = sys.argv[1:]
    k = 1
    slope = None
    if "--sample" in a:
        i = a.index("--sample"); k = int(a[i + 1]); del a[i:i + 2]
    if "--slope" in a:
        i = a.index("--slope"); slope = a[i + 1].split(","); del a[i:i + 2]
    cases = load(a[0], k)
    order = sorted(cases, key=lambda c: (not c.startswith("counter"), not c.startswith("dashboard"), c.startswith("heavy"), int(c.split("-")[1]) if c.startswith("scroll-") else 0))
    S = {c: [summarize(L, F) for L, F in cases[c]] for c in order}
    keys = []
    for c in order:
        for s in S[c]:
            for key in s:
                if key not in keys:
                    keys.append(key)
    print("| holder | " + " | ".join(f"{c} (n={len(S[c])})" for c in order) + (" | per row, KiB |" if slope else " |"))
    print("|---|" + "---:|" * len(order) + ("---:|" if slope else ""))
    for key in keys:
        row = []
        for c in order:
            m = med([s.get(key) for s in S[c]])
            row.append("" if m is None else (f"{m:.0f}" if key == "ledger: surfaces drawn" else f"{m / MiB:.2f}"))
        sl = ""
        if slope:
            a0, a1 = (med([s.get(key) for s in S.get(x, [])]) for x in slope)
            n0, n1 = (int(x.split("-")[1]) for x in slope)
            if a0 is not None and a1 is not None and key != "ledger: surfaces drawn":
                sl = f"{(a1 - a0) / (n1 - n0) / 1024:.3f}"
        print(f"| {key} | " + " | ".join(row) + (f" | {sl} |" if slope else " |"))


if __name__ == "__main__":
    main()
