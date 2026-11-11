#!/usr/bin/env python3
"""Rerun the slots of an out.json whose runs were rejected (error entries), with the same functions and parameters as run.py.
  fill_missing.py <bins> <out.json> <app>"""
import json, os, sys, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import run
bins, out, app = sys.argv[1:4]
run.SHOTS = os.path.join(os.path.dirname(run.TOOLS), "shots-final")
res = json.load(open(out))
lock = os.path.join(os.path.dirname(run.TOOLS), "MEASURING.lock")
open(lock, "w").write(f"fill_missing measuring {app} -> {out}, started {time.strftime('%H:%M:%S')}\n")
try:
    for key, reps in res[app].items():
        for i, r in enumerate(reps):
            if "error" not in r:
                continue
            if key == "dashboard":
                new = run.retried(lambda: run.run_dashboard(run.command(bins, app, "dashboard"), 1.0, tag=f"{app}-dashboard" if i == 0 else None), tries=6)
            else:
                rows = int(key.split("-")[1])
                new = run.retried(lambda: run.run_scroll(run.command(bins, app, "scroll"), rows, 1.0, 3.0, tag=f"{app}-scroll-{rows}" if i == 0 else None), tries=6)
            reps[i] = new
            print(app, key, i, {k: (round(v, 2) if isinstance(v, float) else v) for k, v in new.items() if k != "phases"}, flush=True)
            json.dump(res, open(out, "w"), indent=1)
finally:
    try: os.remove(lock)
    except OSError: pass
