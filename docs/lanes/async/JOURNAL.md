# Lane `async`, round 7: journal

Worktree `wt/r7-async`, branch `r7/async`, from `round6` 3b0b00c698. Read in this order to resume: this file,
`DESIGN.md` (the semantics and the attacks on the two hypotheses), `programs/` (the API as authors use it).

## State (newest first)

### 2026-11-12, session 1

**Read.** ROUND7 (async section, Amendment 1, remote recipe), checkpoint sections 6/7.9/10, the old lane's
HANDOFF and SOLID-ASYNC, lead's R7-LEAD-NOTES (section 3 kernel transitions, section 4 holds as a plane), the
façade's turn (`app.rs`), settle and wires (`rt.rs`), the write log (`signal.rs`), re-description
(`describe.rs`), the current `reader.rs` (cohorts, `produce`, `boundary`), `transition.rs`, the compile-time
schema in `reflect.rs`.

**Defect 1 confirmed (base `round6` 3b0b00c698, `cargo test --release -p bough --test reader_redescribe`,
`BOUGH_SETTLE_AUDIT=1`).** `tests/reader.rs` without `.once()` (new file `tests/reader_redescribe.rs`): both
tests fail at the first held selection, `Shown { story: Some(0), comments: Some(0), sel: 7, now: 7, pending:
true }`: re-description re-applies the wired operand's current (intent) value and tears the held sink.
Prediction from the old handoff ("the base fails at least one torn-frame check in re-describe mode") held.

**API first.** Fourteen programs written in `programs/` before any runtime change (the twelve of the brief plus
skeleton morph and interrupted transition from Amendment 1; route transition includes the cross-fade), against a
fake client library (`programs/api.rs`) whose calls stay pending until a test answers them and record drops as
cancellations. Iterations so far:
* `Some(..)` wrappers removed: one `into(dst)` takes an answer `T` into `Signal<T>` or `Signal<Option<T>>`
  (two impls of an `Accept` trait that do not overlap: checked with rustc on the pinned nightly).
* `.filling(cell)` renamed `.about(cell)` and needed only by handler-landed questions that are refreshed or
  shown as pending (program 8); dropped from program 14 where nothing used it.
* `async gen` blocks work on the pinned nightly (checked): streams are written as plain generators (programs
  5, 11, 12), so no stream combinator library is needed.
* Actions need no registration: their fields are found at mount from the compile-time schema (`type_info`,
  as `reflect.rs` does), so the offset is proven and the landing's view type is checked at mount.

**Hypotheses.** Both attacked on paper in `DESIGN.md` section 3. Short form: "a transition is a journal
withheld" breaks on the input itself (typing must show), on read dependencies across journals (no read sets
for handlers), on values (a write journal carries no old value; re-description needs one) and on granularity;
it survives restricted to the source cells of the questions a turn asked. "Holds as a pending plane, release
= OR the journal into dirty planes" breaks because withholding damage is not withholding content (any other
repaint of the tile draws the held state; `retained == full redraw` fails); it survives one level up, as a
plane over cells.

**Next.** Build `crates/bough-async` (platform-independent: flights, wakers, clock, `compute`, the
publication engine) with a model checker for ilo-prod; then the façade (`ask`/`follow`/`Action`/`pending`,
pre-images and the describe swap, wire groups), the programs as tests, the defects, transitions.

## Experiments (prediction and kill criterion written before the run)

| # | mechanism | prediction | kill | result |
|---|---|---|---|---|
| E0 | base reader test in re-describing mode | fails (torn) | passes | **failed as predicted** (defect confirmed) |
