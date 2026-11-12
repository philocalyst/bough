# How Solid 2.x does async, mechanism by mechanism

Lane `async-entities`, round 6. The question: what problems Solid's async design solves, how, what each answer
costs, and what breaks, so that bough can take the problems and the principles without the machinery.

**What was read.** `ref/solid` (the `next` branch, revision `ce5b288`), package `packages/signals`. All paths
below are relative to `packages/signals` unless they start with `packages/solid` or `documentation/`. The code is
the build after Solid's "L2 hold model" of 2026-10-04 (`docs/INTERNALS-ASYNC-STATE.md:14-16`): §1-§4 of that file
describe an earlier design (`_asyncReporters`, `initTransition`, companions) that no longer runs, and many names
in older write-ups are gone. Where this document names a mechanism it names the running code; the successor table
is in the note at the end. Every line reference was read in this revision; ten were re-checked by hand.

**Read so far / not yet read.** Read: `src/core/{async,core,scheduler,lanes,guesses,verdict,action,truth,seat,
effect,error,error-hooks,constants,types,heap,graph}.ts`, `src/{boundaries,reveal,deferred,from,flatten,affects,
affects-marks}.ts`, the optimistic parts of `src/store/optimistic.ts`, `packages/solid/src/client/flow.ts`, and the
cited sections of `docs/INTERNALS-ASYNC-STATE.md` (§0-§3 in full), `SPEC-ASYNC-SEMANTICS.md`,
`RULES-FUNDAMENTALS.md`, `RULES-DERIVED.md`, `SCENARIOS.md`, `create-deferred.md`. Not read: the rest of
`src/store/` (projections, reconcile, the store's own hold and fold paths), `src/map.ts`, `src/core/attribution*`
and `dev.ts`, the test bodies beyond the ones cited, and the server/SSR streaming paths. The store's per-key holds
are the largest unread async surface.

**The model in one paragraph.** Any computation may return a Promise or an AsyncIterable. A read of a value that
has no answer yet throws `NotReadyError`; the status is pushed through every dependent. When an on-screen reader
(a render effect) goes pending, the flush that caused it *parks*: every write of that flush is held in a
`Transaction`, render effects keep reading committed values, and the transaction *lands* (all its writes commit,
its readers re-run) when nothing on screen still waits on one of its flights. `<Loading>` escapes the hold for
first loads, `isPending`/`latest` show the wait, `createOptimistic` shows guesses on top of a hold, `action`
groups an async mutation into one transaction. The invariant everything serves is F-1, "the screen is whole":
a value and everything on screen derived from it change in the same frame (`docs/RULES-FUNDAMENTALS.md:47`).

---

## 1. Async derivations: a memo that returns a Promise or an AsyncIterable

**Problem.** A derived value needs I/O. Readers need either a value or a "not ready" verdict, and the answer must
arrive later without the author wiring subscriptions.

**Solid's answer.**
* There is no `createAsync` or `createResource` in this tree (`packages/solid/src/index.ts:320`; `createAsync` is
  only named in `documentation/solid-2.0/03-control-flow.md:210`). `createMemo` is `computed()`
  (`src/signals.ts:514-519`); `recompute` hands any non-null object result to `handleAsync`
  (`src/core/core.ts:644-663`), which probes `Symbol.asyncIterator` then `.then` (`src/core/async.ts:466-471`).
* **Flight identity is the promise object**: `ext(el)._inFlight = result` (`async.ts:511`). Every recompute nulls
  it before running the body (`core.ts:469-479`); every callback compares `el._x?._inFlight !== result` and drops
  the answer if superseded (`async.ts:538, 594, 774, 795, 870`).
* An unsettled promise throws `new NotReadyError(context)` (`async.ts:917-921`). The landing, `asyncWrite`
  (`async.ts:593-681`), stages the value through `setSignal` like a user write (`650`), retires the node's own
  pending entry (`landStatus`, `943-976`), wakes readers (`settlePendingSource`, `669`) and calls `flush()`
  synchronously (`670-679`).
* **Streams** are one flight: each later yield is an `asyncWrite` followed by a flush, and only then is the next
  item pulled (`async.ts:708-822`, `776-778`).
* **Commit #0** (`loadingValue`): a node born with a value serves it instead of throwing until the first answer
  lands, is never held, and is never pending (`core.ts:1186-1232`; `async.ts:918-921`; ruling A27,
  `docs/SPEC-ASYNC-SEMANTICS.md:303-309`).

**Cost.** Every memo that returns an object pays the probe (an `untrack` closure, two property lookups) on every
pass, async or not (`async.ts:466-471`). Per flight: five closures (`async.ts:537-846`), two `then` callbacks and a
derived promise (`853-891`), a `NotReadyError`, and a 17-field `NodeExtension` the first time a node needs one
(`core.ts:1293-1313`). Per landing: an updater closure, the settle walk's `Set` (`async.ts:153`) and one full
synchronous `flush()`. Two promises resolving in the same microtask produce two flushes; an N-hop chain produces N.

**What breaks.** No cancellation exists for promise flights (`src/core/types.ts:81-85`: "no cancellation hook
exists"; §12). Reads after the first `await` are untracked, and a `NotReadyError` from such a read leaves the node
pending forever in production (#2987, `async.ts:543-559`). Stream lifecycle needed four fixes: iterators not
closed when superseded (#3122), unobserved streams pumping forever (#2935), protocol throws escaping (#4112), a
pending lazy node never released (#2934). `_inFlight` doubles as "this body registered a flight", which forces a
repair in `onSupersede` (`async.ts:434-440`).

## 2. Pending propagation: how "not ready" travels through the graph

**Problem.** Everything downstream of an unready value must know it is not ready, so that it can hold, show a
fallback or keep its old value, and then recover when the value lands.

**Solid's answer.**
* A tracked read of a `STATUS_PENDING` node throws the node's stored `NotReadyError`, whose `.source` names the
  flight (`core.ts:2475-2515`). The reader's `recompute` catches it, keeps its old committed `_value` and its
  full dependency list, and calls `notifyStatus(el, STATUS_PENDING, e)` (`core.ts:670-694`, `822`, `1070-1077`).
* `notifyStatus` adds the source to the node's lazily created `_pendingSources` Set (`async.ts:69-73, 993`), sets
  status bits, and either reports to a boundary or the root (for an effect) or calls `propagateStatus`, which
  **marks every dependent transitively** without re-running it (`async.ts:978-1146`), stopping at effects,
  re-derive nodes and nodes already carrying the source.
* Landing runs the reverse walk: `settlePendingSource` visits the same cone depth-first with a visited `Set`,
  removes the entry, clears `STATUS_PENDING` where nothing remains and re-queues nodes that threw
  (`async.ts:273-402`). Value changes travel separately, through `insertSubs` and the height heap.

**Cost.** Going pending is O(edges in the dependent cone): every visited node gets `_time` written
(`async.ts:1044`), is pushed onto `pendingNodes` without deduplication (`src/core/scheduler.ts:1395-1399`) and
allocates one `Set` per pending episode. Landing walks the same cone again plus one ring, with a new `Set`. A
plain signal *read* pays five extra async/staging tests on the fast path (`core.ts:2225, 2245, 2246, 2262, 2270`);
a plain *write* about seven (`core.ts:2843, 2848, 2859, 2861-2862, 2894-2900, 2905`), and every write is staged
in `pendingNodes` and committed in a second pass.

**What breaks.** `_pendingSources` has at least seven retirement sites, each one a fix for a node left pending
forever: #2893, #3181, #3226, #3456, #4541 (`tests/pending-source-repark.test.ts`,
`tests/repark-drops-unreached-source-4541.test.ts`) and one found by the seat shadow (`async.ts:1080-1093`). Torn
frames from a stale first hop: #3373/#3376 committed `2 / 1`, a newer input beside an older derived value
(`tests/async-chain-supersession.test.ts`). The structural fragility is named in the code itself: status is
pushed eagerly through the cone while values travel through the heap, and recovery depends on the two channels
agreeing.

## 3. Holds: keeping the committed frame on screen while new values load

**Problem.** A write whose on-screen consequences need async must not show half-applied: the old, consistent frame
stays until the new one is complete, then switches in one step.

**Solid's answer.**
* **Two slots per node**, `_value` (committed) and `_pendingValue` (staged) (`src/core/types.ts:144, 165`). Every
  write stages (`core.ts:2880-2900`).
* **Parking.** At the flush's seam (`settle`, `scheduler.ts:1014-1205`) the flush commits, or parks if any pass
  "joined the future": a render effect went pending, a write hit a held node, a non-render pass read a held node,
  pending propagated onto a held memo, or a landing settled a held dependent (`scheduler.ts:1236-1262`,
  `core.ts:2848`, `core.ts:1326-1342`, `async.ts:1133-1138`, `async.ts:373`). Parking holds each staged node
  (`holdNode`, `scheduler.ts:637-650`), holds its child owners (`holdFrame`, `665-674`) and stashes the flush's
  effect runs into the transaction (`1183-1184`).
* **Which world a read sees** is decided at the read: a render effect, verdict reader or lane pass reading a held
  node is a *frame reader* and is served `_value` and recorded for a re-run at the landing (`frameRead`,
  `staleReader`, `core.ts:1376-1467`, `scheduler.ts:1388-1393`); a memo *joins* the hold instead
  (`joinPass`, `core.ts:1381-1383`); readers born into the transaction see its staging (`bornInto`,
  `core.ts:1474-1480`).
* **Landing.** Every seam walks all live transactions and lands each one that is not `blocked`
  (`scheduler.ts:1147-1157`): re-run the stale readers, commit every held node (`commitPendingNode`,
  `1543-1614`), release the stashed effect runs (`1329-1358`).

**Cost.** Two-phase staging on every write (§2). Parking allocates a 17-field extension per held node without one
and walks the full owned subtree of each staged computed (`holdFrame`). While T transactions are live, every seam
calls `blocked()` on each, restarting the scan after every landing (`k = transactions.length`,
`scheduler.ts:1156`): O(T²) calls, each walking all held nodes and one hop of subscribers (`562-586`) plus an
owner-chain walk for zombies (`598-613`) and a scan of every lane. Every stale reader re-runs at the landing even
if nothing it read changed (pinned `it.fails` in `tests/hold-recovers-landing-reruns-4469.test.ts`).

**What breaks.** The rule "which world does this read see" is spread over about ten deciders (`frameRead`,
`bornInto`, `joinPass`, `stagedScreen`, `stagedRead`, `readerSeesCommitted`, the read/serve arms, the recompute
head, `route`, `verdictValue`, `laneRead`), and a 1,774-line test-only shadow, `src/core/seat.ts`, exists to check
that they agree (`seat.ts:1-46`). Special cases for torn frames: #3305 `CONFIG_INPUTS_PUBLISHED` (a committed value
would tear against inputs already shown), #3463 zombies, #3494 a stale reader linked through a memo, #3469 kept
dependency tails (`heldTrims`), #3546 deferred zombies, #4089 cleanup writes during a landing, #4544
`CONFIG_AHEAD`. Nothing checks F-1 at runtime: INV-7 only looks for leaked stagings at quiescence under `__TEST__`
(`src/core/invariants.ts:86-100`).

## 4. Transactions: the unit of publication

**Problem.** Async results caused by one change, or by changes that overlap in the graph, must publish together;
unrelated changes must not wait for each other.

**Solid's answer.** A `Transaction` has 18 fields (`scheduler.ts:280-351`); live ones sit in a flat array (`354`).
**Membership is the tick's**: the synchronous work one flush settles joins one hold, and a frame that derives from
two futures waits on both (`docs/SPEC-ASYNC-SEMANTICS.md:49`). Transactions merge (`merge`, `scheduler.ts:454-470`;
union-find through `_into` with path compression, `426-438`) when a write hits a held slot (A34, `core.ts:2848`),
a non-render pass reads a held node, pending propagates onto a held memo (#3443), a re-ask reaches a held
observer, a guess lands over a held slot, or action slices continue. Render effects never entangle: a write that
reaches a transaction only through one is its own transaction (#3322, #3407, #3412). A transaction is complete when
no action is open in it, no held pending node is observed by an on-screen reader through a current link, and no
lane under it is blocked (`blocked`, `scheduler.ts:487-497`; `blockedBy`, `537-589`).

**Cost.** One object with five arrays per transaction (`358-373`); a merge copies the merged lists and splices the
array (O(T), `indexOf` at `433, 469`). Completeness is not recorded but *recomputed from links at every seam*: the
old reporter map (`_asyncReporters`) was removed with the L2 rewrite, so `blocked` re-derives observation each time.

**What breaks.** Merges never split: entangled once, entangled for good. Tick membership over-entangles: two
mounts in one tick merge their holds, and a hold waits on first loads of mounts in the same flush
(`docs/SPEC-ASYNC-SEMANTICS.md:49, 62-67`; `tests/tick-scoped-pass-transaction.test.ts`,
`tests/direction-rule-probe.test.ts`). A transaction observed by a flight that never lands holds its writes until
the reader leaves the screen (F-3). A lane re-based over one live frame cannot take a second; the code calls this
"the limit of this build" (`src/core/lanes.ts:930-934`).

## 5. Loading boundaries (Suspense)

**Problem.** A branch reading async data with no answer has nothing to show. Without a boundary the change that
mounted it is held, or the root attach is deferred. The author needs a local "not ready" display that does not
stall the rest of the screen.

**Solid's answer.** A boundary is an owner whose context names it (`src/boundaries.ts:730-733, 787-789`). Its
content is built immediately (eager computeds, `790-795`) and kept alive behind the fallback; the display is an
`output` computed flagged `CONFIG_REDERIVE` (`800-949`). A pending render effect calls `globalQueue.notify`, which
asks the boundary chain first (`effect.ts:147-148`; `scheduler.ts:1236-1237`); a *collecting* boundary records
the reader and shows its fallback, and the reader joins no transaction (`boundaries.ts:297-311`). A loading
boundary collects only before its first content (`isCollecting`: error type, `!_initialized`, `_armed`, or behind
an error fallback, `184-185`); after first content, pending is forwarded and the frame holds with the old content
visible (F-5). `<Loading on={x}>` re-arms the fallback when `x` changes (`arm`, `637-676`), walking the boundary's
whole owner subtree for held readers (`heldUnder`, `scheduler.ts:678-688`). A33: async caught by a fallback holds
nothing, in both orders (`hiddenBy` in `onScreen`, `scheduler.ts:590-600`; `boundaries.ts:323-347`).

**Cost.** Per boundary: an owner, three or four computeds, a reader `Map`, a copied context object, and a fallback
computed per show. Per pending notification: a walk up the boundary chain. Per `blocked()` judgment (every seam ×
every transaction × every candidate reader): `hiddenBy` walks the boundary chain, and for re-derive readers walks
subscribers transitively with a fresh `Set` (`boundaries.ts:338-346`). `prune` at every seam iterates readers and
may run an upstream DFS per reader (`401-405`); N readers clearing cost N²/2 prunes until #4526.

**What breaks.** #3540 (a fresh boundary over a held value joined the hold, or tore; one shape still pinned
`it.fails`, `docs/SCENARIOS.md:360, 376`); #4379 (a fallback staged into a hold committed with it); #4480 (the
fix for #4379 looped error boundaries forever, so it is restricted to loading boundaries, `boundaries.ts:527-531`);
#4568, #4593. Structurally unfixable: an `on` fallback can never be seen when the same source is read outside the
boundary (`boundaries.ts:553-613`), and one that loses a race to an action is silently not shown
(`packages/solid/src/client/flow.ts:491-494`). Boundary state is spread over seven fields and two node bits.

## 6. Reveal order (`<Reveal>`, formerly SuspenseList)

**Problem.** Sibling boundaries resolving in arbitrary order make content pop in out of order.

**Solid's answer.** A `RevealController` in context; direct child boundaries register as slots and start gated
(`src/reveal.ts:120-137, 195-220`; `boundaries.ts:752, 797-799`). `_evaluate` forces slot states for `sequential`,
`together` and `natural` orders (`reveal.ts:145-181`).

**Cost.** Any slot readiness change runs a full `_evaluate`: O(slots) `ready()` calls, each a `prune` over that
slot's readers, then two more O(slots) passes, recursing to the parent controller. Gated content is still built.

**What breaks.** `ready()` mutates (it calls `prune`, which deletes readers); re-entrant evaluations are silently
dropped (`reveal.ts:146`); the async spec places Reveal out of scope, so how gating interacts with holds is
inferred from code, not ruled (`docs/RULES-DERIVED.md:331`).

## 7. `isPending(x)` and `latest(x)`

**Problem.** Indicators and the newest value must appear next to a held screen, without making the hold wait and
without tearing against the value they describe.

**Solid's answer.** Both are read *windows*: they swap `read`'s dispatch to `verdictValue`
(`src/core/verdict.ts:364-376, 410-437`; `core.ts:2225-2237`) and stamp the pass a verdict reader (`339-359`).
Pending is decided from local node state at read time: `affects()` marks, lane nodes, held nodes still in flight
or staging a different value (`heldNotFinal`, `291-301`), staged-not-held nodes answered at the seam
(`provisionalVerdict`), flights (`pendingVerdict`, `304-332`). **Pending is question-scoped** (§5g of the internals
file): only a new question is pending; `refresh()` of an unchanged question is quiet (`REACTIVE_REASK`,
`core.ts:3064-3067`, `792`). The reader is moved into a lazily created *verdict lane* of the holder, shown now and
re-run at the landing (`verdict.ts:124-132, 219-223`). `latest` of a held node serves `_pendingValue`
(`heldLatest`, `447-452`). Plain reads in the same pass stay frame reads, so `[isPending(x), x()]` reads
`[true, stale]` (A10, `core.ts:2438-2444`).

**Cost.** Per window read: a link, a pull, a dispatch swap and about ten branch tests. Per transaction: a lazily
created lane (a `Transaction` object). A verdict reader re-derives on every pending or error change of a dependency
(`async.ts:1073-1097`) and again at the landing. Importing `verdict.ts` installs the lane engine
(`lanes.ts:29-31`): indicators are built on the optimistic machinery.

**What breaks.** `verdict.ts` cites 35 distinct issues (#4365 `latest` used to open a transaction, #4393, #4397,
#4408, #4544, #4648, #4661, #4677, #4689). `<Show when={latest(x)>0}>` mounting a boundary is an open ruling
(`docs/SPEC-ASYNC-SEMANTICS.md:60`).

## 8. Optimistic writes and rollback

**Problem.** Show a predicted value now while the real mutation runs; then confirm it, replace it with a differing
truth, or revert it on failure, without the guess and the truth tearing against each other.

**Solid's answer.** `createOptimistic` is a signal or memo whose setter is `optimisticWrite`
(`src/signals.ts:455-470`); a write pushes `[node, value, question]` onto `pendingGuesses` and is invisible until
the flush (`src/core/guesses.ts:111-125`). At the seam `applyGuesses` opens one *lane* (a `Transaction` with
`_lane` and a `_parent`) for the seam's guesses under the frame's transaction (`guesses.ts:135-191`); a guess with
nothing to be optimistic over is dropped (`164-165`). Display depends on the reader: untracked reads get the guess
(`laneRead`, `lanes.ts:652-655`), a render effect off the lane gets the screen while the lane is held
(`705-727`), a tracked derivation becomes lane work and is computed twice, once in the lane and once on the truth
("two flights for one input ... is the price", `guesses.ts:168-173`). A truth arriving under a guess is held
beneath it if its question is older (`landBeneath`, `lanes.ts:156-159`) and otherwise supersedes it
(`supersede`, `guesses.ts:216-250`). When the action's body has returned and no authoritative flight is up, each
guess is superseded by the truth beneath it: that is the revert (`laneCorrections`, `guesses.ts:260-283`;
`truth.ts:44-46`). Provenance is a global `question` counter stamped on flights and guesses
(`scheduler.ts:770-778`; `async.ts:521`; `stale`, `guesses.ts:334-336`).

**Cost.** Per optimistic write a backward linear scan of pending guesses (`guesses.ts:71-82`): O(G) per write,
O(G²) per tick. Per seam with guesses: a lane object with five arrays and two subscriber walks
(`scheduler.ts:1468-1518`). Every seam while lanes live copies the lane list and may add a heap round. Every
derivation of a guessed value runs twice. Bookkeeping in `WeakMap`s (`covers`) and `WeakSet`s (`marked`).

**What breaks.** lanes.ts cites 47 distinct issues; `tests/fuzz-findings-l2.test.ts:435` is `it.fails` (a stale
reader shows an unrevealed re-guess); `KNOWN_FAILURES` rows in
`tests/store/optimistic-list-mutation-matrix.test.ts:53-90`; the positional residue of a store re-base on unkeyed
lists is "documented, not ruled" (`docs/SPEC-ASYNC-SEMANTICS.md:41`). As far as the code shows, a thrown action
body only decrements `_open` (`src/core/action.ts:194-200`): plain writes staged in its transaction still commit;
only guesses revert.

## 9. Actions

**Problem.** A mutation whose writes span async gaps should commit as one unit, with optimistic UI meanwhile.

**Solid's answer.** `action(function*)` (sync or async generator, `action.ts:129-131`) opens a transaction held
while its body runs (`_open = 1`, `158-161`; `blocked` while `_open !== 0`, `scheduler.ts:494`). Each resumed
slice re-enters it and flushes (`inTransaction`, `action.ts:40-50`). `refresh(x)` re-asks quietly
(`core.ts:3036-3074`; `signals.ts:952-989`).

**Cost.** Per call a Promise, a generator, a transaction and three closures; one full flush per resolved `yield`.

**What breaks.** Code after an `await` and before the next `yield` runs outside the transaction (A26,
`action.ts:74-86`, `tests/action-await-contract.test.ts`); `await refresh(x)` inside a helper the action yields
can hang (#4263, `action.ts:96-100`).

## 10. Errors

**Problem.** A throw must replace a subtree with error UI and offer a retry, without halting the app or leaving
half-rendered content; an error must not publish ahead of the frame it belongs to.

**Solid's answer.** `<Errored>` is the same boundary with `_type = STATUS_ERROR`, always collecting
(`boundaries.ts:739, 748, 1093-1098`). A throw becomes `STATUS_ERROR` carrying `StatusError(source, original)`
(`async.ts:978-985`; `src/core/error.ts:51-63`) and rides graph edges as a mark. Pending passes an error boundary
by and an error passes a loading boundary by (`boundaries.ts:297-304, 859-861`). **D-41, "an error is an answer"**:
a throw on a held node is staged like a value (`CONFIG_STAGED_ERROR`, `stageError`, `async.ts:217-256`), ends the
wait, and shows when a value would have; a shown error stays until the frame carrying its replacement reveals
(`_shownError`, `async.ts:991-992`). `reset()` recomputes each distinct source (`boundaries.ts:681-686`). A throw in
an action body rejects the action's promise at the `yield` and never reaches a boundary (`action.ts:192-199`).

**Cost.** As Loading, plus a signal; `reportClientError` walks the owner chain twice per error object
(`src/core/error-hooks.ts:77-93`); a `_shownError` test on every `setSignal` and `recompute`
(`core.ts:2862, 863`).

**What breaks.** #4592 (error fallback replaced by content with a stale reader), #4526 (a held re-map revealed
pre-error rows), #4480 (infinite flush loop), #4373, #4644, #4270, #3886 (the error arm ran before its frame:
fixed by staging). Error state spans six places: `_statusFlags`, `_error`, `_shownError`, `CONFIG_STAGED_ERROR`,
`_pendingValue`, and the boundary's reader map.

## 11. Effects under async

**Problem.** Side effects must not see values the screen does not show.

**Solid's answer.** Only render effects are readers that hold (`effect.ts:147`; `docs/RULES-FUNDAMENTALS.md:26`).
A pending computation queues no run (`core.ts:889-914`). A parked flush appends all of its runs to the transaction
(`scheduler.ts:1167-1193`); runs behind a fallback are stashed per boundary and released at the reveal
(`effect.ts:186-198`; `boundaries.ts:349-382`). User effects never run with a held or hidden value.

**Cost.** Unrelated effect runs of a parked flush wait for the whole hold (tick membership, D-4).

**What breaks.** #3291, #4606/#4607 (tracked effects with held dependencies), #4343, #3411 (`onSettled` read a
reverted optimistic value beside a stale memo); `createTrackedEffect` is deprecated because it may run several
times per change and tear (`signals.ts:655-667`).

## 12. Cancellation and race ordering

**Problem.** When an input changes while a request is in the air, the older answer must not overwrite the newer
one, and the older work should stop.

**Solid's answer.** No `AbortSignal` and no promise cancellation (`AbortSignal` appears only as an option of
`until()`, `signals.ts:1005-1024`). Superseding is the identity check of §1: the next pass nulls `_inFlight`, every
old callback fails the comparison. Iterators are closed at that moment (`_flightTeardown` → `it.return()`,
`core.ts:478`; #3122). The user's own cancellation is `onCleanup(() => ac.abort())` registered before the first
`await`. A landing after a newer write but before its flush is dropped only if the node is `REACTIVE_DIRTY`
(`async.ts:604-607`); otherwise "answered, then re-asked" in the same flush (D-22/#4495). A → B → A is not
recognised: the first A answer is thrown away although it answers the current question, and the re-asked A is a
new (loud) question, not a quiet re-ask.

**Cost.** Superseded requests and their bodies run to completion, side effects after `await` included; only the
answer is dropped. Every re-ask is a full refetch.

**What breaks.** Wasted work under rapid input changes; #3373/#3376 and the #3374 remount cases. Provenance for
optimistic guesses rests on a global `question` scoped correctly across `await` continuations, which only action
slices and projections set.

## 13. Deferred values, declared motion, quiet re-asks

* **`createDeferred`** (`src/deferred.ts:93-195`) answers "independent panels over one selection all waited for the
  slowest fetch" (`docs/create-deferred.md:14-27`): after its first answer every pass serves the committed value
  instead of throwing, so readers hold nothing, and a mark makes `isPending` true. The cost: lag is not atomic;
  two deferred nodes over one input can show different eras, "the tear is bounded, not removed"
  (`create-deferred.md:295-313`).
* **`affects(x)`** (`src/affects.ts`, `src/affects-marks.ts`) declares that a mutation will change `x`, so
  `isPending(x)` is true before any flight exists. Marking walks all transitive subscribers; while any mark lives,
  every probe allocates a `Set` and walks the dependency graph.
* **Quiet re-ask** (`REACTIVE_REASK`): refreshing an unchanged question shows no pending state; the flag is lost if
  the node was already dirty (`core.ts:3067`).

---

## What it costs in aggregate

| measure | value | where |
|---|---|---|
| node flags | 24 `REACTIVE_*`, 32 `CONFIG_*`, 5 `STATUS_*` | `src/core/constants.ts` |
| `Transaction` fields | 18 | `scheduler.ts:280-351` |
| "which world does this read see" deciders | about 10, cross-checked by a 1,774-line test-only shadow | `src/core/seat.ts:1-46` |
| distinct issue numbers cited in code | 84 `core.ts`, 33 `async.ts`, 50 `scheduler.ts`, 47 `lanes.ts`, 35 `verdict.ts` | grep `#[0-9]{3,4}` |
| test files | 489, of which 179 are named after an issue; 23 `it.fails` in 7 files | `tests/` |
| design rules | 424 rule ids across 15 vocabularies | `docs/RULES-INDEX.md:24-42` |
| design notes | 7,971 lines in `docs/*.md` (1.07 MB) | `docs/` |
| async/transaction source | about 7,600 lines in the core async files alone | see the per-section files |
| plain read / plain write | +5 / +7 tests on the fast path; every write staged and committed in a second pass | §2 |

Ten wrong frames that taught the most (each became a special case; each is a shape bough's design must make
unrepresentable rather than patch):

| issue | wrong frame | fix |
|---|---|---|
| #3408 | `Count 0 \| Details 0 \| Panel 1` | a memo that starts reading a held value joins its hold |
| #3473 | `A: 1` published beside held `B: 0` | a write to a held node joins the hold |
| #3375 | an `on` reset that never releases | readers behind a fallback are off screen |
| #3459 | `B 1 \| Fast 1 \| Slow 0` | `arm` collects forwarded readers (already-pending readers never re-notify) |
| #3540 | `count 1` beside `Count 0`, or a blank slot | first-pass exemption for a fresh boundary over a held read |
| #3869 | `x 0 \| x 1` | content mounted under `latest(x)` is born held |
| #3811 | header `1` beside details `0` | a shown boundary's pending output counts as a frame reader |
| #3886 | `1 \| Error 2 \| 1`, error arm already run | errors staged in the hold (D-41) |
| #3373 | `2 / 1`: newer input beside older derived value | `landStatus` keeps a node pending on a re-asked input |
| #4593 | `a=10 s=0 b=21` | a fresh boundary records held dependencies of its readers |

## What bough takes, and what it does not

**The problems and principles are right, and bough keeps them.**
* F-1, the whole screen: no frame shows a value beside a stale derivation of it, or a derivation ahead of its input.
* Fetch high, block low: work starts where the input is written; waiting is felt only by what shows the answer.
* Latency has three shapes: a question (latest wins, stale answers are worthless), an action (a lost request is a
  bug), a stream (the sequence is the meaning).
* First readiness is not revalidation: after a first answer, a refresh keeps the old content up.
* An error is an answer: it ends the wait and appears when a value would have.
* A write that nets back to what is shown is no proposal (A34(2)); a re-ask of an unchanged question is quiet.

**The machinery comes from three facts of Solid's world that bough does not share.**
1. *The graph is discovered at run time.* Solid learns who depends on a pending value by running them, so pending
   must be pushed through the cone (§2), completeness re-derived from links at every seam (§4), and boundary
   membership found by walking owners (§5). In bough the dependents of a cell are its wires, installed at mount
   from `offset_of!` proofs; which cells feed which questions is known at mount.
2. *A read decides which world it sees.* Ten deciders and a shadow table exist because the same `read()` serves a
   handler, a render effect, a memo joining a hold, a lane, and a verdict window (§3). In bough the phase of the
   turn decides: a handler, a wire, a description and a landing run at different points of `App::turn`, so the
   world can be selected once per phase instead of once per read.
3. *JavaScript has no ownership.* A promise cannot be dropped, so superseded work runs to completion (§12) and
   cleanup is a hook the author must remember. In Rust, dropping the future is the cancellation, and the
   flight's owner is a value with a scope.

The bough design that follows from this is sketched in `HANDOFF.md` ("Design, as far as it got"); it was not
built in this session.

---

*Note on renamed mechanisms (from the internals file's pre-L2 vocabulary to the running code):* `initTransition`
/ `activeTransition` → `newTransaction`, `joinFuture`, `passTx` (`scheduler.ts:358-450`); `_asyncReporters` /
`transitionComplete` → `blocked()` / `blockedBy()` re-walking links at every seam (`487-589`); `stashQueues` →
`t._queues` with `releaseQueues` (`1183-1184`, `1364-1369`); `heldFromStale` / `_gatedSubs` → `frameRead` +
`staleReader` → `t._reruns`; the `isPending`/`latest` companions → verdict windows and verdict lanes (no companions
exist, `docs/SPEC-ASYNC-SEMANTICS.md:99`); `endOptimism` → `laneCorrections`; `origin` / `_overrideStamp` → the
`question` counter and `_x._q`.
