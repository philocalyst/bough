# 10 — Quality: Gates, Testing, and Diagnostics

Scope: every stabilization gate from every pass, consolidated into one registry under permanent
IDs; the testing/demo surface (`Application::demo`, `Win<V>`, turn barriers, semantic queries);
the inspector contract; diagnostics as a designed surface, not an afterthought; and how the
performance and competitive claims elsewhere in this folder are actually proven rather than
asserted.

Depends on: every other document in `final/` (this is where each one's claims become checkable).
Feeds: [11-roadmap.md](11-roadmap.md) (gates become required CI jobs at specific phases).

---

## 1 · The gate registry

Three registry laws govern this section — they are what makes a 100-gate list maintainable
instead of aspirational:

> **A gate ID is a permanent name, never a position.** Numbered gates keep the number the pass
> that ratified them assigned; families carry a prefix (`A` algebra, `R` reactions, `T` platform
> bar, `Text` text, `H` history, `C` color, `MED` media, `M` memory, `FP` fast path, `PG`
> protocols, `LT` latency, `D` diagnostics, `Q` the registry itself). A superseded gate is never
> silently dropped or its ID reused: it gets a tombstone row (§1.12) naming its replacement.

> **Every law has a gate** (gate Q0). Each of L1–L14 is named by at least one row below — the
> traceability table (§1.0) is what makes the constitution *falsifiable by construction*. A law
> with no binding gate is a defect in this document, and Q0 is the CI check that greps for it:
> the registry audits itself the same way it audits the kernel.

> **Every gate has a phase.** The `Ph` column is the [11-roadmap.md](11-roadmap.md) phase at
> which the row becomes a *required* CI job (e.g. `P5` = required from Phase 5 on). A gate with
> no phase cannot be scheduled and therefore cannot be claimed.

### 1.0 Law → gate traceability

| Law | Bound by |
|---|---|
| L1 frame borrows / diff retains | 13, 6b, FP5 |
| L2 publication is a hint | 6, A1, R4 |
| L3 reconciliation | A2, R9 |
| L4 route receiver resolution | 7, R9 |
| L5 producer ownership / `Fault(Revoked)` | 8b, 9 |
| L6 wire boundary = scheduling boundary | 12, R4 |
| L7 circuit vs effect | C1, R6 |
| L8 sequence shape deltas | A2 |
| L9 impact / logged union | A3, FP4, MED6 |
| L10 state is the protocol | R1–R4, R7 |
| L11 restore = re-derivation | 24, H2, R2 |
| L12 latency trichotomy | LT1, 8, 8b |
| L13 gate-3 symmetry | 3, R5, C1 |
| L14 zero tax | 6b, M0, MED8, FP-family floor-tax clause, H0, C10, 22 |

### 1.1 Surface and branch freedom

| # | Gate | Doc | Ph |
|---|---|---|---|
| 1 | No-macro Rust: nested `if`/`match`/`if let`/`?`/early returns/borrowed locals/`Option<El>`/iterators/`Flex`/`Grid` branches/**conditional setters** compile with no erasure call or macro. | [02](02-elements.md) §2 | P5 |
| 2 | One UI-semantics macro in the workspace: `#[reactive]`. No coarse/fine variant, no builder macro, no signal expression DSL. (`#[derive(Delta)]` is a data derive; `tw!` is a validated literal — the census counts them as non-UI-semantics and R7 checks the derive.) | [04](04-reactions.md) §6 | P5 |
| 3 | Semantic equivalence (gate-3 symmetry, L13): adding/removing `#[reactive]` changes only work counters and the dependency manifest — never scene, state, events, focus, a11y, history, or drop order; removing it also preserves the explicit `project` fixture's semantics verbatim. | [00](00-model.md) §4, [04](04-reactions.md) §6 | P5 |
| 4 | LSP: completion, rename, go-to-definition, inlay hints, extract-function, rustfmt, diagnostics, and source-mapped expansion work inside macro bodies; failure deletes the macro from the release, not the no-macro path. | [04](04-reactions.md) §6 | P5 |
| 5 | Builder typing: `Flex`/`Grid` wrong-family methods fail locally with a useful diagnostic; completed elements branch freely; **no rustdoc page or compiler error ever prints a recursive style type**. | [02](02-elements.md) §3 | P5 |
| 6 | Direct parity: handwritten property ports and `project` regions have byte-identical runtime descriptors to macro-lowered equivalents and allocate zero after installation. | [03](03-ports.md) §1 | P5 |
| 6b | zgui by construction: a ported apples-to-apples workload allocates zero on an unchanged subtree, never traverses a clean tree, and stays in ImGui's authoring class and zgui's steady-state class on the same renderer. | [08](08-runtime.md) §8 | P9 |
| 13 | Borrowed element: function roots need no adapter struct; an `El<'f>` that escapes `describe` (stored `'static`, sent, held to the next frame) is a compile error under `trybuild`/Miri; description performs zero clones/interns; the commit diff promotes only the changed operand; **no public API takes `Frame`**; and the covariance triple holds — (a) `El<'static>` flows into a frame arm, (b) `El<'f>` still cannot escape or cross frames, (c) nested candidate-description arenas are brand-separated or provably harmless (02 §1.1). | [02](02-elements.md) §1 | P4 |
| A1 | Algebra matrix: every verb × every `Source`/`SeqSource` instance × every `IntoProp` plug form produces descriptor-identical installations to macro lowering and allocates zero after install; rejected combinations fail with the family's diagnostics. | [03](03-ports.md) §1–2 | P5 |
| A2 | Sequence coherence: each `ChildSeq` form's shape delta lowers to its declared splice class; slot forms preserve identity across permutation, ordinal forms mount-cycle exactly per L3; a plugin boundary is the only place erasure appears in the corpus. | [02](02-elements.md) §4 | P5 |
| A3 | Impact precision: a `PAINT`-only publication runs zero layout/hit/a11y work on a warm tree; a `PLACEMENT` publication moves a subtree without re-layout; **a text edit runs layout on the damaged range only**; the union over a mixed turn is exact and every pass is attributable to a logged port. | [03](03-ports.md) §3 | P5 |
| D1 | Diagnostics corpus: a `trybuild` suite asserts the *message text* of the top predictable mistakes — wrong layout-family method, escaped element, by-value signal, `*reader`, captured `&self` in a retained projection, mismatched route receiver, read-only source in a two-way port — written in this folder's vocabulary (§4). The no-macro surface is held to the same editor-experience bar as the macro. | §4 below | P5 |
| Q0 | The registry audits itself: every law L1–L14 appears in §1.0 with at least one live gate; every gate row carries a phase; every tombstone names its replacement. A claim elsewhere in `final/` of the form "gate X measures this" must resolve to a row here. | this doc | P0 |

### 1.2 Reactions

| # | Gate | Doc | Ph |
|---|---|---|---|
| 7 | Handler flow: guard/blackout fixtures prove guards are ordinary early returns and propagation; recursive deliveries remain FIFO and non-reentrant. | [04](04-reactions.md) §3 | P5 |
| C1 | Circuit lint: the static lint fires on the canonical derivable-state fixture and stays silent on the canonical legitimate-effect fixture (focus claim, sound, entity send, `Handoff` take); the dynamic tier attributes a cross-component derivable-state cycle within two turns. | [04](04-reactions.md) §5 | P5 |
| R1 | Zero-vocabulary grep: `Output<`, `.emit(`, `.fired(`, `impl View<`, `when(`, `Wired`, `.layer(`, `.overlay(` appear nowhere in the corpus; no embed functions remain; struct-view children are plain fields. | [00](00-model.md) §6 | P5 |
| R2 | `Handoff` linearity: give-then-take round-trips; double-take is `None`; an untaken `Handoff` at turn-end produces the located diagnostic; `Latest` coalesces; FIFO drains in order; durability (give → journal checkpoint → restore → take succeeds — the crash-safe delivery fixture no event system can pass). | [01](01-state.md) §6 | P4 |
| R3 | Depth: a grandparent consumes a grandchild transition through the recursive delta with no intermediate code in the middle component. | [04](04-reactions.md) §1 | P4 |
| R4 | Delta completeness and cost: every publication kind (signal write, `List` splice/edit/move, entity recursion, `Handoff` give, plain-field commit compare) is representable and observed in one `changed` body in the corpus; assembly is **O(publications), never O(fields)** — a large struct with one signal publish produces one delta entry and zero compares beyond the destination's own. | [01](01-state.md) §8 | P4 |
| R5 | Loop attribution: a `changed` that re-dirties its own read set is surfaced by the inspector with the cycle path within two turns. | [04](04-reactions.md) §2 | P5 |
| R6 | Containment: no reaction construct appears in any `view` body in the corpus; no `changed` body constructs elements. Grep-able, and the point of the split. | [04](04-reactions.md) §1 | P5 |
| R7 | `#[derive(Delta)]` output is public, readable, `cargo expand`-stable, and round-trips serde; the macro census still counts exactly one UI-semantics macro. | [01](01-state.md) §8 | P4 |
| R8 | TEA fixture: the userland message-passing pattern (04 §7) is implemented with zero framework additions. | [04](04-reactions.md) §7 | P5 |
| R9 | Reaction lifecycle leak: delta-driven invocation edges and mount-resolved routes arm at mount and die at unmount under a churn/leak fixture; a mount-cycle (L3) revokes every producer cell, route record, and pending claim below it exactly once. (Replaces tombstoned gate 19 — §1.12.) | [02](02-elements.md) §6, [04](04-reactions.md) §2 | P4 |
| 10 | Entity/link cost: application-only incremental rebuild and link benchmarks prove new `Entity<T>` types add only proof shims/table entries, not scheduler/renderer copies; no dylib is required. | [09](09-boundaries.md) §6 | P8 |

### 1.3 Async and cohorts

| # | Gate | Doc | Ph |
|---|---|---|---|
| 8 | Work freedom: Tokio, smol, a custom executor, `std::thread`, and a channel drive equivalent readers/actions with no GPUI task type; **physical cancellation may differ across them — admitted pixels, event order, and stale-delivery rejection may not**; 100 duplicate/spurious `PollWake`s cause zero signal writes or memo evaluations; only `Poll::Ready` emits one `StampedCompletion`, which runs no application code before its next ordinary turn; a completion arriving mid-flush is deferred. | [05](05-async.md) §2 | P4 |
| 8b | Async reads match the reference state machine (§9 of [05](05-async.md)): declaration waterfalls, first readiness, no-fallback revalidation, `reload_on`, `is_pending`, `latest`, **urgent text input above a pending search never freezes**, unrelated-cohort progress, `A -> B -> A`, out-of-order success/fault, unmount/remount, retry (the `Fault → Pending` heal row), owner revocation, and the entanglement rule (05 §9.4) all named and fixture-covered; `trybuild` rejects `*Reader<T>`, a retained projection capturing `&self`, a reader/element across `await`, and an unfrozen worker input. | [05](05-async.md) §9 | P4 |
| 9 | Fallibility: fuzz stale slots/targets, cancellation, first-load and stale revalidation failures, window/media/device loss, **drop rebase**, worker panic, mutable-handler panic, plugin unwind, `panic=abort`; domain `E`, cancellation, and framework `Fault` never alias; no sibling reader is cancelled by another's failure; no half-cohort publishes; **no result is silently swallowed** — every discarded delivery is a counted, attributed disposition. | [05](05-async.md) §6 | P4 |
| LT1 | Latency trichotomy: per-shape contract fixtures (question/action/stream) plus the mis-spelling rejections (a stream result cannot install as a property port; a question producer performing an observable side effect is inspector-attributed). *(Renamed from "L1" — §1.12 — to free the `L` prefix for laws.)* | [05](05-async.md) §8 | P4 |

### 1.4 Platform bar (required CI job on every supported backend)

| # | Gate | Doc | Ph |
|---|---|---|---|
| T1 | Activation: AccessKit connection on an idle, never-repainted window yields a complete, correct initial tree; incremental updates thereafter are delta-exact. | [07](07-platform.md) §5 | P2 |
| T2 | IME completeness: preedit, commit, surrounding-text deletion, UTF-8/UTF-16 range agreement, selection sync, candidate-rect tracking, focus-loss settlement, cancellation, single-transaction undo grouping — scripted IME, in CI. | [06](06-text.md) §3, [07](07-platform.md) §4 | P6 |
| T3 | Controlled coherence: no reachable interleaving of edits, async work, focus changes, and a11y edits leaves the box/signal/history/a11y node disagreeing; blur never loses text. | [06](06-text.md) §3 | P6 |
| T4 | Media directness: every `IntoMedia` plug form reaches pixels with no user-written encode, data URL, or canvas detour. | [07](07-platform.md) §3 | P7 |
| T5 | Default semantics: a semantic audit over the standard element set finds no retained interactive node without a role, name, and action surface. | [07](07-platform.md) §5 | P5 |
| T6 | i18n raster: a mixed-script CJK + RTL + fallback-font fixture — an Arabic selection-and-caret pass, a CJK IME entry pass, and a mixed-direction paragraph — runs on every supported backend and the reference CPU renderer. | [07](07-platform.md) §2, [06](06-text.md) | P6 |

### 1.5 Windows, application, platform, migration corpus

| # | Gate | Doc | Ph |
|---|---|---|---|
| 11 | Application unity: binaries, examples, tests, multi-window plans, and embedded loops begin with `Application`; no hidden scheduler remains. | §2 below | P5 |
| 12 | Promotion: `T <-> Signal`, `Vec <-> List`, and contained `<-> Entity` fixtures preserve ordinary view source except honest dereference/reader changes. | [01](01-state.md) §2–5 | P5 |
| 14 | Demo barriers: open/key/pointer/text/clock/ready-work drivers return after one committed presentation; normal assertions contain no `settle`, sleep, window index, or scene traversal. | §2 below | P5 |
| 15 | Typed windows: multi-window tests retain `Win<V>` values, no title/string lookup; closed/stale handles return a typed miss; scale/display/activation transitions publish. | [07](07-platform.md) §1 | P5 |
| 16 | Platform matrix (one fixture family): window material/chrome, CPU fallback, device recovery, text raster policy, clipboard/drag, menus/tray/dialogs, IME, RTL/inheritance, display readiness, external-loop — each works or returns its declared `Unsupported`/fallback result. | [07](07-platform.md) | P7 |
| 17 | Zed migration corpus: representative editor/pane/modal/notification/context-menu/workspace-item/command/**subscription**/async/custom-paint/multi-window files compile with no ambient-context threading (`cx: &mut Context<_>` — the scoped phase token `Cx<'s, K>` is exempt), no branch erasure, no framework `Task`, no manual notify; the editor's glyph run reaches the scene arena as a borrow, not a per-frame clone. | [11](11-roadmap.md) | P9 |
| 18 | Editor/display pipeline: `Pos<Buffer>` cannot be used where `DisplayPos<'m>` is required; a display position cannot escape or cross snapshot scopes; queued numeric packets fail a mismatched `MapRevision`; transform layers compose only when spaces align; one `MultiBuffer` drives two differently sized editors plus a nested minimap with distinct coherent snapshots; **A→B→A width/font/document changes, reordered wrap completions, typing through a provisional wrap, display-worker faults, and bounded block reflow never mix map/hit/paint/a11y/selection epochs; selection drag and caret blink patch ports with zero owner reruns**; `trybuild` rejects `El<'f>` in a retained block or worker. | [06](06-text.md) §5 | P8 |
| 20 | Terse launch: `Application::run(root)` and `Win::demo(root)` drive the same turn engine as the builder, nothing on `Drop`; a one-window binary and a one-window test each fit one line. | §2 below | P5 |
| 21 | Motion semantics: timed/keyframe samples agree at identical timestamps across 60/120/irregular Hz; interruption begins at presented value/velocity; pause/seek/reverse/fill/cancel/replace-add/gesture-handoff/reduced-motion/structural-teardown reach declared final state; 10,000 warm tracks allocate zero, **write no source, run no owner or effect, and patch only their declared destination lanes**. | [03](03-ports.md) §4 | P6 |
| 22 | Temporal shading: first-frame/resize/profile/device-loss resets, portable ping-pong, **failed/dropped-frame publication, downsampled-coordinate correctness, history eviction, cadence aliasing**, hazards, velocity disocclusion, bounded readback; **a retained motion-mask → decay → flow/blob → composite fixture dispatches without rebuilding its element/canvas or cloning its scene**; a UI without a temporal graph has zero history/velocity/pass work. | [07](07-platform.md) §2.3 | P7 |
| 24 | Workspace item protocol: two independent `typetag` tag-spaces coexist in one process; duplicate kinds fail deterministically; all known tabs restore in parallel; close/move/remount rejects stale completion; duplicate tabs share a locator but preserve distinct instance state; unknown/disabled plugin records survive **byte-for-byte** behind a recovery surface; plugin leases make unsafe unload impossible; **compile-fail: no item can borrow a pane/workspace/window/app across `await`, use a global, serialize a runtime `EntityId`/`ItemInstanceId`, name an `ItemInstanceId` at all, or reach for a framework `ItemCatalog`/`ItemSpec` (open sets are `typetag`/serde; dynamic plugins are `abi_stable`)**. | [09](09-boundaries.md) §2 | P8 |
| 25 | Cycles, focus, presence: cyclic-construction panic leaves every self-handle permanently dead (`Delivery::Closed`), and a send/inspect through a not-yet-published construction handle generation-fails rather than observing a partial `T`; focus reclaim runs once after a successful target commit and **loses to unmount, a newer claim, or user/OS focus**; presence fuzzing covers replay/gap-resync/incarnation/protocol mismatch/**missing item alias**/echo suppression/reliable `Open;Close` ordering/**coalesced cursor-scroll pressure**/atomic batch application; no peer sends a runtime `EntityId`; one transport fault cannot cancel a sibling view. | [09](09-boundaries.md) §1, §4 | P8 |
| PG1–PG6 | Cross-cutting protocols: leased projections cost zero with no consumers; a dev-build no-drift oracle rebuilds each projection from scratch; fault injection mid-gesture (span retirement, window loss, splice under an active anchor); the 100k×1k grid bar (two transform patches for scroll, frozen-header-then-fling stays on the compositor); FlowIndex order ≡ semantic-tree text order; a default `pan(VERTICAL)` list begins resident scroll without entering the recognizer DAG. | [07](07-platform.md) §8 | P6 |

### 1.6 Color (the C family — gate 23's members, individually binding)

| # | Gate | Ph |
|---|---|---|
| C0 | One math: Metal/HLSL/WGSL/CPU share conversion code; no per-backend name-swapped math. | P7 |
| C1c | Working space: linear vs encoded 50%-red-over-50%-green golden matches CSS `color-mix(in srgb-linear, …)`; the old gamma blend is documented as non-default. | P7 |
| C2 | Oklab round-trip: Björn Ottosson samples, CPU vs every GPU backend. | P7 |
| C3 | Text tint: same token under macOS grayscale vs Windows ClearType passes a contrast-ratio oracle (not pixel identity); dilation-band crossings are counted. | P7 |
| C4 | Tokens: changing one palette entry patches only indexed paint consumers; zero layout. | P7 |
| C5 | Contrast: default theme roles for text/surface meet WCAG 2 AA in both appearances ([03](03-ports.md) §5). | P7 |
| C6 | Destination contract: every scene records working space, surface contract, **reference white**, tone-map policy, and realized tag/format; no fixed-P3 working path, no silent clamp. | P7 |
| C7 | Extended range: negative and `>1` fixtures survive every working intermediate and an extended offscreen target; `SdrSrgb8` applies the declared gamut/tone map. | P7 |
| C8 | Profile corpus: sRGB/Display-P3/Adobe-RGB/gray/CMYK, matrix-TRC and LUT ICC fixtures match a pinned CMS oracle within tolerance; malformed profiles fail explicitly. | P7 |
| C9 | Display move / HDR: moving a live window across SDR/HDR/profile revisions updates the presentation transform and headroom without rebuilding the logical or layout tree; no double transform. | P7 |
| C10 | Zero tax: the SDR fixture keeps the surface byte count, attachment count, pass count, **shader-variant and pipeline creation counters**, and clean-frame work of a build with color management disabled — exactly, on Metal, DirectX, wgpu, and `ReferenceCpu`. | P7 |

### 1.7 Media (from the pipeline's non-negotiable gates; [07](07-platform.md) §3)

| # | Gate | Ph |
|---|---|---|
| MED1 | Convergence: a cached static path and an equivalent HTTP response run the same parse/decode/variant/residency code after acquisition; no widget-local cache or task poll. | P7 |
| MED2 | Generation revocation: replacing a reactive media source revokes the old generation — late bytes, decode output, and imports cannot overwrite the new image or clear its pending state. | P7 |
| MED3 | Decode windows: animated-image and video decoded memory stays within declared byte/time windows under a stalled UI, renderer, or decoder. | P7 |
| MED4 | Native planes: an external/native fast path holds every source plane through GPU completion and returns each exactly once. | P7 |
| MED5 | Device loss: recovery uses no stale handle and does not refetch still-valid encoded content. | P7 |
| MED6 | Frame-advance cost: a one-frame video advance patches only the media fragment and intersecting damage — no widget rebuild, no unrelated graph computation, no relayout of fixed geometry, no RGBA copy on a proven native-import path. | P7 |
| MED7 | Fidelity: color/crop/orientation match the reference pipeline within declared backend tolerance. | P7 |
| MED8 | Zero tax: with no media present, no per-frame traversal or allocation; a static warm image schedules no work and retains only budget-accounted cache/residency entries. | P7 |

### 1.8 Memory (the M family; [08](08-runtime.md) §5 — verified by the phase-tagged debug allocator)

| # | Gate | Ph |
|---|---|---|
| M0 | Steady-state zero: idle, hover storm, scroll, animation, typing-in-existing-field, and plain-`T` counter workloads run `allocations == 0` in every phase after warmup. *Non-vacuity:* a first mount shows its class-2 allocations with correct provenance. | P4 |
| M1 | Mount storm: opening N panels allocates `O(nodes)` bytes with the stored per-type constant (learned size classes); closing returns live bytes to baseline **and** trim returns OS-measured RSS within the idle-trim window. | P4 |
| M2 | Ledger completeness: the full workload suite produces zero unattributed allocations; the inspector memory map sums to the allocator's totals. | P4 |
| M3 | Cascade order: under synthetic pressure, evictions follow the normative order, never demote below skip-paint, and never evict what the same frame then needed (thrash counter bounded). | P7 |
| M4 | Soak: a 24-hour mount/unmount/scroll/edit churn holds every ledger line flat within noise, with attribution. | P9 |
| M5 | History cap: a 10⁶-operation session holds history RAM at budget with disk spill; undo across the spill boundary is correct and its latency is printed. | P6 |
| M6 | Fragmentation: live/committed per pool stays above the stored threshold under the churn matrix; compaction is idle-lane only. | P7 |

### 1.9 Fast path (the FP family; [08](08-runtime.md) §6 — each mechanism's CI claim, none vacuous)

Every FP row also carries the floor-tax clause of L14: the mechanism's PR ships a tier-0
idle/counter/no-`Signal` before/after benchmark and is rejected on any floor regression.

| # | Mechanism gate | Ph |
|---|---|---|
| FP1 | Fast lane: a pointer-crossing/wheel trace that changes only variants/offset asserts `logical_epochs_committed == 0`, `turn_settles == 0`, `dynamic_computed_runs == 0`; damage equals exactly old+new row rects (hover) / exposed band (scroll); p50 **and** p99 input-to-present beat the ported zgui numbers. *Non-vacuity:* a crossing that hits a discrete handler or a layout-affecting variant shows `turn_settles >= 1` — the lane escalates real work instead of swallowing it. | P8 |
| FP2 | Predictive promotion: the *first* scroll/animation frame does zero restyle/relayout/repaint (transform/composite only) with promotion reason `Predicted(*)`; warm→demote thrash stays bounded under flick-cancel fuzz; `predicted-promotions == 0` on idle/counter workloads. | P8 |
| FP3 | SoA patch queues: zero allocations after warmup; dirty/upload bytes sublinear in sinks via coalescing on the 1→1000-sink fanout sweep. | P8 |
| FP4 | Damage compaction: damaged rects/pixels match old+new (hover) / exposed band (scroll); full-damage promotion fires only above the density threshold; checkerboard fuzz keeps damaged pixels bounded; promotion is conservative — artifacts are never a performance trade. | P8 |
| FP5 | Zero-alloc turns: property/counter/hover/scroll workloads assert `allocations == 0` steady-state; churn returns live gauges and arena high-water to a stable baseline; reused buffers are generationally fenced (never trust leftover bytes). | P8 |
| FP6 | Tape dedup: `spans_lowered ≪ spans_described` on the 10k-static-sibling and 100k-table workloads; collision fuzz forces a hash collision and asserts the mandatory memcmp rejects false reuse; the shared-picture immutability oracle proves per-instance differences are overlays, never mutations; the intern table respects its byte budget under churn. If dedup shows wins on genuinely non-repeating content within noise, the mechanism fails its own gate. | P8 |
| FP7 | Layout fast paths: nodes-visited/nodes-measured slopes stay in the dependency closure; the counter workload shows layout `== 0` for fixed-digit text (tabular-nums / digit-width canonicalization); every memo hit is validated by the from-scratch relayout oracle; the single-pass seam, if used, beats or matches the ported Rosin fixture. | P8 |
| FP8 | Frame pipelining: p99 input-to-present under sustained animation stays below one frame; skipped frames are counted with reason; the two-epoch fault oracle proves no torn frame between pipelined stages. *Non-vacuity:* disabling the pipeline regresses animation throughput but not input latency. | P8 |
| FP9 | Text warm paths: shaped-bytes/lines-rebroken counters stay in the edit's dependency range; first-glyph latency improves with idle-only atlas warming; layout `== 0` with tabular nums. | P8 |
| FP10 | Idle speculation: speculative pre-describe/warming is idle-lane only and never delays input (priority-lane p99 unaffected); real turns discard speculative drafts via the two-epoch oracle. *Non-vacuity:* under input saturation the speculative count drops to zero — preemption proven. | P8 |

### 1.10 Text (the Text family — distinct from the platform bar's T family by design)

| # | Gate | Doc | Ph |
|---|---|---|---|
| Text-0 | Schema completeness: every CSS text field is fluent, schema, later, or a named wall; tests enumerate fields and check obligation metadata. | [06](06-text.md) §1 | P6 |
| Text-1 | Placeholder IFC: a paragraph with text and two chips wraps, copies, selects, and speaks correctly across bidi and a soft wrap between chips; no `place_in` escape. | [06](06-text.md) §2 | P6 |
| Text-2 | Decorating box: a paragraph underline continues under a color mark and skips a `skip_decoration` chip; skip-ink does not collide with descenders. | [06](06-text.md) §1 | P6 |
| Text-3 | Spacing stack composes (letter-spacing, word-spacing, indent, hanging-punct, justify); a hanging period is ink overflow in read-only text and scrollable in an editor. | [06](06-text.md) §1 | P6 |
| Text-4 | Wrap styles: `stable` never reflows lines above the caret while typing; `balance` on a 3-line title matches a pinned screenshot. | [06](06-text.md) §1 | P6 |
| Text-5 | Fill/stroke/shadow do not change wrap; damage is paint-only. | [06](06-text.md) §1 | P6 |
| Text-6 | Paragraph isolation: an edit in paragraph *k* does not reshape *k±1*. | [06](06-text.md) §5 | P6 |
| Text-7 | Cross-backend paint: skip-ink, wavy, and text-shadow goldens on every backend with a stated perceptual tolerance. | [06](06-text.md) | P6 |

### 1.11 History (`gpui_history`; subsection pointers are into [01](01-state.md) §10)

| # | Gate | Doc | Ph |
|---|---|---|---|
| H0 | Zero tax: a binary whose owners do not implement `History` has no track-arena symbols in the hot path; counters stay zero under a 10k-click script; `History`'s methods are not in `View`'s vtable. | 01 §10 | P6 |
| H1 | Round-trip oracle: apply/invert/compare-to-checkpoint/redo/compare for every sealed delta, randomized groups; `Exact` failures block CI. | 01 §10.2 | P6 |
| H2 | Serialization: FileStore recover-after-kill restores tagged nodes/cursors/meta; `Silent` in-flight gestures do not resurrect; checksums catch torn writes. | 01 §10.2 (`Store`), §10.5 | P6 |
| H3 | Composer script: type, Cmd-Z (undo type), type, Send, Cmd-Z (undo Send, draft restored), Cmd-Shift-Z (redo Send) — one Edit menu, two lanes, no second stack. | 01 §10.2 | P6 |
| H4 | Selective: rename A, move B, `undo_node(rename)` keeps the move; `undo_node(add-column)` after a move *into* that column conflicts, naming both nodes. | 01 §10.2 (`Track`) | P6 |
| H5 | IME/echo: composition open ⇒ Cmd-Z cancels composition, never applies a document inverse into the mark; undo of a committed edit does not start a new journal node from two-way writeback. | 01 §10.3 | P6 |
| H6 | Canvas scale: 100k strokes, undo last is one `ListDelta`, one damage region; track memory is `O(tagged strokes)`, not `O(samples)`. | 01 §10.2 (`compose`) | P6 |
| H7 | Collaboration origin: remote edits never appear on the local track; `undo` after a remote insert still undoes the local user's last `Point`; transform conflicts surface as `Conflict`, never silent clobber. | 01 §10.5 | P6 |
| H8 | Routing: inspector-focused ⇒ inspector track; document-focused ⇒ document track; chrome-focused ⇒ union/`MainHistory`; `trybuild` proves `history::undo` without an exposed track is a located error. | 01 §10.3 | P6 |
| H9 | Secrets: redacted fields never appear in a snapshot or unencrypted FileStore. | 01 §10.2 (`Store`) | P6 |
| H10 | Panic atomicity: a panic inside `commit(...)` publishes nothing and leaves the DAG unchanged. | 01 §10.2 | P6 |

### 1.12 Tombstones

An ID is never silently dropped or reused; these are the registry's only retirements:

| Old ID | Disposition |
|---|---|
| 19 (entity observation: `on_change`/`Emitter` park-arm-die leak fixture) | Mechanism deleted by passes 16/18 (no subscriptions exist to leak). Its *lifecycle* obligation survives as **R9**, restated over the replacement machinery (delta edges, mount-resolved routes, producer cells). |
| L1 (latency trichotomy) | Renamed **LT1** — the bare `L` prefix is reserved for laws L1–L14, which this document cites by design. |

## 2 · The testing and demo surface

Tests use the same owner as production — `Application`, the real dispatcher, a virtual clock —
never a shadow `TestApp` with its own semantics. Two entry points cover the common and the
general case:

```rust,ignore
#[test]
fn picks_a_suggestion() {
    let search = Win::demo(SearchPage::default());
    search.type_text(search.root().input, "ban");
    search.click(search.root().find);
    assert_eq!(search.root().hits.len(), 3);
}
```

`Win::demo(root) -> Win<V>` constructs a demo `Application`, opens one document window, and
returns the typed handle that owns it. Multi-window tests take the owner explicitly:
`Application::demo()` then `open` each window — both spellings drive the same turn engine.

**Every demo input method is a turn barrier**: it validates and dispatches the input, drains
ready work, settles ports, commits layout/paint/accessibility, and only then returns. Assertions
observe the result with no public `settle`, `run_until_idle`, or arbitrary sleep. A virtual-clock
`advance(duration)` is also a turn barrier; truly external/nondeterministic work is awaited by a
typed event or `app.until(deadline, predicate)`, which reports pending work on timeout.

`Win<V>` exposes semantic test queries directly — `contains_text`, `role`, `focused`,
`bounds(Target<K>)`, `filter_count`, `damage`, and root reads — using the same typed capability
visitor as accessibility (07 §5). `scene()` remains an expert renderer diagnostic, not the price
of asserting ordinary UI. Calling `.run()` on a demo opens the same plan interactively for an
example gallery; embedding uses `app.attach(host_loop)` or `app.poll(deadline)` (09 §7), driving
the same turn engine and never a second scheduler.

## 3 · The inspector contract

Every epoch exposes an inspectable `UiSnapshot`: the retained logical tree, layout, property
slabs, typed routes, action generations, projection dependencies, list deltas, the accessibility
projection, scene packets, damage, queues, producer/request stamps, presentation cohorts
(05 §9), pending/fault provenance, and per-owner/region/layout/producer/surface work attribution.
It never snapshots a half-settled logical or presentation draft (08 §2). The inspector answers:
which source dirtied which sink; which handler consumed or propagated an event; why a target
missed a focus claim; why a result was stale; which boundary retains a cohort; whether an async
declaration waterfall (05 §4) is declared or structural; and how much time/allocation each unit
of work consumed. Snapshot history and detailed traces are opt-in and byte-budgeted — when
disabled, no shadow tree is maintained, and inspector-disabled complexity matches the
non-instrumented path exactly (this is itself a counted gate).

## 4 · Diagnostics as a designed surface

The no-macro path leans on exactly the Rust features that produce the worst default
diagnostics — sealed traits, invariant lifetimes, GAT-bearing bounds — so diagnostics are treated
as part of the sealed surface, not an afterthought:

- every sealed trait ships `#[diagnostic::on_unimplemented]` copy written in this folder's own
  vocabulary ("a property takes a *feed*: pass `&self.count`, not `self.count`") and
  `#[diagnostic::do_not_recommend]` on blanket impls that would otherwise dominate suggestions;
- brand-escape errors name the concept this folder teaches (`Frame`/`Scope`), not an internal
  type;
- a `trybuild` corpus asserts the *message text* of the top predictable mistakes — wrong
  layout-family method, an escaped element, a by-value signal where `&Signal<T>` was meant, a
  captured `&self` in a retained projection, a mismatched route receiver, a read-only source in a
  two-way port — and this corpus **is** gate D1 (§1.1): gate 5 generalized to the whole surface,
  not only `Flex`/`Grid`, with the message text itself under snapshot test.

The bet that ordinary Rust plus good types beats a DSL is won or lost in error messages, and every
gate above that mentions a "located error" or "located diagnostic" is binding this bet to a
checked fixture, not a promise.

## 5 · Performance: what "surpasses" means and how it is proven

Architecture can establish a ceiling and rule out classes of work; it cannot guarantee a speedup
before comparable implementations run on the same machine, renderer, content, and build profile.
Every competitive claim in this folder is bound to that discipline:

- **Verified baselines, pinned to commits.** zgui, Floem, Xilem, Leptos, Haalka/Jonmon, Slint, and
  others are audited against specific pinned revisions, not vibes about "how React-shaped
  frameworks perform."
- **A counter contract.** Every "work avoided" counter is paired with a "work performed" counter
  and a non-vacuity test that drives both paths; live-object gauges are sampled before and after
  churn to reveal leaks. Counters are compiled in for tests/profile builds and compiled out or
  sampled cheaply in release.
- **Reference workloads**, run headless for deterministic work counts and on real GPUs for
  latency/presentation: idle/park, counter locality, hover storm, scroll split, a 100k-row keyed
  table, variable-height virtualization, an editor fixture with a worst-case reflow paragraph, a
  kitchen-sink screen, animation tiers, churn/teardown, cold/warm start, an adversarial dependency
  graph, and — critically — a developer-loop workload (clean/incremental compile time, rust-
  analyzer latency, binary size) so the design is checked against its own promise that plain Rust
  stays plain Rust.
- **"Surpasses" requires all of**: no correctness or feature reduction, no worse asymptotic slope
  on any locality workload, lower p50 **and** p99 input-to-present outside noise, no material
  regression in cold start/steady memory/incremental compile without an explicit accepted trade,
  no hidden work migration from CPU to upload bandwidth or background threads, reproduced on two
  hardware classes before marketing language is used.
- **Failure criteria that stop and redesign**, not tune around: dirty-node discovery walking the
  clean tree; reactive bookkeeping costing more than the avoided work; retained caches growing
  without enforceable budgets; keyed updates remounting resident rows; damage bookkeeping
  collapsing to full-surface work routinely; a compatibility island becoming the dominant
  implementation; a proc macro becoming *required* to obtain the promised complexity.

The point of this contract is to make an ambitious design disprovable early. If a chosen
mechanism fails one of the failure criteria, this folder's laws (00 §4) and this document's gates
are what must survive — the mechanism is replaced, not the invariant.
