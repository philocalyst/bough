#!/usr/bin/env bash
# push.sh REMOTE_CHECKOUT FILE... : write working-tree files (paths relative to the repository root) into a
# checkout on the remote host, for a quick edit-build loop between commits; prints a sha256 check per file.
# Everything travels in one rx.sh script, so the files together must stay under ~40 KiB (use a bundle and
# xfer.sh beyond that). Commit and send a bundle before trusting a remote result: identity records name commits.
set -e
R=$1; shift
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
T=$(mktemp -d)
{
  echo "set -e; cd $R"
  for f in "$@"; do
    h=$(shasum -a 256 "$ROOT/$f" | cut -d' ' -f1)
    echo "mkdir -p \$(dirname $f); echo $(base64 < "$ROOT/$f" | tr -d '\n') | base64 -d > $f"
    echo "[ \"\$(sha256sum $f | cut -d' ' -f1)\" = $h ] && echo 'ok   $f' || echo 'BAD  $f'"
  done
} > "$T/p.sh"
[ "$(wc -c < "$T/p.sh")" -lt 46000 ] || { echo "push: $(wc -c < "$T/p.sh") bytes, too large for one rex message"; rm -rf "$T"; exit 1; }
"$(dirname "$0")/rx.sh" "$T/p.sh" 20
rm -rf "$T"
