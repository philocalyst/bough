#!/usr/bin/env bash
# bough's quick bench: build what it needs (release), measure, print one JSON object and one table.
#
#   bench/quick/run.sh                                  # JSON to stdout, then the table
#   bench/quick/run.sh --out results.json               # JSON to a file
#   bench/quick/run.sh --base bench/quick/baselines/aa9943e7ae.json   # every cell's ratio to a baseline, flags
#   bench/quick/run.sh --self-test --base <file>        # the negative control: inject a known cost, expect flags
#   bench/quick/run.sh --only r0,redescribe --no-build  # a subset (groups: r0 r1 redescribe mount life size smoke)
#
# README.md (next to this file) says what every cell measures and how the quick numbers relate to the full ones.
# Environment as ROUND6.md: rustup's proxies first on PATH, RUSTC_WRAPPER cleared, CARGO_BUILD_JOBS=4 by default,
# CARGO_TARGET_DIR the checkout's own target. BOUGH_REF locates the reference checkouts (the DejaVu font).
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
[ -d "$HOME/.local/share/cargo/bin" ] && export PATH="$HOME/.local/share/cargo/bin:$PATH"
export RUSTC_WRAPPER=
export CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-4}
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$ROOT/target}
case $(cd "$ROOT" && cargo --version 2>&1) in
  *nightly*) ;;
  *) echo "quick: cargo is not the pinned nightly (put rustup's proxies first on PATH)" >&2; exit 2 ;;
esac
exec python3 "$HERE/quick.py" "$@"
