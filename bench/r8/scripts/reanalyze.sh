source /root/work/r8/bench/env.sh
B=/root/work/r8/bench; P=$B/results/profiles
for f in $P/collapsed/*.txt; do k=$(basename $f .txt); echo "== $k"; python3 $B/prof_an.py $f $P/top/$k.json; done
