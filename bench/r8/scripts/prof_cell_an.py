# prof_cell_an.py RUN_CELL_DIR PROBE MODE_DIR... : windows of a probe's measured iterations (the harness's own
# CLOCK_MONOTONIC nanosecond stamps), as `perf script --time` strings, or the sum of perf stat -I intervals inside them.
import sys, json, os, csv
cell, probe = sys.argv[1], sys.argv[2]
j = json.load(open(os.path.join(cell, probe + ".json")))
win = [(it["started"] / 1e9, it["finished"] / 1e9) for it in j["iterations"] if it.get("measured") is not None and it.get("ok", True)]
if len(sys.argv) == 3:
    for a, b in win: print(f"{a:.6f},{b:.6f}")
    sys.exit()
# stat mode: DIR with stat.csv and IDENTITY.txt (T0_uptime)
d = sys.argv[3]
t0 = float([l for l in open(os.path.join(d, "IDENTITY.txt")) if "T0_uptime" in l][0].split("T0_uptime=")[1].split()[0])
sums, used, total_span = {}, 0, sum(b - a for a, b in win)
for row in csv.reader(l for l in open(os.path.join(d, "stat.csv")) if l.strip() and not l.startswith("#")):
    try: ts = float(row[0]); val = float(row[1])
    except (ValueError, IndexError): continue
    ev = row[3].replace(":u", ""); t_end = t0 + ts; t_start = t_end - 0.05
    if any(a <= t_start and t_end <= b for a, b in win):
        sums[ev] = sums.get(ev, 0.0) + val
        if ev == "instructions": used += 1
print(json.dumps({"events": sums, "intervals_used": used, "window_seconds": total_span}))
