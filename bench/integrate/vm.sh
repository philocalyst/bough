#!/bin/zsh
# usage: vm.sh <binary> [env assignments...]  -- run ~7 s, vmmap --summary at 4.5 s, print the lines that matter
BIN=$1; shift
env "$@" $BIN > /dev/null 2>&1 &
sleep 4.5
PID=$(pgrep -n -f "$(basename $BIN)$")
vmmap --summary $PID 2>/dev/null | grep -E "Physical footprint:|^IOSurface|^IOAccelerator|owned unmapped|^MALLOC_(MEDIUM|NANO|SMALL|TINY)  |^__DATA  |^TOTAL  |^VM_ALLOC|^mapped file" | awk '{printf "%s | ", $0} END {print ""}' | sed 's/  */ /g'
wait
