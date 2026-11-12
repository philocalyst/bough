#!/bin/bash
# usage: rx.sh 'shell command'   -- runs on ilo-prod in session cmp-r6, prints captured output
cmd="$1"
out=$(rex -S ilo-prod run -s cmp-r6 --label cmp --wait --keep-open --focus=false --json -- bash -c "$cmd" 2>&1) || { echo "$out"; exit 1; }
blk=$(echo "$out" | python3 -c 'import sys,json;print(json.load(sys.stdin)["block_ids"][0])')
win=$(echo "$out" | python3 -c 'import sys,json;print(json.load(sys.stdin)["window_id"])')
rex -S ilo-prod capture -s cmp-r6 -b "$blk" --trim --unwrap 2>&1
rex -S ilo-prod window close -s cmp-r6 "$win" >/dev/null 2>&1
