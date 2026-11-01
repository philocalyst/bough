# bough — public API and how it lowers

Spellings follow the GPUI-rework contract (archive `reactivity/CONTRACT.md`, local `final/00-model.md`): no `cx`,
`View::view(&self)` describes, state lives in plain structs, methods are routes, `changed` is the one effect site.
What is new is *how each spelling lowers*: every public form compiles to a fixed, monomorphic patch path, and the
cost of each rung is stated.

## State

```rust
pub struct Signal<T> { value: T, head: WireHead }   // WireHead = Cell<u32>: wire-list index + QUEUED bit
impl<T> Deref for Signal<T>    { /* plain read; no tracking in the static path */ }
impl<T> DerefMut for Signal<T> { /* sets QUEUED (one OR) if the signal has wires, returns &mut T */ }
impl<T: PartialEq> Signal<T>   { pub fn set(&mut self, v: T) -> bool /* equality cut-off, then mark */ }

pub struct List<T>   // Vec-backed rows with durable Slot<T> identity; splice/move/edit deltas recorded per turn
pub struct Entity<T> // independently owned/scheduled state (window-scope capability, generational)
pub struct Reader<T> // async/derived question: Ready(T) | Pending | Fault; holds last Ready across revalidation
```

`Signal<u32>` is 8 bytes. A signal never owns a pointer to the runtime; its 4-byte head indexes the owner's wire
table, assigned when a view first binds it (interior mutability: binding happens under `&self`).

## Views and elements

```rust
pub trait View: Sized + 'static {
    /// When true, `view` is called once at mount; afterwards only wires, lists and readers change the subtree
    /// (zgx's "components mount once"). When false, an arbitrary `&mut self` handler re-describes this owner
    /// and diffs it against its records (immediate mode). Default false.
    const FIXED: bool = false;
    fn view(&self) -> impl Element<Self> + '_;
    fn changed(&mut self, _d: Changed<'_, Self>) {}
}
```

Builders are typed (`Column<(A, B, C)>`, `Text<S>`, `Quad`, `Button<L, R>` …) and implement the sealed
`Element<V>` trait; `El<'_, V>` erases a builder only where branches must unify. Each element type exposes
`const NODES: usize` for its static part, so a static subtree's template is computed at compile time and a mount
writes it with one copy.

Operands accept the closed `IntoProp<P>` family, one lowering per form:

| operand | lowering | per-update cost |
|---|---|---|
| `T` (constant) | written into the record/instance at mount | none |
| `sig!(self.f)` | wire: `offset_of!` field offset → (record, property) | one patch fn |
| `&Signal<T>` (plain borrow) | constant of this description | re-description only |
| `sig!(self.f).map(f)` (`f: Copy`, ≤ 16 B of captures) | wire with the recipe inline | `f` + one patch fn |
| `&Reader<T>` / `reader.project(...)` | wire guarded by readiness; Pending holds the previous value | patch on Ready |
| `&List<T>` + row fn | keyed region; rows are owners; deltas splice runs | per inserted/removed row |

**Wires are installed from compiler-proven field paths.** `sig!(self.count)` (a `macro_rules!`, not a proc
macro) expands to `Wired::new(&self.count, core::mem::offset_of!(Self, count))`; nested struct paths work
(`sig!(self.stats.p99)`). `offset_of!` only accepts struct field paths, and a struct field can never change
location or type for the owner's life, so the patch's `owner + offset` read is sound. A plain borrow
(`text(&self.count)`) is a constant of this description, refreshed when the owner re-describes.

As built (`crates/bough/src/signal.rs`): `sig!(self.a.b)` inside a `View` impl, `sig!(v: Type => a.b)` where the owner is
a value (a list's row function). The expansion keeps the field access *outside* its `unsafe` block, so a union field
or a packed field needs the caller's own `unsafe` and a path through `Box`/`Deref` fails `offset_of!` (compile-fail
doctests); `Wired<'_, T, V>` carries the owner type and only a `Mounter<'_, V>` of the same `V` lowers it. Operands:
`sig!(..)` (a `Signal<T>` property), `sig!(..).map(f)`, and for text `sig!(..).num(f, NumFmt)` (the value rendered into
the label's 16-byte vector, no `String`). `Signal::head`, the wire arena and `Rt::add_wire` are crate-private (a copied
head would run wires of another type).

Rejected (review of the integration prototype, 2026-11-01): inferring the offset at runtime from
`addr(field) − addr(owner)` and accepting it when it lies inside `size_of::<V>()`. That proves the signal is
inside the owner's bytes, not that it stays there: a signal inside an enum variant or `Option` field has its bytes
reused when the variant changes, and the wire would then read another type's bytes as `Signal<T>` — undefined
behaviour reachable from safe code. The runtime range check survives only as a debug assertion.

## Routes and turns

- `Self::COUNT.update(|c| *c += 1)` — a typed route: the runtime knows the exact source; the turn runs that
  signal's wires only.
- `on_click(|m: &mut Self| …)` — arbitrary handler: after it returns, the runtime scans the owner's wired
  signals for `QUEUED` (O(wired fields of that owner)), runs their wires, and — unless `FIXED` — re-describes
  the owner and diffs.
- Timers/intervals/inboxes deliver through the same two forms.

Turn: admit input → invoke owner → settle wires (creation order = topological order; sort only if disturbed) →
dynamic projections in height order → `changed` (effects; may enqueue a new wave) → layout (only records whose
layout inputs were patched) → damage/cull/draw/present for the new epoch.

## Async (fetch high, block low)

`let results = query.map_async(search);` declares a Reader owned by the component. Consumers that read a Pending
reader observe Pending; nothing else waits (passing is not reading). `boundary().loading(f)` shows its fallback
only until first readiness. On revalidation the reader holds its previous value and every wire downstream of it
holds too until the cohort is ready, then the cohort patches in one epoch (no tearing, no transition API).
`is_pending(&reader)` / `latest(&signal)` are per-source questions answered by the runtime, usable in any
operand.

## Cost table (targets, to be measured)

| operation | target |
|---|---|
| typed route → one colour wire → frame ready | < 2 K instructions before the GPU encoder |
| text wire with a 6-char number (format + shape + instances) | < 500 instructions |
| idle | 0 instructions, no wakeups |
| mount of a 1,000-record static template | one copy + operand writes |

## As built in round 3 (branch `api`; measurements in `crates/bough/API-RESULTS.md`)

What the contract above promised and the façade now has, and where the implementation changed it.

**`List<T>`** (`list.rs`, `listview.rs`). A field of an owner. Items live in a slab of 64-item chunks that
never move, so an item is an *owner* at a stable address; `order` is a vector of slot ids; `Slot<T>` is
(index, generation). Ops: `push insert remove pop take(slot) clear truncate retain move_to move_before swap
reverse sort_by sort_by_key edit(pos) edit_slot(slot) set`; reads: `len get slot get_slot position iter
slots` (no `Deref<[T]>`: the store is chunked, not contiguous). Each op appends to a per-turn delta log
(`Insert Remove Move Permute Edit`) only while the list is displayed; a slot freed this turn is reused only
after the turn settles, so a log entry never names two items.
`list(sig!(self.items))` (rows are `T: View`, mounted by `T::view`) or `list_with(sig!(..), fn(&T, &mut
Mounter<T>))` shows it as a **stack of rows**: the container is a sized leaf of its parent's layout (the
region applies its padding and gap), every row's top box is a layout group of its own in a space of its
own, and row `k` sits at a prefix sum of the heights before it. Contract change: a list region is always
a vertical stack (no `horizontal`, no flex distribution among rows) — that is what lets an edit lay out
one row and a splice move the rest by space writes. `sig!(self.items)` proves any headed cell, not only
`Signal` (`signal::Headed`).

**`Entities<T>` / `Entity<T>`** (`entity.rs`). The slab is a field (`Entities::with_capacity` carves whole
chunks up front); `Entity<T>` is a `Copy` generational handle; `get`/`inspect` read, `edit` mutates (logged
once per turn), `remove` kills the handle. `entity_view(sig!(self.docs), handle)` mounts `T::view` with
the entity as owner (several views of one entity follow its edits). Contract change: mutation is a direct
`edit` inside the owning turn, not a queued `send`; an entity view inside a re-describing owner is not
supported.

**`Reader<T>`** (`reader.rs`). A field (no `Deref`). `produce(sig!(self.query), sig!(self.results), f)`
declares the producer in the view (`f: fn(&S) -> Request<T>`, a `Send` closure); it runs whenever the
source's value changes (frozen input compared by `==`), at mount too (fetch high). The executor is
`Exec::Manual` (deterministic: `App::run_jobs`) or `Exec::Threads`. Completions are admitted at the start of
the next turn (`App::turn`/`poll`) by ticket; a superseded one is dropped. Pending holds: the reader keeps
its last value, the source that began the cohort is `HELD` (a bit in its wire head: the settle scan skips
its non-`latest` wires), `latest(sig!(..).map(f))` opts a sink out. When every member of the cohort is
staged, all commit in one turn (one epoch). Operands: `sig!(self.r).map(|v: Option<&T>| ..)` (before first
readiness `None`), `sig!(self.r).pending().map(|p: bool| ..)`; `Reader::latest()`, `is_pending()`. Not
built: `boundary().loading(..)`, faults/`try_map_async`, `project` regions.

**`FIXED`** (`describe.rs`). Default `false`, as the contract says: every `App::turn` re-describes the
owner and walks the new description in lockstep with the retained records (tape of node ids in preorder;
`Option`/`Vec`/`each` are dynamic parts, realigned positionally and spliced as preorder runs). Every scene
that mounts once declares `const FIXED: bool = true`. Restrictions: a layout box cannot gain or lose its
paint between descriptions, a laid-out text cannot change its font size.

**`changed`** (`changed.rs`). `fn changed(&mut self, d: Changed<'_, Self>)` on `View`: `d.list(sig!(..))`
(the turn's ops), `d.entities(sig!(..))` (edited slab indices), `d.reader(sig!(..))` (committed this turn);
writes settle in the same turn (at most 8 waves). Root owner only; no `#[derive(Delta)]`.

**New operands and elements.** `sig!(self.sel).select(key, on, off)`: all selects of one signal share one
keyed wire (a change patches the old key's and the new key's destinations only). `lanes(n, sig!(self.t),
|t, i| Lane { .. })`: `n` quads as one record and one instance run, evaluated in one pass.

**Soundness additions.** A wire records the address of the head it was linked into; `settle` and
`add_wire` cut a chain whose wire does not match (a `Signal` moved between owners or apps, a list item
taken out and put back), so an index from another arena is never run. A list item leaves the slab with
its heads cleared; its row's own wires are freed at once.
