#!/usr/bin/env python3
"""Top functions of a crate (or a substring of the demangled name) in an ld64 map: crate_fns.py <map> <crate> [n]"""
import sys, re, collections
sys.path.insert(0, __import__('os').path.dirname(__file__))
import attribute as A
objs, sects, syms = A.parse(sys.argv[1])
names = A.demangle([s[3] for s in syms])
want = sys.argv[2]; n = int(sys.argv[3]) if len(sys.argv) > 3 else 25
rows = []
for addr, size, f, name in syms:
    if not A.section_of(sects, addr).endswith('__text'): continue
    d = A.strip_hash(names.get(name, name))
    if A.crate_of_name(d) == want or (want.startswith('~') and want[1:] in d):
        rows.append((size, d))
rows.sort(reverse=True)
print(f'{want}: {sum(r[0] for r in rows):,} B in {len(rows)} functions')
for s, d in rows[:n]: print(f'{s:8,}  {d[:160]}')
