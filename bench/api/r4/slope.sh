#!/bin/sh
# Instructions per tick by the slope method: least of 3 `/usr/bin/time -l` runs at TICKS=$HI and TICKS=$LO,
# (I(HI) - I(LO)) / (HI - LO). Usage: slope.sh LABEL BINARY [ENV=VAL ...]
label=$1; bin=$2; shift 2
HI=${HI:-1800}; LO=${LO:-600}
least() {
  best=""
  for i in 1 2 3; do
    n=$(env "$@" /usr/bin/time -l "$bin" 2>&1 >/dev/null | awk '/instructions retired/ {print $1}')
    if [ -z "$best" ] || [ "$n" -lt "$best" ]; then best=$n; fi
  done
  echo "$best"
}
a=$(least "$@" TICKS=$HI)
b=$(least "$@" TICKS=$LO)
echo "$label $(echo "($a - $b) / ($HI - $LO)" | bc) instr/tick (I($HI)=$a I($LO)=$b)"
