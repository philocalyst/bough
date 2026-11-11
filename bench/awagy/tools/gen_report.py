#!/usr/bin/env python3
"""Build the evaluation page from results.json (this Mac), sizes.json (production binaries) and the site's own
benchmarks.json (its Linux run), with screenshots embedded as JPEG data URIs.

  gen_report.py <awagy dir> <out.html>
"""
import base64, json, os, statistics, subprocess, sys, html

W = sys.argv[1]
OUT = sys.argv[2]
R = json.load(open(os.path.join(W, "results.json")))
SIZES = json.load(open(os.path.join(W, "sizes.json"))) if os.path.exists(os.path.join(W, "sizes.json")) else {}
SITE = json.load(open(os.path.join(W, "benchmarks.json")))["runs"][0]
ORDER = ["bough", "vizia", "vizia-optimized"]
NAMES = {"bough": "bough", "egui": "egui", "iced": "iced", "slint": "Slint", "zgui": "zgui", "freya-optimized": "Freya (optimized)",
         "vizia": "Vizia", "vizia-optimized": "Vizia (optimized)", "gpui": "GPUI"}
ROWS = [1000, 3000, 5000, 7000]


def ok(reps):
    return [r for r in reps if "error" not in r]


def agg(app, key, field, how="median"):
    reps = ok(R.get(app, {}).get(key, []))
    vals = [r[field] for r in reps if r.get(field) is not None]
    if not vals:
        return None
    if how == "max":
        return {"v": max(vals), "lo": min(vals), "hi": max(vals), "n": len(vals)}
    return {"v": statistics.median(vals), "lo": min(vals), "hi": max(vals), "n": len(vals)}


def errors(app, key):
    return [r["error"] for r in R.get(app, {}).get(key, []) if "error" in r]


def site_val(metric, scen, fw):
    for g in SITE["groups"].get(metric, []):
        if g["scenario"] == scen:
            for p in g["points"]:
                if p["framework"] == fw and p.get("value") is not None:
                    return p["value"]
    return None


apps = [a for a in ORDER if a in R]
data = {"apps": apps, "names": NAMES, "rows": ROWS, "scroll": {}, "dash": {}, "sizes": SIZES, "site": {}}
for a in apps:
    data["scroll"][a] = {str(n): {f: agg(a, f"scroll-{n}", f) for f in ("fps", "p95_ms", "p99_ms", "max_gap_ms", "cpu_ms", "rss_mib", "footprint_mib")}
                         | {"errors": errors(a, f"scroll-{n}")} for n in ROWS}
    d = ok(R.get(a, {}).get("dashboard", []))
    data["dash"][a] = {"worst_gap_ms": agg(a, "dashboard", "worst_gap_ms", "max"), "p95_ms": agg(a, "dashboard", "p95_ms"),
                       "rss_mean_mib": agg(a, "dashboard", "rss_mean_mib"), "footprint_mean_mib": agg(a, "dashboard", "footprint_mean_mib"),
                       "cpu_ms": agg(a, "dashboard", "cpu_ms"), "presents_per_phase": d[0].get("presents_per_phase") if d else None,
                       "errors": errors(a, "dashboard")}
    site_fw = a
    data["site"][a] = {"fps7000": site_val("renderer_fps", "scroll-7000", site_fw), "p95_7000": site_val("frame_gap_p95_ms", "scroll-7000", site_fw),
                       "max7000": site_val("frame_gap_max_ms", "scroll-7000", site_fw), "fps1000": site_val("renderer_fps", "scroll-1000", site_fw),
                       "worst": site_val("dashboard_worst_gap_ms", "dashboard", site_fw), "rss": site_val("dashboard_rss_mean_mib", "dashboard", site_fw)}


# ---------------------------------------------------------------- extrapolation to every registered framework
# Anchors: frameworks measured here AND in the site's collection. bough, Vizia and Vizia (optimized) come from the
# head-to-head run; egui, iced and Slint from the earlier full run (same day, same protocol, same corrected input).
P = json.load(open(os.path.join(W, "results-8fw-partial.json"))) if os.path.exists(os.path.join(W, "results-8fw-partial.json")) else {}
MEAS = {a: R[a] for a in R}
for a in ("egui", "iced", "slint"):
    if a in P:
        MEAS[a] = P[a]
ALL = [f if isinstance(f, str) else f.get("id") for f in SITE["frameworks"]]
NAMES.update({"freya": "Freya", "floem": "Floem", "dioxus-native": "Dioxus Native", "dioxus-native-vello-hybrid": "Dioxus Native (Vello hybrid)",
              "makepad": "Makepad", "gpui-fast": "GPUI Fast", "rune": "Rune", "blinc": "Blinc", "whisker": "Whisker"})
CAP_L, CAP_M = 144.0, 120.0
SITE_KEYS = {"fps": "renderer_fps", "p95_ms": "frame_gap_p95_ms", "max_gap_ms": "frame_gap_max_ms"}


def meas(a, n, f):
    reps = [r for r in MEAS.get(a, {}).get(f"scroll-{n}", []) if "error" not in r and r.get("frames")]
    return statistics.median(r[f] for r in reps) if len(reps) >= 2 else None


def capbound(site_fps):
    return site_fps is not None and site_fps >= 0.95 * CAP_L


# calibration pairs pooled over row counts, split by regime (held at the refresh limit on Linux, or CPU-bound)
cal = {"fps_cap": [], "fps_cpu": [], "p95_ms_cap": [], "p95_ms_cpu": [], "max_gap_ms_cap": [], "max_gap_ms_cpu": [], "rss": []}
anchors = [a for a in MEAS if a != "bough" and a in ALL]
for a in anchors:
    for n in ROWS:
        sf = site_val("renderer_fps", f"scroll-{n}", a)
        mf = meas(a, n, "fps")
        if sf is None or mf is None:
            continue
        reg = "cap" if capbound(sf) else "cpu"
        cal["fps_" + reg].append((a, n, mf / CAP_M if reg == "cap" else mf / sf))
        for f in ("p95_ms", "max_gap_ms"):
            sv, mv = site_val(SITE_KEYS[f], f"scroll-{n}", a), meas(a, n, f)
            if sv and mv:
                cal[f"{f}_{reg}"].append((a, n, mv / sv))
    d = [r for r in MEAS.get(a, {}).get("dashboard", []) if "error" not in r]
    srss = site_val("dashboard_rss_mean_mib", "dashboard", a)
    if d and srss:
        cal["rss"].append((a, None, statistics.median(r["rss_mean_mib"] for r in d) / srss))


def band(key):
    v = [x[2] for x in cal[key]]
    return (statistics.median(v), min(v), max(v)) if v else None


est = {}
for a in ALL:
    if a in MEAS:
        continue
    e = {"scroll": {}, "dash": {}}
    for n in ROWS:
        sf = site_val("renderer_fps", f"scroll-{n}", a)
        row = {}
        if sf is not None:
            reg = "cap" if capbound(sf) else "cpu"
            m, lo, hi = band("fps_" + reg)
            if reg == "cap":
                row["fps"] = {"v": CAP_M * m, "lo": CAP_M * lo, "hi": CAP_M * hi, "reg": reg}
            else:
                row["fps"] = {"v": min(CAP_M, sf * m), "lo": min(CAP_M, sf * lo), "hi": min(CAP_M, sf * hi), "reg": reg}
            for f in ("p95_ms", "max_gap_ms"):
                sv = site_val(SITE_KEYS[f], f"scroll-{n}", a)
                b = band(f"{f}_{reg}")
                if sv is not None and b:
                    row[f] = {"v": sv * b[0], "lo": sv * b[1], "hi": sv * b[2], "reg": reg}
        e["scroll"][str(n)] = row
    srss = site_val("dashboard_rss_mean_mib", "dashboard", a)
    b = band("rss")
    if srss is not None and b:
        e["dash"]["rss_mean_mib"] = {"v": srss * b[0], "lo": srss * b[1], "hi": srss * b[2]}
    e["dash"]["worst_site"] = site_val("dashboard_worst_gap_ms", "dashboard", a)
    est[a] = e

measured_all = {}
for a in MEAS:
    measured_all[a] = {"scroll": {str(n): {f: meas(a, n, f) for f in ("fps", "p95_ms", "max_gap_ms")} for n in ROWS}}
    d = [r for r in MEAS[a].get("dashboard", []) if "error" not in r]
    measured_all[a]["dash"] = {"rss_mean_mib": statistics.median(r["rss_mean_mib"] for r in d) if d else None,
                               "worst_gap_ms": max(r["worst_gap_ms"] for r in d) if d else None,
                               "worst_site": site_val("dashboard_worst_gap_ms", "dashboard", a)}
    measured_all[a]["earlier"] = a in ("egui", "iced", "slint")
data["all"] = {"order": ["bough"] + [a for a in ALL], "measured": measured_all, "est": est,
               "cal": {k: {"median": band(k)[0], "lo": band(k)[1], "hi": band(k)[2], "n": len(cal[k]),
                           "anchors": sorted({x[0] for x in cal[k]})} if band(k) else None for k in cal},
               "names": NAMES}
data["names"] = NAMES

# executable sizes: the site's Linux x86_64 production builds, converted with the anchors' macOS arm64 / Linux ratio
BS = json.load(open(os.path.join(W, "binary-sizes.json"))) if os.path.exists(os.path.join(W, "binary-sizes.json")) else {"results": []}
linux_size = {r["framework"]: r["bytes"] for r in BS["results"]}
size_ratios = [(a, SIZES[a] / linux_size[a]) for a in SIZES if a in linux_size]
sr = [r for _, r in size_ratios]
data["all"]["size"] = {"measured": {a: SIZES[a] for a in SIZES},
                       "est": {a: {"v": linux_size[a] * statistics.median(sr), "lo": linux_size[a] * min(sr), "hi": linux_size[a] * max(sr)}
                               for a in linux_size if a not in SIZES} if sr else {},
                       "ratio": {"median": statistics.median(sr), "lo": min(sr), "hi": max(sr), "anchors": [a for a, _ in size_ratios]} if sr else None}


def jpeg(png, width=900):
    if not os.path.exists(png):
        return None
    tmp = png + ".jpg"
    subprocess.run(["sips", "-s", "format", "jpeg", "-s", "formatOptions", "70", "--resampleWidth", str(width), png, "--out", tmp], capture_output=True)
    return "data:image/jpeg;base64," + base64.b64encode(open(tmp, "rb").read()).decode()


shots = {}
for a in apps:
    shots[a] = {"scroll": jpeg(os.path.join(W, "shots-final", f"{a}-scroll-7000.png")), "dashboard": jpeg(os.path.join(W, "shots-final", f"{a}-dashboard.png"))}

page = open(os.path.join(W, "tools", "report_template.html")).read()
page = page.replace("/*__DATA__*/null", json.dumps(data)).replace("/*__SHOTS__*/null", json.dumps(shots))
page = page.replace("__VERDICT__", open(os.path.join(W, "verdict.html")).read())
page = page.replace("__MACHINE__", html.escape("Apple M3 Pro · macOS 15.7.4 · built-in Liquid Retina XDR (ProMotion, 120 Hz)"))
page = page.replace("__SITE_MACHINE__", html.escape("AMD Ryzen 5 5600X · AMD 9060 XT 16 GB · Linux 6.17 · 144 Hz (collection 2026-11-10)"))
open(OUT, "w").write(page)
print(f"wrote {OUT} ({os.path.getsize(OUT) / 1e6:.1f} MB)")
