#!/usr/bin/env python3
"""Generate artifact.html (page content for the Artifact publisher) and index.html (the same page in a
full document, for opening locally) from data.json. Python 3 standard library only.

Every chart is inline SVG generated here from data.json. Tooltips and load animation are inline JS and CSS.
Nothing is drawn that is not in data.json: missing values are drawn as explicit "not measured" states.
"""

import html
import json
import math
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
DATA_PATH = os.path.join(HERE, "data.json")
ARTIFACT_PATH = os.path.join(HERE, "artifact.html")
INDEX_PATH = os.path.join(HERE, "index.html")

TITLE = "Bough Round 6 Results"  # the published artifact's name; kept across rounds
KINDS = ("measured", "reported", "estimated")
LOG_THRESHOLD = 50  # use a log scale when max/min exceeds this
PAIR_LABEL = re.compile(r"(base|before|main)\b")
FAMILY_ID = re.compile(r"^(gb6-.+)-n(\d+)$")
# Series whose N=... points belong to one gb6 family (the table_* series repeat the gb6 base values).
TABLE_FAMILY = {
    "table_select": "gb6-table-coarse-plain-select",
    "table_update_every_10th": "gb6-table-coarse-plain-update10",
    "table_swap": "gb6-table-coarse-plain-swap",
    "table_remove": "gb6-table-coarse-plain-remove",
}

TOKENS_LIGHT = """
  color-scheme: light;
  --bg: #f6f6f2; --panel: #ffffff; --fg: #1b1b1f; --muted: #5f5f68; --rule: #dcdcd6; --grid: #e9e9e3;
  --measured: #2a5db0; --reported: #b4661a; --estimated: #7a4fb0; --good: #17806a; --bad: #c0503a;
  --c1: #2a5db0; --c2: #17806a; --c3: #b4661a; --c4: #7a4fb0; --c5: #b03a72;
  --f1: #2a78d6; --f2: #eb6834; --f3: #1baf7a; --f4: #eda100;
"""
TOKENS_DARK = """
  color-scheme: dark;
  --bg: #131316; --panel: #1c1c21; --fg: #ececef; --muted: #9d9da7; --rule: #34343c; --grid: #2a2a31;
  --measured: #7aa3e8; --reported: #e8a464; --estimated: #b994e8; --good: #5fd0b2; --bad: #f08f76;
  --c1: #7aa3e8; --c2: #5fd0b2; --c3: #e8a464; --c4: #b994e8; --c5: #ef8fbd;
  --f1: #3987e5; --f2: #d95926; --f3: #199e70; --f4: #c98500;
"""

CSS_TOKENS = (
    ":root {" + TOKENS_LIGHT + "}\n"
    "@media (prefers-color-scheme: dark) {\n"
    '  :root:not([data-theme="light"]) {' + TOKENS_DARK + "}\n"
    "}\n"
    ':root[data-theme="dark"] {' + TOKENS_DARK + "}\n"
)

CSS_BODY = """
body { margin: 0; background: var(--bg); color: var(--fg);
  font: 16px/1.45 system-ui, -apple-system, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif; }
[hidden] { display: none !important; }
* { box-sizing: border-box; }
main { max-width: 880px; margin: 0 auto; padding-block: 16px;
  padding-left: max(16px, env(safe-area-inset-left, 0px)); padding-right: max(16px, env(safe-area-inset-right, 0px)); }
h1 { font-size: 1.6rem; margin: 0.3em 0 0.1em; }
h2 { font-size: 1.15rem; margin: 2.2rem 0 0.6rem; padding-bottom: 0.3em; border-bottom: 1px solid var(--rule);
  display: flex; flex-wrap: wrap; gap: 0.2em 0.6em; align-items: baseline; }
h3 { font-size: 1rem; margin: 1.4rem 0 0.2rem; overflow-wrap: break-word; }
h4 { font-size: 0.95rem; margin: 0 0 0.2rem; overflow-wrap: break-word; }
p { margin: 0.4em 0; overflow-wrap: break-word; }
.meta, .cap, .small-note { color: var(--muted); font-size: 0.875rem; }
.meta { margin: 0 0 1rem; }
.cap { margin: 0.35rem 0 0; }
.panel { background: var(--panel); border: 1px solid var(--rule); border-radius: 10px; padding: 12px 14px; margin: 0.6rem 0; }
.tag { display: inline-block; font-size: 0.78rem; font-weight: normal; padding: 0 0.45em; margin-left: 0.2em;
  border: 1px solid var(--muted); border-radius: 4px; color: var(--muted); white-space: nowrap; line-height: 1.5; }
.tag.m { border-color: var(--measured); color: var(--measured); }
.tag.r { border-color: var(--reported); color: var(--reported); }
.tag.e { border-color: var(--estimated); color: var(--estimated); border-style: dotted; }
.tag.n { border-style: dashed; }
.tiles { display: grid; grid-template-columns: repeat(auto-fit, minmax(128px, 1fr)); gap: 10px; margin: 0.8rem 0 0; }
.tile { background: var(--panel); border: 1px solid var(--rule); border-radius: 10px; padding: 10px 12px; }
.tile b { display: block; font-size: 1.45rem; font-variant-numeric: tabular-nums; }
.tile span { color: var(--muted); font-size: 0.85rem; }
.tile.donut { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; }
.legend-list { list-style: none; padding: 0; margin: 0; display: grid; gap: 6px; font-size: 0.9rem; }
.legend-list li { display: flex; gap: 8px; align-items: center; }
.legend-list svg { flex: none; }
.scroll { overflow-x: auto; max-width: 100%; -webkit-overflow-scrolling: touch; padding-bottom: 4px; }
svg { display: block; }
svg.fit { width: 100%; height: auto; max-width: 100%; }
svg.wide { width: 600px; height: auto; }
svg text { font-family: inherit; font-size: 14px; fill: var(--fg); font-variant-numeric: tabular-nums; }
svg .lbl { fill: var(--muted); }
svg .small { font-size: 12px; }
svg .mid { text-anchor: middle; }
svg .end { text-anchor: end; }
svg .grid { stroke: var(--grid); stroke-width: 1; }
svg .axis { stroke: var(--rule); stroke-width: 1; }
svg .hit { fill: transparent; cursor: default; }
svg .hit:focus { outline: none; }
svg [data-tip]:focus-visible { stroke: var(--fg); stroke-width: 1.5; }
svg .guide { fill: none; stroke: var(--muted); stroke-width: 1.2; stroke-dasharray: 2 5; }
/* kind encodings: measured = solid and filled; reported = hollow and dashed; estimated = dotted */
.k-m { fill: var(--k, var(--measured)); stroke: var(--k, var(--measured)); stroke-width: 1.5; }
.k-r { fill: var(--bg); stroke: var(--k, var(--reported)); stroke-width: 2; stroke-dasharray: 3 2; }
.k-e { fill: none; stroke: var(--k, var(--estimated)); stroke-width: 2; stroke-dasharray: 1 3; stroke-linecap: round; }
.k-n { fill: none; stroke: var(--muted); stroke-width: 1.5; stroke-dasharray: 3 3; }
.l-m { fill: none; stroke: var(--k, var(--measured)); stroke-width: 2.5; stroke-linejoin: round; }
.l-r { fill: none; stroke: var(--k, var(--reported)); stroke-width: 2; stroke-dasharray: 6 4; stroke-linejoin: round; }
.l-e { fill: none; stroke: var(--k, var(--estimated)); stroke-width: 2; stroke-dasharray: 1 4; stroke-linecap: round; }
.c-good { stroke: var(--good); stroke-width: 4; stroke-linecap: round; fill: none; }
.c-bad { stroke: var(--bad); stroke-width: 4; stroke-linecap: round; fill: none; }
.c-flat { stroke: var(--muted); stroke-width: 4; stroke-linecap: round; fill: none; }
.b-m { fill: var(--measured); }
.b-r { fill: var(--reported); fill-opacity: 0.25; stroke: var(--reported); stroke-width: 1.5; stroke-dasharray: 4 3; }
.b-d { fill: none; stroke: var(--muted); stroke-width: 2; stroke-dasharray: 1 3; stroke-linecap: round; }
.b-t { fill: var(--fg); fill-opacity: 0.12; stroke: var(--fg); stroke-width: 1.5; }
.h-good { fill: var(--good); }
.h-bad { fill: var(--bad); }
.h-none { fill: none; stroke: var(--muted); stroke-dasharray: 4 3; stroke-width: 1.2; }
.sw { display: inline-block; width: 12px; height: 12px; border-radius: 50%; background: var(--k, var(--muted)); }
/* load animation: connectors draw in, marks fade in, bars grow; staggered with --d */
@keyframes draw { from { stroke-dashoffset: 1; } to { stroke-dashoffset: 0; } }
@keyframes rise { from { opacity: 0; } to { opacity: 1; } }
@keyframes grow { from { transform: scaleX(0); } to { transform: scaleX(1); } }
.anim-draw { stroke-dasharray: 1; animation: draw 1.1s ease-out var(--d, 0s) both; }
.anim-fade { animation: rise 0.6s ease var(--d, 0s) both; }
.anim-grow { transform-box: fill-box; transform-origin: left center; animation: grow 0.9s cubic-bezier(0.2, 0.7, 0.2, 1) var(--d, 0s) both; }
@media (prefers-reduced-motion: reduce) {
  .anim-draw, .anim-fade, .anim-grow { animation: none; }
}
.cards { display: grid; grid-template-columns: repeat(auto-fill, minmax(240px, 1fr)); gap: 12px; margin-top: 0.4rem; }
.card { background: var(--panel); border: 1px solid var(--rule); border-radius: 10px; padding: 10px 12px; min-width: 0; }
.card ol { margin: 0.4em 0 0; padding-left: 1.2em; font-size: 0.85rem; color: var(--muted); }
.card ol li { overflow-wrap: anywhere; margin: 0.1em 0; }
.card ol b { color: var(--fg); font-weight: 600; }
.empty { border: 1px dashed var(--muted); border-radius: 10px; padding: 12px 14px; color: var(--muted); }
.empty b { color: var(--fg); }
.singles { margin: 0.4rem 0 0; padding-left: 1.2em; }
.singles li { margin: 0.25em 0; overflow-wrap: anywhere; }
.tip { position: fixed; z-index: 10; left: 0; top: 0; max-width: min(320px, calc(100vw - 16px)); pointer-events: none;
  background: var(--panel); color: var(--fg); border: 1px solid var(--rule); border-radius: 8px; padding: 8px 10px;
  font-size: 13px; line-height: 1.4; white-space: pre-line; overflow-wrap: anywhere;
  box-shadow: 0 6px 20px rgba(0, 0, 0, 0.18); }
footer { margin-top: 2.4rem; color: var(--muted); font-size: 0.85rem; }
.fb { fill: var(--k); }
.fb-x { fill: var(--panel); stroke: var(--k); stroke-width: 1.5; stroke-dasharray: 2 2; }
.k-x { fill: var(--panel); stroke: var(--measured); stroke-width: 1.5; stroke-dasharray: 2 2; }
.warnbox { border: 2px solid var(--reported); border-radius: 10px; padding: 10px 14px; margin: 0.6rem 0; background: var(--panel); }
.warnbox .warnhead { font-weight: 700; font-size: 1.05rem; margin: 0 0 0.3em; color: var(--fg); }
.warnbox ul { margin: 0.2em 0 0.4em; padding-left: 1.2em; font-size: 0.9rem; }
.tbl { margin: 0.6rem 0; font-size: 0.85rem; }
.tbl summary { cursor: pointer; color: var(--muted); }
.tbl table { border-collapse: collapse; margin-top: 0.4rem; font-variant-numeric: tabular-nums; }
.tbl th, .tbl td { border-bottom: 1px solid var(--rule); padding: 3px 8px; text-align: right; white-space: nowrap; }
.tbl th[scope="row"], .tbl thead th:first-child { text-align: left; }
"""


def esc(text):
    return html.escape(str(text), quote=True)


def num(v):
    """Exact value as written: integers with separators, floats keep their decimals."""
    return f"{v:,}"


def short(text, n):
    text = str(text)
    return text if len(text) <= n else text[: n - 3].rstrip() + "..."


def kcls(kind, prefix):
    return prefix + "-" + {"measured": "m", "reported": "r", "estimated": "e"}.get(kind, "n")


def tagcls(kind):
    return {"measured": "m", "reported": "r", "estimated": "e"}.get(kind, "n")


def tip(*lines):
    return "\n".join(str(x) for x in lines if x)


def point_tip(s, p):
    lines = [s["title"], f'{p["label"]}: {num(p["value"])} {s["unit"]} ({p["kind"]})']
    if p.get("source"):
        lines.append("source: " + p["source"])
    if p.get("note"):
        lines.append(p["note"])
    return "\n".join(lines)


def find(data, sid):
    return next((s for s in data["series"] if s["id"] == sid), None)


def is_pair(s):
    """A before/after series: first point is a base/before/main value and the series is not marked paired: false."""
    if len(s["points"]) < 2:
        return False
    if "paired" in s:
        return bool(s["paired"])
    return bool(PAIR_LABEL.match(s["points"][0]["label"]))


def make_scale(vals):
    """Return a dict with mode ('log' or 'linear'), a, b and fn(v) -> 0..1.

    Log scale only when max/min exceeds LOG_THRESHOLD; linear scales start at zero.
    """
    lo, hi = min(vals), max(vals)
    if lo > 0 and hi / lo > LOG_THRESHOLD:
        a = math.floor(math.log10(lo))
        b = math.ceil(math.log10(hi))
        if b == a:
            b += 1
        return {"mode": "log", "a": a, "b": b, "fn": lambda v: (math.log10(v) - a) / (b - a)}
    top = hi if hi > 0 else 1
    return {"mode": "linear", "a": 0, "b": top, "fn": lambda v: max(v, 0) / top}


def tick_text(v):
    if abs(v - round(v)) < 1e-9:
        return f"{round(v):,}"
    return f"{v:,.1f}"


def ratio_text(r, lower):
    if abs(r - 1) < 1e-9:
        return f"{r:.3f}x (no change)"
    if r == 0:
        return "0.000x (reached zero)"
    improved = (r < 1) if lower else (r > 1)
    if not improved:
        return f"{r:.3f}x (worse)"
    factor = max(r, 1 / r)
    return f"{r:.3f}x ({factor:.1f}x {'lower' if lower else 'higher'})"


def pretty_family(fam):
    return fam[4:].replace("-", " ") if fam.startswith("gb6-") else fam


# ---------------------------------------------------------------- family data for the scaling charts

def build_families(data, warnings):
    """family -> {'base': {N: info}, 'round6': {N: info}} from gb6-*-nN series and the table_* series."""
    fams = {}

    def add(fam, n, role, p, s):
        d = fams.setdefault(fam, {"base": {}, "round6": {}})
        slot = d[role]
        info = {"value": p["value"], "kind": p["kind"], "label": p["label"], "n": n, "series": s["title"],
                "tip": point_tip(s, p)}
        if n in slot:
            if slot[n]["value"] != p["value"]:
                warnings.append(f"{fam} N={n} {role}: two different values ({slot[n]['value']} and {p['value']})")
            return
        slot[n] = info

    for s in data["series"]:
        m = FAMILY_ID.match(s["id"])
        if m:
            fam, n = m.group(1), int(m.group(2))
            for p in s["points"]:
                if p["label"].startswith("base"):
                    add(fam, n, "base", p, s)
                elif p["label"].startswith("round6"):
                    add(fam, n, "round6", p, s)
        elif s["id"] in TABLE_FAMILY:
            fam = TABLE_FAMILY[s["id"]]
            for p in s["points"]:
                m2 = re.search(r"N=(\d+)", p["label"])
                if m2:
                    add(fam, int(m2.group(1)), "base", p, s)
    return fams


def slope_chart(fams):
    """Log-log cost against rows. Flat lines are cost that does not grow with N."""
    if not fams:
        return '<p class="empty"><b>not measured</b>: no families.</p>', ""
    W, H = 600, 330
    ML, MR, MT, MB = 78, 28, 22, 48
    PW, PH = W - ML - MR, H - MT - MB
    vals = [i["value"] for d in fams.values() for role in d for i in d[role].values()]
    lo = math.floor(math.log10(min(vals)))
    hi = math.ceil(math.log10(max(vals)))
    anchor = None
    for fam, d in fams.items():
        if 10 in d["base"]:
            anchor = d["base"][10]["value"]
            break
    if anchor:
        hi = max(hi, math.ceil(math.log10(anchor * 1000)))

    def xn(n):
        return ML + (math.log10(n) - 1) / 3 * PW

    def yv(v):
        return MT + PH - (math.log10(v) - lo) / (hi - lo) * PH

    out = [f'<svg class="wide" viewBox="0 0 {W} {H}" role="img" aria-label="Log-log cost against rows, by operation family">']
    out.append(f'<text class="lbl small" x="{ML}" y="{MT - 8}">instructions per operation (log scale)</text>')
    for k in range(lo, hi + 1):
        y = yv(10 ** k)
        out.append(f'<line class="grid" x1="{ML}" x2="{W - MR}" y1="{y:.1f}" y2="{y:.1f}"/>')
        out.append(f'<text class="lbl small end" x="{ML - 8}" y="{y + 4:.1f}">{num(10 ** k)}</text>')
    for n in (10, 100, 1000, 10000):
        x = xn(n)
        out.append(f'<line class="grid" x1="{x:.1f}" x2="{x:.1f}" y1="{MT}" y2="{MT + PH}"/>')
        out.append(f'<text class="lbl small mid" x="{x:.1f}" y="{MT + PH + 18}">{num(n)}</text>')
    out.append(f'<text class="lbl small mid" x="{ML + PW / 2:.1f}" y="{H - 6}">rows N (log scale)</text>')
    if anchor:
        x0, x1 = xn(10), xn(10000)
        out.append(f'<line class="guide" x1="{x0:.1f}" y1="{yv(anchor):.1f}" x2="{x1:.1f}" y2="{yv(anchor * 1000):.1f}"/>')
        out.append(f'<text class="lbl small end" x="{x1 - 6:.1f}" y="{yv(anchor * 1000) - 8:.1f}">slope 1: proportional to N</text>')

    legend = []
    for idx, (fam, d) in enumerate(fams.items()):
        style = f"--k: var(--c{idx % 5 + 1});"
        for role in ("base", "round6"):
            pts = sorted(d[role].items())
            if not pts:
                continue
            kind = "reported" if role == "base" else "measured"
            if len(pts) >= 2:
                path = " ".join(f"{'M' if j == 0 else 'L'}{xn(n):.1f} {yv(i['value']):.1f}" for j, (n, i) in enumerate(pts))
                out.append(f'<path class="{kcls(kind, "l")}" style="{style}" d="{path}"/>')
            for n, i in pts:
                cx, cy = xn(n), yv(i["value"])
                out.append(f'<circle class="{kcls(kind, "k")}" style="{style}" cx="{cx:.1f}" cy="{cy:.1f}" r="5"/>')
                out.append(f'<circle class="hit" data-tip="{esc(i["tip"])}" cx="{cx:.1f}" cy="{cy:.1f}" r="11" tabindex="0"/>')
        pretty = pretty_family(fam)
        base_n = sorted(d["base"]) or ["none"]
        r6_n = sorted(d["round6"]) or []
        missing = [n for n in (10, 100, 1000, 5000) if n not in d["round6"]] if r6_n else []
        note = "base only" if not r6_n else f"round6 at N={', '.join(str(n) for n in r6_n)}"
        if missing and r6_n:
            note += f"; round6 not measured at N={', '.join(str(n) for n in missing)}"
        legend.append(f'<li><span class="sw" style="--k: var(--c{idx % 5 + 1});"></span>'
                      f'<span><b>{esc(pretty)}</b> <span class="lbl">base N={", ".join(str(n) for n in base_n)}; {esc(note)}</span></span></li>')
    out.append("</svg>")
    return "\n".join(out), "<ul class=\"legend-list\">" + "".join(legend) + "</ul>"


def heat_grid(fams):
    cols = [10, 100, 1000, 5000]
    LBL, CW, RH, HDR = 190, 62, 46, 30
    W = LBL + CW * len(cols)
    H = HDR + RH * len(fams) + 4
    out = [f'<svg class="fit" viewBox="0 0 {W} {H}" role="img" aria-label="Heat grid of round6 over base ratio by operation and rows">']
    for j, n in enumerate(cols):
        out.append(f'<text class="lbl small mid" x="{LBL + j * CW + CW / 2:.1f}" y="18">N={num(n)}</text>')
    for r, (fam, d) in enumerate(fams.items()):
        y = HDR + r * RH
        out.append(f'<text x="0" y="{y + RH / 2 + 5:.1f}">{esc(short(pretty_family(fam), 26))}</text>')
        for j, n in enumerate(cols):
            x = LBL + j * CW + 2
            b, a = d["base"].get(n), d["round6"].get(n)
            cy = y + 2
            if b and a and b["value"] > 0:
                ratio = a["value"] / b["value"]
                cls = "h-good" if ratio < 1 else "h-bad"
                op = min(0.6, 0.18 + abs(math.log2(ratio)) * 0.6)
                t = tip(f"{pretty_family(fam)}, N={num(n)}",
                        f"base: {num(b['value'])} instructions ({b['kind']})",
                        f"round6: {num(a['value'])} instructions ({a['kind']})",
                        f"ratio round6 / base = {ratio:.3f}" + (" (improved)" if ratio < 1 else " (regression)"),
                        a.get("note", ""))
                out.append(f'<rect class="{cls}" style="fill-opacity: {op:.2f}" x="{x}" y="{cy}" width="{CW - 4}" height="{RH - 4}" rx="6" data-tip="{esc(t)}" tabindex="0"/>')
                out.append(f'<text class="mid" x="{x + (CW - 4) / 2:.1f}" y="{cy + RH / 2 + 1:.1f}">{ratio:.3f}x</text>')
            else:
                if b:
                    msg, t = ["round6", "not", "measured"], f"{pretty_family(fam)}, N={num(n)}\nbase: {num(b['value'])} ({b['kind']})\nround6: not measured"
                elif a:
                    msg, t = ["base", "not", "measured"], f"{pretty_family(fam)}, N={num(n)}\nround6: {num(a['value'])} ({a['kind']})\nbase: not measured"
                else:
                    msg, t = ["not", "measured"], f"{pretty_family(fam)}, N={num(n)}: not measured"
                out.append(f'<rect class="h-none" x="{x}" y="{cy}" width="{CW - 4}" height="{RH - 4}" rx="6" data-tip="{esc(t)}" tabindex="0"/>')
                step = 11
                top = cy + RH / 2 - (len(msg) - 1) * step / 2 + 4
                for k, word in enumerate(msg):
                    out.append(f'<text class="lbl small mid" x="{x + (CW - 4) / 2:.1f}" y="{top + k * step:.1f}">{word}</text>')
    out.append("</svg>")
    return "\n".join(out)


def dumbbell_panel(title, unit, rows):
    """rows: list of series (paired). First and last point of each series; ratio = last / first."""
    W, TOP, ROWH, AXH = 440, 6, 74, 26
    X0, X1 = 18, 422
    vals = [v for s in rows for v in (s["points"][0]["value"], s["points"][-1]["value"])]
    sc = make_scale(vals)
    H = TOP + ROWH * len(rows) + AXH
    out = [f'<svg class="fit" viewBox="0 0 {W} {H}" role="img" aria-label="{esc(title)}, before and after">']
    if sc["mode"] == "log":
        ticks = [10 ** k for k in range(sc["a"], sc["b"] + 1)]
    else:
        ticks = [0, sc["b"] / 2, sc["b"]]
    for t in ticks:
        x = X0 + sc["fn"](t) * (X1 - X0) if t > 0 or sc["mode"] == "log" else X0
        out.append(f'<line class="grid" x1="{x:.1f}" x2="{x:.1f}" y1="{TOP}" y2="{H - AXH}"/>')
        anchor = "start" if t == ticks[0] else ("end" if t == ticks[-1] else "mid")
        out.append(f'<text class="lbl small {anchor}" x="{x:.1f}" y="{H - 8}">{tick_text(t)}</text>')
    for i, s in enumerate(rows):
        y0 = TOP + i * ROWH
        first, last = s["points"][0], s["points"][-1]
        lower = s.get("lower_is_better", True)
        r = last["value"] / first["value"] if first["value"] else None
        notes = [p["note"] for p in (first, last) if p.get("note")]
        t = tip(s["title"], f'{first["label"]}: {num(first["value"])} {unit} ({first["kind"]})',
                f'{last["label"]}: {num(last["value"])} {unit} ({last["kind"]})',
                f"last / first = {r:.3f}" if r is not None else "",
                "source: " + "; ".join(sorted({p["source"] for p in (first, last) if p.get("source")})) if any(p.get("source") for p in (first, last)) else "",
                *notes)
        out.append(f'<rect class="hit" x="0" y="{y0}" width="{W}" height="{ROWH}" data-tip="{esc(t)}" tabindex="0"/>')
        out.append(f'<text x="{X0}" y="{y0 + 16}">{esc(short(s["title"], 52))}</text>')
        xa = X0 + sc["fn"](first["value"]) * (X1 - X0)
        xb = X0 + sc["fn"](last["value"]) * (X1 - X0)
        yt = y0 + 44
        if r is not None:
            improved = (r < 1) if lower else (r > 1)
            ccls = "c-flat" if abs(r - 1) < 1e-9 else ("c-good" if improved else "c-bad")
            d_ = f"M{xa:.1f} {yt} L{xb:.1f} {yt}"
            out.append(f'<path class="{ccls} anim-draw" style="--d: {0.15 + i * 0.12:.2f}s" pathLength="1" d="{d_}"/>')
            out.append(f'<text class="small mid" x="{(xa + xb) / 2:.1f}" y="{y0 + 33}">{esc(ratio_text(r, lower))}</text>')
        for p, x in ((first, xa), (last, xb)):
            out.append(f'<circle class="{kcls(p["kind"], "k")} anim-fade" style="--d: {0.3 + i * 0.12:.2f}s" cx="{x:.1f}" cy="{yt}" r="6.5"/>')
        out.append(f'<text class="lbl small" x="{X0}" y="{y0 + 65}">{esc(short(first["label"], 20))} {num(first["value"])}</text>')
        out.append(f'<text class="lbl small end" x="{X1}" y="{y0 + 65}">{esc(short(last["label"], 20))} {num(last["value"])}</text>')
    out.append("</svg>")
    scale_word = "log scale, each gridline is 10x" if sc["mode"] == "log" else "linear scale from zero"
    cap = (f"Unit: {esc(unit)}. {scale_word}. Filled dot = measured, hollow dashed = reported. "
           "Green connector = improved, orange = worse. Ratio = last / first.")
    return f'<h3>{esc(title)}</h3><p class="cap">{cap}</p>\n<div class="panel">\n' + "\n".join(out) + "\n</div>"


def waterfall(data):
    s = find(data, "memory_holders_counter")
    foot = find(data, "memory_footprint")
    total_pt = next((p for p in foot["points"] if p["label"] == "counter"), None) if foot else None
    if not s or not total_pt:
        return '<p class="empty"><b>not measured</b>: no decomposition of the counter footprint in data.json.</p>'
    total = total_pt["value"]
    holders = s["points"]
    hsum = round(sum(p["value"] for p in holders), 1)
    resid = round(total - hsum, 1)
    steps = [(p["label"], p["value"], "b-r", point_tip(s, p)) for p in holders]
    if abs(resid) > 1e-9:
        steps.append(("unattributed (derived here: footprint minus holders)", resid, "b-d",
                      f"derived, not in data.json: {num(total)} minus {num(hsum)} = {resid}\n"
                      f"counter footprint is reported in memory_footprint ({num(total)} {s['unit']})"))
    W, ROWH, TOP, X0, X1 = 440, 42, 6, 16, 424
    H = TOP + ROWH * (len(steps) + 1) + 24
    out = [f'<svg class="fit" viewBox="0 0 {W} {H}" role="img" aria-label="Waterfall of the counter footprint by holder">']
    cum = 0.0

    def xs(v):
        return X0 + max(v, 0) / total * (X1 - X0)

    for i, (label, delta, cls, t) in enumerate(steps):
        y0 = TOP + i * ROWH
        start, end = cum, cum + delta
        cum = end
        lo_, hi_ = min(start, end), max(start, end)
        xa, xb = xs(lo_), xs(hi_)
        out.append(f'<rect class="hit" x="0" y="{y0}" width="{W}" height="{ROWH}" data-tip="{esc(t)}" tabindex="0"/>')
        out.append(f'<text x="{X0}" y="{y0 + 14}">{esc(short(label, 46))}</text>')
        out.append(f'<text class="end small" x="{X1}" y="{y0 + 14}">{tick_text(delta)} {esc(s["unit"])}</text>')
        out.append(f'<rect class="{cls} anim-grow" style="--d: {0.1 + i * 0.1:.2f}s" x="{xa:.1f}" y="{y0 + 20}" width="{max(xb - xa, 2):.1f}" height="12" rx="3"/>')
    y0 = TOP + len(steps) * ROWH
    foot_tip = tip("counter footprint", total_pt["label"] + ": " + num(total) + " " + s["unit"] + " (" + total_pt["kind"] + ")", "source: memory_footprint")
    out.append(f'<rect class="hit" x="0" y="{y0}" width="{W}" height="{ROWH}" data-tip="{esc(foot_tip)}" tabindex="0"/>')
    out.append(f'<text x="{X0}" y="{y0 + 14}">counter footprint (memory_footprint)</text>')
    out.append(f'<text class="end small" x="{X1}" y="{y0 + 14}">{tick_text(total)} {esc(s["unit"])}</text>')
    out.append(f'<rect class="b-t anim-grow" style="--d: {0.1 + len(steps) * 0.1:.2f}s" x="{X0}" y="{y0 + 20}" width="{xs(total) - X0:.1f}" height="12" rx="3"/>')
    out.append(f'<text class="lbl small" x="{X0}" y="{H - 6}">0 to {tick_text(total)} {esc(s["unit"])}, linear</text>')
    out.append("</svg>")
    cap = ("Each bar starts where the one above ends. Hollow dashed = reported holders. "
           "The dotted bar is not in data.json: it is the gap between the footprint and the sum of the holders.")
    return f'<p class="cap">{cap}</p>\n<div class="panel">\n' + "\n".join(out) + "\n</div>"


def donut(counts):
    total = sum(counts.values()) or 1
    r, cx, cy, sw = 46, 60, 60, 16
    C = 2 * math.pi * r
    out = [f'<svg width="120" height="120" viewBox="0 0 120 120" role="img" aria-label="Points by kind">']
    out.append(f'<circle cx="{cx}" cy="{cy}" r="{r}" fill="none" style="stroke: var(--grid)" stroke-width="{sw}"/>')
    offset = 0.0
    for kind in KINDS:
        n = counts.get(kind, 0)
        if not n:
            continue
        seg = C * n / total
        out.append(f'<circle cx="{cx}" cy="{cy}" r="{r}" fill="none" style="stroke: var(--{kind})" '
                   f'stroke-width="{sw}" stroke-dasharray="{seg:.2f} {C - seg:.2f}" stroke-dashoffset="{-offset:.2f}" '
                   f'transform="rotate(-90 {cx} {cy})" data-tip="{esc(f"{kind}: {n} of {total} points")}" tabindex="0"/>')
        offset += seg
    out.append(f'<text x="{cx}" y="{cy + 5}" class="mid" style="font-size:18px;font-weight:600">{total}</text>')
    out.append("</svg>")
    return "\n".join(out)


def sparkline_card(s):
    pts = s["points"]
    vals = [p["value"] for p in pts]
    sc = make_scale(vals)
    connect = is_pair(s)
    W, H = 240, 92
    L, R, T, B = 10, 10, 12, 14
    n = len(pts)

    def px(i):
        return L + (i / (n - 1)) * (W - L - R) if n > 1 else W / 2

    def py(v):
        return T + (H - T - B) * (1 - sc["fn"](v))

    svg = [f'<svg class="fit" viewBox="0 0 {W} {H}" role="img" aria-label="{esc(s["title"])}">']
    svg.append(f'<line class="axis" x1="{L}" x2="{W - R}" y1="{H - B}" y2="{H - B}"/>')
    if connect and n > 1:
        d_ = " ".join(f"{'M' if i == 0 else 'L'}{px(i):.1f} {py(p['value']):.1f}" for i, p in enumerate(pts))
        svg.append(f'<path class="l-m anim-draw" style="--k: var(--lbl-line, var(--muted)); stroke-width: 1.8" pathLength="1" d="{d_}"/>')
    for i, p in enumerate(pts):
        svg.append(f'<circle class="{kcls(p["kind"], "k")} anim-fade" style="--d: {0.2 + i * 0.1:.2f}s" cx="{px(i):.1f}" cy="{py(p["value"]):.1f}" r="4.5" '
                   f'data-tip="{esc(point_tip(s, p))}" tabindex="0"/>')
    svg.append("</svg>")
    src = "; ".join(sorted({p["source"] for p in pts if p.get("source")}))
    head = [f'<h4>{esc(s["title"])}</h4>']
    cap = [esc(s["unit"])]
    if s.get("lower_is_better"):
        cap.append("lower is better")
    cap.append("connected in order" if connect else "dots, not a sequence")
    cap.append(f"{sc['mode']} scale" if n > 1 else "one point")
    head.append(f'<p class="cap">{" · ".join(cap)}</p>')
    body = ["\n".join(svg)]
    if n == 1:
        body.append(f'<p class="cap"><span class="tag n">not measured</span> no second point, so no before/after.</p>')
    items = []
    for p in pts:
        items.append(f'<li>{esc(p["label"])}: <b>{num(p["value"])}</b> <span class="tag {tagcls(p["kind"])}">{esc(p["kind"])}</span>'
                     + (f' <span>{esc(p["note"])}</span>' if p.get("note") else "") + "</li>")
    foot = ""
    if src:
        foot += f'<p class="cap">source: {esc(src)}</p>'
    if s.get("note"):
        foot += f'<p class="cap">{esc(s["note"])}</p>'
    return ('<article class="card">' + "".join(head) + "".join(body) + f"<ol>{''.join(items)}</ol>" + foot + "</article>")


def singles(data):
    rows = []
    for s in data["series"]:
        if len(s["points"]) == 1:
            p = s["points"][0]
            src = f' source: {esc(p["source"])}.' if p.get("source") else ""
            rows.append(f'<li><b>{esc(s["title"])}</b>: {num(p["value"])} {esc(s["unit"])} '
                        f'<span class="tag {tagcls(p["kind"])}">{esc(p["kind"])}</span>. '
                        f'<span class="tag n">before: not measured</span>{src}</li>')
    if not rows:
        return ""
    return '<h3>Single values</h3><p class="cap">These series have one point, so there is no before value to draw against.</p>' \
           '<ul class="singles">' + "".join(rows) + "</ul>"


def zortax_section(data):
    entries = data.get("not_measured", [])
    out = []
    for e in entries:
        out.append(f'<h2>{esc(e["group"])} <span class="tag n">{esc(e["status"])}</span></h2>')
        out.append('<div class="empty"><p><b>' + esc(e["status"]) + '.</b> ' + esc(e.get("note", "")) + "</p>"
                   '<p>Points in data.json: 0. Nothing is drawn for this section until a point is added.</p></div>')
        out.append('<div class="cards" style="grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));">')
        out.append('<article class="card"><h4>Radial comparison</h4><p class="cap">not measured</p>'
                   '<svg class="fit" viewBox="0 0 240 150" role="img" aria-label="Radial comparison: not measured">'
                   '<circle cx="120" cy="75" r="60" fill="none" style="stroke: var(--muted)" stroke-dasharray="4 4"/>'
                   '<circle cx="120" cy="75" r="30" fill="none" style="stroke: var(--grid)"/>'
                   '<text class="mid" x="120" y="80" style="fill: var(--muted)">not measured</text></svg></article>')
        out.append('<article class="card"><h4>Beeswarm against other frameworks</h4><p class="cap">not measured</p>'
                   '<svg class="fit" viewBox="0 0 240 150" role="img" aria-label="Beeswarm: not measured">'
                   '<line x1="16" x2="224" y1="110" y2="110" style="stroke: var(--muted)" stroke-dasharray="4 4"/>'
                   '<text class="mid" x="120" y="70" style="fill: var(--muted)">not measured</text></svg></article>')
        out.append("</div>")
    return "\n".join(out)


def nice_ticks(scale, n=4):
    """Tick values for a make_scale result: decades on a log scale, round steps from zero on a linear one."""
    if scale["mode"] == "log":
        return [10 ** e for e in range(scale["a"], scale["b"] + 1)]
    top = scale["b"]
    raw = top / n
    mag = 10 ** math.floor(math.log10(raw)) if raw > 0 else 1
    step = next(m * mag for m in (1, 2, 2.5, 5, 10) if m * mag >= raw)
    out, v = [], 0.0
    while v <= top * 1.0001:
        out.append(v)
        v += step
    return out


def dot_strip(title, unit, rows, note=""):
    """One metric across frameworks: a row per framework, a dot at its value (hollow and dashed when the report
    was invalid), a dash when there is no value. Single hue: identity is the row label, not a colour."""
    vals = [r[1]["value"] for r in rows if r[1]]
    if not vals:
        return ""
    sc = make_scale(vals)
    left, right, top, rh = 112, 22, 14, 22
    w = 420
    plot = w - left - right
    h = top + rh * len(rows) + 30
    order = sorted(rows, key=lambda r: (r[1] is None, r[1]["value"] if r[1] else 0))
    best = order[0][1]["value"] if order and order[0][1] else None
    out = [f'<svg class="fit" viewBox="0 0 {w} {h}" role="img" aria-label="{esc(title)} by framework, {esc(unit)}">']
    for t in nice_ticks(sc):
        x = left + plot * sc["fn"](t) if (sc["mode"] == "linear" or t > 0) else left
        out.append(f'<line class="grid" x1="{x:.1f}" x2="{x:.1f}" y1="{top - 6}" y2="{top + rh * len(rows)}"/>')
        out.append(f'<text class="lbl small mid" x="{x:.1f}" y="{top + rh * len(rows) + 16}">{esc(tick_text(t))}</text>')
    for i, (fw, v) in enumerate(order):
        y = top + rh * i + rh / 2
        out.append(f'<text class="lbl small end" x="{left - 10}" y="{y + 4:.1f}">{esc(fw)}</text>')
        if v is None:
            out.append(f'<text class="lbl small" x="{left + 4}" y="{y + 4:.1f}">no value</text>')
            continue
        x = left + plot * sc["fn"](v["value"])
        cls = "k-x" if v["invalid"] else "k-m"
        lab = f'{fw}: {v["value"]:,.2f} {unit}' + (" (from a report the harness marked invalid)" if v["invalid"] else "")
        out.append(f'<line class="grid" x1="{left}" x2="{x:.1f}" y1="{y:.1f}" y2="{y:.1f}"/>')
        out.append(f'<circle class="{cls} anim-fade" style="--d:{0.03 * i:.2f}s" cx="{x:.1f}" cy="{y:.1f}" r="5"/>')
        if v["value"] == best:
            out.append(f'<text class="small" x="{min(x + 9, w - 4):.1f}" y="{y + 4:.1f}">{v["value"]:,.1f}{"†" if v["invalid"] else ""}</text>')
        out.append(f'<rect class="hit" x="{left - 100}" y="{y - rh / 2:.1f}" width="{w - left + 100 - right + 20}" height="{rh}" tabindex="0" data-tip="{esc(lab)}"/>')
    out.append(f'<text class="lbl small" x="{left}" y="{h - 1}">{esc(unit)}{", log scale" if sc["mode"] == "log" else ""}, lower is better</text>')
    out.append("</svg>")
    return f'<article class="card"><h4>{esc(title)}</h4>{"<p class=cap>" + esc(note) + "</p>" if note else ""}' + "".join(out) + "</article>"


def zortax_quick1_section(data):
    z = data.get("zortax_quick1")
    if not z:
        return ""
    out = [f'<div class="warnbox"><p class="warnhead">{esc(z["label"])}</p><ul>']
    out += [f"<li>{esc(c)}</li>" for c in z["caveats"]]
    out.append(f'</ul><p class="small-note">Provenance: {esc(z["identity"])}. {esc(z["scenario_note"])}.</p></div>')
    out.append('<p class="cap">Filled dot: a valid report. Hollow dashed dot (†): the value comes from a report that failed the harness\'s validity checks on this busy host. The best value of each metric is labelled.</p>')
    out.append('<div class="cards" style="grid-template-columns: repeat(auto-fill, minmax(300px, 1fr));">')
    for mi, m in enumerate(z["metrics"]):
        rows = [(f["framework"], f["values"][mi]) for f in z["frameworks"]]
        out.append(dot_strip(m["name"], m["unit"], rows))
    out.append("</div>")
    # table view
    th = "".join(f'<th>{esc(m["name"])} ({esc(m["unit"])})</th>' for m in z["metrics"])
    trs = []
    for f in z["frameworks"]:
        tds = "".join("<td>" + ("no value" if v is None else f'{v["value"]:,.2f}{" †" if v["invalid"] else ""}') + "</td>" for v in f["values"])
        trs.append(f'<tr><th scope="row">{esc(f["framework"])}</th>{tds}</tr>')
    out.append(f'<details class="tbl"><summary>Table view</summary><div class="scroll"><table><thead><tr><th>framework</th>{th}</tr></thead><tbody>{"".join(trs)}</tbody></table></div></details>')
    return "\n".join(out)


def workloads_section(data):
    """The standing workloads: per scene, instructions per step (p50) by phase as bars on a shared log scale, the
    wall time per step beside them, and the model and identity of the run."""
    wl = data.get("workloads")
    if not wl:
        return ""
    out = [f'<p class="cap">{esc(wl["note"])}</p>', f'<p class="small-note">Provenance: {esc(wl["identity"])}</p>']
    allv = [r["instr_p50"] for sc in wl["scenes"] for r in sc["phases"] if r.get("instr_p50", 0) > 0]
    sc_ = make_scale(allv) if allv else None
    for sc in wl["scenes"]:
        rows = sc["phases"]
        left, right, top, rh = 104, 150, 8, 24
        w = 640
        plot = w - left - right
        h = top + rh * len(rows) + 28
        svg = [f'<svg class="fit" viewBox="0 0 {w} {h}" role="img" aria-label="{esc(sc["scene"])}: instructions per step by phase">']
        if sc.get("failed"):
            out.append(f'<h3>{esc(sc["scene"])} <span class="tag n">{esc(sc["failed"])}</span></h3>')
        if sc_:
            for t in nice_ticks(sc_):
                x = left + plot * sc_["fn"](t)
                svg.append(f'<line class="grid" x1="{x:.1f}" x2="{x:.1f}" y1="{top - 4}" y2="{top + rh * len(rows)}"/>')
                svg.append(f'<text class="lbl small mid" x="{x:.1f}" y="{top + rh * len(rows) + 16}">{esc(short_num(t))}</text>')
        for i, r in enumerate(rows):
            y = top + rh * i
            v = r.get("instr_p50", 0)
            svg.append(f'<text class="lbl small end" x="{left - 8}" y="{y + 16}">{esc(r["phase"])}</text>')
            if v > 0 and sc_:
                bw = max(plot * sc_["fn"](v), 2)
                tipt = tip(f'{sc["scene"]}, {r["phase"]} ({r["steps"]} steps)', f'instructions per step: p50 {v:,}, p95 {r.get("instr_p95", 0):,}',
                           f'wall per step: p50 {r.get("wall_us_p50", 0):,.1f} µs, p95 {r.get("wall_us_p95", 0):,.1f} µs', f'footprint {r.get("footprint_mib", 0):,.1f} MiB')
                svg.append(f'<rect class="b-m anim-grow" style="--d:{0.04 * i:.2f}s" x="{left}" y="{y + 5}" width="{bw:.1f}" height="{rh - 10}" rx="3"/>')
                svg.append(f'<rect class="hit" x="0" y="{y}" width="{w}" height="{rh}" tabindex="0" data-tip="{esc(tipt)}"/>')
            svg.append(f'<text class="small" x="{w - right + 8}" y="{y + 16}">{esc(short_num(v))} · {esc(ms_text(r.get("wall_us_p50", 0)))}</text>')
        svg.append(f'<text class="lbl small" x="{left}" y="{h - 1}">instructions per step (p50, log scale) · right: p50 instructions · p50 wall</text>')
        svg.append("</svg>")
        out.append(f'<article class="panel"><h4>{esc(sc["scene"])}: {esc(sc["model"])}</h4><p class="cap">{esc(sc.get("note", ""))}</p>' + "".join(svg) + "</article>")
    th = "<th>scene</th><th>phase</th><th>steps</th><th>instr p50</th><th>instr p95</th><th>wall p50 µs</th><th>wall p95 µs</th><th>footprint MiB</th>"
    trs = []
    for sc in wl["scenes"]:
        for r in sc["phases"]:
            trs.append(f'<tr><th scope="row">{esc(sc["scene"])}</th><td>{esc(r["phase"])}</td><td>{r["steps"]}</td><td>{r.get("instr_p50", 0):,}</td><td>{r.get("instr_p95", 0):,}</td>'
                       f'<td>{r.get("wall_us_p50", 0):,.1f}</td><td>{r.get("wall_us_p95", 0):,.1f}</td><td>{r.get("footprint_mib", 0):,.1f}</td></tr>')
    out.append(f'<details class="tbl"><summary>Table view</summary><div class="scroll"><table><thead><tr>{th}</tr></thead><tbody>{"".join(trs)}</tbody></table></div></details>')
    return "\n".join(out)


def short_num(v):
    v = float(v)
    for div, suf in ((1e9, " G"), (1e6, " M"), (1e3, " K")):
        if abs(v) >= div:
            return f"{v / div:.3g}{suf}"
    return f"{v:.3g}"


def ms_text(us):
    us = float(us)
    if us >= 1e6:
        return f"{us / 1e6:.3g} s"
    return f"{us / 1000:.3g} ms" if us >= 1000 else f"{us:.3g} µs"



FW_TOKEN = {"bough": "--f1", "egui": "--f2", "iced": "--f3", "gpui-kit": "--f4"}


def fmt_ms(v):
    if v >= 1000:
        return f"{v / 1000:.2f} s"
    if v >= 100:
        return f"{v:.0f} ms"
    if v >= 10:
        return f"{v:.1f} ms"
    return f"{v:.2f} ms"


def frameworks_scenes_section(data):
    fs = data.get("frameworks_scenes")
    if not fs:
        return ""
    fws = fs["frameworks"]
    out = []
    out.append('<p class="cap">The same scene in each framework, drawn from one shared spec (layout, colours, data, keys) and '
               'measured by gui-bench\'s runner under fakeland: input to presented frame, p50 over the probe\'s repeats. One session '
               'on one host, cells in sequence. Hollow dashed bars come from a report that failed a validity check of the harness '
               '(here always <code>wake_lateness</code>: a few compositor wakes later than 2 ms on the shared host).</p>')
    out.append('<div class="warnbox"><p class="warnhead">Renderers differ</p><ul>'
               + "".join(f'<li><span class="sw" style="--k: var({FW_TOKEN[f]})"></span> <b>{esc(f)}</b>: {esc(fs["renderer"][f])}</li>' for f in fws)
               + '</ul><p class="small-note">ilo-prod has no GPU. bough draws with its CPU executor on one thread; the others rasterise '
               'through lavapipe, which spreads the work over many threads (144-386 % CPU in the canvas). Provenance: '
               + esc(" · ".join(fs["identity"])) + '.</p></div>')
    cells = fs["cells"]
    order = sorted(cells, key=lambda k: (k.split("|")[0] != "sheet", int(k.split("|")[1])))
    for key in order:
        scene, size = key.split("|")
        byfw = cells[key]
        mets = fs["metrics"]
        vals = [byfw[f][f"{m['probe']}.{m['metric']}"]["value"] for f in fws if f in byfw for m in mets if f"{m['probe']}.{m['metric']}" in byfw[f]]
        vals = [v for v in vals if v > 0]
        if not vals:
            continue
        sc = make_scale(vals + [max(vals) * 1.3])
        left, right, top, bh, bg, gg = 190, 120, 8, 12, 2, 14
        w = 640
        plot = w - left - right
        gh = len(fws) * (bh + bg) - bg
        h = top + len(mets) * (gh + gg) + 24
        svg = [f'<svg class="fit" viewBox="0 0 {w} {h}" role="img" aria-label="{esc(scene)} at {esc(size)}: latency by framework">']
        for t in nice_ticks(sc):
            x = left + plot * sc["fn"](t)
            svg.append(f'<line class="grid" x1="{x:.1f}" x2="{x:.1f}" y1="{top - 4}" y2="{h - 20}"/>')
            svg.append(f'<text class="lbl small mid" x="{x:.1f}" y="{h - 6}">{esc(fmt_ms(t))}</text>')
        for mi, m in enumerate(mets):
            y0 = top + mi * (gh + gg)
            svg.append(f'<text class="small end" x="{left - 10}" y="{y0 + gh / 2 + 4:.1f}">{esc(m["label"])}</text>')
            for fi, f in enumerate(fws):
                y = y0 + fi * (bh + bg)
                cell = byfw.get(f, {}).get(f"{m['probe']}.{m['metric']}")
                if not cell:
                    svg.append(f'<text class="lbl small" x="{left + 4}" y="{y + bh - 1}">{esc(f)}: not measured</text>')
                    continue
                v = cell["value"]
                # a dot on a log axis (bars would misstate ratios there), filled when the report is valid
                x = left + plot * sc["fn"](v)
                style = f"--k: var({FW_TOKEN[f]})"
                cls = "fb" if cell["valid"] else "fb-x"
                tipt = tip(f"{scene} {size}, {m['label']}", f"{f}: {fmt_ms(v)}" + ("" if cell["valid"] else f" (report failed: {cell['failed']})"), fs["renderer"][f])
                cy = y + bh / 2
                svg.append(f'<line class="grid" x1="{left}" x2="{x:.1f}" y1="{cy:.1f}" y2="{cy:.1f}"/>')
                svg.append(f'<circle class="{cls} anim-fade" style="{style}; --d:{0.02 * (mi * 4 + fi):.2f}s" cx="{x:.1f}" cy="{cy:.1f}" r="4.5"/>')
                svg.append(f'<text class="small" x="{x + 8:.1f}" y="{y + bh}">{esc(fmt_ms(v))} <tspan class="lbl">{esc(f)}</tspan></text>')
                svg.append(f'<rect class="hit" x="0" y="{y - 1}" width="{w}" height="{bh + bg}" tabindex="0" data-tip="{esc(tipt)}"/>')
        svg.append(f'<text class="lbl small" x="{left}" y="{h - 20 + 14}"></text>')
        svg.append("</svg>")
        # the table: the app's own share, memory, idle CPU, scroll
        th = "<th>framework</th>" + "".join(f"<th>{esc(e['label'])}</th>" for e in fs["extra"])
        trs = []
        for f in fws:
            tds = []
            for e in fs["extra"]:
                c = byfw.get(f, {}).get(f"{e['probe']}.{e['metric']}")
                if not c:
                    tds.append("<td>-</td>")
                    continue
                v = c["value"]
                if e["metric"].endswith("rss_max"):
                    txt = f"{v / 1e6:,.0f} MB"
                elif e["metric"].endswith("cpu_ms_per_s"):
                    txt = f"{v:,.2f} ms/s"
                elif "smoothness" in e["metric"]:
                    txt = f"{v:.0f}"
                else:
                    txt = fmt_ms(v)
                tds.append(f"<td>{txt}{'' if c['valid'] else ' †'}</td>")
            trs.append(f'<tr><th scope="row"><span class="sw" style="--k: var({FW_TOKEN[f]})"></span> {esc(f)}</th>{"".join(tds)}</tr>')
        ref = fs.get("macref", {}).get(key)
        refline = ""
        if ref:
            def ph(n):
                return fmt_ms(ref[n][1] / 1000) if n in ref else "-"
            refline = (f'<p class="small-note">bough on the Mac for reference (M3 Pro, Metal, bough\'s own harness, re-describing mode; '
                       f'wall time per step including the GPU, not input to presented frame, so not on the chart\'s axis): '
                       f'hover {ph("hover")}, theme {ph("theme")}, edit {ph("steady")}, resize {ph("resize")}, mount {ph("mount")}.</p>')
        label = {"sheet": f"Sheet, {int(size):,} rows ({int(size) * 10:,} cells)", "canvas": f"Canvas, {int(size):,} nodes"}.get(scene, f"{scene} {size}")
        out.append(f'<article class="panel"><h4>{esc(label)}</h4>' + "".join(svg)
                   + f'<details class="tbl"><summary>More: the app\'s own share, memory, idle CPU (table)</summary><div class="scroll"><table><thead><tr>{th}</tr></thead><tbody>{"".join(trs)}</tbody></table></div></details>'
                   + refline + "</article>")
    out.append('<p class="small-note">† from a report that failed a validity check. Ports and every deviation from the spec: '
               '<code>comparisons/gui-bench-scenes/DEVIATIONS-*.md</code>; source: <code>' + esc(fs["source"]) + '</code>.</p>')
    return "\n".join(out)


SCRIPT = """
(function () {
  "use strict";
  var tip = document.createElement("div");
  tip.className = "tip";
  tip.setAttribute("role", "tooltip");
  tip.hidden = true;
  document.body.appendChild(tip);
  var timer = null;

  function place(x, y) {
    var w = tip.offsetWidth, h = tip.offsetHeight, gap = 14;
    var left = x + gap, top = y + gap;
    if (left + w > window.innerWidth - 8) left = x - w - gap;
    if (top + h > window.innerHeight - 8) top = y - h - gap;
    tip.style.left = Math.max(8, left) + "px";
    tip.style.top = Math.max(8, top) + "px";
  }
  function hide() { tip.hidden = true; }
  function showFor(el, x, y) {
    var text = el.getAttribute("data-tip");
    if (!text) { hide(); return; }
    tip.textContent = text;
    tip.hidden = false;
    place(x, y);
  }
  function target(e) {
    return e.target && e.target.closest ? e.target.closest("[data-tip]") : null;
  }
  document.addEventListener("pointerover", function (e) {
    var el = target(e);
    if (el) { showFor(el, e.clientX, e.clientY); } else { hide(); }
  });
  document.addEventListener("pointermove", function (e) {
    if (!tip.hidden) { place(e.clientX, e.clientY); }
  });
  document.addEventListener("pointerdown", function (e) {
    var el = target(e);
    clearTimeout(timer);
    if (el) {
      showFor(el, e.clientX, e.clientY);
      timer = setTimeout(hide, 2600);
    } else {
      hide();
    }
  });
  document.addEventListener("focusin", function (e) {
    var el = target(e);
    if (!el) { return; }
    var r = el.getBoundingClientRect();
    showFor(el, r.left + r.width / 2, r.top);
  });
  document.addEventListener("focusout", hide);
  document.addEventListener("scroll", hide, true);
})();
"""


def build_body(data):
    warnings = []
    fams = build_families(data, warnings)
    counts = {k: 0 for k in KINDS}
    n_points = 0
    for s in data["series"]:
        for p in s["points"]:
            counts[p["kind"]] += 1
            n_points += 1
    not_measured = data.get("not_measured", [])

    # Scaling section
    slope_svg, slope_legend = slope_chart(fams)

    # Before and after: one dumbbell panel per (group, unit) of paired series
    panels = []
    seen = []
    for s in data["series"]:
        key = (s["group"], s["unit"])
        if key not in seen and any(is_pair(t) for t in data["series"] if (t["group"], t["unit"]) == key):
            seen.append(key)
    for group, unit in seen:
        rows = [t for t in data["series"] if (t["group"], t["unit"]) == (group, unit) and is_pair(t)]
        panels.append(dumbbell_panel(f"{group} · {unit}", unit, rows))

    # Series cards grouped
    groups = {}
    for s in data["series"]:
        groups.setdefault(s["group"], []).append(s)
    card_groups = []
    for g, items in groups.items():
        card_groups.append(f'<h3>{esc(g)}</h3>\n<div class="cards">' + "".join(sparkline_card(s) for s in items) + "</div>")

    tiles = [
        f'<div class="tile"><b>{len(data["series"])}</b><span>series</span></div>',
        f'<div class="tile"><b>{n_points}</b><span>points</span></div>',
        f'<div class="tile"><b>{counts["measured"]}</b><span>measured</span></div>',
        f'<div class="tile"><b>{counts["reported"]}</b><span>reported</span></div>',
        f'<div class="tile"><b>{len(not_measured)}</b><span>section not measured</span></div>',
    ]
    donut_tile = f'<div class="tile donut">{donut(counts)}<div><b>{counts["measured"]} of {n_points}</b><span>points measured</span></div></div>'

    legend = """<div class="panel"><ul class="legend-list">
<li><svg width="26" height="14" aria-hidden="true"><rect class="b-m" x="1" y="1" width="24" height="12" rx="3"/></svg><span><b>measured</b>: a number someone produced with the stated method. Solid, filled.</span></li>
<li><svg width="26" height="14" aria-hidden="true"><rect class="b-r" x="1" y="1" width="24" height="12" rx="3"/></svg><span><b>reported</b>: an agent's report not yet replayed by anyone else. Hollow, dashed.</span></li>
<li><svg width="26" height="14" aria-hidden="true"><rect class="b-d" x="1" y="1" width="24" height="12" rx="3"/></svg><span><b>estimated</b>: a derived or projected figure. Dotted. None in data.json yet.</span></li>
<li><svg width="26" height="14" aria-hidden="true"><rect fill="none" style="stroke: var(--muted)" stroke-dasharray="3 3" x="1" y="1" width="24" height="12" rx="3"/></svg><span><b>not measured</b>: an explicit empty state. Nothing is invented to fill it.</span></li>
</ul></div>"""

    heat = heat_grid(fams)
    wf = waterfall(data)
    sing = singles(data)
    zortax = zortax_section(data)
    zq1 = zortax_quick1_section(data)
    fwscenes = frameworks_scenes_section(data)
    workloads = workloads_section(data)

    body = f"""<main>
<h1>bough, rounds 6 and 7</h1>
<p class="meta">Updated {esc(data.get("updated", ""))} · {len(data["series"])} series, {n_points} points · every chart is generated from data.json by build.py</p>
<div class="tiles">{"".join(tiles)}{donut_tile}</div>
{legend}

<h2>Standing workloads (round 7) <span class="tag m">measured</span></h2>
{workloads}

<h2>Scaling with rows</h2>
<p class="cap">Log-log. Each line is one operation family at N rows: dashed hollow = base (reported), solid filled = round6 (measured). A flat line means the cost does not grow with N. The dotted guide is slope 1, cost proportional to N.</p>
<div class="scroll">{slope_svg}</div>
{slope_legend}
<h3>Round 6 over base, by cell</h3>
<p class="cap">Ratio = round6 / base for each operation and N, lower is better. Green = improved, orange = regression; deeper colour = larger change. Cells with no pair say not measured.</p>
<div class="panel">{heat}</div>

<h2>Before and after</h2>
<p class="cap">Each row is one before/after series, drawn from its first point to its last point, with the ratio on the connector.</p>
{"".join(panels)}
{sing}

<h2>Where the counter's footprint goes</h2>
{wf}

<h2>Every series over commits</h2>
<p class="cap">One card per series, grouped as in data.json. Connected lines are before/after or commit sequences; dots are scenes or values with no order. Scale is log when the range exceeds 50x, otherwise linear from zero.</p>
{"".join(card_groups)}

<h2>The heavy scenes in four frameworks (ilo-prod, fakeland, software rendering) <span class="tag m">measured</span></h2>
{fwscenes}

<h2>zortax gui-bench under fakeland <span class="tag n">rough baseline only</span></h2>
{zq1}
{zortax}

<footer>Generated by build.py from data.json · {len(data["series"])} series · {n_points} points · no external sources or scripts beyond this page.</footer>
</main>
<script>{SCRIPT}</script>
"""
    return body, warnings


def page_parts(data):
    body, warnings = build_body(data)
    head_style = "<style>" + CSS_TOKENS + CSS_BODY + "</style>"
    return body, head_style, warnings


def validate(data):
    if not isinstance(data.get("series"), list):
        raise SystemExit("data.json: 'series' must be a list")
    seen = set()
    for s in data["series"]:
        sid = s.get("id")
        if not sid or sid in seen:
            raise SystemExit(f"data.json: missing or duplicate series id {sid!r}")
        seen.add(sid)
        for key in ("group", "title", "unit"):
            if not s.get(key):
                raise SystemExit(f"data.json: series {sid!r} has no '{key}'")
        for p in s.get("points", []):
            v = p.get("value")
            if isinstance(v, bool) or not isinstance(v, (int, float)):
                raise SystemExit(f"data.json: series {sid!r} point {p.get('label')!r} has a non-numeric value")
            if p.get("kind") not in KINDS:
                raise SystemExit(f"data.json: series {sid!r} point {p.get('label')!r} has bad kind {p.get('kind')!r}")
    for e in data.get("not_measured", []):
        if not e.get("group") or not e.get("status"):
            raise SystemExit("data.json: not_measured entries need 'group' and 'status'")


def main():
    with open(DATA_PATH, encoding="utf-8") as f:
        data = json.load(f)
    validate(data)
    body, head_style, warnings = page_parts(data)
    for w in warnings:
        print(f"warning: {w}", file=sys.stderr)
    head_title = f"<title>{esc(TITLE)}</title>\n<meta name=\"claude:viewport-fit-sides\" content=\"cover\">\n"
    artifact = head_title + head_style + "\n" + body
    with open(ARTIFACT_PATH, "w", encoding="utf-8") as f:
        f.write(artifact)
    index = (
        '<!doctype html>\n<html lang="en">\n<head>\n<meta charset="utf-8">\n'
        '<meta name="viewport" content="width=device-width, initial-scale=1">\n'
        f"<title>{esc(TITLE)}</title>\n{head_style}\n</head>\n<body>\n{body}</body>\n</html>\n"
    )
    with open(INDEX_PATH, "w", encoding="utf-8") as f:
        f.write(index)
    n_points = sum(len(s["points"]) for s in data["series"])
    print(f"Wrote {ARTIFACT_PATH} ({len(artifact.encode('utf-8')):,} bytes) and {INDEX_PATH}: "
          f"{len(data['series'])} series, {n_points} points")


if __name__ == "__main__":
    sys.exit(main())
