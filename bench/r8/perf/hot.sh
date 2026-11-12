#!/usr/bin/env bash
# hot.sh [options] BIN [CELL...] : the cost model of ROUND8 section 8 for the hot paths of one binary.
# Threads lane, round 8. Other lanes run it unchanged on their own binary (docs/lanes/threads/MEASURE.md).
#
# BIN honours the convention of crates/bough-x/examples/hot_x.rs:
#   CELLS=1 BIN            prints "name<TAB>what" per cell (one of them is `floor`: an empty repetition)
#   OP=cell REPS=n BIN     runs that cell n times after set-up
#   EVICT_KB=k             walks a k-KiB buffer between repetitions (outside the counted bracket)
#   BOUGH_PMU=a,b,c        extra in-process events (bough_mem::pmu), and BIN prints one TSV row per cell:
#                          scene cell evict_kb reps <event>_p50.. <event>_max.. [more]
#
# What it writes into OUT (default ./hot-out):
#   hot.tsv        one row per (cell, eviction level): instructions, cycles, branch misses, L1 misses, L2 misses,
#                  refills from L2 / L3 / DRAM, dTLB misses (p50 per repetition, and the max of instructions and
#                  cycles), and `chain_lb`: (cycles with L3 cold - cycles warm) / FILL_CYCLES, a lower bound on
#                  the dependent (chained) cache lines of the path. Each level's misses, read at the level
#                  evicted, count the distinct lines the path touches.
#   pstat-<cell>.tsv   (-x) the same cell by `perf stat` slopes with `floor` subtracted: the cross-check
#   <cell>.svg     (-f) a flamegraph of the cell (perf record, dwarf unwinding; inferno)
#   first line(s): the identity record (host, load, commit, binary hash)
#
# Options:
#   -o DIR    output directory
#   -r N      repetitions per cell for the in-process table (default 200; the cold levels use at most 64)
#   -c CPU    pin to this CPU (Linux; default 2)
#   -x        also run the perf stat cross-check (Linux)
#   -f        also record flamegraphs (Linux; needs `nix shell nixpkgs#inferno` or inferno on PATH)
#   -R ROWS   passed to BIN as ROWS
#
# Linux needs perf on PATH (ilo-prod: /nix/store/bw8frr0nk26m740a2mwaw5yi06mgxakb-perf-linux-7.3.1/bin) for -x
# and -f; the in-process table needs only perf_event_paranoid <= 2. On macOS only instructions and cycles
# exist (thread_selfcounts), so the miss columns are empty and the cold levels still show as cycles.
# Zen 2: `cache-misses` are L2 misses; groups of three events (more are multiplexed); no L3 miss event, so
# what came from L3 or DRAM is read from the refill sources (dc-fill-ccx, dc-fill-dram).
set -u
OUT=hot-out; R=200; CPU=2; XCHECK=0; FLAME=0; ROWS=""
while [ $# -gt 0 ]; do
  case $1 in
    -o) OUT=$2; shift 2 ;;
    -r) R=$2; shift 2 ;;
    -c) CPU=$2; shift 2 ;;
    -x) XCHECK=1; shift ;;
    -f) FLAME=1; shift ;;
    -R) ROWS=$2; shift 2 ;;
    -h|--help) sed -n '2,/^set -u/p' "$0" | sed '$d' | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) break ;;
  esac
done
[ $# -gt 0 ] || { echo "hot: no binary (see --help)" >&2; exit 2; }
BIN=$1; shift
[ -x "$BIN" ] || { echo "hot: $BIN is not executable" >&2; exit 2; }
HERE=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$OUT"
[ -n "$ROWS" ] && export ROWS
if [ $# -gt 0 ]; then CELLS_="$*"; else CELLS_=$(CELLS=1 "$BIN" | cut -f1 | tr '\n' ' '); fi

# cache sizes of the CPU we run on, in KiB; a level is evicted by walking twice its size
kib() { case $1 in *K) echo "${1%K}" ;; *M) echo $(( ${1%M} * 1024 )) ;; *) echo $(( $1 / 1024 )) ;; esac; }
if [ "$(uname)" = Linux ]; then
  PIN="taskset -c $CPU"
  C=/sys/devices/system/cpu/cpu$CPU/cache
  L1=0; L2=0; L3=0
  for i in "$C"/index*; do
    [ "$(cat "$i/type")" = Instruction ] && continue
    s=$(kib "$(cat "$i/size")")
    case $(cat "$i/level") in 1) L1=$s ;; 2) L2=$s ;; 3) L3=$s ;; esac
  done
  if grep -q GenuineIntel /proc/cpuinfo; then
    # Intel (mothership, Coffee Lake): the generic cache events are last-level-cache requests and misses; L2
    # misses of retired loads are a raw event. `event=alias` renames a column.
    GROUPS_="branch-misses,l1d-misses,raw:0x10d1=l2-load-misses cache-refs=llc-refs,cache-misses=llc-misses,dtlb-misses"
  else
    GROUPS_="branch-misses,l1d-misses,cache-misses dc-fill-l2,dc-fill-ccx,dc-fill-dram dtlb-misses"
  fi
  [ -n "${HOT_GROUPS:-}" ] && GROUPS_=$HOT_GROUPS
  FILL_CYCLES=${FILL_CYCLES:-300}
else
  PIN=""
  L1=$(( $(sysctl -n hw.perflevel0.l1dcachesize 2>/dev/null || echo 65536) / 1024 ))
  L2=$(( $(sysctl -n hw.perflevel0.l2cachesize 2>/dev/null || echo 4194304) / 1024 ))
  L3=0
  GROUPS_="none"
  FILL_CYCLES=${FILL_CYCLES:-350}
fi
LEVELS="warm:0 l1:$(( L1 * 2 )) l2:$(( L2 * 2 ))"
[ "$L3" -gt 0 ] && LEVELS="$LEVELS l3:$(( L3 * 2 ))"

T=$(mktemp -d)
for lv in $LEVELS; do
  name=${lv%%:*}; kb=${lv#*:}
  reps=$R; [ "$name" != warm ] && [ "$reps" -gt 64 ] && reps=64
  g=0
  for grp in $GROUPS_; do
    g=$(( g + 1 ))
    pmu=$(echo "$grp" | tr ',' '\n' | sed 's/=.*//' | paste -sd, -); [ "$grp" = none ] && pmu=""
    # column renames for this group: `event=alias` (a raw event prints as `raw`)
    ren=""
    for ev in $(echo "$grp" | tr ',' ' '); do
      case $ev in *=*) n=${ev%%=*}; a=${ev#*=}; case $n in raw:*) n=raw ;; esac; ren="$ren -e s/\t${n}_p50/\t${a}_p50/ -e s/\t${n}_max/\t${a}_max/" ;; esac
    done
    for cell in $CELLS_; do
      env OP="$cell" REPS="$reps" EVICT_KB="$kb" BOUGH_PMU="$pmu" $PIN "$BIN" 2>>"$T/err" | sed $ren -e "s/^/$name\t$g\t/" >> "$T/raw"
    done
  done
done

{
  "$HERE/identity.sh" "$BIN"
  echo "# hot.sh cpu_pin=${PIN:-none} l1d=${L1}KiB l2=${L2}KiB l3=${L3}KiB evict=2x fill_cycles=$FILL_CYCLES reps=$R (cold levels at most 64)"
  cp "$T/raw" "$OUT/raw.tsv"
  if command -v python3 >/dev/null && python3 -c 1 2>/dev/null; then
    python3 "$HERE/hot_merge.py" "$T/raw" "$FILL_CYCLES"
  else
    echo "# no python3 here: fold $OUT/raw.tsv with bench/r8/perf/hot_merge.py RAW $FILL_CYCLES on another host"
  fi
  [ -s "$T/err" ] && { echo "# stderr of the runs:"; sed 's/^/#   /' "$T/err" | sort | uniq -c | head -20; }
} > "$OUT/hot.tsv"
cat "$OUT/hot.tsv"

if [ "$XCHECK" = 1 ] && [ "$(uname)" = Linux ]; then
  # floor's slope is the cost of the loop itself; a cell's slope minus it is the cell
  for cell in $CELLS_; do
    OP=$cell "$HERE/pstat.sh" -c "$CPU" -g core,l1,l2,fill,tlb -s REPS=2000,22000 -o "$OUT/pstat-$cell.tsv" -- "$BIN" > /dev/null
  done
  python3 - "$OUT" $CELLS_ <<'PY'
import sys, os
out, cells = sys.argv[1], sys.argv[2:]
def load(c):
    d = {}
    p = os.path.join(out, f'pstat-{c}.tsv')
    if not os.path.exists(p):
        return d
    for line in open(p):
        f = line.rstrip('\n').split('\t')
        if len(f) >= 7 and f[0] not in ('group',) and not line.startswith('#'):
            try:
                d[(f[0], f[1])] = (float(f[4]), f[6])
            except ValueError:
                pass
    return d
fl = load('floor')
with open(os.path.join(out, 'pstat.tsv'), 'w') as w:
    w.write('cell\tgroup\tevent\tper_rep_minus_floor\trun%\n')
    for c in cells:
        if c == 'floor':
            continue
        for k, (v, run) in load(c).items():
            w.write(f'{c}\t{k[0]}\t{k[1]}\t{v - fl.get(k, (0.0, ""))[0]:.1f}\t{run}\n')
print(open(os.path.join(out, 'pstat.tsv')).read())
PY
fi

if [ "$FLAME" = 1 ] && [ "$(uname)" = Linux ]; then
  # inferno: on PATH, or in $INFERNO (a directory), or built once by nix (the first build fetches nixpkgs)
  if ! command -v inferno-flamegraph >/dev/null; then
    [ -n "${INFERNO:-}" ] || INFERNO=$(nix build --no-link --print-out-paths nixpkgs#inferno 2>/dev/null)/bin
    PATH=$PATH:$INFERNO
  fi
  command -v inferno-flamegraph >/dev/null || { echo "hot: no inferno (set INFERNO=<dir with inferno-flamegraph>)" >&2; FLAME=0; }
fi
if [ "$FLAME" = 1 ] && [ "$(uname)" = Linux ]; then
  inf() { "$@"; }
  for cell in $CELLS_; do
    [ "$cell" = floor ] && continue
    # about a second of the cell: repetitions from its warm cycles
    cyc=$(awk -F'\t' -v c="$cell" '$2==c && $3=="warm" {print $7}' "$OUT/hot.tsv" | head -1)
    n=$(python3 -c "print(max(2000, min(20000000, int(3e9 / max(float('${cyc:-1000}' or 1000), 50)))))")
    env OP="$cell" REPS="$n" $PIN perf record -q -e cycles:u -F 4999 --call-graph dwarf,8192 -o "$T/$cell.data" -- "$BIN" > /dev/null 2>>"$T/err"
    perf script -i "$T/$cell.data" 2>/dev/null | inf inferno-collapse-perf > "$T/$cell.folded" 2>/dev/null
    inf inferno-flamegraph --title "$(basename "$BIN") $cell ($n repetitions)" "$T/$cell.folded" > "$OUT/$cell.svg" 2>/dev/null
    cp "$T/$cell.folded" "$OUT/$cell.folded"
    echo "flamegraph: $OUT/$cell.svg ($(wc -l < "$T/$cell.folded") stacks)"
  done
fi
rm -rf "$T"
