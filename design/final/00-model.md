# 00 — The Model

Scope: the whole reactive GPUI model in one readable pass — vocabulary, laws, the storage
ladder, the runtime picture, and the standing rejection list. This is the constitution; every
other document in `final/` is a statute that expands one section of this one and must agree
with it. Read this first.

Depends on: nothing (it is the root). Feeds: every other document in `final/`.

Source note: this folder consolidates 19 passes of `designs/reactivity/*.md` into the final
surface. Precedence, highest first, was `19 > 18 > 17 > 16 > 15 > 14 > 13 > 11 > 07 >` supporting
docs `>` historical docs. Where a later pass deleted something an earlier pass introduced, the
deletion is final and the name does not appear anywhere in `final/`. §9 records where genuine
conflicts between sources were resolved and why.

---

## 1 · Vocabulary at a glance

| Name | What it is | Home |
|---|---|---|
| `El<'f>` | the one concrete, sealed, frame-borrowed element currency | [02](02-elements.md) |
| `Element<'f>` (trait) | sealed trait `El` and every builder implement; `impl Element` is its lifetime-hiding leaf spelling | [02](02-elements.md) |
| `Signal<T>` | independently publishable scalar cell; a `Cell` in the port algebra | [01](01-state.md), [03](03-ports.md) |
| `List<T>` | sequence with durable `Slot<T>` identity and splice/edit/move deltas | [01](01-state.md) |
| `Entity<T>` | independently owned/shared state with its own schedule | [01](01-state.md), [09](09-boundaries.md) |
| `Reader<T>` | non-`Deref` read capability crossing a latency or ownership boundary | [05](05-async.md) |
| `Settled<T>` | an always-ready reader (so `Deref`-able); `reader.or(fallback)` folds `Pending`/`Fault` into a default, trading verdict visibility for field access | [05](05-async.md) |
| `Handoff<T>` | linear give/take occurrence source (child→parent intents) | [01](01-state.md), [04](04-reactions.md) |
| `Target<K>` | typed addressable mounted part (focus, test queries, capability visitor) | [02](02-elements.md), [04](04-reactions.md) |
| `Slot<T>` | durable generational item identity inside a `List<T>` | [01](01-state.md) |
| `#[derive(Delta)]` / `Changed<'_, T>` | data derive generating `Option<Change<T>>` fields; the lazy view over a turn's publication log | [01](01-state.md), [04](04-reactions.md) |
| `changed` | the one reaction method on `View` | [04](04-reactions.md) |
| `Source` / `Sink` / `Cell` | the sealed value algebra everything else is defined over | [03](03-ports.md) |
| `SeqSource` / `ChildSeq` | the sequence and children algebras | [02](02-elements.md), [03](03-ports.md) |
| `IntoProp<T>` | the closed plug family every property port accepts | [03](03-ports.md) |
| `#[command("id")]` | a remappable route: a `&'static str` id + link-time registration (ecosystem `inventory`/`linkme`), no framework newtype; in-process the `fn` is still the identity | [04](04-reactions.md) |
| entity kernel | non-generic, **scope-owned** entity core; generational `Entity<T>` capability, no strong/weak axis, cyclic construction | [09](09-boundaries.md) |
| open items | `typetag`/serde in-process, `abi_stable` for dynamic plugins — no framework catalog, closed sets are plain enums | [09](09-boundaries.md) |

## 2 · The whole model in one paragraph

A GPUI program is ordinary Rust with no macro; a hot program is the same Rust plus direct typed
wires; a signal-dense program may opt into one macro (`#[reactive]`), and none of those choices
changes the runtime, the meaning of control flow, or the element API. State lives in a struct:
plain fields, `Signal<T>`/`List<T>`/`Entity<T>` cells, `Handoff<T>` occurrence cells, and
`Target<K>` addresses — nothing else is a field. `View::view(&self) -> El<'_>` is a pure
projection of that state, described into a per-turn borrowed frame arena and diffed against
retained storage; `View::changed(&mut self, Changed<'_, Self>)` is the one place effects run,
invoked with a lazy, typed view over what this turn's publications changed. A component reports
upward the same way it reports to itself: the parent holds it as a plain field and reads its
delta in its own `changed` — recursively, through nested fields, for free. An occurrence that is
a value rather than a resting state — "the user picked X," "the child is done" — is a
`Handoff<T>`: give once, take once, durable, diagnosed when left unconsumed. Everything that
crosses a latency boundary is a `Reader<T>` produced by `map_async`/`try_map_async`/`compute`
over the sealed `Source` algebra, admitted through exact request stamps so a stale completion can
never touch presentation. Every port a value can enter — a property sink, a child sequence slot,
a media surface — is a sealed, closed plug family with one cost per plug form and a declared
`Impact`, so the framework runs exactly the passes a publication actually dirtied. Nothing is
ambient: no `cx`, no global message bus, no hook order, no string-keyed registry outside an
actual plugin or process boundary.

### 2.1 The model on one screen

One small component exercising the whole vocabulary — each field is a different rung of it:
`query` is a **Cell** (independently publishable, bound two-way by a sink), `results` is a
**question** (a latency-bearing derived read, 05 §1), `picked` is an **occurrence** (a
consume-once value, 01 §6), and `input` is an **address** (data-free, 01 §7). Note what is
absent: `view` contains no subscription, callback, or await; `changed` contains no derivation —
everything derivable is a circuit wired in `view` (04 §5), and the one effect is the `Handoff`
take.

```rust,ignore
#[derive(Delta, Serialize, Deserialize)]
struct Search {
    query: Signal<String>,           // a Cell: text_input binds it two-way
    results: Reader<Vec<ResultRow>>, // a question: latency-bearing derived read
    picked: Handoff<ResultId>,       // a consume-once occurrence from a row
    input: Target<TextInput>,        // a data-free address: focus, tests
}

impl Search {
    fn new() -> Self {
        let query = Signal::new(String::new());
        let results = query.map_async(search_index);   // declared once, owner-scoped
        Self { query, results, picked: Handoff::new(), input: Target::new() }
    }
}

impl View for Search {
    fn view(&self) -> El<'_> {
        column((
            text_input(&self.query).target(self.input),
            boundary()
                .loading(results_skeleton)
                .child(self.results.project(|rows| {
                    column(rows.iter().map(|row| result_row(row, &self.picked)))
                })),
        ))
    }

    fn changed(&mut self, d: Changed<'_, Self>) {
        while let Some(id) = d.picked.take() {
            self.open_result(id);        // the only effect; everything else is a circuit
        }
    }
}
```

The full specifications: fields and the ladder in [01](01-state.md); `El<'_>`, `column`, and
`boundary` in [02](02-elements.md); the sink algebra `text_input(&self.query)` plugs into in
[03](03-ports.md); `changed` and the delta in [04](04-reactions.md); `map_async`, `project`,
and `boundary` policy in [05](05-async.md).

## 2.2 · Two authoring modes: immediate and retained (not static and dynamic)

There is no "static tier" and no "dynamic tier." There is **immediate** authoring and **retained**
authoring, and they are the same language — the difference is only whether a value is *tracked*.

- **Immediate (untracked).** The quick, honest floor. A mockup is plain `T` fields, mutated behind
  `&mut self`, and `view` reruns whole. No signals, no `Reader`, no macro, no constraints to
  satisfy — you are writing ordinary Rust that happens to describe a tree. The retained-tape diff
  (02 §1) still turns each rerun into the actual delta of work, so immediate is not "slow mode," it
  is "undeclared dependencies mode": correct, and cheap for anything small.
- **Retained (tracked).** Promote a field one rung (`T → Signal<T> → List<T> → Entity<T>`), bind a
  live value into a port, or add `#[reactive]` and reach through signals with `*` derefs — and that
  value now updates its destinations *without* rerunning its owner. Promotion is local and
  progressive: one field, one attribute, one component at a time, and **the `view` body does not
  change** when you promote (03's ports accept every rung; §3's ladder is promotion-of-grain, not
  a rewrite). You track exactly what has earned tracking and leave the rest immediate.

This is a spectrum you walk *inside one program*, not a mode you pick for it. The reason it is one
language and not two is the zero-tax law (L14) and gate-3 symmetry (L13): adding tracking changes
*when* work runs, never *what it means*. It is also where the design spends its cleverness, on
constant factors rather than a second runtime:

- **`const` is a first-class tier of "static."** Layout recipes are `const` (`const TOOLBAR: Flex
  = Flex::row().gap(8)`, 02 §3); command ids are plain `&'static str` registered at link time by
  `#[command]` (04 §3, ecosystem `inventory`/`linkme` — not a framework type); token
  looks are `const` recipes (03 §5); a const-evaluable subtree can lower to a static tape template
  with a patch table (09 §5, 08 §7). "Compile-time" here is Rust's own `const`, not a bespoke
  compiler — the more of a screen that is const, the less there is to diff at all.
- **Monomorphization is the delivery mechanism, not a cost to hide.** Every rung is a concrete
  type, so a promoted field is a different *type* the same `view` code accepts, and the port it
  plugs into is monomorphic per form (03 §2 law 3: one cost per plug form). Components stay typed
  end to end — even a library's public component is `impl View`/`El<'_>`, not an erased box — so
  Rust's own error messages carry the design (a `Reader` handed to a two-way control is a located
  type error, 03 §1, not a runtime surprise). Erasure exists at exactly one seam, `AnyEntity` at
  the plugin boundary (09 §2), available when you genuinely need open-world dispatch and nowhere
  else.

The floor test (§5) is this framing stated as an acceptance test; the ladder (§3) is its storage
mechanics; every example in `final/examples/` is written to be *read* immediate-first and promoted
where the comment says a real program would.

## 3 · The storage ladder

Promotion changes storage and delivery grain, never view architecture — every rung is accepted
by the same sinks as the rung below it.

| Rung | Type | Grain | Delivered without the parent when |
|---|---|---|---|
| 0 | `T` (plain field) | whole owner | never — a plain-field change reruns the owning `view` |
| 1 | `Signal<T>` | one scalar | wired directly (`&self.count` in a sink) |
| 1 | `Vec<T>` | whole sequence, tape-diffed | never — realigns like any other owner-grained value |
| 2 | `List<T>` | one slot (splice/edit/move) | wired (`&self.rows`, `list.each(f)`) |
| 2 | contained `T: View` | one child | its own `view`/`changed`, scheduled at its parent's grain unless every input is ladder-shaped (02 §5's wire-boundary scheduling law) |
| 3 | `Entity<T>` | independent schedule and lifetime | always — it owns its own turn |

`Reader<T>` is deliberately absent from the ladder: it owns no application value. It is the
uniform read capability any storage or derivation produces once its value must cross an
ownership or latency boundary — see [05-async.md](05-async.md).

## 4 · The laws

Numbered so every other document can cite `L<n>` instead of re-deriving the argument. Full
statements and proofs live at the cited home; this is the index.

> **L1 — The frame borrows; the diff retains.** `El<'f>` borrows from exactly one per-turn bump
> arena, branded by a *generative* `'f` — universally quantified per describe scope, so an
> `El<'f>` provably cannot escape `describe`. `El<'f>` is **covariant** in `'f`: covariance only
> permits shortening toward a frame, never extending past one, which is what lets `El<'static>`
> flow into any frame arm while cross-frame smuggling still requires an outlives relation two
> disjoint scopes can never prove. (The invariant token is the kernel's `Frame`/`Scope`, not
> `El`.) The commit diff promotes only the *changed* operand to owned storage; an unchanged
> operand and every signal-backed port cost nothing. — [02 §1](02-elements.md)

> **L2 — Publication is a hint; the destination compare is the truth.** A `DerefMut` guard
> publishes unconditionally on drop; every retained destination compares the incoming operand
> against its retained one and no-ops on equality. Spurious publication costs one enqueue and one
> compare, never a rerun. — [03 §6](03-ports.md)

> **L3 — Reconciliation is parent × ordinal × kind; `List` is the only keying.** Tape identity is
> parent identity × child ordinal × node kind. A kind change or an unkeyed ordinal shift
> mount-cycles everything at and below that position; `List<T>` is the sole mechanism that
> survives reordering. — [02 §6](02-elements.md)

> **L4 — A bare fn-item route resolves its receiver at mount**, against the mounting
> description's owner chain: nearest enclosing owner whose type matches the route's receiver, no
> match is a located mount error, and reaching anything that is not an ancestor of the mount site
> needs an explicit handle. — [04 §2](04-reactions.md)

> **L5 — A producer belongs to the scope that created it**, and a `Reader<T>` naming a dead
> owner presents `Fault(Revoked)` — never pending-forever, never last-value-forever. —
> [05 §10](05-async.md)

> **L6 — The wire boundary is the scheduling boundary.** A child invoked with borrowed arguments
> schedules at its parent's grain; independent child scheduling requires every input to be
> ladder-shaped. — [02 §5](02-elements.md)

> **L7 — Derived state is a circuit; `changed` synchronizes with the world outside it.** A value
> that is a function of current state belongs in `view`/the source algebra and updates by port
> patch; `changed` is for I/O, focus claims, sounds, entity commands, `Handoff` consumption,
> protocol sends. The self-side is a *compile-time shape*, not a lint: `Changed<'_, Self>` exposes
> only boundary sources (children, entities, readers), so a component cannot read its own scalar's
> delta at all (04 §1.2); the residual cross-component case (a `changed` writing a boundary field a
> child's `view` derives) stays lintable (04 §5). — [04 §1.2, §5](04-reactions.md)

> **L8 — A sequence form owns its shape delta.** The kernel never re-derives structure by
> comparing rendered children; each `ChildSeq` form applies the delta class it declares. —
> [02 §4](02-elements.md), [03 §1.1](03-ports.md)

> **L9 — Every port declares a constant `Impact`; the scheduler runs the logged union.** No user
> API invalidates in either direction — precision is the framework's obligation, measured by
> counters, not claimed by review. — [03 §3](03-ports.md)

> **L10 — State is the protocol.** An occurrence is a state transition; a payload rides the
> transition; a child "reporting" to a parent is the parent observing a field it already owns
> through the recursive delta. `Handoff<T>` is the one primitive for occurrences that are
> consume-once values rather than resting states. — [01 §6](01-state.md), [04 §1](04-reactions.md)

> **L11 — Cold start, restore, and reconnect are one code path: construct from sources.** State
> is journaled structurally through `serde`; restoring the sources re-arms every producer through
> the same admission machinery as any other turn. — [01 §9](01-state.md), [09 §2](09-boundaries.md)

> **L12 — Latency has exactly three shapes.** A repeatable derived read is a *question*
> (`Reader<T>`, latest-input-wins); a finite effect with a declared policy is an *action*
> (`.action(...)`, reliable/idempotent by declaration); an owned protocol whose messages are the
> domain is a *stream* (`Remote<T>`, explicit lanes). No fourth shape. — [05 §8](05-async.md)

> **L13 — Gate-3 symmetry.** Adding or removing `#[reactive]` (or any macro-cut edge) changes
> invocation counters only — never pixels, state, focus, event order, or drop order. This is the
> soundness condition the macro's optimizations must preserve everywhere it appears in this
> folder. The macro may narrow a `changed` invocation only along delta reads it can *prove*; a
> body containing a delta-independent observable effect — an unconditional call that leaves the
> component — is not narrowed: it keeps the coarse any-field-published invocation, and the
> circuit lint (04 §5) flags the pattern. — [04 §6](04-reactions.md)

> **L14 — The zero-tax law.** No path pays for a mechanism it does not use. Every runtime
> mechanism is pay-per-declaration, strictly-replacing an equal-or-greater existing cost, or
> idle-lane-only and preemptible by real input. — [08 §2](08-runtime.md)

## 5 · The floor test

> A small GPUI program is ordinary Rust with no macro. A hot program is the same Rust plus direct
> typed wires. A signal-dense program may opt into one macro. None of those choices changes the
> runtime, the meaning of control flow, or the element API.

Every design decision in this folder is checked against this sentence. A mechanism that requires
the macro to reach its stated performance, or that changes meaning when the macro is added or
removed (L13), has failed regardless of how it benchmarks.

## 6 · The standing rejection list

None of the following returns in response to any future convenience argument, benchmark, or
survey finding. This list is the union of every prior pass's deletions; nothing here is
provisional.

**Reactive/component vocabulary:** `Output<E>`, `.emit`, `.fired()`, `when(...)` children,
`Wired`/`Wires<T>`, `Emitter`/`type Event`, `impl View<Out>` / any generic `Out` parameter on
`View`, catcher parameters, embed functions (`composer(...)`, `typeahead(...)`), constructor
routes on services, `if_(...)`/`match_(...)`/`select_(...)` expression combinators, `el(...)` as
a public erasure wrapper (the rare cross-opaque-type join is `std::convert::Into`, never a
bespoke word — §9), `.children(...)` as a builder method, `Effects<Self>`.

**Identity/state vocabulary:** hooks and positional state (`use_state`, `use_ref`, `remember`),
call-order or hook-slot identity in any costume, string properties/signals, implicit list keys,
location-only identity, `.key()` on an element (identity belongs to the container or the
embedding site, never the element), a hand-rolled `PaneId`-plus-`HashMap` in place of `Slot<T>`.

**Placement vocabulary:** `layers(...)`, `stack(...)`, and `.overlay(align, x)` as stacking
verbs — the placement algebra (`.child(x.align(..))` / `.at(h)` / `.above(h)` / `.z(n)`, 02 §7)
is the one spelling.

**Runtime vocabulary:** ambient context threading — `cx: &mut Context<T>` passed through
ordinary application methods (the scoped phase capability `Cx<'s, K>`, 02 §1.2, is explicitly
not this), manual `notify`, stored subscriptions, lifecycle-owning callbacks,
ambient reactive tracking, a global message enum, a framework-wide `Task`/`Command`/subscriptions
type, an event bus, user-facing widget invalidation, public `Fn() -> T` property fallbacks.

**Boundary vocabulary:** `Any` event payloads, IPC or metaobjects for in-process component
updates, a mandatory DSL/RSX/QML-like language, data URLs as an in-memory image API, a second
runtime tree for selection/accessibility/grids/docking, string capability queries in application
code, `WeakEntity<T>`/`downgrade`/`upgrade` and any strong/weak handle axis (the kernel is
scope-owned, not refcounted — 09 §1), a framework `ItemCatalog`/`ItemSpec`/`catalog.register(...)`
(open item sets are `typetag`/serde in-process and `abi_stable` for dynamic plugins — 09 §2), an
application- or item-visible `ItemInstanceId` (it is framework-minted like `Slot<T>` — 09 §3).

**Command/style vocabulary:** a framework `Command<A>` newtype, `Command::new("id")`, a separate
`const`, and a `.command(...)` builder step — a remappable command is a `&'static str` id plus
ecosystem link-time registration (`inventory`/`linkme`); in-process the `fn` is the identity and a
bare key/click needs no name at all (04 §3). And `colorize` as a verb (inking coverage is `.fg()`,
modulating color is `.tint()`; there is no third — 03 §5).

**Async vocabulary:** `async fn view`, fallible/pending `Deref`, panic-as-suspension, app-global
pending state, `start_transition`, a public `Task` type outside the sealed `.action(...)` local
adapter and `Remote<T>` service handle.

## 7 · How the folder is organized

| Doc | Scope |
|---|---|
| [00-model.md](00-model.md) | this file |
| [01-state.md](01-state.md) | storage ladder in full, field law, `#[derive(Delta)]`, journaling, history |
| [02-elements.md](02-elements.md) | `El<'f>`, builders, `ChildSeq`, placement, encapsulation, reconciliation |
| [03-ports.md](03-ports.md) | `Source`/`Sink`/`Cell`, `IntoProp`, `Impact`, motion, style/theme tokens |
| [04-reactions.md](04-reactions.md) | `changed`, handlers/routes, commands, focus, gate-3, TEA posture |
| [05-async.md](05-async.md) | `Reader` graph, wake law, trichotomy, cohort state machine |
| [06-text.md](06-text.md) | text stack, `text_input`, IME, editor kernel |
| [07-platform.md](07-platform.md) | windows, renderer/color, clipboard/menus/IME, media |
| [08-runtime.md](08-runtime.md) | turns, scheduler, data structures, memory, fast path |
| [09-boundaries.md](09-boundaries.md) | entity kernel, plugins, durable identity, presence, IPC |
| [10-quality.md](10-quality.md) | gates, testing, inspector, diagnostics, performance proof |
| [11-roadmap.md](11-roadmap.md) | build order, exit gates, phase-by-phase CI |

## 8 · Reading order for a team building toward this

New engineers: 00 → 01 → 02 → 04 (skip 03/05 on first pass) → 10 §2 (the testing surface) →
11. Runtime engineers: 00 → 08 → 03 → 05 → 10. Platform/text engineers: 00 → 06 → 07 → 03 §4.
Nobody needs to read 09 to ship an ordinary screen; it is the workspace/plugin/collaboration
layer.

## 9 · Resolved conflicts

Sources genuinely disagreed in a few places where precedence alone did not settle the question
(a later pass revised only part of an earlier one, or a lower-precedence supporting document
carried an alternate design the higher-precedence passes never explicitly overrode). Recorded
here so the choice is visible rather than silently picked.

1. **`el(...)` as a join constructor.** `13-composition.md` deleted the original `el(...)`
   erasure wrapper; `14-el-and-wires.md §4.2` then reintroduced a *narrower* `el(x)` as the
   zero-cost join for two different opaque `impl Element + Caps` return types, alongside its own
   `§1.2` answer that the same case should use `std::convert::Into`. This folder keeps only the
   `.into()` answer: sinks are generic (`impl Children`/`impl Element`), so passing a component
   anywhere seals it implicitly, and application views default to `El<'_>`; the one residual case
   — joining two distinct opaque types in one expression — is ordinary Rust `.into()`, never a
   named function. No `el(...)` spelling of any kind appears in this folder.
2. **`Entity<T>: Deref`.** `07-the-ladder.md §13` left a public `Deref` conditional on an
   unproven soundness argument. `11-consolidations.md` finding 1 recommends dropping the
   ambition now rather than carrying a maybe-unsound convenience to stabilization. This folder
   adopts 11's recommendation: `Entity<T>` has no public `Deref`; imperative code uses
   `entity.inspect(|state| ...)`, and `view`/`changed` observe entities exclusively through the
   recursive delta (01 §5, 04 §1).
3. **Async vocabulary in `05-one-register.md`, `migration.md`, `pr-plan.md`.** These documents
   predate passes 15–19 and spell child→parent flow as `Output<E>`/`.emit`, reactions as
   `on_change`/`.on(...)`/`Emitter`, and async work as a public `Task` value with
   `self.spawn`/type-routed handler dispatch. All three are superseded outright by 18/19 on the
   component-I/O and reaction axes, and by `07 §7`'s explicit "no public `Task`" rule on the async
   axis. This folder mines them only for content that does not depend on those deleted spellings
   (data-structure sizes, gate methodology, `List`/`Slot` semantics, the undo bright line) and
   respells every surviving example to `changed`/`Handoff`/`Reader`/`.action(...)`.
4. **`List<T>: Deref<Target = [T]>` vs. sum-tree promotion** (`runtime/datastructures.md` Flag
   A). Kept resolved as that document's own recommendation: item storage stays contiguous at
   every size (a chunked/gap-buffered store internally); "promotion" changes the *geometry/order
   index* layered over that store, never the item container. `List<T>: Deref<[T]>` is honest at
   every scale.
5. **Hot-map hashing** (Flag B). `fastpath.md`/`datastructures.md` measured `std::HashMap`'s
   default SipHash re-hashing an already-computed content hash as a dominant cost on the tape
   intern table and the handler-routing `TypeId` map. Resolved as measured: every hot map keyed
   by a precomputed hash, an interned slot, or a `TypeId` uses an identity or Fx-class hasher,
   enforced as a lint (10 §5).
6. **`Effects<Self>` for focus claims** (`07 §11.2` vs. `11 §12`). 11's queued-verb form
   (`target.claim_focus()`) is adopted; `Effects<Self>` never appears — it was a one-off parameter
   type for a single feature that the ladder's existing queued-verb law (`entity.send`,
   `win.activate`) already covered.
7. **`05-one-register.md`'s type-routed handler dispatch and `Self::scope` closures.** These are
   an alternate authoring surface for the same mechanisms `07-the-ladder.md §6` already
   specifies (bare fn-item routes with `.with(...)` binding, methods-as-views). `07` is
   higher-precedence and normative; this folder does not adopt receiver-type routing to a wired
   cell or `Self::scope` as public spellings. The underlying representational insights (a signal
   cell wakes exactly its readers, a component and a signal differ only in size) are kept as
   rationale in [01](01-state.md) and [08](08-runtime.md), not as surface syntax.
8. **The domain-keyed list.** `runtime/collections.md` supplies `List<T, K>` via
   `.keyed_by(T::id)` (a defaulted second type parameter, invisible unless keyed);
   `11-consolidations.md §11` forbade a second type parameter and routed durable keys through
   domain data plus aliasing, while explicitly allowing "a deliberate storage-ladder extension"
   if a design earned it. Resolution adopted: keep `.keyed_by` as that deliberate extension —
   the parameter types the durable-key *index* only; the key value lives in `T`; `Slot<T>`
   remains the only runtime identity, and the key never aliases it or reintroduces position.
9. **`pipe` vs. `apply`.** Pass 13's decision — mixins are plain functions, and the in-chain
   spelling of applying one is `.apply(f)`, not a `pipe` combinator — survives in 02 §2:
   `card(column((..)))` and `column((..)).apply(card)` are two equal spellings of the same
   function application.
