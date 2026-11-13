# heavy scenes, run 2: sheet 1,000 and 10,000 rows, canvas 1,000 and 5,000 nodes, bough/iced/egui/gpui-kit, one invocation
source /root/work/r8/bench/env.sh
B=/root/work/r8/bench
bash $B/wait-quiet.sh ${WAITMIN:-20} ${QUIET:-2} $B/logs/heavy-1-wait.log
bash $B/run-gb.sh heavy-1 scenes --scenarios sheet,canvas --frameworks bough,iced,egui,gpui-kit
python3 $B/startup.py $B/results/heavy-1/run $B/results/heavy-1/flat/startup.tsv
