# indirect-branch group for every headless path (the first pass used event names that do not exist on Zen 2)
source /root/work/r8/bench/env.sh
B=/root/work/r8/bench; P=$B/results/profiles; HL=$B/target-headless/release/headless
declare -A ARGS=(
  [rows3k-hover]="--scenario rows --rows 3000 --op hover"
  [table-select]="--scenario table --rows 1000 --variant fine --op select"
  [table-update10]="--scenario table --rows 1000 --variant fine --op update10"
  [table-swap]="--scenario table --rows 1000 --variant fine --op swap"
  [table-remove]="--scenario table --rows 1000 --variant fine --op remove"
  [table-insert]="--scenario table --rows 1000 --variant fine --op insert"
  [table-replace]="--scenario table --rows 1000 --variant fine --op replace"
  [table-append]="--scenario table --rows 1000 --variant fine --op append"
  [table-clear]="--scenario table --rows 1000 --variant fine --op clear"
)
for L in rows3k-hover table-select table-update10 table-swap table-remove table-insert table-replace table-append table-clear; do
  echo "== $L"
  ONLY=g_ind bash $B/pstat5.sh $P/stat/$L.ind.tsv $L 2 100 1000 -- $HL ${ARGS[$L]} --render
done
