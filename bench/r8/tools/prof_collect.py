"""prof_collect.py : results/profiles -> results/profiles/profiles.json (an HTML fragment for the dashboard and the numbers for
RESULTS.md). Profiles only; nothing here is a timing."""
import os, sys, json, glob, html, base64
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
P = os.path.join(ROOT, "results", "profiles")
def esc(t): return html.escape(str(t), quote=True)

def load_stat(path):
    r = {}
    if not os.path.exists(path): return r
    for l in open(path):
        if l.startswith("#") or l.startswith("group"): continue
        f = l.rstrip("\n").split("\t")
        if len(f) < 7: continue
        try: per = float(f[4])
        except ValueError: continue
        r.setdefault(f[1], (per, f[5], f[6]))
    return r

PATHS = [("rows3k-hover", "rows 3,000 hover"), ("table-select", "table fine select"), ("table-update10", "table fine update 10th rows"), ("table-swap", "table fine swap"),
         ("table-remove", "table fine remove"), ("table-insert", "table fine insert"), ("table-replace", "table fine replace"), ("table-append", "table fine append"), ("table-clear", "table fine clear")]

def row_from(a, i):
    ins = a.get("instructions", (float("nan"),))[0]
    cyc = a.get("cycles", (float("nan"),))[0]
    return {"instr": ins, "cycles": cyc, "ipc": ins / cyc if cyc else float("nan"), "l1": a.get("L1-dcache-load-misses", (float("nan"),))[0],
            "l2": a.get("cache-misses", (float("nan"),))[0], "branches": a.get("branches", (float("nan"),))[0], "bmiss": a.get("branch-misses", (float("nan"),))[0],
            "ind": i.get("bp_dyn_ind_pred", (float("nan"),))[0], "indmiss": i.get("ex_ret_brn_ind_misp", (float("nan"),))[0], "run": a.get("cycles", (0, 0, "?"))[2]}

def main():
    rows = []
    for key, label in PATHS:
        a = load_stat(os.path.join(P, "stat", key + ".tsv")); i = load_stat(os.path.join(P, "stat", key + ".ind.tsv"))
        if not a: continue
        r = row_from(a, i); r.update({"key": key, "label": label, "kind": "headless"})
        tp = os.path.join(P, "top", key + ".json")
        if os.path.exists(tp): r["top"] = json.load(open(tp))
        sv = os.path.join(P, "svg", key + ".svg")
        if os.path.exists(sv): r["svg"] = key + ".svg"
        rows.append(r)
    CELLS = [("sheet10k-hover", "sheet 10,000 rows hover (gui-bench cell)"), ("sheet10k-theme", "sheet 10,000 rows theme switch (gui-bench cell)"), ("canvas5k-hover", "canvas 5,000 nodes hover (gui-bench cell)")]
    for key, label in CELLS:
        sp = os.path.join(P, f"stat-{key}.json"); pp = os.path.join(P, f"params-{key}.json")
        if not os.path.exists(sp): continue
        st = json.load(open(sp)); pr = json.load(open(pp))
        reps = pr["params"].get("repeats", 1); nit = len([x for x in pr["iterations"] if x["measured"] is not None]); ops = reps * nit
        def ev(g, e):
            return st.get(g, {}).get("events", {}).get(e)
        d = lambda a, b: (a / b) if a is not None and b else float("nan")
        r = {"key": key, "label": label, "kind": "cell", "ops": ops, "ops_note": f"{nit} measured iterations x {reps} repeats; one repeat = pointer onto the target and away (hover) or one switch (theme)",
             "instr": d(ev("cyc", "instructions"), ops), "cycles": d(ev("cyc", "cycles"), ops), "branches": d(ev("cyc", "branches"), ops), "bmiss": d(ev("br", "branch-misses"), ops),
             "l1": d(ev("l1", "L1-dcache-load-misses"), ops), "l2": d(ev("l2", "cache-misses"), ops), "ind": float("nan"), "indmiss": float("nan"),
             "window_s": st.get("cyc", {}).get("window_seconds"), "run": "n/a"}
        r["ipc"] = d(ev("cyc", "instructions"), ev("cyc", "cycles"))
        r["l1_per_kinstr"] = 1000 * d(ev("l1", "L1-dcache-load-misses"), ev("l1", "instructions")); r["l2_per_kinstr"] = 1000 * d(ev("l2", "cache-misses"), ev("l2", "instructions"))
        r["bmiss_per_kinstr"] = 1000 * d(ev("br", "branch-misses"), ev("br", "instructions"))
        tp = os.path.join(P, "top", key + ".json")
        if os.path.exists(tp): r["top"] = json.load(open(tp))
        if os.path.exists(os.path.join(P, "svg", key + ".svg")): r["svg"] = key + ".svg"
        rows.append(r)
    f = lambda v, d=0: "-" if v is None or v != v else (f"{v:,.{d}f}")
    th = "".join(f"<th>{h}</th>" for h in ["path", "instructions / op", "cycles / op", "IPC", "L1d misses / op", "L2 misses / op", "branches / op", "branch misses / op", "indirect predictions / op", "indirect mispredicts / op"])
    trs = []
    for r in rows:
        r.setdefault("l1_per_kinstr", 1000 * r["l1"] / r["instr"]); r.setdefault("l2_per_kinstr", 1000 * r["l2"] / r["instr"]); r.setdefault("bmiss_per_kinstr", 1000 * r["bmiss"] / r["instr"])
        trs.append(f'<tr><th scope="row">{esc(r["label"])}</th><td>{f(r["instr"])}</td><td>{f(r["cycles"])}</td><td>{f(r["ipc"], 2)}</td><td>{f(r["l1"])}</td><td>{f(r["l2"])}</td><td>{f(r["branches"])}</td><td>{f(r["bmiss"])}</td><td>{f(r["ind"])}</td><td>{f(r["indmiss"])}</td></tr>')
    h = ['<p class="cap">Per operation (one input and its frame; hover on or off, one table key press). Counts are user-mode, from two repeat counts so that mount and process start cancel (slope); each group of three events runs on its own, pinned to one CPU. '
         'L1d misses = <span class="mono">L1-dcache-load-misses</span> (requests from the data cache to L2); L2 misses = <span class="mono">cache-misses</span> on this Zen 2 CPU. Indirect = Zen 2 <span class="mono">bp_dyn_ind_pred</span> (indirect branches predicted by the indirect predictor) and <span class="mono">ex_ret_brn_ind_misp</span>. '
         'The PMU was shared with another lane\'s perf session, so each group was on the counters about 75 % of the time (counts are scaled; ratios inside a group are exact).</p>',
         f'<div class="scroll"><table class="vt"><thead><tr>{th}</tr></thead><tbody>{"".join(trs)}</tbody></table></div>']
    for r in rows:
        if "top" in r:
            t = r["top"]
            lis = "".join(f'<tr><td>{esc(x["symbol"])}</td><td>{x["pct"]:.1f} %</td><td>{(y["pct"] if y else 0):.1f} %</td><td>{esc(y["symbol"][:70]) if y else ""}</td></tr>' for x, y in zip(t["top10"], t.get("top10_attributed", [None] * 10) + [None] * 10))
            img = ""
            if r.get("svg"):
                b = base64.b64encode(open(os.path.join(P, "svg", r["svg"]), "rb").read()).decode()
                img = f'<img class="flame" alt="Flamegraph of {esc(r["label"])}" src="data:image/svg+xml;base64,{b}">'
            disp = ", ".join(f'{esc(d["symbol"][:60])} ({d["inclusive_pct"]:.1f} %)' for d in t["dispatch_like_frames"][:4]) or "none"
            h.append(f'<details class="tbl"><summary>{esc(r["label"])}: top ten symbols by self time and flamegraph <span class="mono">{esc(r.get("svg", ""))}</span></summary>'
                     f'<table class="vt symtab"><thead><tr><th>top ten by self time</th><th>self</th><th>charged to the nearest Rust frame</th><th>symbol</th></tr></thead><tbody>{lis}</tbody></table>'
                     f'<p class="small-note">{t.get("total_cycles_weighted_samples", t.get("total_samples", 0)):,} cycles sampled ({esc(t.get("process", ""))}; {t.get("other_processes_pct", 0):.1f} % of the recording was in other processes and is excluded). Frames that look like dispatch (closures, call_once, vtable or shim frames): {t["dispatch_like_self_pct"]:.1f} % of self time; inclusive: {disp}.</p>{img}</details>')
    R = {r["key"]: r for r in rows}
    def tp(k, n=3, attr=False):
        t = R.get(k, {}).get("top", {}); lst = t.get("top10_attributed" if attr else "top10", [])[:n]
        return ", ".join(f'{esc(x["symbol"].replace("bough::", "").replace("<bough::", "<")[:46])} {x["pct"]:.0f} %' for x in lst)
    find = []
    if "sheet10k-hover" in R:
        r = R["sheet10k-hover"]; t = r.get("top", {})
        find.append(f'<li><b>Sheet 10,000 rows hover looks like dispatch and pointer chasing.</b> {r["instr"]/1e6:.0f} M instructions per hover repeat at IPC {r["ipc"]:.2f}, {r["l1_per_kinstr"]:.0f} L1d and {r["l2_per_kinstr"]:.1f} L2 misses per 1,000 instructions '
                    f'(the table operations run at IPC 1.8 to 2.6 and 3.5 to 6 L1d misses per 1,000). Top self time: {tp("sheet10k-hover", 5)}. <span class="mono">Rt::run_head</span> is the wire-chain walk; <span class="mono">SealCheck::word</span> is a per-word integrity check. '
                    f'Closure, call_once and chain frames account for {t.get("dispatch_like_self_pct", 0):.0f} % of self time. The indirect-branch counters were measured on the headless paths only (the attached cells produced no indirect-branch counts).</li>')
    if "canvas5k-hover" in R:
        r = R["canvas5k-hover"]
        find.append(f'<li><b>Canvas 5,000 nodes hover is arithmetic, not memory.</b> {r["instr"]/1e9:.1f} G instructions per repeat at IPC {r["ipc"]:.2f} with {r["l1_per_kinstr"]:.1f} L1d and {r["l2_per_kinstr"]:.2f} L2 misses per 1,000 instructions. Top self time: {tp("canvas5k-hover", 5)}: the CPU executor evaluates blurred shadows with an exp-based error function per pixel (shadow, erf2, expf about 60 % together).</li>')
    if "rows3k-hover" in R:
        r = R["rows3k-hover"]
        find.append(f'<li><b>Rows hover through the headless driver is copying and allocation.</b> {r["instr"]/1e6:.1f} M instructions per hover at IPC {r["ipc"]:.2f}; top self time {tp("rows3k-hover", 6)}. memmove, memcmp, malloc, free and realloc are about 85 % of the cycles (with memset); only {r["ind"]:.0f} indirect predictions and {r["indmiss"]:.0f} indirect mispredicts per operation, so dispatch is not the cost here. The driver ends at the CPU executor, without the window host.</li>')
    for k in ("table-remove", "table-update10"):
        if k in R:
            r = R[k]
            find.append(f'<li><b>{esc(r["label"])}:</b> {r["instr"]/1e6:.1f} M instructions per operation, IPC {r["ipc"]:.2f}; top self time {tp(k, 4)}.</li>')
    out = {"rows": rows, "html": "\n".join(h[:2]) + '<h3>What the profiles show</h3><ul>' + "".join(find) + '</ul>' + "\n".join(h[2:])}
    json.dump(out, open(os.path.join(P, "profiles.json"), "w"), indent=0)
    print("profiles.json", len(rows), "paths")
main()
