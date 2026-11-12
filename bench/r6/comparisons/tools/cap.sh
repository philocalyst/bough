#!/bin/bash
# usage: cap.sh block-id [tail-lines]
rex -S ilo-prod capture -s cmp-r6 -b "$1" --trim --unwrap 2>&1 | tail -n ${2:-40}
