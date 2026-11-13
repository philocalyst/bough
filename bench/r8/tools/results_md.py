"""results_md.py : writes the data-driven parts of RESULTS.md (tables with every value marked valid, invalid or estimate)
from dashboard/data.r8.json into results_tables.md; RESULTS.md embeds them (written by hand around them)."""
import json, os, sys
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
D = json.load(open(os.path.join(ROOT, "dashboard", "data.r8.json")))

def f(v, nd):
    if v is None: return "-"
    if abs(v) >= 1000: return f"{v:,.0f}"
    if abs(v) >= 100: return f"{v:,.1f}"
    return f"{v:.{nd}f}"

def cell(d, est, nd):
    if d["st"] == "ok": return f(d["v"], nd)
    if d["st"] == "inv": return f(d["v"], nd) + " †"
    if est: return "*~" + f(est["v"], nd) + "*" + (" (failed cell)" if "failed" in (d.get("msg") or "") else "")
    return "-"

def table(cols, fws, ids, title=None):
    hdr = "| framework | " + " | ".join(f'{cols[i]["label"]} ({cols[i]["unit"]})' for i in ids) + " |"
    out = [hdr, "|---|" + "---:|" * len(ids)]
    for fw in fws:
        r = []
        for i in ids:
            c = cols[i]; r.append(cell(c["vals"].get(fw, {"v": None, "st": "miss"}), (c.get("est") or {}).get(fw), c["nd"]))
        out.append(f"| {fw} | " + " | ".join(r) + " |")
    return "\n".join(out)

GROUPS = [
    ("Startup, memory, idle", ["start3k", "startv", "spin0_start", "idle_rss3k", "vrss", "spin0_rss", "idle_cpu3k", "spin0_idle"]),
    ("Hover (rows)", ["hover3k", "hover3k_c", "hover100", "hover100_c", "hover3k_cpu", "hover100_cpu"]),
    ("Scrolling", ["scroll3k", "scroll3k_fps", "vscroll"]),
    ("Typing, search, resize", ["typing", "typing_h", "typing_cpu", "search", "search_c", "search_cpu", "resize", "resize_c", "resize_cpu"]),
    ("Table operations (1,000 rows, per-row state; egui has its one variant)", ["tbl_remove", "tbl_remove_c", "tbl_remove_cpu", "tbl_update", "tbl_update_c", "tbl_update_cpu", "tbl_swap", "tbl_swap_c", "tbl_swap_cpu"]),
    ("Ticker (100 row updates per tick)", ["tick100", "tick100_fps"]),
    ("Spinner", ["spin0", "spin10k", "spin0_fps", "spin10k_fps"]),
]
def winloss_md(cols, fws, higher):
    wins, losses = [], []
    for cid, c in cols.items():
        if c["unit"] in ("fraction", "count"): continue
        hb = cid in higher
        b = c["vals"].get("bough")
        if not b or b["st"] != "ok": continue
        oks = [(fw, d["v"]) for fw, d in c["vals"].items() if d["st"] == "ok" and fw != "bough" and d["v"] is not None]
        if not oks: continue
        best = max(oks, key=lambda x: x[1]) if hb else min(oks, key=lambda x: x[1])
        ok = (b["v"] >= best[1]) if hb else (b["v"] <= best[1])
        (wins if ok else losses).append((c["label"], c["unit"], b["v"], best, c["nd"]))
    out = [f"Lowest valid value in {len(wins)} metrics, behind another framework in {len(losses)}.\n"]
    if losses:
        out.append("Behind (valid measurements only):\n")
        out += [f"- {l} ({u}): bough {f(v, nd)}, best valid {b[0]} {f(b[1], nd)}" for l, u, v, b, nd in losses]
    return "\n".join(out)

HIGHER = {"scroll3k_fps", "tick100_fps", "spin0_fps", "spin10k_fps", "a0_fps", "a10k_fps"}
out = []
for key in ("unthrottled", "fixed", "vrr"):
    m = D["modes"].get(key)
    if not m: continue
    h = m["headline"]
    out.append(f"# VIEW {m['title']} ({h['run']}{', ' + m['heavy']['run'] if m.get('heavy') else ''}; timing {m['timing']})\n")
    out.append("## Where bough wins and loses, headline cells\n"); out.append(winloss_md(h["cols"], h["fws"], HIGHER)); out.append("")
    if m.get("heavy"):
        out.append("## Where bough wins and loses, heavy scenes (four measured frameworks)\n"); out.append(winloss_md(m["heavy"]["cols"], m["heavy"]["measured_fws"], HIGHER)); out.append("")
    out.append("## Headline table (the cmp-1 columns)\n")
    HEAD = ["hover3k", "hover3k_c", "start3k", "idle_rss3k", "idle_cpu3k", "scroll3k", "typing", "resize", "spin10k", "tbl_remove", "tbl_remove_cpu"]
    out.append(table(h["cols"], h["fws"], HEAD)); out.append("")
    for t, ids in GROUPS:
        out.append(f"## {t}\n"); out.append(table(h["cols"], h["fws"], [i for i in ids if i in h["cols"]])); out.append("")
    out.append("## Estimates, with method per value\n")
    out.append("| column | framework | estimate | range | basis |\n|---|---|---:|---|---|")
    allc = {**h["cols"], **((m.get("heavy") or {}).get("cols", {})), **m["animation"]["cols"]}
    for cid, c in allc.items():
        for fw, e in (c.get("est") or {}).items():
            out.append(f'| {c["label"]} ({c["unit"]}) | {fw} | ~{f(e["v"], c["nd"])} | {f(e["lo"], c["nd"])} to {f(e["hi"], c["nd"])} | {e["how"]}: {e["note"]} |')
    out.append("")
    if m.get("heavy"):
        hv = m["heavy"]; fws = h["fws"]
        for sc, n, title in (("sheet", "1000", "Sheet, 1,000 rows"), ("sheet", "10000", "Sheet, 10,000 rows"), ("canvas", "1000", "Canvas, 1,000 nodes"), ("canvas", "5000", "Canvas, 5,000 nodes")):
            ids = [i for i, c in hv["cols"].items() if c["scenario"] == sc and str(c["rows"]) == n]
            out.append(f"## Heavy scenes: {title}\n"); out.append(table(hv["cols"], fws, ids)); out.append("")
        if key == "fixed":
            out.append("## Heavy scenes, then (r7-scenes-1) and now (heavy-1), measured frameworks\n")
            mf = hv["measured_fws"]
            out.append("| metric | " + " | ".join(f"{x} now / then" for x in mf) + " |\n|---|" + "---:|" * len(mf))
            for cid, c in hv["cols"].items():
                if "prev" not in c: continue
                r = []
                for x in mf:
                    d = c["vals"].get(x, {"v": None, "st": "miss"}); pv = c["prev"].get(x)
                    now = (f(d["v"], c["nd"]) + (" †" if d["st"] == "inv" else "")) if d["v"] is not None else ("failed" if "failed" in (d.get("msg") or "") else "-")
                    then = (f(pv["v"], c["nd"]) + ("" if pv["valid"] else " †")) if pv else "-"
                    r.append(f"{now} / {then}")
                out.append(f'| {c["label"]} ({c["unit"]}) | ' + " | ".join(r) + " |")
            out.append("")
    an = m["animation"]
    out.append("## Animation: the spinner scenarios (the only animation cells)\n")
    ids = ["a0_fps", "a0_int50", "a0_int99", "a0_jank", "a0_missed", "a0_cpuf", "a0_cpus", "a10k_fps", "a10k_int50", "a10k_int99", "a10k_jank", "a10k_missed", "a10k_cpuf", "a10k_cpus"]
    out.append(table(an["cols"], an["fws"], [i for i in ids if i in an["cols"]])); out.append("")
    out.append("## Latency split (input to commit, wait, present)\n")
    out.append("| framework | probe | total p50 | input to commit | commit to ready | ready to latch | latch to present |\n|---|---|---:|---:|---:|---:|---:|")
    for pid in ("hover3k", "tbl_update", "tbl_remove", "tbl_swap", "search"):
        sp = m["splits"][pid]
        for fw in h["fws"]:
            c = sp["fws"][fw]
            if c["total"]["v"] is None: continue
            g = lambda k: f(c[k]["v"], 2) + (" †" if c["total"]["st"] == "inv" else "")
            out.append(f'| {fw} | {sp["label"]} | {g("total")} | {g("commit")} | {g("c2r")} | {g("latch")} | {g("present")} |')
    out.append("")
if D.get("sweep"):
    sw = D["sweep"]; rates = sorted(sw, key=int); fws = sw[rates[0]]["fws"]
    out.append("# Refresh sweep (rows 3,000 hover, typing 1,000, spinner), fixed grid, measured, † = invalid by the harness\n")
    keys = [("hover_total", "hover total p50", 2), ("hover_commit", "hover input to commit", 2), ("hover_ready", "hover commit to ready", 2), ("hover_latch", "hover ready to latch", 2), ("hover_present", "hover latch to present", 2),
            ("typing_p50", "typing p50", 2), ("typing_h", "typing handling p50", 2), ("spin_fps", "spinner 10,000 rows fps", 1), ("spin_int99", "spinner 10,000 interval p99", 2), ("spin_cpuf", "spinner 10,000 CPU per frame", 2), ("spin0_fps", "spinner no rows fps", 1), ("spin0_cpuf", "spinner no rows CPU per frame", 2)]
    cols = [(r, fw) for fw in fws for r in rates]
    out.append("| metric (ms unless noted) | " + " | ".join(f"{fw} {r} Hz" for r, fw in cols) + " |\n|---|" + "---:|" * len(cols))
    for k, lab, nd in keys:
        row = []
        for r, fw in cols:
            d = sw[r]["cells"][fw][k]; row.append(f(d["v"], nd) + (" †" if d["st"] == "inv" else "") if d["v"] is not None else "-")
        out.append(f"| {lab} | " + " | ".join(row) + " |")
    out.append("")
open(os.path.join(ROOT, "results_tables.md"), "w").write("\n".join(out))
print("wrote results_tables.md", len("\n".join(out)), "bytes")
