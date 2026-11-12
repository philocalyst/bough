# Async in bough, round 7: the design the programs asked for

Lane `async`, round 7. The fourteen programs in `programs/` were written first, in the API wished for, and
this document is what they need. Where a rule below has a reason, the reason is a program or a broken
hypothesis; nothing here is copied from Solid's machinery (`../async-entities/SOLID-ASYNC.md`), only its
problems.

## 0. The model in one paragraph

State is the view's fields. A **question** is a view element that keeps a cell equal to an async function of
other cells (`ask(sig!(self.query), search).into(sig!(self.hits))`); it asks again when its input changes,
drops the future it no longer needs, and lands the answer as an ordinary write. While a question is out, its
**source cells are held**: everything on screen that shows them keeps showing the value whose answer is on
screen, so a question and its answer always appear together; inputs and `latest(..)` show what was asked. The
questions one turn asks are **one publication**: they appear together, in one frame, with their inputs. An
**action** is a command a handler runs (`self.save.run(x)`); all of its runs complete, they land in the order
sent, and what is in flight is readable state (that is the whole of optimistic UI). Cancellation is drop, an
error is an answer, a stream is a question with many answers, and time is the runtime's clock.

## 1. What the programs needed (and what they did not)

| program | needs |
|---|---|
| 1 search | `ask(..).into`, latest-wins with drop, holds, `pending`, inputs read intent, ready-on-first-poll publishes in the same turn |
| 2 pager / feed | netting (back to the shown page is free); `Action` with issue-order landing and `inflight()` |
| 3 optimistic | `Action::inflight()` read where shown; landings and end-of-flight in one turn |
| 4 dependent | two questions on one source publish together; cascade (author asked from the story) keeps the hold; mapped sources (`sig!(x).map(f)`) cut re-asks |
| 5 stream | `follow` (async iterators: first item publishes, later items are their own frames); streaming `Action`; `cancel` |
| 6 images | questions scoped to a list row (dropped on unmount); `compute` on a worker with a cancel flag |
| 7 route | a route is a question whose answer is a page (fetch high); `latest` for the tab asked; `crossfade` on release |
| 8 swr | quiet re-ask (`refresh`), `every(d)`, landing handlers (`then`) with `about` |
| 9 errors | `Option`/`Result` as elements (the boundary is a `map`), `refresh`, `pending` on a destination |
| 10 race | nothing from the runtime but dropping: `race`, `timeout` are future combinators |
| 11 retry | `async gen` streams; drop stops a backoff sleep |
| 12 compute | slices on the UI thread (`yield` in a stream), `compute_stream` on a worker, `deferred` questions |
| 13 skeleton | `morph` with `tag` pairs, released with the publication |
| 14 interrupt | `slide` + `keyed`, revival of a leaving item, retarget from the current curve value |

Things no program needed and that are therefore not in the API: a `Resource`/`Query` handle type, a loading
enum, a suspense boundary component, a transaction object, an abort signal, a scheduler priority, a cache
(program 14 keeps its own map; a client library can cache below the futures).

## 2. Semantics

**Questions.** `ask(src, f)` with `src` a `sig!(..)`, a `sig!(..).map(g)`, a tuple of those (`(a, b).map(g)`)
or `()`. The input `S: Clone + PartialEq + 'static` is computed from the source cells whenever one of them is
written; `f(S)` returns a `'static` future (`follow`: an `AsyncIterator`). Finished by `.into(dst)` (the answer
replaces `dst`; an answer `T` goes into a `Signal<T>` or a `Signal<Option<T>>`) or `.then(handler)` (a
`fn(&mut V, O)` run as a write; `.about(sig!(cell))` names the cell `refresh`/`pending` reach). Modifiers:
`.deferred()` (holds nothing), `.every(d)` (quiet re-ask on the clock).

* Equal input, nothing happens. A different input drops the flight in the air (cancellation is drop: a
  debounce `sleep` or a request inside the future goes with it) and starts a new one, polled once at once.
* **Netting:** an input equal to the one whose answer is shown drops the flight and asks nothing.
* **First answer:** a question asked at mount (or when its subtree mounts) holds nothing: there is no shown
  answer to keep consistent, and its destination shows its initial value (`None`: the loading state).
* **Quiet re-ask:** `refresh()` or `every` asks again with the same input; nothing is held (the input did not
  change), `pending` is true on the destination.

**Holds.** While a non-deferred question is in flight, its source cells are held:
* Wires of a held cell do not run, except `latest(..)` and `pending(..)` operands and other questions.
* A description (re-describing mode) reads a held cell's *shown* value: the runtime swaps the kept value in
  for the description and back after it (section 4). `self.x.latest()` reads intent there.
* Handlers, `changed`, landings and futures' inputs always see intent: a handler reads its own writes.
* `pending(sig!(x))` / `self.x.pending()` is true while `x` is held or a question about `x` is in flight.

**Publications.** The questions fired by one turn form one publication, merged with any live publication that
holds one of their sources or contains one of them (a re-fire). A publication commits when none of its
questions is in flight: every answer is landed (in mount order), then its cells are released (their wires
run), all in one turn: one settle, one damage epoch, one frame. A question that is dropped (superseded into
another publication, netted, or revoked with its scope) leaves its publication; an empty publication commits.

**Cascades.** If committing lands a cell that is the source of another question and that question fires, the
fired question inherits the committing publication's held cells, which stay held: a waterfall shows its first
and last step together (program 4). If it does not fire (equal input), the commit releases at once.

**Streams.** `follow`: the first item lands as the question's answer and commits the publication; each later
item is a landing of its own (its own turn and frame), holding nothing. `.into(dst)` replaces per item. A new
input drops the stream (`AsyncIterator` dropped mid-poll) and holds until the new stream's first item; a
stream that ends with no item releases.

**Errors** are answers (`Result` outputs land like values). A panic in a future propagates on the UI thread
at the poll, as a panic in a handler would; a worker's panic is carried back and resumed there.

**Actions.** `Action<I, O>` is a field: `Action::new(f).then(handler)` (`Action::stream(f)` for streams).
`run(i)` in a handler starts `f(i)` at the end of the turn; every run completes unless `cancel()`led; landings
of one action run in the order of their runs (a later answer waits for an earlier one); a landing and the
removal of its input from `inflight()` are one turn. Actions hold nothing. The action's landing is bound to
its owner by the wire machinery (its cell is wired at mount), so an action of a removed row never lands.

**Time and executors.** Futures are polled on the UI thread (no `Send` needed). A waker marks its flight and
asks the host for a turn; woken flights are polled at the start of the next turn (admission), never as a test
inside an idle turn. `sleep`/`timeout`/`every` use the runtime clock (a fake clock in tests: `App::advance`).
`compute(f)` runs `f(&Cancel)` on a worker pool and is the only place `Send` is required; dropping it raises
the flag. `compute_stream(f)` gives `f` a `Report<T>` whose sends are stream items on the UI thread.

## 3. The two hypotheses, attacked

### 3.1 "A transition is a journal withheld" (the brief)

Taken literally (the turn's journal of writes is applied when its pending set empties; frames only show
applied journals), it breaks in four places:

1. **The input itself.** The turn that writes `query` must not be withheld from the text field: typing would
   freeze for the length of the request. So a journal is applied for some consumers (inputs, `latest`,
   `pending`) and withheld for others. "Frames only show applied journals" is false as stated; the true
   invariant is per consumer class: held consumers show the shown cut, intent consumers show intent.
2. **Read dependencies across journals.** J1 (`tab = 2`, waiting) is withheld; J2 is an unrelated handler that
   *reads* `tab` (intent 2) and writes `status = "tab 2"`, with nothing pending. A journal of writes does not
   say what its handler read, so J2 is applied and shows "tab 2" beside tab 1's content. Ordering J2 after J1
   needs read sets (handlers are arbitrary Rust) or FIFO publication (an unrelated click waits for a slow
   fetch). Neither is acceptable; the honest rule is "derive, do not copy": the tear exists only for state a
   handler copies out of a held cell, which bough's style (wires and descriptions derive) avoids, and a debug
   check can flag (a write in a turn that read a held cell).
3. **Values.** In re-describing mode the description reads the struct, which already holds intent. A journal of
   (slot, plane, class) entries carries no old value, and an in-place mutation (`self.items.push(..)`) leaves
   none to recover. Withholding a write from `view` needs the old value, so it can only be done for cells that
   keep one: the pre-image mechanism of section 4, at a clone per write turn. Doing that for every cell of
   every turn is a clone of everything written; doing it for the sources of questions is cheap and enough.
4. **Granularity.** Withholding whole turns over-holds: a handler that writes the query and clears an unrelated
   error banner would hide the banner change until the search lands.

What survives, and is what is built: **a transition is the set of questions a turn asked; what is withheld is
the propagation of their source cells** (their wires and their reads by descriptions), released as one when
the set is empty. At the level of cells this is exactly "a journal withheld", restricted to the cells whose
values the runtime keeps. "Cancellation is drop" and "holds are a mask, not a walk" survive unchanged: a hold
is one bit on the cell's head (and, once core has per-owner planes, one bit in a `held` plane).

### 3.2 "Holds as a pending plane; release = OR the journal into the dirty planes" (lead's notes §4)

1. **Withholding damage is not withholding content.** If record writes of a held world are applied to the
   region and only their dirty bits are withheld, any repaint of a tile those records touch (other damage
   nearby, a buffer-age repair of an older surface, a full redraw after a resize or a purged surface, the
   CPU executor) draws the withheld state. Test that kills it: hold a world, write it, force a full redraw;
   `retained == full redraw of the published state` fails. So the region must never contain held state: the
   record writes themselves must wait, which is what not running the wires does.
2. **Two worlds on one page.** W1 and W2 both touch page p; releasing W1 ORs p into the dirty plane and p's
   content already includes W2's writes. Same cause as 1.
3. **Descriptions** read state, not records; a record-page journal cannot hide a cell from `view`.

What survives: **the plane is right one level up**, over cells instead of record pages. A world is a mask over
an owner's cells; holding is `held |= mask`, releasing is `held &= !mask; queued |= mask` (the queued plane
core is building), and the propagation that follows writes the records and their dirty bits in the ordinary
way. A record-level `pending` plane is still useful, for *presentation*: dimming or shimmering what is held as
a kernel parameter with no turn (a skeleton's shimmer is the same parameter), never for correctness.

## 4. Re-describing mode: worlds by swapping, with pre-images

A description must see the shown value of a held cell. The runtime keeps it:

* A cell that is a source of a holding question has a `KEEP` bit on its head. The first write of a turn to a
  `KEEP` cell, or the first write to a held cell during its hold, clones its value before it changes (in the
  write log's cold path, which is already the "first write of this cell in this turn" event): a **pre-image**.
* When a question fires and holds a cell, the cell's pre-image of this turn (the value on screen) becomes its
  kept value. A held cell never written during its hold needs nothing (intent equals shown).
* Around the description the runtime swaps every held cell's kept value with its content and swaps back after
  (O(held cells), nothing when nothing is held). Wires installed by the description on a held cell read the
  shown value too, and run with intent at the release.
* At release the kept value is dropped. Unused pre-images are dropped at the end of the turn.

Cost: one clone per turn that writes a source cell of a holding question (a keystroke clones the query
string), nothing for any other cell, nothing per read. The alternative of a per-read decider (Solid's ten,
and the previous lane's per-operand `shown(rt)` hook) costs every read.

## 5. Transitions tied to async (Amendment 1)

A transition is a parameter of a kernel advanced by the presenter: `(from, to, t0, duration, curve)` per
animated lane, evaluated at the presenter's `now`; no turn runs while it plays, its damage is the swept bounds
of `from` and `to` (known when it starts), and interruption retargets from the curve's value at `now`, which the
CPU computes with the same function.

The programs use three element-level transitions, all on dynamic parts (the elements whose item can change):
`crossfade(curve, e)`, `slide(curve, e)` and `morph(curve, e)` with `.tag(n)` pairs. When the item's key
changes (an `Arm` variant, a `keyed` key), the leaving item is kept mounted in its own space and the entering
item is mounted in another; each space gets one transition (alpha, translation and scale); the leaving item is
released by one turn scheduled at the end time. Re-keying to a leaving item revives it and retargets both
spaces. Async needs nothing more: the hold decides *when* the item changes (when the data is there), the
element decides *how*. A publication-level form ("release this world with this transition", every changed
record animating from its old state) needs per-record from-lanes in the instance and is the contract proposed
to core and compute (`TRANSITIONS.md`).

## 6. Open questions for the user

1. Default hold scope: a question holds its sources (consistency first). The opt-out is `.deferred()` per
   question and `latest(..)` per consumer. Solid and React default the same way; is that the default wanted?
2. Mapped sources over large cells (`sig!(self.items).map(len)`) hold the whole cell: the list does not show
   an edit until the question about it lands. Correct by the rule, surprising in practice; `.deferred()` is
   the fix. Should a mapped source hold nothing by default?
3. A question mounted later than its destination was last written (a pane reopened while the selection
   changed) shows the old answer beside the new input until it lands (stale, flagged by `pending`). Making it
   tear-free needs a hidden first mount ("reveal when ready", section 5's speculative form). Wanted?
4. Action landings wait for earlier runs of the same action (issue order). Right for writes; a search-like
   action wants latest-wins, which is a question. Is a third policy (latest-wins actions) wanted?
