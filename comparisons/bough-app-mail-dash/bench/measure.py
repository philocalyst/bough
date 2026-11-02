#!/usr/bin/env python3
"""Measure one bough apps_bench configuration the way zgx's scripts/perf/suite.py `measure` does.

    measure.py LABEL BINARY [KEY=VAL ...]

Runs BINARY REPEAT times (default 3) under `/usr/bin/time -l` with the given environment, reports the least
retired-instruction count (whole process), its spread and the median wall time, and the per-interaction lines
(`<app>: <interaction>: X ms, Y M instructions`) as the median of the runs. Appends one JSON line to
results.jsonl next to this file and writes every run's stdout and stderr to raw/LABEL.<k>.txt.
"""
import json
import os
import pathlib
import re
import statistics
import subprocess
import sys
import time
from datetime import datetime, timezone

HERE = pathlib.Path(__file__).resolve().parent
RAW = HERE / 'raw'
RESULTS = HERE / 'results.jsonl'
REPEAT = int(os.environ.get('MEASURE_REPEAT', '3'))
LINE = re.compile(r'^(.*): ([0-9.]+) ms, ([0-9.]+) M instructions', re.M)


def one_run(binary, env):
    started = time.monotonic()
    r = subprocess.run(['/usr/bin/time', '-l', str(binary)], env=env, capture_output=True, text=True)
    found = re.search(r'(\d+)\s+instructions retired', r.stderr)
    return {'rc': r.returncode, 'instructions': int(found.group(1)) if found else None,
            'seconds': time.monotonic() - started, 'stdout': r.stdout, 'stderr': r.stderr}


def main():
    if len(sys.argv) < 3:
        sys.exit(__doc__)
    label, binary, pairs = sys.argv[1], sys.argv[2], sys.argv[3:]
    env = dict(os.environ)
    for p in pairs:
        k, _, v = p.partition('=')
        env[k] = v
    runs = [one_run(binary, env) for _ in range(REPEAT)]
    RAW.mkdir(parents=True, exist_ok=True)
    for k, r in enumerate(runs):
        (RAW / f'{label}.{k + 1}.txt').write_text(r['stdout'])
        (RAW / f'{label}.{k + 1}.stderr.txt').write_text(r['stderr'])
    good = [r for r in runs if r['rc'] == 0 and r['instructions'] is not None]
    rec = {'label': label, 'binary': str(binary), 'env': dict(p.split('=', 1) for p in pairs),
           'when': datetime.now(timezone.utc).isoformat(timespec='seconds'), 'repeat': REPEAT,
           'instructions_runs': [r['instructions'] for r in runs], 'returncodes': [r['rc'] for r in runs],
           'wall_runs_s': [round(r['seconds'], 3) for r in runs]}
    if len(good) == len(runs):
        counts = [r['instructions'] for r in good]
        lo = min(counts)
        per = {}
        for r in good:
            for name, ms, m in LINE.findall(r['stdout']):
                per.setdefault(name, []).append((float(ms), float(m)))
        rec.update({'ok': True, 'least_instructions': lo, 'least_instructions_M': round(lo / 1e6, 1),
                    'spread_pct': round((max(counts) - lo) / lo * 100, 2),
                    'median_wall_s': round(statistics.median(r['seconds'] for r in good), 2),
                    'interactions': {n: {'ms_median': round(statistics.median(x[0] for x in v), 3),
                                         'M_median': round(statistics.median(x[1] for x in v), 3),
                                         'M_min': min(x[1] for x in v)} for n, v in per.items()}})
    else:
        rec.update({'ok': False, 'error': runs[-1]['stderr'][-600:]})
    with RESULTS.open('a') as f:
        f.write(json.dumps(rec) + '\n')
    if rec['ok']:
        print(f"{label}: {rec['least_instructions_M']} M  spread {rec['spread_pct']}%  median {rec['median_wall_s']} s")
        for n, v in rec['interactions'].items():
            print(f"  {n}: {v['ms_median']} ms, {v['M_median']} M (median of {REPEAT}; min {v['M_min']})")
    else:
        print(f"{label}: FAILED {rec['error'][:300]}")


if __name__ == '__main__':
    main()
