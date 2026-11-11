#!/bin/sh
# Build the zgx apps with --profile production (dependencies pinned by git rev in apps/zgx/Cargo.toml).
W=$1
A=$(cd "$(dirname "$0")/../apps/zgx" && pwd)  # the apps live in the repo (bench/awagy/apps)
export RUSTUP_HOME=$HOME/.local/share/rustup CARGO_HOME=$HOME/.local/share/cargo RUSTC_WRAPPER= CARGO_TARGET_DIR=$W/targets/zgx PATH=$HOME/.local/share/cargo/bin:/usr/bin:/bin:/usr/sbin:/sbin
mkdir -p $W/bins $W/logs
cd $A
start=$(date +%s)
cargo +1.97.1 build --profile production --manifest-path Cargo.toml > $W/logs/build-zgx-scroll-production.log 2>&1 && cp $W/targets/zgx/production/zgx-scroll $W/bins/zgx-scroll && echo "scroll OK $(( $(date +%s) - start ))s $(stat -f %z $W/bins/zgx-scroll)" || { echo scroll FAILED; tail -n 20 $W/logs/build-zgx-scroll-production.log; exit 1; }
start=$(date +%s)
cargo +1.97.1 build --profile production --manifest-path Cargo.toml --no-default-features --features dashboard > $W/logs/build-zgx-dashboard-production.log 2>&1 && cp $W/targets/zgx/production/zgx-scroll $W/bins/zgx-dashboard && echo "dashboard OK $(( $(date +%s) - start ))s $(stat -f %z $W/bins/zgx-dashboard)" || { echo dashboard FAILED; tail -n 20 $W/logs/build-zgx-dashboard-production.log; }
