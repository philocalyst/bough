# Per-phase medians over the 'profile 120 frames' lines of each heavy_headless mode.
import re, statistics, glob, os
D = '/Users/mileswirht/projects/bough/bench/baselines/macos/raw/profile'
phases = ['update', 'layout', 'damage', 'paint', 'encode', 'submit']
pat = re.compile(r'(\w+) ([\d.]+)k instr ([\d.]+) us')
lines = ['mode,lines,' + ','.join(f'{p}_kinstr_med,{p}_us_med' for p in phases)]
for mode in ['idle', 'stream', 'scroll', 'both']:
    text = open(os.path.join(D, f'heavy_headless_{mode}.stderr')).read().splitlines()
    prof = [l for l in text if l.startswith('profile ')]
    cols = {p: ([], []) for p in phases}
    for l in prof:
        for name, k, us in pat.findall(l):
            cols[name][0].append(float(k)); cols[name][1].append(float(us))
    cells = []
    for p in phases:
        cells += [f'{statistics.median(cols[p][0]):.1f}', f'{statistics.median(cols[p][1]):.1f}']
    lines.append(f'{mode},{len(prof)},' + ','.join(cells))
open(os.path.join(D, 'profile_medians.csv'), 'w').write('\n'.join(lines) + '\n')
print('\n'.join(lines))
