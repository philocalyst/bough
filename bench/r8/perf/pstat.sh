#!/usr/bin/env bash
# pstat.sh [options] -- CMD [ARGS...] : Linux `perf stat` of CMD in event groups that each fit the PMU, with an
# identity record (MEASUREMENT.md, "perf stat on ilo-prod").
#
#   -g GROUPS   comma list of groups (default core,l1,l2,fill), each run separately, three events each:
#                 core  cycles instructions branch-misses
#                 l1    instructions L1-dcache-loads L1-dcache-load-misses
#                 l2    instructions cache-references cache-misses
#                 fill  instructions ls_refills_from_sys.ls_mabresp_lcl_cache ls_refills_from_sys.ls_mabresp_lcl_dram
#                 tlb   instructions dTLB-load-misses ls_refills_from_sys.ls_mabresp_lcl_l2
#               Zen 2: cache-references / cache-misses are all L2 requests / L2 misses (0xff60, 0x0964);
#               lcl_cache = an L1 refill served by L3 or another core's L2 in the CCX, lcl_dram = from DRAM. On
#               ilo-prod a perf stat group of four or more events is on the PMU only ~50 % of the time (cause not
#               found; three schedule 100 %), so every group here has three, and `run%` must read 100.
#   -e EVENTS   one extra group of your own (comma list; keep it to three)
#   -r N        repeats per group (perf's -r; mean and its relative stddev are printed), default 5
#   -s VAR=A,B  slope: run every group twice, with VAR=A and VAR=B in the environment, and print
#               (count(B) - count(A)) / (B - A) per event: the cost of one repetition with setup subtracted
#   -k          count kernel mode too (default: user mode only, `:u`)
#   -c CPU      pin to this CPU (default 2: CCX0, away from the `build` cores 4-7/12-15)
#   -o FILE     also write the table (with the identity record) to FILE
#
# Output: one row per event: group, event, count (mean over repeats), relative stddev, run% (time the group was on
# the PMU), and with -s the per-repetition slope. Needs perf on PATH (env.sh) and root or perf_event_paranoid <= 2
# (user mode). Never compare these with macOS instruction counts on one axis: other ISA, other compiler backend.
set -u
GROUPS_="core,l1,l2,fill"; EXTRA=""; R=5; SLOPE=""; MOD=":u"; CPU=2; OUT=""
while [ $# -gt 0 ]; do
  case $1 in
    -g) GROUPS_=$2; shift 2 ;;
    -e) EXTRA=$2; shift 2 ;;
    -r) R=$2; shift 2 ;;
    -s) SLOPE=$2; shift 2 ;;
    -k) MOD=""; shift ;;
    -c) CPU=$2; shift 2 ;;
    -o) OUT=$2; shift 2 ;;
    --) shift; break ;;
    -h|--help) sed -n '2,/^set -u/p' "$0" | sed '$d' | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "pstat: unknown option $1" >&2; exit 2 ;;
  esac
done
[ $# -gt 0 ] || { echo "pstat: no command (see --help)" >&2; exit 2; }
command -v perf >/dev/null || { echo "pstat: perf not on PATH (source env.sh)" >&2; exit 2; }

F="ls_refills_from_sys.ls_mabresp"
group() {
  case $1 in
    core) echo "cycles,instructions,branch-misses" ;;
    l1) echo "instructions,L1-dcache-loads,L1-dcache-load-misses" ;;
    l2) echo "instructions,cache-references,cache-misses" ;;
    fill) echo "instructions,$F""_lcl_cache,$F""_lcl_dram" ;;
    tlb) echo "instructions,dTLB-load-misses,$F""_lcl_l2" ;;
    extra) echo "$EXTRA" ;;
    *) echo "pstat: unknown group $1" >&2; exit 2 ;;
  esac
}
list=$(echo "$GROUPS_" | tr ',' ' ')
[ -n "$EXTRA" ] && list="$list extra"

T=$(mktemp -d)
run() { # group tag env-assignment...
  local g=$1 tag=$2; shift 2
  local ev; ev=$(group "$g" | tr ',' '\n' | sed "s/\$/$MOD/" | paste -sd, -)
  # {..}: one perf group, scheduled all or nothing, so every count in it covers the same instructions (ratios
  # are exact even when the PMU is shared and the group is multiplexed: run% says how long it was on).
  env "$@" taskset -c "$CPU" perf stat -x, -r "$R" -e "{$ev}" -o "$T/$g.$tag.csv" -- "${CMDV[@]}" > "$T/$g.$tag.out" 2>&1
}
CMDV=("$@")
for g in $list; do
  if [ -n "$SLOPE" ]; then
    var=${SLOPE%%=*}; vals=${SLOPE#*=}; a=${vals%,*}; b=${vals#*,}
    run "$g" a "$var=$a"; run "$g" b "$var=$b"
  else
    run "$g" a PSTAT=1
  fi
done

{
  "$(dirname "$0")/identity.sh" "${CMDV[0]}"
  echo "# perf=$(perf --version | cut -d' ' -f3) cpu_pin=$CPU repeats=$R mode=${MOD:-user+kernel} slope=${SLOPE:-none}"
  echo "# cmd=${CMDV[*]}"
  python3 - "$T" "$SLOPE" $list <<'PY'
import sys, os
d, slope, groups = sys.argv[1], sys.argv[2], sys.argv[3:]
def load(path):
    rows = {}
    for line in open(path):
        if not line.strip() or line.startswith('#'):
            continue
        f = line.rstrip('\n').split(',')
        # value, unit, event, stddev%, run-time, run%, ...
        val, ev = f[0], f[2]
        sd = f[3].rstrip('%') if len(f) > 3 else ''
        run = f[5] if len(f) > 5 else ''
        rows[ev.replace(':u', '')] = (val, sd, run)
    return rows
hdr = ['group', 'event', 'count', 'rsd%', 'run%']
if slope:
    var, vals = slope.split('=')
    a, b = (float(x) for x in vals.split(','))
    hdr = ['group', 'event', f'count({var}={vals.split(",")[0]})', f'count({var}={vals.split(",")[1]})', 'per-rep', 'rsd%', 'run%']
print('\t'.join(hdr))
for g in groups:
    A = load(os.path.join(d, f'{g}.a.csv'))
    B = load(os.path.join(d, f'{g}.b.csv')) if slope else None
    for ev, (val, sd, run) in A.items():
        if slope:
            vb, sdb, runb = B.get(ev, ('', '', ''))
            try:
                per = (float(vb) - float(val)) / (b - a)
                per = f'{per:.1f}'
            except ValueError:
                per = 'n/a'
            print('\t'.join([g, ev, val, vb, per, f'{sd}/{sdb}', f'{run}/{runb}']))
        else:
            print('\t'.join([g, ev, val, sd, run]))
PY
} > "$T/table"
cat "$T/table"
[ -n "$OUT" ] && cp "$T/table" "$OUT"
for f in "$T"/*.out; do
  if grep -qiE 'error|not supported|<not counted>' "$f"; then echo "--- $(basename "$f")"; grep -iE 'error|not supported|<not counted>' "$f" | head -5; fi
done
rm -rf "$T"
