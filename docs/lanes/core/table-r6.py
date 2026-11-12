#!/usr/bin/env python3
"""Round 6 cost tables from the raw CSVs in data/ (run from anywhere).

  table-r6.py table <tag>            -> the instruction matrix of <tag> (fresh + warm + spatial), exponents, spread
  table-r6.py compare <base> <new>   -> every cell new/base at each N; flags > 1.02x

A cell's value is the median over runs of the per-run MINIMUM of its five samples: the code is
deterministic and everything the counter adds is additive (first-touch page faults, interrupts taken
while 15,000 cold page headers are walked: `ri_instructions` includes kernel work). Spread is
(max - min) / median of the per-run minima; `noise` is the median of all samples over the value, minus 1.
Exponent = log10 of the ratio between adjacent decades.
"""
import csv, glob, math, statistics, sys
from pathlib import Path

root = Path(__file__).resolve().parent / 'data'
NS = (1000, 10000, 100000, 1000000)

def load(tag, kind):
    """{(shape, op): {n: [run medians]}} and outputs."""
    cells, out, allv = {}, {}, {}
    for f in sorted(glob.glob(str(root / f'{tag}-{kind}-run*.csv'))):
        per = {}
        for r in csv.DictReader(open(f)):
            k = (r['shape'], r['operation'], int(r['n']))
            per.setdefault(k, []).append(float(r['instructions_per_op']))
            out[k] = int(r['output'])
        for (s, o, n), v in per.items():
            cells.setdefault((s, o), {}).setdefault(n, []).append(min(v))
            allv.setdefault((s, o, n), []).extend(v)
    NOISE.update({k: statistics.median(v) for k, v in allv.items()})
    return cells, out

NOISE = {}

def ideal(op):
    if op in ('bulk-build', 'build-append8', 'freeze-cold', 'freeze-all'): return 1.0, 'N'
    if op == 'translate-half': return 1.0, 'pages moved (N/128); 0 with lazy range tags'
    if op.startswith('cull'): return 1.0, 'emitted records (output grows with N)'
    if op.startswith('freeze'): return 0.0, 'changed pages'
    if op == 'insert8-front+posall': return 1.0, 'one lookup per page'
    if op == 'set-inks-handles64': return 0.0, '64 writes'
    if op == 'state-run-half': return 1.0, 'one word per page (N/128)'
    if op.startswith('state-any'): return 0.0, 'log N with directory nodes; one test per page today'
    if op.startswith(('insert', 'remove', 'move', 'append', 'translate64')): return 0.0, 'K + log N'
    return 0.0, 'log N'

def med(v): return statistics.median(v)

def table(tag):
    L = [f'# R6 instruction matrix: `{tag}`', '',
         'Retired instructions per operation (`proc_pid_rusage`): median over three runs of the minimum of five samples',
         '(deterministic code; the counter only ever adds: kernel work for page faults and interrupts is included).',
         '`e` columns: growth exponent per decade (log10 of the ratio of adjacent N); **bold** = more than 0.25',
         'above the ideal exponent. `spread` = worst (max - min) / median of the three per-run minima over the four N;',
         '`noise` = worst (median of all 15 samples / value - 1) over the four N.',
         '`fresh` = edit applied to a just-bulk-built tree (includes the one-time reallocation of the directory',
         'columns, which a bulk build leaves at exact capacity); `warm` = the same after a split and its inverse',
         'at the tail took that reallocation (`R6_WARM=1`). Queries and culls do not edit: one kind.', '']
    for kind, title in (('fresh-core', 'Structure, fresh tree'), ('warm-core', 'Structure, warm columns'), ('spatial', 'Spatial')):
        cells, out = load(tag, kind)
        if not cells: continue
        L += [f'## {title}', '', '| shape / operation | 1e3 | 1e4 | e | 1e5 | e | 1e6 | e | ideal e | spread | noise | ideal bound |', '|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|']
        for (shape, op), byn in cells.items():
            ie, why = ideal(op)
            row, prev, spread, noise = [], None, 0.0, 0.0
            for n in NS:
                v = byn[n]; m = med(v)
                spread = max(spread, (max(v) - min(v)) / m if m else 0)
                noise = max(noise, NOISE[(shape, op, n)] / m - 1 if m else 0)
                if prev is not None:
                    e = math.log10(m / prev) if prev > 0 and m > 0 else float('nan')
                    row.append(f'**{e:.2f}**' if e > ie + 0.25 else f'{e:.2f}')
                row.insert(len(row) - (1 if prev is not None else 0), f'{m:,.0f}')
                prev = m
            extra = ''
            if op.startswith('cull'):
                extra = ' out=' + '/'.join(str(out[(shape, op, n)]) for n in NS)
            L.append(f'| {shape} / {op} | ' + ' | '.join(row) + f' | {ie:.0f} | {spread*100:.1f}% | {noise*100:.1f}% | {why}{extra} |')
        L.append('')
    return '\n'.join(L)

def compare(base, new):
    L = [f'# `{new}` against `{base}`', '', "new / base per cell (values as in the matrix). **bold**: above 1.02x; `~`: above 1.02x but inside the cell's own scatter. Cells under 200 instructions",
         'are counter-dominated: `+k` is the absolute difference in instructions.', '']
    worst, noisy = [], []
    spread_of = lambda v: (max(v) - min(v)) / med(v) if med(v) else 0
    for kind, title in (('fresh-core', 'Structure, fresh tree'), ('warm-core', 'Structure, warm columns'), ('spatial', 'Spatial')):
        b, _ = load(base, kind); noise_b = {k: NOISE[k] / med(b[k[:2]][k[2]]) - 1 for k in NOISE if k[:2] in b}
        NOISE.clear()
        n_, _ = load(new, kind); noise_n = {k: NOISE[k] / med(n_[k[:2]][k[2]]) - 1 for k in NOISE if k[:2] in n_}
        NOISE.clear()
        if not b or not n_: continue
        L += [f'## {title}', '', '| shape / operation | 1e3 base | 1e3 new | 1e3 | 1e4 | 1e5 | 1e6 base | 1e6 new | 1e6 |', '|---|---:|---:|---:|---:|---:|---:|---:|---:|']
        for key in b:
            if key not in n_: continue
            r = {}
            for n in NS:
                x, y = med(b[key][n]), med(n_[key][n])
                q = y / x if x else float('nan')
                t = f'{q:.3f}'
                if x < 200 and abs(y - x) >= 0.5: t += f' ({y - x:+.0f})'
                if q > 1.02:
                    # a cell whose own samples scatter more than the excess is not evidence of a regression
                    sp = max(spread_of(b[key][n]), spread_of(n_[key][n]), noise_b.get((key[0], key[1], n), 0), noise_n.get((key[0], key[1], n), 0))
                    if sp > q - 1:
                        t = f'{t}~'; noisy.append((q, kind, key, n, x, y, sp))
                    else:
                        t = f'**{t}**'; worst.append((q, kind, key, n, x, y))
                r[n] = (x, y, t)
            L.append(f'| {key[0]} / {key[1]} | {r[1000][0]:,.0f} | {r[1000][1]:,.0f} | {r[1000][2]} | {r[10000][2]} | {r[100000][2]} | {r[1000000][0]:,.0f} | {r[1000000][1]:,.0f} | {r[1000000][2]} |')
        L.append('')
    L += ['## Cells above 1.02x', '']
    if not worst: L.append('None.')
    for q, kind, key, n, x, y in sorted(worst, reverse=True):
        L.append(f'* {kind} {key[0]} / {key[1]} N={n:,}: {x:,.0f} -> {y:,.0f} ({q:.3f}x)')
    L += ['', "## Above 1.02x but inside the cell's own scatter (`~`)", '',
          "The larger of the two tags' run-to-run spread and sample noise exceeds the excess: not a measured regression.", '']
    if not noisy: L.append('None.')
    for q, kind, key, n, x, y, sp in sorted(noisy, reverse=True):
        L.append(f'* {kind} {key[0]} / {key[1]} N={n:,}: {x:,.0f} -> {y:,.0f} ({q:.3f}x; scatter {sp*100:.0f}%)')
    return '\n'.join(L) + '\n'

if __name__ == '__main__':
    print(table(sys.argv[2]) if sys.argv[1] == 'table' else compare(sys.argv[2], sys.argv[3]))
