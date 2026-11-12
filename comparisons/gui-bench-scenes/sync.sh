#!/usr/bin/env bash
# sync.sh [PATH...] : copy the overlay (or only the named overlay-relative paths) over the gui-bench checkout on
# the remote host, through rex (bench/remote/xfer.sh), and list what changed there.
#   REX_SESSION_ID as bench/remote; GB (default /root/work/r7/bench/gb) is the remote gui-bench checkout.
set -e
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
GB=${GB:-/root/work/r7/bench/gb}
T=$(mktemp -d)
# no macOS metadata, owner root on the remote
( cd "$HERE/overlay" && export COPYFILE_DISABLE=1 && if [ $# -gt 0 ]; then tar --no-xattrs --uid 0 --gid 0 -czf "$T/o.tgz" "$@"; else tar --no-xattrs --uid 0 --gid 0 -czf "$T/o.tgz" .; fi )
"$ROOT/bench/remote/xfer.sh" "$T/o.tgz" "$GB/../xfer/overlay.tgz" | tail -1
printf 'cd %s && tar --no-same-owner -xzf ../xfer/overlay.tgz && git status --short | head -40\n' "$GB" > "$T/x.sh"
"$ROOT/bench/remote/rx.sh" "$T/x.sh" 20
rm -rf "$T"
