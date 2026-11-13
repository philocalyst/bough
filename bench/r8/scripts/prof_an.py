# prof_an.py COLLAPSED OUT.json : from an inferno-collapse-perf file (first frame = process name; counts are weighted by the
# sampling period, so they are cycles): the top ten symbols by self time, the top ten attributed to the nearest Rust frame
# (a libc or kernel leaf such as memmove or malloc is charged to the code that called it), the share of each process, and
# frames that look like dispatch (closures, call_once, vtable and shim frames, function-pointer names).
import sys, re, json, collections
col, out = sys.argv[1], sys.argv[2]
DISP = re.compile(r"call_once|\{\{closure\}\}|\{closure#|vtable|::shim|trampoline|dyn |PatchFn|run_head", re.I)
stacks = []
comm = collections.Counter()
for line in open(col):
    line = line.rstrip("\n")
    if not line: continue
    stack, _, n = line.rpartition(" ")
    try: n = int(n)
    except ValueError: continue
    fr = stack.split(";")
    comm[fr[0]] += n
    stacks.append((fr[0], fr[1:], n))
main = comm.most_common(1)[0][0]
total_all = sum(comm.values())
self_ = collections.Counter(); attr = collections.Counter(); total = 0; stack_has = collections.Counter(); disp_self = 0
def is_rust(f): return "::" in f and not f.startswith("core::arch") or f.startswith("<")
for c, fr, n in stacks:
    if c != main or not fr: continue
    total += n
    leaf = fr[-1]; self_[leaf] += n
    if DISP.search(leaf): disp_self += n
    a = next((f for f in reversed(fr) if is_rust(f)), leaf)
    attr[a] += n
    for f in set(fr):
        if DISP.search(f): stack_has[f] += n
pct = lambda x: 100.0 * x / total if total else 0
res = {"process": main, "total_cycles_weighted_samples": total, "other_processes_pct": 100.0 * (total_all - total) / total_all if total_all else 0,
       "top10": [{"symbol": s, "samples": n, "pct": pct(n)} for s, n in self_.most_common(10)],
       "top10_attributed": [{"symbol": s, "samples": n, "pct": pct(n)} for s, n in attr.most_common(10)],
       "dispatch_like_self_pct": pct(disp_self),
       "dispatch_like_frames": [{"symbol": s, "inclusive_pct": pct(n)} for s, n in stack_has.most_common(8)]}
json.dump(res, open(out, "w"), indent=1)
print("process", main, "other processes %.2f%%" % res["other_processes_pct"])
for t in res["top10"]: print(f'{t["pct"]:5.1f}%  {t["symbol"][:110]}')
print("-- attributed to the nearest Rust frame")
for t in res["top10_attributed"]: print(f'{t["pct"]:5.1f}%  {t["symbol"][:110]}')
print("dispatch-like self %", round(res["dispatch_like_self_pct"], 1))
