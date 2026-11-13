#!/usr/bin/env bash
# put.sh LOCAL... : copy small scripts (each under ~30 KiB) into /root/work/r8/bench/ on ilo-prod, sha256-checked.
H=$(cd "$(dirname "$0")" && pwd)
T=$(mktemp -d)
{ for f in "$@"; do b=$(basename $f); h=$(shasum -a 256 $f | cut -d' ' -f1)
  echo "echo $(base64 < $f | tr -d '\n') | base64 -d > /root/work/r8/bench/$b; [ \"\$(sha256sum /root/work/r8/bench/$b | cut -d' ' -f1)\" = $h ] && echo ok $b || echo BAD $b"; done; } > $T/p.sh
"$H/rx.sh" $T/p.sh 30; rm -rf $T
