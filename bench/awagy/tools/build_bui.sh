#!/bin/sh
# Build the BUI (blenderui) apps with --profile production (dependencies pinned by git rev in apps/bui/Cargo.toml; needs cmake on PATH).
# usage: build_bui.sh <W> [scroll|dashboard]...
W=$1; shift
A=$(cd "$(dirname "$0")/../apps/bui" && pwd)  # the apps live in the repo (bench/awagy/apps)
export RUSTUP_HOME=$HOME/.local/share/rustup CARGO_HOME=$HOME/.local/share/cargo RUSTC_WRAPPER= CARGO_TARGET_DIR=$W/targets/bui PATH=${CMAKE_BIN:+$CMAKE_BIN:}$HOME/.local/share/cargo/bin:/usr/bin:/bin:/usr/sbin:/sbin
mkdir -p $W/bins $W/logs
cd $A
for bench in ${@:-scroll dashboard}; do
  start=$(date +%s)
  cargo +1.97.1 build --profile production --manifest-path Cargo.toml --no-default-features --features $bench > $W/logs/build-bui-$bench-production.log 2>&1 \
    && cp $W/targets/bui/production/bui-scroll $W/bins/bui-$bench && echo "$bench OK $(( $(date +%s) - start ))s $(stat -f %z $W/bins/bui-$bench)" \
    || { echo "$bench FAILED"; tail -n 30 $W/logs/build-bui-$bench-production.log; }
done
