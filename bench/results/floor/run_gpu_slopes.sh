#!/bin/sh
# GPU ms per frame (headless RENDER=1, slope method, I/O Registry GPU time via gputime.dylib) for every scene,
# bough (BOUGH_DIST) and zgx. Build the instruments first: bench/floor/build.sh.
cd "$(dirname "$0")/../../.."
export BOUGH_DIST=${BOUGH_DIST:-$(git rev-parse --show-toplevel)/target/floor/dist}
export ZGX_HEADLESS=${ZGX_HEADLESS:-$(git rev-parse --show-toplevel)/target/zgx-headless}
for fw in ${FWS:-bough zgx}; do
  python3 bench/floor/gpu_slope.py $fw heavy stream scroll both
  python3 bench/floor/gpu_slope.py $fw lab stream scroll both
  python3 bench/floor/gpu_slope.py $fw animate
  python3 bench/floor/gpu_slope.py $fw vlist scroll fling jump
done
