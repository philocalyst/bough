#!/bin/zsh
# Headless microbenchmarks (zgx micro_bench, GPUI Fast gpui-fast-micro), 3 repeats each.
# Raw stdout/stderr per run under raw/micro/. Font: DejaVu Sans via ZGX_FONT_FILE.
set -u
ZGX=/Users/mileswirht/projects/_ref/zgx
OUT=/Users/mileswirht/projects/bough/bench/baselines/macos/raw/micro
export ZGX_FONT_FILE=$ZGX/comparisons/fonts/DejaVuSans.ttf
MICRO=$ZGX/target/dist/examples/micro_bench
GPUI_MICRO=$ZGX/comparisons/gpui-fast-micro/target/dist/zgx-compare-gpui-fast-micro
for rep in 1 2 3; do
  for scene in state vscroll-fixed vscroll-variable; do
    # zgx, RENDER=0 (no GPU encode/submit)
    env SCENE=$scene RENDER=0 $MICRO > $OUT/zgx_${scene}_render0_rep${rep}.out 2> $OUT/zgx_${scene}_render0_rep${rep}.err; echo "zgx render0 $scene rep$rep exit=$?"
    # zgx, default renderer (RENDER=1)
    env SCENE=$scene RENDER=1 $MICRO > $OUT/zgx_${scene}_render1_rep${rep}.out 2> $OUT/zgx_${scene}_render1_rep${rep}.err; echo "zgx render1 $scene rep$rep exit=$?"
    # GPUI Fast (no RENDER switch: headless test platform, no renderer)
    env SCENE=$scene $GPUI_MICRO > $OUT/gpuifast_${scene}_rep${rep}.out 2> $OUT/gpuifast_${scene}_rep${rep}.err; echo "gpui-fast $scene rep$rep exit=$?"
  done
done
