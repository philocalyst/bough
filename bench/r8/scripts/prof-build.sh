# profile builds (run after every timing run is finished): the headless driver of comparisons/bough-guibench,
# same source and profile as the app the matrix ran (release, debug=1 line tables, thin LTO) at bough 74e665786c.
source /root/work/r8/bench/env.sh
B=/root/work/r8/bench
build() { taskset -c 0-7 nice -n 10 "$@"; }
set -x
date -u; git -C $B/bough rev-parse HEAD
cd $B/bough/comparisons/bough-guibench
time CARGO_TARGET_DIR=$B/target-headless build cargo build --release --locked --offline -j6 --bin headless 2>&1 | tail -15
ls -la $B/target-headless/release/headless && sha256sum $B/target-headless/release/headless
git -C $B/bough status --short | head -5
date -u
