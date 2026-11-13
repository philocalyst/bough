#!/usr/bin/env bash
# rx.sh SCRIPT [tries] : run a local bash script on the remote host through rex and print its output.
#   REX_HOST (default ilo-prod), REX_SESSION_ID (required: the rex session on that host), RX_CWD (default /root).
# The script travels base64-encoded inside one rex control message, so it must stay under ~45 KiB; for files,
# use xfer.sh. The block is closed once its output is read (`rex kill` takes sessions, not blocks: a helper that
# calls `rex kill -b` leaves every block open, and a rex session holds at most 64 windows). Polls every 2 s up to `tries` times (default 40); a run longer than that keeps going on the
# remote (print RX_TIMEOUT and the block id; capture it later with `rex -S $REX_HOST capture -b <block>`).
export REX_SESSION_ID=${REX_SESSION_ID:-session:01a1ccd3-828f-72bc-8d3d-490dde484869}
H=${REX_HOST:-ilo-prod}
S=${REX_SESSION_ID:?set REX_SESSION_ID to the rex session on $H}
C=${RX_CWD:-/root}
P=$(base64 < "$1" | tr -d '\n')
J=$(rex -S "$H" run -s "$S" --focus=false --keep-open --json --label rx -c "$C" -- bash -c "echo $P | base64 -d > /tmp/rx.\$\$.sh; bash /tmp/rx.\$\$.sh 2>&1 </dev/null; rm -f /tmp/rx.\$\$.sh; echo; echo RX_END")
B=$(echo "$J" | grep -o 'block:[0-9a-f-]*' | head -1)
[ -z "$B" ] && { echo "no block: $J"; exit 1; }
for i in $(seq 1 "${2:-40}"); do
  out=$(rex -S "$H" capture -s "$S" -b "$B" --trim --unwrap 2>&1)
  if echo "$out" | grep -q '^RX_END'; then echo "$out" | sed '/^RX_END/,$d'; rex -S "$H" block close -s "$S" "$B" >/dev/null 2>&1; exit 0; fi
  sleep 2
done
echo "$out" | tail -20; echo "RX_TIMEOUT block=$B"
