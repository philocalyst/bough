source /root/work/r8/bench/env.sh
B=/root/work/r8/bench
cd $B/bough-scenes
grep -q 'pub mod pmu' crates/bough-mem/src/lib.rs || sed -i 's/^pub mod count;/pub mod count;\npub mod pmu;/' crates/bough-mem/src/lib.rs
git status --short; git diff --stat
cd $B/app-scenes && time CARGO_TARGET_DIR=$B/target-app-scenes build cargo build --release -j8 2>&1 | grep -v '^warning\|^ *|\|^ *=\|^$' | tail -40
cp $B/target-app-scenes/release/bough-bench $B/bin/scenes/release/ && ls -la $B/bin/scenes/release/
