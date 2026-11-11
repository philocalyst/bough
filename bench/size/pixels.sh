#!/bin/sh
# The size lane's pixel gate (G1) on binaries built with a candidate configuration:
#   bench/size/pixels.sh <base dir> <candidate dir> [scratch dir]
# Each dir holds heavy_shot, scene_shot, vlist_shot and counter_gate built with one configuration. Every case must
# report 0 differing bytes against a full redraw (heavy also against a fresh mount) in BOTH builds, and the
# candidate's presented frame must be byte-identical to the base's. Prints one line per case; exits 1 on any
# failure. ZGX_FONT_FILE as the slope scripts (zgx's DejaVuSans.ttf).
A=$1; B=$2; S=${3:-${TMPDIR:-/tmp}/bough-size-pixels.$$}
: "${ZGX_FONT_FILE:=$HOME/projects/_ref/zgx/comparisons/fonts/DejaVuSans.ttf}"; export ZGX_FONT_FILE
mkdir -p "$S" || exit 2
fail=0
# case <name> <binary> <env...>: run in A and B, check the diff line, compare the dumps
case_() {
  name=$1; bin=$2; shift 2
  for side in A B; do
    d=$A; [ $side = B ] && d=$B
    env "$@" OUT="$S/$name.$side.bgra" "$d/$bin" > "$S/$name.$side.log" 2>&1
    echo $? > "$S/$name.$side.rc"
  done
  la=$(grep -E 'differing bytes' "$S/$name.A.log" | tail -1); lb=$(grep -E 'differing bytes' "$S/$name.B.log" | tail -1)
  ok=yes
  case "$la" in *"differing bytes vs full redraw: 0"*|*" 0 differing bytes"*) ;; *) ok=no;; esac
  case "$lb" in *"differing bytes vs full redraw: 0"*|*" 0 differing bytes"*) ;; *) ok=no;; esac
  [ "$(cat "$S/$name.B.rc")" = 0 ] || ok=no
  if cmp -s "$S/$name.A.bgra" "$S/$name.B.bgra"; then same=identical; else same=DIFFERENT; ok=no; fi
  [ $ok = yes ] || fail=1
  echo "$ok  $name  frames $same  | A: $la | B: $lb"
  rm -f "$S/$name.A.bgra" "$S/$name.B.bgra"
}
case_ heavy-stream heavy_shot ZGX_MODE=stream
case_ heavy-scroll heavy_shot ZGX_MODE=scroll
case_ heavy-both heavy_shot ZGX_MODE=both
case_ heavy-both-fresh heavy_shot ZGX_MODE=both FRESH=1
case_ lab-both scene_shot SCENE=lab ZGX_MODE=both
case_ animate scene_shot SCENE=animate
case_ vlist-scroll vlist_shot ZGX_MODE=scroll
case_ vlist-fling vlist_shot ZGX_MODE=fling
case_ vlist-jump vlist_shot ZGX_MODE=jump
case_ counter counter_gate
[ $fail = 0 ] && echo "G1 PASS" || echo "G1 FAIL"
exit $fail
