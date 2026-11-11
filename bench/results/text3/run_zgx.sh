#!/bin/sh
# zgx's text workloads on this Mac (release builds of 06debe9d from bench/zgx/build_text.sh): whole-run
# instructions retired and peak footprint from /usr/bin/time -l, plus each program's own output.
B=${ZGX_BIN:-$(git rev-parse --show-toplevel)/target/text3-zgxbin}
OUT=${1:-/dev/stdout}
run() { name=$1; shift; echo "== $name ($*)"; /usr/bin/time -l "$@" 2>/tmp/zgx_time_$$ | tail -12; grep -E "instructions retired|peak memory footprint|maximum resident" /tmp/zgx_time_$$; }
{
for r in 1 2 3; do run text_bench $B/text_bench; done
for r in 1 2 3; do run editor_streaming $B/editor_streaming; done
for r in 1 2 3; do run i18n_bench_i18n $B/i18n_bench_zgx; run i18n_bench_template $B/i18n_bench_zgx template; done
for m in message template switch; do for r in 1 2; do MODE=$m run i18n_script_$m env MODE=$m $B/i18n_bench_script; done; done
for r in 1 2; do run code_editor_bench $B/code_editor_bench; done
} > $OUT 2>&1
