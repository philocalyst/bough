# bough gui-bench, round 6

Candidate rows/table stage on the normal application update path. Default capacities are unchanged.
`src/spec.rs` and `src/cli.rs` are verbatim from zortax/gui-bench commit
`13f3cb61687c25f80ea46625680d8e0804f6fbed`, shared with its zgui reference port (Apache-2.0 package).
The suite does not compare text rasterization against zgui. It checks gui-bench's fixed RGB layout points,
retained repair against full redraw, and fresh mount against retained state.

This directory is its own workspace. Use the repository's pinned toolchain, an isolated target and four jobs:

```sh
cargo build --release --locked --manifest-path comparisons/bough-guibench/Cargo.toml --bins
cargo test --release --locked --manifest-path comparisons/bough-guibench/Cargo.toml
"$CARGO_TARGET_DIR/release/bough-bench" --scenario rows --rows 100
"$CARGO_TARGET_DIR/release/bough-bench" --scenario table --rows 100 --variant fine
"$CARGO_TARGET_DIR/release/headless" --scenario table --rows 100 --variant coarse --op select --gate --repeats 4
```

`GUIBENCH_SECONDS=3` bounds a window smoke run. `BOUGH_FONT` selects the same face in both drivers;
otherwise `ZGX_FONT_FILE`, then system Arial. Table F1–F8 use the host's private-use Unicode Key::Char
representation. The root starts focused without a ring. Native no-focus window shortcuts and row-owner
routes are missing; see the exact escapes in `docs/lanes/generalization/FRICTION.md`.

Headless `--render` includes the Metal frame; without it, the same input and frame path ends at GPU input.
`--gate` draws, compares pixels and validates the model; its reported timings are **not performance data**.
`--ring 1..4` controls the headless surface ring; `--mutant` adds a deliberately linear selection scan.
`--op` accepts mount, hover, scroll, select, update10, swap, remove, insert, replace, append, truncate, clear.
One repeat is operation plus undo; replace and remove/insert allocate fresh IDs exactly as the shared spec does.

`run.py` isolates cells with 120-second timeouts and preserves all process exits. A measured batch needs
`--lock "$BOUGH_MEASURE_LOCK"` pointing at the round's shared lock, and an otherwise idle machine.
Only the lock acquired by this invocation is removed. Gate-only runs are correctness evidence, not timings.
The application currently rejects the six later-stage scenarios explicitly. No Linux or fakeland run is claimed.
