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
