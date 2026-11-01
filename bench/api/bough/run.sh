#!/bin/bash
# bough's ports of zgx's suite scenes, measured as zgx's suite.py does (least of 3 under /usr/bin/time -l).
# Build first:  CARGO_TARGET_DIR=/Users/mileswirht/projects/bough/target/api cargo build --profile dist -p bough-suite
set -u
export ZGX_FONT_FILE=/Users/mileswirht/projects/_ref/zgx/comparisons/fonts/DejaVuSans.ttf
D=${D:-/Users/mileswirht/projects/bough/target/api/dist}
M="python3 $(dirname "$0")/measure.py"
rm -f "$(dirname "$0")/results.jsonl"
$M mount $D/mount_bench
$M mount-N10000 $D/mount_bench N=10000
$M teardown $D/teardown_bench
$M teardown-N2000 $D/teardown_bench N=2000 REPEAT=1
$M teardown-N10000 $D/teardown_bench N=10000 REPEAT=1
$M teardown-N30000 $D/teardown_bench N=30000 REPEAT=1
$M script-update $D/update_bench
for s in template function-prop rows rows-in-place handler; do $M script-update-$s $D/update_bench ONLY=$s; done
$M anime $D/anime_bench
$M anime-wires $D/anime_bench ONLY=wires
$M anime-lanes $D/anime_bench ONLY=lanes
$M anime-wires-F1200 $D/anime_bench ONLY=wires FRAMES=1200
$M anime-lanes-F1200 $D/anime_bench ONLY=lanes FRAMES=1200
$M style $D/style_bench
$M style-F1200 $D/style_bench FRAMES=1200
$M style-labels $D/style_bench -- labels
$M script-layout $D/script_layout_bench
$M list-state $D/list_state_bench
$M list-state-T400 $D/list_state_bench TICKS=400
$M i18n $D/i18n_bench
$M i18n-template $D/i18n_bench -- template
$M variable-list $D/variable_list_bench
$M core $D/core_bench
$M startup $D/startup_bench
$M stream-memory $D/stream_memory
$M text $D/text_bench
