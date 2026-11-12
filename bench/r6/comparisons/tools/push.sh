#!/bin/bash
# usage: push.sh localdir remotedir   -- copies localdir contents to ilo-prod:remotedir via rex (tar | base64 chunks)
set -e
src="$1"; dst="$2"
tmp=$(mktemp -d)
COPYFILE_DISABLE=1 tar -C "$src" --exclude target --exclude '.git' -czf - . | base64 | tr -d '\n' > "$tmp/all.b64"
size=$(wc -c < "$tmp/all.b64")
echo "payload $size base64 bytes"
HERE=$(dirname "$0")
"$HERE/rx.sh" "mkdir -p '$dst' && rm -f /root/work/cmp/.push.b64" >/dev/null
off=0; chunk=90000
while [ $off -lt $size ]; do
  part=$(dd if="$tmp/all.b64" bs=1 skip=$off count=$chunk 2>/dev/null)
  "$HERE/rx.sh" "printf '%s' '$part' >> /root/work/cmp/.push.b64" >/dev/null
  off=$((off+chunk))
done
"$HERE/rx.sh" "base64 -d /root/work/cmp/.push.b64 | tar -C '$dst' -xzf - && echo extracted && rm -f /root/work/cmp/.push.b64"
rm -rf "$tmp"
