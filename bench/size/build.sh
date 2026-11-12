#!/bin/sh
# Build one size-lane configuration and copy its executables into <out>:
#   bench/size/build.sh <out> [counter|apps|gate|all]
# counter: the counter example; apps: counter + the AWAGY scroll and dashboard apps; gate: the pixel-gate and
# hot-cell binaries (heavy_headless heavy_shot lab_headless animate_headless scene_shot vlist_headless vlist_shot
# counter_gate); all: apps + gate.
# Configuration (environment):
#   PROFILE   cargo profile (default production)
#   CONFIG    extra `--config` arguments, space separated (e.g. profile overrides: CONFIG="profile.x.opt-level='s'")
#   FLAGS     rustflags appended to the root .cargo/config.toml's (v0 mangling, --cfg tokio_unstable), which RUSTFLAGS
#             would otherwise replace
#   STD       1: rebuild std (-Z build-std=std,panic_abort --target aarch64-apple-darwin) with STD_FEATURES:
#             "default" (std's own default set: backtrace, panic-unwind), "none" (no features), or a list
#             (e.g. optimize_for_size). The panic strategy is the profile's (CONFIG profile.<p>.panic=...;
#             "immediate-abort" needs ZFLAGS=-Zpanic-immediate-abort)
#   ZFLAGS    extra cargo -Z flags
#   FEATURES  extra cargo features for every package built (e.g. bough/sabotage_copy)
#   NOCX      1: without bough's default `complex-text` feature (no rustybuzz): --no-default-features everywhere
#   MAP       a directory: also link an unstripped twin of the counter (and apps with SET apps/all) with an ld64 map
# Uses the caller's CARGO_TARGET_DIR (the worktree's target). Prints each executable's size.
set -e
OUT=$1; SET=${2:-counter}
[ -n "$OUT" ] || { echo "usage: build.sh <out> [counter|apps|gate|all]"; exit 2; }
mkdir -p "$OUT"
PROFILE=${PROFILE:-production}
T=${CARGO_TARGET_DIR:?set CARGO_TARGET_DIR}
base="-C symbol-mangling-version=v0 --cfg tokio_unstable"
export RUSTFLAGS="$base ${FLAGS:-}"
args="--profile $PROFILE"
for c in ${CONFIG:-}; do args="$args --config $c"; done
for z in ${ZFLAGS:-}; do args="$args $z"; done
dir=$T/$PROFILE
if [ "${STD:-0}" = 1 ]; then
  sf=${STD_FEATURES:-default}
  args="$args -Z build-std=std,panic_abort --target aarch64-apple-darwin"
  case "$sf" in default) ;; none) args="$args -Z build-std-features=";; *) args="$args -Z build-std-features=$sf";; esac
  dir=$T/aarch64-apple-darwin/$PROFILE
fi
feat=""; [ -n "${FEATURES:-}" ] && feat="--features $FEATURES"
nd=""; [ "${NOCX:-0}" = 1 ] && nd="--no-default-features"
log=$OUT/build.log; : > "$log"
echo "RUSTFLAGS=$RUSTFLAGS cargo build $args $feat" >> "$log"
b() { cargo build $args "$@" >> "$log" 2>&1 || { tail -30 "$log"; exit 1; }; }
m() { # m <name> <cargo rustc target args...>: unstripped twin with a link map
  [ -n "${MAP:-}" ] || return 0
  mkdir -p "$MAP"; n=$1; shift
  cargo rustc $args "$@" -- -C strip=none -C link-arg=-Wl,-map,"$MAP/$n.map" >> "$log" 2>&1 || { tail -30 "$log"; exit 1; }
}
if [ "$SET" = counter ] || [ "$SET" = apps ] || [ "$SET" = all ]; then
  b -p bough --example counter $nd $feat
  cp "$dir/examples/counter" "$OUT/counter"
  if [ -n "${MAP:-}" ]; then m counter -p bough --example counter $nd $feat; cp "$dir/examples/counter" "$MAP/counter.unstripped"; fi
fi
if [ "$SET" = apps ] || [ "$SET" = all ]; then
  sfeat="$feat"; [ -n "$nd" ] && sfeat="$nd --features scroll${FEATURES:+,$FEATURES}"
  b -p bough-awagy $sfeat
  cp "$dir/bough-awagy" "$OUT/awagy-scroll"
  if [ -n "${MAP:-}" ]; then m awagy-scroll -p bough-awagy --bin bough-awagy $sfeat; cp "$dir/bough-awagy" "$MAP/awagy-scroll.unstripped"; fi
  dfeat="--no-default-features --features dashboard"; [ -z "$nd" ] && dfeat="$dfeat,complex-text"; [ -n "${FEATURES:-}" ] && dfeat="$dfeat,$FEATURES"
  b -p bough-awagy $dfeat
  cp "$dir/bough-awagy" "$OUT/awagy-dashboard"
  if [ -n "${MAP:-}" ]; then m awagy-dashboard -p bough-awagy --bin bough-awagy $dfeat; cp "$dir/bough-awagy" "$MAP/awagy-dashboard.unstripped"; fi
fi
if [ "$SET" = gate ] || [ "$SET" = all ]; then
  b -p bough-heavy -p bough-scenes -p bough-vlist --bin heavy_headless --bin heavy_shot --bin lab_headless \
    --bin animate_headless --bin scene_shot --bin vlist_headless --bin vlist_shot $nd $feat
  for x in heavy_headless heavy_shot lab_headless animate_headless scene_shot vlist_headless vlist_shot; do cp "$dir/$x" "$OUT/$x"; done
  b -p bough --example counter_gate $nd $feat
  cp "$dir/examples/counter_gate" "$OUT/counter_gate"
fi
for f in "$OUT"/*; do [ -x "$f" ] && [ -f "$f" ] && printf '%10d  %s\n' "$(/usr/bin/stat -f %z "$f")" "$(basename "$f")"; done
exit 0
