#!/usr/bin/env python3
"""The adversarial matrix: one process per cell (a panic is that cell's result). Predictions are stated here,
before the run. `caps` other than `default` is a labelled follow-up, run only after the default cell.

  adv.py --adversarial target/release/adversarial --headless target/release/headless --out DIR [--only RE]
"""
import argparse, json, os, re, subprocess
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

RAISED = 'glyphs=2097152,spaces=32768,instances=262144,runs=262144'
C = []


def adv(group, prediction, case, op, n=0, caps='default', env=None, **kw):
    args = ['--case', case, '--op', op, '--n', str(n)]
    for k, v in kw.items():
        args += ['--' + k] if v is True else ['--' + k, str(v)]
    C.append(dict(group=group, prediction=prediction, bin='adversarial', args=args, caps=caps, env=env or {}))


def head(group, prediction, scenario, op, n, caps='default', extra=(), env=None):
    C.append(dict(group=group, prediction=prediction, bin='headless', caps=caps, env=env or {},
                  args=['--scenario', scenario, '--rows', str(n), '--op', op, *extra]))


# 1. windows beyond the damage plane (128 x 128 tiles of 32 px)
P = 'the 128-tile damage row mask cannot mark x >= 4096: far-edge repair missing, or a refusal'
for w, h in [(1280, 800), (2560, 800), (4096, 800), (4128, 800), (6016, 3384)]:
    for op in ['hover-far', 'flip-far']:
        adv('wide', P if w > 4096 else 'control: works; hover cost independent of width', 'wide', op, w=w, h=h)
for h in [1280, 2560, 4096, 4128]:
    for op in ['hover-far', 'flip-far']:
        adv('high', 'the 128 tile rows cannot hold y >= 4096' if h > 4096 else 'control: works; hover cost independent of height', 'high', op, w=1280, h=h)

# 2. content taller than i16 / u16 / f32-exact pixels
P = 'placement saturates or loses precision past 32,767 / 65,535 / 2^24 px'
for px in [20000, 40000, 70000, 16777216 + 1000, 40000000]:
    for op in ['end', 'end-1']:
        adv('tall', P if px > 32767 else 'control', 'tall-block', op, n=px)
for n in [1000, 1020, 1030, 1100, 2100]:
    adv('tall', P if n > 1023 else 'control: 32 px rows end below 32,767 px', 'tall-rows', 'end', n=n)
for n in [500, 520, 600]:
    adv('tall', 'at scale 2 the device-pixel limit is reached at half the points: 16,383 pt', 'tall-rows', 'end', n=n, scale=2)
for n in [100000, 600000, 1000000, 4000000]:
    for op in ['end', 'end-1']:
        adv('tall', P + ' (virtual list, 32 px rows)', 'tall-virtual', op, n=n)

# 3. 100 K mounted records without virtualisation (20,000 rows of 5 records in a scroll box)
for caps in ['default', RAISED]:
    for op in ['mount', 'hover', 'scroll']:
        head('100k', 'default caps overflow at mount (instances 65,536 / glyphs 524,288)', 'rows', op, 20000, caps, env={'GUIBENCH_REHOVER': 'same'})

# 4. unequal-height wrapping rows: one row grows by lines
P = 'a height change re-places every row below it: O(mounted rows), not O(visible)'
for n in [10, 100, 1000, 10000]:
    adv('wrap', P, 'wrap', 'grow', n=n)
for n in [10, 100, 1000, 4000]:
    adv('wrap', P, 'wrap', 'grow', n=n, flag=True)
for n in [10, 100, 1000]:
    adv('wrap', 'the whole Vec is re-described and every row re-laid: O(N)', 'wrap-coarse', 'grow', n=n)

# 5. nested scroll boxes
for op in ['inner', 'outer', 'both', 'chain']:
    adv('nested', 'composed spaces / copy history fail exact repair when both scroll in one frame', 'nested', op)

# 6. a tree 200 levels deep
for d in [10, 50, 100, 200]:
    for op in ['hover', 'flip']:
        adv('deep', 'a depth field overflows or recursion fails; hover cost grows with depth', 'deep', op, n=d)
    adv('deep', 'nested list rows exhaust spaces or clips, or layout recursion fails', 'deep-list', 'toggle', n=d)

# 7. 10 K independent signals in one turn (10 K mounted list rows; default caps first)
P = 'linear in writes with a small constant; 10 K mounted rows exceed Caps::spaces'
for n, k, caps in [(4000, 4000, 'default'), (10000, 10000, 'default')] + [(10000, k, RAISED) for k in (1, 10, 100, 1000, 10000)]:
    adv('signals', P, 'signals', 'write', n=n, k=k, caps=caps, flag=True)
for k in (1, 100, 10000):
    adv('signals', 'unmounted rows have no wires: a write costs the signal store only', 'signals', 'write', n=10000, k=k)

# 8. one signal with 10 K dependents; 9. a re-described owner with 10 K children and one plain field changed
for n in [10, 100, 1000, 10000]:
    adv('fan', 'O(N) is required', 'fan', 'sig-text', n=n)
    adv('fan', 'O(N) paint writes are required', 'fan', 'sig-paint', n=n)
    adv('fan', 'one text changes; the re-description still walks all N children: O(N)', 'fan', 'plain', n=n)
    adv('fan', 'nothing changes; the re-description still walks all N children: O(N)', 'fan', 'noop', n=n)
    adv('fan', 'wire path alone (mount_once harness option): O(N) writes', 'fan', 'sig-paint', n=n, once=True)
    adv('fan', 'wire path alone (mount_once harness option): O(N) text writes', 'fan', 'sig-text', n=n, once=True)

# 10. non-Latin text
for font in ['', '/System/Library/Fonts/Supplemental/Arial Unicode.ttf']:
    adv('scripts', 'no fallback and no bidi: missing glyphs draw nothing or boxes; the self-render gates still pass', 'scripts', 'ink',
        env={'BOUGH_FONT': font} if font else {})

# 11. structure churn
P = 'each front insert shifts order and rebuilds a height suffix: per-insert cost grows with N (quadratic total)'
for n in [10, 1000, 10000, 100000]:
    adv('churn', P, 'churn', 'front', n=n, k=1000)
    adv('churn', 'a move permutes a span: per-move cost grows with N', 'churn', 'moves', n=max(n, 100), k=1000)
for n in [10, 1000]:
    adv('churn', P + '; mounted rows', 'churn', 'front', n=n, k=1000, flag=True)
    adv('churn', 'a move permutes a span: per-move cost grows with N; mounted rows', 'churn', 'moves', n=max(n, 100), k=1000, flag=True)
for n in [10, 100, 1000]:
    adv('churn', 'positional re-description rewrites every row per insert: O(N) texts per insert', 'churn-coarse', 'front', n=n, k=100)
    adv('churn', 'positional re-description rewrites the moved span', 'churn-coarse', 'moves', n=max(n, 100), k=100)

# 12. scale: no live entry point (FRICTION G11); static scale-2 cells
for op in ['hover', 'scroll']:
    head('scale', 'scale 2 mounts and repairs exactly (static; a live 1 -> 2 -> 1 switch has no host entry point)', 'rows', op, 100, extra=['--scale', '2'], env={'GUIBENCH_REHOVER': 'same'})
for op in ['hover', 'select', 'swap']:
    head('hover-redescribe', 'a hover crossing or click on a plain (non-scroll) coarse table re-describes the root: O(N)', 'table', op, 1000, extra=['--variant', 'coarse', '--plain'])
    head('hover-redescribe', 'control at 100 rows', 'table', op, 100, extra=['--variant', 'coarse', '--plain'])
head('scale', 'as above', 'table', 'select', 100, extra=['--variant', 'fine', '--scale', '2'])
head('scale', 'as above', 'table', 'update10', 100, extra=['--variant', 'fine', '--scale', '2'])


def run(c, bins, timeout):
    env = dict(os.environ, GUIBENCH_CAPS=c['caps'], **c['env'])
    out = dict(c)
    if c['bin'] == 'adversarial':
        runs = [('all', c['args'])]
    elif '--op' in c['args'] and c['args'][c['args'].index('--op') + 1] == 'mount':
        runs = [('mount', c['args'] + ['--repeats', '0'])]
    else:
        runs = [('gate', c['args'] + ['--gate', '--repeats', '4']), ('R0', c['args'] + ['--repeats', '8', '--blocks', '3']),
                ('R1', c['args'] + ['--repeats', '8', '--blocks', '3', '--render'])]
    res = {}
    for name, args in runs:
        try:
            p = subprocess.run([bins[c['bin']], *args], env=env, capture_output=True, text=True, timeout=timeout)
            lines = [l for l in p.stdout.splitlines() if l.startswith(('stage', 'perop', 'result'))]
            m = re.search(r'^mount .*$', p.stderr, re.M)
            if m:
                lines.append(m[0])
            pan = re.search(r"panicked at ([^\n]*)\n([^\n]*)", p.stderr)
            if p.returncode != 0:
                lines.append(f'EXIT {p.returncode} ' + ((pan[1].split('/')[-1] + ' ' + pan[2]) if pan else p.stderr.strip()[-300:]))
            res[name] = lines
        except subprocess.TimeoutExpired:
            res[name] = [f'TIMEOUT {timeout}s']
    out['result'] = res
    return out


def main():
    p = argparse.ArgumentParser()
    p.add_argument('--adversarial', required=True)
    p.add_argument('--headless', required=True)
    p.add_argument('--out', required=True)
    p.add_argument('--only', default='')
    p.add_argument('--jobs', type=int, default=4)
    p.add_argument('--timeout', type=int, default=600)
    a = p.parse_args()
    bins = dict(adversarial=str(Path(a.adversarial).resolve()), headless=str(Path(a.headless).resolve()))
    cells = [c for c in C if re.search(a.only, c['group'] + ' ' + ' '.join(c['args']))]
    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    with ThreadPoolExecutor(a.jobs) as ex:
        results = list(ex.map(lambda c: run(c, bins, a.timeout), cells))
    name = 'adversarial' + ('-' + re.sub(r'\W+', '_', a.only) if a.only else '')
    (out / f'{name}.json').write_text(json.dumps(results, indent=1) + '\n')
    for r in results:
        print('##', r['group'], r['bin'], ' '.join(r['args']), 'caps=' + r['caps'], ' '.join(f'{k}={v}' for k, v in r['env'].items()))
        print('   prediction:', r['prediction'])
        for k, lines in r['result'].items():
            for l in lines:
                print('   ', k + ':' if r['bin'] == 'headless' else '', l[:900])


if __name__ == '__main__':
    main()
