#!/usr/bin/env bash
# bg.sh LOG SCRIPT : start a local bash script on the remote host as its own rex block (label "bg"), its output
# appended to LOG on the remote, followed by "EXIT <status>". Returns at once and prints the block id.
# Long builds and runs go here: a process started with nohup/setsid from an rx.sh script can die when rx.sh kills
# its block. Poll with `rx.sh` (e.g. `tail LOG`); the block closes itself when the script ends.
export REX_SESSION_ID=${REX_SESSION_ID:-session:01a1ccd3-828f-72bc-8d3d-490dde484869}
H=${REX_HOST:-ilo-prod}
S=${REX_SESSION_ID:?set REX_SESSION_ID to the rex session on $H}
L=$1
P=$(base64 < "$2" | tr -d '\n')
rex -S "$H" run -s "$S" --focus=false --json --label bg -- bash -c "echo $P | base64 -d > /tmp/bg.\$\$.sh; mkdir -p \$(dirname $L); bash /tmp/bg.\$\$.sh >> $L 2>&1 </dev/null; echo EXIT \$? >> $L; rm -f /tmp/bg.\$\$.sh" | grep -o 'block:[0-9a-f-]*' | head -1
