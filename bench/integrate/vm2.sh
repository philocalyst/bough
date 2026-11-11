#!/bin/zsh
# usage: vm2.sh <binary> <sample-at-seconds> [env...] : footprint/IOSurface/owned at t, plus the process's stderr
BIN=$1; AT=$2; shift; shift
env "$@" $BIN > /tmp/vm2.out 2> /tmp/vm2.err &
P=$!
sleep $AT
vmmap --summary $P 2>/dev/null | grep -E "Physical footprint:|^IOSurface|owned unmapped|IOAccelerator \(graphics\)" | sed 's/  */ /g' | tr '\n' '|'
echo
wait $P
grep -E "surfaces|blink|animation" /tmp/vm2.err /tmp/vm2.out
