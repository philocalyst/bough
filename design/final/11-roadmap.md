# 11 — Roadmap

Scope: the dependency-ordered build plan — what to build first, what proves what, which gates
from [10-quality.md](10-quality.md) become required CI jobs at which phase, and where the design
tolerates a legacy island so the migration is incremental rather than a flag day.

Depends on: [10-quality.md](10-quality.md) (every exit gate cited here). Feeds: nothing — this is
the terminal document, the one a team actually schedules against.

This roadmap distills two source plans (`migration.md` Part II's phase structure and
`pr-plan.md`'s ~100 PR-granular sequencing) into phase-level exit gates, respelled to the final
surface — no `cx.notify`, `on_change`, `Task`-as-a-value, or `Signal: Deref/DerefMut`-write
examples survive from those sources; every code shape below is `changed`/`Handoff`/`Reader`/
`.action(...)`.

---

## Cross-cutting rules for every phase

1. **Green main, always.** Every change compiles and passes existing tests behind the current
   public surface until its phase's flip point. New machinery enters as crate-private or behind a
   feature flag; nothing leaks a public spelling before its surface phase.
2. **Oracle before optimization.** A change that skips work lands *with* the from-scratch oracle
   it is checked against, and with its counters (10 §5). A perf change without a counter delta
   does not merge.
3. **Gates as CI jobs.** Each gate in [10-quality.md §1](10-quality.md) lands as a named,
   required CI job at the phase noted below — not "eventually," at that phase.
4. **Deletions are their own changes**, gated on zero first-party usage, and never remove a legacy
   path the same change bypasses.
5. **The zero-tax law (L14) is a requirement on every performance PR**, not an aspiration: it
   ships a floor-tax benchmark (tier-0 idle/counter/no-`Signal` workloads before/after, within
   noise or improved) or it is rejected regardless of its promoted-path win.

## Phase 0 — Freeze the contracts

Conformance fixtures for the current layout/paint/input/focus/text/accessibility behavior;
deterministic headless clock and event traces; a first `UiSnapshot` schema reusing the existing
accessibility debug capture as an adapter; a counter registry and frame-trace format
(no-op in release); benchmark runners for the reference workloads (10 §5); pinned competitor
ports at fixed commits; a `trybuild` harness for the diagnostics corpus (10 §4); a panic-free lint
baseline (`unwrap`/`expect`/indexing denied in kernel non-test code, existing violations
allow-listed with a burn-down file).

**Exit gate:** counters prove non-vacuity on one fixture; benchmark results are reproducible; the
current baseline is stored. This is the phase where "surpasses" becomes measurable at all.

## Phase 1 — Retained identity and ownership

Per-window generational node arena and owner tree; hot node-header slabs, kind-segregated
property slabs, cold side records (08 §4); internal branded arena views (`Scope<'id, K>`, 02 §8)
turning validated public ids into non-escaping capabilities, with `trybuild` coverage for
escape/alias misuse; explicit mount/unmount transactions and a deferred reclamation epoch; stable
handler/focus/IME/accessibility/semantic ids; a **legacy-view island**: the retained tree can
host today's existing render output, and vice versa, so one screen migrates at a time while the
rest of the application runs unchanged (the single most load-bearing item in this phase — every
later phase's incrementality depends on it existing first); an inspector v0 over the arena with
live-object gauges.

**Exit gate:** reorder/mount/unmount tests preserve the right local state; cleanup runs exactly
once; late callbacks fail harmlessly; duplicate semantic ids diagnose precisely; churn gauges
return to baseline (gates M1/M4, 10 §1).

## Phase 2 — Persistent layout, dispatch, and accessibility

Persistent layout-node lifecycle with style/children patch operations; write-free `Cx<'s,
Layout>`/`Cx<'s, Geometry>` (02 §1.2) with custom measure unable to reach write/structure APIs
(compile-fail coverage); reasoned layout dirt and dirty-child summaries; retained event routing
and focus order; an incrementally updated hit-test spatial index; accessibility delta
publication; the sparse placement graph (02 §7) with located cycle/fallback diagnostics; the
bounded layout-feedback loop (02 §1.2's `WrapDemand`-shaped bridge). A debug mode recomputes
layout/hit/a11y from scratch and compares results on every randomized mutation test.

**Exit gate:** leaf layout work scales with the true dependency closure; scroll translation
rebuilds no hit state; no incremental/full-oracle mismatch remains; feedback cannot demand-compute
structure/effects/resources or exceed its declared cap; anchored placements track scroll/
transform/scale and publish matching paint/hit/a11y geometry in one epoch.

## Phase 3 — Retained fragments and damage-aware presentation

Immutable per-node scene fragments and resource references; the order sum-tree with localized
splice/move (08 §4); an adaptive spatial damage index queried before retained-order replay;
transform-only and fragment-replay paths; the renderer frontend boundary (07 §2.2) with one
backend selection per packet/pass/range, never per primitive; bounded in-flight frame-packet
versions with backpressure and device/surface-loss recovery; experimental stable GPU
primitive/glyph slots behind a capability tier; the retained canvas node and render-graph pass
contract (02 §1.2's phase-capability surface extended to custom shaders; the temporal graph of
07 §2.3); full-damage kill switches as diagnostics, not a permanent architecture.

**Exit gate (gate 6b begins here in spirit, required at Phase 8):** visual equivalence under
randomized change; idle submits nothing; paint-only leaf and translation scroll meet their
zero-work phase gates; the reference/GPU frontends consume the same recorded packet protocol;
device loss rejects old imports and reconstructs residency without stale handles.

## Phase 4 — The reactive kernel

The heart of the design, and the phase where the ladder's semantics — not yet its public surface —
get proven:

- an intern table minting dense per-window slots on first bind (crate-private);
- `Signal<T>` core: `Deref`/`DerefMut`, publish-on-drop, `!Clone`, the four-byte-overhead
  representation (08 §4); the described no-view-write invariant as `trybuild` negatives;
- the describe-frame read-set (per-owner recorded cells, diffed each describe) and the dirty law:
  an owner re-describes exactly when a handler ran on it, a recorded read's cell published, or
  structure changed — nothing else, proven as a property test over random turn scripts;
- direct property ports with consumer-side equality cutoff (03 §6/L2): a spurious write patches
  once, zero describes;
- the tape diff with span hashes: an unchanged describe patches nothing;
- the turn protocol (08 §1) with two-epoch atomic fault injection;
- `List<T>`/`Slot<T>` with sealed delta ops and the contiguous-item-store representation (01 §4,
  08 §4);
- `project`/`.map` regions with capture-free-closure dedup (identical projections over the same
  sources share one computation);
- `changed`'s delta assembly: `Changed<'_, Self>` as a lazy view, `O(publications)`, recursing
  through `Entity<T>` fields exactly as through contained structs (01 §8, 04 §1);
- the FIFO dispatcher sink for entity `send`, and `Handoff<T>` give/take with the diagnosed-
  untaken fixture (01 §6);
- `Reader<T>` and the `StampedCompletion` wake law (05 §2), with stale-completion and
  drop-cancel races as required tests;
- the two-way writeback commit that `text_input` (06 §3) will be built on;
- arrangements (`filtered`/`sorted`) with the field-aware-sealed-vs-wildcard-predicate split
  (03 §1, priced by counters);
- the kernel delta log — `ListDelta`, `Replace`, text/dock deltas, `Origin`, the gesture token —
  that [`01-state.md` §10](01-state.md)'s history crate will consume, off by default.

**Exit gate:** writes run no reactive computation recursively; event read-your-writes and
between-event publication pass; graph fuzzing finds no glitches; key size/`Copy`/`!Send`
assertions, stale/runtime-mismatch, same-slot reentrant-draft-borrow, and cross-thread-key tests
pass; direct-field wires meet their allocation/byte gates before the arbitrary dynamic path is
accepted as the default fallback. **Milestone: kernel semantics are proven; no public surface
exists yet.**

## Phase 5 — The public surface

Everything in [02-elements.md](02-elements.md), [03-ports.md](03-ports.md), and
[04-reactions.md](04-reactions.md) as compiled, gated fixtures:

- the sealed boundary traits (`Source`/`Sink`/`Cell`, `IntoProp`, `ChildSeq`) with
  `#[diagnostic::on_unimplemented]` text, snapshot-tested (the diagnostics corpus gate,
  [10-quality.md](10-quality.md) §1/§4);
- constructor-position children, text-leaf-only strings, the one-children-rule negatives
  (02 §4.1);
- style literals and the `Impact`-tagged schema (03 §3, §5);
- route resolution (L4) and the handler-outcome table (04 §3) as a fixture matrix;
- `changed`, `Handoff`, and the recursive-delta depth fixture (04 §1, gate "Depth");
- identity — `Target<K>` (minted at construction, bound once per describe, parked claims when
  unbound), `Slot<T>`, no id enum, no derive on application structs;
- the typestate-holding test driver and inspector target panel (10 §2's `Win<V>` semantic query
  surface);
- `text_input`'s document-of-record transaction and interceptor law (06 §3), built at the port
  layer;
- the overlay family (popover/dialog/sheet/hover-card) over the placement graph;
- windows as elements (`Application::run(root)`, `self.open(...)`, `Window<V>` builder) wrapping
  the platform spawn path;
- the fallibility surface — `Miss` reasons, delivery revalidation, absence-as-state on every
  lookup.

**Exit gate:** gates 1, 2, 4, 5, A1, A2 (10 §1) become required jobs from this phase on — the
promotion-invariance CI (source compiled against every ladder rung) and the full compile-error
corpus. **Milestone: surface complete; libraries can be written against it.**

## Phase 6 — Collections, text, motion, and history at scale

Four tracks that fan out in parallel once Phase 5 is stable, because each depends only on the
kernel (Phase 4) and the public surface (Phase 5), not on each other:

- **Collections**: `virtual_list` with anchoring/variable extents/retention reasons (07 §8's
  virtualization law); row recycling; the typed drag world with linear in-hand delivery (a
  dropped payload restores unless taken — `Handoff`-class linearity); grid and tree protocols
  (07 §8).
- **Text**: the shaping/bidi substrate; the placeholder-atom IFC (06 §2, gate Text-1); the full
  CSS-complete inline style schema (06 §1, gate Text-0); `text_input` and its interceptor/IME
  transaction law (06 §3, gates T2/T3) rebuilt on the retained-field kernel from Phase 4.
- **Motion**: the timeline/effect/playback core and dense kind-segregated lanes (03 §4); springs,
  decay, interruption, and gesture handoff; the structural presentation host for enter/exit/hero
  motion; earned/predicted composite layers (08 §6).
- **History**: the `gpui_history` crate (01 §10) over Phase 4's delta log — `Track`/DAG/tagging,
  selective undo and conflict transforms, `FileStore` and Sudden-Termination recovery. This
  crate links nothing the kernel depends on; gate H0's symbol audit proves it.

**Exit gate:** platform IME fixtures, cursor/model echo, and animation phase-matrix tests pass;
editor and animation reference workloads meet their work gates; 10,000-track sampling reaches
only its declared lanes and matches the scalar reference interpolator at identical timestamps;
gates H0–H10 and Text-0–Text-7 (10 §1) become required.

## Phase 7 — Style schema, media, canvas/shaders, color

Generated multiword field masks with obligation metadata; the slotted cascade (03 §5); the
`tw!` proc macro emitting the same schema operations as fluent methods; the media pipeline
(07 §3) — staged acquisition, decode/variant/residency caches, the `IntoMedia` plug family, native
video import with counted fallback; the canvas/shader public surface (`impl Canvas`, typed
uniforms, retained temporal graph, 03 §4/07 §2.3); tagged `Color`, the linear-extended-sRGB
working contract, and the negotiated `ExtendedSrgb16` surface (07 §2.1) — with the SDR fixture's
byte/pass/pipeline counters held exactly unchanged from a build with the feature disabled.

**Exit gate:** the style mutation matrix proves exact phase entry for every field; unknown/invalid
`tw!` classes fail at the right span; the 10k-consumer palette gate and opacity-class promotion
pass without per-consumer computed closures; T4, C0–C10, and the temporal-shading gate (22 in
[10-quality.md](10-quality.md)) become required.

## Phase 8 — The editor kernel and workspace boundaries

`Pos<Space>` coordinate spaces and the composed display-map transform stack (06 §5); the async
wrap producer wired through Phase 4's `Reader`/`StampedCompletion` machinery; `MultiBuffer` and N
viewports including nested minimap; the entity kernel's non-generic, scope-owned core and cyclic
construction with one `Entity<T>` type — no strong/weak axis (09 §1); open item sets via
`typetag`/serde (in-process) and `abi_stable` (dynamic plugins), not a framework catalog, with
structural journaling collapsing open/restore toward one code path (01 §9, 09 §2); the three-layer
durable identity story
(09 §3); `PresenceSession` as a supervised protocol (09 §4).

**Exit gate:** gate 18 (editor/display pipeline `trybuild` negatives, N-viewport coherent
snapshots), gate 24 (workspace item protocol: two `typetag` tag-spaces, parallel restore, stale-completion
rejection, unsafe-unload impossibility), and gate 25 (cycles/focus/presence fuzzing) become
required. This is also where the fast-path work of [08-runtime.md §6](08-runtime.md) begins in
earnest — sequenced *after* Phase 4–8 prove semantics, never before, because the fast lane is a
runtime-internal optimization of an already-correct kernel, not a substitute for one.

## Phase 9 — Product migration and the default flip

Migrate representative screens in increasing difficulty: counter/settings form; menus/overlays/
focus traversal; a large keyed table and virtual list; an editor surface with IME; a multi-pane
workspace with drag/drop, actions, and custom paint; arbitrary anchored overlays plus a
shader-backed canvas and a deterministic end-to-end integration suite. For each screen, record
legacy-island percentage, frame work, latency, allocations, memory, and behavioral gaps.

The **default flip**: the retained surface becomes what the framework *is* — docs, templates, and
new first-party code assume it; the legacy path remains available behind its island (Phase 1)
until the deprecation pass below. APIs may still break while the retained model is greenfield;
once two genuinely difficult products converge on it, the public vocabulary freezes and a
compatibility window is published.

**Exit gate:** gate 17 (the Zed-shaped migration corpus) and gate 6b (the competitive zgui-class
workload) are both required from this phase; no new first-party UI uses the legacy island; the
competitive gates ([10-quality.md](10-quality.md) §1) reproduce on two hardware classes.

## Phase 10 — Deprecation and legacy removal

Every legacy-era spelling this folder's rejection list names (00 §6) — plus whatever migration
concept-map entries a team's own codebase still carries — gets a deprecation lint naming the
final spelling as its fix-it, snapshot-tested. Legacy subsystems are removed one at a time, each
gated on zero first-party usage and a released compatibility window: the deprecation pass never
removes a path the same change bypasses (cross-cutting rule 4). Full-damage/full-relayout kill
switches (Phase 3) narrow to real platform/device-recovery cases only; a release build cannot
enable a diagnostic fallback.

**Exit gate:** a symbol audit shows each deleted path is gone; the legacy island still works via
its Phase-1 seam until its own final removal PR; one architecture remains.

## Phase 11 — Optional compiler assistance

Only after the explicit Rust API is stable: diagnostics/boilerplate improvements to
`#[reactive]`; static dependency hints verified against runtime tracking in debug builds; const
subtree templates (08 §7) for mount-time dense allocation, lowering to the same public primitives
and providing no exclusive steady-state fast path a hand-written equivalent couldn't also reach.
Any such addition must expand to readable public primitives, preserve source spans, and bail out
to runtime tracking rather than guess across an opaque call — it is rejected outright if it
becomes *necessary* for peak performance or introduces a second semantic model. This is the one
phase this roadmap does not schedule a date for: it is evaluated, not planned.

## Milestone summary

| Milestone | After phase | Meaning |
|---|---|---|
| Island era begins | 1 | new and old coexist screen-by-screen |
| Kernel semantics proven | 4 | ladder nouns exist; no public surface yet |
| Surface complete | 5 | libraries can be written against the final API |
| Feature-complete parity | 8 | text/history/motion/media/editor/workspace all gated |
| **Default flip** | 9 | the final surface is what GPUI *is* |
| Legacy gone | 10 | one architecture remains |

The sequencing protects its highest-value early wins deliberately: the Phase-1 island makes every
later phase safe to land incrementally; Phase 4's `Signal`/describe-frame/tape/`changed` triad
delivers the authoring-experience story long before any performance claim is made; Phase 6's text
and history tracks are user-visible flagships that need only the kernel, not the fast path; and
all of [08-runtime.md](08-runtime.md) §6's fast-path mechanisms are optional-until-proven — the
default flip at Phase 9 does not wait for them, because the ladder's floor (microsecond
re-describe plus tape diff) already has to clear the Phase 3 gates to get there at all.
