#!/usr/bin/env bash
# baseline.sh OUTDIR [scene...] : the standing workloads' baseline set (SCENES.md), one file per configuration:
#   once-render.tsv        MODE=once RENDER=1   (the headline: wires only, the GPU draws)
#   redescribe-render.tsv  MODE=redescribe RENDER=1
#   once-cpu.tsv           MODE=once RENDER=0   (frames end at the tile list: the CPU's share alone)
#   alloc.tsv              workload_alloc, MODE=once RENDER=1 (allocations per step; its counters are not release numbers)
#   cold.txt               hyperfine of PHASES=cold per scene (exec to first frame), and /usr/bin/time's peak RSS
# REPS (default 32) and SCENE_SCALE (default 1) pass through. Build first:
#   cargo build --profile dist -p bough-workloads --bin workload --bin workload_alloc
set -u
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
OUT=$1; shift
SCENES=${*:-sheet ide dash canvas chat}
# copy the binaries first: another cargo invocation in the same target dir (an integration test of this crate
# rebuilds its binaries, with whatever features it was given) must not swap them mid-run
TT=$(mktemp -d); T=$TT/dist; mkdir -p "$T"   # keeps "/dist/" in the path, which the identity record reads
cp "${CARGO_TARGET_DIR:-$ROOT/target}/dist/workload" "${CARGO_TARGET_DIR:-$ROOT/target}/dist/workload_alloc" "$T/"
trap 'rm -rf "$TT"' EXIT
mkdir -p "$OUT"
export REPS=${REPS:-32} SCENE_SCALE=${SCENE_SCALE:-1}
run() { # file env... : run every scene with the given environment into one file
  local f=$1; shift
  { "$ROOT/bench/perf/identity.sh" "$BIN"; echo "# env $* REPS=$REPS SCENE_SCALE=$SCENE_SCALE"; } > "$OUT/$f"
  for s in $SCENES; do env "$@" "$BIN" "$s" 2>&1 | { [ "$s" = "${SCENES%% *}" ] && cat || grep -v '^scene.phase'; } >> "$OUT/$f"; done
}
BIN=$T/workload run once-render.tsv MODE=once RENDER=1
BIN=$T/workload run redescribe-render.tsv MODE=redescribe RENDER=1
BIN=$T/workload run once-cpu.tsv MODE=once RENDER=0
BIN=$T/workload_alloc run alloc.tsv MODE=once RENDER=1
{
  "$ROOT/bench/perf/identity.sh" "$T/workload"
  for s in $SCENES; do
    echo "## $s"
    PHASES=cold REPS=1 hyperfine -N --warmup 2 --runs 10 "$T/workload $s" 2>&1 | grep -E 'Wall|Memory|Time'
    if [ "$(uname)" = Darwin ]; then
      PHASES=cold REPS=1 /usr/bin/time -l "$T/workload" "$s" 2>&1 >/dev/null | grep -E 'real|maximum resident|peak memory|instructions retired'
    fi
  done
} > "$OUT/cold.txt" 2>&1
echo "wrote $OUT"
