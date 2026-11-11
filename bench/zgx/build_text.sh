#!/bin/sh
# zgx's text workloads (release, as scripts/perf/suite.py builds them) from a shared clone of zgx 06debe9d, plus
# bough's drivers (bench/zgx/*.rs: font_probe, text_bench_dejavu) copied into the clone's examples. Binaries
# land in $OUT (default target/text3-zgxbin). The zgx checkout itself is never touched.
set -eu
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
ZGX=${ZGX:-${BOUGH_REF:-$HOME/projects/_ref}/zgx}
COPY=${ZGX_COPY:-$(git rev-parse --show-toplevel)/target/text3-zgx}
OUT=${OUT:-$(git rev-parse --show-toplevel)/target/text3-zgxbin}
[ -d "$COPY/.git" ] || git clone --shared --quiet "$ZGX" "$COPY"
git -C "$COPY" checkout --quiet 06debe9d
cp "$ROOT/bench/zgx/font_probe.rs" "$ROOT/bench/zgx/text_bench_dejavu.rs" "$COPY/crates/zgx-gpu/examples/"
export CARGO_TARGET_DIR="$COPY/target" CARGO_INCREMENTAL=0 RUSTC_WRAPPER=
cd "$COPY"
mkdir -p "$OUT"
cargo build --release --locked --quiet -p zgx-gpu --example text_bench --example text_bench_dejavu --example font_probe -p zgx --example editor_streaming
cp target/release/examples/text_bench target/release/examples/text_bench_dejavu target/release/examples/font_probe target/release/examples/editor_streaming "$OUT/"
cargo build --release --locked --quiet -p zgx --example i18n_bench && cp target/release/examples/i18n_bench "$OUT/i18n_bench_zgx"
cargo build --release --locked --quiet -p zgx-script --example code_editor_bench --example i18n_bench
cp target/release/examples/i18n_bench "$OUT/i18n_bench_script"; cp target/release/examples/code_editor_bench "$OUT/"
echo "built into $OUT"
