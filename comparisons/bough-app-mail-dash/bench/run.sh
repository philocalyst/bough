#!/bin/bash
# bough's mail and dashboard apps, measured as zgx's suite.py measures apps_bench (least of 3 under
# /usr/bin/time -l), with zgx's configurations (bench/api/zgx/run_part1.sh): mail at REPS 20, the dashboard
# at REPS 5, RENDER 1 and 0. Build first:
#   CARGO_TARGET_DIR=/Users/mileswirht/projects/bough/target/api cargo build --profile dist -p bough-app-mail-dash
set -u
export ZGX_FONT_FILE=/Users/mileswirht/projects/_ref/zgx/comparisons/fonts/DejaVuSans.ttf
D=${D:-/Users/mileswirht/projects/bough/target/api/dist}
M="python3 $(dirname "$0")/measure.py"
rm -f "$(dirname "$0")/results.jsonl"
$M app-mail $D/apps_bench APP=mail
$M app-mail-R0 $D/apps_bench APP=mail RENDER=0
$M app-dashboard $D/apps_bench APP=dashboard REPS=5
$M app-dashboard-R0 $D/apps_bench APP=dashboard REPS=5 RENDER=0
$M app-dashboard-REPS20 $D/apps_bench APP=dashboard
$M app-dashboard-REPS20-R0 $D/apps_bench APP=dashboard RENDER=0
