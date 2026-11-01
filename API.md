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
