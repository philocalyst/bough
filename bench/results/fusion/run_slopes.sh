#!/bin/sh
# Per-frame instructions, slope method, every scene, bough and zgx, RENDER 0 and 1 (median of 3; vlist of 5).
cd "$(dirname "$0")/../../.."
for fw in bough zgx; do
  for r in 0 1; do
    python3 bench/integrate/slope.py $fw $r heavy
    python3 bench/integrate/slope.py $fw $r lab
    python3 bench/integrate/slope.py $fw $r animate
    REPS=3 python3 bench/layout/slope.py $fw $r
  done
done
