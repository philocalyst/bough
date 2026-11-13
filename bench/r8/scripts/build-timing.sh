source /root/work/r8/bench/env.sh
B=/root/work/r8/bench
build() { taskset -c 0-7 nice -n 10 "$@"; }
set -x
cd $B/gb && git diff --stat | tail -3; cp runner/src/cell.rs $B/xfer/cell.rs.before-timing
python3 $B/patch-timing.py
git diff runner/src/cell.rs > $B/xfer/runner-cell-all-diffs.patch; diff -u $B/xfer/cell.rs.before-timing runner/src/cell.rs > $B/xfer/gb-timing.patch; cat $B/xfer/gb-timing.patch | head -40
[ -d $B/target-runner-timing ] || cp -al runner/target $B/target-runner-timing
cd runner && time CARGO_TARGET_DIR=$B/target-runner-timing build cargo build --release --offline -j6 2>&1 | tail -8
ls -la $B/target-runner-timing/release/gui-bench; sha256sum $B/target-runner-timing/release/gui-bench $B/gb/runner/target/release/gui-bench $B/xfer/gb-timing.patch
