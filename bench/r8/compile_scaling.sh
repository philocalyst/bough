#!/bin/sh
# Compile time and code size as the number of distinct components grows (journal P14).
# Run from the repository root with the lane's cargo environment. Writes the generated examples under
# crates/bough-x/examples/gen_*.rs (ignored) and prints one TSV row per build.
set -eu
out=${1:-bench/results/r8/expr/compile-scaling.tsv}
printf '# host=%s profile=dist commit=%s date=%s load=%s\n' "$(hostname -s)" "$(git rev-parse --short=10 HEAD)" "$(date -u +%FT%TZ)" "$(uptime | sed 's/.*averages*: //')" > "$out"
printf 'mode\tn\tinstructions\tuser_s\treal_s\ttext_bytes\tfile_bytes\n' >> "$out"
for mode in distinct shared; do
  for n in 1 8 32 128; do
    ex=gen_${mode}_$n
    python3 bench/r8/gen_components.py $n $mode crates/bough-x/examples/$ex.rs
    touch crates/bough-x/examples/$ex.rs
    /usr/bin/time -l nice -n 15 cargo build --profile dist -p bough-x --example $ex 2> /tmp/r8-expr-time.$$ >/dev/null || { cat /tmp/r8-expr-time.$$; exit 1; }
    ins=$(awk '/instructions retired/ {print $1}' /tmp/r8-expr-time.$$)
    usr=$(awk '/ real / {print $3}' /tmp/r8-expr-time.$$)
    real=$(awk '/ real / {print $1}' /tmp/r8-expr-time.$$)
    bin=$CARGO_TARGET_DIR/dist/examples/$ex
    text=$(/usr/bin/size -m "$bin" | awk '/Section __text:/ {print $3; exit}')
    file=$(wc -c < "$bin" | tr -d " ")
    "$bin" >&2
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\n' $mode $n "$ins" "$usr" "$real" "$text" "$file" >> "$out"
    rm -f /tmp/r8-expr-time.$$
  done
done
cat "$out"
