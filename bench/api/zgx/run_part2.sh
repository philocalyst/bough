#!/bin/bash
# Part 2: zgx examples built out-of-tree (CARGO_TARGET_DIR=/Users/mileswirht/projects/bough/target/zgx-api).
export RUSTC_WRAPPER= PATH=$HOME/.local/share/cargo/bin:$PATH
export ZGX_FONT_FILE=/Users/mileswirht/projects/_ref/zgx/comparisons/fonts/DejaVuSans.ttf
M=/Users/mileswirht/projects/bough-api/bench/api/zgx/measure.py
EX=/Users/mileswirht/projects/bough/target/zgx-api/release/examples
m() { python3 "$M" "$@"; }

m teardown "$EX/teardown_bench" REPEAT=1
m teardown-N2000 "$EX/teardown_bench" N=2000 REPEAT=1
m teardown-N10000 "$EX/teardown_bench" N=10000 REPEAT=1
m teardown-N30000 "$EX/teardown_bench" N=30000 REPEAT=1
m variable-list "$EX/variable_list_bench"
m callback-mount "$EX/callback_mount_bench"
m callback-mount-reads "$EX/callback_mount_bench" READS=1
m i18n "$EX/i18n_bench"
m i18n-template "$EX/i18n_bench" -- template
m editor-streaming "$EX/editor_streaming"
m startup "$EX/startup_bench"
m stream-memory "$EX/stream_memory"
echo PART2 DONE
