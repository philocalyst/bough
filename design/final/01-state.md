# 01 — State and Data

Scope: the storage ladder in full, the semantics of `Signal<T>`, `List<T>`/`Slot<T>`,
`Entity<T>`, and `Handoff<T>`, the field law that closes what a struct field may be,
`#[derive(Delta)]`, structural serde journaling (restore = re-derivation), and undo/redo as the
`gpui_history` crate over the same delta log.

Depends on: [00-model.md](00-model.md) §3–4 (ladder, L1–L14). Feeds: [02](02-elements.md) (fields
feeding `view`), [03](03-ports.md) (`Source`/`Cell` instances), [04](04-reactions.md) (`Changed`
consumption), [09](09-boundaries.md) (durable identity, journaling at the plugin boundary).

---

## 1 · The field law

> **A struct field is data, a source, or a data-free endpoint (`Target<K>`, a history `TrackRef`
> (§10.2) — and nothing else stores a route or a closure).** Every relationship — subscription, observation, output routing
> — is a state transition observed through `Changed` in `changed`, never a stored callback, a
> `Subscription` handle, or a wiring table. Consequently every view struct derives
> `Serialize`/`Deserialize` cleanly with no `#[serde(skip)]` ceremony, and restore is literally
> mechanical (§6).

This closes with no exceptions: a field is exactly one of

- **plain data** — `T`, `Vec<T>`, an enum, a nested struct: owner-grained, no independent
  publication;
- **a source or sink cell** — `Signal<T>`, `List<T>`, `Entity<T>`, `Handoff<T>`: independently
  publishable, each a `Source`/`Sink`/`Cell` instance per [03](03-ports.md);
- **a data-free endpoint** — `Target<K>`, or a history `TrackRef` (§10.2): an addressable
  capability, never storage.

There is no fourth kind. A field that would store "who to call back" (a route, a subscription
handle, a boxed closure) is not spellable in this model — the relationship it would have encoded
is instead a delta read in the observer's `changed` (04 §1), which is why nothing needs
`#[serde(skip)]`.

## 2 · `T`, `Vec<T>` — owner-grained state

Plain fields and plain `Vec<T>` are the floor: no macro, no signal, no list. A handler mutating
`&mut self` reruns the owner's `view`, and the retained-tape diff (02 §1) turns that rerun into
the actual delta of work — for a small view this is already cheap; for one that has earned
independence, promote a field one rung.

```rust,ignore
struct Counter { count: u32 }

impl Counter {
    fn increment(&mut self) { self.count += 1; }
}

impl View for Counter {
    fn view(&self) -> El<'_> {
        row((text(format!("Count: {}", self.count)), button("Increment").on_click(Self::increment)))
    }
}
```

`Vec<T>` stays plain and stays the default sequence type; it realigns on every parent rerun and
is tape-diffed like any other value. It is never deprecated by `List<T>` — promotion is a choice
made when rows need durable identity across reorders, not a mandatory upgrade.

## 3 · `Signal<T>` — independently publishable scalar state

`Signal<T>` owns one current value inline in its owner. It implements `Deref<Target = T>`; a
publish is `DerefMut`, which requires `&mut self` and therefore cannot occur during `view`.

```rust,ignore
struct Composer { subject: Signal<String> }

fn send(&mut self) { *self.subject += " (sent)"; }          // publish — needs &mut self

fn view(&self) -> El<'_> {
    text_input(&self.subject)                                // a wire: no Rust rerun on edit
}
```

A property reference (`&self.subject`) is the zero-ceremony fast path. `Signal<T>` is the
canonical `Cell` in the port algebra (03 §1): it is both readable and writable, so it is the type
a two-way control demands. `Reader<T>` (05 §1) is the read-only capability produced by any
storage or derivation whose value must cross a latency or ownership boundary; `Signal<T>` does
not itself cross those boundaries — a `Reader` is derived from it when needed.

**Publication is a hint; the destination compare is the truth** (L2, 00 §4). A `DerefMut` guard
publishes unconditionally on drop; every retained destination — property slab, region operand,
delta consumer — compares the incoming operand against its retained one and no-ops on equality.
`*self.count += 0` therefore costs one enqueue and one compare, never a rerun.

## 4 · `List<T>` and `Slot<T>` — durable sequence identity

`List<T>` is contiguous storage plus a generational slot directory and a delta log.
`Slot<T>` is **item identity, never a position**: minted per insertion, `Copy`, generational,
and it transfers across containers of the same `T` (drag a card between columns; the caret
mid-edit in that card's text input survives the move).

```rust,ignore
let slot: Slot<Column> = self.columns.push(Column::default());
self.columns.insert_at(index, slot, value);   // slot supplied: identity preserved
self.columns.take(slot);                      // -> T; slot stays valid for re-adoption this turn
self.columns.move_before(slot, other);
self.columns.edit(slot, |col| { .. });        // bumps that slot's revision only
```

Reads dereference to `[T]` — the item store stays contiguous at every scale (see 08 §4 for the
paged/chunked internal representation that makes this affordable past the vector tier); it
intentionally has no `DerefMut<[T]>`. The op set is sealed and each op has a known inverse
(`push⁻¹ = take`, `move⁻¹ = move`), which is exactly what makes `List` mutations undo-eligible
for free (§7).

**Promotion changes delta grain, not meaning.** `&Vec<T>` is the degenerate realign instance of
the same sequence algebra (`SeqSource`, 03 §2): the same verb, a coarser "everything moved"
delta. Delivery revalidates every slot; stale state is `Option`, never a panic. A row that needs
durable local state while backed by a `Vec` produces a located diagnostic naming `List<T>` as the
fix.

Domain-keyed collections (persistence, collaboration) upgrade with one optional parameter:
`List<Thread, ThreadId>` via `.keyed_by(Thread::id)`. The second parameter types the durable-key
*index* only — the key value lives in `T`, `Slot<T>` remains the only runtime identity, and the
key never aliases slots or reintroduces position. This is the deliberate storage-ladder extension
pass 11 §11 explicitly left room for (recorded in 00 §9). Duplicate domain keys are located errors
naming both insertion sites. Runtime `Slot<T>` and durable domain keys never alias — see 09 §2
for the three-layer identity story this feeds.

## 5 · `Entity<T>` — independent ownership

`Entity<T>` is for genuinely shared, independently scheduled ownership — a document several
viewports present, a service several panes observe. It is not the default wrapper for every
view; a component that is merely owned by its parent stays a contained field (rung 2 of the
ladder).

```rust,ignore
struct Workspace {
    project: Entity<Project>,           // shared, independently scheduled
    editor: Editor,                     // contained: a plain field, its own View
}
```

An entity has no public `Deref` (00 §9 resolves this: the conditional soundness proof in earlier
passes is not carried to stabilization). Two ways to read one:

- **Imperative code**: `entity.inspect(|state| ...)` — a scoped, generation-checked read.
- **`view`/`changed`**: the entity's own `#[derive(Delta)]` recurses through the field —
  `d.project.root` reaches through `Entity<Project>` into `Project`'s delta exactly as if
  `project` were a contained struct (04 §1). This is what makes entities compose into the same
  recursive-delta story as everything else — there is no separate "entity event" mechanism.

Mutation is a queued, non-reentrant method call: `self.project.send(Project::rename.with(name))`.
`send` never invokes user code synchronously; a read after `send` sees the current committed
epoch, not the pending write. The full kernel (allocation, generation checks, weak construction,
plugin erasure) is [09-boundaries.md](09-boundaries.md).

## 6 · `Handoff<T>` — occurrences as durable linear state

Some data is not a resting state but a consume-once value: "the user picked X," "the child is
done." `Handoff<T>` is that source, promoted from the kernel's `Take<T>`-style linear-move
semantics to a first-class field type (L10, 00 §4):

```rust,ignore
struct Composer {
    stage: Signal<Stage>,
    sent: Handoff<OutgoingMail>,        // linear: give once, take once
}

fn send(&mut self) { self.sent.give(self.outgoing()); }

// parent, in changed:
fn changed(&mut self, d: Changed<'_, Inbox>) {
    if let Some(mail) = d.composer.sent.take() {
        self.deliver(mail);              // taking IS the acknowledgement
    }
}
```

`Handoff<T>` is *state* — a `Source` like any other — which buys guarantees no event bus offers:

1. **Durable in flight.** Journaled like every other source (§9); give, crash, restore: the
   value is still there to take.
2. **Untaken is diagnosed, never silent.** A handoff still full at turn-end is a located
   inspector report naming giver and non-taker.
3. **Ordering and coalescing come from the publication log.** Two gives in one turn: `take()`
   drains FIFO (loop, or `.drain()`); `Handoff<T, Latest>` keeps only the newest — a declared
   policy, the same vocabulary as `Remote<T>`'s lanes (05 §8).
4. **Stamps apply.** A handoff filled by superseded or revoked async work is rejected by the same
   admission every publication passes through (05 §7).

`#[derive(Delta)]` generates `Option<Given<T>>` for a `Handoff<T>` field, mirroring
`Option<Change<T>>` for a `Signal<T>` field (§8).

## 7 · `Target<K>` — addressable, data-free

`Target<K>` is a `Copy`, generational, typed name for a mounted part — a text input to focus, a
test to query, a capability to visit. It is not storage: content flows through the field it
names, and `Target<K>` resolves fresh on every use. It serializes like `PhantomData` (unit in,
fresh mint out) because its persistent content is nothing — a restored process re-mints it
anyway. Full semantics (claim, focus-scope defaults, the typed capability visitor used by tests
and accessibility) are [04 §4](04-reactions.md).

## 8 · `#[derive(Delta)]` and the shape of change

`#[derive(Delta)]` is a **data derive**, structdiff-shaped, with no UI-semantics content — it is
the serde-precedent macro, not the one UI macro (`#[reactive]`, 04 §6). It is **required only on a
type that is observed across a boundary**, and that requirement is *pulled by the observer*: you
add it to `Composer` exactly when some parent writes `d.composer.field`, and until then it is
absent. A missing derive is not a silent gap — the read is a located error naming the fix (derive
`Delta`, or fall back to `d.composer.any()`, the coarse changed-marker degrade tier). A leaf that
crosses no boundary derives nothing; the ceremony lands on the one edge that earns it (04 §1.2):

```rust,ignore
#[derive(Delta, Serialize, Deserialize)]
struct MailStore {
    unread: Signal<u32>,
    account: Signal<String>,
    labels: List<Label>,
    sent: Handoff<OutgoingMail>,
}
// generates TWO things: a full nested delta (for a parent projecting through this field) …
pub struct MailStoreDelta {                  // the full T::Delta — every field, for `d.store.unread`
    pub unread:  Option<Change<u32>>,       // Change { old, new }
    pub account: Option<Change<String>>,
    pub labels:  Option<ListDelta<Label>>,  // splice/edit/move — richer than an array diff
    pub sent:    Option<Given<OutgoingMail>>,
}
// … and a boundary-only SELF view (the argument to `MailStore::changed`), which exposes only the
// fields whose timing MailStore does not clock — here, none are boundary sources, so a MailStore
// with no child/entity/reader field has an inert self-view and reacts to nothing (04 §1.2).
```

- **The derive classifies each field, boundary vs local.** A contained `T: View`, `Entity<T>`,
  `Reader<T>`, and `List<T: View>` are **boundary sources**; a `Signal<T>`, plain `T`/`Vec<T>`, a
  given `Handoff`, and `Target<K>` are **local state**. The full `T::Delta` above carries every
  field (a parent projects through it); the *self-view* `Changed<'_, Self>` carries only the
  boundary sources, so a component cannot read its own scalar's delta — that field is not on the
  type (04 §1.2). Derive-don't-react (L7) is a compile-time shape, not a lint, on the input side.
- **Generated fields, not methods.** Access is plain field syntax — `d.store.unread` — and
  nothing is callable that isn't visible in the (rustdoc'd, `cargo expand`-stable) generated type.
- **The delta is a lazy view over the turn's publication log**, not a materialized struct: an
  untouched path costs nothing, and assembly is `O(publications this turn)`, never `O(fields)`. A
  field whose type has no `#[derive(Delta)]` degrades to a changed/unchanged marker with `Deref`
  to the new value.
- **Recursion follows fields.** `d.store.unread` on an `Entity<MailStore>` field reaches through
  the entity into `MailStore`'s own delta exactly as it would for a contained struct — see 04 §1.
- Full reaction-side consumption (`Changed<T>`, `changed`, guard-the-function over old/new) is
  [04-reactions.md](04-reactions.md).

## 9 · Structural journaling: restore is re-derivation

> **Applications name sources. Everything derived from a source is owned by the framework at the
> place it is mounted, and is rebuilt from its sources whenever it needs to exist again** (L11,
> 00 §4).

**State is journaled through `serde`, not a bespoke `Save` trait.** An owner's persistent state
is the composition of its fields' persistent state, bound by the ecosystem's own
`Serialize + DeserializeOwned`. GPUI ships the impls for its own containers:

- `Signal<T>` serializes as `T`.
- `Entity<T>` serializes as a journal node reference.
- `List<T>`/`Slot<T>` serializes as a stable node reference; slots are re-minted as fresh
  generational values on restore, remapped entirely inside the journal.
- `Target<K>` serializes as unit and deserializes by minting fresh (§7).
- `Handoff<T>` serializes its held value if one is in flight.

No application type contains an id, and no application type derives anything GPUI invented —
`#[derive(Serialize, Deserialize)]` is the whole contract. This buys composability everywhere
serde reaches: the same state tree exports to JSON for a bug report, snapshots into a test
fixture, or crosses a sync protocol with zero extra impls.

**Everything derived rebuilds itself.** Producers are declared from inputs; the reader graph,
display projections, and caches are all *derived*. Restoring the sources re-arms every producer
concurrently, through the same admission machinery as any other turn (05 §7). So:

> **Cold start, restore, and reconnect are one code path: construct from sources.**

A workspace item's constructor stores journaled domain data (a path, an anchor set, a mode); the
async producers declared from those fields do the resolving on first launch *and* on restore,
identically. Save is the journal. Migration uses serde's own vocabulary first (defaults, aliases,
untagged fallbacks); explicit journal-level migrations handle genuinely breaking changes, owned
exactly where the data is declared. Static heterogeneity is a plain enum (`Tab::Editor(..) |
Tab::Terminal(..)`), matched exhaustively in `view` — a registry for an application's own closed
set of types was always a plugin mechanism misapplied; polymorphic restore (`typetag`/serde, not a
framework catalog) appears only at the actual plugin boundary (09 §2). The journal is explicit at exactly one call —
`Application::new().journal("workspace")` — nothing persists ambiently.

## 10 · History: the model records, the UI tags

Undo/redo/persistence is a library — `gpui_history` — over the kernel's delta log, not kernel
law: the kernel ships sealed invertible deltas, an opt-in per-turn delta log, an `Origin` tag on
transactions, and a gesture token. Everything else below is `gpui_history`, linked by nothing the
kernel depends on.

> **The model records. The UI tags.** Capture is automatic and is always a value. Tagging is
> presentation policy. Closures never enter the journal. An undo is an ordinary transaction with
> `Origin::Undo`.

### 10.1 Two graphs, not one stack

| Graph | A node is | For |
|---|---|---|
| **Revision DAG** | a `Revision` — a state the model occupied plus the `Delta` that produced it | never-lose-work, timeline UI, crash recovery, collaboration |
| **Undo `Track`** | a `Node` — a *tagged* revision this activity considers one Cmd-Z | the Edit menu, per-field undo, per-canvas-stroke undo |

A `Track` is a cursor over the DAG plus a filter of which revisions this activity tagged; several
tracks may view one DAG (the document timeline, the Edit-menu track, a field's IME-aware lane, an
inspector's own track). Linear undo moves the track cursor toward the root; a new edit after
undo *forks* the DAG rather than destroying the abandoned path — a history panel can still
address it.

### 10.2 The trait stack

Four traits, learned once:

- **`Delta`** — `apply`, `invert` (self-contained, no closure over `model`), `compose` (adjacent
  same-kind deltas fold; typing bursts, slider scrubs, stroke samples), `transform` (operational
  transform against a concurrent delta — required for text and for selective undo), and
  `invertibility()` (`Exact | Snapshot | Coarse | Irreversible`). Every sealed cell op — `List`
  splice/edit/move, a `Signal` write, a text splice, a dock delta — already implements this;
  application code writes it once for a domain op that isn't a sealed cell op.
- **`History`** — the opt-in marker: `type Scope: TrackScope = MainHistory;` names which activity
  this owner journals into; `type Store: HistoryStore = MemoryStore;` optionally persists across
  Sudden Termination. Implementing it is the whole cost; a `Track` lives in a side table keyed by
  the owner's generation, so the struct does not grow a field unless the author wants to hold a
  `TrackRef`.
- **`Track<M: History>`** — `undo`/`redo` (ordinary transactions, `Origin::Undo`/`Origin::Redo`),
  `undo_node(id)`/`redo_node(id)` (selective: invert one past node, transform later tagged nodes
  across the inverse, commit as one new revision — the DAG is never rewritten), `barrier()`
  (stop coalescing), `current`/`set_current` (timeline scrub).
- **`Tag`** — `Silent` (captured, invisible to Cmd-Z: IME composition, in-flight drag, remote
  echo), `Point { label }` (one press of Cmd-Z), `Coalesce { key, label }` (merges with the
  previous node while the coalesce key matches and no barrier fired — typing, scrubbing, stroke
  sampling). The default tagger is the gesture token the dispatcher already mints for
  capture/cancellation.

```rust,ignore
impl History for Board { type Scope = MainHistory; }

impl Board {
    fn add_column(&mut self) { self.columns.push(Column::default()); }   // captured automatically,
}                                                                         // tagged Point("Add column")

fn send(&mut self) {
    self.track().commit("Send", |board| {
        board.draft.clear();
        board.threads.insert_at(0, /* .. */, thread);
    });                                                                  // one Point, two lanes
}
```

`commit(label, f)` is the only grouping primitive; nested `commit` calls flatten with the outer
label winning. Two signal marks are deliberate and live at rest, not as view-time annotations:
`.journaled()` opts a field in even on a non-`History` owner (a widget with its own tiny track);
`.ephemeral()` opts a field out on a `History` owner (a scrub position is not Cmd-Z).

### 10.3 Routing: focus picks a track

Cmd-Z resolves, at key-down: (1) if a composition is open, cancel it first — not a track op; (2)
the focused document lane (every text field's own document-scoped track — `impl History` with
`Scope = DocumentHistory`); (3) the focused
surface track (a canvas, board, sheet implementing `History`); (4) the nearest ancestor exposing
`.history::<Scope>()`; (5) the union of exposed tracks in focus order, labelled with the winning
node's `label()` — the Edit menu's last resort. A generic library never names the host's storage;
it resolves the nearest exposed track.

### 10.4 What is never undone

External effects are never falsely undone: `Announce`, clipboard writes, haptics, and
action/`Remote<T>` completions carry no DAG entry. An optimistic mutation is a tagged apply whose `Meta` records the
request id; a server reject applies the inverse with `Origin::System` and a `Silent` tag (the
user already saw the optimistic `Point`; a second Cmd-Z entry would be cruel).

### 10.5 What is captured, what is honestly not

| Write | Captured when |
|---|---|
| `List` op (`push`/`take`/`insert_at`/`move_before`/`edit`) | owner implements `History` |
| `Signal` write | the cell is `.journaled()`, or the owner is `History` and the cell is not `.ephemeral()` |
| plain field assignment | only inside `commit(label).replace(selector, value)` — bare `self.title = x` is honestly not undoable |
| `Handoff` give/take | captured with the owning cell's policy |
| executor work (`.action(...)` completions, `Remote<T>` sends), `Announce`, network | never |
| remote apply | captured on the DAG with `Origin::Remote`; local tracks that did not originate it never tag it |

Mutation descriptors and `List` ops are journal-eligible for undo with zero author work, because
both are sealed delta sets with known inverses — the same bright line that makes arbitrary
`&mut self` bodies honestly *not* capturable: apps that want deep undo route edits through
`commit(...)` or promote the field to a journaled cell.

Gates H0–H10 (zero tax on non-`History` owners, round-trip oracle, Sudden-Termination recovery,
selective-undo conflict naming, IME/echo, collaboration origin, panic atomicity) are in
[10-quality.md](10-quality.md) §1.
