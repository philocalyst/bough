#!/usr/bin/env bash
# pstat3.sh OUT LABEL OPS_PER_REPEAT REPS_A REPS_B -- BIN ARGS-BEFORE-REPEATS... : perf stat of a driver in groups of
# three events (all user mode, pinned to CPU 2), each group run at two repeat counts; the per-operation figure is
# (count(B) - count(A)) / ((B - A) * OPS_PER_REPEAT), so mount and process start cost cancel. The command gets
# `--repeats N` appended (bough headless driver) or, with env style, use VARNAME via REPEAT_VAR.
# Groups: g_cyc (cycles instructions branches), g_br (instructions branches branch-misses),
#         g_l1 (instructions L1-dcache-loads L1-dcache-load-misses), g_l2 (instructions cache-references cache-misses).
# Zen 2 note: cache-misses is L2 misses, cache-references all L2 requests, L1-dcache-load-misses L2 requests from the
# data cache (MEASUREMENT.md). A group of four or more is multiplexed to ~50 % on this host; three schedule 100 %.
source /root/work/r8/bench/env.sh
OUT=$1; LABEL=$2; OPR=$3; A=$4; B=$5; shift 6
R=${PSTAT_RUNS:-3}
declare -A G=( [g_cyc]="cycles,instructions,branches" [g_br]="instructions,branches,branch-misses" \
               [g_l1]="instructions,L1-dcache-loads,L1-dcache-load-misses" [g_l2]="instructions,cache-references,cache-misses" )
T=$(mktemp -d)
for g in g_cyc g_br g_l1 g_l2; do
  ev=$(echo ${G[$g]} | tr ',' '\n' | sed 's/$/:u/' | paste -sd, -)
  for rep in $A $B; do
    if [ -n "$REPEAT_VAR" ]; then
      env $REPEAT_VAR=$rep taskset -c 2 $PERF stat -x, -r $R -e "{$ev}" -o $T/$g.$rep.csv -- "$@" > $T/$g.$rep.out 2>&1
    else
      taskset -c 2 $PERF stat -x, -r $R -e "{$ev}" -o $T/$g.$rep.csv -- "$@" --repeats $rep > $T/$g.$rep.out 2>&1
    fi
  done
done
{
  echo "# label=$LABEL ops_per_repeat=$OPR repeats=$A,$B perf_runs=$R cpu_pin=2 mode=user"
  echo "# host=$(hostname -s) kernel=$(uname -r) governor=$(cat /sys/devices/system/cpu/cpu2/cpufreq/scaling_governor) load=$(cut -d' ' -f1-3 /proc/loadavg) date=$(date -u +%FT%TZ)"
  echo "# perf=$($PERF --version) bin=$(sha256sum $1 | cut -c1-16) cmd=$*"
  python3 - $T $OPR $A $B <<'PY'
import sys,os,glob
d,opr,a,b=sys.argv[1],float(sys.argv[2]),sys.argv[3],sys.argv[4]
print("group\tevent\tcount_A\tcount_B\tper_op\trsd%_B\trun%_B")
def load(p):
    r={}
    for l in open(p):
        if not l.strip() or l.startswith('#'): continue
        f=l.rstrip('\n').split(',')
        r[f[2].replace(':u','')]=(f[0],f[3] if len(f)>3 else '',f[5] if len(f)>5 else '')
    return r
for g in ("g_cyc","g_br","g_l1","g_l2"):
    A=load(f"{d}/{g}.{a}.csv"); B=load(f"{d}/{g}.{b}.csv")
    for ev,(vb,sd,run) in B.items():
        va=A.get(ev,('',))[0]
        try: per=(float(vb)-float(va))/((float(b)-float(a))*opr)
        except: per=float('nan')
        print(f"{g}\t{ev}\t{va}\t{vb}\t{per:.2f}\t{sd}\t{run}")
PY
} > $OUT
for f in $T/*.out; do grep -qiE 'error|not supported|<not counted>' $f && { echo "--- $(basename $f)"; grep -iE 'error|not supported|<not counted>' $f | head -3; }; done
rm -rf $T
cat $OUT
