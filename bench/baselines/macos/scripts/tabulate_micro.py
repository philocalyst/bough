# Parses raw/micro/*.out (one JSON line per figure) into micro.csv, and prints
# per-configuration medians across the 3 repeats of median/p10/p90 (ns per op).
import csv, glob, json, os, re, statistics, sys
RAW = '/Users/mileswirht/projects/bough/bench/baselines/macos/raw/micro'
rows = []
for path in sorted(glob.glob(os.path.join(RAW, '*.out'))):
    name = os.path.basename(path)[:-4]
    m = re.match(r'(zgx|gpuifast)_(.+?)(?:_render([01]))?_rep(\d)$', name)
    if not m:
        continue
    fw, scene, render, rep = m.group(1), m.group(2), m.group(3), int(m.group(4))
    for line in open(path):
        line = line.strip()
        if not line.startswith('{'):
            continue
        d = json.loads(line)
        rows.append(dict(file=name, framework=d['framework'], scene=d['scene'], render_flag=d['render'],
                         configured_render=render or 'n/a', rep=rep, figure=d['figure'], ops=d['ops'],
                         batch=d['batch'], median_ns=d['median_ns'], p10_ns=d['p10_ns'], p90_ns=d['p90_ns'],
                         min_ns=d['min_ns'], max_ns=d['max_ns'], batches=d['batches'], clock=d['clock']))
out = os.path.join(os.path.dirname(RAW), 'micro.csv')
with open(out, 'w', newline='') as f:
    w = csv.DictWriter(f, fieldnames=list(rows[0].keys()))
    w.writeheader(); w.writerows(rows)
print(f'wrote {out} ({len(rows)} rows)')
groups = {}
for r in rows:
    key = (r['framework'], r['scene'], r['configured_render'], r['figure'])
    groups.setdefault(key, []).append(r)
print(f"{'framework':10} {'scene':22} {'RENDER':6} {'figure':6} {'median (ns)':>22} {'p10 (ns)':>20} {'p90 (ns)':>20}  reps")
for key in sorted(groups, key=lambda k: (k[1], k[0], k[2], k[3])):
    g = groups[key]
    med = [r['median_ns'] for r in g]; p10 = [r['p10_ns'] for r in g]; p90 = [r['p90_ns'] for r in g]
    fmt = lambda v: f"{statistics.median(v):,.0f} [{min(v):,.0f}-{max(v):,.0f}]"
    print(f"{key[0]:10} {key[1]:22} {key[2]:6} {key[3]:6} {fmt(med):>22} {fmt(p10):>20} {fmt(p90):>20}  {len(g)}")
