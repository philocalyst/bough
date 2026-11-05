# 04 — Reactions

Scope: `changed` and the `Changed<'_, Self>` delta it receives, the circuit/effect lintable law,
handlers and route resolution, the handler-outcome table, remappable product commands, focus
claims, gate-3 scheduling symmetry, and the TEA posture.

Depends on: [00-model.md](00-model.md) L4, L7, L10, L13; [01-state.md](01-state.md) §6, §8
(`Handoff`, `Changed`). Feeds: [05](05-async.md) (async completions arrive through the same
`changed`/delta surface), [09](09-boundaries.md) (entity commands), [10](10-quality.md) (gates 1–6
below).

---

## 1 · State is the protocol

A component's API is its state machine, and its whole reactive surface is one trait:

```rust,ignore
pub trait View {
    fn view(&self) -> El<'_>;
    fn changed(&mut self, delta: Changed<'_, Self>) {}   // provided; inert by default
}
```

There is no `Output<E>`, no generic `Out` parameter on `View`, no catcher, no embed function, no
`.emit`, no `.fired()`. A component's "output" was always a lifecycle — say so in the type, and
let the parent hold the child as a plain field:

```rust,ignore
#[derive(Clone, PartialEq, Serialize, Deserialize)]
enum Stage { Editing, Discarded }            // resting states only

#[derive(Delta, Serialize, Deserialize)]
pub struct Composer {
    to: Signal<String>,
    subject: Signal<String>,
    body: Signal<String>,
    stage: Signal<Stage>,
    sent: Handoff<OutgoingMail>,             // the consume-once payload (01 §6)
    pub first_field: Target<TextInput>,
}

impl Composer {
    fn send(&mut self) {
        if self.to.is_empty() { self.first_field.claim_focus(); return; }
        self.sent.give(self.outgoing());
    }
    fn discard(&mut self) { *self.stage = Stage::Discarded; }
}
```

```rust,ignore
fn changed(&mut self, d: Changed<'_, Inbox>) {
    if let Some(mail) = d.composer.sent.take() {
        self.deliver(mail);                             // taking IS the acknowledgement
    }
    if let Some(c) = &d.composer.stage {
        if let Stage::Discarded = c.new {
            *self.composing = false;
            self.search_box.claim_focus();
        }
    }
}
```

The split is the rule: **a resting state the parent answers by changing its own state — here,
unmounting the composer — is an enum variant observed as a `Change`; a payload that must be
consumed exactly once is a `Handoff`, and taking it is the acknowledgement.**

No `Msg`, no wrapping, no forwarding chain: a *grandparent* that cares reads
`d.workspace.inbox.composer.stage` directly — recursive deltas compose through nesting
automatically, which conventional message-passing plumbing never did. Sibling communication is
the parent taking a value and republishing it into its own state — one explicit hop, visible in
`changed`.

The component's output type is the generated `Delta` — declared never, derived always
(01 §8). Inputs are parameters and fields; outputs are the diff of the fields. The whole DAG is
spelled by struct definitions and one derive.

### 1.1 Scorecard

| | Observer/subscription | Message-passing (Elm-shaped) | This model |
|---|---|---|---|
| declare an observable | trait + registration | variant + update arm + wiring | a field |
| deliver a payload | callback invocation | dispatch | state transition (journaled, stamped) |
| deep consumer | re-subscribe per level | wrap/unwrap per level | recursive delta path, free |
| unhandled occurrence | silent | `_ => {}` you wrote | located diagnostic (untaken `Handoff`) |
| crash mid-delivery | lost | lost | restored from the journal (01 §9) |
| coverage checking | none | exhaustive match on `Msg` | exhaustive match on the transition enum |
| granularity control | per-subscription | per-message | macro-narrowed delta reads (§6) |

### 1.2 The self-delta is boundary-only, structurally — not by lint

A recurring mistake is to imagine `changed` as a place to watch your *own* scalar fields — to
mutate `self.count` in a click handler and then "react" to `d.count`. This model does not merely
discourage that; **it makes it unspellable**, the way it makes every other footgun a type error
rather than a review note.

The `#[derive(Delta)]` macro classifies each field by its type:

- **Boundary sources** — a contained `T: View`, an `Entity<T>`, a `Reader<T>`, a `List<T: View>`,
  and any nested `Delta` reached through one. These transition on a schedule *you do not clock*: a
  child gives a `Handoff`, an entity commits, an async request lands. Their timing is not yours,
  which is the only reason a diff exists.
- **Local state** — your own `Signal<T>`, plain `T`/`Vec<T>`, a `Handoff` you *give*, a `Target<K>`.
  These change only because `&mut self` in *your own* method wrote them, and that method already
  had the value in hand.

`Changed<'_, Self>` — the argument to `Self::changed` — exposes **only the boundary sources** at its
top level. Your own scalars are simply not fields on it:

```rust,ignore
fn changed(&mut self, d: Changed<'_, Self>) {
    let _ = d.count;        // ✗ compile error: no field `count` on `Changed<Self>` — a
                            //   `Signal<u32>` you own is local state; handle its edit in the
                            //   handler that writes it (`on_click`/`on_change`), not here.
    let _ = d.child.count;  // ✓ across the boundary, `child`'s FULL delta is available, including
                            //   its own signals — because now the timing is not yours.
}
```

The asymmetry is the whole point: `count` is local *to you* and boundary *to your parent*. Your
parent reads `d.you.count` through the `you` boundary and gets it; you cannot read `d.count`,
because there is no diff to consult about a value you just wrote. The generated full `T::Delta`
still carries every field (that is what the parent projects through); it is only the *self-view*,
`Changed<'_, Self>`, that is narrowed to boundaries. So "derive, don't react" (L7) stops being a
lint you can violate and becomes a shape the compiler enforces on the input side. (The §5 lint
survives for the one case types cannot catch: a `changed` that writes a *boundary* field which a
child's `view` then derives — a cross-component circuit, not a self-read.)

Two corollaries fall out:

- **The derive is pulled by the observer, never pushed.** A leaf that observes nothing across a
  boundary derives nothing. A type needs `Delta` only when some *other* component reads it across a
  boundary — `d.child.field` — and a missing derive is a located error at that read naming the fix
  (01 §8): *"observing `Composer` here requires `#[derive(Delta)]` on `Composer`, or read
  `d.composer.any()` for a coarse changed-marker."* The cost lands on the edge that earns it.
- **Every existing example already obeys this.** `inbox.rs`, `reach.rs`, `derived_observe.rs`,
  and `reactions.rs` read only `d.store.*`, `d.project.*`, `d.doc.*`, `d.composer.*`, `d.drafts.*`
  in their `changed` — entities and children, never a bare own scalar. The rule was already the
  practice; making it structural just deletes the way to violate it.

## 2 · The semantics of `changed`

- **Invocation.** `changed` is a handler: the kernel invokes it post-publish with the turn's
  composed delta, at most once per turn, FIFO with other queued deliveries, never reentrantly. An
  effect only ever runs from a handler — this is the handler.
- **The delta is the turn's publication log, typed.** Assembled `O(publications)`; recursion
  follows fields (01 §8) — `d.store.unread` reaches through an `Entity<MailStore>` field into
  `MailStore`'s own delta exactly as it would for a contained struct. A field whose type has no
  `#[derive(Delta)]` degrades gracefully to a changed/unchanged marker with `Deref` to the new
  value.
- **Writes from `changed` are ordinary turn writes**: they publish, schedule the next round, and
  a `changed` that re-dirties what it reads is a loop the inspector attributes within two turns
  — the loop-attribution gate (10 §1).
- **Scheduling is gate-3 symmetric** (§6): with no macro, `changed` is invoked whenever *any*
  field-source of this component publishes — coarse, cheap (a no-op pass over `None`s), and
  semantically exact. With `#[reactive]`, the statically visible delta accesses are cut into
  exact invocation edges. Removing the attribute changes scheduling counters only.

> A bare fn-item **route** resolves its receiver at mount, against the mounting description's
> owner chain: the nearest enclosing owner whose type matches the route's receiver (L4, 00 §4).
> No match is a located mount error; nested same-type owners resolve to the nearest; reaching
> anything that is not an ancestor of the mount site requires an explicit handle
> (`entity.send(Workspace::method)`), never search.

> **The frame path never waits for `changed`.** Presentation-rate data — property ports, motion
> lanes, `List` slot deltas, text pipelines — patches its destinations directly (03, 08 §6);
> `changed` is an application-effect surface and nothing on the frame path blocks on it. Grain
> is component grain, controlled by decomposition: a component with one frantic field and one
> calm reaction splits in two, or promotes the frantic consumers to direct ports — no second
> granularity system exists. Emission routing is one queued monomorphic method call, no
> subscription table.

## 3 · Handlers are methods with return values

The route shape is uniform across the whole surface:

```text
fn(&mut Receiver, [site context], [event], [bound inputs]) -> R
```

The receiver selects the owner (resolved by §2's law). Site context supplies values delivery has
already validated (`Slot<T>`, a drag payload). `.with(...)` binds trailing owned inputs. `R` is
interpreted by the one normative table below — nothing else in the surface assigns meaning to a
return type.

| Declared return | Meaning at delivery |
|---|---|
| `()` | handled; input events consume |
| the event type itself | explicit `.consume()`/`.propagate()` decision |
| `Option<E>` | `Some` fires the registration's declared secondary route (e.g. an `.action(...)` adapter's `.then`); `None` is silence |
| `Result<T, E>` | routed intact unless the site split it with `.then`/`.catch` |
| any other `R` | only legal where the registration's route type names it |

**Guard the function, never the chord.** There is no `IntoGuard`, predicate registry, or string
key context — the handler owns state-dependent dispatch with ordinary control flow:

```rust,ignore
editor_surface().on_key(Key::Escape, Workspace::escape);

fn escape(&mut self, event: KeyEvent) -> KeyEvent {
    if !self.in_insert_mode() { return event.propagate(); }
    self.to_normal();
    event.consume()
}
```

### 3.1 The method path is the route; a non-capturing closure is a legal but uglier fallback

The idiom is the fn-item path — `Workspace::escape` — and it is the idiom because it reads well:
the operation is named, the receiver is inferred, and there is nothing to annotate. Prefer it
always. A route is, at bottom, anything that satisfies the target's fn signature
(`fn(&mut R, [ctx], [event], [bound]) -> Out`), and Rust gives that bound a second inhabitant — a
non-capturing closure, which coerces to a bare `fn` pointer — but it is deliberately the worse
spelling and the design does not pretend otherwise:

```rust,ignore
editor_surface()
    .on_key(Key::Escape, Workspace::escape)             // ← the idiom: named, no annotation, clean
    .on_key(Key::Enter,  |ws: &mut Workspace, e| { .. });  // legal, but note the ugly `&mut Workspace`
```

The closure's ugliness is not incidental and it is not a gap to close: coercion to `fn` needs the
receiver type spelled (`ws: &mut Workspace`), because routes resolve their receiver at *mount*
(L4), not at the builder call — so the type cannot be inferred from context the way a method path
supplies it for free. That friction is a feature: it makes "just name the method" the path of least
resistance for anything beyond a one-liner, which is where a handler belongs anyway. The closure
exists only so a genuinely throwaway one-liner need not be lifted to a named method; reach for it
rarely.

Both spellings are the *same* thing to the kernel — a zero-capture route slot (08 §runtime), no
allocation, `&mut Self` at dispatch — and the one hard rule is that a closure may **capture
nothing**. The moment it reaches for an environment value it stops coercing to `fn`, and the
located error is the fix-it:

```rust,ignore
.on_key(Key::Tab, |ws| ws.indent(width))   // ✗ captures `width`
//                                             error: this closure captures its environment, so it
//                                             is not a route. Name a method and bind the value:
//                                             .on_key(Key::Tab, Workspace::indent.with(width))
```

This is why the field law (01 §1) can forbid stored closures without costing ergonomics: a handler
is never *state* (it is `&mut self` plus event plus owned `.with(...)` bindings, resolved at
dispatch), so the only closures that reach a route carry no state to store. `.with(...)` is the
capture list made explicit, owned, and serializable — the environment a capturing closure would
have smuggled, spelled where it can be journaled and named in a keymap.

### 3.2 The function *is* the identity — until it has to leave the type system

The right instinct is: the function already is the identity. `Pane::split` is a distinct
zero-sized type with a unique path; `on_key(Key::Backspace, Pane::split.with(Axis::Horizontal))`
identifies the operation completely, and **a key, a click, a menu built in Rust, or a call from
another component never needs anything more.** There is no `Command`, no registration, no name. This
is the overwhelmingly common case and it stays bare.

The one thing a `fn` cannot do is *survive serialization*. A keymap in a config file, an extension
manifest, or a remote peer's message names an operation **as data** — a string in JSON — and Rust
gives a function item no stable runtime representation and no reverse lookup: there is no
reflection that turns `"pane.split"` back into `Pane::split`, and `type_name` is explicitly not
stable across builds. So the boundary needs exactly one thing the language does not provide on its
own: a `&'static str → typed-dispatch-thunk` table, populated at startup. That is the whole
mechanism — and it is not a bespoke framework type, it is **the ecosystem's ordinary
link-time-registration pattern** (`inventory` / `linkme` distributed slices), the same tool crates
already use to register plugins, test cases, and CLI subcommands:

```rust,ignore
impl Pane {
    #[command("pane.split")]                 // sugar: submits one (id, thunk) to a distributed slice
    fn split(&mut self, axis: Axis) { .. }
}

// desugars to an ordinary registration you could write by hand:
inventory::submit!(CommandEntry {
    id: "pane.split",
    // thunk: deserialize the payload as the fn's own parameter type, then dispatch to the route
    dispatch: |bytes, target| Pane::split.dispatch(target, from_json::<Axis>(bytes)?),
});
```

So there is no `Command<A>` newtype, no `Command::new`, and no `.command(...)` builder step. The id
is a plain `&str`; the payload's type safety is the *fn's own signature*, checked where the thunk
deserializes the bytes into the parameter type; the registry is a crate you already depend on for
everything else that must be enumerated across a binary. `#[command("pane.split")]` earns its keep
only as boilerplate removal over that `inventory::submit!` — delete the attribute and the method is
an ordinary un-named route again, with no other edit. In-process, the function is still the
identity; the string exists only where identity must outlive the type system, and nothing new was
invented to carry it.

## 4 · Focus is a committed claim

A render-time `reclaim_focus`/`focus_pending` flag is not harmless view scratch — mutating it and
calling focus during description makes tree shape and rerun count affect behavior. Focus claiming
is the same vocabulary as every other typed-handle verb (`entity.send`, `win.activate`): a
**queued verb on the target**, applied after the target's generation commits.

```rust,ignore
fn closed_palette(&mut self) {
    self.editor_target.claim_focus();   // queued; applied after the target's generation commits
}

fn view(&self) -> El<'_> {
    editor().target(self.editor_target).focus_default()
}
```

`view` only binds a `Target<K>` and may declare a default for a newly committed scope; it has no
focus capability, and neither does any phase closure (02 §1.2). A claim on a temporarily unbound
target parks for its declared generation/deadline, then returns a typed miss or follows an
explicit fallback. Claims are ordered and coalesced by scope, so duplicate description cannot
focus twice and an old claim cannot steal focus after navigation.

`Target<K>` is also the one typed reach a test or the accessibility bridge has into a mounted
part: a **capability visitor** (walk committed nodes; query semantic role/text/bounds; drive the
a11y action surface) is the same vocabulary `Win<V>`'s semantic test queries use (10 §2). There is
no string query API in application code, and none is needed at the test tier either.

## 5 · The circuit/effect law, lintable

> **L7 — a value that is a function of current state is a circuit; `changed` synchronizes with
> the world outside it** (00 §4). A circuit — a prop plug, a `map`, a `project` region, a derived
> reader — lives in `view` or the source algebra and updates by port patch, never occupying a
> turn. `changed` is for I/O, focus claims, sounds, entity commands, `Handoff` consumption,
> protocol sends. A `changed` that only computes state from state is a circuit written as an
> effect: one extra turn of latency, one glitch window, and one lint.

The discipline is *"derive, don't react"*: if a change should alter what is shown, that belongs
in `view` — a branch over state, a property port, a `project` region — because `view` is already
a function of state and the diff does the rest. Writing state in `changed` that `view` could have
derived is the smell.

Two enforcement tiers make this more than a style note:

- **Static tier.** `#[reactive]` (and a shipped lint for the no-macro path) flags a `changed`
  whose body, along some path, does nothing but assign fields from pure functions of the delta —
  no calls that leave the component, no `Handoff::take`, no capability use. The diagnostic names
  the port to use instead: *"`self.badge` is derivable — bind `self.store.unread.map(badge_level)`
  at the sink instead."* Warn-by-default, suppressible with one attribute.
- **Dynamic tier.** The inspector detects the runtime signature — a `changed`-initiated write
  that triggers re-description and no external effect, converging the following turn — including
  across components (a parent's `changed` writes what a child's `view` derives), and reports the
  same finding with the actual runtime cycle. An infinite version is the loop report of §2; a
  converging version is the derivable-state report.

This also settles what macro-narrowed scheduling *means*: narrowing invocation edges is legal
precisely because `changed` is an effect surface — a circuit may be arbitrarily reordered or
fused, an effect may only be *skipped when its inputs did not change*.

## 6 · Gate-3 symmetry

> **L13 — adding or removing `#[reactive]` (or any macro-cut edge) changes invocation counters
> only** (00 §4) — never pixels, state, focus, event order, or drop order. This is the soundness
> condition every macro-cutting optimization in this folder must preserve, and it is the reason
> `#[reactive]` can compress `changed` bodies (§2), `view` bodies, and `project` regions with one
> uniform guarantee: the no-macro spelling is not a slower dialect, it is the same semantics at
> coarser invocation granularity.
>
> The soundness obligation: the macro may narrow a `changed` invocation only along delta reads
> it can *prove*; a body containing a delta-independent observable effect — an unconditional
> call that leaves the component — is not narrowed: it keeps the coarse any-field-published
> invocation, and the circuit lint (§5) flags the pattern.

## 7 · TEA is a userland pattern, not a framework posture

The foundation is deliberately unopinionated about architecture the way a plain-Rust API is: it
ships mechanism — fields, delta, two methods, routes-as-fn-items — and no reducer, no message
enum, no `update` dispatch. But the mechanisms compose into a message-passing style in userland
with nothing added:

```rust,ignore
enum Msg { Compose, Send, Discard, StoreChanged(MailStoreDelta) }

impl View for Inbox {
    fn view(&self) -> El<'_> {
        column((button("New").on_click(Inbox::update.with(Msg::Compose)), /* .. */))
    }
    fn changed(&mut self, d: Changed<'_, Self>) {
        if let Some(delta) = &d.store { self.update(Msg::StoreChanged(delta.clone())); }
    }
}

impl Inbox {
    fn update(&mut self, msg: Msg) { match msg { /* .. */ } }   // one reducer, by choice
}
```

One funnel method, one enum, an exhaustive match — built from routes and `changed` without a
framework blessing. The default style (a method per concern, as in §1) is equally first-class;
the foundation refuses to pick, because there is no framework-owned reaction vocabulary left for
an architecture to conflict with.

## 8 · What is deleted, restated

`Output<E>`, `.emit`, `.fired()`, `impl View<Out>`/any generic `Out` on `View`, catcher
parameters, embed functions, `Wired`/`Wires<T>`, constructor-line `on_change`/`on`, `Emitter`,
`when(...)` children, and `Effects<Self>` are gone with zero exceptions: nothing in the system
registers, subscribes, catches, or emits. Entity "events" several observers care about are state
transitions observed through the recursive delta (01 §5); something genuinely multi-cast is a
protocol, owned by `Remote<T>`/services (05 §8) — it was never a UI observation.
