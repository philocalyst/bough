#!/bin/bash
# usage: bg.sh label 'shell command' -> starts it in a new tab of cmp-r6 on ilo-prod, prints block id
rex -S ilo-prod run -s cmp-r6 --label "$1" --keep-open --focus=false --json -- bash -c "$2" 2>&1 | python3 -c 'import sys,json;d=json.load(sys.stdin);print(d["block_ids"][0], d["window_id"])'
