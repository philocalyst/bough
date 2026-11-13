#!/usr/bin/env bash
# pull-run.sh NAME : pack results/NAME (flat tables, identity, load log, runner log, links, run.json) on ilo-prod, bring it
# back through rex hash-verified, unpack into bench-r8/results/NAME.
set -e
H=$(cd "$(dirname "$0")" && pwd); R=$(dirname "$H")
N=$1
SP=$(mktemp -d)
cat > $SP/p.sh <<EOS
cd /root/work/r8/bench/results/$N
rm -f /root/work/r8/bench/xfer/$N-flat.tgz
tar czf /root/work/r8/bench/xfer/$N-flat.tgz IDENTITY.txt links.txt load.log runner.log flat run/run.json
ls -la /root/work/r8/bench/xfer/$N-flat.tgz
EOS
"$H/rx.sh" $SP/p.sh 60
mkdir -p "$R/results/$N"
"$H/fetch.sh" /root/work/r8/bench/xfer/$N-flat.tgz "$R/results/$N/$N-flat.tgz" 300
(cd "$R/results/$N" && tar xzf $N-flat.tgz)
rm -rf $SP
ls "$R/results/$N"
