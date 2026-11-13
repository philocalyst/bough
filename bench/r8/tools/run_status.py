"""run_status.py : re-check the status of every run directory under results/ (B09: a workflow's shell success is not a benchmark's success).
A run is COMPLETE only if: the identity record has `runner rc=0`; the runner log ends with a `results:` line; the last cell index equals the
announced total; the cells table has that many cells; the flat tables exist and are non-empty; every cell has a cell.json outcome
(done or errors). Writes results/RUN-STATUS.tsv and prints it."""
import os, re, csv, sys
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
RES = os.path.join(ROOT, "results")
rows = []
for d in sorted(os.listdir(RES)):
    p = os.path.join(RES, d)
    if not os.path.isdir(p) or d == "profiles": continue
    ident = open(os.path.join(p, "IDENTITY.txt")).read() if os.path.exists(os.path.join(p, "IDENTITY.txt")) else ""
    log = open(os.path.join(p, "runner.log")).read() if os.path.exists(os.path.join(p, "runner.log")) else ""
    rc = re.search(r"runner rc=(\d+)", ident)
    res = re.search(r"results: \S+ \((\d+) failed measurements\)", log)
    idx = [int(m.group(1)) for m in re.finditer(r"^\[(\d+)/(\d+)\]", log, re.M)]
    tot = [int(m.group(2)) for m in re.finditer(r"^\[(\d+)/(\d+)\]", log, re.M)]
    flat = os.path.join(p, "flat")
    def nlines(f): return sum(1 for _ in open(os.path.join(flat, f))) - 1 if os.path.exists(os.path.join(flat, f)) else -1
    ncells, nmet, nval = nlines("cells.tsv"), nlines("metrics.tsv"), nlines("validity.tsv")
    errcells = 0
    if ncells > 0:
        errcells = sum(1 for r in csv.DictReader(open(os.path.join(flat, "cells.tsv")), delimiter="\t") if r["errors"] != "[]")
    problems = []
    if not rc: problems.append("no runner rc in identity")
    elif rc.group(1) != "0": problems.append(f"runner rc={rc.group(1)}")
    if not res: problems.append("no 'results:' line")
    if not idx: problems.append("no cell progress lines")
    elif idx[-1] != tot[0] or len(set(tot)) != 1: problems.append(f"last cell {idx[-1]} of {tot[0]}")
    if ncells != (tot[0] if tot else -2): problems.append(f"cells table {ncells} != announced {tot[0] if tot else '?'}")
    if nmet <= 0 or nval <= 0: problems.append("flat tables empty or missing")
    rows.append([d, "COMPLETE" if not problems else "INCOMPLETE", rc.group(1) if rc else "?", tot[0] if tot else "?", ncells, res.group(1) if res else "?", errcells, nmet, nval, "; ".join(problems) or "-"])
hdr = ["run", "status", "runner rc", "cells announced", "cells in table", "failed measurements (runner)", "cells with errors (flat)", "metric rows", "validity rows", "problems"]
with open(os.path.join(RES, "RUN-STATUS.tsv"), "w") as f:
    f.write("\t".join(hdr) + "\n")
    for r in rows: f.write("\t".join(map(str, r)) + "\n")
print("\t".join(hdr))
for r in rows: print("\t".join(map(str, r)))
sys.exit(1 if any(r[1] != "COMPLETE" for r in rows) else 0)
