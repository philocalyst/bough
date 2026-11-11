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
    fn view(&self) -> impl Element<Self> + '_;
    fn changed(&mut self, _d: Changed<'_, Self>) {}
}
```

There is no per-view mode switch (`FIXED` was removed). Which owners rerun follows from their state, as the
design's ladder says (`designs/reactivity/final/00-model.md` §3): a `Signal` bound into an operand is a direct
port, so a handler that only publishes signals reruns nothing. A handler that changes plain fields reruns the
owner's `view`, which is then diffed (re-description, branch `api`). Whether an owner *can* hold plain state is
known at compile time from its type, through `type_info` reflection (`api`'s `reflect.rs`): every field is a
ladder cell (`Signal`/`List`/`Entity`/`Reader`), or it is not. That decides whether a handler on it ever reruns
`view`, with no annotation.

### The application surface (`bough::prelude`, `crates/bough/src/ui.rs`)

The design's spellings, laid out by cephal. No element takes a coordinate.

```rust
use bough::prelude::*;

const BAR: Flex = Flex::row().align(Align::Center).gap(12.0); // a const layout recipe

pub struct Counter { pub count: Signal<u32> }

impl Counter {
    pub fn inc(&mut self) { *self.count += 1; }             // a route is a method
}

impl View for Counter {
    fn view(&self) -> impl Element<Self> + '_ {
        div()
            .layout(BAR)
            .p(24.0)
            .child(text(sig!(self.count).map(|n| format!("Count: {n}"))))   // a direct port
            .child(button("+").on_click(Counter::inc))
    }
}

fn main() {
    Application::run(Window::document(Counter { count: Signal::new(0) }).title("Counter"));
}
```

| spelling | what it is |
|---|---|
| `text(x)` | one line in flow |
| `para(x)` | wrapped text |
| `button(label)` | a padded, rounded, hoverable box around a label |
| `div()` | the styled box, born empty: `.child(x)` adds content, `.layout(Flex)` arranges it |
| `column(children)` / `row(children)` | flow containers, born with their children (tuples, iterators) |
| `.gap`, `.p`, `.px`, `.py`, `.pad`, `.w`, `.h`, `.w_pct`, `.grow`, `.shrink`, `.items`, `.justify`, `.center()` | `Styled` |
| `.bg`, `.rounded`, `.border` | paint |
| `.on_click(Owner::method)`, `.hover_bg(c)` | routes and hover |
| `Application::run(view)` or `Application::run(Window::document(view).title(..).size(w, h))` | starts the app; the default face is `BOUGH_FONT`, then `ZGX_FONT_FILE`, then the system's Arial |

The benchmark ports use `bough::raw` (absolutely placed quads and text, templates, measured lists, the `App`
driver), which is the old prelude unchanged.

Still to close against the design:
* `&self.count` / `self.count.map(f)` as ports, without `sig!`. That needs the runtime offset proven against
  the compile-time schema, which `type_info` makes possible.
* The return type `El<'_>` instead of `impl Element<Self> + '_`.
* `.child(x.align(..))` / `.at(handle)` placement.
* `Handoff`, `Target`, `.with(..)` bindings, `#[command]`.

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

## Round 4 (branch `api`): reflection, virtualization as exposure, interface cleanup

**Reflection, typed, no opt-in** (`reflect.rs`). Built on rustc's compile-time type reflection
(`#![feature(type_info)]`, this toolchain): `reflect::schema::<T>()` is a `const` per type, a preorder of
nodes (kind, field / variant name, the compiler's offset, size, child count) with, per node, the vtables of
`Debug`, bough's `Edit` (write from text), a blanket `Meta` (type name; raw-pointer receivers, so the schema
names types it has no value of) and bough's `Items` (a container's children: `Vec`, `Option`, `Box`, `Cell`,
`RefCell`, `Rc`/`Arc` read-only, `List`, `Reader` read-only), each found by `TypeId::trait_info_of` at
compile time. Nothing is derived or registered in any component crate. A struct whose first field is a
`WireHead` is a reactive cell (`Signal`, `List`, `Reader`, `Entities` now all start with one): a `Signal`'s
node is its value, and an edit through reflection queues the cell's head, so it runs as an ordinary write
(wires, damage, effects). Safe API: `reflect::fields_of(&v)`, `reflect::set_in(&mut v, "a.b[3].c", "text")`;
on `App`: `reflect_fields`, `reflect_set` (a turn), `reflect_records` (records with element type, layout
box, style, paint, text), `reflect_style` / `reflect_set_style` (cephal's `Style`, itself reflected),
`reflect_set_color`, `reflect_stubs`. Limits: enums are listed with every variant and field in the schema
but not opened in a value walk (the active variant index is not available; `discriminant_value` is the
declared discriminant), `Option` opens through `Items`; std containers are opaque (shown by `Debug`); a
schema holds at most 256 nodes (`truncated`); `TypeId::info` ICEs on tuple types in this toolchain, so tuples
are recognised by a trait first. A `TypeId` from the schema never leaves `reflect.rs` (no downcasting).

**Virtualization is exposure, not a list option** (`listview.rs`, `rt.rs`). Every list region (`list`,
`list_with`, `index_list`) mounts only the rows meeting what is visible of it: its own viewport when it has
one (`.viewport()` / `.scroll(sig!(..))`), otherwise its clip ∩ the window. No flag turns this on. The
window follows any exposure: the region's scroll; its container moving (layout, or an enclosing space
moving: spaces now nest, `alloc_space_in`, and `set_space` / `shift_space` carry the move to nested spaces
and to the regions placed in them, which re-window); the visible rect changing (window resize, a clip
changing, a sibling revealing it). Rows leaving it are released (two-phase teardown); their state lives in
the list (or the owner, for index rows), so materialize → dematerialize → materialize returns the same
pixels. The unmounted runs are typed placeholders (`Rt::stubs`, `Rt::stub_at`, `App::reflect_stubs`:
region, rows, extent), sized by the region's extent index (`mlist::HeightIndex`, measured heights, the
estimate for rows never mounted; without `.estimate()` the first rows measured set it); cephal sees the
container as a sized leaf of the index's total. Nested regions (a list in a row of a list) count their rows'
records in the enclosing row (`lr_count`), so releasing the outer row releases them.
Representation differs from the lead's "stub record" idea in one place: placeholders are not Bough records
(the presenter dispatches on payload, so a non-empty ink would draw), and exposure is detected as a change
of what a region sees (O(regions) per flush), not by walking page summaries for stub inks. The extent
index is per region, not in the Bough's page summaries. Not unified: the template `virtual_list` (zgui's
long list: absolute rows with no layout) is still its own mechanism.

**Interface cleanup and soundness** (each kept or tightened STATUS §4):
* `Wired::proven` (a public `unsafe` constructor taking an offset and an unrelated accessor) is gone; a field
  `[Signal<T>; N]` is proven by `sig!(self.counts)` and indexed by `.at(i)` (checked), which was the one use.
* Routes (`on_click`, `on_press`, `on_hover`, `on_key`, `on_drag`), `scrolls` and `text_field` are refused at
  mount (a panic, not UB) inside a list row or an entity view: `App` dispatches them with the root owner, so a
  row-typed route would have been called with the wrong owner (found by the widgets and mail ports).
* `Lists::regs` is private (`regs()` reads): a region's index names its monomorphised row functions, so a
  reordered table reachable from `app.rt` would have run them on the wrong source.
* `List` and `Entities` slab chunks are held by raw pointers from `Box::into_raw` (`list::Chunk`): an item's
  address carries the allocation's provenance, so `List::edit` and owner writes never go through a pointer
  derived from a shared borrow of the list.
* A moved text's damage: a text rebuilt after its row or scroll view moved damaged its cached ink (where it
  was laid out) instead of the record's (where it is): the stale glyphs stayed on screen (found by the
  reflection pixel gate, reproduced with direct writes). Old ink is now read from the Bough (`Rt::cur_ink`).
* Unchanged on purpose: `sig!` keeps its two forms (`self.path` in a `View`, `v: Type => path` for a value);
  `prove` stays `#[doc(hidden)] pub unsafe` (the macro's expansion needs a public path); `App::frame` is the
  only frame entry point on this branch; `NumFmt` stays re-exported at the root for `.num(f, fmt)`;
  `Mounter`'s fields stay crate-private (custom elements outside the crate remain a gap).
