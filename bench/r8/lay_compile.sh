#!/bin/bash
# What one row type costs to compile: user seconds and __text bytes of an example that lays out one row type,
# against one that lays out none. Run from the repository root; profile dist. The library is built first.
#   bench/r8/lay_compile.sh [profile]
P=${1:-dist}
cargo build --profile $P -p bough-lay --example one_none 2>/dev/null
for e in one_none one_sheet one_ide one_chat; do
  touch crates/bough-lay/examples/$e.rs
  t=$( { /usr/bin/time -p cargo build --profile $P -p bough-lay --example $e 2>&1 | grep -E '^user' | awk '{print $2}'; } )
  f=${CARGO_TARGET_DIR:-target}/$P/examples/$e
  if size -m "$f" >/dev/null 2>&1; then text=$(size -m "$f" | awk '/__text/ {print $3; exit}'); else text=$(size -A "$f" | awk '$1==".text" {print $2}'); fi
  echo -e "$e\tuser_s=$t\ttext=$text\tfile=$(wc -c < $f | tr -d ' ')"
done
