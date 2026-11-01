#!/bin/sh
# Per-frame instructions (slope method) for every bough scene, RENDER 0 and 1. BOUGH_DIST picks the binaries
# (default: this branch's target/floor/dist). zgx's numbers: bench/results/fusion/slopes.txt (unchanged binaries).
cd "$(dirname "$0")/../../.."
export BOUGH_DIST=${BOUGH_DIST:-/Users/mileswirht/projects/bough/target/floor/dist}
export ZGX_HEADLESS=${ZGX_HEADLESS:-/Users/mileswirht/projects/bough/target/zgx-headless}
for r in 0 1; do
  python3 bench/integrate/slope.py bough $r heavy
  python3 bench/integrate/slope.py bough $r lab
  python3 bench/integrate/slope.py bough $r animate
  REPS=3 python3 bench/layout/slope.py bough $r
done
