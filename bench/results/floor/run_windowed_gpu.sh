#!/bin/sh
# 10 s windows sampled by zgx's own sampler (bench/floor/windowed_gpu.py: CPU, instructions, footprint, GPU time
# from the I/O Registry, GPU Mcycles/s, window server CPU/GPU), every STATUS §1.2 cell: bough after
# ($AFTER), bough before ($BEFORE, optional) and zgx, interleaved per cell. One JSON line per run.
cd "$(dirname "$0")/../../.."
W="python3 bench/floor/windowed_gpu.py"
for cell in "heavy stream" "heavy scroll" "heavy both" "lab stream" "lab scroll" "lab both" "animate -" "blink -" "vlist scroll" "vlist fling" "vlist jump" "heavy idle" "lab idle"; do
  BOUGH_DIST=$AFTER $W bough $cell | sed 's/^/after /'
  [ -n "$BEFORE" ] && BOUGH_DIST=$BEFORE $W bough $cell | sed 's/^/before /'
  $W zgx $cell | sed 's/^/zgx /'
done
