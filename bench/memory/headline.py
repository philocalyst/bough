#!/usr/bin/env python3
"""The ledger's headline rows (LEDGER.md) from ledger.py's raw runs: per case, the median over repetitions of
phys_footprint, RSS, the GPU driver's unmapped memory (footprint's `IOAccelerator (graphics)` minus what
`vmmap --summary` shows mapped under that tag, plus `IOAccelerator` likewise), the surfaces, the region (resident
and in use), the other GPU objects, the small mapped Metal buffers (mapped graphics minus region and GPU objects),
the malloc zones, the Rust heap and the rest. Usage: headline.py <outdir> [sample=1]"""
import glob, json, os, re, statistics, sys

MiB = 1 << 20
U = {"K": 1 << 10, "M": 1 << 20, "G": 1 << 30}


def vm_resident(path, tag):
    for l in open(path):
        if l.startswith(tag + " ") and l[len(tag):len(tag) + 2].strip() == "" or l.startswith(tag + "  "):
            f = l[len(tag):].split()
            v = f[1]
            return float(v[:-1]) * U[v[-1]] if v[-1] in U else float(v)
    return 0.0


def one(d, k):
    L = json.loads(open(f"{d}/ledger.jsonl").read().splitlines()[k - 1])["ledger"]
    F = json.load(open(f"{d}/footprint-{k}.json"))["processes"][0]["categories"]
    p, lines = L["proc"], L["lines"]
    vm = f"{d}/vmmap-{k}.txt"
    g_fp = F.get("IOAccelerator (graphics)", {}).get("dirty", 0)
    a_fp = F.get("IOAccelerator", {}).get("dirty", 0) + F.get("IOKit", {}).get("dirty", 0)
    g_map = vm_resident(vm, "IOAccelerator (graphics)")
    a_map = vm_resident(vm, "IOAccelerator") + vm_resident(vm, "IOKit")
    region = next(x for x in lines if x["group"] == "region" and x["name"].startswith("whole"))
    gpu_other = sum(x["reserved"] for x in lines if x["space"] == "Gpu" and not x["name"].startswith(("surface", "(region")))
    surf = [x for x in lines if x["name"].startswith("surface") and (x["resident"] or 0) > 0]
    used = sum(x["used"] for x in lines if x["space"] == "Region" and x["group"] != "region")
    malloc = sum(v["dirty"] + v.get("swapped", 0) for c, v in F.items() if c.startswith("MALLOC"))
    heap = sum(x["resident"] if x["resident"] is not None else x["reserved"] for x in lines if x["space"] == "Heap")
    r = {
        "phys_footprint": p["phys_footprint"],
        "RSS": p["resident"],
        "driver, unmapped graphics": g_fp - g_map,
        "driver, IOAccelerator + IOKit unmapped": a_fp - a_map,
        "surfaces drawn (n)": len(surf) * MiB,
        "surfaces": sum(x["resident"] for x in surf),
        "region resident": region["resident"],
        "region in use": used,
        "other GPU objects": gpu_other,
        "small mapped Metal buffers": g_map - region["resident"] - gpu_other,
        "malloc zones": malloc,
        "Rust heap live": p["rust_live"] or None,
        "named heap lines": heap,
        "frameworks' malloc": (p["malloc_in_use"] - p["rust_live"]) if p["rust_live"] else None,
    }
    named = r["driver, unmapped graphics"] + r["driver, IOAccelerator + IOKit unmapped"] + r["surfaces"] + g_map + malloc
    r["rest (images, framework data, page table, stacks)"] = p["phys_footprint"] - named
    return r


def main():
    out, k = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 1
    cases = ["counter", "dashboard", "scroll-100", "scroll-1000", "scroll-7000", "heavy"]
    rows = {}
    for c in cases:
        rs = []
        for d in sorted(glob.glob(os.path.join(out, f"{c}-[0-9]"))):
            try:
                rs.append(one(d, k))
            except (IndexError, FileNotFoundError, StopIteration):
                pass
        rows[c] = rs
    keys = list(rows["counter"][0].keys()) if rows["counter"] else []
    print("| holder | " + " | ".join(f"{c} (n={len(rows[c])})" for c in cases) + " |")
    print("|---|" + "---:|" * len(cases))
    for key in keys:
        cells = []
        for c in cases:
            v = [r[key] for r in rows[c] if r.get(key) is not None]
            if not v:
                cells.append("–")
            elif key == "surfaces drawn (n)":
                cells.append(f"{statistics.median(v) / MiB:.0f}")
            elif key in ("phys_footprint", "RSS"):
                cells.append(f"{statistics.median(v) / MiB:.1f} ({min(v) / MiB:.1f}–{max(v) / MiB:.1f})")
            else:
                cells.append(f"{statistics.median(v) / MiB:.2f}")
        print(f"| {key} | " + " | ".join(cells) + " |")


if __name__ == "__main__":
    main()
