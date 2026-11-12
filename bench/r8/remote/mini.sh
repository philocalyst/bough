#!/usr/bin/env bash
# mini.sh OUT.tar.gz CRATE... : pack a small workspace of the named crates (crates/<name>), the perf scripts and
# the toolchain pin, for a host that has no checkout (mothership). The crates must depend only on each other.
# Threads lane, round 8; see docs/lanes/threads/MEASURE.md.
set -e
ROOT=$(cd "$(dirname "$0")/../../.." && pwd)
OUT=$1; shift
T=$(mktemp -d); W=$T/mini
mkdir -p "$W/crates" "$W/bench/r8"
for c in "$@"; do cp -R "$ROOT/crates/$c" "$W/crates/$c"; rm -rf "$W/crates/$c/target"; done
cp -R "$ROOT/bench/r8/perf" "$W/bench/r8/perf"
cp "$ROOT/rust-toolchain.toml" "$W/"
{
  echo '[workspace]'
  echo 'resolver = "3"'
  printf 'members = ['; for c in "$@"; do printf '"crates/%s", ' "$c"; done; echo ']'
  # the package table and the two profiles every number is taken at, as the root manifest has them
  awk '/^\[workspace.package\]/{p=1} /^\[/{if($0!~/workspace.package/)p=0} p' "$ROOT/Cargo.toml"
  awk '/^\[profile.release\]/{p=1} /^\[profile.dist\]/{p=1} /^\[/{if($0!~/profile.(release|dist)\]/)p=0} p' "$ROOT/Cargo.toml" | grep -v '^#'
} > "$W/Cargo.toml"
echo "commit=$(git -C "$ROOT" rev-parse --short=10 HEAD)$(git -C "$ROOT" status --porcelain --untracked-files=no | head -1 | sed 's/.*/ dirty/')" > "$W/MINI"
COPYFILE_DISABLE=1 tar --no-xattrs -czf "$OUT" -C "$T" mini 2>/dev/null || COPYFILE_DISABLE=1 tar -czf "$OUT" -C "$T" mini
rm -rf "$T"
ls -la "$OUT"
