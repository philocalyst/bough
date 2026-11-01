#!/bin/sh
# Windowed adapters, 10 s each, sampled at 1.5 s and 9 s from outside (bench/integrate/windowed.py), bough and zgx
# interleaved, two repetitions (idle: one). Do not run while compiling.
cd "$(dirname "$0")/../../.."
W="python3 bench/integrate/windowed.py"
for rep in 1 2; do
  for cell in "heavy stream" "heavy scroll" "heavy both" "lab stream" "lab scroll" "lab both" "animate -" "blink -" "vlist scroll" "vlist fling" "vlist jump"; do
    for fw in bough zgx; do $W $fw $cell; done
  done
done
for cell in "heavy idle" "lab idle"; do for fw in bough zgx; do $W $fw $cell; done; done
