#!/usr/bin/env python3
"""Measure one zgx headless benchmark the way scripts/perf/suite.py `measure` does.

    measure.py LABEL BINARY [KEY=VAL ...] [-- ARG ...]

Runs BINARY REPEAT times (default 3) under `/usr/bin/time -l` with the given
environment and argv. Reports the least retired-instruction count (the
reported one), the spread (max-min)/min*100, and the median wall time.
Appends one JSON line to results.jsonl next to this file and writes the
stdout of the last run to raw/LABEL.txt (stderr to raw/LABEL.stderr.txt).
A failing run is recorded rather than aborting, so a batch continues.

Env: MEASURE_CWD sets the working directory (default: the zgx checkout,
read-only use). Each ZGX_ variable is also set under its ZGUI_ name, as
suite.py does for builds from before the rename.
"""
import json
import os
import pathlib
import platform
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
CWD = os.environ.get('MEASURE_CWD', os.path.expanduser(os.environ.get('BOUGH_REF', '~/projects/_ref')) + '/zgx')


def split_argv(argv):
    if '--' in argv:
        i = argv.index('--')
        return argv[:i], argv[i + 1:]
    return argv, []


def one_run(binary, env, args):
    command = ['/usr/bin/time', '-l', str(binary), *args]
    started = time.monotonic()
    result = subprocess.run(command, env=env, cwd=CWD, capture_output=True, text=True)
    seconds = time.monotonic() - started
    found = re.search(r'(\d+)\s+instructions retired', result.stderr)
    return {
        'returncode': result.returncode,
        'instructions': int(found.group(1)) if found else None,
        'seconds': seconds,
        'stdout': result.stdout,
        'stderr': result.stderr,
    }


def main():
    head, args = split_argv(sys.argv[1:])
    if len(head) < 2:
        sys.exit(__doc__)
    label, binary, pairs = head[0], head[1], head[2:]
    env = dict(os.environ)
    for pair in pairs:
        key, _, value = pair.partition('=')
        env[key] = value
        if key.startswith('ZGX_'):
            env['ZGUI_' + key[4:]] = value  # rename-to-zgx: keep (as suite.py)

    runs = [one_run(binary, env, args) for _ in range(REPEAT)]
    good = [r for r in runs if r['returncode'] == 0 and r['instructions'] is not None]
    record = {
        'label': label,
        'binary': str(binary),
        'env': dict(p.split('=', 1) for p in pairs),
        'argv': args,
        'cwd': CWD,
        'when': datetime.now(timezone.utc).isoformat(timespec='seconds'),
        'platform': platform.platform(),
        'repeat': REPEAT,
        'instructions_runs': [r['instructions'] for r in runs],
        'returncodes': [r['returncode'] for r in runs],
        'wall_runs_s': [round(r['seconds'], 3) for r in runs],
    }
    RAW.mkdir(parents=True, exist_ok=True)
    (RAW / f'{label}.txt').write_text(runs[-1]['stdout'])
    (RAW / f'{label}.stderr.txt').write_text(runs[-1]['stderr'])
    if len(good) == len(runs):
        counts = [r['instructions'] for r in good]
        lo = min(counts)
        record.update({
            'ok': True,
            'least_instructions': lo,
            'least_instructions_M': round(lo / 1e6, 1),
            'spread_pct': round((max(counts) - lo) / lo * 100, 2) if len(counts) > 1 else 0.0,
            'median_wall_s': round(statistics.median(r['seconds'] for r in good), 2),
        })
    else:
        tail = runs[-1]['stderr'][-600:]
        record.update({'ok': False, 'error': f'{len(good)}/{len(runs)} runs ok; last stderr tail: {tail}'})
    with RESULTS.open('a') as f:
        f.write(json.dumps(record) + '\n')
    if record['ok']:
        print(f"{label}: {record['least_instructions_M']} M  spread {record['spread_pct']}%  "
              f"median {record['median_wall_s']} s")
    else:
        print(f"{label}: FAILED  {record['error'][:300]}")


if __name__ == '__main__':
    main()
