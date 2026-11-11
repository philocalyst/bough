#!/bin/bash
# Part 1: prebuilt zgx release examples (zgx 06debe9d), measured via measure.py.
export RUSTC_WRAPPER= PATH=$HOME/.local/share/cargo/bin:$PATH
export ZGX_FONT_FILE=${BOUGH_REF:-$HOME/projects/_ref}/zgx/comparisons/fonts/DejaVuSans.ttf
M=$(git rev-parse --show-toplevel)/bench/api/zgx/measure.py
EX=${BOUGH_REF:-$HOME/projects/_ref}/zgx/target/release/examples
m() { python3 "$M" "$@"; }

m mount "$EX/mount_bench"
m mount-rust "$EX/mount_bench" ONLY=rust
m mount-luau "$EX/mount_bench" ONLY=luau
m mount-rust-N10000 "$EX/mount_bench" ONLY=rust N=10000

m script-update "$EX/update_bench"
for s in template function-prop rows rows-in-place handler; do
  m "script-update-$s" "$EX/update_bench" ONLY=$s
done

m anime "$EX/anime_bench"
for v in luau rust direct; do m "anime-$v" "$EX/anime_bench" ONLY=$v; done
for v in rust direct; do m "anime-$v-F1200" "$EX/anime_bench" ONLY=$v FRAMES=1200; done

m script-layout "$EX/script_layout_bench"
m list-state "$EX/list_state_bench"
m list-state-T400 "$EX/list_state_bench" TICKS=400
m state "$EX/state_bench"
m layout "$EX/layout_bench"
m frame "$EX/frame_bench"
m text "$EX/text_bench"
m core "$EX/core_bench"
m style "$EX/style_bench"
m style-F1200 "$EX/style_bench" FRAMES=1200
m style-labels "$EX/style_bench" -- labels
m widgets "$EX/widget_bench"
m app-mail "$EX/apps_bench" APP=mail
m app-dashboard "$EX/apps_bench" APP=dashboard REPS=5
m app-sketch "$EX/apps_bench" APP=sketch
m app-code "$EX/apps_bench" APP=code
m app-dashboard-R0 "$EX/apps_bench" APP=dashboard REPS=5 RENDER=0
m app-mail-R0 "$EX/apps_bench" APP=mail RENDER=0
echo PART1 DONE
