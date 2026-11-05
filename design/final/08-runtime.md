# 08 — Runtime

Scope: the mechanical model underneath the authoring surface — the turn pipeline, the scheduler's
graph/publication rules, the measured data-structure choices, the memory ledger, the fast-path
mechanisms that reach and beat the zgui competitive bar, const subtree templates, and the
by-construction performance claim. Everything named `Scope`/`Update`/`Read`/`State<T>` in this
document is **kernel-internal** — application code never spells it; it exists to make the public
surface's laws (00 §4) true, not to extend it.

Depends on: [00-model.md](00-model.md) L1, L2, L14; [05-async.md](05-async.md) §2 (the wake law).
Feeds: [10-quality.md](10-quality.md) (every gate in this document's §5 is a required CI job);
[11-roadmap.md](11-roadmap.md) (phase-by-phase delivery of everything here).

---

## 1 · The turn pipeline

Every dispatched input, batch of stamped completions, remote message, or test action begins one
turn:

```text
validate event context
  -> run one handler to completion
  -> drain returned outputs and queued entity sends FIFO
  -> drain StampedCompletion records; validate generation + request stamps
  -> mark their ordinary memo nodes dirty; apply admitted values/faults
  -> speculatively describe affected regions against one immutable candidate
  -> seal producer sites + pending/fault verdicts; only now enqueue owned work
  -> settle each ready cohort / retain prior regions / select first-load fallbacks
  -> publish every eligible structure/property cohort atomically
  -> layout / hit / accessibility / paint / composite one epoch
```

User code cannot settle recursively: a route may enqueue another route but cannot execute it
inside the mutation that produced it. After a handler finishes, one description/projection
invocation sees one immutable borrow of current model intent plus one internally consistent set
of reader versions; it never observes a half-published worker result. Dispatch and hit-testing
see one committed presentation epoch. Speculative description is necessary, not optional
cleverness: a write may select a branch whose producer did not exist in the old tree, so the
kernel must describe that candidate branch to discover and arm its work without first showing an
inconsistent half-tree — the borrowed frame (02 §1) still does not survive it: the draft is
diffed and promoted into a private owned candidate tape before the arena drops.

A panic during immutable description/projection discards that unpublished draft and records a
fault at the narrowest boundary (05 §6). A panic during an arbitrary mutable handler has no
general rollback: queued effects are suppressed and its owner is poisoned or dropped unless the
state uses an explicit transactional representation (the `History` `commit(...)` builder, 01
§10.2, is exactly that representation where it applies).

> **Effect causality.** After-publication effects cannot invalidate the epoch that triggered
> them; a write from an effect becomes a new transaction. Effect transactions of event *N*
> settle before event *N+1* dispatches — they are causally part of event *N* — so an effect that
> moves focus wins over an already-queued keypress. This is what makes `changed`-driven focus
> claims deterministic under input bursts.

## 2 · Atomic publication

The runtime never edits the published retained epoch in place. Each turn owns a sparse
`LogicalDraft` of changed source cells, derived outputs/edges, structural splices, port values,
and owner lifecycle records; unchanged records are referenced from the published epoch. Successful
settle validates the draft, then swaps one logical epoch pointer; a computation/equality/
reconcile/budget fault drops the draft and leaves source revisions, structure, ports, and event
ownership unchanged. Style/layout/hit/a11y/scene consume that logical epoch into a separate
sparse `PresentationDraft`; a presentation-stage fault does not roll back already-published
application state — it keeps the prior coherent interaction/frame, pins the owners it referenced,
and retries or enters a diagnosed fallback (05 §6 is this same two-epoch law applied to async
specifically). Pinning has a hard per-window cap with a stated escalation: a window that cannot
present does not pin unbounded history, and a full re-present on recovery is counted, not hidden.

The layout engine is the one foreign mutable structure inside the presentation draft, and its
reversibility is a mechanism, not an implication: the draft records an **inverse-operation
journal** (`set_style`/`set_children`/insert/remove with prior values); a discard replays the
inverses and marks touched nodes dirty, validated by the from-scratch relayout oracle. If the
journal exceeds its byte budget, discard degrades to conservatively marking the affected subtree
dirty — correct, counted, slower. Neither path lets a discarded draft leave the layout tree
disagreeing with the published presentation epoch.

Focus and key routing are logical-epoch state: a second key event routes against the focus
committed by the first event's logical publication even while a presentation fault is pending —
only coordinate-dependent input waits on presentation recovery. Unmounting the focused branch
resolves focus inside the same logical publication (04 §4's fallback chain), so no epoch ever
publishes focus on a dead node.

## 3 · Graph ordering, equality, and priority

Each node stores a topological height, current inputs, reverse observers, revision, dirty epoch,
owner, equality policy, and priority. A cycle is detected while a computation reads another node,
not when an unrelated input is written; the active computation stack reports the full path.
Dirty nodes run in increasing height (indexed buckets for common heights, a heap for sparse/high
ones); a queued bit prevents duplicates. Node equality is selected at creation —
`Eq(fn(&T,&T)->bool)`, `Revision(fn(&T)->Revision)` for buffers/models, `Identity` for stable
handles, or `Always` for opaque/time-varying values — and this is exactly the mechanism behind
L2's "publication is a hint" (00 §4, 03 §6): the consumer compares, not the publisher.

Priority lanes are `Input`, `Animation`, `Normal`, `Idle`. Within the graph, dependency order
always wins over priority — a high-priority consumer cannot run before its normal-priority
prerequisite; priority selects only among independent ready nodes. Idle work has a time slice and
an age; once aged past a threshold it is promoted, unless its owner is unobserved and explicitly
suspendable. Input is never blocked behind cache warming.

Two correctness backstops ride the height machinery. **Pull-before-read** (`ensure_current`):
when `Update::get`, a dynamic read, or property settle touches a stale target, its prerequisites
are made current first, the recompute runs, and the equality cutoff applies before the value is
returned — height ordering efficiently settles the *known* dirty graph, but it is never allowed
to return a stale node merely because that node was unobserved or absent from the heap when the
flush began. And `update.prior(key)` is the explicit committed-value escape (pre-update value
for a source; last published output for a derived key, without executing user code) — it can be
expensive (it may clone or ask a revisioned model for a snapshot), so it never sits on ordinary
direct-sink paths; a port classifier that needs old/new values declares and measures that cost.

Direct source/field sinks (03 §2's `Signal`-backed ports) do not enter this dynamic scheduling
machinery at all: their subscriber records are dense slab entries with a validated destination
port and patch thunk, mounting has no heap allocation after slab capacity, and unmount is O(1)
via removal metadata carried on the reverse-observer link — this is the concrete mechanism behind
"a signal-backed property is never promoted, never diffed." Generations never silently wrap into
validity: a wrap forces a new epoch or a terminal diagnostic before an old public handle could
alias a new slot — the runtime fact that makes "ABA-safe by generation" (§4) true rather than
probabilistic.

**Teardown and eviction share one protocol.** Every node, observer, effect, and producer has an
owner epoch; teardown first revokes the epoch, queued work checks it before executing, and
physical arena slots are reclaimed only after the current dispatch/settle epoch — a teardown
callback cannot reenter a half-freed owner. Cache eviction is not teardown but follows the same
protocol: evicting a keep-alive branch or recycling a virtualized row that holds an active IME
composition or pointer capture first sends the exactly-once platform cancellation, and the
virtualizer's retention reasons (07 §8) veto eviction while such a protocol is active — a
composing or captured subtree is unevictable until released.

**Sender delivery is exact, not vibes.** The stream ingress under `Remote<T>` (05 §8) is
`Send + Sync + 'static`, holds no borrow into UI storage, tags each owned message with its
destination owner epoch, and wakes with an idempotent non-blocking waker. FIFO is preserved per
sender; racing senders linearize into one traceable total order (domain order across producers
requires one sender or a carried sequence). Capacity is explicit — bounded `try_send` returns
`Full`, `send_async` waits without blocking the UI thread, `latest(key)` replaces, coalescing
combines domain deltas, lossy feeds count drops; unbounded is never the default. A successful
send means the queue accepted ownership, not that the UI applied it; **reliable delivery is
prefix-loss-only per sender** — revocation discards a suffix of that sender's FIFO, never an
interior message, so a multi-message protocol can rely on gap-free prefixes — and
`close_and_drain` is the separate synchronization for callers that need acknowledgement. Tests
force every enqueue/revoke/drain interleaving.

## 4 · Measured data structures

Every representation below was measured, not assumed (Apple M3 Pro, `rustc` release, single
thread — re-run before trusting absolute numbers on another microarchitecture; trust the ratios).
Full experimental detail lives in the historical `runtime/datastructures.md`/`datastructures-2.md`
source passes; this is the adopted set.

| Structure | Representation | Headline |
|---|---|---|
| `Signal<T>` cell | `{ value: T, slot: Cell<u32> }`; bookkeeping in a side per-window table | overhead **4 B** flat; an unwired `Signal` allocates nothing; steady read is one field load |
| `Slot<T>` | `NonZeroU64` packed `[index:32 \| gen:32]`, hand-written `Copy` (no spurious `T: Copy` bound) | 8 B; `Option<Slot>` free; ABA-safe by generation, with wrap forcing a new epoch/terminal diagnostic (§3) rather than probabilistic safety |
| `List<T>` item store | contiguous array (honest `Deref<[T]>`) + a side generational slot directory | `get(slot)` O(1); item storage stays contiguous at every scale — "promotion" changes the order/geometry index layered over it, never the container (00 §9); the contiguous trade's price is a measured 1.8–2.8× splice identity-tax (directory fixups on physical moves), paid knowingly for scan speed and the slice contract |
| Collections order/extent tier | adaptive: inline `Vec`/`SmallVec` below a measured `N_vec`, sum-tree order/extent above it, with hysteresis and a promotion-thrash counter | O(log N) claims apply only in the promoted tier; the wired list's whole claim — **rows-run ≈ rows-changed** — is a counter that proves or falsifies it per op (the contained-oracle fuzz applies) |
| Retained display store | flattened DFS (paint-order) SoA hot record, 8 B, cold payload side-tabled; at scale, **chunked 256-node pages** + a Fenwick tree over counts | full traversal is one linear scan (tens of × faster than pointer-chase); a structural edit is a bounded in-page shift, not an O(N) memmove, at every measured scale from 10k to 1M nodes |
| Tape span | 32 B record, 128-bit rolling content hash, mandatory `memcmp` confirm before reuse | dedups identical rows/reruns to one lowering; SIMD hashing is bandwidth-bound |
| Subscriber lists | intrusive links by default (O(1) zero-alloc churn); a contiguous CSR cache for high-fanout, low-churn cells (a theme token) | intrusive wins on subscribe/unsubscribe; contiguous wins ~7× on publish fanout — chosen per cell by measured shape |
| Describe read-sets | inline-8 sorted small vector + merge-diff, epoch-stamped intra-frame dedup | zero-alloc, exact added/removed set; a bitset loses because it scans total slots, not reads |
| SoA patch queues | dense-scatter dedup (`winner[slot] = last write`, no full sort) + coalesced upload ranges | **3.3 ns/patch vs 9.9 (std sort) / 9.4 (radix)** at 10k patches — beats sorting whenever there is write amplification, the common case; only the *distinct* touched set is ever sorted |
| Spatial/order index | uniform tile-bucket hit index; 64 px damage tile bitset; a Fenwick tree for extent/order queries | hit test **53.6 ns/query vs 9.1 µs linear (170×)**; damage union of 200 rects in 542 ns, O(1) dedup; extent query **3.3 ns vs 3.66 µs naive (~1100×)**, 8.8 ns update — O(log n) both ways, pointer-free |
| Hot-map hashing | identity/Fx-class hasher on every map keyed by a precomputed hash, an interned slot, or a `TypeId` | `std`'s default SipHash re-hashes an already-avalanched value and dominated cost wherever measured — a lint, not a footnote (00 §9) |
| Order without position (durable cross-container order) | `u64` windowed-relabel labels on a doubly linked slab | O(1) amortized insert-after; two orders of magnitude cheaper than physical reordering at scale; fractional-key schemes exhaust in tens of inserts and are rejected |
| Shared header+slice payloads | thin, single-allocation handles (header + slice in one allocation behind a one-word pointer) | half the size of a two-word fat-pointer `Arc<[T]>`; one allocation instead of two |
| `SharedString` | 24 B small-string-optimized union (≤22 B inline, `Arc<str>` heap arm — same size as the plain enum) | inline clone 1.88 ns vs Arc 4.96; a text hole `write!`s into a retained buffer + byte-compare at **14 ns/publish, zero heap** vs `format!` at 41.7 ns + 1 alloc (3×) |
| Closure storage | 3-word inline union (`fn ptr + ≤2 words` of captures) | fn items and capture-free producers — the common region shape — are **0 B**; oversized captures spill to a pooled box that is *counted* (`closure_spill`), so an inline-capacity mistake is data, not a mystery |
| Cold/serialized topology | succinct balanced-parentheses bitvector — **never the live tree** | **3.0 bits/node vs 64** (250 KB of topology for a 1M-node tree) for `UiSnapshot` wire format and history checkpoints; navigation is 20–470× slower than the explicit `subtree_size` load, which is exactly why it is the cold form only |
| Invariant-backed `unsafe` (all Miri-gated) | unchecked slab `get` under the `gens.len()==items.len()` invariant (**1.49×**); unchecked SoA scatter on interned slots (**1.13×**); `NonZeroU32` niche-stacking wherever optional handles cluster (4× `Option` in **16 B**, all-None = all-zero) | each has a one-line stated invariant; the measured *losers* are recorded too — bit-packed u64 nodes lose to the aligned 8 B struct, and a tagged u64 value buys nothing over the 8 B niche-clean enum |

### 4.1 The measured laws

Five results from the experiment lab graduated from "adopted representation" to **law** — each
is a rule the scheduler or a reviewer applies, with its measured endpoints attached so the rule
survives contact with a different microarchitecture as a re-measurement, not a leap of faith:

1. **Rebuild-vs-retain.** Retain + diff; rebuild only per-owner pockets. At 1M nodes a 64-edit
   frame is **~200 µs retained vs 12.7 ms rebuilt (~60×)**; at 10k the rebuild is 114 µs — fine,
   which is precisely *why* the owner-grained floor tier works and why nothing larger may adopt
   rebuild-per-frame.
2. **Adaptive traversal cutover.** The dirty walk (`subtree_size` skip + epoch-stamped
   dirty-ancestor lane + `parent[]`) is **20× at 0.01 % dirty** with work attached, break-even
   near 0.1 %, and a *loss* past ~1 % — the skip walk is branchy dependent loads while the full
   scan streams. **The scheduler picks the traversal per frame by dirty count**; both endpoints
   are measured constants, so the cutover is hardcodable per record width. (The epoch lane vs a
   1-bit L2-resident lane is a recorded re-measure obligation on the final record layout.)
3. **Width is the frame bill.** A full scan at 1M nodes costs **73/113/205/432 µs at
   4/8/16/32 B per record** — every "harmless" inline u32 is another 25 µs/frame. The hot record
   stays 8 B; a pass that reads only flags reads a **1 B flags lane (6×)**; cold data is
   side-tabled. Kind-segregated SoA taken to per-pass lanes.
4. **Borrow-to-read.** No kernel read path takes an owned handle: an `Arc` clone to *look* at a
   payload is 3.91 ns vs 0.84 ns for the same call over a borrow (**4.6×** of pure refcount
   traffic — ~30–80 µs/frame at 10k styled nodes). Ownership transfers take the handle; reads
   take the borrow.
5. **Chunked stable arenas for interners.** Every interner/registry that hands out `&T` under
   append (style intern, wire table, kind registries) uses a chunked stable arena — stable
   addresses at **3.0 ns/push ≈ plain `Vec`**, state-independent, ~16 B/item — never per-item
   boxing (7.8–255 ns, allocator-state-dependent, ~40 B/item).

## 5 · Memory: zero steady-state allocation

Every allocation in the pipeline is classifiable as exactly one of three kinds, and anything that
fits none is a design bug, not a tuning problem:

1. **Steady-state: zero.** A running screen doing what it did last frame — re-describes, signal
   patches, hover, scroll, animation, typing into an existing field — allocates nothing, asserted
   by a debug allocator hook per phase.
2. **Novelty: page-granular, counted, amortized.** Structural novelty (a first mount, a new wire,
   a cache miss) draws from window-level free-listed pages, touching the OS allocator only when
   the free list is empty; every novelty allocation is counted with provenance.
3. **Budgeted caches with a correctness-safe miss path.** Every retained cache — shaping, layout
   memo, tape-dedup intern, atlas, media variants, composite layers, history — has a byte/entry
   budget, eviction telemetry, and a miss path that is slower, never wrong.

One `MemoryLedger` per window registers every pool by name, live bytes, committed bytes, budget,
and eviction callback; in debug builds an unattributed allocation is an error, so the ledger sums
to the truth and a leak is attributable the day it appears. The pressure cascade evicts, in
order: speculative work → dedup/intern caches → shaping/layout memos → earned composite layers
(never below skip-paint) → media variant caches → arena high-water trim. High-water trim
periodically decommits pages toward the recent p95 usage so RSS actually falls after a burst, not
merely the ledger's accounting.

Four mechanisms are what make class-1 zero actually reachable rather than aspirational:
**learned size classes** — the first describe of a View *type* records its page footprint, so
every later mount of that type is one exact-fit page pull with zero realloc-growth (this is what
makes M1's mount-storm constant achievable); **`Display` into the retained buffer** — a
projection whose output is `impl Display` lowers as a text wire that `write!`s directly into the
port's retained byte buffer and byte-compares for the consumer cutoff, so steady-state derived
text (counters, "n selected", timestamps) allocates nothing, while the floor's rung-0 `format!`
lands in the describe bump page and is arena-absorbed rather than heap churn; **intrusive
everything** — subscriber lists, dirty lists, eviction queues, and free lists live as intrusive
links inside the records they organize, never a side `Vec<Box<_>>` on a hot path; and **counted
spill** — every usually-inline optimization (closure SSO, small child lists, inline strings) has
a counted spill path, so an inline-capacity mistake is inspector data.

What still allocates, honestly: one boxed future per spawned producer — pooled, size-classed,
counted; class-2 novelty by definition (first mounts, first binds, cache fills); whatever the
OS, GPU driver, and platform text/IME engines allocate internally (observed via RSS in M1/M4,
not controllable); and debug-only instrumentation. Nothing else — and the ledger is the proof,
because in debug an unattributed allocation is an error.

The public surface never grows a memory annotation; the one touchpoint is an application policy,
`Application::run(view).memory_budget(..)`, defaulting adaptively from the platform's device class
and memory-pressure signals. Gates M0–M6 ([10 §1.8](10-quality.md)) hold steady-state zero with a
non-vacuity clause, mount-storm bytes plus OS-measured RSS return, ledger completeness, cascade
ordering with a bounded thrash counter, the 24-hour churn soak, history's disk-spill cap with
spill-boundary correctness, and fragmentation bounds with idle-lane-only compaction.

## 6 · The zero-tax law and the fast lane

> **L14 — no path pays for a mechanism it does not use** (00 §4): every fast-path mechanism is
> pay-per-declaration, strictly-replacing an equal-or-greater existing cost, or idle-lane-only and
> preemptible by real input. Every fast-path change ships a floor-tax benchmark — tier-0 idle/
> counter/no-`Signal` workloads before and after — and a regression there is rejected regardless of
> how much it wins on the promoted workloads.

The competitive target is zgui's own pinned numbers: ~44 µs translation-only scroll, ~202 µs
median hover crossing, ~1.02 ms recycle frame, zero frames when idle, no clean-tree traversal.
The governing structural idea is a **second loop** zgui does not structurally separate:

```text
NORMAL LOOP  (UI thread, turn-based)      FAST LOOP  (input/compositor, epoch-resident)
event -> handler(&mut) -> settle graph    raw pointer/wheel -> resolve vs PresentationResolver
      -> publish LOGICAL epoch                              -> apply precomputed variant/xform delta
      -> style/layout/paint                                 -> damage rects -> recomposite
      -> publish PRESENTATION epoch        (zero graph work, zero Rust, zero logical epoch;
      -> lower -> submit FramePacket        escalate to NORMAL LOOP only for discrete/owner work)
```

Alongside each presentation epoch the kernel publishes an immutable `PresentationResolver` — a
hit index, a precomputed per-node hover/active/focus variant delta table (the cascade is resolved
once, at publish time, even when the author wrote a closure-shaped `.hover(...)`), and a scroll
table. A pointer move or wheel event resolves entirely against this resolver — hit-test, apply a
precomputed slot write, damage the union rect, recomposite — with **zero graph work, zero Rust,
and zero logical epoch**, as long as the crossing changes only hover/active variants and no
handler needs the event. The soundness key is that machine-rate events already cannot reach
`fn(&mut Owner)` (03 §2, §4) — the type system has already proven the owner does not care about
each pointer move — so classification toward the fast lane is sound by construction wherever it
applies, and *conservative by default* (escalate to the normal loop whenever ambiguous) wherever
it does not.

Ranked alongside this fast lane: predictive layer promotion (warm a composite layer the instant a
`.pan(...)` claim or a motion activation makes divergence *inevitable*, rather than waiting to
measure it — closing the "first frame is slow" gap); a structure-of-arrays interned patch queue
with coalesced GPU range uploads; tile-based damage compaction; reused, reset-not-freed per-turn
arenas with a steady-state allocation detector; tape dedup via the interned span cache of §4;
persistent-layout fast paths (a leaf-metric short-circuit — with digit-width canonicalization/
tabular-nums making a fixed-digit counter's layout cost literally zero — a
`(style_hash, constraint)` memo validated by the from-scratch relayout oracle, and — only if
measurement demands it — a single-pass layout engine behind the existing recipe-typed surface,
never a public-vocabulary change); frame pipelining with a latency lane that lets input bypass a
pipelined animation frame; and idle-lane speculative pre-describe/atlas warming, gated to never
delay input. None of these adds a public spelling — every hint is derived from a declaration
already written for a semantic reason (`.hover`, `.pan`, `animate(...)`) or from pure runtime
measurement.

Each mechanism above carries its own numbered, non-vacuous CI claim — the FP gate family
([10 §1.9](10-quality.md), FP1–FP10): the fast lane's `turn_settles == 0` trace with its
escalation proof, predictive promotion's zero-work first frame with bounded warm→demote thrash,
the patch queues' fanout sweep, damage exactness with conservative promotion, zero-alloc turns
with generational buffer fencing, tape dedup's collision fuzz and shared-picture immutability
oracle, the layout memo's oracle validation, pipelining's torn-frame oracle and lane split, and
speculation's preemption proof. A mechanism whose gate cannot show *both* the win and its
non-vacuity half fails its own gate.

## 7 · Const subtree templates

Layout recipes are already `const`-able (02 §3: `const TOOLBAR: Flex = Flex::row()...`). Pushed
one step further, a subtree whose operands are all constants is, in principle, a **const-
evaluable opcode template**: sealed tape bytes computable at compile time, so description of such
a subtree becomes one `memcpy` of a static template plus patches at recorded live-operand offsets,
and the commit diff sees byte-identical ranges and skips them wholesale. This needs no macro, no
new user-facing type, and no semantic change — only that recipe/constructor methods be `const fn`
where operands permit, plus a kernel template format with a patch table. It is purely an
optimization tier: if the measured win on real chrome (toolbars, menus, gutters — subtrees that
never change) is large, the design gets the compiled-UI approach's headline benefit inside
ordinary ladder semantics; if not, nothing was spent on API.

## 8 · The by-construction competitive claim

The "secondary tier" — the no-macro hot path of borrowed-frame description, direct signal ports,
`project` regions, and `List` deltas — is not a benchmark hope; it falls out of the data
structures above, stated as a theorem with a construction proof against the two honest extremes:
**Dear ImGui** (immediate mode — rebuild a borrowed draw list every frame, no retained widget
state) and **zgui** (retained mode — persistent document, computed styles, persistent layout
nodes, shaped paragraphs, damage tracking).

> **Claim.** Per description turn, the secondary tier's authoring cost is in ImGui's cost class
> (arena appends, no per-frame heap allocation) with strictly-less-or-equal copying, and its
> *steady-state* cost is in zgui's cost class (`O(changed)`, damage-tracked, zero clean-tree
> traversal) — beating each baseline exactly where that baseline is weakest.

*Authoring ≈ ImGui, by the arena*: an `El<'f>` append is a pointer bump into the per-turn arena
carrying a borrowed operand — the same operation as an `ImDrawList` append — but the borrowed
frame copies nothing during description and promotes only the commit-diff delta, so per-frame
allocation is `≤` ImGui's and exactly `0` on an unchanged subtree. *Steady state ≈ zgui, by the
diff and the ports*: the retained tape diffs the draft and touches only changed slots, but the
hot sinks never enter description at all — a signal publication patches its property slab through
a direct port with zero turns and zero Rust, and a `List` edit patches only its affected slots —
so a typical frame does `O(changed sinks)` work, not `O(tree)`. The floor this bounds
(`max(ImGui_authoring, zgui_steady_state)`) is a construction fact of the arena, the diff, and
the sealed port slab — not a tuning result, which is why gate 6b (10 §1) treats a clean-frame
allocation or a clean-tree walk as a build failure rather than a regression to schedule.
