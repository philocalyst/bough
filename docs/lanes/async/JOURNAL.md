# Lane `async`, round 7: journal

Worktree `wt/r7-async`, branch `r7/async`, from `round6` 3b0b00c698. Read in this order to resume: this file,
`DESIGN.md` (the semantics and the attacks on the two hypotheses), `programs/` (the API as authors use it).

## State (newest first)

### 2026-11-12, session 1 (continued): the façade, the programs as tests, costs

**Built** (commits after `3cdcdda60f`):
* `crates/bough-async` (no platform code): flights (futures and `async gen` streams) polled on the UI thread,
  wakers that ring one atomic flag, the clock (`sleep`, `timeout`, intervals; fake in tests), `compute` /
  `compute_stream` on a worker pool with a cancel flag, `race`, `yield_now`, and the publication engine
  (questions, holds, two-phase commits, cascades, netting, revocation, actions). Address-keyed maps use a
  multiply hasher (`addr`).
* `crates/bough`: `ask`/`follow` question elements (`ask.rs`); `Action` fields found at mount from the
  owner type's compile-time layout (`action.rs`; the comptime trait lookup does not see impls with lifetime
  bounds, so the bounded operations are fn pointers made in `Action::new`); head bits `HELD`/`KEEP`/
  `LOADING` (`HEAD_IDX` now 28 bits; conflicts with surface-api's planned bit 29 at integration); wire
  groups (questions, status, plain; newest first within a group) replacing the `latest` flag, which also fixes
  defect 2 (a status wire declared before its producer); pre-images in the write log's cold path and the
  swap around re-description (`signal::keep`); inputs read intent (text-field wires in the status group);
  `pending`, `Signal::{pending, latest, refresh}`; `Result`/`Arm2..4` sum elements; `Config::inert`,
  `App::{published, mirror_status_from, advance, use_fake_clock, run_until_stalled}` for oracles and tests.
* `tests/programs.rs`: programs 1-12 driven against their own doc comments with the fake client and clock,
  in re-describing mode, a published-state oracle (a fresh inert app built from what the screen shows, status
  bits mirrored, compared record by record) after the steps that matter. 17 tests, all pass under
  `BOUGH_SETTLE_AUDIT=1` (the audit no longer exempts held heads). `tests/reader_redescribe.rs` (defect 1)
  fails on the base and passes here.

**Bugs the programs and the model found** (each fixed): an unclosed cascade group; a quiet answer landing
beside a held source (the rule now: a question whose source is held joins the holder, at ask and at landing);
the one-phase commit that showed a cascade's non-feeding members beside the held selection (now two phases);
a stale pre-image table after a runtime was dropped and its address reused; `queue` treating "non-zero head"
as "has wires" (a `LOADING` bit on an unwired destination stuck `QUEUED`); refreshes queued after a settle
wave had collected its entries; `reflect_records` mapping released, unreclaimed nodes onto reused handle
indices (stale texts; a reflection bug, fixed in `reflect.rs`); program 5's chat appended tokens to the
wrong line with two replies in flight (`Action::then_with` gives the landing its run's input).

**Costs** (instructions on the calling thread, `signal_cost`/`async_cost` examples, this Mac (load 12),
release without debug info, three identical runs; base = `round6` 3b0b00c698 built the same way):

| cell | base | branch |
|---|---:|---:|
| idle turn, 256 rows, mount-once, app without async | 247 | 250 |
| colour write, 256 rows, mount-once, app without async | 606 | 613 |
| idle turn, 256 rows, re-describing, app without async | 139.6-141.1 K | 139.5-140.1 K |
| idle turn, async app, mount-once, 0 / 1 / 100 / 1000 questions pending | | 239 / 386 / 386 / 386 |
| start a question (turn), 1 / 1000 sinks on its source | | 3,879 / 3,879 |
| a landing turn (16 sinks), and the same writes by hand | | 68,054 / 63,704 |
| allocations per landing turn, and by hand | | 62.34 / 59.45 |

Getting the app-without-async cells there took a bisection: the first build cost +69 per idle turn. Not the
thread-local (a `Vec` in it, removed anyway) and not struct layout: `after_handler` and `redescribe` stopped
being inlined into the turn once they carried the async path, and the turn paid calls and prologues for code
it never ran. Both are now an inlined fast path plus an out-of-line async path. Folding the async checks into
`settle`'s own thread-local access cost apps without async +30 (codegen of `settle`), so it was reverted.

**Interim note sent to the coordinator** (API, hypotheses, what is implemented).

**Next.** Transitions as kernel parameters (programs 13, 14 and the cross-fade in 7: today pass-through
stubs); the model checker on ilo-prod (bigger alphabet: actions, two-source questions, intervals,
revocation of a loud member; mutants); defect 3 (pre-armed batch timeout on suspend); the interactive host's
wake and timer hook (`Engine::set_ring`, `next_wake`).

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
| E1 | model checker on the first engine | finds nothing in 300 seeds | n/a | **found 2 bugs and a design flaw** on its first run |
| E2 | idle turn with N pending (mount-once) | flat in N within 3 instr | grows with N | **held**: 386 for N = 1, 100, 1000 |
| E2' | idle turn with N pending, async app vs no async | within 3 instr of the app without async | | **killed**: +147 (239 -> 386), the async turn path itself |
| E3 | start a question, 1 vs 1000 sinks (mount-once) | within 5 % | > 5 % | **held**: equal (the hold breaks at the first plain wire) |
| E3' | the same in re-describing mode | within 5 % | > 5 % | **killed**: 5.6 K vs 158 K (re-description walks the sinks; not async) |
| E4 | allocations per landing beyond the same writes by hand | <= 2 | > 2 | **killed**: 2.9 |
