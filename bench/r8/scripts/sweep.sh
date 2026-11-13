# run 3, refresh sweep: rows (hover), typing, spinner for bough, iced, freya, dioxus-skia, one invocation per rate.
# Starts when heavy-1 has finished (its log has the EXIT line), so two runs never overlap. Pinned to CPUs 0-7 by run-gb.sh.
source /root/work/r8/bench/env.sh
B=/root/work/r8/bench
until grep -q '^EXIT' $B/logs/heavy-1.log 2>/dev/null; do sleep 10; done
for hz in 60 120 240; do
  bash $B/run-gb.sh sweep-$hz zortax --quick --scenarios rows,typing,spinner --frameworks bough,iced,freya,dioxus-skia --hz $hz
  python3 $B/startup.py $B/results/sweep-$hz/run $B/results/sweep-$hz/flat/startup.tsv
done
