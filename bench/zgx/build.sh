#!/bin/sh
# Build zgx's headless drivers (dist profile, the same tree as the measured 06debe9d binaries) into
# $ROOT/target/zgx-headless: workload_headless and animation_headless exist upstream but are not built by
# zgx's dist step; vlist_headless is zgx's windowed vlist_workload loop as a headless example (bench/zgx/
# vlist_headless.rs, added to a clone so the zgx checkout stays untouched).
set -eu
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
ZGX=${ZGX:-${BOUGH_REF:-$HOME/projects/_ref}/zgx}
COPY=${ZGX_COPY:-$ROOT/target/zgxcopy}
[ -d "$COPY/.git" ] || git clone --shared --quiet "$ZGX" "$COPY"
git -C "$COPY" checkout --quiet 06debe9d
cp "$ROOT/bench/zgx/vlist_headless.rs" "$COPY/crates/zgx-desktop/examples/"
(cd "$COPY" && RUSTC_WRAPPER= cargo build --profile dist -p zgx-desktop --example workload_headless --example animation_headless --example vlist_headless)
mkdir -p "$ROOT/target/zgx-headless"
for e in workload_headless animation_headless vlist_headless; do cp "$COPY/target/dist/examples/$e" "$ROOT/target/zgx-headless/"; done
echo "built into $ROOT/target/zgx-headless"
