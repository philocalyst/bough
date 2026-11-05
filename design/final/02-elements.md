# 02 — Elements

Scope: the borrowed frame arena and `El<'f>`, the `Element` trait/currency split, layout recipes,
the sealed `ChildSeq` contract and the one children rule, the placement algebra, encapsulation
via struct views, methods-as-views, and the reconciliation (identity) law.

Depends on: [00-model.md](00-model.md) L1, L3, L6, L8. Feeds: [03](03-ports.md) (property/child
ports on elements), [04](04-reactions.md) (`view`'s counterpart, `changed`), [06](06-text.md)
(the editor kernel is the same `El<'f>` mechanism at full strength).

---

## 1 · One borrowed `El<'f>`

Every finished builder and every application helper returns the same small, concrete, sealed
type: `El<'f>`. There is no `Box<dyn Element>`; `El<'f>` is a compact **draft cursor** into the
current description frame — a node key plus a short packet range in a per-turn bump arena.
Constructors and setters append sealed opcodes whose operands may **borrow** directly from `self`
and from that arena; nothing is cloned, interned, or refcounted merely to name it.

```rust,ignore
pub trait View {
    fn view(&self) -> El<'_>;
    fn changed(&mut self, delta: Changed<'_, Self>) {}
}

fn toolbar(document: &Document) -> El<'_> { /* borrows document for this frame */ }
fn empty_state() -> El<'static> { /* borrows nothing */ }
```

A function returning `El<'_>` (or `El<'static>`) is itself a root/child view — `View` is needed
only when a named stateful type has earned its existence. `Window::document` accepts a function
item, an owned `Fn(Frame) -> El`, an `Entity<T: View>`, or a plain `T: View`, and invokes it
inside description once per turn against that turn's frame.

### 1.1 Why it borrows, and why that's sound

Description **borrows** `self` and scalars into a per-turn arena — a pointer bump, the same
operation Dear ImGui performs when it outputs vertex buffers each frame — and the commit diff
promotes only the *changed* operand to owned storage at mount. A signal-backed property is never
promoted at all: it installs a direct port that reads the signal's own inline storage. This is
strictly less copying than an owned-draft design and zero on an unchanged subtree.

The borrow is sound **by construction**. The *scope tokens* — `Frame<'f>` and the shared
`Scope<'id, K>` primitive behind it (§8) — are invariant and unforgeable; that is the
`GhostCell`/`generativity` trick. Description runs inside a scope whose `'f` is generative —
universally quantified per `describe` call — and `El<'f>` and every builder are tagged with that
brand.

```rust,ignore
// kernel-internal; application code never spells the brand
pub struct Frame<'f>(PhantomData<fn(&'f ()) -> &'f ()>);

fn describe<R: RootView>(root: &R, f: Frame<'_>) -> Committed {
    let draft = root.view(f);   // El<'f>: may borrow root + f's arena
    commit(draft)                // diff + promote; arena dropped here
}
```

Because `'f` is generative and universally quantified, an `El<'f>` provably cannot be stored in
a `'static` field, sent to another thread, smuggled into the next frame, or returned past
`describe` — the borrow checker rejects it the way it rejects a `GhostCell` token escaping its
branding closure. `El<'f>` is additionally move-only, `!Send`, `!Sync`, and `#[must_use]`.

**`El<'f>` itself is covariant in `'f`** — invariance lives in the tokens, not the currency.
Covariance only permits *shortening* a lifetime, which is what lets `El<'static>` (a helper that
borrows nothing) coerce into any frame arm — `match { Some(s) => s.view(), None =>
empty_state() }` unifies without ceremony. It does not permit smuggling a longer-lived draft
into a shorter frame; that direction still needs the borrow checker to prove `'f1: 'f2`, which
two disjoint frame scopes never satisfy. One caveat: if the kernel ever holds two live frames
with a genuine outlives relation — a candidate/speculative description nested inside a turn —
the candidate arena gets its own brand, so an outer element cannot flow into the inner draft. A
`trybuild`/Miri fixture pins all three cases — escape, cross-frame smuggling, outer-into-draft
(gate 13).

There is one deliberate source of friction: a **stateless free function that builds arena
temporaries but borrows no receiver** has no frame to borrow from — and the resolution is not to
hand it one. Such a helper is simply `fn empty_state() -> El<'static>` (or `-> impl Element`):
`El<'static>` carries a small owned spillover buffer (inline-then-heap) for operands that would
otherwise need the arena, coerced into the arena at the next `.child()`/return splice. This is
rare — borrowing from `self` is the overwhelmingly common shape — and `Frame` never appears in a
public application signature; it is kernel-internal, full stop.

### 1.2 Describe-time compute is a kernel phase, not application mutation

A serious text or graphics surface performs real computation while geometry is known — wrapping,
shaping, hit geometry, display-map maintenance. This is a named, sealed **kernel phase**, not a
loophole in "`view` is pure":

```rust,ignore
editor_surface(&self.document)
    .layout(|layout: Cx<'_, Layout>| EditorLayout::compute(layout, &self.viewport))
    .geometry(|geo: Cx<'_, Geometry>, layout| EditorGeometry::compute(geo, layout))
    .paint(|paint: Cx<'_, Paint>, geometry| EditorPaint::compose(paint, geometry));
```

`Cx<'s, Layout>`, `Cx<'s, Geometry>`, and `Cx<'s, Paint>` are one generic kernel primitive
(`Scope<'id, K>` for kind `K`, §6) with per-phase methods. None can mutate application entities or
signals, dispatch commands, claim focus, or recursively flush; `Paint` can only append
renderer-neutral scene packets. If geometry reveals new work (soft-wrap, for instance), the phase
may only emit an owned, revision-keyed demand into the private display-pipeline draft — the
phase/epoch barrier deduplicates it and arms an already-mounted producer; the result cannot
re-enter the same frame. A phase fault discards the candidate geometry, never the document. This
is the one sealed bridge from geometry to work; application layout cannot turn measurement into
arbitrary effects. See [06-text.md](06-text.md) for the editor kernel built on this phase surface.

## 2 · `Element` the trait, `El<'f>` the currency

`Element` is the sealed trait; `El<'f>` is the concrete sealed currency both builders and `El`
itself implement:

```rust,ignore
pub trait Element<'f>: sealed {}
pub struct El<'f> { .. }                    // implements Element<'f>
```

Builders and every completed component finish as concrete `El<'f>` by default, so every `if`,
`match`, `Option`, and iterator unifies with zero ceremony — arms are the same nominal type. This
is deliberate: Rust gives exactly one zero-annotation way to unify branch arms — the same nominal
type — and an opaque-return default (`impl Element` everywhere) would have paid for that unity
back at every call site.

```rust,ignore
impl View for Tab {
    fn view(&self) -> El<'_> {
        match self {                          // no wrapper: arms are the same nominal type
            Tab::Editor(editor) => editor.view(),
            Tab::Terminal(terminal) => terminal.view(),
        }
    }
}

fn swatch(color: Color) -> impl Element {     // no lifetime, borrows nothing
    div().size_8().bg(color).rounded_md()
}
```

`impl Element` survives as the **lifetime-hiding leaf spelling**: a helper that is never branched
over may return `impl Element` (or `impl Element + '_` when it borrows) instead of naming `El<'_>`
or `El<'static>` — the contract stated in the signature is *"return `El<'_>` to be branched on;
return `impl Element` to hide the plumbing; every sink takes both."* Capability traits —
`Styled`, `Container`, `Interactive`, and the `IntoProp`/`Prop<T>` bounds of
[03-ports.md](03-ports.md) — retreat to **bounds only**: `fn card<E: Styled>(e: E) -> E`, and a
library that wants restyleability across an opaque return may spell `-> impl Element + Styled`.
A mixin is just such a function: `card(column((..)))` and `column((..)).apply(card)` are two
equal spellings of applying `fn card<E: Styled>(e: E) -> E` — `.apply(f)` exists so a chain need
not be unwound to wrap it.
Per-kind method gating on *finished, concrete* elements is retired: `text(x).columns(3)` is a
missing-method error at the call site, not a type error engineered into a recursive typestate.

**Encapsulation is the return type, and struct views are the strong form.** A function component
returning `El<'_>` is *open* — every builder method is still callable on it, which is what a
helper wants. A component that has earned sealed internals earns a struct instead:
`struct ConfirmDialog { .. } impl View for ConfirmDialog` — a struct view embedded as a child
exposes no style surface at all, by ordinary module privacy, the oldest sealing Rust has. This
completes one ladder: a component that earns state gets a struct; a component that earns
encapsulation gets one too.

**The rare forced cross-library join** — two genuinely different `impl Element` opaque types
meeting in one expression — uses `std::convert::Into`, never a bespoke word (00 §9 resolves an
earlier, since-abandoned `el(...)` constructor in favor of this). If your own code hits this, the
fix is normally to change that helper's return type to `El<'_>`, not to convert.

## 3 · Layout recipes without branch poisoning

A typed recipe, passed into a monomorphic element, keeps layout-family methods from
autocompleting on the wrong container without encoding the family into the element's own type:

```rust,ignore
div()
    .layout(Flex::row().align(Align::Center).gap(8).wrap(&self.wrap))
    .bg(theme.surface)
    .child(content)
```

`Flex` owns `row`/`column`/`wrap`/`justify`/`align`/`gap`; `Grid` owns `columns`/`rows`/`areas`
and grid placement — those methods do not exist on the wrong recipe, so completion is useful and
misuse fails at the exact call. `div().layout(...)` still returns `El`. Conditional configuration
is plain Rust (`if *self.wrap { layout.wrap(true) } else { layout }`); switching layout families
entirely is an ordinary branch, because both arms finish as `El`. Layout recipes are `const`-able
(`const TOOLBAR: Flex = Flex::row().align(Align::Center).gap(8);`), which is also what makes
const subtree templates possible (08 §5). `row()`/`column()` remain convenience constructors
returning `El` with the corresponding static `Flex` recipe.

## 4 · Children: `ChildSeq` and the one rule

Every child sink in the surface — `row(...)`, `.child(x)`, boundary content — accepts a sealed,
closed **`ChildSeq`** family, each form owning its own shape delta:

```rust,ignore
pub trait ChildSeq<'f>: sealed {}   // internal: arity class, shape-delta protocol, tape-splice lowering
```

| Form | Arity | Shape delta | Lowering |
|---|---|---|---|
| `El<'f>` / `impl Element` | exactly 1 | none | in-place diff |
| `(A, B, …)` tuple of `ChildSeq`s | fixed | none | per-position diff |
| `Option<impl ChildSeq>` | 0 or 1 | insert/remove | splice at position |
| `impl Iterator<Item: ChildSeq>` | dynamic | ordinal realign | windowed splice |
| `&Vec<T: View>` | dynamic | ordinal realign | windowed splice |
| `&List<T: View>` / `list.each(f)` | dynamic | slot deltas | per-slot splice/edit/move |

> **L8 — a sequence form owns its shape delta** (00 §4). The kernel never re-derives structure by
> comparing rendered children; it applies the delta class the form declares. Ordinal forms
> mount-cycle on reorder (§6 — the price of not keying); slot forms preserve identity across any
> permutation; fixed forms cannot change shape at all, so their positions never cycle. Nesting
> composes deltas positionally: a tuple of `Option`s of iterators is exactly as precise as its
> parts.

Erasure exists in exactly one place: the open-world plugin boundary, crossing as `AnyEntity`
behind a versioned protocol (09 §2). Application code has no `AnyElement`, no box-to-branch — and
this is a sealed-trait fact a `trybuild` gate pins (10 §1, gate A2), not a style rule a reviewer
enforces.

### 4.1 One children rule, no exceptions

**Containers take `impl ChildSeq` in the constructor, always** — a tuple, an `Option`, an
iterator, `&list`/`&vec` of views, `list.each(f)`, or one element — and the `.children(...)`
builder method is deleted:

```rust,ignore
column((
    text(title).text_sm(),
    row((left, right)).gap(8),
))
.gap(12)

row(&self.tiles)                     // T: View — delta-wired implicitly; the common case
row(self.tiles.each(Tile::compact))  // explicit row fn — a non-view or an alternate view method
column(self.log.iter().map(|l| text(l).text_xs()))   // plain data: ordinary iterator
```

`&List<T>`/`&Vec<T>` implement `ChildSeq` directly when `T: View` (the row *is* the view — no
information is added by naming a row function); `list.each(f)` is the explicit projection when
you choose the mapping; a plain iterator is the spelling for plain data. `each(f)` accepts
either `fn(&T) -> El<'_>` or `fn(&T, Slot<T>) -> El<'_>` — a row that binds slot-relative
targets or slot-carrying routes asks for its slot in its signature. Explicitness is opt-in,
exactly where it carries meaning. `virtual_list(&self.rows)` remains a widget, not a children
spelling, because virtualization is a scrolling behavior; it accepts the same forms internally.

**Flow containers are born with their children; the box primitive is born empty.** `column(...)`,
`row(...)`, and `grid(...)` exist to arrange children, so they take them in the constructor.
`div()` is the styled-box primitive — spacers, swatches, panels — and stays nullary; content is
added with `.child(x)`, the one child verb, and `div(())` is never written.

## 5 · Encapsulation and methods-as-views

§2 covers the return-type axis; the other axis is **how a stateful child receives per-frame data
from its parent**. `View::view(&self)` takes nothing; constructor inputs are fixed at
construction. The answer is one law with two halves:

1. **Methods are views.** `fn view_with(&self, results: &[Hit]) -> El<'_>` on a stateful
   component composes exactly like any view function — `View::view` is merely the canonical
   zero-argument case that roots and containers call.
2. **The wire boundary is the scheduling boundary** (L6, 00 §4). A child invoked with *borrowed*
   arguments schedules at its parent's grain — the kernel cannot rerun it independently, because
   the arguments cannot be re-borrowed without the parent. Independent child scheduling requires
   every input to be ladder-shaped: `Signal`, `Reader`, `List`, `Entity`.

This is the honest answer to "why must this field become a signal": not for change *detection*
but for change *delivery without the parent*. Promotion at a component boundary and promotion for
a hot leaf are the same story told at different grains.

## 6 · The reconciliation (identity) law

07's architecture answers the oldest retained-UI question — when a conditional flips or a sibling
is inserted, which retained cells survive — unusually cheaply, and it is a law because an entire
stratum (conditional producer cells, action cells, animation tracks, focus defaults, boundary
cohorts) keys off it:

> **Application state never lives in the tree.** Owner fields, signals, lists, and entities are
> identity-free with respect to description — this is why there are no hooks. Tape identity is
> **parent identity × child ordinal × node kind**. A turn that changes the kind at an ordinal — or
> shifts ordinals by inserting an unkeyed sibling — *mount-cycles* everything at and below that
> position: producer cells are revoked (their in-flight completions die by the stamp discipline
> in [05](05-async.md)), tracks and memos drop, focus falls back by [04 §4](04-reactions.md).
> Keyed survival across reordering is what `List<T>` is for; there is no other keying mechanism,
> by design.

This defines what a branch flip does to a pending action (revoked — which is exactly why
`.queue(Reliable)` exists, 05 §8), whether swapping `match` arms replays enter animations (yes —
motion state is presentation, 03 §4), and makes the cost model honest: the inspector attributes
mount-cycling churn, so an author who interleaves an unkeyed conditional sibling above a hot
subtree sees the price rather than discovering it as an unexplained frame spike.

## 7 · Placement: marks are elements, geometry is a source

A decoration — a peer cursor, a caret label, a minimap overlay — needs nothing `rect(...)` and
`label(...)` don't already do: same builders, same property ports, same routes, same
accessibility story. There is one child verb and a placement algebra, not a second drawing
vocabulary:

| Spelling | Meaning | Web equivalent |
|---|---|---|
| `.child(x)` | in flow, per the container's layout | normal flow |
| `.child(x.align(TopRight))` | out of flow, box space | `position: absolute` + inset |
| `.child(x.at(handle))` / `.above(h)` / `.below(h)` | out of flow, the handle's coordinate space (text, anchor, pointer) | no equivalent |
| `.z(n)` | reorder within the surface's stacking context | `z-index` |

A surface's computed geometry (line boxes, caret rects, block metrics) is itself a **source** —
`impl Source<Value = CodeGeometry>` in the [03](03-ports.md) algebra — resolved as an *inherited*
source, supplied by the nearest enclosing surface at mount, a located mount error outside one:

```rust,ignore
fn peer_cursors(presence: &Signal<PresenceSnapshot>) -> impl Element + '_ {
    (presence, Code::geometry()).project(|(peers, code)| {
        peers.iter().filter_map(|peer| {
            let caret = code.caret(peer.anchor)?;
            Some((
                rect(caret).fill(peer.color.alpha(0.8)),
                label(&peer.name).above(caret).fg(peer.color),
            ))
        })
    })
}
```

The scheduling is the elegant part: the decoration reruns exactly when the presence signal or the
geometry publishes — the same law as every other region in the system. No trait, no phase, no
paint callback. A mark that wants interaction attaches a route like any element; marks without
routes cost nothing at dispatch. Document order remains the default paint order; a surface
establishes a stacking context exactly as CSS does. What remains sealed is only the hot floor:
`draw(|paint: Cx<'_, Paint>| ...)` keeps zero-copy streaming for glyph runs and selections-by-the-
thousand, and the built-in strata (`glyphs`, `selection`, blinking `caret`) are implemented on
that floor and exposed as configured wires (`.selection(&self.selection)`), not reimplemented by
applications. The ladder — placed mark → decoration region → configured built-in → `draw`
closure → full custom surface with its own phase closures (§1.2) — is reachable at every rung
without changing vocabulary.

## 8 · One generativity primitive

Three places in the kernel need an invariant-lifetime brand with a no-escape proof: the frame
arena (`'f`, §1), a display-map snapshot brand (06 §3), and the phase-capability tokens (§1.2).
Rather than three hand-rolled instances with three soundness arguments, the kernel has one:

```rust,ignore
pub struct Scope<'id, K: ScopeKind>(PhantomData<(fn(&'id ()) -> &'id (), K)>);
```

with sealed kinds `FrameK`, `SnapshotK`, and one `PhaseK` per phase — `Frame<'f>` is
`Scope<'f, FrameK>`, a display snapshot carries `Scope<'m, SnapshotK>`, and the phase contexts are
`Cx<'s, Layout>` / `Cx<'s, Geometry>` / `Cx<'s, Paint>`, one generic type with per-phase `impl`
blocks. One invariance proof, one escape-fixture corpus parameterized by kind, one documented
nesting law: a snapshot scope opens inside a phase scope opens inside a frame scope, and a
branded value is dead when its own scope closes — stated once, true everywhere.

## 9 · Authoring rules

Compressed from the sketching passes — review heuristics, not compiler laws:

- **Fields are sources.** A derived field is a smell; derive it in `view` or the source algebra
  (04 §5).
- **Name the strata, not the plumbing.** Domain words (`composer`, `stage`, `caret`) over kernel
  words (`node`, `port`, `memo`) in application code.
- **Enums for closed sets, `typetag`/serde for open ones** (09 §2) — no framework catalog.
- **Prefer a named local over a chain that crosses two concerns.**
- **The return type is the encapsulation contract**: `El<'_>` is open, a struct view is sealed
  (§2).
- **One drawing vocabulary.** Decorations are placed elements (§7); `draw` appears only at the
  sealed floor.
