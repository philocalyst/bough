# 05 — Async and the Cohort State Machine

Scope: the `Reader<T>` graph and its one wake law, the question/action/stream latency trichotomy,
boundaries and first readiness, request admission and cancellation, the failure table, the
reader/producer ownership law, and — authored here for the first time as a complete artifact —
the cohort reference state machine that gates 07's async semantics have always cited but that no
prior pass wrote down in full.

Depends on: [00-model.md](00-model.md) L5, L12; [03-ports.md](03-ports.md) §1 (`Reader<T>` is a
`Source`). Feeds: [04-reactions.md](04-reactions.md) (completions surface through `changed`),
[10-quality.md](10-quality.md) (gate 8/8b/L1 bind to this document).

---

## 1 · Rust owns work; the graph owns latency

GPUI does not invent a second application-visible executor, future wrapper, `Task<T>` field, or
async context. Real Rust runtimes — Tokio, smol, a thread pool, GPUI's own foreground executor —
create and poll work. GPUI owns only what a latency-bearing value *means* after it enters the
reactive graph: declared inputs, mounted identity, readiness, request generations, stale-result
rejection, coherent presentation, cancellation of delivery, and fault propagation.

The one graph read facade:

```rust,ignore
pub struct Reader<T> {
    raw: RawSource,             // small typed generational key; no T and no executor handle
    marker: PhantomData<fn() -> T>,
}
```

`Signal<T>::reader()`, a synchronous projection, an async projection, an application resource,
and an external cache adapter all produce the same `Reader<T>`. It is `Copy`, inert, and cheap to
pass through constructors. It deliberately does **not** implement `Deref`: before first readiness
there is no `T` to borrow, and a Rust dereference must never secretly block, return a hole, or
unwind.

```rust,ignore
fn local_story(id: StoryId) -> Story { /* .. */ }
async fn fetch_story(id: StoryId) -> Result<Story, LoadError> { /* .. */ }

let story = selected_id.map(local_story);                 // Reader<Story>
let story = selected_id.try_map_async(fetch_story);       // Reader<Story>
let byline = story.map(|story| format_byline(story));     // Reader<String>; latency composes
```

`map_async` admits `Future<Output = T>`; `try_map_async` admits `Future<Output = Result<T, E>>`
and lifts `E` into the graph's typed failure channel (§6). `compute`/`try_compute` are the CPU-pool
twins, so choosing an I/O executor versus a CPU pool is visible exactly once, at creation.

## 2 · The wake law

> The whole runtime bridge is one sentence: **at the graph boundary, an async completion is only
> a stamped memo update scheduled for the next reactive flush. A `Future` wake itself only
> schedules another poll.**

An async producer is an ordinary memo node with a private evaluation record
(`Empty | Ready(prev) | Pending(request) | Fault` — this is §9's per-node record). Its future is
polled by whichever runtime is installed; that runtime remains responsible for reactors, timers,
work stealing, and physical cancellation. `Waker::wake` may be duplicate or spurious and carries
no value — it stays inside the driver as a coalesced `PollWake` and cannot dirty a graph node, run
a memo, or start a flush. When polling returns `Ready(value_or_fault)`, the driver puts the owned
outcome in a private slot and enqueues one small record through the same ingress core as
`Remote<T>`:

```rust,ignore
StampedCompletion {
    node: RawSource,
    owner_generation: Generation,
    request_ticket: Ticket,
    slot_generation: Generation,   // owned Ready-or-Fault is already in the private slot
}
```

Enqueueing a `StampedCompletion` is `Send`, host-wake-capable, and inert: it never runs
application Rust, edits a tree, or publishes presentation reentrantly. The next normal turn
validates the stamps (§7), stages the accepted outcome, marks that memo dirty, and lets the next
reactive flush settle it through the same height/equality/fault/commit machinery as a synchronous
memo. A completion arriving mid-flush waits for the following turn.

```text
signal write -> reactive flush -> dirty async memo -> runtime.spawn(frozen input)
runtime poll / I/O / timer -> PollWake -> runtime polls again
runtime observes Ready -> StampedCompletion(node, ticket) -> next normal turn
                              -> dirty memo -> reactive flush -> ordinary readers settle
```

The default application installs GPUI's own runtime handles; a host may inject Tokio, smol, or a
custom adapter once. `map_async` selects the default I/O lane; `map_async_on(runtime, f)` and
`compute_on(pool, f)` make exceptional affinity visible at the producer and nowhere downstream — a
runtime handle should never appear threaded through an ordinary call chain the way `cx` once was.

## 3 · `project` is the honest await point

Intermediate functions accept and forward `Reader<T>`, never `Future<T>`, `Option<T>`, or
`Result<T, E>`:

```rust,ignore
fn story_page(story: Reader<Story>) -> impl Element {
    row((sidebar(), story_detail(story)))
}

fn story_detail(story: Reader<Story>) -> impl Element {
    story.project(|story: &Story| article((h1(&story.title), text(&story.text))))
}
```

`project` installs a retained consumer region and invokes its closure only when the reader has a
value, so inside the closure the reader is plain `&Story`, not `Option<&Story>`. A direct
property sink is even smaller: `h1(story.map_ref(|story| story.title.as_str()))`. The private
evaluator has a three-way verdict — `Ready(&T)`, `Pending(wait_set)`, `Fault(failure_key)` — that
is never application state and never returned by a reader; property ports and `project` regions
understand the verdict, and `boundary`/`failed` policy (§4) consumes it.

A retained reader projection is conceptually `for<'f> Fn(&'f T) -> El<'f> + 'static`: it cannot
capture `&self` or retain `El<'f>` across turns. Imperative code and tests use
`reader.resolve().await` for an owned snapshot — illegal during description, a one-shot wait, and
never a hidden dependency.

### 3.1 Why `Reader<T>` has no `Deref` — and the one shape where it soundly could

The ambition is reasonable: `*self.story` would be lovely. It is refused for `Reader<T>` because a
reader has three verdicts — `Ready(&T)`, `Pending`, `Fault` — and `Deref::deref(&self) -> &T` must
return a `&T` unconditionally. For a value that may not be ready yet, the only ways to honor that
signature are the three the rejection list bans (00 §6): panic (panic-as-suspension), block (async
`Deref`), or hand back a hole/last-value. Each of them **loses stuff** — specifically, it makes a
`Fault` (a network error, a revoked owner) invisible at the use site, which is exactly the thing a
UI must not silently swallow. So the no-`Deref` rule is not timidity; it is the refusal to hide a
failure behind a dereference.

And you do not pay ergonomics for it, because the design already hands you a plain `&T` at every
point where it is *sound* — inside `project` and inside a `boundary`'s ready child, the reader is
`&Story`, not `Option<&Story>` (§3), and `map_ref` feeds a property port with the same directness.
Deref-grade ergonomics, exactly where a value provably exists, losing nothing.

There is precisely one shape where a real `Deref<Target = T>` is *total* and therefore sound: a
reader that can never be un-ready because you supplied what the un-ready cases collapse to.

```rust,ignore
let title: Reader<Story>  = self.selected.try_map_async(fetch);   // may be Pending/Fault — no Deref
let title: Settled<Story> = title.or(Story::placeholder());        // always Ready → Deref is total

fn view(&self) -> El<'_> {
    h1(&self.title.headline)     // ✓ `Settled<T>: Deref<Target = T>` — field access, no project
}
```

`reader.or(fallback)` folds both `Pending` and `Fault` into `fallback`, yielding a `Settled<T>` — a
still-`Source`, still-reactive value that is *always* `Ready`, so `Deref` on it can never lie. This
is the ambitious answer, and it is honest about the trade rather than hiding it: opting into
`Settled` is opting *out* of ever seeing the pending and fault verdicts, so it is the right tool for
a value with a sensible default (a cached avatar, an empty document, a zero count) and the wrong one
for anything whose loading or failure the user must see — that stays a `Reader` through a
`boundary`. The inspector still shows which `Settled` values are currently presenting their fallback
versus a real completion, so the discarded distinction is observable in tooling even though it is,
by your explicit choice, absent from the code.

## 4 · Fetch high, block low

A boundary is a retained element; its child is a reader/property/projection descriptor, so the
boundary is installed before the child is evaluated:

```rust,ignore
fn app(&self) -> El<'_> {
    row((
        story_list(&self.selected_id),
        boundary().loading(detail_skeleton).failed(load_failed).child(story_page(self.story)),
    ))
}
```

The fetch belongs to the model/root producer and begins as soon as its declared input publishes;
the skeleton belongs to the detail pane. Moving the producer changes *when* work starts; moving
`loading` changes *what the user sees*; neither move forces promise types through the intervening
layout. `Loading` means **first readiness**, not "some request exists": before this branch has
committed real content a pending reader selects the fallback; after the first successful commit,
revalidation keeps the last coherent content mounted and interactive by default. A semantic
replacement that should deliberately re-show its skeleton says so explicitly:
`boundary().loading(detail_skeleton).reload_on(&self.selected_id).child(story_page(self.story))`.

**Producer declaration precedes evaluation**, so two nodes driven by the same already-ready input
arm together even when their consumers are nested differently in the visual tree — source
position does not serialize independent producers. A producer created *inside* a blocked
`project` region is intentionally downstream and cannot arm before that region becomes ready; the
inspector calls this a **declaration waterfall**, names the blocking edge, and suggests hoisting
when the input was already available.

## 5 · `is_pending` and `latest`

A write begins a **candidate presentation cohort** (§9). The model mutation itself completes
normally; existing retained ports/regions keep their last settled owned operands while candidate
description reads current model intent. Two read-only questions expose this without a writable
transition flag:

```rust,ignore
results_list().opacity(is_pending(&self.results).map(|p| if p { 0.65 } else { 1.0 }));
story_list().selected(latest(&self.selected_id));   // opt into current intent now
```

`is_pending(x)` means "does the next visible answer for this source/read still have an unresolved
causal dependency?" It is not a writable flag and not an application-global spinner; it can be
asked at an async reader, a derived reader, or the source write that began the cohort.
`latest(input)` means "bind this presentation sink directly to current model intent rather than
its cohort's retained operand" — for input sources; it does not predict an unresolved fetch's
result. Controlled text/IME, pointer capture, and other latency-critical writeback paths declare
the same immediate/latest policy so an async search below them never freezes typing.

## 6 · Failures travel with the read

One failure path, not a resource `.error` flag competing with boundaries. A fallible producer's
`E` is checked at creation and retained exactly; it is erased only at a heterogeneous subtree
boundary (a plugin, 09 §2) and remains downcastable/inspectable there.

| Outcome | Meaning | Presentation rule |
|---|---|---|
| not yet ready | first request has no accepted value | nearest `loading` fallback |
| refreshing | newer request, previous accepted value exists | keep the coherent previous subtree; `is_pending` is true |
| domain `E` | expected operation failure | nearest matching `failed` policy; retryable |
| superseded/revoked | a newer ticket or dead owner won | no error UI; delivery is discarded |
| worker/executor fault | panic, driver shutdown, admission failure | framework `Fault`; never forged into `E` |
| invariant/plugin panic | programmer or isolation failure | discard draft, poison the narrowest unsafe pocket, then recover/escalate |

A revalidation failure never publishes half of the candidate cohort and never silently overwrites
the last accepted value; boundary policy chooses replace-with-error or keep-stale-plus-adornment,
and the default supervisor reports any unhandled failure so "keep stale" cannot become "swallow
error." A dependency change or successful retry heals a domain boundary automatically.

## 7 · Request admission and cancellation

Every async/CPU request carries at least:

```text
RequestStamp {
    producer: (owner, site, mount_generation),
    ticket: monotonic_per_producer,
    frozen_inputs: [(source, version)],
    policy_generation,
    candidate_epoch,
}
```

The complete input tuple freezes once from one immutable candidate description. GPUI stages the
request and pending verdict, atomically admits that private graph record without publishing its
presentation, and only then enqueues owned work — a draft that fails admission never launches an
orphan request. Completion enters through a later ordinary turn and is admitted only when
producer generation, ticket, every input version, and policy generation still match; this rejects
out-of-order completion and the `A -> B -> A` alias a value-only or global-epoch check misses.

> Once work is superseded, unmounted, or revoked, its completion cannot affect presentation.

Logical cancellation (ticket revocation) is universal; cancelling a queued driver is
executor-specific; dropping a polled future is only as cancellation-safe as that future. Saves and
other effects use actions with explicit reliable/idempotent policy, never latest-wins read
producers (§8).

## 8 · The trichotomy: question, action, stream

> **L12 — latency has exactly three shapes** (00 §4). The classifying question is one sentence:
> *is a stale answer worthless (question), is a lost request a bug (action), or is the sequence
> itself the meaning (stream)?* Nothing else exists; a fourth shape is a design error.

- **Question.** A repeatable derived read — `source.map_async(f)`/`try_map_async`/`compute` —
  producing a `Reader<T>`, revision-stamped, latest-input-wins, superseded or unmounted work
  cannot affect presentation. Finite by definition: one accepted answer per stamped request.
- **Action.** A finite effect with a declared policy — a method, or a mounted
  `.action(...).queue(Queue::Reliable).pending("Saving…").then(Editor::saved).catch(Editor::save_failed)`.
  Reliable or idempotent by declaration, because cancelling a mutation is not like cancelling a
  read. Completion is one stamped delivery to its owner; unmount revokes delivery, never un-does
  the world:

  ```rust,ignore
  button("Save")
      .action(Editor::persist.with(&self.document.snapshot))
      .queue(Queue::Reliable)
      .pending("Saving…")
      .then(Editor::saved)
      .catch(Editor::save_failed)
  ```

  The invocation cell lives at the button's mount identity (02 §6). Optimistic writes are tagged
  candidate deltas; success reconciles them with the canonical reader; failure applies a typed
  inverse or an explicit reset/rebase policy.
- **Stream.** An owned protocol whose *messages are the domain*, not the completion of a
  derivation. `Remote<T>`/services with explicit lanes (`Reliable`, `Latest`, `Coalesce`,
  bounded); owner-bound — it lives exactly as long as the entity or mount that owns it, and its
  teardown is that owner's teardown. Never latest-wins, never silently coalesced unless the lane
  says so.

No public `Task` type exists at any of the three shapes — the closest thing to it, `.action(...)`,
is a small local adapter over the same admission machinery §7 already specifies, and `Remote<T>`
is a service handle, not a generic future wrapper.

## 9 · The cohort reference state machine

This is the artifact §7 has always presupposed and no prior pass wrote in full: the per-node
record, the cohort record, and the transition table — including the entanglement rule between a
cohort-held input and an unrelated live source sharing one consumer region.

> **Atomicity is per publication cut; coherence is per region; reveal is per boundary.** One
> flush publishes one consistent cut, and a region's operands always come from one cut — the
> forbidden "half-cohort" is a single consumer observing part of a candidate's operands, which
> operand selection (§9.4) makes unrepresentable. Reveal granularity is boundary policy: a
> boundary reveals when every reader *it guards* is ready; sibling boundaries and unguarded
> regions reveal independently (§4's nested-boundary rule).

### 9.1 Per-node record

Every producer node — sync or async, question-shaped — carries exactly one of:

```text
Empty                  no accepted value has ever been admitted
Ready(prev)            a settled, presentable value; `prev` is the retained operand
Pending(request)       an admitted RequestStamp is outstanding; `prev`, if any, remains presentable
Fault(kind)            domain E | superseded/revoked | worker fault | plugin panic (§6)
```

`Pending` and `Fault` both retain the last `Ready` operand when one exists — this is what lets
`is_pending`/`refreshing` (§6) present coherent stale content instead of flashing a fallback on
every revalidation.

### 9.2 Cohort record

A **cohort** is the set of node records a single write's candidate description touches, keyed by
the write's `candidate_epoch` and carrying:

```text
Cohort {
    candidate_epoch,
    policy_generation,             // bumps on every admitted write that could supersede this one
    read_set: [RawSource],         // every source the candidate description actually read
    members: [RawSource -> NodeState],  // this cohort's view of each touched producer
}
```

A cohort is created the instant a write's candidate description discovers a producer whose
`frozen_inputs` include a value from that write (08 §1's speculative-description step). It is
retired — its members' verdicts either commit into the published epoch or are discarded — at the
next successful or failed atomic publish that resolves it.

### 9.3 Transition table

| From (node state) | Event | To | Presentation effect |
|---|---|---|---|
| `Empty` | admitted request stamped | `Pending(request)` | nearest `loading` fallback |
| `Pending(r)` | `StampedCompletion` admits, value | `Ready(value)` | first commit: reveal atomic with every co-member the same boundary guards; siblings independent |
| `Pending(r)` | `StampedCompletion` admits, domain `E` | `Fault(E)` | nearest `failed` policy; retryable |
| `Pending(r)` | `StampedCompletion` fails admission (stale stamp) | *unchanged* | discarded silently; no presentation effect (§7) |
| `Ready(prev)` | a newer write freezes new inputs, request stamped | `Pending(request)`, `prev` retained | `is_pending` becomes true; `prev` stays presented |
| `Ready(prev)` | newer write's completion admits, value | `Ready(new value)` | revalidation reveal: replaces `prev` |
| `Ready(prev)` | newer write's completion admits, domain `E` | `Fault(E)`, `prev` retained as adornment target | boundary's replace-or-keep-stale policy (§6) |
| `Fault(E)` | an explicit retry, or any frozen-input source publishes | `Pending(request)` with a new ticket | boundary keeps its failure content until the new request's first successful admit, then heals (§6) |
| `Ready`/`Pending`/`Fault` | owner/mount generation revoked (02 §6 mount-cycle) | node destroyed | in-flight completions die by stamp mismatch (§7); no presentation effect |
| any | worker/executor fault, plugin panic | `Fault(worker)` | framework `Fault`; discard draft, poison narrowest pocket (§6) |
| `Pending(r1)` then `Pending(r2)` on the same node (`A -> B -> A` alias) | `r1`'s completion arrives after `r2` was admitted | *unchanged* | `r1` fails the `request_ticket`/`policy_generation` check in §7; only `r2`'s completion can settle the node |

### 9.4 The entanglement rule

Two questions the state machine must answer that a per-node table alone does not settle:

1. **A `project` region reads one cohort-held input and one unrelated live source; the live
   source publishes.** Does the region rerun with `(old retained operand, new live value)` and
   republish, or hold entirely until the cohort resolves?
2. **Two overlapping cohorts** (write A, then an unrelated write B) share one consumer region
   through their read sets. What resolves the shared consumer?

Both are settled by one rule, adopted as normative:

> **A region rewrites whenever any input it reads publishes, and it always reads each input's
> presentation-selected operand: the retained operand for a cohort-held input, the current value
> for a live one.** Cohort join across two overlapping writes is read-set union, with the newer
> `policy_generation` winning wherever the two cohorts' membership disagrees on a shared node.

Consequence for question 1: the region reruns immediately on the live source's publish, reading
the cohort-held input's *last settled* value (its `Ready(prev)`, not a torn or half-updated
value) alongside the live source's new value, and republishes — the live-source consumer never
waits on an unrelated cohort. Consequence for question 2: a region entangled with both A and B by
shared read set is a member of the *joined* cohort (candidate_epoch = the later write's), and
when member nodes disagree between A's and B's admitted stamps, the higher `policy_generation`
(the newer write) governs which stamp is authoritative for that node — exactly the supersession
rule §7 already states per-input, generalized to a region with more than one cohort-touching
read.

The initiating written source is itself **cohort-held**: already-presented sinks bound to it
keep their retained pre-write operand until the cohort resolves — this is exactly why the
default cannot show a new selected id beside the old story — and `latest(input)` (§5) is the
per-sink opt-out that rebinds one presentation sink to live intent. "Live" in the rule above
therefore means: neither the initiating source nor anything downstream of it in this cohort's
read set.

### 9.5 Worked trace

`selected_id` (a `Signal<StoryId>`) feeds two `try_map_async` producers — `story` and `comments`
— and a `project` region renders both plus an unrelated `unread_count` signal:

1. Click selects id 7. Candidate description freezes `selected_id = 7` for both producers; both
   requests admit and stamp; both nodes move `Empty/Ready(6) -> Pending(request@7)`. The region's
   read set includes `story`, `comments`, `unread_count`. Because `unread_count` did not publish,
   the region does not rerun yet.
2. `unread_count` publishes (an unrelated signal write). The region reruns per §9.4: it reads
   `story`'s and `comments`' *retained* operands (`Ready(6)`'s values — the cohort-held inputs
   have not resolved) alongside `unread_count`'s new value, and republishes — the badge updates
   without waiting on the fetch.
3. `story`'s completion for id 7 admits: `Pending(request@7) -> Ready(story@7)`. The region
   reruns, now reading `story@7` fresh and `comments` still at `Ready(6)`'s retained value if it
   has not yet resolved (`is_pending(&comments)` is true for any consumer that asks). This is
   the per-boundary reveal rule at work: `story` and `comments` sit behind separate boundaries
   in this screen — one boundary around both would hold the reveal until both admit.
4. Before `comments`' completion arrives, the user clicks id 9. `policy_generation` bumps; a new
   cohort is created for `selected_id = 9`; both `story` and `comments` move
   `Ready -> Pending(request@9)`, retaining `Ready(7)`/`Ready(6)` respectively as presented
   content. `comments`' outstanding `request@7` — whenever it now arrives — fails the `ticket`
   check in §7 against the newer `request@9` admission and is discarded silently (the
   `A -> B -> A`-shaped case, generalized to `A -> B` with a superseded in-flight request).

## 10 · Reader/producer ownership law

> **L5 — a producer belongs to the scope that created it**, and a `Reader<T>` naming a dead owner
> presents `Fault(Revoked)` — never pending-forever, never last-value-forever (00 §4).

A producer created during owner construction belongs to that owner's lifetime; a producer created
during description belongs to the mount identity that created it (02 §6). The scope-determines-
ownership rule is one boxed law: *nothing else can create a producer.* A `Reader<T>` is a `Copy`
capability onto that producer, valid exactly while the owning scope's generation lives; naming a
dead owner is a distinct, inspector-attributed fault, never confused with an ordinary pending or
domain failure. An owned, non-`Copy` producer handle (`!Copy`, drop = revoke) is not a named
public type — owner-scoped lifetime covers every real case observed so far; this is a deliberate
minimalism, revisited only if a fixture demands otherwise.
