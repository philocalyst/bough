#!/usr/bin/env python3
"""lines_fold.py DIR CELL REPS : classify the addresses lines.sh sampled for CELL by the regions the binary
printed, remove what the `floor` cell also touches, and print distinct lines and samples per repetition per
kind of memory, then the code lines per function."""
import sys, re, collections
d, cell, reps = sys.argv[1], sys.argv[2], int(sys.argv[3])

def regions(tag):
    out = []
    for l in open(f'{d}/{tag}.regions', errors='replace'):
        f = l.rstrip('\n').split('\t')
        if f[0] == 'REGION' and int(f[3]) > 0:
            out.append((f[1], int(f[2], 16), int(f[3]), int(f[4]), int(f[5])))
    return out

def kind(name, off, page, rec_off):
    if name.startswith('pages'):
        arch = name.split('.')[-1]
        return f'pages.{arch}.' + ('rects' if off % page < rec_off else 'records')
    if name.startswith('tree.'):
        return 'tree (stores, sites)'
    if name.startswith('table.'):
        return 'table (column entries, readers)'
    return {'block.shown': 'block', 'model.block': 'block', 'model.struct': 'model', 'model.rows': 'model',
            'world': 'world (driver state)', 'tiles.bits': 'tiles (damage bitmap)', 'half.live': 'live words',
            'half.plan': 'plan (offset form)', 'half.spaces': 'spaces', 'arena.shared': 'arena head and dirty notes',
            'harness.evict': 'harness', 'stack': 'stack'}.get(name, name)

def data(tag):
    regs = regions(tag)
    lines, samples = collections.defaultdict(set), collections.Counter()
    for l in open(f'{d}/{tag}.txt', errors='replace'):
        f = l.split()
        if not f:
            continue
        try:
            a = int(f[0], 16)
        except ValueError:
            continue
        k = 'other (heap, statics)'
        for name, start, ln, page, ro in regs:
            if start <= a < start + ln:
                k = kind(name, a - start, page or 1, ro)
                break
        lines[k].add(a >> 6)
        samples[k] += 1
    return lines, samples

def code(tag):
    lines = collections.defaultdict(set)
    for l in open(f'{d}/{tag}.txt', errors='replace'):
        f = l.split(None, 1)
        if len(f) < 1:
            continue
        try:
            ip = int(f[0], 16)
        except ValueError:
            continue
        sym = re.sub(r'\+0x[0-9a-f]+$', '', f[1].strip()) if len(f) > 1 else '?'
        sym = re.sub(r'::h[0-9a-f]{16}$', '', sym)
        lines[sym].add(ip >> 6)
    return lines

print(f'# cell={cell} reps={reps}: distinct 64-byte lines over the measured repetitions, the floor cell\'s subtracted per kind; samples per repetition')
for tag, title in (('l1', 'data lines loaded (L1 cold)'), ('l3', 'data lines from memory (L3 cold)')):
    L, S = data(f'{cell}.{tag}')
    FL, FS = data(f'floor.{tag}')
    print(f'## {title}')
    print('kind\tlines\tsamples_per_rep')
    tot = 0
    rows = []
    for k in L:
        # the floor run is another process (other addresses): its lines are subtracted by count, per kind
        own = len(L[k]) - len(FL.get(k, ()))
        if k == 'harness' or own <= 0:
            continue
        rows.append((own, k, (S[k] - FS.get(k, 0)) / reps))
        tot += own
    for n, k, s in sorted(rows, reverse=True):
        print(f'{k}\t{n}\t{s:.1f}')
    print(f'total\t{tot}\t')
C, FC = code(f'{cell}.code'), code('floor.code')
print('## code lines fetched from L3 or memory (L3 cold)')
print('function\tlines')
rows = sorted(((len(v) - len(FC.get(k, ())), k) for k, v in C.items()), reverse=True)
tot = sum(n for n, _ in rows if n > 0)
for n, k in rows[:25]:
    if n:
        print(f'{k[:110]}\t{n}')
print(f'total\t{tot}')
