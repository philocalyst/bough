"""Compare core_bench's main tables across ISAs: `isa_table.py A.md B.txt` (A = aarch64 RESULTS §6, B = x86-64 output).

For every (op, shape) row in both, prints Bough and Arena instructions on each ISA, each one's x86/aarch64 ratio, and
`norm` = Bough ratio / Arena ratio. The Arena is plain scalar Rust on both ISAs, so it is the yardstick for what the
ISA change alone costs; a norm well above 1 marks a Bough kernel whose x86-64 path is weaker than its aarch64 one
(typically a NEON kernel with a scalar twin). Instructions are not time and the ratio is not a speed ratio.
"""
import re, sys

def tables(text):
    out, op = {}, None
    for line in text.splitlines():
        m = re.match(r'^#{3,5} (.+?)(:.*)?$', line)
        if m:
            op = m.group(1).strip()
            continue
        cells = [c.strip() for c in line.strip().strip('|').split('|')]
        if op and len(cells) == 9 and cells[1].isdigit() and cells[4].isdigit():
            out.setdefault((op, cells[0]), (int(cells[1]), int(cells[4])))
    return out

a = tables(open(sys.argv[1]).read())
b = tables(open(sys.argv[2]).read())
print('| op | shape | Bough a64 | Bough x86 | ratio | Arena a64 | Arena x86 | ratio | norm |')
print('|---|---|---:|---:|---:|---:|---:|---:|---:|')
for k in a:
    if k not in b:
        continue
    (ba, aa), (bb, ab) = a[k], b[k]
    if min(ba, aa, bb, ab) == 0:
        continue
    rb, ra = bb / ba, ab / aa
    n = rb / ra
    flag = ' **' if n >= 1.5 else ''
    print(f'| {k[0]} | {k[1]} | {ba} | {bb} | {rb:.2f} | {aa} | {ab} | {ra:.2f} | {n:.2f}{flag} |')
