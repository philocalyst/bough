#!/usr/bin/env bash
# lines.sh [-o OUT] [-c CPU] [-r REPS] BIN CELL... : which cache lines a cell touches when it starts cold, by
# what they are. Threads lane, round 8 (docs/lanes/threads/MEASURE.md). Linux, Intel (PEBS data addresses);
# works as an ordinary user with perf_event_paranoid <= 2.
#
# BIN follows hot.sh's convention and, with REGIONS=1, prints `REGION name start bytes page rec_off` lines on
# stderr when it ends (crates/bough-x/examples/hot_x.rs; `World::regions`).
#
# BIN also honours PERF_CTL / PERF_ACK (perf's control FIFOs): it enables the sampler for its measured
# repetitions only, so set-up is not sampled.
#
# For each cell, three sampled runs (every event is a sample, `-c 1`), each with the caches below made cold
# between repetitions by a store-only walk:
#   data, L1 cold   mem_load_retired.l1_miss   -> every data line the cell loads from
#   data, L3 cold   mem_load_retired.l3_miss   -> the data lines that come from memory
#   code, L3 cold   frontend_retired.l2_miss   -> the code lines fetched from L3 or memory
# and the same for `floor`, whose counts per kind (the harness's own lines) are subtracted. Output: OUT/<cell>.lines.tsv with, per
# kind of memory: distinct lines, and samples per repetition; OUT/<cell>.code.tsv with code lines per function.
set -u
OUT=lines-out; CPU=3; R=64
while [ $# -gt 0 ]; do
  case $1 in -o) OUT=$2; shift 2 ;; -c) CPU=$2; shift 2 ;; -r) R=$2; shift 2 ;; *) break ;; esac
done
BIN=$1; shift
HERE=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$OUT"
kib() { case $1 in *K) echo "${1%K}" ;; *M) echo $(( ${1%M} * 1024 )) ;; *) echo $(( $1 / 1024 )) ;; esac; }
C=/sys/devices/system/cpu/cpu$CPU/cache; L1=32; L3=8192
for i in "$C"/index*; do
  [ "$(cat "$i/type")" = Instruction ] && continue
  s=$(kib "$(cat "$i/size")"); case $(cat "$i/level") in 1) L1=$s ;; 3) L3=$s ;; esac
done
T=$(mktemp -d)
rec() { # tag event evict_kb cell fields
  rm -f "$T/ctl" "$T/ack"; mkfifo "$T/ctl" "$T/ack"
  # started disabled; the binary enables the sampler around its measured repetitions (PERF_CTL / PERF_ACK)
  env OP="$4" REPS="$R" EVICT_KB="$3" REGIONS=1 PERF_CTL="$T/ctl" PERF_ACK="$T/ack" taskset -c "$CPU" perf record -q -e "$2" -c 1 -d -D -1 --control "fifo:$T/ctl,$T/ack" -o "$T/$1.data" -- "$BIN" > /dev/null 2> "$T/$1.regions"
  perf script -i "$T/$1.data" -F "$5" 2>/dev/null > "$T/$1.txt"
}
{
  "$HERE/identity.sh" "$BIN"
  echo "# lines.sh cpu=$CPU reps=$R l1_evict=$(( L1 * 2 ))KiB l3_evict=$(( L3 * 2 ))KiB"
} > "$OUT/identity.txt"
for cell in floor "$@"; do
  rec "$cell.l1" mem_load_retired.l1_miss:upp $(( L1 * 2 )) "$cell" addr,ip,sym
  rec "$cell.l3" mem_load_retired.l3_miss:upp $(( L3 * 2 )) "$cell" addr,ip,sym
  rec "$cell.code" frontend_retired.l2_miss:upp $(( L3 * 2 )) "$cell" ip,sym
done
for cell in "$@"; do
  python3 "$HERE/lines_fold.py" "$T" "$cell" "$R" > "$OUT/$cell.lines.tsv"
  cat "$OUT/identity.txt" "$OUT/$cell.lines.tsv"
done
rm -rf "$T"
