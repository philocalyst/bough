#!/bin/sh
# The site's binary-size measurement (apps/sizes.py) on macOS arm64: each scroll app built with its production
# profile (LTO, 1 codegen unit, stripped), the executable's size in bytes -> sizes.json.
W=$1; R=$W/src/repo
BOUGH_ROOT=${BOUGH_ROOT:-$(cd "$(dirname "$0")/../../.." && pwd)}   # this checkout; override for another
export RUSTUP_HOME=$HOME/.local/share/rustup CARGO_HOME=$HOME/.local/share/cargo RUSTC_WRAPPER= PATH=$HOME/.local/share/cargo/bin:/usr/bin:/bin:/usr/sbin:/sbin
out=$W/sizes.tsv; : > $out
for app in vizia vizia-optimized; do
  extra=""; [ "$app" = gpui ] && extra=",gpui/runtime_shaders"
  if cargo +1.97.1 build --profile production --locked --manifest-path "$R/apps/$app/Cargo.toml" --target-dir "$W/targets/$app" --no-default-features --features "scroll$extra" > "$W/logs/size-$app.log" 2>&1; then
    exe=$(find "$W/targets/$app/production" -maxdepth 1 -type f -perm -111 | head -1); echo "$app	$(stat -f %z "$exe")" >> $out
  else echo "$app FAILED"; fi
done
cd "$BOUGH_ROOT" && CARGO_TARGET_DIR="$BOUGH_ROOT/target/awagy" cargo build --profile production -p bough-awagy > "$W/logs/size-bough.log" 2>&1 && echo "bough	$(stat -f %z target/awagy/production/bough-awagy)" >> $out
python3 -c "import json,sys; print(json.dumps({l.split()[0]: int(l.split()[1]) for l in open('$out') if l.strip()}))" > $W/sizes.json; cat $W/sizes.json
