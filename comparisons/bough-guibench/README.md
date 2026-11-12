# bough gui-bench, round 6

The eight gui-bench scenarios on `bough::prelude` (raw escapes listed in `docs/lanes/generalization/FRICTION.md`), a
headless driver, and an adversarial matrix. Results and findings: `docs/lanes/generalization/OVERFIT.md`.
`src/spec.rs` and `src/cli.rs` are verbatim from zortax/gui-bench 13f3cb61687c25f80ea46625680d8e0804f6fbed.
This directory is its own workspace (its own `Cargo.lock`); use the repository's pinned toolchain.

```sh
cargo build --release --manifest-path comparisons/bough-guibench/Cargo.toml --bins   # --features deep: static 200-level tree (minutes)
cargo test  --release --manifest-path comparisons/bough-guibench/Cargo.toml
```

* `bough-bench --scenario NAME --rows N [--variant fine|coarse]`: the real window (`GUIBENCH_SECONDS=3` bounds a run).
* `headless --scenario NAME --rows N --op OP [--variant ..] [--plain] [--mounted] [--scale S]` plus one of
  `--repeats 0` (mount only), `--gate --ring R --repeats 4` (pixel and model gates; its counters are not costs),
  `--repeats 8 --blocks 3 [--render]` (instructions per operation: R0, or R1 with `--render`).
  Ops: mount, hover, scroll, jump, select, update10, swap, remove, insert, replace, append, truncate, clear,
  replace-select, ticker, spinner, typing, filter, filter-nochange, resize. `--mutant` is the negative control.
* `adversarial --case NAME --op OP --n N [--w --h --scale --k --flag --once]`: one adversarial cell (see `adv.py`).
* `sweep.py` (gui-bench cells), `adv.py` (adversarial matrix, predictions inside), `report.py` (the tables).
  `matrix.py`, `run.py`, `report_matrix.py` are the previous stage's runners, kept for its recorded data.

Environment: `GUIBENCH_CAPS` = `default` | `raised-spaces` | `name=value,..` (never implicit; default is
`ui.rs::APP_CAPS`); `GUIBENCH_REHOVER=same|jitter` and `GUIBENCH_IDLE=k` are diagnostics (re-deliver the pointer after
a scroll; k idle frames after each operation); `BOUGH_FONT` selects the face (default system Arial).
