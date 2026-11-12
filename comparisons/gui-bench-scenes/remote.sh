#!/usr/bin/env bash
# remote.sh COMMAND ... : the porting loop on the remote host (ilo-prod), for one app at a time.
#
#   remote.sh build APP              sync APP's overlay (and apps/shared), build it (release), print the errors
#   remote.sh cell APP SCENE [ROWS]  run one quick gui-bench cell of SCENE for APP (default ROWS: the scene's
#                                    quick size); print the outcome, the probes' medians and the layout check, and
#                                    copy the screenshot to $SHOTS/APP-SCENE-ROWS.png (default: /tmp)
#   remote.sh dev APP SCENE [ROWS]   build, then cell
#   remote.sh runner                 sync the runner overlay and build the runner (the bench lane does this)
#   remote.sh log APP                the last build log of APP
#
# APP is iced, egui, gpui-kit or bough. Environment as bench/remote (REX_SESSION_ID). Builds run on the remote's
# build cores at nice 10, -j6; a cell runs the runner under nice 10. One command at a time per APP.
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
B=${REMOTE_BENCH:-/root/work/r7/bench}
GB=$B/gb
SHOTS=${SHOTS:-/tmp}
cmd=${1:?command}; shift
T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT

# run a remote script as its own rex block, wait for its log's DONE line, print the log
run_bg() { # name script-file tries
  local log=$B/logs/dev-$1.log
  printf 'rm -f %s\n' "$log" > "$T/rm.sh"; "$ROOT/bench/remote/rx.sh" "$T/rm.sh" 10 >/dev/null
  "$ROOT/bench/remote/bg.sh" "$log" "$2" >/dev/null
  printf 'for i in $(seq 1 280); do grep -q "^EXIT" %s 2>/dev/null && break; sleep 3; done; cat %s\n' "$log" "$log" > "$T/w.sh"
  "$ROOT/bench/remote/rx.sh" "$T/w.sh" "${3:-450}"
}

# bough's app builds from the bough checkout next to gui-bench: push the working-tree files of its scene crates
push_bough() {
  local f
  for f in comparisons/gui-bench-scenes/overlay/apps/shared/scenes.rs comparisons/bough-guibench-scenes/Cargo.toml \
    comparisons/bough-guibench-scenes/src/*.rs comparisons/bough-workloads/Cargo.toml comparisons/bough-workloads/src/*.rs; do
    "$ROOT/bench/remote/push.sh" "$B/bough" "${f#$ROOT/}" | grep -v '^ok' || true
  done
}

sync_app() { # app
  [ "$1" = bough ] && (cd "$ROOT" && push_bough)
  local paths="apps/shared"
  [ -d "$HERE/overlay/apps/$1" ] && paths="$paths apps/$1"
  # shellcheck disable=SC2086
  "$HERE/sync.sh" $paths >/dev/null
}

build_script() { # app
  cat > "$T/b.sh" <<EOS
source $B/gb-env.sh
cd $GB/apps/$1 || exit 1
t0=\$(date +%s)
build cargo build --release -j6 2>&1 | tee $B/logs/build-$1.log | grep -E '^(error|warning)(\\[|:)' -A12 | head -150
echo "build \${PIPESTATUS[0]} in \$(( \$(date +%s) - t0 )) s: \$(ls -la target/release/$1-bench 2>/dev/null | awk '{print \$5, \$6, \$7, \$8}')"
EOS
}

cell_script() { # app scene rows
  local rows=$3
  cat > "$T/c.sh" <<EOS
source $B/gb-env.sh
cd $GB
out=$B/results/dev/$1-$2-$rows
rm -rf \$out
nice -n 10 runner/target/release/gui-bench --quick --scenarios $2 --frameworks $1 --rows $rows --out \$out > $B/logs/cell-$1-$2.log 2>&1
tail -5 $B/logs/cell-$1-$2.log
d=\$(dirname \$(find \$out -name cell.json | head -1))
echo "== cell.json"; cat \$d/cell.json | python3 -c 'import json,sys; c=json.load(sys.stdin); print("done", c["done"]); print("errors", c["errors"]); print("skipped", c["skipped"])'
echo "== reports"
for f in \$d/*.json; do
  n=\$(basename \$f .json); case \$n in cell|startup) ;; *) python3 - \$f <<'PY'
import json, sys
r = json.load(open(sys.argv[1]))
name = sys.argv[1].rsplit('/', 1)[1][:-5]
v = r.get('validity') or {}
bad = [c['name'] for c in v.get('checks', []) if c.get('status') == 'fail']
out = []
for sec in r.get('sections', []):
    for m in sec.get('metrics', []):
        if any(k in m['name'] for k in ('total_p50', 'input_to_commit_p50', 'cpu_ms', 'rss_max', 'frames', 'cpu_ms_per_s', 'dropped', 'p95')):
            out.append(f"{sec['name']}.{m['name']}={m.get('median', 0):.4g}{m.get('unit', '')}")
print(f"{name}: valid={v.get('valid')} failed_checks={bad} " + '; '.join(out[:12]))
PY
  ;; esac
done
echo "== app.log (tail)"; tail -15 \$d/app.log
echo "SHOT \$d/screenshot.png"
EOS
}

fetch_shot() { # remote-png local-png
  printf 'base64 -w 0 %s; echo\n' "$1" > "$T/f.sh"
  "$ROOT/bench/remote/rx.sh" "$T/f.sh" 30 | tr -d '\r\n ' | base64 -d > "$2" 2>/dev/null && echo "screenshot: $2 ($(wc -c < "$2") bytes)"
}

do_cell() { # app scene [rows]
  local rows=${3:-}
  if [ -z "$rows" ]; then case $2 in sheet) rows=1000 ;; canvas) rows=1000 ;; *) rows=100 ;; esac; fi
  cell_script "$1" "$2" "$rows"
  out=$(run_bg "cell-$1-$2" "$T/c.sh" 450)
  echo "$out"
  shot=$(echo "$out" | grep '^SHOT ' | tail -1 | cut -d' ' -f2)
  [ -n "$shot" ] && fetch_shot "$shot" "$SHOTS/$1-$2-$rows.png"
}

case $cmd in
  build) sync_app "$1"; build_script "$1"; run_bg "build-$1" "$T/b.sh" 600 ;;
  cell) do_cell "$@" ;;
  dev) sync_app "$1"; build_script "$1"; run_bg "build-$1" "$T/b.sh" 600; do_cell "$@" ;;
  runner)
    "$HERE/sync.sh" runner apps/shared >/dev/null
    printf 'source %s/gb-env.sh\ncd %s/runner && build cargo build --release -j6 2>&1 | grep -E "^(error|warning)" -A8 | head -60; ls -la target/release/gui-bench\n' "$B" "$GB" > "$T/r.sh"
    run_bg runner "$T/r.sh" 400 ;;
  log) printf 'tail -60 %s/logs/build-%s.log\n' "$B" "$1" > "$T/l.sh"; "$ROOT/bench/remote/rx.sh" "$T/l.sh" 20 ;;
  *) echo "remote.sh: unknown command $cmd" >&2; exit 2 ;;
esac
