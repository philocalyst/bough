# 09 — Boundaries

Scope: the non-generic, scope-owned entity kernel (no strong/weak axis), the open-item boundary
(`typetag`/serde in-process, `abi_stable` for dynamic plugins — not a framework catalog), the
three-layer durable identity story, workspace presence as a
supervised protocol, the IPC/typed-ABI law, and the linking decision that keeps incremental
compilation viable.

Depends on: [01-state.md](01-state.md) §5 (`Entity<T>` field semantics), §9 (structural
journaling). Feeds: [10-quality.md](10-quality.md) (gates 10, 24, 25).

---

## 1 · The entity kernel is a typed facade, not a generic runtime

`Entity<T>` is for genuinely shared, independently scheduled ownership — it must not become the
default wrapper for every view, and it must not force a giant dynamically-linked engine crate.
The representation is split deliberately:

```rust,ignore
#[repr(transparent)]
pub struct Entity<T: 'static> {
    raw: RawEntity,               // non-generic generational key into a scope-owned slab
    marker: PhantomData<fn() -> T>,
}
```

`Entity<T>`, `Remote<T>` (05 §8), and typed routes are tiny inline proof adapters; allocation,
generation checks, scheduling, queues, and fault isolation live in non-generic `EntityCore` code.
The concrete type is registered once with a compact method/drop/describe table, so the hot kernel
does not monomorphize per application type — the same `Slot<T>`-proves-then-erases pattern as
`List<T>` (01 §4), applied at the entity grain. Plugin and dynamic-library boundaries use
`AnyEntity` plus a versioned scene/input/accessibility protocol (§2); Rust generic layout is never
treated as a stable ABI.

**`Entity<T>` is a scope-owned generational *capability*, not a reference-counted owning pointer**,
and this is the whole reason there is no strong/weak distinction. Liveness and lifetime are two
different questions, answered by two different mechanisms already in the model:

- **Lifetime is scope ownership (L5).** A producer belongs to the scope that created it — the
  mount site, the journal node, the workspace. That scope frees the entity's `T` when it closes.
  Dropping the last `Entity<T>` *handle* does not free anything and is not how an entity ends; the
  owning scope is. So there are no ownership cycles to break: two entities that name each other are
  two capabilities into the same scope's slab, and both die when the scope does.
- **Liveness is the generation check.** Every `send`/`inspect` on `Entity<T>` is
  generation-checked and returns `Delivery::Closed`/`Option` when the target's generation has moved
  on — a strong handle *already behaves safely after the target is gone*, with no `upgrade` and no
  panic. A dangling `Entity<T>` presents `Fault(Revoked)` exactly as a `Reader` naming a dead owner
  does (L5, 05 §10).

Because a handle neither owns nor keeps-alive, `WeakEntity<T>` has nothing left to be: it was the
patch for a refcount that this kernel does not have. It is deleted (00 §6). One type, `Entity<T>`,
is the capability at every use — shared ownership, back-edges, optional forward queries — and it is
generation-safe at all of them.

Mutation is a queued, non-reentrant method call: `self.project.send(Project::rename.with(new_name))`.
`send` never invokes user code synchronously; a read after `send` sees the current committed
epoch, not the pending write. `with` has one meaning across the surface: bind trailing owned
inputs to a function item — a value is constant, a signal/reader is sampled into an owned value
at dispatch.

### 1.1 Cyclic construction, with one entity type

A genuine reference cycle (a child that must address its own parent) and an optional forward query
are cases where *lifetime*, not event meaning, is the point — and state observation via the
recursive delta (04 §1) already covers most of what a callback handle was ever for. What remains is
construction: an entity that needs a handle to *itself* before its `T` exists. That is minting a
generational key before filling its slot — the same move `List<T>` makes when a `Slot<T>` is minted
ahead of its value (01 §4) — and it produces an ordinary `Entity<T>`, not a second type:

```rust,ignore
let editor = Entity::new_cyclic(|me: Entity<Editor>| Editor::new(me, document));

editor.inspect(|editor| editor.mode());
editor.send(Editor::refresh);   // Delivery::Closed after the owning scope frees it
```

The key exists before `T`; `me` is a live capability whose slot is empty until the constructor
returns and publishes. A read or send through `me` *during* construction generation-fails cleanly
(`Delivery::Closed`) — a capability that names an empty slot is simply not-yet-live, the same
verdict as one that names a freed slot, so a partial `T` is never observed. A panic or failed
construction leaves every copy permanently dead. Crucially, `me` being an ordinary `Entity<Editor>`
held *inside* `Editor` does not leak it: a handle does not keep its target alive (§1), so a
self-held or mutually-held capability is not a leak and needs no weakening. There is no `downgrade`,
`upgrade`, strong/weak axis, `Deref`, or convention about carrying a handle across `await` —
`send`/`inspect` are generation-checked at every call, which is the only safety a handle to
shared, independently scheduled state can honestly offer.

## 2 · Open item sets: reach for the ecosystem, not a bespoke catalog

An "item catalog" is genuinely uncommon, and the right instinct is to not invent one. The open-set
problem decomposes into three cases, and only the last needs any special handling at all — none of
it framework-owned:

- **A closed set (the overwhelmingly common case): a plain enum.** `Tab::Editor(..) |
  Tab::Terminal(..)`, matched exhaustively in `view` (01 §9). No registry, no tags, no dynamic
  dispatch — the linker resolves everything. An app that reaches for a catalog for its *own* item
  types has misapplied a plugin mechanism.

- **An open set in one binary (first-party item types that still all compile together): `typetag`.**
  "Restore a `Box<dyn Item>` from a string tag" is exactly the problem the serde ecosystem already
  solved. An item is nothing but `View + Serialize + Deserialize` with a tag:

  ```rust,ignore
  #[typetag::serde(tag = "zed.editor")]     // ecosystem crate — the string↔type registry IS typetag
  impl Item for EditorItem { /* View + domain methods */ }

  #[derive(Serialize, Deserialize)]         // restore = re-derivation (L11): deserialize IS construct
  struct EditorItem { locator: EditorLocator, view_state: EditorViewState /* .. */ }
  ```

  There is no `ItemCatalog`, no `catalog.register::<T>()`, and no `ItemSpec` bag of function
  pointers. `typetag` registers the tag→deserialize thunk at link time (the same `inventory` pattern
  the command boundary uses, 04 §3); the framework contributes *only* a `KIND` version convention
  for migration, and migration itself is serde's own aliases/defaults — an explicit journal-level
  migration only for a genuinely breaking change, owned where the data is declared.

- **A genuinely dynamic set (separately compiled `.so`/`.dll` plugins): the stable-ABI problem,
  solved by `abi_stable`/`stabby`.** This is rare, it is *not* the same problem as the previous
  bullet, and the design does not roll its own C-ABI layer for it. Such a plugin crosses with a
  versioned manifest and `AnyEntity` proving the item protocol; erasure is confined to that seam,
  never spread through panes, and Rust generic layout is never treated as a stable ABI (§1).

So there is no framework "catalog." What survives as a genuine framework concern is only what the
serialization boundary *forces*: construction requests own every argument they need across `await`
(they never retain a pane/workspace/window borrow), and a failed or unknown restore is a typed
item-load error, not a framework fault — a recovery item retains the original bytes byte-for-byte
and offers retry/migrate/export/remove, so a corrupt or unknown record never silently deletes a
tab. `TypeId` may accelerate an in-process reverse lookup but is never a persisted or wire
identity.

## 3 · Durable identity: three layers that never alias

Runtime identity is generational and process-local: `EntityId`, `Slot<T>`, retained-node keys,
and `Target<K>` are all capabilities of one running process. Serializing one turns an allocator
accident into a protocol, so a persisted item has three explicit layers instead:

```text
SavedItem {                           // journal-internal — no application or item type spells this
    kind: ItemKind + protocol version,
    instance: ItemInstanceId,         // framework-minted, like Slot<T>; never named in item code
    locator: versioned item-owned bytes,   // <- the only bytes an item author writes: its serde
    state: versioned item-owned bytes,     // <-
}
```

**`ItemInstanceId` is automatic, and that is the point.** Only `locator` and `state` are item-owned
— they are literally the item's own `#[derive(Serialize, Deserialize)]` bytes (§2). `kind` comes
from the `const KIND`. `instance` is *minted by the framework* on open and carried in the journal
node exactly as `Slot<T>` is minted per insertion (01 §4): it preserves tab and history
relationships across restore and maps to a fresh runtime `EntityId` on success, and **no
application or item type ever declares, stores, reads, or compares one.** An item author who finds
themselves reaching for an instance id is describing durable *domain* identity — which belongs in
the `locator` (an editor's path, a terminal's session), not in a synthetic per-tab id. The synthetic
id stays where `Slot<T>` lives: below the field law, framework-owned, never in a struct. There is no
universal "domain key" — an editor locator may hold project/worktree/path or remote document
identity plus excerpt-set identity; a terminal may hold a session or relaunch descriptor; an
ephemeral item declares itself unrestorable.

The same durable `ItemKind + locator` vocabulary crosses collaboration: peer-local entity ids and
pane slots never do. A session alias table resolves a remote item reference to the current local
entity after authorization and restore; missing, migrated, and unauthorized locators are typed
protocol outcomes. `Target<K>`, `Slot<T>`, and retained-node keys never serialize (01 §7, §4); a
`List` domain key (01 §4) is the only sanctioned durable collection reference: the key *value*
lives in `T`, the `.keyed_by` parameter types the index over it, and it never aliases runtime
`Slot<T>` identity or reintroduces position (00 §9 records the resolution).

## 4 · Presence: a supervised protocol, not view observation

Follower state, a leader-update channel, and a long-lived apply loop are a distributed system,
and collapsing that into `changed`, a signal assignment, or a detached task loses ordering,
reconnection, identity, and failure semantics. The replacement is an explicit application/
workspace-owned `PresenceSession` — a service `Entity`, constructed with **no route parameter**
(service entities take no constructor routes, per 04 §1/01 §5: the workspace holds the entity as
a plain field and reads its outcomes through the recursive delta, exactly like any other entity):

```rust,ignore
let presence: Entity<PresenceSession> = PresenceSession::connect(transport);
// workspace, in changed:
if let Some(batch) = d.presence.applied.take() { self.apply(batch); }   // Handoff, not a route
```

Every envelope carries `{ session, leader, incarnation, peer, lane, sequence, origin, item_ref,
payload }`, where `incarnation` advances on leader/reconnect/resync, `origin` suppresses echoes,
and `item_ref` is the versioned `ItemKind + locator/session alias` of §3 — never an `EntityId`.
The receiver accepts only the active leader/incarnation, dedups replays, admits contiguous
deltas, and requests a snapshot when a bounded gap cannot close; a new incarnation begins with an
authoritative snapshot that atomically replaces obsolete peer/item replica state. Unknown local
item mappings park or trigger authorized acquisition — never alias another local entity. Applying
one admitted batch is one ordinary reactive transaction, so selection, scroll, followed item, and
peer status never expose a half-applied remote update.

One queue policy cannot serve all presence traffic:

- structural verbs (begin/end following, open/close/switch item, session membership) are
  reliable, ordered, bounded, and version-acknowledged; overflow backpressures or resyncs, never
  drops;
- cursor, scroll, viewport, and ephemeral status are latest-per-`(peer, item, field)`,
  coalescible, and may be replaced under backpressure;
- buffer edits remain the document collaboration protocol, never presence messages.

Transport ingress is `Remote<PresenceEnvelope>` (05 §8) with an explicit lane policy — presence is
an ordinary application of the stream shape, not a special case. Session cancellation revokes
future delivery; reconnect advances the incarnation so an old stream cannot resurrect stale
commands; transport loss publishes a typed `PresenceHealth` plus retry/backoff state rather than
being forged into a domain failure, and never cancels sibling views.

## 5 · The typed-IPC law

Two boundary decisions are restated here as one-line laws because they are cited from several
directions in this folder:

> **Schema-derived typed IPC exists only at actual ABI boundaries.** The dynamic-plugin boundary (§2),
> foreign shells, and separately compiled extensions get versioned, generated, checked contracts.
> In-process component communication is state, deltas, and methods — never serialized commands,
> never a metaobject registry. A string-keyed action registry does not return.

> **Compilation is a tier, not a language.** Const-evaluable subtrees may lower to static tape
> templates with patch tables (08 §7); `#[reactive]` may cut ports and regions (04 §6); any future
> tooling lowers to the same `El`/port/impact schema plain Rust already produces. A second
> runtime, a second tree, or a second semantics is out, permanently, at any measured benefit.

## 6 · Linking

The concrete `El<'f>`, the non-generic entity kernel, and the protocol boundaries above are chosen
so ordinary incremental compilation and normal crate-level linking remain effective — the design
does not require an engine-wide dynamic library the way some competitors do to keep relink times
sane, because the hot kernel is already non-generic per application type (§1). An optional
development-only dynamic kernel may be measured later, but the public API never relies on it and
release binaries never require it. CI tracks generic instantiation count, incremental rebuild time
after an application-only edit, clean link time, artifact size, and exported symbol count — a
change that makes a new `Entity<Foo>` pull a copy of scheduler or renderer code fails the gate
(10 §1, gate 10).

## 7 · Embedding and event-loop attachment

`Application::run` owns a normal platform loop by default. `attach(host)` and `poll(deadline)` —
covered fully in [07-platform.md §7](07-platform.md) — let an external owner (a game engine, an
embedded shell) drive the same turn queue without a second scheduler; nothing in this document
adds a third integration path. A foreign-language integration consumes the same versioned
scene/input protocol §2 uses for plugins, never the Rust `Element`/`Entity<T>` ABI directly.
