#!/usr/bin/env bash
# xfer.sh LOCAL_FILE REMOTE_PATH : copy a file to the remote host through rex, verified by sha256.
#   Same environment as rx.sh (REX_HOST, REX_SESSION_ID). Prints both hashes; exits 1 if they differ.
# rex refuses a control message over 64 KiB, so the file goes as base64 in 63,000-byte chunks, each appended by
# one `rex run --wait`. Measured from the Mac to ilo-prod: ~16 KB/s (a 1.0 MB git bundle in 66 s).
#
# Source recipe (what the bench lane uses; the remote needs one commit you also have, e.g. main):
#   git bundle create /tmp/x.bundle <branch> --not <commit-the-remote-has>   # incremental: KBs to a few MB
#   bench/remote/xfer.sh /tmp/x.bundle /root/work/r7/<lane>/xfer/x.bundle
#   remote: git fetch ../xfer/x.bundle <branch>:<branch> && git checkout <branch>   (or git bundle verify first)
# A first copy without any shared commit: `git clone` from a checkout already on the host, then fetch the bundle.
set -e
export REX_SESSION_ID=${REX_SESSION_ID:-session:01a1ccd3-828f-72bc-8d3d-490dde484869}
H=${REX_HOST:-ilo-prod}
S=${REX_SESSION_ID:?set REX_SESSION_ID to the rex session on $H}
F=$1; R=$2
L=$(shasum -a 256 "$F" | cut -d' ' -f1)
T=$(mktemp -d)
base64 < "$F" | tr -d '\n' | split -b 63000 - "$T/c."
rex -S "$H" run -s "$S" --focus=false --wait -- bash -c "mkdir -p \$(dirname $R); : > $R.b64" >/dev/null
n=0
for c in "$T"/c.*; do
  n=$((n+1))
  rex -S "$H" run -s "$S" --focus=false --wait -- bash -c "printf %s $(cat "$c") >> $R.b64" >/dev/null
done
printf 'base64 -d %s.b64 > %s && rm %s.b64\necho "remote $(sha256sum %s | cut -d" " -f1)"\n' "$R" "$R" "$R" "$R" > "$T/v.sh"
out=$("$(dirname "$0")/rx.sh" "$T/v.sh" 30)
rm -rf "$T"
echo "sent $n chunks"; echo "$out"; echo "local  $L"
echo "$out" | grep -q "remote $L" || { echo "HASH MISMATCH"; exit 1; }
