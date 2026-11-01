#!/bin/sh
# GPU-time instruments (no root, no Screen Recording): the injectable per-process GPU-time reporter
# (gputime.dylib, headless runs of either framework) and zgx's own windowed sampler (macsample, the I/O
# Registry's per-client GPU time + IOReport's GPU clock). Output: $OUT (default target/floor-tools).
set -e
cd "$(dirname "$0")/../.."
OUT=${OUT:-$PWD/../bough/target/floor-tools}
mkdir -p "$OUT"
clang -O2 -dynamiclib -framework IOKit -framework CoreFoundation -lIOReport -o "$OUT/gputime.dylib" bench/floor/gputime.c
swiftc -O bench/windowed/macsample.swift -o "$OUT/macsample" -lIOReport
echo "$OUT/gputime.dylib $OUT/macsample"
