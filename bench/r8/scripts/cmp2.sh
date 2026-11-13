# wait for a quiet host (1-minute load below 0.8, up to 10 minutes), then cmp-2
for i in $(seq 1 120); do l=$(cut -d' ' -f1 /proc/loadavg); awk -v l=$l 'BEGIN{exit !(l<0.8)}' && break; sleep 5; done
bash /root/work/r8/bench/run-gb.sh cmp-2 zortax --quick --scenarios rows,table,ticker,spinner,typing,search,virtual,resize
