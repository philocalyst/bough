source /root/work/r8/bench/env.sh
B=/root/work/r8/bench
export GB_TIMING=unthrottled GUIBENCH=$B/target-runner-timing/release/gui-bench
bash $B/run-gb2.sh smoke-u1 zortax --quick --scenarios rows,spinner,typing,resize --frameworks bough,iced
python3 $B/startup.py $B/results/smoke-u1/run $B/results/smoke-u1/flat/startup.tsv
