#!/usr/bin/env bash
# run.sh [OUT.tsv] [scene...] : the standing workloads with an identity record (MEASUREMENT.md).
# Environment as `bough_workloads::harness` (SCENE_SCALE, REPS, MODE, RENDER, SCALE, PHASES, BOUGH_PMU), plus
# BIN (default <checkout>/target/dist/workload; build it with
# `cargo build --profile dist -p bough-workloads --bin workload --bin workload_alloc`).
# Writes to OUT.tsv when given (and prints it); every line is TSV, `#` lines are the identity and the model.
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
BIN=${BIN:-${CARGO_TARGET_DIR:-$ROOT/target}/dist/workload}
OUT=""
case ${1:-} in *.tsv) OUT=$1; shift ;; esac
{
  "$ROOT/bench/perf/identity.sh" "$BIN"
  echo "# env SCENE_SCALE=${SCENE_SCALE:-1} REPS=${REPS:-48} MODE=${MODE:-once} RENDER=${RENDER:-default} SCALE=${SCALE:-2} PHASES=${PHASES:-default} BOUGH_PMU=${BOUGH_PMU:-}"
  "$BIN" "$@"
} | if [ -n "$OUT" ]; then tee "$OUT"; else cat; fi
