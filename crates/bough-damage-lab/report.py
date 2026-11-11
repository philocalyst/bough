#!/usr/bin/env python3
"""Join results/verify_*.csv and results/bench_*.csv into markdown tables.

GPU cost model (calibrated on the M3 Pro, calib/metal_calib*.swift):
  gpu_us = PASS (if anything is drawn) + max(T32 * hw_tiles_touched, FRAG * fragments)
           + INST * instances + COPY * copied_px
  Tile load/store bandwidth and fragment ALU overlap (4x full-screen overdraw
  costs the same as 1x until fragments exceed the bandwidth time).
Consumption formats:
  S: one draw (zgx: one per region), scissor = region bbox, fragments masked to
     the region; tiles touched = tiles under each drawn prim's coverage ∩ scissor.
  C: geometry clipped to runs of damaged 32 px hardware tiles; tiles touched =
     region tiles; instances = (prim, run) pairs.
zgx as shipped also copies its retained texture to the drawable every frame:
ZGX_COPY µs (blit_full_surface, measured), scaled by window area.

usage: report.py headline|key|full  [verify csvs...] -- [bench csvs...]
"""
import csv, sys, os
from collections import OrderedDict

PASS, T32, INST, FRAG, COPY = 15.0, 0.025, 0.002, 0.000005, 0.00008
ZGX_COPY_4MPX = 255.0  # µs for 2560x1600 (blit_full_surface, quiet machine)
D = os.path.join(os.path.dirname(os.path.abspath(__file__)), "results")

def load(names):
    rows = []
    for n in names:
        with open(os.path.join(D, n)) as f:
            rows += list(csv.DictReader(f))
    return rows

def fnum(v, k):
    try:
        return float(v.get(k, "nan"))
    except (TypeError, ValueError):
        return float("nan")

def gpu(v, fmt):
    f = lambda k: fnum(v, k) if v.get(k) not in (None, "") else 0.0
    drawn = 1.0 if f("prims") > 0 else 0.0
    if fmt == "S":
        return PASS * drawn + max(T32 * f("t32_s"), FRAG * f("frags")) + INST * f("prims") + COPY * f("copy_px")
    return PASS * drawn + max(T32 * f("t32_r"), FRAG * f("frags")) + INST * f("inst_c") + COPY * f("copy_px")

AREA = {"heavy": 2560 * 1600, "lab": 1920 * 1440, "animate": 1920 * 1440, "blink": 1920 * 1440, "vlist": 1440 * 1600}

def zgx_copy(trace):
    a = AREA[trace.split("-")[0]]
    return ZGX_COPY_4MPX * a / (2560 * 1600)

def fmt_of(form):
    return "C" if form.endswith("c") and form not in ("full",) else "S"

def main():
    mode = sys.argv[1]
    args = sys.argv[2:]
    sep = args.index("--")
    V, B = OrderedDict(), {}
    for r in load(args[:sep]):
        V[(r["trace"], r["ring"], r["form"])] = r
    for r in load(args[sep + 1:]):
        B[(r["trace"], r["ring"], r["form"])] = r
    # Format-C variants reuse their base formulation's verified region/metrics.
    for (t, ring, form), b in list(B.items()):
        if (t, ring, form) not in V and form.endswith("c") and (t, ring, form[:-1]) in V:
            V[(t, ring, form)] = V[(t, ring, form[:-1])]
    traces = list(OrderedDict.fromkeys(k[0] for k in V))
    key_forms = ["full", "zgx", "zgx-vp", "bbox", "tile16", "tile32", "exact4", "exact32", "stamps16", "sig32", "xg32", "tstamp32", "bands32", "hw32", "hw32c"]
    head_forms = ["zgx", "bbox", "tile16", "exact4", "sig32", "hw32", "hw32c"]
    out = []
    if mode == "headline":
        out.append("| workload | age | oracle tiles | formulation | CPU instr/frame | CPU µs | hw tiles | prims | GPU µs | total µs |")
        out.append("|---|---:|---:|---|---:|---:|---:|---:|---:|---:|")
        for t in traces:
            for ring in ["1", "3"]:
                for form in head_forms:
                    v, b = V.get((t, ring, form)), B.get((t, ring, form))
                    if not v or not b:
                        continue
                    ci = fnum(b, "track_instr") + fnum(b, "cull_instr")
                    cn = (fnum(b, "track_ns") + fnum(b, "cull_ns")) / 1000
                    f = fmt_of(form)
                    g = gpu(v, f)
                    tiles = fnum(v, "t32_r" if f == "C" else "t32_s")
                    prims = fnum(v, "inst_c" if f == "C" else "prims")
                    name = form
                    if form == "zgx" and ring == "1":
                        g += zgx_copy(t)
                        name = "zgx (+ retained copy)"
                    out.append(f"| {t} | {ring} | {fnum(v,'oracle_t32'):,.0f} | {name} | {ci:,.0f} | {cn:,.1f} | {tiles:,.0f} | {prims:,.0f} | {g:,.1f} | {cn+g:,.1f} |")
        print("\n".join(out))
        return
    forms_of = (lambda t, ring: [k[2] for k in V if k[0] == t and k[1] == ring]) if mode == "full" else (lambda t, ring: [f for f in key_forms if (t, ring, f) in V])
    for t in traces:
        out.append(f"\n#### {t}\n")
        for ring in ["1", "2", "3"]:
            fl = forms_of(t, ring)
            if not fl:
                continue
            o = V[(t, ring, fl[0])]
            out.append(f"\nage {ring} — oracle Δ: {fnum(o,'oracle_px'):,.0f} px, {fnum(o,'oracle_t32'):,.1f} hw tiles (32 px), {fnum(o,'oracle_t16'):,.1f} tiles@16, {fnum(o,'oracle_t4'):,.1f} tiles@4\n")
            out.append("| form | track instr | cull instr | track ns | cull ns | region px | prims | hw tiles S | hw tiles C | inst C | GPU µs S | GPU µs C | mem B |")
            out.append("|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|")
            for form in fl:
                v, b = V[(t, ring, form)], B.get((t, ring, form), {})
                out.append(
                    f"| {form} | {fnum(b,'track_instr'):,.0f} | {fnum(b,'cull_instr'):,.0f} | {fnum(b,'track_ns'):,.0f} | {fnum(b,'cull_ns'):,.0f} | "
                    f"{fnum(v,'region_px'):,.0f} | {fnum(v,'prims'):,.0f} | {fnum(v,'t32_s'):,.0f} | {fnum(v,'t32_r'):,.0f} | {fnum(v,'inst_c'):,.0f} | "
                    f"{gpu(v,'S'):,.1f} | {gpu(v,'C'):,.1f} | {v.get('mem','')} |"
                )
    print("\n".join(out))

if __name__ == "__main__":
    main()
