# round 8 amendment 6: the matrix without the compositor's refresh grid. GB_TIMING selects fakeland's timing model (local runner
# patch, scripts/patch-timing.py). Everything pinned to CPUs 0-7 by run-gb2.sh. Sequential: no two runs overlap.
source /root/work/r8/bench/env.sh
B=/root/work/r8/bench
export GUIBENCH=$B/target-runner-timing/release/gui-bench
CMP="--quick --scenarios rows,table,ticker,spinner,typing,search,virtual,resize"
HEAVY="--scenarios sheet,canvas --frameworks bough,iced,egui,gpui-kit"
one() { # name mode timing args...
  n=$1; m=$2; t=$3; shift 3
  GB_TIMING=$t bash $B/run-gb2.sh $n $m "$@" || { echo "RUN FAILED $n rc=$?"; exit 1; }
  python3 $B/startup.py $B/results/$n/run $B/results/$n/flat/startup.tsv
}
one smoke-v1 zortax vrr:48-144 --quick --scenarios rows,spinner,typing --frameworks bough,iced
one cmp-u1 zortax unthrottled $CMP
one heavy-u1 scenes unthrottled $HEAVY
one cmp-v1 zortax vrr:48-144 $CMP
one heavy-v1 scenes vrr:48-144 $HEAVY
