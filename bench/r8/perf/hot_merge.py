#!/usr/bin/env python3
"""hot_merge.py RAW FILL_CYCLES : fold the raw rows hot.sh collected (one per cell, level and event group) into
one row per cell and level, subtract the `floor` cell, and add `chain_lb`. hot.sh runs it; on a host with no
python3, copy `raw.tsv` back and run it here."""
import sys
raw, fill = sys.argv[1], float(sys.argv[2])
hdr, rows, order = None, {}, []
for line in open(raw):
    f = line.rstrip('\n').split('\t')
    if len(f) < 4 or f[2].startswith('#'):
        continue
    level, _, body = f[0], f[1], f[2:]
    if body[0] == 'scene':
        hdr = body
        continue
    if hdr is None or len(body) != len(hdr):
        continue
    key = (body[1], level)
    if key not in rows:
        rows[key] = {}
        order.append(key)
    for h, v in zip(hdr, body):
        rows[key].setdefault(h, v)
# each output column: the first of these the host has (AMD names first, then Intel's)
cols = [['instructions_p50'], ['cycles_p50'], ['branch-misses_p50'], ['l1d-misses_p50'], ['cache-misses_p50', 'l2-load-misses_p50'],
        ['dc-fill-l2_p50'], ['dc-fill-ccx_p50', 'llc-hits'], ['dc-fill-dram_p50', 'llc-misses_p50'], ['dtlb-misses_p50'], ['instructions_max'], ['cycles_max'], ['wall_ns_p50'], ['wall_ns_max']]
names = ['instr', 'cycles', 'br_miss', 'l1_miss', 'l2_miss', 'fill_l2', 'fill_l3', 'fill_dram', 'dtlb_miss', 'instr_max', 'cycles_max', 'wall_ns', 'wall_ns_max']
print('\t'.join(['scene', 'cell', 'level', 'evict_kb', 'reps'] + names + ['chain_lb']))
def num(r, c):
    for k in ([c] if isinstance(c, str) else c):
        if k == 'llc-hits':
            a, b = num(r, 'llc-refs_p50'), num(r, 'llc-misses_p50')
            if a is not None and b is not None:
                return max(a - b, 0.0)
            continue
        try:
            return float(r.get(k, ''))
        except ValueError:
            pass
    return None
for (cell, level) in order:
    r = rows[(cell, level)]
    warm = rows.get((cell, 'warm'), {})
    floor = rows.get(('floor', level), {})
    out = [r.get('scene', ''), cell, level, r.get('evict_kb', ''), r.get('reps', '')]
    for c in cols:
        v, fl = num(r, c), num(floor, c)
        # the floor cell's count is what an empty repetition costs at this level (the first touch of the
        # harness's own lines after the walk); subtract it from every other cell
        if v is not None and fl is not None and cell != 'floor' and not c[0].endswith('_max'):
            v = max(v - fl, 0.0)
        out.append('' if v is None else f'{v:.0f}')
    cold = rows.get((cell, 'l3'), rows.get((cell, 'l2'), {}))
    fcold = rows.get(('floor', 'l3'), rows.get(('floor', 'l2'), {}))
    fwarm = rows.get(('floor', 'warm'), {})
    a, b, fa, fb = num(cold, 'cycles_p50'), num(warm, 'cycles_p50'), num(fcold, 'cycles_p50'), num(fwarm, 'cycles_p50')
    if None in (a, b, fa, fb) or cell == 'floor':
        out.append('')
    else:
        out.append(f'{max(((a - fa) - (b - fb)) / fill, 0.0):.1f}')
    print('\t'.join(out))
