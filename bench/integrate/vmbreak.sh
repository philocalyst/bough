#!/bin/zsh
# usage: vmbreak.sh <binary> <at-seconds> [env...]: dirty+swapped KB per vmmap category at t, summed into the headline buckets
BIN=$1; AT=$2; shift; shift
env "$@" $BIN > /dev/null 2>&1 &
P=$!
sleep $AT
vmmap --summary $P 2>/dev/null > /tmp/vmb.txt
wait $P
python3 - <<'PY'
import re
def kb(s):
    m = re.match(r"([\d.]+)([KMG]?)", s)
    v = float(m.group(1)); u = m.group(2)
    return v * {"": 1/1024, "K": 1, "M": 1024, "G": 1048576}[u]
fp = None; rows = {}
for l in open("/tmp/vmb.txt"):
    if l.startswith("Physical footprint:"):
        fp = l.split()[-1]
    m = re.match(r"^(\S.*?)\s{2,}(\S+)\s+(\S+)\s+(\S+)\s+(\S+)\s+(\S+)\s+(\S+)\s+(\S+)\s+(\d+)", l)
    if m and not l.startswith(("REGION", "===", "TOTAL")):
        rows[m.group(1)] = kb(m.group(4)) + kb(m.group(5))   # dirty + swapped
b = {"IOSurface": 0, "GPU driver (owned unmapped + IOAccelerator*)": 0, "malloc": 0, "image/data (__DATA*, __AUTH*, mapped file, stack, misc)": 0}
for k, v in rows.items():
    if k == "IOSurface": b["IOSurface"] += v
    elif k.startswith("IOAccelerator") or k == "owned unmapped memory": b["GPU driver (owned unmapped + IOAccelerator*)"] += v
    elif k.startswith("MALLOC"): b["malloc"] += v
    else: b["image/data (__DATA*, __AUTH*, mapped file, stack, misc)"] += v
print("footprint", fp, " | " + " | ".join(f"{k}: {v/1024:.1f} MB" for k, v in b.items()))
PY
