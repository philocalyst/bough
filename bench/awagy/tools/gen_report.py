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
# Frameworks measured here that the site does not register (zgx, BUI): results-extra.json / sizes-extra.json. They have no
# site anchor, so they are shown as measured here and are kept out of the extrapolation's calibration (MEAS below).
EX = json.load(open(os.path.join(W, "results-extra.json"))) if os.path.exists(os.path.join(W, "results-extra.json")) else {}
SIZES_EX = json.load(open(os.path.join(W, "sizes-extra.json"))) if os.path.exists(os.path.join(W, "sizes-extra.json")) else {}
EXTRA = [a for a in ("zgx", "bui") if a in EX]
RR = dict(R, **{a: EX[a] for a in EXTRA})  # every app shown in the primary charts
SITE = json.load(open(os.path.join(W, "benchmarks.json")))["runs"][0]
ORDER = ["bough", "vizia", "vizia-optimized", "zgx", "bui"]
NAMES = {"bough": "bough", "egui": "egui", "iced": "iced", "slint": "Slint", "zgui": "zgui", "freya-optimized": "Freya (optimized)",
         "vizia": "Vizia", "vizia-optimized": "Vizia (optimized)", "gpui": "GPUI", "zgx": "zgx", "bui": "BUI (Blender UI)"}
ROWS = [1000, 3000, 5000, 7000]


def ok(reps):
    return [r for r in reps if "error" not in r]


def agg(app, key, field, how="median"):
    reps = ok(RR.get(app, {}).get(key, []))
    vals = [r[field] for r in reps if r.get(field) is not None]
    if not vals:
        return None
    if how == "max":
        return {"v": max(vals), "lo": min(vals), "hi": max(vals), "n": len(vals)}
    return {"v": statistics.median(vals), "lo": min(vals), "hi": max(vals), "n": len(vals)}


def errors(app, key):
    return [r["error"] for r in RR.get(app, {}).get(key, []) if "error" in r]


def site_val(metric, scen, fw):
    for g in SITE["groups"].get(metric, []):
        if g["scenario"] == scen:
            for p in g["points"]:
                if p["framework"] == fw and p.get("value") is not None:
                    return p["value"]
    return None


apps = [a for a in ORDER if a in RR]
data = {"apps": apps, "names": NAMES, "rows": ROWS, "scroll": {}, "dash": {}, "sizes": SIZES | {a: SIZES_EX[a]["bytes"] for a in EXTRA if a in SIZES_EX}, "site": {}, "extra": EXTRA}
for a in apps:
    data["scroll"][a] = {str(n): {f: agg(a, f"scroll-{n}", f) for f in ("fps", "p95_ms", "p99_ms", "max_gap_ms", "cpu_ms", "rss_mib", "footprint_mib")}
                         | {"errors": errors(a, f"scroll-{n}")} for n in ROWS}
    d = ok(RR.get(a, {}).get("dashboard", []))
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
# zgx and BUI: measured here, no site anchor, not in MEAS and so not in the calibration above
for a in EXTRA:
    def mx(n, f, a=a):
        reps = [r for r in EX[a].get(f"scroll-{n}", []) if "error" not in r and r.get("frames")]
        return statistics.median(r[f] for r in reps) if len(reps) >= 2 else None
    measured_all[a] = {"scroll": {str(n): {f: mx(n, f) for f in ("fps", "p95_ms", "max_gap_ms")} for n in ROWS}, "earlier": False, "extra": True}
    d = [r for r in EX[a].get("dashboard", []) if "error" not in r]
    measured_all[a]["dash"] = {"rss_mean_mib": statistics.median(r["rss_mean_mib"] for r in d) if d else None,
                               "worst_gap_ms": max(r["worst_gap_ms"] for r in d) if d else None, "worst_site": None}
data["all"] = {"order": ["bough"] + EXTRA + [a for a in ALL], "measured": measured_all, "est": est,
               "cal": {k: {"median": band(k)[0], "lo": band(k)[1], "hi": band(k)[2], "n": len(cal[k]),
                           "anchors": sorted({x[0] for x in cal[k]})} if band(k) else None for k in cal},
               "names": NAMES}
data["names"] = NAMES

# executable sizes: the site's Linux x86_64 production builds, converted with the anchors' macOS arm64 / Linux ratio
BS = json.load(open(os.path.join(W, "binary-sizes.json"))) if os.path.exists(os.path.join(W, "binary-sizes.json")) else {"results": []}
linux_size = {r["framework"]: r["bytes"] for r in BS["results"]}
size_ratios = [(a, SIZES[a] / linux_size[a]) for a in SIZES if a in linux_size]
sr = [r for _, r in size_ratios]
data["all"]["size"] = {"measured": {a: SIZES[a] for a in SIZES} | {a: SIZES_EX[a]["bytes"] for a in EXTRA if a in SIZES_EX},
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


# ---------------------------------------------------------------- zgx and BUI: the section of notes and the side-by-side table
def cell(v, d=1):
    return "—" if v is None else f"{v:,.{d}f}"


def med(a, key, f):
    x = data["scroll"].get(a, {}).get(key, {}).get(f)
    return x["v"] if x else None


def side_by_side():
    names = [a for a in ("bough", "zgx", "bui") if a in apps]
    cols = [("Scroll fps 1K", lambda a: med(a, "1000", "fps"), 1), ("3K", lambda a: med(a, "3000", "fps"), 1), ("5K", lambda a: med(a, "5000", "fps"), 1),
            ("7K", lambda a: med(a, "7000", "fps"), 1), ("7K p95 ms", lambda a: med(a, "7000", "p95_ms"), 1), ("7K max gap ms", lambda a: med(a, "7000", "max_gap_ms"), 1),
            ("7K CPU ms", lambda a: med(a, "7000", "cpu_ms"), 0), ("7K RSS MiB", lambda a: med(a, "7000", "rss_mib"), 0), ("7K footprint MiB", lambda a: med(a, "7000", "footprint_mib"), 0),
            ("Dash worst gap ms", lambda a: (data["dash"][a]["worst_gap_ms"] or {}).get("v"), 1), ("Dash p95 ms", lambda a: (data["dash"][a]["p95_ms"] or {}).get("v"), 1),
            ("Dash RSS MiB", lambda a: (data["dash"][a]["rss_mean_mib"] or {}).get("v"), 0),
            ("Size MiB", lambda a: data["sizes"].get(a, 0) / 1048576 or None, 2)]
    h = "<thead><tr><th>Framework</th>" + "".join(f"<th>{html.escape(c[0])}</th>" for c in cols) + "</tr></thead>"
    rows = ""
    for a in names:
        rows += f'<tr class="{"self" if a == "bough" else ""}"><td>{html.escape(NAMES[a])}</td>' + "".join(f"<td>{cell(c[1](a), c[2])}</td>" for c in cols) + "</tr>"
    return f'<div class="table-scroll"><table>{h}<tbody>{rows}</tbody></table></div>'


def clicks_note():
    out = []
    for a in ("bough", "zgx", "bui"):
        d = [r for r in RR.get(a, {}).get("dashboard", []) if "error" not in r]
        if d:
            sent = [next(p["events_sent"] for p in r["phases"] if p["phase"] == "refresh") for r in d]
            out.append(f"{NAMES[a]} {min(sent)}–{max(sent)} refresh clicks sent per run")
    return "; ".join(out)


EXTRA_HTML = ""
if EXTRA:
    EXTRA_HTML = f"""
  <section aria-labelledby="x-h">
    <div style="display:grid;gap:6px">
      <h2 id="x-h">bough next to zgx and BUI</h2>
      <p class="lede">Two frameworks the site does not register, built and measured here with exactly the protocol of the other apps: zgx (Zeron) and BUI, which is Blender’s user interface as a standalone library. They have no site numbers, so they are <strong>measured here only</strong> and are not part of the calibration that converts the site’s Linux numbers. Median of three runs (the dashboard’s worst gap is the largest of three).</p>
    </div>
    {side_by_side()}
    <div class="card" style="gap:10px">
      <h3>Each one’s deviations and caveats</h3>
      <div style="display:grid;gap:10px;color:var(--ink-2);font-size:14px;max-width:78ch">
        <p><strong style="color:var(--ink)">zgx.</strong> <code>zeronsh/zgx</code> at <code>0f0a08af</code> (2026-11-11), a fresh clone used by path; crates 0.1.10 with default features. Its <code>docs/getting-started.md</code> was followed in full, including the <code>[patch.crates-io]</code> set it gives an app (zgx’s forks of wgpu-hal, winit and the AccessKit adapters, which the docs say cut Metal command buffers), taken by path from the clone’s <code>vendor/</code>, the same source as the docs’ git patches. Built with a production profile (fat LTO, one codegen unit, stripped), the docs’ <code>dist</code> profile plus strip; no <code>mimalloc</code> feature and no other tuning. The scroll app is a plain <code>scroll()</code> with <code>keyed(..)</code> rows, 960 × 48, padding 12, 14 px black text, alternating colours, not virtualized. The dashboard is written zgx-natively (the spec allows toolkit-native styling) and reaches the toolbar by layout (input at 24, 70; Refresh and toggle to its right). zgx draws in its own default font: its <code>ZGX_FONT_FILE</code> is read only by its examples, so the other apps’ DejaVu Sans file is not used here.</p>
        <p><strong style="color:var(--ink)">How zgx’s frames were counted.</strong> zgx does not take <code>CAMetalLayer</code> drawables. It shows its own IOSurface as a tile of a plain layer and hides the previous tile, once per frame. The observer therefore hooks <code>-[CALayer setHidden:NO]</code> on a hidden layer whose contents are an IOSurface. It was checked against zgx’s own <code>ZGX_PRESENT_LOG</code>: over a 7,000-row scroll the hook recorded 319 tile flips and zgx logged 319 <code>plain-set</code> events, 0.01 to 0.22 ms apart, with no video or drawable path used. The layer’s <code>setContents:</code> calls when zgx allocates a surface (one arrived mid-scroll) are not frames and are excluded for zgx.</p>
        <p><strong style="color:var(--ink)">BUI.</strong> <code>ppmpreetham/BUI</code> at <code>d6be0397</code> (2026-11-11), GPL-3.0-or-later: Blender’s widget drawing and interaction ported into a C kit with a Rust crate (<code>blenderui</code> 0.5.0, default features, built by path from the clone). It is not Blender and not a general-purpose toolkit: it looks like Blender and follows Blender’s interaction rules. On macOS BUI has no GPU renderer (its GPU renderers are Direct3D 11 and OpenGL 3.3 on Windows and Linux): it draws with its CPU raster into the window’s NSView as a CGImage, once per frame (<code>setNeedsDisplayInRect</code> then <code>displayIfNeeded</code>). That is not a drawable, a GL flush or a published surface, so the observer hooks <code>-[NSView displayIfNeeded]</code> on BUI’s view and stamps it when the call returns. Verified on an animation whose frames the app counted itself: 85–88 renders a second against 85.6 observed records a second. BUI does not wait for the display, so one scrolled frame is handed to AppKit per wheel event whether or not the display can show it (hence 138 presents a second at 1,000 rows against a 120 Hz panel): its “presents per second” counts frames produced, and it overstates what was shown more than the Metal apps’ drawables do.</p>
        <p><strong style="color:var(--ink)">How BUI’s apps were written.</strong> They are the site’s own <code>apps/gpui</code> sources, which BUI’s gpui-compatible syntax accepts, with the crate imported under the name <code>gpui</code> and these changes, all recorded in the files: the <code>div</code> import is dropped (BUI’s prelude supplies it); the root is wrapped in a margin-less <code>&lt;body&gt;</code> of the viewport’s height (BUI’s root is an HTML document with an 8 px body margin that grows with its content, so nothing scrolled without it); in the dashboard the site’s hand-written text input (<code>input.rs</code>, built on gpui’s text system, which BUI lacks) is replaced by BUI’s own <code>input()</code> with the same 360 × 40 box and placeholder, <code>img(path)</code> becomes <code>img().src(path)</code> and the images load from files at run time. BUI’s text is set to the same DejaVu Sans file the other apps use (<code>BUI_FONT</code>). The scroll app has no virtualization; the dashboard mounts all 1,000 records. BUI’s default text colours and the site’s gpui geometry are kept, so its toolbar sits 8 px lower than zgx’s (controls 40 px tall in a 56 px row; input at 24, 78) and the harness’s clicks at (200, 90), (496, 90) and (704, 90) land inside the controls.</p>
        <p><strong style="color:var(--ink)">Clicks.</strong> In the run whose screenshot was taken (in the scroll phase, after the Refresh phase), the Refresh count shown was 480 for zgx and 480 for BUI, equal to the 480 clicks the driver sent in that phase: none was dropped. ({html.escape(clicks_note())}; bough’s run that was short of its 480 is discussed in the verdict above.) BUI’s window must be activated by a click first (the harness does this); a first click on an inactive window only activates it. Only that one run per app was inspected this way.</p>
        <p><strong style="color:var(--ink)">BUI’s dashboard worst gap</strong> was 27.2, 37.4 and 279.8 ms over its three runs, so its 279.8 ms median-of-max is dominated by one stall. That run was the one taken straight after attempts rejected for a covered window (the window was uncovered when it was accepted); it is reported as measured, not removed.</p>
        <p><strong style="color:var(--ink)">Both.</strong> A rejected run (window covered by another window, as happened to BUI’s 7,000-row run 3 and dashboard runs while another window was in front of the app) was run again, not reported. A first batch for both was discarded because another build ran on the Mac during it, and everything was measured again with the Mac otherwise idle.</p>
      </div>
    </div>
  </section>"""

page = open(os.path.join(W, "tools", "report_template.html")).read()
page = page.replace("/*__DATA__*/null", json.dumps(data)).replace("/*__SHOTS__*/null", json.dumps(shots))
page = page.replace("__EXTRA__", EXTRA_HTML)
page = page.replace("__VERDICT__", open(os.path.join(W, "verdict.html")).read())
page = page.replace("__MACHINE__", html.escape("Apple M3 Pro · macOS 15.7.4 · built-in Liquid Retina XDR (ProMotion, 120 Hz)"))
page = page.replace("__SITE_MACHINE__", html.escape("AMD Ryzen 5 5600X · AMD 9060 XT 16 GB · Linux 6.17 · 144 Hz (collection 2026-11-10)"))
open(OUT, "w").write(page)
print(f"wrote {OUT} ({os.path.getsize(OUT) / 1e6:.1f} MB)")
