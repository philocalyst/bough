# Runtime Scheduler

The scheduler serves the retained plan in [../03-hybrid-plan.md](../03-hybrid-plan.md). It is a
single-writer, host-driven graph with dynamic dependencies, explicit commit phases, equality
cutoffs, and priority lanes. It does not use universal MVCC or type-level durability.

## Core rules

1. The graph belongs to the UI runtime and is `!Send + !Sync`; `State<T>` is only a two-word,
   generational, `Copy`, `!Send + !Sync` key into it.
2. A transaction writes isolated field drafts or typed delta logs. No API mutates the published
   value through arbitrary user code. Returning normally contributes the draft to a sparse pending
   epoch; unwinding drops it. It never calls reactive user code recursively.
3. Every arbitrary derived computation records the exact source/derived keys it read. Recompute diffs old and
   new input sets and maintains reverse edges.
4. Consumers run only after their inputs settle. Equality policy stops propagation.
5. Source drafts, derived outputs, structure, and ports publish atomically as one logical epoch.
   Style/layout/hit/a11y/scene publish as a second presentation epoch. User effects run only after
   logical publication and can start a later transaction, never mutate an epoch being committed.

## Keys, authority, and drafts

`State<T>`, row fields, derived outputs, and document handles share one representation: a runtime id
plus generational slot key and a zero-sized type/locality marker. They are `Copy` without requiring
`T: Copy`, remain `!Send + !Sync`, own nothing, and cannot dereference themselves. `Scope::initial`,
`Update::get/with/prior`, and dynamic `Read::get/with` supply the actual runtime reference and validate
runtime/owner/slot generations. A captured key in layout or paint is inert because those phases have
no resolving capability. There is no TLS lookup, weak pointer, refcount, phase guard, or ambient
`get`/`set`.

Generated models are contiguous groups of ordinary field slots. Exact field writes therefore mark
that slot's direct observers immediately—no projection node or field-mask routing. An aggregate
descriptor can assemble/scatter a whole model only for explicit snapshots/replacements/drafts;
opaque aggregate publication is conservatively wildcard at the group boundary.

`Update::set(key, value)` moves a complete replacement into the sparse pending epoch. `Update::edit` first
creates an isolated draft according to the source's declared policy (`Copy`, `Clone`, persistent/COW,
or generated delta draft), invokes user code on that draft, and records it only on normal return.
Large text, lists, and documents expose typed delta builders rather than clone-and-edit. Types with
no rollback-capable policy simply do not implement `EditDraft`; they still support replacement and
their specialized deltas. A panic cannot partially mutate published source storage, so poisoning and
repair machinery are unnecessary on this path.

Within a handler, `update.get(key)` observes its pending draft (read-your-writes), while
`update.prior(key)` observes the committed value from before the update—the pre-update value for a
source, the last published output for a derived key—and never executes user code. Derived keys
passed to `update.get` may demand-stabilize pending prerequisites and execute deterministic,
idempotent, externally side-effect-free user code; `update.try_get` is the fallible form when the
target may be poisoned. Dynamic bodies receive inferred `Read`; only `read.get/with(key)` records
dynamic edges. Same-source conflicting draft borrows fail immediately with acquisition sites; independent
slots—including fields of the same generated model—can be accessed independently.

An unobserved derived node is stale but unscheduled. Reading it or attaching a live consumer
validates/recomputes its prerequisite closure. Direct state/field sinks never enter this graph.

## Atomic publication

“Keep the previous frame on fault” is implemented, not aspirational. The runtime never edits the
published retained epoch in place. Each turn owns a sparse `LogicalDraft` containing only changed
source cells, derived outputs/edges, structural splices, port values, and owner lifecycle records.
Unchanged records are referenced from the published epoch. Successful settle validates the draft,
then swaps one logical epoch pointer/index; a computation/equality/reconcile/budget fault drops the
draft and leaves source revisions, structure, ports, and event ownership unchanged.

Style/layout/hit/a11y/scene consume that logical epoch into a separate sparse
`PresentationDraft`. Changed layout/scene records use shadow slots or reversible range journals;
unchanged records remain shared.

Taffy is the one foreign mutable structure inside that draft, and its reversibility is an explicit
mechanism, not an implication: the draft records an inverse-operation journal
(`set_style`/`set_children`/insert/remove with prior values); a discard replays inverses and marks
touched nodes dirty, validated by the from-scratch relayout oracle. If the journal exceeds its byte
budget, discard degrades to conservatively marking the affected subtree dirty for recomputation on
the next attempt—correct, counted, slower. Neither path lets a discarded draft leave Taffy
disagreeing with the published presentation epoch. A successful geometry-bounded pass atomically advances the
interaction/presentation epoch before submission. A later GPU submission fault keeps the last
presented target and retries from the completed CPU presentation packet. A presentation-stage
fault does not roll back already published application state; it keeps the prior coherent
interaction/presentation epoch, pins owners referenced by it, queues coordinate-dependent input,
and retries or enters a diagnosed fallback. This explicit two-epoch rule avoids claiming that one
pointer swap can roll back external user state and GPU work.

Sparse overlay bytes, copied records, journal entries, pinned old epochs, queued-input delay, and
publication swaps are counters and benchmark gates. Pinning has a hard per-window cap with a stated
escalation: a window that cannot present (occlusion, device loss) does not pin unbounded history;
past the cap it detaches presentation demand, drops to a conservative full re-present on recovery,
and counts the escalation. Dense mutation promotes to a contiguous double-buffered packet when that
is cheaper. Full-copy MVCC for every value is not required.

## Host protocol

```text
accept one event or task completion
  -> handler receives `Update`; keys resolve only through it; writes create isolated drafts
  -> write-incapable tracked/dynamic computations
  -> reconcile structure into the logical draft
  -> install/initialize properties introduced by structure
  -> compact direct/field/sealed-circuit property sinks
  -> validate and atomically publish the logical epoch
  -> run after-publication effects; their writes enqueue a new transaction

before drawing
  -> consume the newest logical epoch into a sparse presentation draft
  -> style -> layout
  -> restricted geometry feedback (sealed layout-only cells)
  -> relayout only if feedback wrote and the geometry cap remains
  -> hit/a11y/prepaint/paint/composite packet
  -> atomically publish interaction/presentation epoch
  -> bulk renderer-frontend lowering -> submit immutable FramePacket -> retain through fence
```

Settle between separately queued input events. Otherwise a second click/key event could target a
tree that should have been removed by the first. Writes inside one event remain implicitly batched.

Focus and key routing are logical-epoch state: the second key event routes against the focus
committed by the first event's logical publication even while a presentation fault is pending—only
coordinate-dependent input waits on presentation recovery. Unmounting the focused branch resolves
focus inside the same logical publication via the focus-scope stack (declared restore target, else
scope owner, else window default), so no epoch ever publishes focus on a dead node and
accessibility observes the resolved focus atomically.

A `LegacyView` island rebuild runs at the structural-draft stage with an `Update`-equivalent
capability. Legacy code that mutates state during its own rebuild becomes an after-publication
transaction under the same per-turn budget; the island cannot observe the half-reconciled retained
tree outside its boundary.

After-publication effects cannot invalidate the epoch that triggered them; a write becomes a new
transaction, which the host may settle in the same turn under its ordinary transaction budget.
Effect transactions of event N settle before event N+1 dispatches: they are causally part of event
N, so an effect that moves focus wins over an already-queued keypress. A
write after layout begins is impossible except through `LayoutFeedback`. Its write target is the
private half of a sealed geometry-feedback cell; the read half can feed only whitelisted layout/
anchor ports and cannot enter general properties, derived computations, structure, style, effects,
senders, or resources. Each edit consumes the geometry budget and relayouts the presentation draft.
Exhaustion discards that draft, preserves the prior interaction/presentation epoch, and reports the
declaration chain.

Property sinks are not user effects: a sink can only patch a sealed retained port and emit the
port's reviewed obligation. Sealed static circuits are framework-owned operators with known
purity, equality, and fault policy and may fuse into such a patch. An arbitrary `map(f)` remains a
fixed-edge user computation; an arbitrary `scope.memo` closure tracks dynamic dependencies.
Neither is fused merely because it is small. Dynamic computation bodies are write-incapable,
deterministic, idempotent, and externally side-effect-free because demand/retry can evaluate them
more than once. Shared computeds run once per settle before their
fanout sinks. User effects see only a successful source/structure/port commit and can queue later
work.

## Graph ordering

Each node stores a topological height, current inputs, reverse observers, revision, dirty epoch,
owner, equality policy, source location, and priority. When recomputation discovers a new edge:

- an edge to a node currently on the computation stack is a cycle;
- otherwise, if the edge violates height order, raise the consumer and affected downstream nodes;
- commit the input-set diff only after the computation returns successfully.

The internal `ensure_current` path—used only by `Update::get`, dynamic `Read`, and property settle—
first makes a stale target's prerequisites current,
recomputes it, and applies its equality cutoff before returning. The active stack detects a cycle.
This pull-before-read rule is the correctness backstop for a newly discovered dynamic edge: height
ordering efficiently settles the known dirty graph, but it is never allowed to return a stale node
merely because that node was unobserved or absent from the heap when the flush began.

Direct source/field sinks do not enter this dynamic scheduling machinery: their subscriber records
are dense slab entries with a validated destination port and patch thunk. Mounting one has a target
of no heap allocation after its owner/slabs have capacity. Reverse observer links carry removal
metadata, so unmount is O(1) or amortized O(1) and never scans every source; bounded tombstone
compaction is separately counted. Generations do not silently wrap into validity: wrap causes a
new epoch/terminal diagnostic before an old public handle could alias a new slot.

Dirty nodes run in increasing height. Common small heights use indexed buckets; sparse/high heights
fall back to a heap. A queued bit prevents duplicates. Dynamic rewiring can make height repair more
than constant time; counters expose repaired nodes, heap operations, and dirty nodes popped. If
profiling favors it, the implementation may instead collect the dirty induced subgraph and
topologically sort it. The public contract is glitch-free ordering, not a specific queue.

A computation observes one value from each dependency within a settle epoch. Because writes and
computation occur on one thread and paint begins after commit, a frame cannot tear between graph
versions. This is the useful guarantee the earlier MVCC proposal sought, achieved without version
chains for every local value.

An event handler reading a source after writing it sees the draft value immediately. Reading a dirty
derived key from an `Update` demand-stabilizes the write-incapable prerequisite subgraph before returning;
the write itself still executed no reactive user code. `update.prior(key)` is the explicit
committed-value escape: the pre-update value for a source, the non-executing last published output
for a derived key. On a derived key that has never published, it returns the node's declared
initial/`None` projection rather than executing user code. Structural commands and effects remain
deferred until the commit boundary, so demand stabilization cannot expose a half-reconciled tree.

`update.prior` can be expensive: it may clone/retain the old value or ask a revisioned model for a
snapshot. It is not placed on ordinary direct-sink paths. A port classifier needing old/new values
declares and measures that cost; a conservative obligation is preferable to hidden old-model
materialization when it wins in a workload.

## Cycles and runaway work

A dependency cycle is created while a computation reads another node, not when an unrelated input
is written. The active computation stack detects direct and dynamic cycles and reports the full path
with definitions and current recompute sites.

Cycles routed through after-publication transactions or layout observations are not graph cycles. A per-turn
budget covers:

- computation runs;
- structural reconciliation passes;
- after-publication transactions;
- geometry-observation rounds.

Debug/tests fail with a trace. Production preserves the last fully committed frame, emits one
diagnostic/telemetry event, and follows a bounded retry policy. It never paints a knowingly
half-settled tree merely because a numeric budget was exhausted.

## Equality and revision semantics

Node equality is selected when the node is created:

- `Eq(fn(&T, &T) -> bool)` for value equality;
- `Revision(fn(&T) -> Revision)` for buffers/models;
- `Identity` for stable handles/shared allocations;
- `Always` for opaque or time-varying values.

Changing a source revision marks observers. Recomputing a derived node increments its output
revision only when its equality policy says the result changed. A consumer that detached while
offscreen can reattach by comparing revisions and recomputing only if an input advanced.

The runtime records fanout and `Always` propagation. Devtools should make a broad always-dirty node
obvious rather than hiding it behind a generic effect flame.

## Priority and starvation

Priority lanes are `Input`, `Animation`, `Normal`, and `Idle`. Within the graph, dependency order
wins over priority: a high-priority consumer cannot run before its normal-priority prerequisite.
Priority selects among independent ready nodes and among host commands.

Idle work has a time slice and an age. Once aged past a threshold it is promoted, unless its owner is
unobserved and explicitly suspendable. Input is never blocked behind cache warming. Counters track
queue delay, promotions, deadline misses, and discarded owner work.

Durable/session/volatile type parameters are unnecessary. Reverse edges already limit traversal to
observers of the changed source. Theme nodes are cheap on pointer frames because they are not dirty,
not because every generic UI value carries a durability lattice.

## Owners, disposal, and reentrancy

Every node, observer, effect, listener, resource, and task has an owner epoch. Teardown first revokes
the epoch; queued work checks it before executing. Physical arena slots are reclaimed after the
current dispatch/settle epoch, preventing teardown callbacks from reentering a half-freed owner.

Cache eviction is not teardown but follows the same protocol: evicting a `keep_alive` branch or
recycling a virtualized row that holds an active composition or pointer capture first sends the
exactly-once platform cancellation (composition unmark/end, capture release) — and the retention
reasons of the virtualizer veto eviction while the protocol is active, so a composing or captured
subtree is unevictable until released.

Generational access has fallible and panicking forms. Framework internals use fallible access where
stale work is expected. Panics include definition, disposal, and access locations. Local storage may
use `RefCell`/carefully audited unsafe cells, but a same-thread conflicting borrow must fail
immediately; no `RwLock` is used on the local path.

## Cross-thread work edges and raw bridges

The default one-input/one-result boundary is `scope.compute(source, function)` for CPU work or
`scope.resource(source, async_function)` for I/O. A sealed `WorkInput` with an associated owned type
freezes the whole input against the pending logical view; only worker methods require that owned
value/result to be `Send + 'static`. One owner record schedules work and backs a copyable
`Work<T, E>` handle with coherent read-only value/pending/error projections, cancels queued work,
and rejects stale/late results by work generation, owner epoch, request ticket, exact input stamps,
and policy generation. This is layered on
GPUI's existing background `Task`, foreground wake, and owner epoch machinery. It does not promise
preemption or cooperative cancellation of already-running synchronous CPU work unless the explicit
token-polling form is used. `scope.input(source, freeze_fn)` is the application extension point;
downstream types do not implement the private scheduler trait.

`scope.action(function)` covers event-triggered reliable/latest/coalescing input. An imperative
one-shot can use `scope.spawn(future).then(|update, output| ...)`. Both preserve ordinary Rust functions
and futures while keeping transport and owner routing internal.

`Sender<T>` is the lower-level boundary for a genuine stream, external service, or independently
owned producer. It is `Send + Sync + 'static`, holds no borrow into UI storage,
queues an owned message tagged with destination owner epoch, and invokes an idempotent non-blocking
`FrameWaker`. FIFO is preserved per sender; after racing enqueue operations linearize, the queue
assigns a traceable total order across senders. It does not promise reproducible ordering between
concurrent producers. Multiple sends coalesce the wake, not reliable messages.

The cross-sender sequence is the queue's linearization order, not a promise about producer
wall-clock order. Racing producers that need domain order use one sender or carry a domain
sequence/revision. Reliable, latest, and coalescing policies therefore have intentionally distinct
ordering semantics. Accepted ownership still does not mean applied work: owner revocation before
drain discards the tagged message as documented below.

Capacity/policy is explicit: bounded reliable `try_send` returns `Full`; async `send_async` waits
without blocking the UI thread; `latest(key)` replaces an undelivered value; `coalescing` combines
domain deltas; and explicitly lossy feeds count drops. An unbounded queue is not the default. A
successful send means the queue accepted ownership, not that the UI applied it: closure may win the
subsequent race, in which case delivery rejects the owner epoch and increments a closed-drop
counter. Reliable delivery is prefix-loss-only per sender: revocation discards a suffix of that
sender's FIFO, never an interior message, so a multi-message protocol can rely on gap-free
prefixes. A send that observes the already-revoked epoch returns `SendError<T>` with its payload.
`close_and_drain` is a separate synchronization operation for callers that truly require
acknowledgement. Tests force every enqueue/revoke/drain interleaving.

When no durable work edge exists, advanced code may manually create an immutable snapshot and raw
bridge. This is an escape hatch, not the canonical indexing/resource API:

```rust
let input: Snapshot<SortInput> = scope.snapshot((document, sort_spec), |(doc, spec)| SortInput {
    document: doc.immutable_snapshot(),
    spec: spec.clone(),
});
let tx = scope.sender(results);
worker.spawn(move || tx.send(sort(input)));
```

`Snapshot<T>` owns cloned/`Arc`/persistent data and contains no graph handle, retained-node
reference, platform object, or borrow; moving it to a worker requires `T: Send + 'static` (sharing
it requires `Sync`). The result carries source revisions. Application chooses `AcceptLatest`,
`RejectStale`, `Rebase`, or a domain merge closure. This is explicit because conflict semantics are
application semantics.

Framework-owned pure stages may similarly lower a large dirty batch into an immutable owned stage
snapshot and use workers above an adaptive threshold. This does not make graph handles `Send`.
Commit validates node generation, owner epoch, and input revision; required frame work joins before
the next stage, and stale/speculative results are discarded. Small batches remain local to avoid
queue/join overhead.

## Media delivery and presentation deadlines

Media uses the same owner-targeted wake mechanism but not one FIFO policy for every stage. Metadata,
errors and explicit playback commands are reliable; acquisition/decode status is latest by source/
seek generation; decoded presentation frames are latest-valid by media timestamp and dependency.
All queues are bounded by bytes and, for timed media, duration. A slow UI or renderer applies
backpressure to acquisition/decoding or drops only frames permitted by codec/disposal policy. It
never accumulates an unbounded frame history merely because a generic sender is reliable.

A media completion validates owner epoch, source generation, seek generation, exact input stamps and
renderer device generation as applicable. Successful validation patches the retained media
resource/fragment and schedules its damage; it does not mutate a public image value or poll a task
from paint. A rejected old completion cannot clear pending/error state belonging to the new
generation. Native frame leases remain owned by the in-flight `FramePacket` until the backend fence
retires and then return exactly once to their decoder/pool.

The injected clock shared with animation supplies media presentation deadlines. Video with audio
uses its declared A/V master policy, translated into scheduler deadlines; hidden-window throttling,
rate changes and seeks are explicit clock inputs. A frame deadline can request presentation without
creating a source-state update or general graph effect. See [media.md](media.md) for acquisition,
decode, import and fallback contracts.

## Observability

After atomic presentation publication, the runtime may emit an immutable epoch-tagged `UiSnapshot`
or delta for the inspector/test protocol. It never snapshots a half-settled logical or presentation
draft. Inspector/test input re-enters the normal platform dispatcher and therefore exercises hit
testing, capture, focus, IME, event routes, `Update`, settlement, and presentation; it cannot call a
stored handler directly. An injected clock and seeded executor make timers, motion, and work
completion controllable. Snapshot history and detailed traces are opt-in and byte-budgeted; when
disabled, no shadow tree is maintained.

The inspector exposes:

- current dependency and owner graphs;
- state write -> computation -> obligation chains;
- per-node run/skip/equality/fanout/revision/priority data;
- height repairs, queue waits, budget rounds, stale owner drops, and wakes;
- media source/seek/device generations, bounded queue pressure, chosen frame timestamps, import/
  copy paths, cache lineage, leases and fallback reasons, with private request data redacted;
- “why did this property/phase run?” from source location to retained node.

Deterministic tests can pause after any protocol step and assert graph, commands, obligations, and
frame output without a platform window.
