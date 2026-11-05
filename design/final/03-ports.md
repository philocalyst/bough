# 03 — Ports

Scope: the sealed `Source`/`Sink`/`Cell`/`SeqSource` value algebra everything else is defined
over, the `IntoProp<T>` closed plug family and its three laws, the `Impact` taxonomy that gives
the scheduler exact invalidation, motion values, and the style/theme token cascade.

Depends on: [00-model.md](00-model.md) L2, L8, L9. Feeds: [01](01-state.md) (`Signal`/`List`/
`Handoff` are algebra instances), [02](02-elements.md) (property and child sinks), [05](05-async.md)
(`Reader<T>` is a `Source`), [07](07-platform.md) (media as a plug family, color/HDR detail).

---

## 1 · The value algebra

07 counted twelve surfaces — property sinks, `project`, `map`/`map_async`, entity observation,
`animate`, `is_pending`, `latest`, `reload_on`, `.with(...)`, boundary children, window
properties, theme tokens — each independently redefining "a place a value comes from," each with
its own prose about readiness, equality, and staleness. The fix is one sealed, kernel-owned
algebra with a direction axis, ratified as normative:

```rust,ignore
pub trait Source: sealed {          // "can be read reactively"
    type Value;
    type Ready<'a>: Copy;           // &'a T for scalars; slot views for sequences
    // internal: raw key(s), verdict (Ready/Pending/Fault), stamp, equality hook, impact
}
pub trait Sink: sealed {            // "can be written through the turn machinery"
    type Value;
}
pub trait Cell: Source + Sink {}    // both — Signal<T> is the canonical instance
```

- **Instances.** `Signal<T>` is a `Cell`. `Reader<T>`, derived projections, `is_pending(_)`,
  `latest(_)`, committed window properties (`win.active`), theme tokens, and `Entity<T>`
  (any-change, `Value = ()`) are `Source`-only. `Handoff<T>` is a `Source` whose ready view is
  linear (01 §6). Constants are the degenerate always-ready source. Tuples implement `Source` by
  product — readiness is the meet of the parts, the stamp is the tuple of stamps — so the mixed
  signal/reader tuple is *defined*, not special-cased. A named struct of sources implements one
  sealed `SourceSet` trait by hand: five explicit lines, no derive, no arity cliff.
- **Every verb is defined once, over the algebra.** `project`, `map`/`map_async`, prop sinks,
  `animate`, `reload_on`, `.with` sampling, boundary children, delta observation — one definition
  each, over `impl Source`. There is no per-verb "what does this accept" prose; a conformance
  matrix (10 §1, gate A1) is the reference instead.
- **Direction is checked where the widget is typed.** A two-way control demands `impl Cell`:

  ```rust,ignore
  fn text_input(text: impl Cell<Value = String>) -> El<'_>;       // wants a Cell — read AND write
  fn label<'a>(text: impl IntoProp<'a, SharedString>) -> El<'a>;  // wants any Source

  text_input(&self.query)          // ✓ Signal is a Cell
  text_input(&self.derived_name)   // ✗ Reader<String> is not a Sink — located error:
                                    //   "a two-way port needs a place to write; bind a
                                    //    Signal<String>, or use label(...) to display"
  ```

  You cannot accidentally hand a read-only derivation to an editing control, and you never
  annotate direction — the port's bound states it. (`&Signal<String>` is today's one public
  `Cell`; the `impl Cell` bound is what carries the `#[diagnostic::on_unimplemented]` text the
  error above quotes.)

### 1.1 `SeqSource` — the sequence half

`SeqSource` is the collection counterpart, with deltas instead of values as its change protocol.
`List<T>` is the canonical instance: slot-keyed, splice/edit/move deltas, with `filtered`/
`sorted`/grouped arrangements as slot-preserving read projections. `&Vec<T>` is the degenerate
realign instance — same verb, coarser delta. Promotion (`Vec → List`) changes the delta grain and
nothing else. `ChildSeq` (02 §4) is the element-tree lowering of this same algebra.

### 1.2 What the algebra dissolves

Three questions that used to need per-feature design collapse into type facts:

- `is_pending`/`latest` become total functions on `impl Source` — an adapter returning another
  `Source` (`is_pending: Source → Source<Value = bool>`; `latest` returns the same source under a
  different cohort policy, 05 §5, §9).
- Composition questions answer themselves: `animate(Fg::Muted)` type-checks iff tokens are
  sources (§4); a boundary `reload_on`-ing a derived reader type-checks because a derived reader
  is a source.
- `if_(cond, a, b)` — a conditional *expression combinator*, the first word of a forbidden third
  language — is unnecessary and deleted: `is_pending(&self.results)` is already
  `Source<Value = bool>`, and `map` is closed over sources: `.opacity(is_pending(&self.results)
  .map(|p| if p { 0.65 } else { 1.0 }))`. The same argument pre-deletes any future
  `select_`/`match_`/`bool::then_`-style feed helper — the answer is always `map` over a source.

## 2 · `IntoProp<T>`: the sealed plug family

Every property port accepts exactly this closed family — nothing else, and never a bare
`Fn() -> T`:

```text
IntoProp<T> ⊇  T | &T                    constant; promoted only when the diff finds it changed
             | &Signal<T>                direct two-way-capable port; publication patches the slab
             | Reader<T>                 latency-aware port; verdicts route to boundaries
             | impl Source<Value = T>    any algebra composition (map, is_pending, tuples…)
             | Motion<T>                 sampled on a timeline lane (§4)
             | semantic token            cascade-resolved feed (§5)
             | Option<any of the above>  only where the port declares one meaning for absence
```

Three laws ride the family:

1. **Closures never enter a port.** The only closure-accepting positions in the surface are
   `map`/`map_async`/`project`/phase closures, where the dependency set is the explicit left-hand
   source and the phase is named. `source.map(f)` is the spelling for "computed value," with
   knowable deps, an equality gate, and a stated cost — there is no bare `.opacity(|| …)`.
   Style-variant builders (`.hover(|s| ...)`, `.active(...)`, §5) are not value plugs: their
   closure is describe-time *configuration*, resolved at publish into the precomputed variant
   table (08 §6), never stored or run per event — the law bans closures as *values*, not
   declarative configuration.
2. **`Option` is admitted only where absence has exactly one meaning.** A port where `None` could
   mean either *clear* or *inherit* takes a two-variant enum instead — enforced at the family
   definition, not per-setter review.
3. **One cost per plug form, every port.** A constant costs a diff compare; a signal costs a
   direct slab patch; a reader costs a verdict check plus a slab patch; a token costs a cascade
   edge. The same form never costs differently at different ports, which is what makes the family
   teachable, and gate A1 (10 §1) measures it as descriptor identity, not stated intention.

## 3 · `Impact`: the invalidation taxonomy with zero user surface

Every mutation declares which rendering passes it dirties, attached to the one place every
mutation already funnels: the port descriptor.

```rust,ignore
bitflags! {                        // kernel-internal; never public API
    struct Impact: u8 {
        const LOGICAL;             // retained logical tree / semantics-bearing structure
        const LAYOUT;              // box sizes and positions must be recomputed
        const PLACEMENT;           // transform/offset only — layout untouched
        const PAINT;               // display-list bytes for this node
        const HIT;                 // hit-test geometry
        const A11Y;                // accessibility projection
    }
}
```

- **Every sealed port carries a `const IMPACT`**, fixed when the port seals, so the flags cost
  nothing at runtime: `.bg(...)` is `PAINT`; `.gap(...)` is `LAYOUT | PAINT`; `.translate_y(...)`
  is `PLACEMENT | HIT`; text content is `LAYOUT | PAINT | A11Y`; a structural splice is
  `LOGICAL | LAYOUT | PAINT | HIT | A11Y`. A publication's impact is a bitwise-or over the log,
  and the epoch runs exactly the union's passes (08 §1's phase list takes this as admission
  input). Media ports classify identically: a new frame on an installed media port is `PAINT`
  (a texture/fragment patch); an intrinsic-size change is `LAYOUT | PAINT`; media mount/unmount
  is structural (07 §3).
- **Custom surfaces declare impact in their phase obligations** (02 §1.2): a custom port with no
  declared impact does not seal — the mistake is unrepresentable, not reviewed. Motion lanes
  (§4) pre-classify by the same kind-segregated scheme, so this section is the naming of a
  discipline the runtime already obeyed.
- **User code never invalidates, in either direction.** No `request_paint()`, and no ability to
  suppress one either — an author cannot claim a text edit is paint-only. Precision is the
  framework's obligation; gate A3 (10 §1) holds it to counters: a `bg` publication on a laid-out
  tree runs zero layout, zero a11y, zero hit work.

The quiet payoff is diagnostic: the inspector answers *"why did layout run this frame?"* with a
port name and a source, because the impact union is an attributed log, not an emergent
consequence of dirty bits scattered through the tree.

## 4 · Motion: a property value, sampled on a timeline

Animation is a `Source`, not a per-frame closure — a borrowed mount descriptor accepted by the
same typed property sink as a constant or a signal:

```rust,ignore
fn view(&self) -> El<'_> {
    let reveal = animate(&self.reveal).spring(Spring::snappy().preserve_velocity());

    popover()
        .opacity(reveal)
        .translate_y(reveal.reverse().scale(8))
        .child(icon(SPINNER).rotate(repeat(1.turn().over(2.s()))))
}
```

`animate(&Signal<T>)` means "observe this target and present a sampled `T`." `over`, cubic
Bezier, spring, delay, keyframes, sequence, parallel, race, repeat, reverse, and gesture handoff
are values/combinators, not callbacks. `Motion<'f, T>` is a small `Copy` draft key; its temporary
program type is erased at mount into a compact `MotionProgram` and never propagates through
`El<'f>` or application signatures. A program separates four axes: **timeline** (monotonic
presentation time, scroll, gesture, media, or explicit demo time), **effect** (tween/keyframes/
spring/decay/FLIP), **playback** (delay, fill, pause, seek, direction, rate, iteration, finish,
cancel), and **composition** (`Replace` by default; `Add`/`Accumulate` only for reviewed types).

Built-in numbers, lengths, angles, transforms, colors/brushes, compatible-topology paths, and
shader parameter fields have generated interpolation metadata; a custom `Interpolatable` must
define distance and, for a spring, velocity semantics — "four floats happen to lerp" is not
sufficient. Retargeting samples the value and velocity actually *presented* at the interruption
instant; `jump(target)` is the explicit discontinuity that resets velocity. The built-in damped
spring evaluates analytic position/velocity from initial conditions rather than variable-delta
Euler integration, so a timestamp produces the same result at 60 Hz, 120 Hz, or irregular
cadence. Reduced motion substitutes/snaps while reaching the identical final state. Completion is
one typed queued output, never a per-frame application callback.

Active programs live in kind-segregated dense lanes — paint, transform/opacity, placement,
layout, shader uniforms, structural hosts — matching the `Impact` classes of §3. One injected
frame clock samples each active lane and batch-patches retained destinations directly: there is
no signal write, owner turn, dependency discovery, heap allocation, or one task per animation
frame after warm-up. Dormant tracks request no frame; code that never supplies a motion value
allocates no lane, clock subscriber, composite layer, or shader variant.

Structural enter/exit/hero motion commits the new logical tree first, then holds byte/time-
bounded old/new presentation snapshots — visual only: they cannot retain focus, hit targets, or
accessibility nodes after logical removal, which is exactly the reconciliation law's promise
(02 §6) applied to motion.

## 5 · Style and theme tokens

The chainable Tailwind-like builder API is the authoring surface for style; the retained plan
changes only its cost model. Methods accepting dynamic values already take `impl IntoProp<T>` —
the algebra family covers literal, live, and token spellings on one method:

```rust,ignore
div()
    .flex().gap_2().px_4()
    .bg(theme.surface)
    .w(sidebar_width)
    .hover(|s| s.bg(theme.surface_hover))
```

A constant is stored directly with no graph node; a `Signal<T>`/computed field installs a direct
sink with no parent routing, no tracking, no user effect; a theme token installs a cascade edge.

**Generated schema, reviewed semantics.** The style schema generates stable field ids, layer
merge code, and — critically — reviewed obligation metadata per field, the same `Impact`
vocabulary as §3 (`background_color: PAINT`, `width: LAYOUT`, `transform: PLACEMENT | HIT | A11Y`,
`visibility: LAYOUT? | PAINT | HIT | A11Y`). A derive macro can enforce metadata presence and
generate code; it cannot infer a field's phase from its Rust type or name.

**Layered cascade.** Resolved style is `defaults < inherited/theme < base builders < interaction
variants < inline reactive values < transition/animation override`. Each layer owns a mask of
fields it specifies and a mask changed this transaction; resolution visits only fields whose
winning input might have changed. Themes use palette-token indirection — a field stores
`PaletteToken` plus an opacity/blend class, with reverse-use lists — so a palette swap patches
only its indexed consumers, not ten thousand dynamic closures. A role is a **pair**
(`fill` + `on`): `theme.accent` in a fill port is the fill, `theme.accent.on` is the ink legal on
it, and `Pair::new` always runs a contrast check — a token cannot ship without a documented
contrast pair (10 §1, gate C5).

```rust,ignore
button("Save").look(Look::Accent)          // a look = a TokenIndex recipe: pair + hover/active/disabled layers
icon(Magnifier).fg(theme.accent.on)        // an icon is ink; ink is `.fg()` — the same port as text
column().token_scope(Scope::Terminal)      // nested cascade scope; a terminal `danger` never walks editor consumers
```

**There is one inking port, `.fg()`, and one modulation port, `.tint()` — no `colorize`.** A
glyph, a monochrome icon, and a single-channel mask sprite are all *alpha coverage inked with a
color*; that color is `.fg()`, the identical property port text already uses, taking the same
`IntoProp<Color>` family (constant, signal, token, `Source`). A separate `colorize` verb was a
leak — it named the icon case of a port that already existed. What is genuinely *not* `.fg()` is
`.tint(color)`: a multiplicative modulation of an existing multi-channel image (07 §3), which
composes with the image's own colors rather than replacing coverage. So the rule is two words with
disjoint meanings — **`.fg()` inks coverage, `.tint()` modulates color** — and `colorize` names
neither.

**`tw!`** is a stable proc macro over a string literal, tokenizing and validating classes and
emitting the same ordered schema operations as fluent methods — never a UI-semantics macro (there
remains exactly one of those, `#[reactive]`, 04 §6). Unknown or invalid classes fail at the
source span. Dynamic class strings use an explicitly slower, explicitly named
`class_dynamic` escape and are never accepted by `tw!`.

**Named colors are `Color`, not an enum of hex spellings.** `Color` is a tagged, `Copy` authoring
type (`Color::Red`, `rgb(0x334155).alpha(0.4)`, `oklch(...)`, a theme token); the GPU composites
premultiplied linear extended sRGB internally, and application code never names the internal
linear type — the port conversion is sealed. Full color-space, HDR, and profile detail is
[07-platform.md](07-platform.md) §2; this document owns only the *token* and *cascade* mechanism.

**Transitions** observe a field's resolved target and are classified the same way as any other
port: a color/brush change repaints; an isolated opacity/transform update may be composite-only;
width/padding/font-size enters layout. Interrupted transitions begin from the presented value;
reduced motion snaps to the final state. Named filters (`.blur(5)`, `.backdrop_blur(12)`) are
style fields with reviewed obligation metadata, not a shader DSL on every node — a custom pass is
an isolated `.shader(MyFx)` or the retained temporal graph of [07 §2.3](07-platform.md), both on the
same phase-capability surface as §1.2 of [02](02-elements.md).

## 6 · Publication is a hint

> **L2 — publication is a hint; the destination compare is the truth** (00 §4). A `DerefMut`
> guard publishes on drop, unconditionally and cheaply. Every retained destination — property
> slab, region operand, delta consumer — compares the incoming operand against its retained one
> and no-ops on equality.

This single invariant answers the recurring "when does my handler actually fire" question:
`*self.count += 0` costs one key enqueue plus one compare, never a rerun, a repaint, or a
`changed` invocation. It makes the equality gate on a derived read (`d.store.unread` firing only
when the *value* changed, not merely republished) a corollary rather than a special mechanism,
and it gives destinations that cannot compare (non-`PartialEq` payloads) an honest fallback:
declare always-propagate, and let the inspector show which edges are compare-gated versus
always-on.
