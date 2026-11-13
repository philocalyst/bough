# profiles of bough's hot paths through the headless driver (CPU executor, --render), kept apart from timings.
# For each path: perf stat slope (five groups of three events, user mode, CPU 2), perf record (DWARF call graph, CPU 2),
# top ten symbols by self time, and a flamegraph SVG. Usage: prof-headless.sh [LABEL...]  (default: all)
source /root/work/r8/bench/env.sh
B=/root/work/r8/bench; P=$B/results/profiles; mkdir -p $P/svg $P/stat $P/top $P/collapsed
HL=$B/target-headless/release/headless
INF=/nix/store/qxrk44bap2vnldsmni7882ym9iyl8mvy-inferno-0.12.8/bin   # nixpkgs inferno 0.12.8, already in the store (built by `nix build nixpkgs#inferno`); nothing installed
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
ORDER="rows3k-hover table-select table-update10 table-swap table-remove table-insert table-replace table-append table-clear"
LABELS=${@:-$ORDER}
for L in $LABELS; do
  A="${ARGS[$L]} --render"
  echo "== $L: $A"
  # sanity run: what the driver did
  taskset -c 2 $HL $A --repeats 20 2>&1 | grep -E "^(mount|result)" | cut -c1-240
  # perf stat slope: ops per repeat = 2 (operation and its undo)
  [ "${DO_STAT:-1}" = 1 ] && bash $B/pstat4.sh $P/stat/$L.tsv $L 2 ${REPS_A:-100} ${REPS_B:-1000} -- $HL $A
  [ "${DO_REC:-1}" = 1 ] || continue
  # perf record, DWARF unwinding
  REC=$P/rec-$L.data
  taskset -c 2 $PERF record -q -F 4999 --call-graph dwarf,16384 -o $REC -- $HL $A --repeats ${REC_REPS:-2000} > $P/rec-$L.out 2>&1
  bash -c "$PERF script -i $REC 2>/dev/null | $INF/inferno-collapse-perf > $P/collapsed/$L.txt; $INF/inferno-flamegraph --title 'bough $L (headless driver, CPU executor)' --subtitle 'perf record DWARF, 4999 Hz, user+kernel samples of one thread pinned to CPU 2; profile, not a timing' --minwidth 0.2 < $P/collapsed/$L.txt > $P/svg/$L.svg"
  python3 $B/prof_an.py $P/collapsed/$L.txt $P/top/$L.json
  ls -la $P/svg/$L.svg
  rm -f $REC
done
