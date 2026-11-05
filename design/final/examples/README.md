# `final/` examples — ports and reactivity, in full

These sketches target the **final** surface (`final/00`–`11`) directly, and they exist to make two
things concrete that prose can only assert: the **port algebra** (03) and the **reaction model**
(01 §6/§8, 04). They fold in the resolutions from the criticism pass — the ecosystem-first command
and item boundaries, the boundary-only delta, `.fg()` inking, and the deletion of `WeakEntity`.

They are **syntax sketches**, not expected to compile against current GPUI. Read them
immediate-first: every file opens in the untracked style (plain fields, `&mut self`, whole-`view`
reruns) and promotes to tracked (`Signal`/`List`/`Reader`, `#[reactive]`) only where a comment says
a real program would earn it — that is the immediate→retained spectrum of 00 §2.2, not a static vs.
dynamic split.

| Sketch | Anchors | What it demonstrates |
|---|---|---|
| [progressive.rs](progressive.rs) | 00 §2.2, §3 (ladder), L13/L14, 02 §3 (`const`) | **One component, four rungs.** The same `view` body as `count: u32` (immediate) → `Signal<u32>` → a hot `List` row → an `Entity`, with the diff (gate 3) proving pixels never move — only *who reruns* changes. `const` layout recipes throughout. |
| [ports_gallery.rs](ports_gallery.rs) | 03 §1–§5, 05 §3.1, L2, L9 | **The whole `IntoProp`/`Source` algebra through one method.** A constant, `&Signal`, `Reader`, token, `Motion`, and a mixed tuple all plug into `.opacity(..)`/`.fg(..)`; the `Cell` vs `Source` direction check; `is_pending`/`map` compose because they are total functions on `impl Source`; `.fg()` inks, `.tint()` modulates, no `colorize`; `reader.or(fallback)` → a Deref-able `Settled`. |
| [reactions.rs](reactions.rs) | 04 §1.2–§3, L7, L10 | **The self-delta is boundary-only, structurally.** A parent reacts to a child `Handoff`, a shared `Entity`, and an async `Reader`; its own scalar edits are not even *fields* on `Changed<Self>` — they route to methods. `#[derive(Delta)]` is pulled by the observer with a located error. Routes are method paths (the idiom); the non-capturing closure is the dispreferred fallback. |
| [commands.rs](commands.rs) | 04 §3.1–§3.2 | **The function is the identity.** Bare keys/clicks/menus route to fn-items with no name. The one thing that must cross serialization — a keymap file — uses a plain `&str` id registered with the ecosystem's `inventory` slice, not a `Command<A>` newtype. Remap round-trips through JSON. |
| [plugin_item.rs](plugin_item.rs) | 09 §1–§3, L5, L11 | **No catalog, no weak handles.** Closed tab set is an enum; an open, first-party set is `typetag` polymorphic serde; a dynamic plugin is `abi_stable`. `Entity<T>` is a scope-owned generational capability — cyclic construction with one type, no `WeakEntity`. `ItemInstanceId` is never named. |

## The two ideas these files are really about

**Ports (03).** A *port* is any place a value enters the tree — a property sink, a child slot, a
style field. There is exactly one closed family of things you may plug in (`IntoProp<T>`), and one
sealed algebra they are all instances of (`Source`/`Sink`/`Cell`). The payoff is that questions
that look like they need per-feature design — "can I animate a token?", "is this pending?", "can I
feed a derived value here?" — become *type facts*, answered once over `impl Source`. `ports_gallery.rs`
is that sentence made executable.

**Reactivity (01/04).** State lives in fields; `view` is a projection of them; `changed` is the one
effect surface, handed a typed diff. The diff only ever carries what crossed a **boundary you did
not clock** — a child, an entity, an async landing — because a transition you caused yourself, you
handle in the handler that caused it. `reactions.rs` is that split made executable, and it is why
the `Delta` derive is pay-per-observation rather than a blanket tax.
