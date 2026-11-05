# Collections, Virtualization, and Text Ranges

Status: normative contract for the one-register model
([05-one-register.md](../05-one-register.md) §2). This revision replaces the 03-era draft
(`scope.keyed_list()`, `Update`-mediated ops, `#[derive(Reactive)]` row fields); the identity
matrix, retention reasons, accessibility rules, and text-range contract carry forward.

> **Revised by [07-the-ladder.md](../07-the-ladder.md):** (1) ops take **`&mut self`**, not
> staged `&self` — `view(&self)` cannot compile a write; nested writes are
> `get_mut(slot)`/`edit(slot, f)` and republish the outer slot (one row re-describe, priced in
> 07 §1); (2) `keyed(&vec).by(..)` is deleted — contained `Vec` rows are anonymous by contract,
> and identity-needing rows promote to `List` (the located error names it); (3) `.with` binds
> row-fn arguments only (07 §3): a `SignalRef` argument is a pass-through handle that reruns
> nothing, `&Signal` is a live row input. Where code blocks below show `&self` ops or `keyed()`,
> read them under 07's rules.

## The two consumption modes are the triad applied to collections

| Mode | Spelling | Cost of a parent rerun | Cost of a mutation |
|---|---|---|---|
| **Contained** (value move) | `Vec<T>` + `.children(iter)`, or `keyed(&vec).by(T::id)` at the parent | re-emit + tape diff (keyed realignment when declared) | owner transaction → rerun + diff |
| **Wired** (reference move) | `.children(&list, row_fn)`, `virtual_list(&list)`, `grid`, `tree` | **one version check; rows are not iterated** | the op's splice: only new/edited/moved rows run |

`Vec` is the plain-Rust default and is not deprecated by anything below. `List<T>` is a
**cell** the way `Signal<T>` is: its inherent ops publish typed splices to its consumers. There
is no third mode and no returned-delta API — a handler cannot hand back a `ListDelta`, because a
hint that does not skip `view()` is a lying API. The delta algebra survives *inside* the
container (ops, inverses, oracle, undo journal), never as a handler return.

## `List<T>` — identity derived, keys optional

```rust,ignore
struct Board { columns: List<Column> }              // identity DERIVED: inserts mint Slot<Column>
struct Inbox { threads: List<Thread, ThreadId> }    // domain-keyed upgrade: .keyed_by(Thread::id)

let slot = self.columns.push(column);   // -> Slot<Column>: typed, Copy, generational
self.columns.insert_at(index, slot, v); // slot supplied = identity preserved (transfers!)
self.columns.take(slot);                // -> V, slot stays valid for re-adoption this turn
self.columns.move_before(slot, other);
self.columns.edit(slot, |col| { .. });  // bumps that slot's revision; returns the closure's value
```

`Slot<T>` is **item identity, not position**: it is minted per item, follows the item across
moves, and *transfers between containers of the same `T`* (`take` from one `List<Card>`,
`insert_at` into another, same slot — a caret mid-edit survives a cross-column drop). No id
newtypes, no `next_id` counters, no mandatory key parameter. `List<T, K>` via `.keyed_by(T::id)`
adds domain keys when the data has real ones (DB rows, persistence, cross-process references);
then ops and selection address by `K`. Serialization serializes items (and domain keys); slots
are runtime identity and never serialize.

- **Ops take `&self`** (staged interior mutability, same turn discipline as `Signal`): reachable
  through shared references, batched per turn, published at the turn boundary. They are equally
  callable inside `fn(&mut Self)` owner handlers; the difference is only what wakes.
- **The op set is sealed and invertible** (`push⁻¹ = remove`, `move⁻¹ = move`, `edit` journals
  via the item's own draft policy or marks the entry coarse). Wired-collection mutations are
  therefore journal-eligible for undo with zero author work — the same bright line as mutation
  descriptors. The algebra, tagging, and Cmd-Z routing are [08-history.md](../08-history.md);
  `ListDelta` is a sealed `Delta`. Slot identity is why selective undo of structure usually
  *commutes* (a rename of A and a move of B do not share an address).
- **Keys**: slots are minted by default (`push(v) -> Slot<T>`); domain keys come from
  `.keyed_by(T::id)` at construction (then ops also address by `K`). Duplicate domain keys are
  located errors naming both insertion sites. A key may not mutate while resident; changing
  identity is remove+insert.
- **Nested cells compose**: a `List` or `Signal` *inside* an item is its own cell.
  `board.columns.get(id).cards.push(..)` publishes on the inner cell only; `columns.edit(id, f)`
  publishes the outer slot (that row reruns). Choose the narrowest cell that owns the change.

## Identity is the container's — the key identity contract

Unchanged in substance from the previous draft, now stated against slots:

| Operation | Node/owner/local state | Position |
|---|---|---|
| insert | mount new | insert |
| remove | unmount and dispose | close gap |
| move | **preserve** | change |
| edit resident slot | **preserve** | preserve unless an arrangement moves it |
| replace under a new key | unmount old, mount new | may reuse the position, never the identity |

Focus, IME composition, pointer/drag capture, running motion, `hero` continuity, and
accessibility focus follow the **slot** across moves — including across containers when a drag
protocol moves a slot between two `List`s (the payload's slot knows its source container;
`Drop { payload, source, target, index }`). This contract is stricter than a generic `VecDiff`:
a stream that says `UpdateAt` and means "remount" is a bug here, not an alternative.

Per [05 §2](../05-one-register.md): **no `.key()` method exists on elements.** Contained
collections declare keys once at the parent (`keyed(&vec).by(T::id)`, optional
`.revision(T::rev)` with the dev-build revision auditor, `.with(&shared_input)` for declared
shared inputs). Unkeyed dynamic runs are stateless by contract, enforced at lowering:
identity-requiring elements (text inputs, focusables, component children, `hero`, capture-taking
gestures) inside one are a located mount error naming both fixes.

## Wired rows

```rust,ignore
// The common case is the bare pair — a reference and a function, no builder:
column().children(&self.threads, Inbox::thread_row)

// Arrangements live on the collection reference, keeping the pair uniform:
column().children(
    self.threads
        .filtered(&self.query, |t, q| t.title.contains(q))
        .sorted(&self.order, Thread::sort_key)
        .with(self.filter.reader()),               // declared shared input (Copy, versioned)
    Inbox::thread_row)                             // fn(&Thread, ..) -> impl Element + '_

// list(..) / virtual_list(..) builders exist only for list-level chrome:
virtual_list(&self.threads).selected(&self.selection).pan(Pan::VERTICAL)
virtual_list(&self.threads, Inbox::thread_row).selected(&self.selection).pan(Pan::VERTICAL)
```

- **The row's read set is machine-checked**: no `&self` is spellable, so a row reads its item
  plus declared inputs, nothing else. Signal reads inside a row subscribe *that row's* producer;
  per-row live values are wires (`progress_bar(&item.progress)`).
- **Handlers on the item type route to the slot** (05's first-parameter-is-the-address rule):
  `row().on_click(Thread::toggle_read)` mutates this row's item, bumps its slot revision, reruns
  this row. Cross-cell operations (move a card between columns) are owner handlers by
  construction — the signature cannot name two slots.
- **Arrangements are declared, versioned inputs.** `filter_with`/`sort_with` re-arrange when
  their cell input or the membership changes; a field-aware sealed key re-evaluates one affected
  row with logarithmic order work; an arbitrary closure predicate has wildcard dependencies and
  conservatively rebuilds, reported explicitly. The **arrangement, not the store, is what
  downstream consumers index** — the grid's Lattice, selection order, and the virtualizer all
  subscribe to the arranged order (the freeze+filter drift failure is unrepresentable).
  Slint `ModelExt` (`.filter().map().sort()`) is this chain; `FilterModel.unfiltered_row` is
  **`Slot`** — `list.slot_at(arranged_index)` / `list.position_of(slot)` are the two maps.
  Selection stores slots, never arranged indices. `reset()` when a predicate's captured
  environment changes is the wildcard rebuild already priced when a bound cell publishes.
- **List-level protocols**: `.selected(&signal)` is the two-way selection wire (click writes the
  key; the wire styles the row; `.on_select(Owner::method)` is owner interception). Range
  selection stores anchors by key plus order revision, with defined clamping if an endpoint
  disappears.

## Performance tiers (measured, not assumed)

Order storage is adaptive with published crossovers and hysteresis: inline `Vec`/`SmallVec`
below a measured `N_vec`, sum-tree order/extent maintenance above it. O(log N) claims apply only
in the promoted tier; the vector tier documents O(N) moves. Resident-slot edits that don't
affect membership/order are O(1) lookup plus that row's work. Batch ops coalesce within a turn
with stable move/insert ordering. Counters report promotion thrash, arrangement rebuild causes,
and rows-run-per-op — the wired list's whole claim is that rows-run ≈ rows-changed, and the
counter proves or falsifies it (G10's contained-oracle fuzz applies).

## Virtualization is a view policy, not graph disposal

`virtual_list(&list)` adds a window over the wired contract: prefix-sum extent index
(fixed / known / estimated-corrected extents; 2D is the grid's Lattice), viewport + overscan,
**anchor by key + offset-within-row** (corrections preserve the anchor; inserts above adjust
around it), and placeholder policy under fling (never dropping input, never replacing a focused
editor; prefetch idle-priority and cancellable).

Rows have runtime **retention reasons**, never an application `.pin`:

- keyboard focus or focus-restoration target;
- active IME composition or text-input session;
- pointer capture, pressed gesture, **drag source and active drop target** (autoscroll must not
  unmount them — [06 §9](../06-cross-cutting.md));
- **sticky/pinned span** (compositor pin, 06);
- accessibility focus/action target;
- transition/exit motion, `hero` capture;
- an explicit product keep-alive lease (budgeted).

Subsystems acquire and release reasons automatically — they cannot mount without informing the
owner runtime, so re-declaring them per row would only create under-declarations. Recycled
allocations reinitialize with a new generation; local state never bleeds across slots.

Accessibility is not limited to visually mounted rows: a virtualized interactive list provides a
semantic-item projection (stable id, label/value/state/actions, set size/position) within a
budget; screen-reader traversal materializes and pins the target before dispatch. Offscreen
content is never hidden from AT to protect a perf claim; semantic materialization has its own
counters.

## Trees and grids

Contracted in [06 §2–3](../06-cross-cutting.md); the collection-level obligations they inherit
from this file: pull-based children (`Children::{Leaf, Ready(&[N]), Pending}` — `Pending` is one
placeholder row, `Ready` is one splice), cursor ≠ marked set, collapse rebases cursor/marks/drop
onto the parent or next visible row, `DropGap` validates/rebases or rejects against order
revisions on concurrent change, and the Lattice indexes the arrangement.

## Text range collections

Large text is a rope/sum tree, never a `List<char>`. Edits produce typed byte/UTF-16/line
range deltas with revision and affinity; display lines are cached by content/style/wrap
constraints. Platform IME contracts name their units (`Utf16CodeUnits`, never a bare `usize`)
and revision; a stale range is rejected or transformed by declared policy, never applied to a
different document. An edit: rope summaries O(log n + edit) → invalidate intersecting shaped
runs → rebreak affected paragraphs → reposition following blocks via prefix sums → remap
selection/diagnostics/a11y ranges. Fallback/scale/bidi/wrap-width changes widen the range
deliberately; counters measure bytes shaped and lines rebroken so "one paragraph" cannot hide a
document reshape. The document's selection algebra (N cursors, column mode, IME third range)
lives here, not on FlowIndex (06's law).

## Required tests

- insert/remove/move/edit/replace matrices with slot state, owner counts, and focus/IME/capture
  continuity — including cross-container moves via drag;
- duplicate keys, mutated keys, container-minted vs domain keys;
- wired-vs-contained oracle: same data through both modes renders identically (G10);
- rows-run-per-op counter assertions (push runs one row; edit runs one row; arrangement change
  runs affected rows and reports cause);
- routing: item-type handlers hit the correct slot after moves; cross-cell handler signatures do
  not typecheck as slot handlers;
- `.with`/`filter_with` staleness (input change re-arranges; no silent skip), wildcard-predicate
  rebuild reporting;
- variable-height correction above/below viewport, fling during measurement, sticky-pin +
  autoscroll retention;
- selection anchors through reorders, endpoint disappearance clamping;
- undo journal round-trip of op sequences, including `edit` coarse entries;
- Unicode grapheme/bidi/IME revision mapping through rope edits;
- churn returns mounted rows, observers, fragments, leases, and tasks to baseline.
