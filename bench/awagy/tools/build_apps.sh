#!/bin/sh
# Build the site's apps for both workloads on macOS (release, --locked, the harness's feature selection).
W=$1; shift
R=$W/src/repo
export RUSTUP_HOME=$HOME/.local/share/rustup CARGO_HOME=$HOME/.local/share/cargo RUSTC_WRAPPER= PATH=$HOME/.local/share/cargo/bin:/usr/bin:/bin:/usr/sbin:/sbin
for app in "$@"; do
  for bench in scroll dashboard; do
    T=$W/targets/$app
    start=$(date +%s)
    if cargo +1.97.1 build --release --locked --manifest-path "$R/apps/$app/Cargo.toml" --target-dir "$T" --no-default-features --features "$bench$( [ "$app" = dioxus-native ] && printf ',skia')" > "$W/logs/build-$app-$bench.log" 2>&1; then
      exe=$(find "$T/release" -maxdepth 1 -type f -perm -111 ! -name '*.d' ! -name '*.dylib' | head -1)
      cp "$exe" "$W/bins/$app-$bench" && echo "$app $bench OK $(( $(date +%s) - start ))s $(basename $exe)"
    else
      echo "$app $bench FAILED (see logs/build-$app-$bench.log)"; tail -5 "$W/logs/build-$app-$bench.log"
    fi
  done
done
