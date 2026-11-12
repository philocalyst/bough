# Handoff: lane `async-entities`, round 6

Session: Claude (Opus), 2026-11-12, stopped at the wrap-up order after deliverable 1. Deliverable 1
(`SOLID-ASYNC.md`) is done; deliverable 2 (the bough design) exists only as the sketch below; nothing was built.

## State

* **Branch** `lane/async-entities/round6`. Head: the commit that adds this file (see `git log -1`). Below it:
  `SOLID-ASYNC.md`, then `1373c48db5`, a clean merge of `lead/core-r6` at `4f4c469ef5` (the write log, seals for
  wired operands) into the lane's card commit `71bb94e0d9`. No source file differs from `lead/core-r6`.
* **The previous session's uncommitted edits** (1,439 insertions in `reader.rs`, `elem.rs`, `layout.rs`,
  `boxops.rs`, `transition.rs`, `signal.rs`, `rt.rs`, `app.rs`, `lib.rs`, `Cargo.toml`, a new
  `examples/signal_write.rs`) are preserved verbatim on side branch `lane/async-entities/prev-wip`, commit
  `4812f6afdb`, labelled "never built or tested". They were written against `42df2fbb67`, before the write log:
  their `rt.rs` still has the old scan-every-offset `settle` loop and would conflict with `4f4c469ef5`.
  **Judgement**, from reading all of it: keep the ideas, not the diff.
  * Worth taking: `RequestStamp` (producer slot, mount generation, ticket) admission; `FlightHandle` whose `Drop`
    is the cancellation (closes the flight and drops the unfinished future on the UI thread); `Inlet<T>` as the
    one primitive for streams; `Executor::{manual, threads, custom(spawn)}` chosen once in `Config`; the
    `sabotage_alias` mutant (admission by input equality must fail the `A -> B -> A` test); `Failure` kinds
    (domain / worker / revoked); `wire_to_front` for producers.
  * Worth dropping: the `shown(rt)` method threaded through every `IntoProp`/`TextSrc` and every element's
    `update` so that a re-described operand of a held signal looks its retained value up in a `HashMap` of
    `Box<dyn Any>` (a per-operand hook to fix what a per-phase swap fixes for free, see avenue 2); per-source
    cohorts keyed by a `HashMap`; `Box<dyn Any + Send>` per landing.
* **Merged into `round6`:** nothing from this lane.
* **Working tree:** clean apart from ignored `runtime/`. No background processes of this session remain.

## Numbers

Only one measurement was made: the baseline the zero-cost gate would be judged against.

| cell (`signal_cost`, instr/turn) | 16 rows once | 16 redescribe | 256 once | 256 redescribe |
|---|---|---|---|---|
| colour | 649 | 9,825 | 606 | 140,582 |
| number | 2,766 | 11,942 | 8,751-8,753 | 148,659-148,672 |
| string | 3,524 | 12,698-12,734 | 20,849-20,914 | 160,766-161,592 |
| idle | 247 | 9,423 | 247 | 140,223-140,254 |

Command: `cargo build --release -p bough --example signal_cost` (56 s, `CARGO_BUILD_JOBS=2`), then
`./target/release/examples/signal_cost 16 256`, three runs, load average about 69. Tree: the merge commit
`1373c48db5` (identical sources to `lead/core-r6` `4f4c469ef5`), so these replicate the lead's published
`4f4c469ef5` numbers (idle 247, colour 606, number 8,756, string 20,854 at 256 rows once) within 0.1 %, and
they are the "before" column for any async change. Verified: three runs, ranges shown. No "after" exists.

Solid figures in `SOLID-ASYNC.md` are counts from reading the source (flags, fields, issue references, test
files), not measurements of Solid running: there is no node toolchain on this Mac.

## Not verified / known broken

* Nothing of the design below is built, compiled or tested. The `prev-wip` branch has never compiled.
* **Suspected defect in the current `reader.rs` (base):** both reader tests mount with `.once()`. In
  re-describing mode (every application's mode) `App::turn` runs `redescribe` after `settle`, and an element's
  `update` re-applies `IntoProp::value()`, which reads the signal's *current* value: a sink of a held source would
  be overwritten with the new intent while its cohort is still pending, a torn frame. Not run; the cheapest
  check is `tests/reader.rs` with `.once()` removed.
* **Suspected ordering defect (base):** `Rt::make_latest` only sets the flag; it does not move the wire. A
  `latest(..)`/pending wire declared before the producer can run before the producer sets `HELD` and show "not
  pending" for the whole hold. `prev-wip` added `wire_to_front` for producers for this reason. Not tested.
* `BOUGH_SETTLE_AUDIT=1` was not run on any async path; the base `settle` audit explicitly exempts `HELD`
  heads, so a hold that is never released would not be reported by it.
* Binary size before/after: not measured.

## Experiments

None were run beyond the baseline above. The three this lane's cycle would have run, preregistered here:

1. **Hold propagation is O(latest wires), not O(sinks).** Mechanism: put producer and `latest`/pending wires in
   a front group of the head's chain (insertion order fixed by kind at `add_wire`), and have `run_head` `break`
   at the first ordinary wire while `HELD`. Prediction: the write that starts a request costs the same with 1
   and 1,000 sinks on the held source (±2 %); today it walks all 1,000. Kill: more than 5 % growth from 1 to
   1,000 sinks, or any `signal_cost` cell above 1.00x.
2. **A commit is independent of bystanders.** Mechanism: a publication commit touches only its members and their
   heads through the write log. Prediction: one publication commit costs the same with 0 and 10,000 other
   pending flights (≤ 1.02x). Kill: > 1.05x. (This is the previous card's threshold; the base implementation
   already scans `members.iter().find` per completion, which is O(members), so the base should pass for
   bystanders of other producers and fail inside one large cohort.)
3. **Wake-driven admission costs zero per turn.** Mechanism: admission runs only when the waker fired (the host
   posts a turn), never as a test inside `App::turn`. Prediction: idle turn with 0, 1, 100 and 1,000 pending
   futures = 247 instructions exactly (once) and the redescribe baseline exactly. Kill: any difference > 0 that
   scales with pending count; ≤ 3 constant instructions is reported as a loss, not hidden.

## Design, as far as it got (deliverable 2, unbuilt)

The one sentence: **an async value in bough is an ordinary owner field written later; the runtime owns when the
write becomes visible (publication), Rust's types decide what is visible meanwhile, and the phase of the turn
decides which world a piece of code sees.**

* **Cells.** The destination of a question is a plain `Signal<T>` (what shows meanwhile is its constructed
  value: stale-while-revalidate with no wrapper) or `Signal<Option<T>>` (no meaningful value before the first
  answer: the compiler forces `None` to be handled, and `view` writes plain `match`/`Option<E>` elements). That
  is the typestate: Rust's own `Option`, not a new type. `Reader<T>`/`boundary` stay as one more destination for
  compatibility.
* **Producers** stay elements of `view` (scope-owned, revoked with their scope, L5): `sig!(self.query)
  .map_async(sig!(self.hits), |q| search(q.clone()))`; the general form `.ask(f, land)` takes a
  `land: fn(&mut V, Out)`, so `Result<T, E>` outputs are routed by an ordinary `match` (errors are values; keep
  the last good value and set an error cell, or replace, as the author writes it).
* **Requests** are (frozen input, future, landing); a request is stamped (slot, generation, ticket); the
  producer owns the flight handle; dropping it (supersede, revoke) closes the flight and drops the future on the
  UI thread. Admission compares the stamp only; values are never compared.
* **Publication.** Every request fired by one turn's settle joins that turn's publication; a request that
  supersedes a member of an older live publication merges the older into the newer (it can no longer publish
  without showing a cut no model state had). Mount-time first requests publish alone. A publication commits when
  every member landed: run the landings (writes queue heads through the write log), release holds (clear `HELD`,
  queue the source heads), one settle, one damage epoch, one frame.
* **Holds.** A request's source head carries `HELD`; only the front group (producer, `latest`, pending wires) runs
  while held. `is_pending(sig!(self.query))` is a front-group wire that reads the bit: no pending state is stored
  anywhere else and nothing is propagated.
* **Worlds by swapping** (avenue 2): handlers and `changed` see intent; description sees the published cut; an
  older publication's landings see the truth beneath newer guesses. Selected by swapping the few held or guessed
  cells' contents at the phase boundary, not by a decider per read.
* **Optimistic writes** (avenue 6) are the same staging with the polarity reversed: the guess is visible now, the
  undo is staged in the action's publication.
* **Executors:** local polling on the UI thread with a plain `Waker` (no runtime; `!Send` futures allowed via a
  `map_local`), or a spawn adapter (`Executor::custom(|f| handle.spawn(f))` for Tokio,
  `|f| smol::spawn(f).detach()` for smol). The host's wake callback asks for a turn; admission happens on that
  turn only.

### The six screens, as a specification (unbuilt)

Each must pass under a deterministic manual executor and a fake clock (timers complete when the test advances the
clock), in re-describing mode, with every published frame equal to a fresh mount of the published state, and
`BOUGH_SETTLE_AUDIT=1` clean.

1. **Search as you type.** `query: Signal<String>`, `hits: Signal<Vec<Hit>>`, `error: Signal<Option<String>>`;
   the text field reads intent (its wires are front-group by construction); the header "N results for q" and
   the list are held until the answer for the current query lands; each keystroke drops the previous flight (a
   300 ms debounce is `sleep(300).await` inside the future: superseding cancels it before the fetch starts);
   typing back to the query whose answer is shown releases the hold at once with no request. Gates: no frame
   shows a header beside results of another query; with 10 keystrokes inside the debounce exactly one fetch
   starts.
2. **Paginated list.** `page: Signal<u32>` feeds `rows: Signal<Vec<Row>>`; the pager label and rows switch
   together; page 2 → 3 → 2 with page 2's answer landing last shows page 2 once, never page 3's rows.
3. **Detail pane on a selection.** `sel` feeds `story` and `comments` (two producers, one publication); the
   selection highlight in the list is held with them; an unrelated `unread` counter publishes immediately;
   selection changes during flight cancel both flights.
4. **Optimistic toggle.** `liked: Signal<bool>`, `save: Outbox<bool>`; the click sends and guesses in one
   handler (`v.save.send(want).guess(&mut v.liked, want)`); failure restores; two quick clicks with the first
   failing after the second was sent leave the second's guess on screen and correct the base beneath it.
5. **Form submit with error.** `submit: Outbox<Form>`, landing `match`es `Result<Receipt, SubmitError>` into
   `error: Signal<Option<String>>` and `receipt`; the button is disabled while `is_pending(sig!(self.submit))`;
   two submits both land (an action is not latest-wins), in completion order.
6. **Live values.** `symbol` feeds a stream into `price: Signal<Option<f64>>` through an `Inlet`; changing the
   symbol closes the old inlet (`send` answers `Closed`) and drops its guard; each value after the first is its
   own publication with exact damage (one numeric label).

## Underexplored and exotic avenues

1. **Pending decided by compiled offsets, not a graph walk.** Solid pushes "pending" through the dependent cone
   at every flight and walks it again at landing (`SOLID-ASYNC.md` §2), and re-derives completeness from links at
   every seam (§4). In bough the dependents of a cell are its wires, installed at mount from `offset_of!` proofs,
   and the destination's sinks need no pending state at all (nothing writes them until the commit). Pending is
   one bit on the source head. Buys: the write that starts a request costs the front-group wires only (experiment
   1); Solid's cost is O(cone). Risk: a cell read by `view` directly (not wired) is not covered by the bit, which
   is what avenue 2 fixes. Cheapest test: experiment 1. Depends on nothing.
2. **Worlds by swapping (the strongest idea of the session).** Solid needs about ten deciders and a 1,774-line
   shadow to decide, per read, whether it sees the committed or staged world. bough's turn has fixed phases
   (admit, handler, settle, changed, describe), so the world can be chosen per phase: before `redescribe`, swap
   each held cell's content with its retained value (the producer's last frozen input, moved, not cloned); after,
   swap back. Plain reads in `view` (`if *self.sel == 7`) then see the published cut, the gap the previous card
   left open as "speculative description (R1)", with zero per-operand cost; only `latest(..)` operands pay a
   lookup while held. The same trick runs an older publication's landing against the truth beneath newer guesses.
   Buys: correctness in re-describing mode at O(held cells) per turn (estimated 20 instructions per held cell),
   nothing when nothing is held (fold the test into `App::redescribe`'s existing `once` byte). Risk: any code that
   reads cells between the swaps must be in a known phase; `changed` and handlers must run on the intent side.
   Cheapest test: the reader tests without `.once()` with and without the swap.
3. **Cascades (waterfalls) as one publication.** An answer that is the input of another question (story → author)
   must not show story 7 with author 6. Rule: when committing writes a cell whose head starts with a producer
   wire, the publication's holds pass to the cascade publication instead of being released, and typed
   destinations are written with `mem::replace` so their old value becomes the retained value. If no downstream
   producer fires (equal input), release in the same turn. General `land` functions cannot be cascaded this way
   (their writes are unknown before they run); document that typed destinations are the chainable form.
4. **Publication as a column store through the write log.** A commit is a loop of typed stores into owner
   fields plus write-log entries; settle then runs exactly the logged heads. The staged outputs live in the
   flight's own allocation (one `Arc` per request holding state, future and output slot), so a landing allocates
   nothing. Buys: zero allocations per landing (Solid: an updater closure, a `Set`, a flush), commit cost
   independent of bystanders (experiment 2). Cheapest test: a counting allocator in an `async_cost` example.
5. **Wake-driven admission.** Today `App::turn` calls `admit` every turn. If admission runs only on the turn the
   waker requested (local tasks and cross-thread completions both ring the same doorbell), an idle app with any
   number of pending futures costs exactly the baseline (experiment 3). Depends on the host (`app` lane /
   `platform-host`) posting a run-loop event from the wake callback.
6. **Optimistic writes as reversed staging.** A guess is visible now and its undo is staged in the action's
   publication; failure applies the undo; an older action's failure while a newer guess on the same cell is live
   rewrites the newer guess's base instead of the cell; a question landing on a guessed cell writes the base. A
   guess is never held (a guessed source does not set `HELD`). Per cell this is a short chain, not a lane; no
   derivation runs twice (Solid computes every derivation of a guess twice). API: `v.save.send(x).guess(&mut
   v.liked, want)`: two disjoint field borrows, no ambient context. Risk: guesses on cells of another owner need
   a lifetime proof; restrict to the action's owner first.
7. **Netting writes back to what is shown.** A write that returns the source to the input whose answer is
   published (type "ab", "abc", backspace) cancels the flight and releases the hold with no request. Needs the
   producer to remember the input of its last committed answer, not only its last request.
8. **Turn membership with an escape.** One turn's requests publish together (Solid's tick rule), which
   over-entangles unrelated panels written by one handler. A per-producer `.alone()` (publish by itself) is the
   escape; Solid's equivalent is `createDeferred` with an admitted tear. Measure how often the six screens need it.
9. **Streams via nightly `AsyncIterator`.** The pinned nightly has `core::async_iter::AsyncIterator`; a stream
   producer can poll it directly instead of requiring an `Inlet` pump, keeping the `Inlet` for thread-fed sources.
10. **`Send` only at the worker boundary.** With local polling, `map_local` admits `!Send` futures that borrow
    nothing but own `Rc` state; only the spawn adapter needs `Send`. The executor choice is a runtime value today;
    making it a type parameter of `App` would let the compiler reject a `!Send` future on a spawning executor.
11. **Errors as the future's own output.** No `Failure` type is needed on the main path: `Result<T, E>` lands
    through `land`; only worker faults (panic, executor dropped the task) need a framework path. Keep the
    `Failure` kinds for `Reader` compatibility only.
12. **Record state planes for pending.** The lead's state-plane mechanism (one bit column per state, read by
    variant tables) makes "loading" a flag like hovered or disabled: `is_pending` could set a `Loading` bit on the
    records of a region instead of being a wire, so dimming a pending list is one masked range write. Depends on
    the state-plane work in another lane.
13. **Prefetch as a question without sinks.** A producer whose destination nothing reads yet (hover prefetch)
    costs one flight and no hold; its answer can seed a cache cell the real question reads first. Unexplored.
14. **Retained == full redraw as the async oracle.** The strongest gate for "no torn frame" is mechanical: after
    every step of a random schedule (writes, completions in any order, cancellations, guesses, failures), build a
    fresh app from the *published* state and compare its records with the retained ones. The existing reader
    test checks three text values; the oracle would check everything.

## Where I think the lead or the plan is wrong

* The ROUND6 amendment frames async as "publication" and rejects porting Solid's scheduler: right. But the
  previous card (`cards/2026-11-11-staged-publication.md`) kept "an unwired read of a held signal in `view`
  still reads intent" as an accepted gap. Every application re-describes, so that gap is the common case, not an
  edge; it should be closed before any cost work (avenue 2).
* The `async-entities` lane has been a "second wave" lane, started late twice. Async correctness in re-describing
  mode is a precondition for any real application on bough; it should not trail the micro-optimisation lanes.
* "Zero instructions per turn beyond baseline" is only achievable if admission is wake-driven (avenue 5); with
  the current `admit()` call in `turn` it is at best a constant 2-3 instructions. The gate should say which.

## Next

1. **Close the re-description gap with worlds-by-swapping** on the current `reader.rs`, re-run `tests/reader.rs`
   and `tests/owners.rs` without `.once()`. Predicted: the base fails at least one torn-frame check in
   re-describe mode; with the swap both pass; every `signal_cost` cell stays at 1.00x (the swap's test folds into
   the `once` byte).
2. **Typed destinations + stamped, drop-cancelled flights + wake-driven admission**, taking the listed parts of
   `prev-wip` onto the write-log `settle`. Predicted: idle turn with 1,000 pending futures = 247 instructions
   (once); one landing allocates nothing; a publication commit ≤ 1.02x between 0 and 10,000 bystanders.
3. **The six screens** above as integration tests with a fake clock, plus the full-redraw oracle (avenue 14) over
   random schedules, under `BOUGH_SETTLE_AUDIT=1`. Predicted: the base engine fails screens 1 (re-describe),
   4 and 5 (no actions or guesses exist); the new engine passes all six.
