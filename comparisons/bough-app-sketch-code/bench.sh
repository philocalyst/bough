#!/bin/bash
# bough's and zgx's apps_bench for Sketch and Code, the way bench/api measures (`/usr/bin/time -l`, three runs,
# the least "instructions retired" whole process; per-interaction lines from each binary's own counters).
#
#   cargo build --profile dist -p bough-app-sketch-code     (CARGO_TARGET_DIR=.../target/api)
#   comparisons/bough-app-sketch-code/bench.sh               -> results/raw/*.txt, results/summary.tsv
#
# LINES is unset first: a terminal exports LINES=24, and both harnesses read LINES as the code editor's file
# length (zgx's default is 100,000). The 24-line runs are kept as their own labels (`-L24`).
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
BOUGH=${BOUGH_BIN:-$(git rev-parse --show-toplevel)/target/api/dist/apps_bench_sketch_code}
ZGX=${ZGX_BIN:-${BOUGH_REF:-$HOME/projects/_ref}/zgx/target/release/examples/apps_bench}
OUT=$HERE/results
mkdir -p "$OUT/raw"
export ZGX_FONT_FILE=${BOUGH_REF:-$HOME/projects/_ref}/zgx/comparisons/fonts/DejaVuSans.ttf
export ZGUI_FONT_FILE=$ZGX_FONT_FILE
unset LINES COLUMNS ONLY SKIP REPS SHAPES RENDER APP FULLSCAN
cd /tmp

# `bench.sh zgx` / `bench.sh bough`: only the labels starting with it.
FILTER=${1:-}
run() {
  local label=$1 bin=$2
  shift 2
  case "$label" in "$FILTER"*) ;; *) return ;; esac
  for i in 1 2 3; do
    env "$@" /usr/bin/time -l "$bin" >"$OUT/raw/$label.$i.txt" 2>&1
  done
  local least
  least=$(grep -h "instructions retired" "$OUT"/raw/"$label".[123].txt | awk '{print $1}' | sort -n | head -1)
  printf "%s\t%s\n" "$label" "$least" >>"$OUT/summary.tsv"
  echo "$label $least"
}

for app in sketch code; do
  run "bough-$app-R1" "$BOUGH" APP=$app RENDER=1
  run "bough-$app-R0" "$BOUGH" APP=$app RENDER=0
  run "zgx-$app-R1" "$ZGX" APP=$app RENDER=1
  run "zgx-$app-R0" "$ZGX" APP=$app RENDER=0
done
run "bough-code-R1-L24" "$BOUGH" APP=code RENDER=1 LINES=24
run "bough-code-R0-L24" "$BOUGH" APP=code RENDER=0 LINES=24
run "zgx-code-R1-L24" "$ZGX" APP=code RENDER=1 LINES=24
run "zgx-code-R0-L24" "$ZGX" APP=code RENDER=0 LINES=24
run "bough-code-R1-fullscan" "$BOUGH" APP=code RENDER=1 FULLSCAN=1
run "bough-code-R0-fullscan" "$BOUGH" APP=code RENDER=0 FULLSCAN=1
