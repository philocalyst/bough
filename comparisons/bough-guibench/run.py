#!/usr/bin/env python3
"""Isolated cell driver. Requires explicit --lock for measured batches; logs are private.

Usage: run.py --binary target/release/headless --out runtime/lanes/generalization/run --lock "$BOUGH_MEASURE_LOCK"
Gates may be run with --gates-only; they do not establish performance on a loaded machine.
"""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import statistics
import shutil
import subprocess
import time


def fit(points):
    xs, ys = zip(*points)
    xm, ym = statistics.mean(xs), statistics.mean(ys)
    slope = sum((x-xm)*(y-ym) for x,y in points) / sum((x-xm)**2 for x in xs)
    intercept = ym-slope*xm
    residual = max(abs(y-(intercept+slope*x)) for x,y in points)
    return dict(slope=slope, intercept=intercept, max_residual=residual)


def scaling(points, ideal):
    if len(points) < 2 or any(y <= 0 for _,y in points):
        return dict(verdict="UNKNOWN", reason="fewer than two positive supported sizes")
    points = sorted(points)
    adjacent = [math.log(b[1]/a[1])/math.log(b[0]/a[0]) for a,b in zip(points,points[1:])]
    exponent = fit([(math.log(n),math.log(y)) for n,y in points])["slope"]
    bad = max(exponent, adjacent[-1]) > ideal+0.20
    if ideal == 0:
        bad |= any((b[1]/a[1])**(1/math.log10(b[0]/a[0])) > 2 for a,b in zip(points,points[1:]))
    return dict(exponent=exponent, adjacent=adjacent, ideal=ideal, verdict="OVERFIT" if bad else "within sampled bound")


def run_cell(binary, out, args, tag):
    env = dict(os.environ, GUIBENCH_FAILURE_DIR=str(out/tag))
    command = [shutil.which("time") or "time", "-l", str(binary), *args]
    t = time.monotonic()
    try:
        p = subprocess.run(command, capture_output=True, text=True, env=env, timeout=120)
        result = dict(command=command, code=p.returncode, stdout=p.stdout, stderr=p.stderr)
    except subprocess.TimeoutExpired as e:
        def txt(s): return s.decode(errors="replace") if isinstance(s, bytes) else (s or "")
        result = dict(command=command, code="timeout", stdout=txt(e.stdout), stderr=txt(e.stderr))
    result["elapsed_s"] = time.monotonic()-t
    (out/(tag+".json")).write_text(json.dumps(result,indent=2)+"\n")
    print(tag, result["code"], flush=True)
    return result


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--binary",type=Path,required=True)
    p.add_argument("--out",type=Path,required=True)
    p.add_argument("--lock",type=Path)
    p.add_argument("--gates-only",action="store_true")
    p.add_argument("--mutant",action="store_true")
    p.add_argument("--sizes",default="10,100,1000,10000")
    p.add_argument("--rings",default="1,2,3,4")
    p.add_argument("--operations",default="hover,scroll,select,update10,swap,remove,insert,replace,append,truncate,clear")
    a = p.parse_args()
    a.out.mkdir(parents=True,exist_ok=False)
    a.binary = a.binary.resolve()
    (a.out/"provenance.json").write_text(json.dumps(dict(binary=str(a.binary),sha256=hashlib.sha256(a.binary.read_bytes()).hexdigest(),
        commit=subprocess.check_output(["git","rev-parse","HEAD"],text=True).strip(),gates_only=a.gates_only),indent=2)+"\n")
    held = False
    if not a.gates_only:
        if a.lock is None: p.error("measurement requires --lock")
        a.lock.mkdir()  # fail closed, never remove another lane's lock
        (a.lock/"owner").write_text("generalization\n")
        held = True
    try:
        records = []
        for scenario, variant in [("rows","fine"),("table","fine"),("table","coarse")]:
            ops = [op for op in a.operations.split(',') if (op in ("hover","scroll")) == (scenario == "rows")]
            for n in map(int,a.sizes.split(',')):
                for op in ops:
                    base = ["--scenario",scenario,"--variant",variant,"--rows",str(n),"--op",op] + (["--mutant"] if a.mutant else [])
                    prefix = f"{scenario}-{variant}-{n}-{op}"
                    gates = []
                    for ring in map(int,a.rings.split(',')):
                        result = run_cell(a.binary,a.out,base+["--gate","--repeats","4","--ring",str(ring)],prefix+f"-gate-r{ring}")
                        gates.append(result["code"])
                    rec = dict(scenario=scenario,variant=variant,rows=n,op=op,gates=gates)
                    if not a.gates_only:
                        for render in [False,True]:
                            fits = []
                            for rep in range(3):
                                points = []
                                for repeats in ([8,32,128] if rep % 2 == 0 else [128,32,8]):
                                    result = run_cell(a.binary,a.out,base+["--repeats",str(repeats)]+(["--render"] if render else []),prefix+f"-R{int(render)}-{rep}-{repeats}")
                                    import re
                                    m = re.search(r"(\d+)\s+instructions retired",result["stderr"])
                                    if result["code"] == 0 and m: points.append((repeats,int(m[1])))
                                if len(points) == 3: fits.append(fit(points))
                            rec[f"R{int(render)}"] = fits
                    records.append(rec)
                    (a.out/"cells.json").write_text(json.dumps(records,indent=2)+"\n")
        if not a.gates_only:
            groups = {}
            for rec in records:
                for phase in ["R0", "R1"]:
                    fits = rec.get(phase, [])
                    slopes = [f["slope"] for f in fits]
                    median = statistics.median(slopes) if slopes else 0
                    valid = len(slopes) == 3 and min(slopes) > 0 and (max(slopes)-min(slopes))/median <= .20
                    rec[phase+"_summary"] = dict(pair_instructions=median if valid else None,
                        amortized_op_instructions=median/2 if valid else None,
                        verdict="stable" if valid else "UNKNOWN", pixels=all(code == 0 for code in rec["gates"]))
                    if valid:
                        key = (rec["scenario"],rec["variant"],rec["op"],phase)
                        groups.setdefault(key,[]).append((rec["rows"],median/2))
            exponents = []
            for key,points in groups.items():
                ideal = int(key[2] in ("update10","replace","clear"))
                exponents.append(dict(scenario=key[0],variant=key[1],op=key[2],phase=key[3],
                    points=points, **scaling(points,ideal)))
            (a.out/"scaling.json").write_text(json.dumps(exponents,indent=2)+"\n")
            (a.out/"cells.json").write_text(json.dumps(records,indent=2)+"\n")
    finally:
        if held:
            (a.lock/"owner").unlink()
            a.lock.rmdir()

if __name__ == "__main__": main()
