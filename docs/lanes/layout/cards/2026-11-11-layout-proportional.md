# Card: layout and write-back proportional to what changed

State: FROZEN 2026-11-11 (lane `layout`, round 6).

* **Question.** After one row is removed or inserted near the top of a flex column of N fixed-size rows, and after a
  width-only resize, how many instructions per row are spent in cephal (container recompute, per-child visits),
  `lay_writeback`, `place_quad`, `place_text`, ink writes; and how far can each be reduced without changing any
  result bit?
* **Change.** (1) attribution table; (2) moved-only text placement; (3) moved-only subtree write-back by delta;
  (4) cephal's per-child work on a re-run of a column whose children have cached answers; (5) resize: find why
  rows are re-laid; (6) parallel cold layout measurement.
* **Workloads.** `CHURN=1 redescribe_cost 100 1000 3000` (instructions per row per remove+insert pair),
  `redescribe_cost 10 100 1000` (regression cells), a width-only resize probe (same harness, new mode), cold mount
  of 10,000 rows.
* **Gates.** `cargo test --release -p bough -p bough-suite -p bough-awagy -p bough-zgui -p bough-vlist`;
  `crates/bough/tests/seals.rs` extended with a width change and a nested container; cephal's own tests.
* **Thresholds.** No existing instruction cell regresses past 1.02x. Churn and resize per-row costs must fall.
* **Negative controls.** One mutant per fast path (moved-only placement that forgets the last line; a delta move
  applied off-grid; a column reposition that skips one child) must fail the pixel gate.
* **Stop.** A fast path whose result cannot be shown bit-identical to the slow path is not landed.
