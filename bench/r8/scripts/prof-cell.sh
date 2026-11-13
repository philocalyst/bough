# prof-cell.sh LABEL SCENARIO ROWS MODE : one gui-bench cell of bough (scenes binary, CPU executor, real fakeland window
# host) with perf attached to the app process. Profiles only, never timings: perf perturbs the run.
#   MODE record        perf record -F 999 --call-graph dwarf,8192 -k CLOCK_MONOTONIC (sliced per probe afterwards)
#   MODE stat:GROUP    perf stat -I 50 of one group of three events (user mode); GROUP in cyc br l1 l2 ind
# The runner is pinned to CPUs 0-7 as in the timing runs; the harness's probe windows come from its own report files.
source /root/work/r8/bench/env.sh
B=/root/work/r8/bench; P=$B/results/profiles
LABEL=$1; SC=$2; ROWS=$3; MODE=$4
bash $B/links.sh scenes > /dev/null
O=$P/cell-$LABEL-${MODE//:/-}; rm -rf $O; mkdir -p $O
declare -A G=( [cyc]="cycles,instructions,branches" [br]="instructions,branches,branch-misses" [l1]="instructions,L1-dcache-loads,L1-dcache-load-misses" \
               [l2]="instructions,cache-references,cache-misses" [ind]="instructions,ex_ret_ind_brch_instr,ex_ret_brn_ind_misp" )
{ echo "label=$LABEL scenario=$SC rows=$ROWS mode=$MODE host=$(hostname -s) kernel=$(uname -r) governor=$(cat /sys/devices/system/cpu/cpu2/cpufreq/scaling_governor) load=$(cut -d' ' -f1-3 /proc/loadavg) date=$(date -u +%FT%TZ)"
  echo "perf=$($PERF --version) runner=$(sha256sum $B/gb/runner/target/release/gui-bench | cut -c1-16) app=$(sha256sum -L $B/gb/apps/bough/target/release/bough-bench | cut -c1-16)"; } > $O/IDENTITY.txt
( cd $B/gb && taskset -c 0-7 nice -n 10 runner/target/release/gui-bench --scenarios $SC --frameworks bough --rows $ROWS --hz 60 --out $O/run > $O/runner.log 2>&1; echo "runner rc=$?" >> $O/runner.log ) &
RP=$!
for i in $(seq 1 2000); do APID=$(pgrep -x bough-bench | head -1); [ -n "$APID" ] && break; sleep 0.01; done
[ -n "$APID" ] || { echo "no app process"; exit 1; }
case $MODE in
  record)
    $PERF record -q -F 999 --call-graph dwarf,8192 -k CLOCK_MONOTONIC -p $APID -o $O/perf.data > $O/perf.out 2>&1 ;;
  stat:*)
    g=${MODE#stat:}; ev=$(echo ${G[$g]} | tr ',' '\n' | sed 's/$/:u/' | paste -sd, -)
    echo "T0_uptime=$(cut -d' ' -f1 /proc/uptime) events=$ev" >> $O/IDENTITY.txt
    $PERF stat -I 50 -x, -e "{$ev}" -p $APID -o $O/stat.csv > $O/perf.out 2>&1 ;;
esac
wait $RP
tail -2 $O/runner.log; ls -la $O | head -12
