"""Round 8 sections of the dashboard, built from data.r8.json (tools/collect.py).

Structure follows the way areweactuallyguiyet.mespin.me presents a comparison: a sticky top bar with section links, run
metadata (date, profile, machine) above the charts, framework toggles that show and hide frameworks in every chart,
one compact panel per metric with a row per framework, a "Values" table under each group, a per-framework card list
and a method and caveat section. Our own title, wording and look.

Every framework has a value in every cell:
  measured and valid by the harness's own checks  -> filled dot
  measured, a harness check failed (invalid)      -> hollow dashed dot, dagger, never ranked
  no measurement                                  -> estimate: hollow dotted diamond with a dotted range, "~" in text,
                                                     method in the tooltip, drawn below the measured rows and never ranked
"""
import html, json, math, os, re

HERE = os.path.dirname(os.path.abspath(__file__))

def esc(t):
    return html.escape(str(t), quote=True)

BOUGH = "bough"
CSS = """
.topbar { position: sticky; top: env(safe-area-inset-top, 0px); z-index: 9; background: var(--panel); border-bottom: 1px solid var(--rule);
  padding-block: 8px; padding-left: max(16px, env(safe-area-inset-left, 0px)); padding-right: max(16px, env(safe-area-inset-right, 0px)); }
.topbar .in { max-width: 1100px; margin: 0 auto; display: flex; flex-wrap: wrap; gap: 6px 18px; align-items: baseline; }
.topbar b { font-size: 0.95rem; letter-spacing: 0.01em; }
.topbar nav { display: flex; flex-wrap: wrap; gap: 4px 14px; font-size: 0.88rem; }
.topbar a { color: var(--muted); text-decoration: none; padding: 2px 0; border-bottom: 2px solid transparent; }
.topbar a:hover, .topbar a:focus-visible { color: var(--fg); border-bottom-color: var(--fg); outline: none; }
.topbar a.on { color: var(--fg); border-bottom-color: var(--measured); }
main.wide { max-width: 1100px; }
section { scroll-margin-top: 64px; }
.runmeta { display: grid; grid-template-columns: repeat(auto-fit, minmax(210px, 1fr)); gap: 6px 20px; margin: 0.4rem 0 0.6rem; font-size: 0.86rem; }
.runmeta div { min-width: 0; overflow-wrap: anywhere; }
.runmeta dt { color: var(--muted); font-size: 0.78rem; text-transform: uppercase; letter-spacing: 0.05em; }
.runmeta dd { margin: 0; font-variant-numeric: tabular-nums; }
.toggles { display: flex; flex-wrap: wrap; gap: 6px; margin: 0.4rem 0 0.2rem; align-items: center; }
.toggles button { font: inherit; font-size: 0.84rem; border: 1px solid var(--rule); background: var(--panel); color: var(--fg); border-radius: 999px; padding: 2px 11px; cursor: pointer; }
.toggles button[aria-pressed="false"] { color: var(--muted); text-decoration: line-through; background: var(--bg); }
.toggles button.all { border-style: dashed; }
.toggles button:focus-visible { outline: 2px solid var(--fg); outline-offset: 1px; }
.toggles button.bo { border-color: var(--measured); }
.keyrow { display: flex; flex-wrap: wrap; gap: 6px 22px; font-size: 0.85rem; margin: 0.5rem 0; align-items: center; }
.keyrow span { display: inline-flex; gap: 7px; align-items: center; }
.panels { display: grid; grid-template-columns: repeat(auto-fill, minmax(min(100%, 420px), 1fr)); gap: 12px; align-items: start; }
.mp { background: var(--panel); border: 1px solid var(--rule); border-radius: 10px; padding: 10px 12px 8px; min-width: 0; }
.mp h4 { font-size: 0.92rem; margin: 0; }
.mp .sub { color: var(--muted); font-size: 0.8rem; margin: 0 0 4px; }
.mp svg.fit { width: 100%; height: auto; }
.row .nm { font-size: 12.5px; }
.row.bo .nm { font-weight: 700; }
.dot-ok { fill: var(--ink); stroke: var(--panel); stroke-width: 2; }
.row.bo .dot-ok { fill: var(--measured); }
.dot-inv { fill: var(--panel); stroke: var(--muted); stroke-width: 2; stroke-dasharray: 3 2; }
.dot-est { fill: var(--panel); stroke: var(--estimated); stroke-width: 2; stroke-dasharray: 1.5 2.5; stroke-linecap: round; }
.rng-est { stroke: var(--estimated); stroke-width: 2; stroke-dasharray: 1 3; stroke-linecap: round; opacity: 0.9; }
.rng-run { stroke: var(--muted); stroke-width: 3; stroke-linecap: round; opacity: 0.5; }
.estbg { fill: var(--estimated); opacity: 0.07; }
.estlbl { fill: var(--estimated); font-size: 11px; font-style: italic; }
.gl { stroke: var(--grid); stroke-width: 1; }
.vsm { font-size: 11.5px; fill: var(--muted); }
.seg0 { fill: var(--c1); } .seg1 { fill: var(--c2); } .seg2 { fill: var(--c3); } .seg3 { fill: var(--c3); } .seg4 { fill: var(--muted); }
.sw-sq.seg0 { background: var(--c1); } .sw-sq.seg1 { background: var(--c2); } .sw-sq.seg3 { background: var(--c3); } .sw-sq.seg4 { background: var(--muted); }
.sw-sq { display: inline-block; width: 12px; height: 12px; border-radius: 2px; flex: none; }
.vt { width: 100%; border-collapse: collapse; font-size: 0.82rem; font-variant-numeric: tabular-nums; }
.vt th, .vt td { border-bottom: 1px solid var(--rule); padding: 3px 7px; text-align: right; white-space: nowrap; }
.vt th:first-child, .vt td:first-child { text-align: left; position: sticky; left: 0; background: var(--bg); }
.vt thead th { position: sticky; top: 0; background: var(--bg); font-weight: 600; white-space: normal; min-width: 80px; vertical-align: bottom; }
.vt td.inv { color: var(--muted); border: 1px dashed var(--muted); }
.vt td.est { color: var(--estimated); font-style: italic; border: 1px dotted var(--estimated); background: color-mix(in srgb, var(--estimated) 7%, transparent); }
.vt td.unk { color: var(--muted); font-style: normal; border: 1px solid var(--muted); border-radius: 3px; }
.dot-unk { fill: var(--ink); fill-opacity: 0.3; stroke: var(--ink); stroke-width: 1.5; }
.row.bo .dot-unk { fill: var(--measured); stroke: var(--measured); }
.vt td.miss { color: var(--muted); }
.vt tr.bo td:first-child, .vt tr.bo th:first-child { font-weight: 700; }
.fwcards { display: grid; grid-template-columns: repeat(auto-fill, minmax(min(100%, 300px), 1fr)); gap: 10px; }
.fwc { background: var(--panel); border: 1px solid var(--rule); border-radius: 10px; padding: 10px 12px; font-size: 0.85rem; min-width: 0; }
.fwc h4 { margin: 0 0 4px; font-size: 1rem; }
.fwc dl { margin: 0; display: grid; grid-template-columns: auto 1fr; gap: 2px 10px; }
.fwc dt { color: var(--muted); } .fwc dd { margin: 0; overflow-wrap: anywhere; font-variant-numeric: tabular-nums; }
.pill { display: inline-block; font-size: 0.74rem; border-radius: 999px; padding: 0 8px; border: 1px solid var(--rule); color: var(--muted); }
.pill.bad { border-color: var(--bad); color: var(--bad); } .pill.good { border-color: var(--good); color: var(--good); }
.pill.est { border-color: var(--estimated); color: var(--estimated); border-style: dotted; }
.note-box { border: 1px solid var(--rule); background: var(--panel); border-radius: 10px; padding: 10px 14px; margin: 0.6rem 0; font-size: 0.9rem; }
.note-box.warn { border: 2px solid var(--reported); }
.note-box p { margin: 0.3em 0; }
.note-box ul { margin: 0.2em 0 0.3em; padding-left: 1.2em; }
.loadspark { display: block; width: 100%; max-width: 520px; height: auto; }
.pending { border: 1px dashed var(--muted); border-radius: 10px; padding: 12px 14px; color: var(--muted); }
.pending b { color: var(--fg); }
.mono { font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; font-size: 0.82em; }
details.raw pre { max-height: 360px; overflow: auto; font-size: 0.75rem; background: var(--panel); border: 1px solid var(--rule); border-radius: 8px; padding: 8px; }
.flame { max-width: 100%; height: auto; border: 1px solid var(--rule); border-radius: 8px; background: #fff; }
.symtab td:first-child, .symtab th:first-child { white-space: normal; max-width: 420px; overflow-wrap: anywhere; position: static; background: transparent; }
.earlier-note { margin: 2.4rem 0 0.4rem; }
"""

# ------------------------------------------------------------------ small helpers
def fmtv(v, nd=2):
    if v is None:
        return "-"
    if abs(v) >= 1000:
        return f"{v:,.0f}"
    if abs(v) >= 100:
        return f"{v:,.1f}"
    return f"{v:,.{nd}f}"

def scale_for(vals, higher_better=False):
    vals = [v for v in vals if v is not None and v > 0]
    if not vals:
        return {"mode": "linear", "lo": 0, "hi": 1, "fn": lambda v: 0}
    lo, hi = min(vals), max(vals)
    if not higher_better and hi / lo > 6:
        a = math.floor(math.log10(lo)); b = math.ceil(math.log10(hi))
        if b == a: b += 1
        return {"mode": "log", "a": a, "b": b, "fn": lambda v: (math.log10(max(v, 10 ** a)) - a) / (b - a)}
    top = hi * 1.05
    return {"mode": "linear", "a": 0, "b": top, "fn": lambda v: max(v, 0) / top}

def ticks(sc):
    if sc["mode"] == "log":
        t = [10.0 ** e for e in range(sc["a"], sc["b"] + 1)]
        # add 2x and 5x ticks when the decade span is short
        if sc["b"] - sc["a"] <= 2:
            t = sorted(t + [m * 10.0 ** e for e in range(sc["a"], sc["b"]) for m in (2, 5)])
        return t
    top = sc["b"]; raw = top / 4; mag = 10 ** math.floor(math.log10(raw)) if raw > 0 else 1
    step = next(m * mag for m in (1, 2, 2.5, 5, 10) if m * mag >= raw)
    out, v = [], 0.0
    while v <= top * 1.0001:
        out.append(v); v += step
    return out

def tick_txt(v):
    if v >= 1000:
        return f"{v/1000:g}K"
    if v >= 1 or v == 0:
        return f"{v:g}"
    return f"{v:g}"

EXE = {}        # run name -> {framework: sha256[:16]} (from the identity record), set by build_sections
EST_NOTE = [""]  # one-sentence validation result of the estimate method for the view being built

def exe_line(run, fw):
    h = (EXE.get(run) or {}).get(fw)
    return f"executable sha256 {h} (manifest in Frameworks); behaviour gate: the runner's layout check passed, no other pixel check" if h else ""

def tipv(fw, d, unit, nd, run, kind, est=None, run2=None, spread=None):
    L = []
    if kind == "est":
        L.append(f"{fw}: ~{fmtv(est['v'], nd)} {unit}  ESTIMATE (heuristic), not a measurement")
        L.append(f"stated range {fmtv(est['lo'], nd)} to {fmtv(est['hi'], nd)} {unit}: not a confidence interval")
        L.append("reason for the estimate: " + est.get("reason", "no measurement"))
        if d.get("msg"):
            L.append(d["msg"])
            if "failed" in d["msg"]:
                L.append("a failure is a result: the real value is probably worse than the model suggests")
        L.append("method: " + est["note"])
        L.append(EST_NOTE[0])
        L.append("never ranked with measured values; a missing cell is not evidence that the framework supports the workload")
    elif kind == "inv":
        L.append(f"{fw}: {fmtv(d['v'], nd)} {unit}  INVALID report ({run})")
        L.append("harness check failed: " + (d.get("msg") or ", ".join(d.get("fail", []))))
        L.append("shown as measured but excluded from ranking and from the estimate model")
        L.append(exe_line(run, fw))
    elif kind == "unk":
        L.append(f"{fw}: {fmtv(d['v'], nd)} {unit}  UNCHECKED ({run})")
        L.append(d.get("msg") or "no validity record for this value")
        L.append("shown, never ranked; counted as valid by nobody. Startup in a run taken under load is load-contaminated.")
        L.append(exe_line(run, fw))
    elif kind == "ok":
        L.append(f"{fw}: {fmtv(d['v'], nd)} {unit}  valid ({run})")
        if d.get("warn"):
            L.append("harness warnings: " + ", ".join(d["warn"]) + " (report still valid)")
        if d.get("variant") and d["variant"] != "default" or d.get("variant") == "default" and False:
            L.append(f"variant run: {d['variant']}")
        if run2 and run2.get("v") is not None:
            L.append(f"second run: {fmtv(run2['v'], nd)} {unit} ({'valid' if run2['st']=='ok' else 'invalid'})" + (f", spread {spread*100:.0f}%" if spread is not None else ""))
        L.append(exe_line(run, fw))
    else:
        L.append(f"{fw}: no measurement and no basis for an estimate" + (f"\n{d['msg']}" if d.get("msg") else ""))
    return "\n".join(x for x in L if x)

K_OK, K_UNK, K_INV, K_EST, K_NONE = 0, 1, 2, 3, 4
ST_K = {"ok": K_OK, "unk": K_UNK, "inv": K_INV}

# ------------------------------------------------------------------ the metric panel (one SVG, rows re-laid out by JS)
def panel(col_id, col, fws, run_name, higher_better=False, second=None):
    unit, nd = col["unit"], col["nd"]
    rows = []
    for fw in fws:
        d = col["vals"].get(fw, {"v": None, "st": "miss"})
        est = (col.get("est") or {}).get(fw)
        if d["st"] in ST_K:
            rows.append((ST_K[d["st"]], fw, d, None))
        elif est:
            rows.append((K_EST, fw, d, est))
        else:
            rows.append((K_NONE, fw, d, None))
    key = (lambda r: -r[2]["v"]) if higher_better else (lambda r: r[2]["v"] if r[2]["v"] is not None else 0)
    keyest = (lambda r: -r[3]["v"]) if higher_better else (lambda r: r[3]["v"])
    rows.sort(key=lambda r: (r[0], keyest(r) if r[0] == K_EST else (key(r) if r[0] < K_EST else 0), r[1]))
    allv = []
    for k, fw, d, est in rows:
        if d["v"] is not None and k < K_EST: allv.append(d["v"])
        if est: allv += [est["lo"], est["hi"], est["v"]]
        r2 = d.get("run2")
        if r2 and r2.get("v") is not None and r2["st"] == "ok" and k == K_OK: allv.append(r2["v"])
    if not allv:
        return ""
    sc = scale_for(allv, higher_better)
    W, left, right, top, rh = 440, 108, 54, 8, 22
    plot = W - left - right
    X = lambda v: left + plot * sc["fn"](v)
    best = next((r for r in rows if r[0] == K_OK), None)
    out = [f'<svg class="fit mpsvg" viewBox="0 0 {W} {top + rh * len(rows) + 36}" data-top="{top}" data-rh="{rh}" role="img" aria-label="{esc(col["label"])} by framework">']
    out.append('<g class="gridg">')
    for t in ticks(sc):
        if sc["mode"] == "log" and (t < 10.0 ** sc["a"] * 0.999 or t > 10.0 ** sc["b"] * 1.001): continue
        x = X(t) if t > 0 else left
        out.append(f'<line class="gl" x1="{x:.1f}" x2="{x:.1f}" y1="{top - 4}" y2="{top + rh * len(rows)}"/>')
    out.append("</g>")
    for i, (k, fw, d, est) in enumerate(rows):
        y = top + rh * i
        cy = rh / 2
        cls = "row bo" if fw == BOUGH else "row"
        out.append(f'<g class="{cls}" data-fw="{esc(fw)}" data-k="{k}" transform="translate(0,{y})">')
        if k == K_EST:
            out.append(f'<rect class="estbg" x="0" y="0" width="{W}" height="{rh}"/>')
        out.append(f'<text class="nm end" x="{left - 8}" y="{cy + 4}">{esc(fw)}</text>')
        if k == K_OK:
            x = X(d["v"])
            r2 = d.get("run2")
            if r2 and r2.get("v") is not None and r2["st"] == "ok":
                x2 = X(r2["v"])
                out.append(f'<line class="rng-run" x1="{min(x, x2):.1f}" x2="{max(x, x2):.1f}" y1="{cy}" y2="{cy}"/>')
            out.append(f'<circle class="dot-ok" cx="{x:.1f}" cy="{cy}" r="5.5"/>')
            if r2 and r2.get("v") is not None and r2["st"] == "ok":
                out.append(f'<circle class="dot-ok" cx="{X(r2["v"]):.1f}" cy="{cy}" r="3" style="opacity:.55"/>')
            if fw == BOUGH or (best and fw == best[1]):
                out.append(f'<text class="vsm" x="{min(x + 9, W - 4):.1f}" y="{cy + 4}">{esc(fmtv(d["v"], nd))}</text>')
            tp = tipv(fw, d, unit, nd, run_name, "ok", run2=r2, spread=d.get("spread"))
        elif k == K_UNK:
            x = X(d["v"])
            out.append(f'<circle class="dot-unk" cx="{x:.1f}" cy="{cy}" r="5.5"/>')
            if fw == BOUGH:
                out.append(f'<text class="vsm" x="{min(x + 9, W - 4):.1f}" y="{cy + 4}">{esc(fmtv(d["v"], nd))}?</text>')
            tp = tipv(fw, d, unit, nd, run_name, "unk")
        elif k == K_INV:
            x = X(d["v"])
            out.append(f'<circle class="dot-inv" cx="{x:.1f}" cy="{cy}" r="5"/>')
            if fw == BOUGH:
                out.append(f'<text class="vsm" x="{min(x + 9, W - 4):.1f}" y="{cy + 4}">{esc(fmtv(d["v"], nd))}†</text>')
            tp = tipv(fw, d, unit, nd, run_name, "inv")
        elif k == K_EST:
            x, lo, hi = X(est["v"]), X(est["lo"]), X(est["hi"])
            out.append(f'<line class="rng-est" x1="{lo:.1f}" x2="{hi:.1f}" y1="{cy}" y2="{cy}"/>')
            out.append(f'<path class="dot-est" d="M{x:.1f} {cy - 6} L{x + 6:.1f} {cy} L{x:.1f} {cy + 6} L{x - 6:.1f} {cy} Z"/>')
            rs = est.get("reason", "")
            tag = " cell failed" if "failed" in rs else (" not ported" if "ported" in rs else "")
            out.append(f'<text class="estlbl" x="{min(hi + 6, W - (84 if tag else 52)):.1f}" y="{cy + 4}">~{esc(fmtv(est["v"], nd))}{tag}</text>')
            tp = tipv(fw, d, unit, nd, run_name, "est", est=est)
        else:
            out.append(f'<text class="vsm" x="{left + 4}" y="{cy + 4}">no value, no basis</text>')
            tp = tipv(fw, d, unit, nd, run_name, "none")
        out.append(f'<rect class="hit" x="0" y="0" width="{W}" height="{rh}" tabindex="0" data-tip="{esc(tp)}"/>')
        out.append("</g>")
    ay = top + rh * len(rows)
    out.append(f'<g class="axisg" transform="translate(0,{ay})">')
    for t in ticks(sc):
        if sc["mode"] == "log" and (t < 10.0 ** sc["a"] * 0.999 or t > 10.0 ** sc["b"] * 1.001): continue
        x = X(t) if t > 0 else left
        out.append(f'<text class="lbl small mid" x="{x:.1f}" y="14">{esc(tick_txt(t))}</text>')
    out.append(f'<text class="lbl small" x="{left}" y="31">{esc(unit)}{", log scale" if sc["mode"] == "log" else ""}, {"higher" if higher_better else "lower"} is better</text>')
    out.append("</g></svg>")
    return (f'<article class="mp" data-col="{esc(col_id)}"><h4>{esc(col["label"])}</h4><p class="sub">{esc(unit)} · '
            f'{"higher" if higher_better else "lower"} is better</p>' + "".join(out) + "</article>")

# ------------------------------------------------------------------ values table (every cell, every framework)
def values_table(cols, fws, ids):
    th = "".join(f'<th scope="col">{esc(cols[i]["label"])}<br><span class="small-note">{esc(cols[i]["unit"])}</span></th>' for i in ids)
    trs = []
    for fw in fws:
        tds = []
        for i in ids:
            c = cols[i]; d = c["vals"].get(fw, {"v": None, "st": "miss"}); est = (c.get("est") or {}).get(fw)
            nd = c["nd"]
            if d["st"] == "ok":
                vmark = "&nbsp;‡" if c.get("variant_spec") and c.get("variant_spec") != "default" and d.get("variant") not in (None, c.get("variant_spec")) else ""
                tds.append(f'<td title="{esc(tipv(fw, d, c["unit"], nd, "", "ok"))}">{fmtv(d["v"], nd)}{vmark}</td>')
            elif d["st"] == "unk":
                tds.append(f'<td class="unk" title="{esc(tipv(fw, d, c["unit"], nd, "", "unk"))}">{fmtv(d["v"], nd)}&nbsp;?</td>')
            elif d["st"] == "inv":
                tds.append(f'<td class="inv" title="{esc(tipv(fw, d, c["unit"], nd, "", "inv"))}">{fmtv(d["v"], nd)}&nbsp;†</td>')
            elif est:
                tds.append(f'<td class="est" title="{esc(tipv(fw, d, c["unit"], nd, "", "est", est=est))}">~{fmtv(est["v"], nd)}{" (cell failed)" if "failed" in (d.get("msg") or "") else ""}</td>')
            else:
                tds.append('<td class="miss">-</td>')
        trs.append(f'<tr class="{"bo" if fw == BOUGH else ""}"><th scope="row">{esc(fw)}</th>{"".join(tds)}</tr>')
    return (f'<details class="tbl"><summary>Values ({len(fws)} frameworks, {len(ids)} metrics)</summary><div class="scroll"><table class="vt"><thead><tr><th scope="col">framework</th>{th}</tr></thead>'
            f'<tbody>{"".join(trs)}</tbody></table></div><p class="small-note">Plain = valid. Dashed and † = the harness marked that report invalid. Italic with ~ = estimate (hover for the method). Estimates never rank.</p></details>')

def toggles(fws):
    b = "".join(f'<button type="button" class="{"bo" if f == BOUGH else ""}" data-fw="{esc(f)}" aria-pressed="true">{esc(f)}</button>' for f in fws)
    return f'<div class="toggles" role="group" aria-label="Frameworks shown in every chart">{b}<button type="button" class="all" data-all="1">Show all</button></div>'

def keyrow(second=False):
    k = KEYROW
    if not second:
        i = k.index('<span><svg width="30" height="14" aria-hidden="true"><line class="rng-run"')
        k = k[:i] + "</div>"
    return k

KEYROW = """<div class="keyrow" aria-label="Legend">
<span><svg width="20" height="14" aria-hidden="true"><circle class="dot-ok" cx="10" cy="7" r="5.5"/></svg>valid measurement (the harness's own checks passed; bough in blue)</span>
<span><svg width="20" height="14" aria-hidden="true"><circle class="dot-inv" cx="10" cy="7" r="5"/></svg>measured, report invalid (†): shown, never ranked</span>
<span><svg width="20" height="14" aria-hidden="true"><circle class="dot-unk" cx="10" cy="7" r="5.5"/></svg>unchecked (?): no validity check exists for it (startup); shown, never ranked</span>
<span><svg width="30" height="14" aria-hidden="true"><line class="rng-est" x1="2" x2="28" y1="7" y2="7"/><path class="dot-est" d="M15 1 L21 7 L15 13 L9 7 Z"/></svg>estimate with its likely range (~): no measurement exists, method on hover</span>
<span><svg width="30" height="14" aria-hidden="true"><line class="rng-run" x1="4" x2="26" y1="7" y2="7"/><circle class="dot-ok" cx="4" cy="7" r="3" style="opacity:.55"/><circle class="dot-ok" cx="26" cy="7" r="5"/></svg>two runs (cmp-2 and cmp-3): the bar is the run-to-run spread</span>
</div>"""

def runmeta(run, meta_extra=None):
    ident = run["identity"]
    d = {}
    for l in ident:
        for part in l.replace(" (", "\x00(").split(" "):
            if "=" in part and not part.startswith("("):
                k, _, v = part.partition("=")
                d.setdefault(k, v)
    load = run["load"]
    ls = [v for _, v in load]
    rows = [
        ("run", run["name"]),
        ("host", f'{d.get("host", "?")} · {d.get("kernel", "?")}'),
        ("cpu", next((l.split("=", 1)[1].split(" threads")[0] for l in ident if l.startswith("cpu=")), "?")),
        ("governor", next((l.split("governors=")[1].split(" boost")[0] for l in ident if "governors=" in l), "?")),
        ("started", d.get("start", "?")),
        ("load during", f"mean {sum(ls)/len(ls):.1f}, min {min(ls):.1f}, max {max(ls):.1f} (1-min load, n={len(ls)})" if ls else "?"),
        ("renderers", "bough: CPU executor into wl_shm · others: wgpu or tiny-skia/vello on Mesa lavapipe (software) · compositor: fakeland (pixman)"),
        ("harness", "zortax gui-bench 0fd45107 + fakeland 71695e7c, " + next((l.split("args=")[1] for l in ident if l.startswith("args=")), "") + ", timing " + (re.search(r"GB_TIMING=(\S+)", " ".join(ident)).group(1) if re.search(r"GB_TIMING=(\S+)", " ".join(ident)) else "fixed 60 Hz")),
    ]
    return '<dl class="runmeta">' + "".join(f"<div><dt>{esc(k)}</dt><dd>{esc(v)}</dd></div>" for k, v in rows) + "</dl>"

def loadspark(load, name):
    if not load: return ""
    W, H = 520, 90
    vals = [v for _, v in load]; mx = max(max(vals), 16)
    pts = " ".join(f"{20 + (W - 30) * i / max(len(vals) - 1, 1):.1f},{H - 18 - (H - 30) * v / mx:.1f}" for i, v in enumerate(vals))
    y16 = H - 18 - (H - 30) * 16 / mx
    return (f'<svg class="loadspark" viewBox="0 0 {W} {H}" role="img" aria-label="1-minute load average of ilo-prod during {esc(name)}">'
            f'<line class="gl" x1="20" x2="{W-10}" y1="{y16:.1f}" y2="{y16:.1f}"/><text class="lbl small" x="22" y="{y16-3:.1f}">16 = every thread busy</text>'
            f'<polyline points="{pts}" fill="none" style="stroke: var(--reported)" stroke-width="2"/>'
            f'<text class="lbl small" x="20" y="{H-3}">{esc(load[0][0])} UTC</text><text class="lbl small end" x="{W-10}" y="{H-3}">{esc(load[-1][0])} UTC</text></svg>')


# ------------------------------------------------------------------ latency split and startup breakdown
SPLIT_EST = {"hover3k": ("hover3k", "hover3k_c"), "hover100": ("hover100", "hover100_c"), "tbl_remove": ("tbl_remove", "tbl_remove_c"),
             "tbl_update": ("tbl_update", "tbl_update_c"), "tbl_swap": ("tbl_swap", "tbl_swap_c"), "search": ("search", "search_c"),
             "typing": ("typing", "typing_h"), "resize": ("resize", "resize_c")}
SPLIT_NOTE = {
    "lat": "segments: input to commit (the app) · commit to ready · ready to latch (waiting for the compositor) · latch to present",
    "typing": "segments: handling (the app, the harness's handling_p50) · the rest of the total",
    "resize": "segments: configure to commit (the app) · commit to present",
}

def split_panel(pid, sp, cols, fws, run_name):
    kind = sp["kind"]
    rows = []
    tcol, ccol = SPLIT_EST.get(pid, (None, None))
    for fw in fws:
        c = sp["fws"][fw]
        t = c["total"]
        if t["st"] in ("ok", "inv") and t["v"] is not None:
            rows.append((0 if t["st"] == "ok" else 1, fw, c, None))
        else:
            et = (cols.get(tcol, {}).get("est") or {}).get(fw) if tcol else None
            ec = (cols.get(ccol, {}).get("est") or {}).get(fw) if ccol else None
            rows.append((2 if et else 3, fw, c, (et, ec)))
    rows.sort(key=lambda r: (r[0], (r[2]["total"]["v"] if r[0] < 2 else (r[3][0]["v"] if r[0] == 2 else 0)) or 0, r[1]))
    allv = []
    for k, fw, c, e in rows:
        if k < 2: allv.append(c["total"]["v"])
        elif k == 2: allv += [e[0]["lo"], e[0]["hi"]]
    if not allv: return ""
    sc = scale_for(allv, False)
    sc = {"mode": "linear", "a": 0, "b": max(allv) * 1.08, "fn": lambda v, m=max(allv) * 1.08: max(v, 0) / m}
    W, left, right, top, rh = 440, 108, 54, 8, 22
    plot = W - left - right
    X = lambda v: left + plot * sc["fn"](v)
    out = [f'<svg class="fit mpsvg" viewBox="0 0 {W} {top + rh * len(rows) + 36}" data-top="{top}" data-rh="{rh}" role="img" aria-label="{esc(sp["label"])} latency split by framework">']
    out.append('<g class="gridg">')
    for t in ticks(sc):
        out.append(f'<line class="gl" x1="{X(t):.1f}" x2="{X(t):.1f}" y1="{top-4}" y2="{top + rh * len(rows)}"/>')
    out.append("</g>")
    for i, (k, fw, c, e) in enumerate(rows):
        y = top + rh * i
        out.append(f'<g class="{"row bo" if fw == BOUGH else "row"}" data-fw="{esc(fw)}" data-k="{k}" transform="translate(0,{y})">')
        if k == 2: out.append(f'<rect class="estbg" x="0" y="0" width="{W}" height="{rh}"/>')
        out.append(f'<text class="nm end" x="{left-8}" y="{rh/2+4}">{esc(fw)}</text>')
        if k < 2:
            tot = c["total"]["v"]; inv = k == 1
            parts = []
            if kind == "lat":
                parts = [("input to commit", c["commit"]["v"], "seg0"), ("commit to ready", c["c2r"]["v"], "seg1"), ("ready to latch (compositor wait)", c["latch"]["v"], "seg4"), ("latch to present", c["present"]["v"], "seg3")]
            elif kind == "typing":
                cm = c["commit"]["v"]
                parts = [("handling (app)", cm, "seg0"), ("after handling", (tot - cm) if cm is not None else None, "seg4")]
            else:
                cm = c["commit"]["v"]
                parts = [("configure to commit (app)", cm, "seg0"), ("commit to present", (tot - cm) if cm is not None else None, "seg4")]
            x0 = left; tl = [f"{fw}: total p50 {tot:.2f} ms" + ("  INVALID report" if inv else "") + f"  ({run_name})"]
            if inv and c["total"].get("msg"): tl.append(c["total"]["msg"])
            used = 0
            for lab, v, cl in parts:
                if v is None or v < 0.0005: continue
                w = plot * v / (sc["b"])
                out.append(f'<rect class="{cl}" x="{x0:.1f}" y="4" width="{max(w - 1.5, 0.6):.1f}" height="{rh-8}" rx="2" style="{"opacity:.45" if inv else ""}"/>')
                x0 += w; used += v; tl.append(f"{lab}: {v:.2f} ms")
            if kind == "lat": tl.append(f"pooled percentiles do not add exactly; sum of parts {used:.2f} ms")
            tx = X(tot)
            out.append(f'<text class="vsm" x="{tx+5:.1f}" y="{rh/2+4}">{tot:.1f}{"†" if inv else ""}</text>')
            tp = "\n".join(tl)
        elif k == 2:
            et, ec = e
            xt = X(et["v"]); xl, xh = X(et["lo"]), X(et["hi"])
            if ec:
                out.append(f'<rect class="dot-est" x="{left}" y="4" width="{max(X(ec["v"]) - left, 1):.1f}" height="{rh-8}" rx="2" style="stroke-dasharray:2 2"/>')
            out.append(f'<line class="rng-est" x1="{xl:.1f}" x2="{xh:.1f}" y1="{rh/2}" y2="{rh/2}"/>')
            out.append(f'<path class="dot-est" d="M{xt:.1f} {rh/2-6} L{xt+6:.1f} {rh/2} L{xt:.1f} {rh/2+6} L{xt-6:.1f} {rh/2} Z"/>')
            out.append(f'<text class="estlbl" x="{min(xh+6, W-50):.1f}" y="{rh/2+4}">~{et["v"]:.1f}</text>')
            tp = "\n".join([f"{fw}: ~{et['v']:.2f} ms total  ESTIMATE, not a measurement", f"likely range {et['lo']:.2f} to {et['hi']:.2f} ms"] + ([f"app share ~{ec['v']:.2f} ms (dotted box)"] if ec else []) + ["method: " + et["note"]])
        else:
            out.append(f'<text class="vsm" x="{left+4}" y="{rh/2+4}">no value, no basis</text>'); tp = f"{fw}: no measurement and no basis for an estimate"
        out.append(f'<rect class="hit" x="0" y="0" width="{W}" height="{rh}" tabindex="0" data-tip="{esc(tp)}"/></g>')
    ay = top + rh * len(rows)
    out.append(f'<g class="axisg" transform="translate(0,{ay})">')
    for t in ticks(sc):
        out.append(f'<text class="lbl small mid" x="{X(t):.1f}" y="14">{esc(tick_txt(t))}</text>')
    out.append(f'<text class="lbl small" x="{left}" y="31">ms, p50 of pooled samples, lower is better</text></g></svg>')
    return f'<article class="mp"><h4>{esc(sp["label"])}</h4><p class="sub">{esc(SPLIT_NOTE[kind])}</p>' + "".join(out) + "</article>"

SPLIT_CAVEAT = '<p class="small-note"><b>How to read the split.</b> Each segment is the median of its own pooled samples. Medians of separately pooled phases do not add up to the median of the total (a part can even exceed it), so the bars show the parts the harness records, not an exact decomposition, and the percentiles above p50 compose even less. Paired per-frame samples are not kept. An unthrottled run still contains protocol and buffer scheduling and is neither pure app cost nor input-to-photon.</p>'
SPLIT_KEY = """<div class="keyrow" aria-label="Segments"><span><span class="sw-sq seg0"></span>input to commit (the app's share)</span>
<span><span class="sw-sq seg1"></span>commit to ready</span><span><span class="sw-sq seg4"></span>ready to latch (waiting for the compositor)</span><span><span class="sw-sq seg3"></span>latch to present</span></div>"""

def split_section(data):
    sp = data.get("splits")
    if not sp: return ""
    cols = data["headline"]["cols"]; fws = data["headline"]["fws"]; run = data["headline"]["run"]
    unth = data.get("key") == "unthrottled"
    out = ['<h3>Latency split: the app and the wait</h3>',
           ('<p class="cap"><b>Unthrottled, the compositor presents when the app commits, so there is no latch wait and the split collapses to the app\'s share.</b> The bars below show what the harness records; the parts after input to commit are about 0.03 ms.</p>' if unth else
           '<p class="cap"><b>Under a fixed 60 Hz output the compositor latches once per frame, so the wait for the latch is about 10 ms for every framework (9.3 to 11 ms here) and hides the app\'s share.</b> '
           'For bough rows-3,000 hover, 11.52 ms total is 0.93 ms in the app, 9.58 ms waiting for the latch and 1.00 ms from latch to present. The stacked bars below show the parts the harness records; the panels after them show the app\'s share (input to commit) and the CPU per frame alone.</p>'),
           SPLIT_KEY, SPLIT_CAVEAT]
    for pid in ("hover3k", "hover100", "tbl_remove", "tbl_update", "tbl_swap", "search", "typing", "resize"):
        if pid in sp:
            out.append(split_panel(pid, sp[pid], cols, fws, run))
    return out[0] + out[1] + out[2] + out[3] + '<div class="panels">' + "".join(out[4:]) + "</div>"

def startup_section(data):
    su = data.get("startups"); h = data.get("headline")
    if not su or not h: return ""
    fws = h["fws"]
    out = ["<h3>Startup breakdown (spawn to first present)</h3>"]
    # callout: virtual 100,000 (startup has no validity check: nothing here is ranked)
    c = h["cols"]["startv"]
    vals = sorted(((d["v"], fw) for fw, d in c["vals"].items() if d["st"] in ("ok", "unk") and d["v"] is not None and fw != BOUGH))
    b = c["vals"].get(BOUGH, {})
    if b.get("v") is not None:
        bs = su["virtual100k"]["fws"].get(BOUGH)
        brk = ""
        if bs:
            brk = (f' bough: {bs["connected_ms"]:.0f} ms before the app connects to the compositor, {bs["first_buffer_ms"] - bs["connected_ms"]:.0f} ms from connect to first buffer, '
                   f'{bs["first_present_ms"] - bs["first_buffer_ms"]:.0f} ms from first buffer to first present.')
        cross = ""
        if data.get("key") == "fixed":
            oth = []
            for k in ("unthrottled", "vrr"):
                mm = (data.get("modes") or {}).get(k)
                if mm and mm.get("headline"):
                    bv = mm["headline"]["cols"]["startv"]["vals"].get(BOUGH, {}); bs0 = mm["headline"]["cols"]["spin0_start"]["vals"].get(BOUGH, {})
                    if bv.get("v") and bs0.get("v"):
                        oth.append(f'{mm["headline"]["run"]}: virtual 100,000 {bv["v"]:.0f} ms, spinner without rows {bs0["v"]:.0f} ms')
            sp0 = h["cols"]["spin0_start"]["vals"].get(BOUGH, {}).get("v")
            if oth and sp0:
                cross = (f' <b>This view\'s startup is load-contaminated:</b> cmp-2 was taken unpinned while the host load rose to 21. The same bough cells in the later pinned runs read {"; ".join(oth)}; '
                         f'here the spinner without rows (nothing to mount) took {sp0:.0f} ms. The other frameworks also moved 2 to 5 times between runs.')
        nxt = ", ".join(f"{fw} {v:.1f}" for v, fw in vals[:2])
        out.append(f'<div class="note-box warn"><p><b>Virtual list at 100,000 rows: bough {b["v"]:.1f} ms, unchecked.</b> The lowest other values: {nxt} ms. '
                   f'Startup has no harness validity check, so every startup value is marked unchecked and none is ranked; read these as orientation.{brk}{cross} floem has no measurement.</p></div>')
    W, left, right, rh = 460, 108, 60, 22
    panels = []
    for pid in ("virtual100k", "rows3k", "spinner0"):
        sd = su[pid]; rows = []
        for fw in fws:
            d = sd["fws"][fw]
            rows.append((0 if d else 3, fw, d))
        rows.sort(key=lambda r: (r[0], r[2]["first_present_ms"] if r[2] else 0, r[1]))
        mx = max((r[2]["first_present_ms"] for r in rows if r[2]), default=1) * 1.08
        X = lambda v: left + (W - left - right) * v / mx
        top = 8
        svg = [f'<svg class="fit mpsvg" viewBox="0 0 {W} {top+rh*len(rows)+36}" data-top="{top}" data-rh="{rh}" role="img" aria-label="Startup breakdown, {esc(sd["label"])}"><g class="gridg">']
        for t in ticks({"mode": "linear", "a": 0, "b": mx}):
            svg.append(f'<line class="gl" x1="{X(t):.1f}" x2="{X(t):.1f}" y1="{top-4}" y2="{top+rh*len(rows)}"/>')
        svg.append("</g>")
        for i, (k, fw, d) in enumerate(rows):
            svg.append(f'<g class="{"row bo" if fw == BOUGH else "row"}" data-fw="{esc(fw)}" data-k="{k}" transform="translate(0,{top+rh*i})"><text class="nm end" x="{left-8}" y="{rh/2+4}">{esc(fw)}</text>')
            if d:
                con, fb, fp, tl, fs = d["connected_ms"], d["first_buffer_ms"], d["first_present_ms"], d["toplevel_ms"], d["first_surface_ms"]
                segs = [("spawn to compositor connect", con, "seg4")]
                if fb is not None and fb >= con: segs.append(("connect to first buffer (build, layout, first frame)", fb - con, "seg0"))
                if fb is not None and fp >= fb: segs.append(("first buffer to first present", fp - fb, "seg3"))
                x0 = left
                for lab, v, cl in segs:
                    w = (W - left - right) * v / mx
                    svg.append(f'<rect class="{cl}" x="{x0:.1f}" y="4" width="{max(w-1,0.5):.1f}" height="{rh-8}" rx="2"/>'); x0 += w
                svg.append(f'<text class="vsm" x="{X(fp)+5:.1f}" y="{rh/2+4}">{fp:.0f}</text>')
                tp = "\n".join([f"{fw}: first present {fp:.1f} ms ({sd['label']})"] + [f"{lab}: {v:.1f} ms" for lab, v, _ in segs] +
                               [f"toplevel configured at {tl:.1f} ms" if tl is not None else "", f"first surface at {fs:.1f} ms" if fs is not None else ""])
            else:
                svg.append(f'<text class="vsm" x="{left+4}" y="{rh/2+4}">no measurement (cell failed)</text>'); tp = f"{fw}: no startup measurement for this cell"
            svg.append(f'<rect class="hit" x="0" y="0" width="{W}" height="{rh}" tabindex="0" data-tip="{esc(tp)}"/></g>')
        ay = top + rh * len(rows)
        svg.append(f'<g class="axisg" transform="translate(0,{ay})">')
        for t in ticks({"mode": "linear", "a": 0, "b": mx}):
            svg.append(f'<text class="lbl small mid" x="{X(t):.1f}" y="14">{esc(tick_txt(t))}</text>')
        svg.append(f'<text class="lbl small" x="{left}" y="31">ms from spawn; unchecked (no validity check)</text></g></svg>')
        panels.append(f'<article class="mp"><h4>Startup: {esc(sd["label"])}</h4><p class="sub">unchecked (no validity check). grey: until the app connects to the compositor · blue: connect to first buffer · orange: first buffer to first present</p>' + "".join(svg) + "</article>")
    out.append('<div class="panels">' + "".join(panels) + "</div>")
    return "\n".join(out)

# ------------------------------------------------------------------ groups of headline panels
GROUPS = [
    ("Startup and memory", ["start3k", "startv", "spin0_start", "idle_rss3k", "vrss", "spin0_rss", "idle_cpu3k", "spin0_idle"]),
    ("Hover (rows, 3,000 and 100)", ["hover3k", "hover3k_c", "hover100"]),
    ("Input to commit alone: the app's share of each latency", ["hover3k_c", "hover100_c", "tbl_remove_c", "tbl_update_c", "tbl_swap_c", "search_c", "typing_h", "resize_c"]),
    ("CPU per frame alone", ["hover3k_cpu", "hover100_cpu", "tbl_remove_cpu", "tbl_update_cpu", "tbl_swap_cpu", "search_cpu", "typing_cpu", "resize_cpu"]),
    ("Scrolling", ["scroll3k", "scroll3k_fps", "vscroll"]),
    ("Typing, search and resize", ["typing", "search", "resize"]),
    ("Table operations (1,000 rows, per-row state)", ["tbl_remove", "tbl_remove_cpu", "tbl_update", "tbl_swap"]),
    ("Ticker (100 row updates per tick)", ["tick100", "tick100_fps"]),
    ("Spinner", ["spin0", "spin10k", "spin0_fps", "spin10k_fps"]),
]
HEAD = ["hover3k", "hover3k_c", "start3k", "idle_rss3k", "idle_cpu3k", "scroll3k", "typing", "resize", "spin10k", "tbl_remove", "tbl_remove_cpu"]
HIGHER = {"scroll3k_fps", "tick100_fps", "spin0_fps", "spin10k_fps"}

def winloss(cols, fws):
    """Where bough has the lowest (or highest, for frame rates) valid measured value and where it does not.
    Only valid measurements are ranked; invalid reports and estimates never are."""
    wins, losses, unk = [], [], []
    for cid, c in cols.items():
        if c["unit"] in ("fraction", "count"): continue
        hb = cid in HIGHER
        b = c["vals"].get(BOUGH)
        if not b or b["st"] != "ok": unk.append(c["label"]); continue
        oks = [(fw, d["v"]) for fw, d in c["vals"].items() if d["st"] == "ok" and fw != BOUGH and d["v"] is not None]
        if not oks: continue
        best = max(oks, key=lambda x: x[1]) if hb else min(oks, key=lambda x: x[1])
        better = (b["v"] >= best[1]) if hb else (b["v"] <= best[1])
        nbeat = sum(1 for _, v in oks if (b["v"] > v if hb else b["v"] < v))
        ratio = (best[1] / b["v"]) if hb else (b["v"] / best[1])
        if better:
            wins.append((c["label"], b["v"], c["unit"], best[0], best[1], len(oks)))
        else:
            losses.append((c["label"], b["v"], c["unit"], best[0], best[1], nbeat, len(oks)))
    w = "".join(f"<li>{esc(l)}: {fmtv(v)} {esc(u)}; next valid {esc(n)} {fmtv(nv)}</li>" for l, v, u, n, nv, k in wins)
    l_ = "".join(f"<li><b>{esc(l)}</b>: bough {fmtv(v)} {esc(u)}, best valid {esc(n)} {fmtv(nv)} ({nb} of {k} valid others are slower)</li>" for l, v, u, n, nv, nb, k in losses)
    return (f'<div class="note-box"><p><b>Where bough wins and loses</b> (valid measurements only; invalid reports and estimates are not ranked)</p>'
            f'<p>Lowest valid value in {len(wins)} metrics, behind another framework in {len(losses)}.</p>'
            f'<details><summary>Behind ({len(losses)})</summary><ul>{l_}</ul></details>'
            f'<details><summary>Ahead ({len(wins)})</summary><ul>{w}</ul></details>'
            + (f'<p class="small-note">bough has no valid measurement (invalid, unchecked or failed) in: {esc("; ".join(unk))}.</p>' if unk else "") + '</div>')


MODE_TEXT = {
    "unthrottled": ("Unthrottled", "fakeland's timing model <i>unthrottled</i>: a frame is presented as soon as the app has it ready, with no refresh grid and no wait for a latch. "
                    "Latency is therefore the app's own work from input to commit plus fakeland's present, and frame rates are what each app can sustain. The 60 Hz grid, which adds about 10 ms of waiting for every framework, is gone."),
    "fixed": ("Fixed 60 Hz grid", "fakeland's default: a 60 Hz output where the compositor latches once per frame. Every latency here contains a wait for the next latch, about 10 ms for every framework, so this view carries the split into the app's share, the wait and the present."),
    "vrr": ("Variable refresh", "fakeland's <i>vrr</i> timing, 48 to 144 Hz: the compositor presents once an update is ready and at least 1/144 s after the previous frame, and repeats a frame after 1/48 s. Latency sits between the unthrottled and the fixed-grid views."),
}
CHANGES = {
    "unthrottled": [
        "<b>Meaningful, and as fast as the app can go:</b> hover, typing, search, table operations and resize latency (input to commit to present), startup to first present, idle CPU and memory, and the spinner and ticker frame rate and CPU per frame (nothing paces them but the app itself).",
        "<b>Lose their meaning:</b> missed vblanks, pacing error, jank fraction and presents or commits per refresh (all are measured against a nominal 60 Hz that no longer exists), the ready-to-latch and latch-to-present parts of the split (about 0.03 ms and noise, sometimes slightly negative), and fps measured against a refresh. Scroll fps follows the harness's 240 Hz input rate, not the app.",
        "<b>Still applied:</b> the harness's own validity checks (wake_lateness, host_load, iteration errors, sample counts). They are about the compositor's wakes and the host, so busy-host failures still show.",
        "An app that paces itself still shows its own ceiling: bough draws the spinner at about 62 fps even here.",
    ],
    "vrr": [
        "Latency is bounded below by the shortest VRR period (6.9 ms at 144 Hz) for every frame an app presents faster than that; frame rates are capped at 144.",
        "Missed vblanks and fps against a refresh have little meaning; the validity checks still apply.",
    ],
}

def host_box(run):
    summ = run["summary"]; ls = [v for _, v in run["load"]]
    ident = " ".join(run["identity"])
    pinned = "pin: runner" in ident
    nfail = summ["failed_checks"].get("wake_lateness", 0)
    mean = sum(ls) / len(ls) if ls else 0
    busy = bool(ls) and mean > 6
    txt = (f'Host load during {run["name"]}: mean {mean:.1f}, min {min(ls):.1f}, max {max(ls):.1f} on 16 threads (1-minute load; other tenants build and profile on this host). '
           + ("The runner, compositor and apps were pinned to CPUs 0-7 while other work was confined to CPUs 8-15 (SMT siblings share cores). " if pinned else "The runner was not pinned to CPUs. ")
           + f'The harness\'s own check <span class="mono">wake_lateness</span> failed in {nfail} of {summ["n_reports"]} reports, which are drawn hollow; reports it kept valid still carry its host_load warnings (see tooltips).')
    return f'<div class="note-box{" warn" if busy or nfail > 0.1 * summ["n_reports"] else ""}"><p>{"<b>Busy host.</b> " if busy else "<b>Host conditions.</b> "}{txt}</p>{loadspark(run["load"], run["name"])}</div>'

def failures_box(run, fws):
    summ = run["summary"]; items = []
    for fw in fws:
        for f in summ["cells"].get(fw, {}).get("failed", []):
            if f["scenario"] in ("sheet", "canvas") or True:
                items.append(f'{fw}, {f["scenario"]} {f["rows"]}{"/" + f["variant"] if f["variant"] != "default" else ""}: ' + "; ".join(f"{e[0]}: {e[1]}" for e in json.loads(f["errors"])))
    if not items: return ""
    return '<div class="note-box"><p><b>Failed cells and probes in ' + esc(run["name"]) + '</b> (failures, not slow results; shown as failed, estimates marked)</p><ul>' + "".join(f"<li>{esc(i)}</li>" for i in items[:30]) + "</ul></div>"

def focus_section(data):
    h = data["headline"]; fws = h["fws"]; cols = h["cols"]; hv = data.get("heavy")
    ids = [("typing", cols), ("typing_h", cols), ("search", cols), ("search_c", cols), ("start3k", cols), ("startv", cols)]
    if hv:
        ids += [("sheet10000_rss", hv["cols"]), ("sheet10000_start", hv["cols"])]
    p = "".join(panel(i, c[i], fws, h["run"], i in HIGHER) for i, c in ids if i in c)
    return '<h3>Typing, search, startup and sheet memory</h3><p class="cap">The metrics the readers of this page watch most. The app\'s share of typing and search is the second panel of each pair.</p><div class="panels">' + p + "</div>"

def headline_section(data):
    h = data.get("headline"); pfx = data.get("key", "fixed"); title, text = MODE_TEXT[pfx]
    if not h:
        return (f'<section id="b-{pfx}"><h2>{esc(title)}</h2><p class="cap">{text}</p><div class="pending"><b>Not in yet.</b> This view fills when its run lands '
                f'({esc(data.get("timing", ""))}).</div></section>')
    run = data["runs"][h["run"]]
    fws, cols = h["fws"], h["cols"]
    ev = (data.get("estimate_validation") or {}).get("leave_one_out")
    EST_NOTE[0] = (f"hidden-cell test of this method: the stated range held the true value in {ev['coverage']*100:.0f}% of {ev['n']} tests, median error x{ev['median_factor']:.1f}, 90th percentile x{ev['p90_factor']:.0f}, worst x{ev['worst_factor']:.0f}") if ev else ""
    out = [f'<section id="b-{pfx}"><h2>{esc(title)} <span class="tag m">{esc(h["run"])}</span>'
           + (f' <span class="tag m">{esc(h["second"])}</span>' if h["second"] else "") + "</h2>"]
    out.append(f'<p class="cap">{text} The zortax gui-bench matrix ({len(fws)} builds in one invocation, --quick, software rendering for everyone) with bough added. '
               f'Lower is better except frame rates. Every framework has a value in every chart: measured, measured-but-invalid, or an estimate.</p>')
    if pfx in CHANGES:
        out.append('<div class="note-box"><p><b>What each probe measures in this mode</b></p><ul>' + "".join(f"<li>{c}</li>" for c in CHANGES[pfx]) + "</ul></div>")
    out.append(runmeta(run))
    out.append(host_box(run))
    out.append(keyrow(bool(h['second'])))
    out.append(winloss(cols, fws))
    out.append(focus_section(data))
    out.append("<h3>Headline table</h3>" + values_table(cols, fws, HEAD).replace("<details", "<details open", 1))
    out.append(startup_section(data))
    out.append(split_section(data))
    for t, ids in GROUPS:
        ids = [i for i in ids if i in cols]
        out.append(f'<h3>{esc(t)}</h3><div class="panels">' + "".join(panel(i, cols[i], fws, h["run"], i in HIGHER) for i in ids) + "</div>")
        out.append(values_table(cols, fws, ids))
    out.append(animation_section(data))
    out.append(heavy_section(data))
    out.append("</section>")
    return "\n".join(out)

# ------------------------------------------------------------------ heavy scenes
HEAVY_GROUPS = [("sheet", "1000", "Sheet, 1,000 rows (10,000 cells)"), ("sheet", "10000", "Sheet, 10,000 rows (100,000 cells)"),
                ("canvas", "1000", "Canvas, 1,000 nodes"), ("canvas", "5000", "Canvas, 5,000 nodes")]

def heavy_section(data):
    hv = data.get("heavy"); pfx = data.get("key", "fixed")
    if not hv:
        return (f'<section id="heavy-{pfx}"><h2>Heavy scenes</h2><div class="pending"><b>Not in yet.</b> The sheet (1,000 and 10,000 rows) and the canvas (5,000 nodes) for bough, iced, egui and gpui-kit in one invocation, '
                f'{esc(data.get("timing", ""))}; this section fills when the run lands.</div></section>')
    run = data["runs"][hv["run"]]
    fws = data["headline"]["fws"] if data.get("headline") else hv["measured_fws"]
    cols = hv["cols"]
    out = [f'<section id="heavy-{pfx}"><h2>Heavy scenes <span class="tag m">{esc(hv["run"])}</span></h2>',
           '<p class="cap">bough\'s standing workloads as gui-bench scenes, ported to iced, egui and gpui-kit (bench lane, round 7). The other eight frameworks were never ported to these scenes: their dots are estimates from their own cells in the table above, drawn dotted and never ranked.</p>',
           runmeta(run), keyrow(False), host_box(run), failures_box(run, hv["measured_fws"]),
           ('<div class="note-box"><p>For the sheet at 10,000 rows, the bough app log ends with "the compositor connection closed: Connection reset by peer" after the edit probe; '
            'the log excerpt is in RESULTS.md. This is a failure, not a slow result.</p><p class="small-note">Startup of the first cell of each binary in an invocation (sheet 1,000) pays a cold page cache and shader build; read the later cells.</p></div>' if pfx == "fixed" else ""),
           winloss(cols, hv["measured_fws"])]
    for sc, n, title in HEAVY_GROUPS:
        ids = [i for i in cols if cols[i]["scenario"] == sc and str(cols[i]["rows"]) == n]
        if not ids: continue
        out.append(f'<h3>{esc(title)}</h3><div class="panels">' + "".join(panel(i, cols[i], fws, hv["run"]) for i in ids) + "</div>")
        out.append(values_table(cols, fws, ids))
    if pfx == "fixed":
        out.append(then_now(hv, run))
    out.append("</section>")
    return "\n".join(out)


def then_now(hv, run):
    cols = hv["cols"]; fws = hv["measured_fws"]
    th = "".join(f"<th>{esc(f)}<br><span class='small-note'>now / then</span></th>" for f in fws)
    trs = []
    for cid, c in cols.items():
        if "prev" not in c: continue
        tds = []
        for f in fws:
            d = c["vals"].get(f, {"v": None, "st": "miss"}); pv = c["prev"].get(f)
            now = (fmtv(d["v"], c["nd"]) + ("†" if d["st"] == "inv" else "")) if d["v"] is not None else ("failed" if "failed" in (d.get("msg") or "") else "-")
            then = (fmtv(pv["v"], c["nd"]) + ("†" if not pv["valid"] else "")) if pv else "-"
            tds.append(f"<td>{esc(now)} / {esc(then)}</td>")
        trs.append(f'<tr><th scope="row">{esc(c["label"])} ({esc(c["unit"])})</th>{"".join(tds)}</tr>')
    return ('<h3>Then and now</h3><p class="cap">The first heavy-scene matrix (r7-scenes-1: bough at d4393497a0, unpinned, load 9 to 11) beside this run (heavy-1: bough at 74e665786c, pinned to CPUs 0-7, other tenants on CPUs 8-15). '
            'Different bough commits and host conditions: read it as direction, not as a controlled comparison. † = invalid by the harness.</p>'
            f'<div class="scroll"><table class="vt"><thead><tr><th>metric</th>{th}</tr></thead><tbody>{"".join(trs)}</tbody></table></div>')

# ------------------------------------------------------------------ refresh sweep
def sweep_section(data):
    sw = data.get("sweep")
    if not sw:
        return ('<section id="refresh"><h2>Refresh rates</h2><div class="pending"><b>Not in yet.</b> Rows hover, typing and the spinner for bough, iced, freya and dioxus-skia at 60, 120 and 240 Hz, one invocation per rate, with the split of input to present into input to commit, '
                'ready to latch and latch to present.</div></section>')
    out = ['<section id="refresh"><h2>Refresh rates <span class="tag m">sweep</span></h2>',
           '<p class="cap">One gui-bench invocation per output rate (--quick). Hover is rows at 3,000. Each bar is the total from input to present, split at the points fakeland observes: the app\'s commit, the compositor\'s latch of that commit, and the present.</p>']
    rates = sorted(sw, key=int)
    fws = sw[rates[0]]["fws"]
    hsegs = [("hover_commit", "input to commit (app)", "seg0"), ("hover_ready", "commit to ready", "seg1"), ("hover_latch", "ready to latch (waiting for the compositor)", "seg4"), ("hover_present", "latch to present", "seg3")]

    def chart(title, tkey, segs, partial_total=None):
        W, left, right, rh = 640, 150, 60, 24
        allt = [sw[r]["cells"][f][tkey]["v"] for r in rates for f in fws if sw[r]["cells"][f][tkey]["v"]]
        mx = max(allt) * 1.08 if allt else 1
        plot = W - left - right
        H = 16 + len(rates) * len(fws) * rh + 36
        svg = [f'<svg class="fit" viewBox="0 0 {W} {H}" role="img" aria-label="{esc(title)}">']
        for t in ticks({"mode": "linear", "a": 0, "b": mx}):
            x = left + plot * t / mx
            svg.append(f'<line class="gl" x1="{x:.1f}" x2="{x:.1f}" y1="6" y2="{H-34}"/><text class="lbl small mid" x="{x:.1f}" y="{H-18}">{tick_txt(t)}</text>')
        y = 10
        for fw in fws:
            for r in rates:
                c = sw[r]["cells"][fw]; tot = c[tkey]
                svg.append(f'<text class="nm end" x="{left-8}" y="{y+rh/2+4}" style="font-weight:{700 if fw==BOUGH else 400}">{esc(fw)} · {r} Hz</text>')
                if tot["v"] is None:
                    svg.append(f'<text class="vsm" x="{left+4}" y="{y+rh/2+4}">no value</text>')
                else:
                    inv = tot["st"] != "ok"; x0 = left; used = 0; parts = []
                    for key, lbl, cl in segs:
                        if key == "typing_rest":
                            hv = c["typing_h"]["v"]; v = (tot["v"] - hv) if hv is not None else None
                        else:
                            v = c[key]["v"]
                        if v is None or v < 0.0005: continue
                        w = plot * v / mx
                        svg.append(f'<rect class="{cl}" x="{x0:.1f}" y="{y+3}" width="{max(w-1.5,0.5):.1f}" height="{rh-8}" rx="2" style="{"opacity:.45" if inv else ""}"/>')
                        x0 += w; used += v; parts.append(f"{lbl}: {v:.2f} ms")
                    rest = tot["v"] - used
                    if rest > 0.03 * tot["v"] and tkey == "hover_total":
                        svg.append(f'<rect class="dot-inv" x="{x0:.1f}" y="{y+3}" width="{max(plot*rest/mx-1.5,0.5):.1f}" height="{rh-8}" rx="2"/>')
                        parts.append(f"unaccounted (pooled percentiles do not add): {rest:.2f} ms")
                    svg.append(f'<text class="vsm" x="{left+plot*tot["v"]/mx+5:.1f}" y="{y+rh/2+4}">{tot["v"]:.1f}{"†" if inv else ""}</text>')
                    tp = "\n".join([f"{fw} at {r} Hz ({sw[r]['run']}): total p50 {tot['v']:.2f} ms" + ("  INVALID report: " + (tot.get("msg") or "") if inv else "")] + parts)
                    svg.append(f'<rect class="hit" x="0" y="{y}" width="{W}" height="{rh}" tabindex="0" data-tip="{esc(tp)}"/>')
                y += rh
        svg.append(f'<text class="lbl small" x="{left}" y="{H-3}">ms, p50 of pooled samples; faded = report invalid (†)</text></svg>')
        return f'<article class="mp"><h4>{esc(title)}</h4>' + "".join(svg) + "</article>"

    out.append(SPLIT_CAVEAT)
    out.append('<div class="keyrow">' + "".join(f'<span><span class="sw-sq {c}"></span>{esc(l)}</span>' for _, l, c in hsegs) + '<span><svg width="20" height="14" aria-hidden="true"><rect class="dot-inv" x="2" y="2" width="16" height="10" rx="2"/></svg>unaccounted</span></div>')
    out.append('<div class="panels" style="grid-template-columns:1fr">' + chart("Rows 3,000 hover: where the time goes at each refresh rate", "hover_total", hsegs) +
               chart("Typing: handling (app) and the rest", "typing_p50", [("typing_h", "handling (app)", "seg0"), ("typing_rest", "after handling", "seg4")]) + "</div>")
    # typing + spinner tables
    th = "".join(f"<th>{esc(f)} {r} Hz</th>" for r in rates for f in fws)
    def row(label, key, nd, unit):
        tds = []
        for r in rates:
            for f in fws:
                d = sw[r]["cells"][f][key]
                if d["st"] == "miss": tds.append('<td class="miss">-</td>')
                else: tds.append(f'<td class="{"inv" if d["st"]=="inv" else ""}" title="{esc(d.get("msg") or "")}">{fmtv(d["v"], nd)}{"&nbsp;†" if d["st"]=="inv" else ""}</td>')
        return f'<tr><th scope="row">{esc(label)} ({unit})</th>{"".join(tds)}</tr>'
    rows = [row("hover total p50", "hover_total", 2, "ms"), row("hover input to commit p50", "hover_commit", 2, "ms"), row("hover commit to ready p50", "hover_ready", 2, "ms"),
            row("hover ready to latch p50", "hover_latch", 2, "ms"), row("hover latch to present p50", "hover_present", 2, "ms"),
            row("typing latency p50", "typing_p50", 2, "ms"), row("typing latency p95", "typing_p95", 2, "ms"),
            row("spinner 10,000 rows fps", "spin_fps", 1, "fps"), row("spinner 10,000 rows frame interval p99", "spin_int99", 2, "ms"), row("spinner 10,000 rows CPU per frame", "spin_cpuf", 2, "ms"),
            row("spinner no rows fps", "spin0_fps", 1, "fps"), row("spinner no rows CPU per frame", "spin0_cpuf", 2, "ms")]
    out.append(f'<h3>All sweep values</h3><div class="scroll"><table class="vt"><thead><tr><th>metric</th>{th}</tr></thead><tbody>{"".join(rows)}</tbody></table></div>')
    out.append('<p class="small-note">Every cell here was measured (four frameworks, three rates); † marks a report the harness flagged invalid. Nothing in this section is estimated.</p>')
    out.append("</section>")
    return "\n".join(out)

# ------------------------------------------------------------------ animation
def animation_section(data):
    an = data.get("animation"); pfx = data.get("key", "fixed")
    if not an:
        return f'<section id="anim-{pfx}"><h2>Animation</h2><div class="pending"><b>No data yet.</b></div></section>'
    fws, cols = an["fws"], an["cols"]
    runname = data["headline"]["run"]
    out = [f'<section id="anim-{pfx}"><h2>Animation <span class="tag m">{esc(runname)}</span></h2>',
           '<div class="note-box"><p><b>A richer animation benchmark does not exist yet.</b> The only animation cells in gui-bench are the two spinner scenarios: a turning spinner with no rows, and one above 10,000 static rows. '
           'They measure whether a framework keeps up while drawing one moving thing (against the refresh in the fixed views, as fast as it can go when unthrottled), and what that costs in CPU. They do not cover transitions, springs, many simultaneous animations, animated layout or text. '
           'The ticker is a data-update load, not an animation.</p></div>', keyrow(False)]
    groups = [("Frame pacing", ["a0_fps", "a10k_fps", "a0_int99", "a10k_int99", "a0_jank", "a10k_jank"]),
              ("Cost of each frame", ["a0_cpuf", "a10k_cpuf", "a0_cpus", "a10k_cpus", "a0_dmg", "a10k_dmg"]),
              ("Memory while animating", ["a0_rss", "a10k_rss"])]
    for title, ids in groups:
        out.append(f'<h3>{esc(title)}</h3><div class="panels">' + "".join(panel(i, cols[i], fws, runname, i.endswith("_fps")) for i in ids if i in cols) + "</div>")
        out.append(values_table(cols, fws, [i for i in ids if i in cols]))
    out.append("</section>")
    return "\n".join(out)

# ------------------------------------------------------------------ profiles
def profiles_section(data):
    pf = data.get("profiles")
    head = ('<section id="profiles"><h2>Profiles of bough <span class="tag n">not timings</span></h2>'
            '<div class="note-box warn"><p><b>Profiles are not timings.</b> These runs sample or count one bough process with <span class="mono">perf</span> (user mode, Zen 2 counters, groups of three events, call graphs by DWARF unwinding). '
            'Sampling and counting change the run. Nothing here is compared with another framework or with a timing in this page.</p></div>')
    if not pf:
        return head + '<div class="pending"><b>Not in yet.</b> perf stat and flamegraphs for rows hover, the table operations, sheet hover and theme at 10,000 rows, and canvas hover.</div></section>'
    return head + pf.get("html", "") + "</section>"

# ------------------------------------------------------------------ frameworks
DESC = {
    "zgui": "zgui (zortax's GPU toolkit build), wgpu on lavapipe",
    "gpui-kit": "gpui-kit 0.9.1 (GPUI), wgpu on lavapipe",
    "gpui-kit-fast": "gpui-kit with the fast feature set, wgpu on lavapipe",
    "freya": "Freya (current pin), Skia on lavapipe",
    "freya-056a5b4": "Freya at commit 056a5b4, Skia on lavapipe",
    "vizia": "vizia, Skia on lavapipe",
    "floem": "floem, wgpu or vger on lavapipe",
    "iced": "iced 0.14, wgpu on lavapipe",
    "egui": "eframe/egui 0.37, wgpu on lavapipe (immediate mode, one table variant)",
    "dioxus": "Dioxus Native (Blitz, Vello), wgpu on lavapipe",
    "dioxus-skia": "Dioxus Native with the Skia renderer",
    "bough": "bough at 74e665786c (r7/platform), CPU executor into wl_shm, one thread",
}

SRC = {
    "bough": "bough 74e665786c (branch r7/platform, the round-7 façade), gui-bench app comparisons/bough-guibench, clean tree; heavy scenes: tree 74e665786c + bench overlay (bough-guibench-scenes, bough-workloads, bough-mem/pmu.rs from r7/bench 580bcd2981)",
    "iced": "zortax gui-bench build; scene runs: the bench lane's scene-enabled build", "egui": "zortax gui-bench build; scene runs: the bench lane's scene-enabled build",
    "gpui-kit": "zortax gui-bench build; scene runs: the bench lane's scene-enabled build",
}

def frameworks_section(data):
    h = (data.get("modes", {}).get("fixed") or {}).get("headline") or next((m["headline"] for m in data.get("modes", {}).values() if m.get("headline")), None)
    if not h: return ""
    run = data["runs"][h["run"]]; urun = (data["runs"].get("cmp-u1") or run)
    thr = ((data["modes"].get("unthrottled") or data["modes"].get("fixed"))["headline"]["cols"].get("threads3k", {}) or {}).get("vals", {})
    rows = []; cards = []
    for fw in h["fws"]:
        s = run["summary"]; rp = s["reports"].get(fw, {"ok": 0, "inv": 0, "warn": 0}); ce = s["cells"].get(fw, {"ok": 0, "err": 0, "errors": []})
        bn = run["binaries"].get(fw); bh = (data["runs"].get("heavy-1") or {}).get("binaries", {}).get(fw)
        nth = (thr.get(fw) or {}).get("v")
        pills = []
        if ce["err"]: pills.append(f'<span class="pill bad">{ce["err"]} cells failed</span>')
        if rp["inv"]: pills.append(f'<span class="pill bad">{rp["inv"]} reports invalid</span>')
        if not ce["err"] and not rp["inv"]: pills.append('<span class="pill good">all reports valid</span>')
        why = f'<p class="small-note">Failure: {esc(ce["errors"][0][:150])}. No timing exists for these cells; the charts show estimates.</p>' if ce["errors"] else ""
        cards.append(f'<article class="fwc"><h4>{esc(fw)}</h4><p class="small-note">{esc(DESC.get(fw, ""))}</p>'
                     f'<dl><dt>cells (cmp-2)</dt><dd>{ce["ok"]} completed, {ce["err"]} failed</dd><dt>reports (cmp-2)</dt><dd>{rp["ok"]} valid ({rp["warn"]} with warnings), {rp["inv"]} invalid</dd>'
                     f'<dt>threads (max)</dt><dd>{"-" if nth is None else f"{nth:.0f}"} in the idle rows-3,000 process</dd></dl><p>{"".join(pills)}</p>{why}</article>')
        rows.append(f'<tr><th scope="row">{esc(fw)}</th><td class="mono">{esc(bn["sha"]) if bn else "-"}</td><td>{(bn["bytes"]/1048576):,.0f} MiB</td><td class="mono">{esc(bh["sha"]) if bh and bh["sha"] != (bn or {}).get("sha") else "same"}</td>'
                    f'<td>{esc(SRC.get(fw, "zortax gui-bench build (/root/work/zortax, read-only)"))}</td><td>{"-" if nth is None else f"{nth:.0f}"}</td></tr>')
    manifest = ('<h3>Executable manifest</h3><p class="cap">One row per tested executable. Every value in the charts carries its executable hash in its tooltip. Scope: these are benchmark apps as built for this matrix (release profile, symbols kept, not stripped or size-optimised; profile, features and LTO differ between the builds and are not all recorded), so the sizes are not shipping binary sizes and no ratio of them is a framework size result. '
                'The memory figures are process RSS at the end of the probe: they include shared pages and libraries and, for the wgpu and Skia builds, the lavapipe driver; they are not steady-state memory after scrolling through a whole dataset.</p>'
                f'<div class="scroll"><table class="vt"><thead><tr><th>framework</th><th>cmp runs: sha256[:16]</th><th>size</th><th>heavy-scene build</th><th>source</th><th>threads</th></tr></thead><tbody>{"".join(rows)}</tbody></table></div>')
    return ('<section id="frameworks"><h2>Frameworks</h2><p class="cap">What ran for each build, and what executable it was.</p>' + manifest +
            f'<h3>Per framework</h3><div class="fwcards">{"".join(cards)}</div></section>')

# ------------------------------------------------------------------ method
def validation_table(data):
    rows = []
    for key in ("unthrottled", "fixed", "vrr"):
        m = (data.get("modes") or {}).get(key)
        if not m: continue
        v = m["estimate_validation"]
        for name, lab in (("leave_one_out", "hide one valid cell, estimate it from the rest"), ("floem_like", "keep only the spinner-without-rows cells of a framework, estimate all its others (floem's situation)")):
            x = v.get(name)
            if x: rows.append(f'<tr><th scope="row">{esc(m["title"])}</th><td>{esc(lab)}</td><td>{x["n"]}</td><td>{x["coverage"]*100:.0f} %</td><td>x{x["median_factor"]:.1f}</td><td>x{x["p90_factor"]:.0f}</td><td>x{x["worst_factor"]:,.0f}</td></tr>')
    return ('<div class="scroll"><table class="vt"><thead><tr><th>view</th><th>hold-out test</th><th>tests</th><th>stated range held the true value</th><th>median error</th><th>90th percentile</th><th>worst</th></tr></thead>'
            f'<tbody>{"".join(rows)}</tbody></table></div>')

def method_section(data):
    est_text = """<p><b>An estimate is a heuristic guess, not a measurement, and its range is not a confidence interval.</b> It fills a cell that has no measurement: a failed cell (floem fails the harness's layout check in all but one cell; vizia typing fails it in the unthrottled run), a probe that failed, or a scene a framework was never ported to (the heavy scenes exist for four frameworks only). The method, the same for all of them:</p>
<ol>
<li>Only values the harness judged valid are used (startup, which has no check, feeds only the startup class). Invalid and unchecked values are drawn but feed nothing else.</li>
<li>Columns are grouped into classes of one kind of quantity (input latency, CPU per frame, frame interval, resident memory, idle CPU, startup, frame rate). Within a class the logarithm of a value is modelled as a level for the framework plus a level for the column, fitted by median polish.</li>
<li>The framework's level is the median over its own valid cells in the class, needing two own cells (one for frame rate). With fewer, its level across all classes is used and the estimate is labelled <i>cross-class</i>.</li>
<li>The range is the estimate times exp(plus or minus 2 s), s = 1.4826 x the median absolute residual of the framework's own cells, with a floor of 0.15 (0.30 cross-class). Values are rounded to two significant figures.</li>
</ol>
<p><b>Limits.</b> The model assumes that costs scale multiplicatively between cells. It cannot represent a cliff: a fallback path, an atlas or capacity overflow, a layout failure, worker start-up or a full re-bin. Missing cells are not random: a failed or unported cell is often the expensive case, so an estimate is most likely to be too low exactly where it matters (bough's canvas is the example below). An estimate for a workload a framework failed or never implemented is not evidence that it supports the workload. Cross-class fallback has no demonstrated physical basis. <b>Estimates are kept out of every ranking and every win or loss count on this page;</b> they sit below the measured rows, drawn dotted, and carry a reason (failed cell, scene never ported) in their tooltip.</p>
<p>The method was tested by hiding cells it can see. The stated ranges are too narrow: they are meant to hold about 95 % and hold the true value in roughly three of four tests, and the errors have long tails.</p>""" + validation_table(data) + """
<p class="small-note">Worst cases: the CPU executor's cliffs, for example bough canvas theme switch (measured 1,151 ms, estimated 21 ms from bough's other cells) and bough idle CPU (a value near zero that nothing else resembles).</p>"""
    cav = ["One deployment configuration, not a GPU ranking and not an equal-work comparison. ilo-prod has no GPU. bough draws with its CPU executor into shared-memory buffers on one thread (the idle process has 1 thread); the wgpu and Skia builds render on Mesa lavapipe with 21 to 53 threads (Frameworks section); the compositor is fakeland's pixman. The result says which build works best on this software-rendered host. Nothing here establishes how any of them ranks on a real GPU or on equal CPU resources; per-process CPU time includes every worker thread of the process.",
           "CPU affinity is not isolation. cmp-2 was unpinned while the 1-minute load went from 0.8 to 21 (mean 9.1). Later runs were pinned to CPUs 0-7 while other tenants were confined to CPUs 8-15, but CPU n and n+8 are SMT siblings of one core, so the pinned runs still shared execution units and caches with other people's builds, and memory bandwidth was shared. Instruction counts are steadier than wall time under that sharing, not immune. No ABBA or randomised block design was run; the harness's wake_lateness and host_load checks decide validity and the load is recorded, not corrected for.",
           "Startup is unchecked. The harness records startup to first present with no validity report, so every startup value is marked unchecked, is never ranked, and cmp-2's startup is load-contaminated (bough spinner-without-rows startup read 682 ms there and 38 to 47 ms in the pinned runs).",
           "Validity is explicit. A value counts as valid only when the harness's own checks passed for its report; a missing validity record is shown as unchecked, never valid. Table and ticker columns use the fine variant; egui has one default variant and runs that (marked with a double dagger in the value tables), so the fine and coarse comparison is not two implementations for it.",
           "Bound to semantics, to the extent the harness allows. A cell is rankable only if its runner layout check passed (colour at fixed points) and its report passed the harness's checks; each value's tooltip names the executable hash. There is no independent pixel or behaviour check beyond the layout check, no input-trace replay across frameworks and no failure mutants for stale hover, wrong focus, omitted text or truncated data. A fast value could come from an app that draws less; the layout check is the only guard.",
           "The latency split is not an exact decomposition: medians of separately pooled phases do not add up to the total median, and an unthrottled run still contains protocol and buffer scheduling. It is neither pure app cost nor input-to-photon (fakeland has no display).",
           "bough is the round-7 façade build. It has no runtime scale change; the scale probe is unsupported. The new-core binary is a separate entry (bough-x) and is not on this page until it passes the layout check.",
           "Frameworks are not equally ported: egui has one table variant, freya and dioxus run their own idiom, the shared-state variants exist for seven builds. Quick mode: the smallest sizes with fewer repeats and two iterations.",
           "The heavy scenes cover a sheet and a canvas in four frameworks, not all five standing scenes or all frameworks. The spinner is the only animation cell; a richer animation benchmark does not exist yet. Latin text only: no fallback shaping, bidi, IME, image churn, nested scrollers or multiple windows.",
           "Memory is process RSS at the end of the probe, with shared pages and driver memory included, not a steady-state figure after scrolling a whole dataset. Executable sizes are for symbol-bearing benchmark apps with different profiles, not shipping sizes."]
    raw = json.dumps({k: v for k, v in data.items() if k not in ("headline", "splits", "startups", "animation", "heavy", "profiles")}, separators=(",", ":"))
    rawbox = f'<details class="raw"><summary>The data behind every chart (JSON, {len(raw)//1024} KiB; downloads are blocked inside artifacts, so it is shown here)</summary><pre>{esc(raw)}</pre></details>'
    return ('<section id="method"><h2>Method and caveats</h2><h3>Estimates</h3>' + est_text + '<h3>Caveats</h3><ul>' + "".join(f"<li>{esc(c)}</li>" for c in cav) + "</ul>" + rawbox + "</section>")

def topbar(items):
    nav = "".join(f'<a href="#{i}">{esc(l)}</a>' for i, l in items)
    return f'<header class="topbar"><div class="in"><b>bough bench, round 8</b><nav>{nav}</nav></div></header>'

NAV = [("b-unthrottled", "Unthrottled"), ("b-fixed", "Fixed 60 Hz"), ("b-vrr", "Variable refresh"), ("refresh", "Refresh rates"), ("profiles", "Profiles"),
       ("frameworks", "Frameworks"), ("method", "Method"), ("earlier", "Earlier results")]

JS = r"""
(function(){
  var hidden = {};
  function relayout(){
    document.querySelectorAll("svg.mpsvg").forEach(function(svg){
      var top = +svg.dataset.top, rh = +svg.dataset.rh, i = 0, rows = svg.querySelectorAll("g.row");
      rows.forEach(function(g){
        var h = !!hidden[g.dataset.fw];
        g.style.display = h ? "none" : "";
        if(!h){ g.setAttribute("transform","translate(0,"+(top+rh*i)+")"); i++; }
      });
      var ay = top + rh*i, vb = svg.getAttribute("viewBox").split(" ");
      var ax = svg.querySelector("g.axisg"); if(ax) ax.setAttribute("transform","translate(0,"+ay+")");
      svg.querySelectorAll("g.gridg line").forEach(function(l){ l.setAttribute("y2", ay); });
      svg.setAttribute("viewBox", vb[0]+" "+vb[1]+" "+vb[2]+" "+(ay+36));
    });
    document.querySelectorAll(".toggles button[data-fw]").forEach(function(b){ b.setAttribute("aria-pressed", String(!hidden[b.dataset.fw])); });
  }
  document.addEventListener("click", function(e){
    var b = e.target.closest && e.target.closest(".toggles button"); if(!b) return;
    if(b.dataset.all){ hidden = {}; } else { hidden[b.dataset.fw] = !hidden[b.dataset.fw]; }
    relayout();
  });
  var links = document.querySelectorAll(".topbar a");
  function spy(){
    var cur = null;
    links.forEach(function(a){ var s = document.querySelector(a.getAttribute("href")); if(s && s.getBoundingClientRect().top < 120) cur = a; });
    links.forEach(function(a){ a.classList.toggle("on", a === cur); });
  }
  window.addEventListener("scroll", spy, {passive:true}); spy();
})();
"""

TOP_NOTE = """<div class="note-box warn"><p><b>Read this first.</b></p><ul>
<li><b>This is the round-7 façade, not the new core.</b> Every "bough" value on this page is the round-7 platform build (commit 74e665786c of r7/platform) on its CPU executor. It says nothing about the round-8 core. A separate entry, bough-x, will sit beside it when the e2e lane's binary has passed the runner's layout check.</li>
<li><b>One deployment configuration.</b> Software rendering on a host with no GPU: bough runs one thread; the others run wgpu or Skia on lavapipe with 21 to 53 threads. It is not a GPU ranking and not an equal-work comparison.</li>
<li><b>Pinning is not isolation.</b> Other tenants shared the host and the physical cores (SMT siblings); cmp-2 was not pinned at all.</li>
<li><b>Startup is unchecked</b> (no harness validity check) and cmp-2's startup is load-contaminated. <b>Estimates</b> (dotted diamonds) are heuristic guesses with ranges that are not confidence intervals; they are never ranked.</li>
</ul></div>"""

def build_sections(data, earlier_html):
    modes = data.get("modes", {})
    views = []
    for key in ("unthrottled", "fixed", "vrr"):
        v = dict(data); v.update(modes.get(key, {"headline": None, "heavy": None, "animation": None, "splits": None, "startups": None}))
        v["key"] = key; v.setdefault("timing", {"unthrottled": "timing unthrottled", "fixed": "fixed 60 Hz", "vrr": "vrr 48-144 Hz"}[key])
        views.append(v)
    fws = next((v["headline"]["fws"] for v in views if v.get("headline")), [])
    EXE.clear()
    for rn, rm in data.get("runs", {}).items():
        EXE[rn] = {fw: b["sha"] for fw, b in rm.get("binaries", {}).items()}
    parts = [
        topbar(NAV),
        '<main class="wide">',
        '<h1>Rust GUI frameworks under fakeland, round 8</h1>',
        f'<p class="meta">Generated {esc(data.get("generated",""))} · host ilo-prod (Ryzen 7 3700X, Linux 6.18.54, no GPU) · bough at 74e665786c · every chart is drawn from data.r8.json by r8.py</p>',
        '<p class="cap">First view: the app\'s own latency with no compositor wait (unthrottled). Second: the 60 Hz fixed grid, with the split into the app, the wait and the present. Third: variable refresh. Estimates and invalid reports are marked as before.</p>',
        TOP_NOTE,
        toggles(fws),
    ] + [headline_section(v) for v in views] + [sweep_section(data), profiles_section(data), frameworks_section(data), method_section(data),
        '<section id="earlier"><h2>Earlier results <span class="tag n">rounds 6 and 7</span></h2><p class="cap">The previous version of this page, unchanged: bough\'s own workloads, scaling series, before and after, and the first zortax quick run.</p>' + earlier_html + "</section>",
        "</main>"]
    return "\n".join(parts)
