#!/usr/bin/env python3
"""Round-6 sweep: per cell two pixel-gate processes (ring 4 and 1) and two in-process block measurements
(R0 = through GPU input, R1 = with the Metal frame). Instructions are calling-thread counts; no lock needed.

  sweep.py --binary target/release/headless --out DIR --batch table|rest|control [--sizes 10,100,...]
"""
import argparse, json, math, os, re, subprocess, sys
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

LINEAR = {'update10', 'replace', 'clear', 'filter'}  # ideal exponent 1 against N; everything else 0


def run(binary, args, caps='default', timeout=600):
    env = dict(os.environ, GUIBENCH_CAPS=caps)
    try:
        p = subprocess.run([binary, *args], env=env, capture_output=True, text=True, timeout=timeout)
        return p.returncode, p.stdout, p.stderr
    except subprocess.TimeoutExpired as e:
        return 'timeout', '', (e.stderr or b'').decode(errors='replace') if isinstance(e.stderr, bytes) else (e.stderr or '')


def panic_of(err):
    m = re.search(r"panicked at ([^\n]*)\n([^\n]*)", err)
    if not m:
        return err.strip()[-300:]
    return (m[1].split('/')[-1].strip(':') + ' ' + m[2]).strip()[:400]


def cell(binary, c, repeats, blocks):
    base = ['--scenario', c['scenario'], '--rows', str(c['rows']), '--op', c['op'], *c.get('extra', [])]
    if c['scenario'] in ('table', 'ticker'):
        base += ['--variant', c['variant']]
    caps = c.get('caps', 'default')
    out = dict(c)
    code, so, se = run(binary, base + ['--repeats', '0'], caps)
    m = re.search(r'mount instructions=(\d+) ns=\d+ rss=(\d+) footprint=(\d+) records=(\d+)', se)
    if m:
        out.update(mount_instr=int(m[1]), rss=int(m[2]), footprint=int(m[3]), records=int(m[4]))
    if code != 0:
        out['mount_failure'] = panic_of(se)
        return out
    if c['op'] == 'mount':
        return out
    gates = {}
    for ring in c.get('rings', (4, 1)):
        code, so, se = run(binary, base + ['--gate', '--ring', str(ring), '--repeats', '4'], caps)
        gates[str(ring)] = 'pass' if code == 0 else panic_of(se)
    out['gates'] = gates
    for phase, flag in (('R0', []), ('R1', ['--render'])):
        code, so, se = run(binary, base + ['--repeats', str(repeats), '--blocks', str(blocks)] + flag, caps)
        m = re.search(r'perop .*? med=(\d+) lo=(\d+) hi=(\d+) records=(\d+)', so)
        if code == 0 and m:
            out[phase] = dict(med=int(m[1]), lo=int(m[2]), hi=int(m[3]))
            out['records_after'] = int(m[4])
        else:
            out[phase] = dict(error=panic_of(se), code=code)
    return out


def fit(points):
    pts = [(n, y) for n, y in points if n > 0 and y and y > 0]
    if len(pts) < 2:
        return None
    xs = [math.log10(n) for n, _ in pts]
    ys = [math.log10(y) for _, y in pts]
    mx, my = sum(xs) / len(xs), sum(ys) / len(ys)
    den = sum((x - mx) ** 2 for x in xs)
    slope = sum((x - mx) * (y - my) for x, y in zip(xs, ys)) / den if den else 0.0
    last = (ys[-1] - ys[-2]) / (xs[-1] - xs[-2]) if xs[-1] != xs[-2] else 0.0
    return dict(global_exp=round(slope, 3), last_exp=round(last, 3))


def key_of(c):
    return (c['scenario'], c.get('variant', ''), c['op'], ' '.join(c.get('extra', [])), c.get('caps', 'default'))


def table_cells(sizes):
    ops = ['select', 'update10', 'swap', 'remove', 'insert', 'replace', 'append', 'truncate', 'clear', 'replace-select']
    cells = []
    for n in sizes:
        for variant, extras in (('fine', [[], ['--plain'], ['--mounted'], ['--mounted', '--plain']]), ('coarse', [[], ['--plain']])):
            for extra in extras:
                for op in ops:
                    cells.append(dict(scenario='table', variant=variant, rows=n, op=op, extra=extra))
    return cells


def rest_cells(sizes):
    cells = []
    for n in sizes:
        for op in ['hover', 'scroll']:
            if n == 10 and op == 'scroll':
                continue
            cells.append(dict(scenario='rows', rows=n, op=op))
        for k in (1, 100):
            for variant, extras in (('fine', [[], ['--mounted']]), ('coarse', [[], ['--plain']])):
                for extra in extras:
                    cells.append(dict(scenario='ticker', variant=variant, rows=n, op='ticker', extra=['--updates', str(k)] + extra))
        cells.append(dict(scenario='typing', rows=n, op='typing'))
        cells.append(dict(scenario='search', rows=n, op='filter'))
        cells.append(dict(scenario='search', rows=n, op='filter-nochange'))
        cells.append(dict(scenario='search', rows=n, op='filter', extra=['--plain']))
        cells.append(dict(scenario='search', rows=n, op='filter-nochange', extra=['--plain']))
        cells.append(dict(scenario='resize', rows=n, op='resize'))
    for n in [0] + sizes:
        cells.append(dict(scenario='spinner', rows=n, op='spinner'))
    for n in sizes + [100000, 1000000]:
        for op in ['mount', 'hover', 'scroll', 'jump']:
            cells.append(dict(scenario='virtual', rows=n, op=op))
    return cells


def control_cells(sizes):
    cells = []
    for n in sizes:
        for extra in ([], ['--mutant']):
            cells.append(dict(scenario='table', variant='fine', rows=n, op='select', extra=extra, rings=(4,)))
    return cells


def main():
    p = argparse.ArgumentParser()
    p.add_argument('--binary', required=True)
    p.add_argument('--out', required=True)
    p.add_argument('--batch', required=True)
    p.add_argument('--sizes', default='10,100,1000,10000')
    p.add_argument('--repeats', type=int, default=8)
    p.add_argument('--blocks', type=int, default=3)
    p.add_argument('--jobs', type=int, default=4)
    p.add_argument('--caps', default='default')
    p.add_argument('--only', default='')
    a = p.parse_args()
    sizes = list(map(int, a.sizes.split(',')))
    cells = dict(table=table_cells, rest=rest_cells, control=control_cells)[a.batch](sizes)
    if a.only:
        cells = [c for c in cells if re.search(a.only, ' '.join(map(str, key_of(c))))]
    for c in cells:
        c['caps'] = a.caps
    binary = str(Path(a.binary).resolve())
    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    with ThreadPoolExecutor(a.jobs) as ex:
        results = list(ex.map(lambda c: cell(binary, c, a.repeats, a.blocks), cells))
    groups = {}
    for r in results:
        for ph in ('R0', 'R1'):
            if 'med' in (r.get(ph) or {}):
                groups.setdefault((key_of(r), ph), []).append((r['rows'], r[ph]['med']))
    fits = {}
    for (k, ph), pts in groups.items():
        f = fit(sorted(pts))
        if f:
            ideal = 1 if k[2] in LINEAR else 0
            f['ideal'] = ideal
            f['flag'] = f['global_exp'] > ideal + 0.2 or f['last_exp'] > ideal + 0.2
            fits['|'.join(k) + '|' + ph] = f
    name = a.batch + ('-' + re.sub(r'\W+', '_', a.only) if a.only else '')
    (out / f'{name}.json').write_text(json.dumps(dict(cells=results, fits=fits), indent=1) + '\n')
    for r in results:
        g = r.get('gates', {})
        print(r['scenario'], r.get('variant', ''), r['rows'], r['op'], ' '.join(r.get('extra', [])),
              'R0', (r.get('R0') or {}).get('med', r.get('R0')), 'R1', (r.get('R1') or {}).get('med', r.get('R1')),
              'rec', r.get('records'), 'gates', {k: (v if v == 'pass' else v[:150]) for k, v in g.items()},
              r.get('mount_failure', ''), flush=True)
    for k, f in fits.items():
        print('fit', k, f)


if __name__ == '__main__':
    main()
