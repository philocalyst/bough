#!/usr/bin/env python3
"""Tables of the whole-scene ledger (lane memory, round 7): `scene_table.py <run.jsonl>... [--lines N]`.

Each input line is one sample of `crates/bough/examples/scene_mem` (phase, process totals, ledger lines) and its
`footprint -j` file. Per phase it prints the system's view (phys_footprint, resident, footprint categories with
the GPU driver's unmapped memory as the remainder) and bough's view (ledger groups, the largest lines).
MiB throughout.
"""
import json, sys, collections

MIB = 1048576.0


def load(path):
    out = []
    for line in open(path):
        line = line.strip()
        if line:
            out.append(json.loads(line))
    return out


def fp_categories(path):
    try:
        d = json.load(open(path))
    except Exception:
        return None
    p = d["processes"][0]
    cats = {}
    for k, v in p["categories"].items():
        cats[k] = (v.get("dirty", 0) + v.get("swapped", 0), v.get("clean", 0), v.get("reclaimable", 0), v.get("wired", 0))
    return p.get("footprint", 0), cats


def cost(l):
    return l["resident"] if l["resident"] is not None else l["reserved"]


def main():
    args = sys.argv[1:]
    nlines = 12
    if "--lines" in args:
        i = args.index("--lines")
        nlines = int(args[i + 1])
        del args[i:i + 2]
    for path in args:
        for s in load(path):
            led = s["ledger"]
            p = led["proc"]
            print(f"\n## {s['scene']} / {s['phase']} (t = {s['t']:.1f} s)" + (f", records {s['records']}" if "records" in s else ""))
            extra = []
            for k in ("startup_ns", "startup_instr", "ns", "instr", "frames", "resizes"):
                if k in s:
                    v = s[k]
                    extra.append(f"{k} {v/1e6:.1f} M" if k.endswith("instr") else (f"{k} {v/1e6:.1f} ms" if k.endswith("ns") else f"{k} {v}"))
            if extra:
                print("   " + ", ".join(extra))
            print(f"   phys_footprint {p['phys_footprint']/MIB:.1f}  resident {p['resident']/MIB:.1f}  rust live {p['rust_live']/MIB:.1f}  malloc in use {p['malloc_in_use']/MIB:.1f} (frameworks {max(0, p['malloc_in_use']-p['rust_live'])/MIB:.1f})  Metal allocated {p['metal_allocated']/MIB:.1f}")
            fc = fp_categories(s.get("footprint_file", ""))
            if fc:
                total, cats = fc
                mapped = sum(v[0] for v in cats.values())
                top = sorted(cats.items(), key=lambda kv: -kv[1][0])[:8]
                print("   footprint categories (dirty+swapped): " + ", ".join(f"{k} {v[0]/MIB:.1f}" for k, v in top if v[0] > 0.05 * MIB))
                recl = sum(v[2] for v in cats.values())
                print(f"   mapped dirty {mapped/MIB:.1f}, unmapped (driver etc.) {(p['phys_footprint']-mapped)/MIB:.1f}, reclaimable {recl/MIB:.1f}")
            groups = collections.defaultdict(lambda: [0, 0])
            for l in led["lines"]:
                g = groups[(l["group"], l["space"])]
                g[0] += cost(l)
                g[1] += l["used"]
            print("   ledger groups (cost / used): " + ", ".join(f"{g}/{sp} {c/MIB:.2f}/{u/MIB:.2f}" for (g, sp), (c, u) in sorted(groups.items(), key=lambda kv: -kv[1][0]) if c > 0.01 * MIB))
            big = sorted(led["lines"], key=lambda l: -cost(l))[:nlines]
            for l in big:
                r = "" if l["resident"] is None else f" resident {l['resident']/MIB:.2f}"
                print(f"     {cost(l)/MIB:7.2f}  {l['group']}/{l['space']}: {l['name']} (used {l['used']/MIB:.2f}, reserved {l['reserved']/MIB:.2f}{r})")


if __name__ == "__main__":
    main()
