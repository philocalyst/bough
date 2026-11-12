#!/usr/bin/env python3
"""Markdown tables for OVERFIT.md from the sweep JSON (default caps, raised-caps and idle-frame follow-ups).

  report.py RUNTIME_DIR > tables.md
"""
import json, math, sys
from pathlib import Path

LINEAR = {'update10', 'replace', 'clear', 'filter', 'replace-select'}
IDEAL = {'select': 'O(1)', 'update10': 'O(N/10)', 'swap': 'O(1)', 'remove': 'O(1)', 'insert': 'O(1)', 'replace': 'O(N)',
         'append': 'O(1,000)', 'truncate': 'O(1,000)', 'clear': 'O(N)', 'replace-select': 'O(N)', 'hover': 'O(1)', 'scroll': 'O(visible)',
         'jump': 'O(visible)', 'ticker': 'O(K)', 'spinner': 'O(1)', 'typing': 'O(1)', 'filter': 'O(N shown/hidden)',
         'filter-nochange': 'O(1) + query scan', 'resize': 'O(visible)', 'mount': 'O(visible)'}


def num(v):
    if v is None:
        return '-'
    if v >= 1e9:
        return f'{v / 1e9:.2f}G'
    if v >= 1e6:
        return f'{v / 1e6:.2f}M' if v < 1e7 else f'{v / 1e6:.1f}M'
    if v >= 1e3:
        return f'{v / 1e3:.1f}k' if v < 1e5 else f'{v / 1e3:.0f}k'
    return str(v)


def load(d, name):
    out = []
    for f in sorted(Path(d).glob(name)):
        out += json.loads(f.read_text())['cells']
    return out


def code(c):
    """Gate / failure code of a cell."""
    msgs = list((c.get('gates') or {}).values())
    for ph in ('R0', 'R1'):
        e = (c.get(ph) or {}).get('error')
        if e:
            msgs.append('measured: ' + e)
    if c.get('mount_failure'):
        msgs.append('mount: ' + c['mount_failure'])
    codes = []
    for m in msgs:
        if m == 'pass':
            continue
        pre = 'm' if m.startswith('mount: ') else ('r' if m.startswith('measured: ') else '')
        if 'glyph_x' in m:
            k = pre + 'G'
        elif 'instance column' in m:
            k = pre + 'I'
        elif 'space column' in m:
            k = pre + 'SP'
        elif 'retained-fresh' in m:
            k = 'H' if c['op'] in ('scroll', 'jump') else ('D' if '--mounted' in c.get('extra', []) else 'S')
        elif 'assertion' in m:
            k = 'R'
        else:
            k = '?'
        if k not in codes:
            codes.append(k)
    return codes


def fit(points):
    pts = [(n, y) for n, y in points if n > 0 and y and y > 0]
    if len(pts) < 2:
        return None, None
    xs = [math.log10(n) for n, _ in pts]
    ys = [math.log10(y) for _, y in pts]
    mx, my = sum(xs) / len(xs), sum(ys) / len(ys)
    den = sum((x - mx) ** 2 for x in xs)
    return (sum((x - mx) * (y - my) for x, y in zip(xs, ys)) / den if den else 0.0), (ys[-1] - ys[-2]) / (xs[-1] - xs[-2])


def key(c):
    return (c['scenario'], c.get('variant', ''), ' '.join(c.get('extra', [])), c['op'])


def main():
    d = Path(sys.argv[1])
    base = load(d / 'sweep', '*.json')
    raised = load(d / 'sweep-raised', '*.json') + load(d / 'sweep-raised5k', '*.json')
    idle = load(d / 'sweep-idle', '*.json') + load(d / 'sweep-idle-raised', '*.json') + load(d / 'sweep-idle-raised5k', '*.json')
    cells = {}
    for src, tag in ((base, ''), (raised, '†')):
        for c in src:
            k = key(c)
            cur = cells.setdefault(k, {})
            # a default-caps result that ran wins; a raised-caps one fills a default mount failure
            if c['rows'] not in cur or (cur[c['rows']][0].get('mount_failure') and not c.get('mount_failure')):
                prev = cur.get(c['rows'])
                cur[c['rows']] = (c, tag, code(prev[0]) if prev else [])
    idles = {(key(c), c['rows']): c for c in idle}
    groups = {}
    for k in cells:
        groups.setdefault(k[:3], []).append(k[3])
    order = ['rows', 'table', 'ticker', 'spinner', 'typing', 'search', 'virtual', 'resize']
    for g in sorted(groups, key=lambda g: (order.index(g[0]), g[1], g[2])):
        sizes = sorted({n for op in groups[g] for n in cells[g + (op,)]})
        title = ' '.join(x for x in g if x)
        print(f'\n**{title}**\n')
        print('| operation | ideal | ' + ' | '.join(f'N={n:,}' for n in sizes) + ' | exponent R0 (all / last step) | verdict |')
        print('|---|---|' + '---:|' * len(sizes) + '---|---|')
        for op in groups[g]:
            row, pts, codes_all = [], [], []
            for n in sizes:
                ent = cells[g + (op,)].get(n)
                if not ent:
                    row.append('')
                    continue
                c, tag, default_codes = ent
                cs = code(c)
                r0, r1 = (c.get('R0') or {}).get('med'), (c.get('R1') or {}).get('med')
                txt = f'{num(r0)} / {num(r1)}' if r0 is not None else ''
                if op == 'mount':
                    txt = num(c.get('mount_instr'))
                    r0 = c.get('mount_instr')
                i = idles.get((g + (op,), n))
                if r0 is None and i and (i.get('R0') or {}).get('med') is not None:
                    txt = f"{num(i['R0']['med'])} / {num((i.get('R1') or {}).get('med'))}ⁱ"
                    r0 = i['R0']['med']
                marks = [x for x in default_codes if x not in cs] + cs
                if marks:
                    txt = (txt + ' ' if txt else '') + '[' + ','.join(marks) + ']'
                row.append(txt + tag)
                degenerate = (op in ('scroll', 'jump') and n <= 10) or ('--updates 100' in g[2] and n < 100)
                if r0 is not None and not degenerate:
                    pts.append((n, r0))
                codes_all += [x for x in marks if x not in codes_all]
            ideal = 1 if op in LINEAR else 0
            ga, gl = fit(pts)
            exp = '-' if ga is None else f'{ga:.2f} / {gl:.2f}'
            if 'S' in codes_all:
                verdict = 'WRONG PIXELS (stale; cost not meaningful)'
            elif ga is not None and (ga > ideal + 0.2 or gl > ideal + 0.2):
                verdict = 'OVERFIT'
            elif ga is None:
                verdict = 'not measurable'
            else:
                verdict = 'ok'
            extra = [x for x in codes_all if x != 'S']
            if extra:
                verdict += '; ' + ','.join(extra)
            lab = IDEAL.get(op, '')
            print(f'| {op} | {lab} | ' + ' | '.join(row) + f' | {exp} | {verdict} |')


if __name__ == '__main__':
    main()
