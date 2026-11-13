"""collect.py : every run under results/ as one JSON (dashboard/data.r8.json) for the dashboard builder.

A value is {"v": number|None, "st": "ok"|"inv"|"miss", "fail": [..], "warn": [..], "msg": "..."}.
ok  = every harness check of that report passed or only warned (warnings listed in "warn", not hidden);
inv = a harness check failed (wake_lateness is the one that fails on a busy host): shown hollow, never ranked;
miss = no measurement. Estimates (tools/estimate.py) are kept apart under "est", never mixed into "vals"."""
import sys, os, csv, json, glob, re, statistics, collections
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import gb, headline, estimate

ROOT = os.path.dirname(HERE)
RES = os.path.join(ROOT, "results")
MIB = 1 / 1048576

def tsv(p):
    with open(p) as f:
        return list(csv.DictReader(f, delimiter="\t"))

class Flat:
    """One run's flat dir with validity detail (failed and warned checks per report)."""
    def __init__(self, d):
        self.d = d
        self.r = gb.Run(d)
        self.fail = collections.defaultdict(list)
        self.warn = collections.defaultdict(list)
        self.msg = {}
        for v in tsv(os.path.join(d, "validity.tsv")):
            if not v["check"]:
                continue
            k = (gb.Run.ckey(v), v["report"])
            if v["status"] == "fail":
                self.fail[k].append(v["check"]); self.msg[(k, v["check"])] = v["message"]
            elif v["status"] == "warn":
                self.warn[k].append(v["check"]); self.msg.setdefault((k, v["check"]), v["message"])
        self.start = {}
        p = os.path.join(d, "startup.tsv")
        if os.path.exists(p):
            for row in tsv(p):
                self.start[(row["scenario"], row["framework"], row["variant"], row["rows"], row["updates"], str(int(float(row["hz"] or 60))))] = row

    def get(self, fw, scenario, report, section, metric, rows, variant="default", updates="None", hz="60", scale=1.0):
        for var in ([variant, "default"] if variant != "default" else [variant]):
            k = (scenario, fw, var, str(rows), str(updates), str(hz))
            v = self.r.M.get((k, report, section, metric))
            if v is None:
                continue
            ok = self.r.valid.get((k, report))   # None: no validity record for this report = unchecked, never valid
            f = self.fail.get((k, report), []); w = self.warn.get((k, report), [])
            msg = "; ".join(f"{c}: {self.msg.get(((k, report), c), '')}" for c in f[:2])
            return {"v": v * scale, "st": "unk" if ok is None else ("ok" if ok else "inv"), "fail": f, "warn": w, "msg": msg if ok is not None else "no validity record for this report", "variant": var}
        return {"v": None, "st": "miss", "fail": [], "warn": [], "msg": self.cell_failure(fw, scenario, rows, variant, updates, hz, report)}

    def cell_failure(self, fw, scenario, rows, variant, updates, hz, report):
        for var in ([variant, "default"] if variant != "default" else [variant]):
            c = self.r.cells.get((scenario, fw, var, str(rows), str(updates), str(hz)))
            if c is None:
                continue
            try:
                errs = json.loads(c["errors"])
            except ValueError:
                continue
            for e in errs:
                if e[0] in (report, "layout"):
                    return f"cell failed ({e[0]}): {e[1]}"
        return ""

    def startup(self, fw, scenario, rows, variant="default", updates="None", hz="60"):
        for var in ([variant, "default"] if variant != "default" else [variant]):
            k = (scenario, fw, var, str(rows), str(updates), str(hz))
            row = self.start.get(k)
            if row is None or row["first_present_ms"] in ("None", "") or self.cell_failure(fw, scenario, rows, var, updates, hz, "startup"):
                continue
            return {"v": float(row["first_present_ms"]), "st": "unk", "fail": [], "warn": [], "msg": "startup has no harness validity check (unchecked)", "variant": var}
        return {"v": None, "st": "miss", "fail": [], "warn": [], "msg": ""}

def identity(rundir):
    p = os.path.join(rundir, "IDENTITY.txt")
    lines = open(p).read().splitlines() if os.path.exists(p) else []
    load = []
    lp = os.path.join(rundir, "load.log")
    if os.path.exists(lp):
        for l in open(lp):
            m = re.match(r"(\d\d:\d\d:\d\d) ([\d.]+)", l)
            if m: load.append((m.group(1), float(m.group(2))))
    return lines, load

def cols_cmp(fl, fws):
    out = {}
    for col in headline.COLS:
        cid, label, sc, var, rows, upd, rep, sec, met, scale, unit, nd = col
        out[cid] = {"label": label, "unit": unit, "nd": nd, "cls": headline.klass(cid, unit) if unit != "count" else "other", "variant_spec": var,
                    "vals": {fw: fl.get(fw, sc, rep, sec, met, rows, variant=var, updates=upd, scale=scale) for fw in fws}}
    for cid, label, sc, var, rows, upd in (("start3k", "rows 3,000 startup to first present", "rows", "default", 3000, "None"),
                                          ("startv", "virtual 100,000 startup to first present", "virtual", "default", 100000, "None"),
                                          ("spin0_start", "spinner (no rows) startup to first present", "spinner", "default", 0, "None")):
        out[cid] = {"label": label, "unit": "ms", "nd": 1, "cls": "startup (ms)", "vals": {fw: fl.startup(fw, sc, rows, var, upd) for fw in fws}}
    return out

# heavy scenes: (id, label, scenario, rows, report, section, metric, scale, unit, nd, class)
def heavy_cols():
    cols = []
    for sc, sizes in (("sheet", (1000, 10000)), ("canvas", (1000, 5000))):
        unit_n = "rows" if sc == "sheet" else "nodes"
        for n in sizes:
            nm = f"{sc} {n:,} {unit_n}"
            i = f"{sc}{n}"
            cols += [
                (i + "_hover", nm + " hover total p50", sc, n, "hover", "latency", "total_p50", 1, "ms", 2, "latency (ms)"),
                (i + "_hover_c", nm + " hover input to commit p50", sc, n, "hover", "latency", "input_to_commit_p50", 1, "ms", 2, "latency (ms)"),
                (i + "_theme", nm + " theme switch total p50", sc, n, "theme", "latency", "total_p50", 1, "ms", 2, "latency (ms)"),
                (i + "_edit", nm + " edit total p50", sc, n, "edit", "latency", "total_p50", 1, "ms", 2, "latency (ms)"),
                (i + "_resize", nm + " resize configure to present p50", sc, n, "resize", "resize", "configure_to_present_p50", 1, "ms", 1, "latency (ms)"),
                (i + "_rss", nm + " idle RSS", sc, n, "idle", "resources", "rss_max", MIB, "MiB", 1, "resident memory (MiB)"),
                (i + "_idle", nm + " idle CPU", sc, n, "idle", "resources", "cpu_ms_per_s", 1, "ms/s", 2, "idle CPU (ms/s)"),
            ]
    return cols

def cols_heavy(fl, fws):
    out = {}
    for cid, label, sc, n, rep, sec, met, scale, unit, nd, cls in heavy_cols():
        out[cid] = {"label": label, "unit": unit, "nd": nd, "cls": cls, "scenario": sc, "rows": n,
                    "vals": {fw: fl.get(fw, sc, rep, sec, met, n, scale=scale) for fw in fws}}
    for sc, sizes in (("sheet", (1000, 10000)), ("canvas", (1000, 5000))):
        for n in sizes:
            i = f"{sc}{n}"
            out[i + "_start"] = {"label": f"{sc} {n:,} startup to first present", "unit": "ms", "nd": 1, "cls": "startup (ms)", "scenario": sc, "rows": n,
                                 "vals": {fw: fl.startup(fw, sc, n) for fw in fws}}
    return out

def summarise(fl, fws):
    cnt = {}
    for (k, rep), ok in fl.r.valid.items():
        c = cnt.setdefault(k[1], {"ok": 0, "inv": 0, "warn": 0}); c["ok" if ok else "inv"] += 1
        if ok and fl.warn.get((k, rep)): c["warn"] += 1
    cells = {}
    for k, c in fl.r.cells.items():
        e = cells.setdefault(k[1], {"ok": 0, "err": 0, "errors": []})
        if c["errors"] != "[]":
            e["err"] += 1; e["errors"].append(c["errors"][:200]); e.setdefault("failed", []).append({"scenario": k[0], "rows": k[3], "variant": k[2], "errors": c["errors"]})
        else:
            e["ok"] += 1
    checks = collections.Counter()
    for (k, rep), f in fl.fail.items():
        for c in f: checks[c] += 1
    return {"reports": cnt, "cells": cells, "failed_checks": dict(checks), "n_reports": len(fl.r.valid)}

def load_run(name, kind="cmp"):
    d = os.path.join(RES, name)
    flat = os.path.join(d, "flat")
    if not os.path.exists(os.path.join(flat, "validity.tsv")):
        return None
    fl = Flat(flat)
    ident, load = identity(d)
    bins = {}
    for l in ident:
        p = l.split()
        if len(p) == 3 and p[1].isdigit() and p[2].startswith("apps/"):
            fw = p[2].split("/")[1]
            if fw == "dioxus" and "/skia/" in p[2]: fw = "dioxus-skia"
            bins[fw] = {"sha": p[0], "bytes": int(p[1]), "path": p[2]}
    return fl, {"name": name, "identity": ident, "load": load, "summary": summarise(fl, fl.r.fws), "fws": fl.r.fws, "binaries": bins}

SPLIT_PROBES = [
    # id, label, scenario, variant, rows, report, kind
    ("hover3k", "rows 3,000 hover", "rows", "default", 3000, "hover", "lat"),
    ("hover100", "rows 100 hover", "rows", "default", 100, "hover", "lat"),
    ("tbl_remove", "table fine remove", "table", "fine", 1000, "op-remove", "lat"),
    ("tbl_update", "table fine update10", "table", "fine", 1000, "op-update10", "lat"),
    ("tbl_swap", "table fine swap", "table", "fine", 1000, "op-swap", "lat"),
    ("search", "search filter", "search", "default", 1000, "filter", "lat"),
    ("typing", "typing", "typing", "default", 1000, "typing", "typing"),
    ("resize", "resize", "resize", "default", 1000, "resize", "resize"),
]

def splits(fl, fws):
    out = {}
    for pid, label, sc, var, rows, rep, kind in SPLIT_PROBES:
        d = {}
        for fw in fws:
            g = lambda sec, met: fl.get(fw, sc, rep, sec, met, rows, variant=var)
            if kind == "lat":
                d[fw] = {"total": g("latency", "total_p50"), "commit": g("latency", "input_to_commit_p50"), "c2r": g("latency", "commit_to_ready_p50"),
                         "latch": g("latency", "ready_to_latch_p50"), "present": g("latency", "latch_to_present_p50")}
            elif kind == "typing":
                d[fw] = {"total": g("typing", "latency_p50"), "commit": g("typing", "handling_p50")}
            else:
                d[fw] = {"total": g("resize", "configure_to_present_p50"), "commit": g("resize", "configure_to_commit_p50"), "ack": g("resize", "configure_to_ack_p50")}
        out[pid] = {"label": label, "kind": kind, "fws": d}
    return out

STARTUP_F = ["connected_ms", "first_surface_ms", "toplevel_ms", "first_buffer_ms", "first_present_ms"]

def startups(fl, fws):
    out = {}
    for pid, label, sc, rows in (("rows3k", "rows 3,000", "rows", 3000), ("virtual100k", "virtual list 100,000 rows", "virtual", 100000), ("spinner0", "spinner, no rows", "spinner", 0)):
        d = {}
        for fw in fws:
            row = fl.start.get((sc, fw, "default", str(rows), "None", "60"))
            if row is None or fl.cell_failure(fw, sc, rows, "default", "None", "60", "startup") or row["first_present_ms"] in ("None", ""):
                d[fw] = None
            else:
                d[fw] = {k: (float(row[k]) if row[k] not in ("None", "") else None) for k in STARTUP_F}
        out[pid] = {"label": label, "fws": d}
    return out

def spread(a, b):
    """Run-to-run spread of one value between two runs, both valid: relative difference to the lower."""
    if a["st"] == "ok" and b["st"] == "ok" and a["v"] and b["v"]:
        return abs(a["v"] - b["v"]) / min(a["v"], b["v"])
    return None

ANIM_SPEC = (
    ("fps", "presented frames per second", "pacing", "frame_pacing", "fps", 1, "fps", 1),
    ("int50", "frame interval p50", "pacing", "frame_pacing", "interval_p50", 1, "ms", 2),
    ("int99", "frame interval p99", "pacing", "frame_pacing", "interval_p99", 1, "ms", 2),
    ("jank", "jank fraction", "pacing", "frame_pacing", "jank_fraction", 1, "fraction", 3),
    ("missed", "missed vblanks", "pacing", "frame_pacing", "missed_vblanks", 1, "count", 0),
    ("cpuf", "CPU per frame", "pacing", "frame_cost", "cpu_ms_per_frame", 1, "ms", 2),
    ("cpus", "CPU per second", "pacing", "resources", "cpu_ms_per_s", 1, "ms/s", 1),
    ("rss", "RSS", "pacing", "resources", "rss_max", MIB, "MiB", 1),
    ("dmg", "damage fraction p50", "pacing", "frame_cost", "damage_fraction_p50", 1, "fraction", 3),
)

def mode_data(runs, cmpn, secondn, heavyn, fps_cap):
    """Everything one timing mode shows: headline columns (cmp run, optional second run), splits, startup breakdown,
    spinner animation, heavy scenes, and the estimates fitted on this mode's own valid cells."""
    if cmpn not in runs:
        return None
    fl, meta = runs[cmpn]
    fws = meta["fws"]
    cols = cols_cmp(fl, fws)
    if secondn and secondn in runs:
        cols2 = cols_cmp(runs[secondn][0], fws)
        for c in cols:
            for fw in fws:
                b = cols2[c]["vals"][fw]
                cols[c]["vals"][fw]["run2"] = b
                cols[c]["vals"][fw]["spread"] = spread(cols[c]["vals"][fw], b)
    m = {"splits": splits(fl, fws), "startups": startups(fl, fws),
         "headline": {"run": cmpn, "second": secondn if secondn and secondn in runs else None, "fws": fws, "cols": cols}}
    anim = {}
    for cid, label, rows in (("a0", "spinner, no rows", 0), ("a10k", "spinner above 10,000 rows", 10000)):
        for mid, ml, rep, sec, met, scale, unit, nd in ANIM_SPEC:
            anim[f"{cid}_{mid}"] = {"label": f"{label}: {ml}", "unit": unit, "nd": nd, "scene": label, "metric": ml,
                                    "cls": ("cpu per frame (ms)" if mid == "cpuf" else "frame interval (ms)") if unit == "ms" else {"MiB": "resident memory (MiB)", "ms/s": "idle CPU (ms/s)", "fps": "frame rate (fps)"}.get(unit, "other"),
                                    "vals": {fw: fl.get(fw, "spinner", rep, sec, met, rows, scale=scale) for fw in fws}}
    m["animation"] = {"fws": fws, "cols": anim}
    m["heavy"] = None
    if heavyn and heavyn in runs:
        hfl, hmeta = runs[heavyn]
        m["heavy"] = {"run": heavyn, "measured_fws": hmeta["fws"], "cols": cols_heavy(hfl, hmeta["fws"])}
        for c, d in m["heavy"]["cols"].items():
            for fw in fws:
                d["vals"].setdefault(fw, {"v": None, "st": "miss", "fail": [], "warn": [], "msg": "this scene was never ported to this framework", "ported": False})
    M, cls = {}, {}
    for src in (m["headline"], m["heavy"], m["animation"]):
        if not src: continue
        for c, d in src["cols"].items():
            if d["unit"] in ("fraction", "count"): continue
            M[c] = {fw: (d["vals"].get(fw, {"v": None, "st": "miss"})["v"], d["vals"].get(fw, {"st": "miss"})["st"]) for fw in fws}
            cls[c] = d["cls"]
    E = estimate.estimates(M, cls)
    m["estimate_validation"] = estimate.validate(M, cls, keep_family={"spin0", "spin0_fps", "spin0_rss", "spin0_idle", "spin0_start"})
    for src in (m["headline"], m["heavy"], m["animation"]):
        if not src: continue
        for c, d in src["cols"].items():
            for fw, e in E.get(c, {}).items():
                if e["class"] == "frame rate (fps)" and fps_cap:
                    e["v"] = min(e["v"], fps_cap); e["hi"] = min(e["hi"], fps_cap); e["lo"] = min(e["lo"], e["v"])
                e["v"], e["lo"], e["hi"] = estimate.sig2(e["v"]), estimate.sig2(e["lo"]), estimate.sig2(e["hi"])
                msg = (d["vals"].get(fw) or {}).get("msg", "")
                e["reason"] = "failed cell" if "failed" in msg else ("scene never ported to this framework" if "ported" in msg else "no measurement")
            d["est"] = {fw: e for fw, e in E.get(c, {}).items() if d["vals"].get(fw, {"st": "miss"})["st"] == "miss"}
    return m

def add_prev(m):
    hv = m and m.get("heavy")
    prevp = os.path.join(ROOT, "..", "..", "r7-bench", "bench", "results", "r7", "guibench", "r7-scenes-1", "metrics.tsv")
    if not hv or not os.path.exists(prevp):
        return
    P = {}
    for r in tsv(prevp):
        P[(r["scene"], r["framework"], r["size"], r["probe"], r["metric"])] = (float(r["value"]), r["valid"] == "True")
    for cid, label, sc, n, rep, sec, met, scale, unit, nd, cls in heavy_cols():
        hv["cols"][cid]["prev"] = {fw: {"v": P[(sc, fw, str(n), rep, f"{sec}.{met}")][0] * scale, "valid": P[(sc, fw, str(n), rep, f"{sec}.{met}")][1]}
                                   for fw in hv["measured_fws"] if (sc, fw, str(n), rep, f"{sec}.{met}") in P}
    for sc, sizes in (("sheet", (1000, 10000)), ("canvas", (1000, 5000))):
        for n in sizes:
            hv["cols"][f"{sc}{n}_start"]["prev"] = {fw: {"v": P[(sc, fw, str(n), "startup", "first_present_ms")][0], "valid": True}
                                                  for fw in hv["measured_fws"] if (sc, fw, str(n), "startup", "first_present_ms") in P}

MODES = (  # key, title, timing, cmp run, second run, heavy run, fps cap for estimates
    ("unthrottled", "Unthrottled", "unthrottled", "cmp-u1", "cmp-u2", "heavy-u1", None),
    ("fixed", "Fixed 60 Hz grid", "fixed 60 Hz", "cmp-2", "cmp-3", "heavy-1", 60.0),
    ("vrr", "Variable refresh", "vrr 48-144 Hz", "cmp-v1", None, "heavy-v1", 144.0),
)

def main():
    out = {"generated": None, "runs": {}, "modes": {}, "sweep": None, "profiles": None}
    runs = {}
    for n in ("cmp-2", "cmp-3", "heavy-1", "sweep-60", "sweep-120", "sweep-240", "cmp-u1", "cmp-u2", "heavy-u1", "cmp-v1", "heavy-v1", "smoke-u1", "smoke-v1"):
        r = load_run(n)
        if r:
            runs[n] = r; out["runs"][n] = r[1]
    for key, title, timing, cmpn, secn, hn, cap in MODES:
        m = mode_data(runs, cmpn, secn, hn, cap)
        if m:
            add_prev(m)
            m.update({"key": key, "title": title, "timing": timing})
            out["modes"][key] = m
    # compatibility: the fixed-grid mode also at top level
    for k in ("headline", "splits", "startups", "animation", "heavy"):
        out[k] = out["modes"].get("fixed", {}).get(k)
    sw = {}
    for hz in (60, 120, 240):
        n = f"sweep-{hz}"
        if n not in runs: continue
        fl, meta = runs[n]
        cells = {}
        for fw in meta["fws"]:
            g = lambda sc, rows, rep, sec, met, scale=1: fl.get(fw, sc, rep, sec, met, rows, hz=str(hz), scale=scale)
            cells[fw] = {
                "hover_total": g("rows", 3000, "hover", "latency", "total_p50"),
                "hover_commit": g("rows", 3000, "hover", "latency", "input_to_commit_p50"),
                "hover_ready": g("rows", 3000, "hover", "latency", "commit_to_ready_p50"),
                "hover_latch": g("rows", 3000, "hover", "latency", "ready_to_latch_p50"),
                "hover_present": g("rows", 3000, "hover", "latency", "latch_to_present_p50"),
                "typing_p50": g("typing", 1000, "typing", "typing", "latency_p50"),
                "typing_p95": g("typing", 1000, "typing", "typing", "latency_p95"),
                "typing_h": g("typing", 1000, "typing", "typing", "handling_p50"),
                "spin_fps": g("spinner", 10000, "pacing", "frame_pacing", "fps"),
                "spin_int99": g("spinner", 10000, "pacing", "frame_pacing", "interval_p99"),
                "spin_cpuf": g("spinner", 10000, "pacing", "frame_cost", "cpu_ms_per_frame"),
                "spin0_fps": g("spinner", 0, "pacing", "frame_pacing", "fps"),
                "spin0_cpuf": g("spinner", 0, "pacing", "frame_cost", "cpu_ms_per_frame"),
                "hover100_total": g("rows", 100, "hover", "latency", "total_p50"),
            }
        sw[str(hz)] = {"run": n, "fws": meta["fws"], "cells": cells}
    out["sweep"] = sw or None
    pj = os.path.join(RES, "profiles", "profiles.json")
    if os.path.exists(pj):
        out["profiles"] = json.load(open(pj))
    import datetime
    out["generated"] = datetime.datetime.utcnow().strftime("%Y-%m-%dT%H:%M:%SZ")
    dest = os.path.join(ROOT, "dashboard", "data.r8.json")
    json.dump(out, open(dest, "w"), indent=0)
    print("wrote", dest, os.path.getsize(dest), "bytes; runs:", list(out["runs"]), "modes:", list(out["modes"]))

if __name__ == "__main__":
    main()
