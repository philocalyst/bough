#!/bin/sh
# bough's side of the zgui suite (headless, RENDER=0 unless set), each scenario in its own process, 3 runs;
# raw output to bench/results/input/bough/. zgui's side: bench/results/input/zgui/SUMMARY.md.
cd "$(dirname "$0")/../.."
B=${BOUGH_DIST:-/Users/mileswirht/projects/bough/target/input/dist}
O=bench/results/input/bough
mkdir -p $O
export ZGX_FONT_FILE=/Users/mileswirht/projects/_ref/zgx/comparisons/fonts/DejaVuSans.ttf
uptime > $O/uptime.txt
for run in 1 2 3; do
  for s in idle hover-storm scroll cold-start kitchen-sink app-shell; do
    $B/zgui_scenario $s > $O/$s.$run.txt 2>&1
  done
  $B/label_churn > $O/label-churn.$run.txt 2>&1
  $B/resize_slope > $O/resize-slope.$run.txt 2>&1
  /usr/bin/time -l $B/examples/startup_bench > $O/startup_bench.$run.txt 2>&1
done
uptime >> $O/uptime.txt
