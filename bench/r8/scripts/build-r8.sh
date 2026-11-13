# builds for bench-r8: runner, bough plain app, bough scenes app (nothing is timed while this runs)
source /root/work/r8/bench/env.sh
B=/root/work/r8/bench
set -x
cd $B
if [ ! -d app-plain ]; then
  mkdir -p app-plain app-scenes
  cp -a /root/work/r7/platform/gui-bench/apps/bough/{Cargo.toml,Cargo.lock,rust-toolchain.toml,src} app-plain/
  cp -a gb/apps/bough/{Cargo.toml,Cargo.lock,rust-toolchain.toml,src} app-scenes/
  sed -i 's|\.\./\.\./\.\./bough/|../bough/|g' app-plain/Cargo.toml
  sed -i 's|\.\./\.\./\.\./bough/|../bough-scenes/|g' app-scenes/Cargo.toml
  rm -rf gb/apps/bough
  mkdir -p bin/plain/release bin/scenes/release gb/apps/bough
  ln -s $B/bin/plain gb/apps/bough/target
fi
date -u
cd $B/gb/runner && time build cargo build --release -j8 2>&1 | tail -15
ls -la target/release/gui-bench
cd $B/app-plain && time CARGO_TARGET_DIR=$B/target-app-plain build cargo build --release -j8 2>&1 | tail -15
cp target-app-plain/release/bough-bench $B/bin/plain/release/ 2>/dev/null || cp $B/target-app-plain/release/bough-bench $B/bin/plain/release/
cd $B/app-scenes && time CARGO_TARGET_DIR=$B/target-app-scenes build cargo build --release -j8 2>&1 | tail -30
cp $B/target-app-scenes/release/bough-bench $B/bin/scenes/release/
ls -la $B/bin/*/release/
date -u
