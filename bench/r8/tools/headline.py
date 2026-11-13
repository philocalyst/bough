"""headline.py RUN_FLAT_DIR [--json OUT] : the headline table of a gui-bench run (cmp-2 / cmp-3 style) from flat/*.tsv.

Every value carries a state: ok (the harness judged that report valid), inv (a validity check failed: shown with a
dagger, never ranked), miss (no measurement).  Startup comes from startup.tsv (no validity report: ok unless the
cell errored).  Column spec COLS is shared with the dashboard builder."""
import sys, os, csv, json
sys.path.insert(0, os.path.dirname(__file__))
import gb

MIB = 1 / 1048576
# id, label, scenario, variant, rows, updates, report, section, metric, scale, unit, decimals
COLS = [
 ("hover3k", "rows 3,000 hover total p50", "rows", "default", 3000, "None", "hover", "latency", "total_p50", 1, "ms", 2),
 ("hover3k_c", "rows 3,000 hover input to commit p50", "rows", "default", 3000, "None", "hover", "latency", "input_to_commit_p50", 1, "ms", 2),
 ("hover100", "rows 100 hover total p50", "rows", "default", 100, "None", "hover", "latency", "total_p50", 1, "ms", 2),
 ("idle_rss3k", "rows 3,000 idle RSS", "rows", "default", 3000, "None", "idle", "resources", "rss_max", MIB, "MiB", 1),
 ("idle_cpu3k", "rows 3,000 idle CPU", "rows", "default", 3000, "None", "idle", "resources", "cpu_ms_per_s", 1, "ms/s", 2),
 ("scroll3k", "rows 3,000 scroll CPU per frame", "rows", "default", 3000, "None", "scroll", "frame_cost", "cpu_ms_per_frame", 1, "ms", 2),
 ("scroll3k_fps", "rows 3,000 scroll fps", "rows", "default", 3000, "None", "scroll", "frame_pacing", "fps", 1, "fps", 1),
 ("vscroll", "virtual 100,000 scroll CPU per frame", "virtual", "default", 100000, "None", "scroll", "frame_cost", "cpu_ms_per_frame", 1, "ms", 2),
 ("vrss", "virtual 100,000 idle RSS", "virtual", "default", 100000, "None", "idle", "resources", "rss_max", MIB, "MiB", 1),
 ("typing", "typing p50", "typing", "default", 1000, "None", "typing", "typing", "latency_p50", 1, "ms", 2),
 ("search", "search filter total p50", "search", "default", 1000, "None", "filter", "latency", "total_p50", 1, "ms", 2),
 ("resize", "resize configure to present p50", "resize", "default", 1000, "None", "resize", "resize", "configure_to_present_p50", 1, "ms", 2),
 ("spin10k", "spinner 10,000 CPU per frame", "spinner", "default", 10000, "None", "pacing", "frame_cost", "cpu_ms_per_frame", 1, "ms", 2),
 ("spin0", "spinner (no rows) CPU per frame", "spinner", "default", 0, "None", "pacing", "frame_cost", "cpu_ms_per_frame", 1, "ms", 2),
 ("threads3k", "threads (max, rows 3,000 idle)", "rows", "default", 3000, "None", "idle", "resources", "threads_max", 1, "count", 0),
 ("spin0_fps", "spinner (no rows) fps", "spinner", "default", 0, "None", "pacing", "frame_pacing", "fps", 1, "fps", 1),
 ("spin0_rss", "spinner (no rows) RSS", "spinner", "default", 0, "None", "idle", "resources", "rss_max", MIB, "MiB", 1),
 ("spin0_idle", "spinner (no rows) idle CPU", "spinner", "default", 0, "None", "idle", "resources", "cpu_ms_per_s", 1, "ms/s", 2),
 ("spin10k_fps", "spinner 10,000 fps", "spinner", "default", 10000, "None", "pacing", "frame_pacing", "fps", 1, "fps", 1),
 ("tbl_remove", "table fine remove total p50", "table", "fine", 1000, "None", "op-remove", "latency", "total_p50", 1, "ms", 2),
 ("tbl_remove_cpu", "table fine remove CPU per frame", "table", "fine", 1000, "None", "op-remove", "frame_cost", "cpu_ms_per_frame", 1, "ms", 2),
 ("tbl_update", "table fine update10 total p50", "table", "fine", 1000, "None", "op-update10", "latency", "total_p50", 1, "ms", 2),
 ("tbl_swap", "table fine swap total p50", "table", "fine", 1000, "None", "op-swap", "latency", "total_p50", 1, "ms", 2),
 ("tick100", "ticker fine 100 updates CPU per frame", "ticker", "fine", 1000, "100", "pacing", "frame_cost", "cpu_ms_per_frame", 1, "ms", 2),
 ("tick100_fps", "ticker fine 100 updates fps", "ticker", "fine", 1000, "100", "pacing", "frame_pacing", "fps", 1, "fps", 1),
 ("hover100_c", "rows 100 hover input to commit p50", "rows", "default", 100, "None", "hover", "latency", "input_to_commit_p50", 1, "ms", 2),
 ("tbl_remove_c", "table fine remove input to commit p50", "table", "fine", 1000, "None", "op-remove", "latency", "input_to_commit_p50", 1, "ms", 2),
 ("tbl_update_c", "table fine update10 input to commit p50", "table", "fine", 1000, "None", "op-update10", "latency", "input_to_commit_p50", 1, "ms", 2),
 ("tbl_swap_c", "table fine swap input to commit p50", "table", "fine", 1000, "None", "op-swap", "latency", "input_to_commit_p50", 1, "ms", 2),
 ("search_c", "search filter input to commit p50", "search", "default", 1000, "None", "filter", "latency", "input_to_commit_p50", 1, "ms", 2),
 ("typing_h", "typing handling p50 (app share)", "typing", "default", 1000, "None", "typing", "typing", "handling_p50", 1, "ms", 2),
 ("resize_c", "resize configure to commit p50 (app share)", "resize", "default", 1000, "None", "resize", "resize", "configure_to_commit_p50", 1, "ms", 2),
 ("hover3k_cpu", "rows 3,000 hover CPU per frame", "rows", "default", 3000, "None", "hover", "frame_cost", "cpu_ms_per_frame", 1, "ms", 2),
 ("hover100_cpu", "rows 100 hover CPU per frame", "rows", "default", 100, "None", "hover", "frame_cost", "cpu_ms_per_frame", 1, "ms", 2),
 ("tbl_update_cpu", "table fine update10 CPU per frame", "table", "fine", 1000, "None", "op-update10", "frame_cost", "cpu_ms_per_frame", 1, "ms", 2),
 ("tbl_swap_cpu", "table fine swap CPU per frame", "table", "fine", 1000, "None", "op-swap", "frame_cost", "cpu_ms_per_frame", 1, "ms", 2),
 ("typing_cpu", "typing CPU per frame", "typing", "default", 1000, "None", "typing", "frame_cost", "cpu_ms_per_frame", 1, "ms", 2),
 ("search_cpu", "search filter CPU per frame", "search", "default", 1000, "None", "filter", "frame_cost", "cpu_ms_per_frame", 1, "ms", 2),
 ("resize_cpu", "resize CPU per frame", "resize", "default", 1000, "None", "resize", "frame_cost", "cpu_ms_per_frame", 1, "ms", 2),
]
# class of each column for estimate.py: one kind of quantity per class
CPUF = {"scroll3k", "vscroll", "spin10k", "spin0", "tbl_remove_cpu", "tick100", "hover3k_cpu", "hover100_cpu", "tbl_update_cpu", "tbl_swap_cpu", "typing_cpu", "search_cpu", "resize_cpu"}
def klass(cid, unit):
    if cid in CPUF: return "cpu per frame (ms)"
    if cid.startswith("start") or cid.endswith("_start"): return "startup (ms)"
    return {"ms": "latency (ms)", "MiB": "resident memory (MiB)", "ms/s": "idle CPU (ms/s)", "fps": "frame rate (fps)"}[unit]
HEAD = ["hover3k", "hover3k_c", "idle_rss3k", "idle_cpu3k", "scroll3k", "typing", "resize", "spin10k", "tbl_remove", "tbl_remove_cpu"]

def load(flat):
    r = gb.Run(flat)
    st = {}
    p = os.path.join(flat, "startup.tsv")
    if os.path.exists(p):
        for row in csv.DictReader(open(p), delimiter="\t"):
            st[(row["scenario"], row["framework"], row["variant"], row["rows"], row["updates"])] = row
    return r, st

def value(r, st, fw, col):
    cid, label, sc, var, rows, upd, rep, sec, met, scale, unit, nd = col
    v, s = r.get(fw, sc, rep, sec, met, rows, variant=var, updates=upd)
    if s == "miss" and var != "default":  # immediate-mode builds (egui) have one "default" variant of table and ticker
        v, s = r.get(fw, sc, rep, sec, met, rows, variant="default", updates=upd)
    return (None if v is None else v * scale), s

def startup(r, st, fw, sc, var, rows, upd):
    row = st.get((sc, fw, var, str(rows), str(upd)))
    err = r.cell_errors(fw, sc, rows, variant=var, updates=upd)
    if row is None or row["first_present_ms"] in ("None", ""):
        return None, "miss"
    if err and err != "[]":
        return None, "miss"
    return float(row["first_present_ms"]), "ok"

def table(flat):
    r, st = load(flat)
    out = {"fws": r.fws, "cols": {}}
    for col in COLS:
        out["cols"][col[0]] = {"label": col[1], "unit": col[10], "nd": col[11], "vals": {fw: value(r, st, fw, col) for fw in r.fws}}
    for cid, label, sc, var, rows, upd in (("start3k", "rows 3,000 startup to first present", "rows", "default", 3000, "None"),
                                          ("startv", "virtual 100,000 startup to first present", "virtual", "default", 100000, "None"),
                                          ("spin0_start", "spinner (no rows) startup to first present", "spinner", "default", 0, "None")):
        out["cols"][cid] = {"label": label, "unit": "ms", "nd": 1, "vals": {fw: startup(r, st, fw, sc, var, rows, upd) for fw in r.fws}}
    # validity per framework
    cnt = {}
    for (k, rep), ok in r.valid.items():
        c = cnt.setdefault(k[1], [0, 0]); c[0 if ok else 1] += 1
    out["reports"] = cnt
    out["cells"] = {}
    for k, c in r.cells.items():
        e = out["cells"].setdefault(k[1], [0, 0]); e[1 if c["errors"] != "[]" else 0] += 1
    return out

def md(t, ids):
    fws = t["fws"]
    lines = ["| framework | " + " | ".join(t["cols"][i]["label"] + " (" + t["cols"][i]["unit"] + ")" for i in ids) + " |",
             "|---|" + "---:|" * len(ids)]
    for fw in fws:
        cells = []
        for i in ids:
            v, s = t["cols"][i]["vals"][fw]
            cells.append(gb.fmt(v, s, t["cols"][i]["nd"]))
        lines.append(f"| {fw} | " + " | ".join(cells) + " |")
    return "\n".join(lines)

if __name__ == "__main__":
    flat = sys.argv[1]
    t = table(flat)
    ids = ["hover3k", "hover3k_c", "start3k", "idle_rss3k", "idle_cpu3k", "scroll3k", "typing", "resize", "spin10k", "tbl_remove", "tbl_remove_cpu"]
    print(md(t, ids))
    print()
    print("| framework | cells ok | cells with errors | reports valid | reports invalid |\n|---|---:|---:|---:|---:|")
    for fw in t["fws"]:
        print(f"| {fw} | {t['cells'][fw][0]} | {t['cells'][fw][1]} | {t['reports'].get(fw,[0,0])[0]} | {t['reports'].get(fw,[0,0])[1]} |")
    if "--json" in sys.argv:
        json.dump(t, open(sys.argv[sys.argv.index("--json") + 1], "w"), indent=1)
