#!/usr/bin/env bash
# run-gb.sh NAME LINKMODE RUNNER_ARGS... : one gui-bench runner invocation on ilo-prod with an identity record.
#   Results: /root/work/r8/bench/results/NAME/{run,IDENTITY.txt,load.log,runner.log,flat}. Not for timing anything else
#   at the same time. LINKMODE zortax|scenes selects which binaries gb/apps/*/target point at (links.sh).
source /root/work/r8/bench/env.sh
B=/root/work/r8/bench; GB=$B/gb
NAME=$1; MODE=$2; shift 2
O=$B/results/$NAME; mkdir -p $O
[ -e $O/run ] && { echo "refusing to overwrite $O/run"; exit 3; }
bash $B/links.sh $MODE > $O/links.txt
I=$O/IDENTITY.txt
{
 echo "name=$NAME linkmode=$MODE host=$(hostname -s) kernel=$(uname -r) start=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
 echo "cpu=$(grep -m1 'model name' /proc/cpuinfo | cut -d: -f2 | xargs) threads=$(nproc) mem=$(( $(grep MemTotal /proc/meminfo | awk '{print $2}') / 1048576 ))GiB"
 echo "governors=$(cat /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor | sort | uniq -c | xargs) boost=$(cat /sys/devices/system/cpu/cpufreq/boost 2>/dev/null) cpu0_mhz=$(grep -m1 'cpu MHz' /proc/cpuinfo | cut -d: -f2 | xargs) nmi_watchdog=$(cat /proc/sys/kernel/nmi_watchdog) perf_event_paranoid=$(cat /proc/sys/kernel/perf_event_paranoid)"
 echo "load_start=$(cut -d' ' -f1-3 /proc/loadavg)"
 echo "top_procs_start: $(ps -eo pcpu,comm --sort=-pcpu | sed -n 2,5p | xargs)"
 echo "gui-bench=$(git -C $GB rev-parse HEAD) (+dirty runner overlay: bench lane scenes + bough registered) fakeland=$(git -C $B/fakeland rev-parse HEAD)"
 echo "runner_sha256=$(sha256sum $GB/runner/target/release/gui-bench | cut -c1-16)"
 echo "bough=$(git -C $B/bough rev-parse HEAD) (plain, clean tree)  bough-scenes tree=$(git -C $B/bough-scenes rev-parse HEAD) + bench overlay (bough-guibench-scenes, bough-workloads, bough-mem/pmu.rs from r7/bench 580bcd2981)"
 echo "binaries (sha256[:16] bytes path):"
 for f in $(find -L $GB/apps -maxdepth 5 -path '*/release/*-bench' -type f | sort); do echo "  $(sha256sum $f | cut -c1-16) $(stat -L -c %s $f) ${f#$GB/}"; done
 echo "args=$*"
 echo "pin: runner (compositor, apps and children inherit) taskset -c ${PIN:-0-7}; other lanes confined to CPUs 8-15 at nice 19 by the lead from 21:3x UTC (CPU n and n+8 are SMT siblings, so cores are still shared)"
 echo "env: nice=10 renderer=pixman(fakeland) others=lavapipe(zortax builds) bough=CPU executor into wl_shm"
} > $I
( while true; do echo "$(date -u +%H:%M:%S) $(cut -d' ' -f1-3 /proc/loadavg) | $(ps -eo pcpu,comm --sort=-pcpu | sed -n 2,4p | xargs)"; sleep 15; done ) > $O/load.log 2>&1 &
SP=$!
cd $GB
taskset -c ${PIN:-0-7} nice -n 10 runner/target/release/gui-bench "$@" --out $O/run > $O/runner.log 2>&1
rc=$?
kill $SP 2>/dev/null
{ echo "runner rc=$rc end=$(date -u +%Y-%m-%dT%H:%M:%SZ) load_end=$(cut -d' ' -f1-3 /proc/loadavg)"
  echo "load_during: min/max/mean of 1-min load samples: $(awk '{print $2}' $O/load.log | sort -n | awk '{a[NR]=$1;s+=$1}END{printf "%s %s %.2f (n=%d)", a[1], a[NR], s/NR, NR}')"; } >> $I
python3 $B/flatten.py $O/run $O/flat
echo "flat: $(wc -l < $O/flat/metrics.tsv) metric rows" >> $I
echo DONE $NAME rc=$rc
printf '{"run":"%s","runner_rc":%s,"end":"%s"}\n' "$NAME" "$rc" "$(date -u +%FT%TZ)" > $O/STATUS.json
# B09 (independent review): the script's own exit status is the runner's, not echo's. Runs before this fix were re-checked by tools/run_status.py.
exit $rc
