#!/usr/bin/env python3
"""Render every R6 cell, preserving raw samples in sibling CSVs. Run from this directory."""
import csv
import statistics
from pathlib import Path

root = Path(__file__).resolve().parent
cells = {}
for name in ('core-r6.csv', 'spatial-r6.csv'):
    with (root / name).open() as f:
        for row in csv.DictReader(f):
            key = (row['shape'], row['operation'], int(row['n']))
            cells.setdefault(key, []).append(row)
assert len(cells) == 204, f'missing cells: {len(cells)}/204'
assert all(len(v) == 5 for v in cells.values())

def classification(op, shape):
    if op == 'build-append8': return 'linear N; current repeated splits quadratic', True, True
    if op == 'bulk-build': return 'linear N', False, True
    if op == 'remove8-end': return 'log N; fitting tail trim', False, False
    if op.startswith(('insert', 'remove', 'move')) or op == 'append1':
        return 'log N + K (move: boundary + relink)', True, False
    if op == 'translate-half': return 'log N + boundary for lazy range; current pages', True, False
    if op == 'translate64': return 'log N + touched pages', False, False
    if op.startswith('freeze'):
        return ('linear N' if op in ('freeze-cold', 'freeze-all') else 'changed pages + topology paths'), op in ('freeze-idle', 'freeze-one'), op in ('freeze-cold', 'freeze-all')
    if op in ('subtree-root', 'ancestors-last'): return 'log N + emitted ancestors', True, False
    if op == 'parent-last': return 'log N', shape == 'ordered-wide', False
    if op == 'subtree-leaf': return 'log N', False, False
    if op.startswith('cull'): return 'visited summaries + output; worst linear', False, True
    return 'visited summaries; worst linear', False, False

lines = ['# R6 instruction matrix', '',
         'Median retired instructions per operation; five samples. **A** marks source-proven excess',
         'over the structural ideal, at every N where that cost exists (not a fitted slope claim).',
         '† marks a >2x adjacent-decade ratio for log/constant candidates, or >15x for linear candidates.',
         'Spatial † is a locality warning, not proof an overlapping query has a logarithmic bound.',
         'Raw CSVs retain sample dispersion, batch sizes, page counts and emitted/copy counts.', '',
         '| shape / operation | 1,000 | 10,000 | 100,000 | 1,000,000 | ideal / qualifier |',
         '|---|---:|---:|---:|---:|---|']
for shape, op in sorted({(s,o) for s,o,_ in cells}):
    ideal, excess, linear = classification(op,shape)
    vals=[]
    prev=None
    for n in (1000,10000,100000,1000000):
        v=cells[shape,op,n]
        med=statistics.median(float(x['instructions_per_op']) for x in v)
        flag='†' if prev and med/prev > (15 if linear else 2) else ''
        cell_excess = excess and not (op in ('append1', 'insert8-end') and n < 1000000)
        vals.append(f'{med:,.1f}{flag}' + (' **A**' if cell_excess else ''))
        prev=med
    lines.append('| '+shape+' / '+op+' | '+' | '.join(vals)+' | '+ideal+' |')
lines += ['', 'A marks a structural path, not every invocation: a fitting tail append can avoid the',
          'global rebuild, while the next split cannot. Exact N and occupancy are preserved in CSV.', '',
          'Cull fractions grow emitted work with N, so a 10x slope alone is not excess. The comparison',
          'between ordered and interleaved cells at the same N/output exposes locality loss.', '',
          '## Sample ranges', '', '| shape / operation | N | min | median | max | output |',
          '|---|---:|---:|---:|---:|---:|']
for (shape,op,n),rows in sorted(cells.items()):
    v=[float(x['instructions_per_op']) for x in rows]
    lines.append(f'| {shape} / {op} | {n:,} | {min(v):,.1f} | {statistics.median(v):,.1f} | {max(v):,.1f} | {rows[0]["output"]} |')
(root/'TABLE-R6.md').write_text('\n'.join(lines)+'\n')
