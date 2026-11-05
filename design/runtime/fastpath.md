# Fast-Path Proposals: Reaching and Beating zgui Without Complicating the Simple Path

Status: **runtime contract draft**, ranked by leverage. Built on the normative surface in
[../07-the-ladder.md](../07-the-ladder.md), [scheduler.md](scheduler.md),
[../performance.md](../performance.md), [../05-one-register.md](../05-one-register.md) §3–4,
[collections.md](collections.md), and the zgui bar in
[../00-research/06-competitive-update.md](../00-research/06-competitive-update.md). Everything
here is kernel-internal: no proposal adds a public spelling, per the ladder's law.

The zgui numbers we must beat or match, on a ported fixture, same machine/renderer/content:
**~44 µs translation-only scroll frame** (a compositor `pan()`), **~202 µs median hover crossing**
(paint-only style variance of two rows), **~1.02 ms recycle frame**, **zero frames when idle**,
persistent pixels outside damage, no clean-tree traversal.

---

## The governing idea: two loops, one law

The ladder's law is *"a view never changes when you make it faster."* Authors own the four storage
nouns (`T` / `Signal<T>` / `List<T>` / `Entity<T>`) and the *semantic* declarations (`.hover(|s|…)`,
`.pan(Pan::VERTICAL)`, `.transition(spring())`, `scroll_y(&self.offset)`, `on_frame`, `.on_reach`).
The runtime owns every hint. **Every proposal below derives its fast-path behaviour from a
declaration the author already writes for a semantic reason, or from pure runtime measurement.**
None adds a public spelling. That is the zero-DX-cost proof, restated per proposal.

The single biggest structural lever is recognizing that GPUI already has the ingredients for a
**second loop** that zgui does not structurally separate: a *presentation-resident input loop* that
resolves pointer/scroll/hover entirely against the published presentation epoch, on the input/
compositor side, **never entering the Rust turn or the logical epoch**. zgui still settles its
reactivity between queued events and runs its frame machinery; if we can make scroll and hover never
touch logical state, we beat it by architecture, not by constant factors. Proposals 1–2 build that
loop; 3–6 make both loops cheap; 7–10 attack the remaining stages.

```
NORMAL LOOP  (UI thread, turn-based)      FAST LOOP  (input/compositor, epoch-resident)
event ─▶ handler(&mut) ─▶ settle graph    raw pointer/wheel ─▶ resolve vs PresentationResolver
      ─▶ publish LOGICAL epoch                              ─▶ apply precomputed variant/xform delta
      ─▶ style/layout/paint                                 ─▶ damage rects ─▶ recomposite
      ─▶ publish PRESENTATION epoch        (zero graph work, zero Rust, zero logical epoch;
      ─▶ lower ─▶ submit FramePacket        escalate to NORMAL LOOP only for discrete/owner work)
```

## The zero-tax law (requirement, not aspiration)

**No path pays for a mechanism it does not benefit from.** "Zero simple-path DX cost" (no new
spellings) is necessary but not sufficient — the requirement is *runtime* non-imposition.
Every mechanism in this file must be classifiable as exactly one of:

1. **Pay-per-declaration.** Its cost scales with the declarations/promotions actually present
   and is zero in their absence. The `PresentationResolver`'s variant/scroll tables are built
   from `.hover`/`.pan`/wire declarations — an app with none publishes empty tables at ~zero
   cost. Predictive warming triggers only on `.pan`/motion/`on_frame` — a static app never
   evaluates it (§2e's gate: predicted-promotions == 0 on idle/counter workloads). SoA patch
   queues are the *promoted-cell* destination — plain-`T` floor code lowers through the tape
   and never touches them.
2. **Strictly-replacing.** It replaces an existing equal-or-greater cost on the same path,
   never adds a parallel one. Tile damage compaction replaces the rect-list union; the SoA
   scatter replaces pointer-chasing sinks; the layout memo check replaces work Taffy would
   have done. If the replacement is not a measured win on the floor workloads too, it does
   not merge (Part II rule: disprovable early).
3. **Idle-lane-only and preemptible.** Atlas warming and speculative pre-describe run only
   when nothing else wants the core, are cancelled by real input, and their gates assert the
   input lane's p99 is unchanged under speculation (§10e).

**Honest residual costs, enumerated** — the full list of what the floor pays for the
ceiling's existence, so nothing hides:

- One predictable branch per `Signal` deref (the describe-frame pointer check, 07 §1) — paid
  only by code that uses `Signal` at all; plain-`T` code has no deref. Nanoseconds, on a
  pointer that is already hot.
- Rolling span hashes in the tape — already required for diffing (05 §4); the dedup intern
  adds one budgeted table lookup per lowered span. If the lookup doesn't pay for itself on
  *non-repeating* content within noise, §6 fails its own gate.
- Resolver publish work proportional to declared interactive surface (class 1 above).
- Arena high-water memory from reset-not-free reuse (§5) — a memory cost, not a time cost,
  bounded by the shrink-under-pressure policy and the churn-returns-to-baseline gate.
- Counters are compiled to no-ops in release (Part II, Phase 0).

**The gate (floor-tax):** every Track-10 PR in [../pr-plan.md](../pr-plan.md) must include a
before/after run of the *tier-0 workloads* — idle/park, plain-`T` counter, a no-Signal
screen, first-mount — and the numbers must be within measurement noise (or improved). A
fast-path PR that regresses the floor is rejected regardless of how much it wins on the
promoted workloads. This is CI-tracked alongside the competitive gate (pr-plan PR 102), so
the requirement outlives the people who remember it.

Ranked list (leverage = how much of the zgui gap it closes × breadth of workloads × confidence):

1. Input-to-photon fast lane (presentation-resident pointer/scroll/hover)
2. Predictive / warmed layer promotion (gesture-triggered, no `.layer()`)
3. SoA interned patch queue + GPU range application
4. Damage compaction (tile accumulator + interval coalescing)
5. Zero-alloc steady-state turns (reused arenas, steady-state detector)
6. Tape dedup: interned span/lowering cache + SIMD diff
7. Layout fast paths beyond Taffy (leaf-metric, memo, parallel, single-pass seam)
8. Frame pipelining + latency lanes
9. Text shaping cache + speculative atlas warming
10. Speculative pre-describe of hot variant states (lowest confidence)

---

## 1 · Input-to-photon fast lane — presentation-resident pointer/scroll/hover

**Highest leverage. This is where we beat zgui structurally, not on constants.**

### (a) Mechanism

Publish, atomically alongside each interaction/presentation epoch (scheduler.md's second epoch), a
compact immutable **`PresentationResolver`** — derived data only, holding *no graph handle* (the same
constraint scheduler.md puts on `UiSnapshot` and `Snapshot<T>`, so it can be read from another
thread without touching the `!Send` graph). It contains, for the current epoch:

- **Hit index**: a tile-bucketed BVH over hitboxes → `HitEntry { node, hitbox_behavior,
  hover_variant: Option<VariantId>, active_variant, focus_variant, owner_rate_handlers: HandlerMask,
  scrollport: Option<ScrollportId>, pan_axes: Axes }`. `HitboxBehavior::BlockMouseExceptScroll`
  (already in GPUI) is a field, so an occluding overlay can still let the list under it wheel.
- **Variant table**: for each node carrying retained interaction variants (`.hover`/`.active`/
  `.focused`/`.disabled`), the **already-resolved concrete paint delta** to apply on enter and revert
  on exit — a list of `(slot, packed_value)` SoA writes (color/opacity/transform slots, Proposal 3).
  Crucially the variant *cascade is resolved at publish time*, even when the author wrote
  `.hover(|s| s.bg(theme.surface_hover))` or `.bg(self.on.map(|on| …))`: the fast lane replays a
  concrete slot write, never a closure.
- **Scroll table**: per pan-claiming scrollport → `{ transform_slot, content_extent, offset_slot,
  overscroll_policy, on_composite_layer: bool, reach_thresholds: [..] }`.

A dedicated input path (platform input callback / compositor thread) holds a lock-free, epoch-tagged,
double-buffered read handle to the latest `PresentationResolver` (seqlock or atomic-pointer swap;
old epoch pinned until the reader releases, bounded by scheduler.md's per-window pin cap).

Per raw event, once per vsync (coalesce all queued moves to the latest — natural input coalescing):

- **Pointer move → hover crossing.** Resolve (x,y) against the hit index. If the crossing changes
  only hover/active variants **and `owner_rate_handlers` is empty for the crossed nodes**, apply the
  old node's revert delta + new node's enter delta to the retained pictures (Proposal 3 SoA scatter),
  union the two rects into damage (Proposal 4), submit a recomposite. **No turn, no logical epoch, no
  Rust, no restyle.** The soundness key: the ladder already compiles machine-rate events so they
  *cannot* be `fn(&mut Owner)` (05 §4/G1, scheduler "machine-rate work refuses `fn(&mut Owner)`") —
  so the type system already proved the owner does not care about each pointer move. `owner_rate_handlers`
  is therefore only ever non-empty for discrete gestures (click/press) that need escalation.
- **Wheel/scroll in a pan-claiming container.** Update `offset_slot`, clamp to `content_extent`,
  apply overscroll policy. If `on_composite_layer`, emit a pure `transform` update (CALayer.transform /
  DirectComposition) and submit — this is the 44 µs path with *no CPU scene walk*. Else damage the
  exposed band only and replay intersecting retained pictures (Proposal 4). Mint a discrete
  `.on_reach(Edge)` event into the NORMAL-LOOP queue **only** when the offset crosses a threshold with
  hysteresis (already specified §4; never `scroll_y` at pointer rate). A `scroll_y(&self.offset)`
  `SignalRef` consumer patches paint-side via its interned slot — still no turn.
- **Escalation.** Anything that could change describe output (a discrete handler, a structural change,
  a plain-field read) is enqueued as a normal event and settled on the UI thread between frames. The
  fast lane writes only the presentation epoch's reversible shadow slots / range journal
  (scheduler.md already maintains these), so a subsequent logical publish rebases cleanly: the scroll
  offset and hover target live in interned slots that survive the epoch swap.

### (b) Zero simple-path DX cost

The author writes `.hover(|s| s.bg(…))`, `.pan(Pan::VERTICAL)`, `scroll_y(&self.offset)`,
`.on_reach(Edge::End, f)` — all present today, all *semantic* (which axis scrolls; which variant on
hover; where the scroll position lives; a discrete "reached the end" event). The resolver tables are
derived from these. Nothing new to spell; the simple path (plain `T`, `if self.hovered`) simply never
qualifies and falls to the NORMAL LOOP, exactly as today. No author obligation is added at any rung.

### (c) Perf mechanics vs zgui

- **Scroll:** zgui's 44 µs is a compositor pan still threaded through its frame-settle machinery.
  Ours is a transform-slot write + submit with **zero graph work and O(1) in document size** (satisfies
  the existing "translation-only scroll independent of total document size" complexity gate). Expected
  single-digit-to-low-tens µs; ties or beats 44 µs and removes the settle overhead entirely.
- **Hover:** zgui's 202 µs *restyles and repaints* two rows per crossing. We move all style work to
  publish time (the variant delta is precomputed), so a crossing is: hit-test + two SoA slot writes +
  two rect damages + recomposite. Expected well under 202 µs because the per-crossing cost no longer
  contains cascade/restyle — it is amortized once at publish and replayed as concrete writes.

### (d) What could go wrong / honest limits

- **Classification must be conservative.** The fast lane runs only when the epoch *proves* no
  owner-rate dependency. Any ambiguity → escalate to the NORMAL LOOP (correct, slower). Mis-classifying
  toward the fast lane would replay a stale variant — unacceptable — so the default is "escalate unless
  proven local."
- **Epoch swap mid-gesture.** A NORMAL-LOOP publish during a scroll swaps the resolver; in-flight
  offset must rebase. Kept in an interned slot that survives the swap (the design already interns scroll
  to a slot); a structural change under a stationary pointer forces a re-hit-test (already a Hover-storm
  requirement: "stationary-pointer style changes update the hit result correctly").
- **Thread safety.** The resolver must carry no graph handle, retained-node reference, or platform
  object — same rule as `Snapshot`/`UiSnapshot`. If a hover color depends on a value only in the logical
  epoch, it is still resolved to a concrete delta at publish, so the fast lane only plays back concrete
  slot writes.
- **Variant that changes layout** (e.g., hover grows the row) cannot be fast-laned — it must escalate,
  because layout lives in the presentation build, not the composite. Detected: variant delta touches a
  layout-obligation slot → not eligible. This is a real limit, and the counter must show the escalation.

### (e) Gate (CI-provable)

Extend Hover storm (workload 3) and Scrolling split (workload 4): across a pointer-crossing / wheel
trace that changes only variants/offset, assert `logical_epochs_committed == 0`,
`turn_settles == 0`, `dynamic_computed_runs == 0`; assert damage == exactly (old+new row rects) for
hover and (exposed band) for scroll; assert p50 **and** p99 input-to-present below the zgui ported
numbers. **Non-vacuity:** a crossing/scroll that *does* hit a discrete handler or a layout-affecting
variant shows `turn_settles >= 1` — proving the fast lane escalates real work instead of swallowing it.

---

## 2 · Predictive / warmed layer promotion (gesture-triggered)

### (a) Mechanism

The ladder earns skip-composite layers by *measured* cadence-divergence hysteresis under a VRAM budget
(§6) — deliberately no `.layer()`. The gap: earning is reactive, so the first ~N frames of a scroll or
animation pay the un-promoted cost (restyle/repaint) *before* divergence is measured — a latency spike
exactly at gesture start, where the user is most sensitive and where the competitive gate measures p99.

Add **predictive promotion**: warm a composite layer the instant a divergence becomes *inevitable*,
driven by semantic declarations already present, keeping the existing hysteresis+budget as the demotion
governor:

- **Pan claim on gesture-start** (pointer-down / recognizer arm on a `.pan(...)` scrollport) →
  warm-promote the scrollport content to a composite layer before the first wheel delta. A pan is
  definitionally cadence-diverging.
- **Motion activation** (`.transition(spring())`, `spin`, `pulse`, a wire `.eased(...)`) → warm the
  animated element's transform/opacity layer for the animation's duration; demote on settle.
- **Definitionally machine-rate children** (`on_frame`, `video`, `canvas` under a churning shell) →
  warm on mount.

Promotion reason becomes `Predicted(PanStart | MotionStart | OnFrame)`, visible in the inspector next
to `Measured(CadenceDivergence)`. Budget still caps; predicted-but-unconfirmed layers are the first
eviction victims and demote fast (short warm-hold timer) if the divergence never materializes.

### (b) Zero simple-path DX cost

Triggers are `.pan`, `.transition`/motion values, `on_frame`, `video`, `canvas`, gesture-start — all
existing semantic declarations. There is still **no `.layer()`** and no author override (the ladder's
explicit refusal stands — "authors forcing OS layers is how browsers died"). "Why did/didn't it warm"
remains an inspector row, not an API.

### (c) Perf mechanics vs zgui

zgui's 44 µs assumes the pan layer already exists (it retains). Warming makes **our first scroll frame
and first animation frame also pure transform/composite**, not a repaint — so we match zgui from frame 1
instead of frame ~3. The measurable win is p99 at gesture start (the competitive gate's tail), and it
lets Proposal 1's fast lane apply immediately rather than after divergence is earned.

### (d) What could go wrong / honest limits

- **Thrash** on flicky gestures (warm→demote→warm): bounded by hysteresis + a warm-hold timer; counted.
- **VRAM pressure** when many elements animate at once: the global budget already caps this; predicted
  promotions rank below measured ones for eviction, so a real long-lived diverging layer is never
  starved by a speculative one.
- **A predicted layer that never diverges** wasted a promotion — counted as a false-warm, and the
  hysteresis learns (repeated false-warms on a site raise its threshold).

### (e) Gate

Animation tiers (workload 9) + Scrolling split: assert the *first* scroll/animation frame does zero
restyle/relayout/repaint (transform/composite only) with promotion reason `Predicted(*)`. Assert
warm→demote thrash counter stays bounded under a flick-cancel fuzz. Assert warmed layers never exceed
the VRAM cap and eviction prefers predicted-unconfirmed. **Non-vacuity:** a static element that never
animates is never warmed (predicted-promotions == 0 on the idle/counter workloads).

---

## 3 · SoA interned patch queue + GPU range application

### (a) Mechanism

Intern-on-wire already gives every cell a dense per-window slot and turns patches into array indexes.
Lay out the retained presentation state as **Structure-of-Arrays by patch kind × phase**, indexed by
slot: `paint_color: Vec<PackedColor>`, `transform: Vec<Affine>`, `opacity: Vec<f32>`,
`text_hole: Vec<SharedString>`, each slot carrying its destination GPU primitive range and its
obligation class (paint / composite / layout).

A turn's port sinks do **not** chase pointers through a retained tree. They append into per-kind
`PatchQueue { slots: Vec<u32>, values: Vec<V> }` (SoA, reused arenas — Proposal 5). At settle end each
queue drains in one pass: radix/SIMD sort+dedup by slot (small u32 keys), scatter into the SoA arrays,
then compute the minimal set of dirty GPU byte-ranges by coalescing contiguous touched-slot runs into
one `write_buffer`. Primitive instance buffers are laid out so a color/transform slot maps to a fixed
offset in a persistent GPU buffer (performance.md already counts "stable primitive-slot writes/reuses/
retirements" and "dirty/upload byte ranges") — a patch is a sub-range upload, never a re-encode.

### (b) Zero simple-path DX cost

Entirely internal to how `Signal` / port sinks lower. The author writes `Signal`, `.bg(&self.tint)`,
`text(&self.count)`. The floor (`T`) that re-describes still lowers through the tape; SoA is the
promoted-cell destination. No spelling, no obligation.

### (c) Perf mechanics vs zgui

zgui has persistent renderer buffers and range-dirty already. We match that and add: SoA cache locality
(vectorized scatter, no tree walk — a patch is O(1) index), zero-alloc reused queues, and coalesced
sub-range uploads. For the tiny patch sets of hover/scroll this is a pure constant-factor win that keeps
the fast lane cheap; for the 10k-consumer palette swap (workload 17) it turns N pointer-chases into one
sorted scatter + a few coalesced uploads.

### (d) What could go wrong / honest limits

- **SoA fragmentation** as slots churn (mount/unmount) — needs tombstone + bounded compaction off the
  hot path (scheduler.md already has this concept for observer slabs).
- **Over-upload** if touched slots are scattered (coalescing wins nothing) → measure upload bytes vs
  slots touched; fall back to per-slot writes when coalescing does not reduce bytes.
- **Value size**: `text_hole: SharedString` is a pointer, fine; a large inline value kind would bloat
  the SoA — keep only small POD in SoA, indirect the rest.

### (e) Gate

Property-lowering matrix (15) + Specialized bulk paths (17): assert zero allocations after warmup;
`dirty/upload bytes` sublinear in sinks via coalescing on the 1→1000-sink fanout sweep; stable
primitive-slot reuse (not churn) on repeated patches. **Non-vacuity:** a scattered-slot workload
exercises and reports the per-slot fallback.

---

## 4 · Damage compaction (tile accumulator + interval coalescing)

### (a) Mechanism

Per-window **tile grid** (e.g. 64×64 px) as a damage bitset, plus a small exact-rect list for sub-tile
precision. Each patch marks the tiles it touches. At frame end, union tiles into a bounded set of
scissor/upload rects (merge adjacent tiles row-wise then rect-wise). Above a density threshold, promote
to full-surface (performance.md already counts "full-damage promotions"). For each damaged tile, replay
only the retained paint pictures intersecting it via the order-tree/BVH query — persistent pixels
outside damage survive (zgui parity, "composed target whose pixels outside damage survive").

### (b) Zero simple-path DX cost

Internal. Damage is a runtime hint the author never touches.

### (c) Perf mechanics vs zgui

zgui already tracks a damage set and replays unchanged paint records. Parity, plus: the tile bitset
keeps damage-union O(tiles) regardless of patch count, and interval coalescing bounds the scissor/upload
count. Directly satisfies the "damages only the exposed band" (scroll) and "old/new row interaction
styles/fragments/damage" (hover) complexity gates that Proposal 1 depends on.

### (d) What could go wrong / honest limits

- **Tile size tuning** (too small → many rects; too big → over-damage). Measured; make it a per-window
  adaptive constant, not an author knob.
- **Checkerboard / scattered damage** → density promotion to full-surface, counted, never an artifact.
- **Stacked translucent overlap**: a damaged tile under many translucent layers replays all of them
  ("overlap contributors replayed" counter) — bounded, and a candidate for an earned composite layer
  (Proposal 2).

### (e) Gate

Damage rectangles / damaged pixels counters on Hover storm and Scrolling split match old+new (hover) /
exposed band (scroll); full-damage promotion fires only above the density threshold; a checkerboard fuzz
keeps damaged-pixels bounded and never drops to silent full-surface below threshold. "Damage promotion is
conservative: artifacts are never accepted as a performance trade" (existing correctness gate).

---

## 5 · Zero-alloc steady-state turns (reused arenas + steady-state detector)

### (a) Mechanism

Make every per-turn structure a **reused arena that resets rather than frees**: the sparse `LogicalDraft`
map, the `PresentationDraft` shadow slots, the SoA `PatchQueue`s (Proposal 3), the damage tiles
(Proposal 4), and the describe bump-pages (05 §4). A turn bumps offsets and clears generationally; slabs
(wire subscribers, owner records) are grow-only with tombstone compaction off the hot path. Add a
**steady-state detector**: after warmup (a few frames), a repeating interaction (hover storm / scroll /
animation) is recognized and the loop runs allocation-free. A debug allocator hook counts allocations
during a fast-lane frame or a steady turn and asserts zero.

### (b) Zero simple-path DX cost

Internal. Arena reuse is invisible; correctness rides the epoch generation already in scheduler.md (a
reused buffer is generationally invalidated, so stale reads are impossible).

### (c) Perf mechanics vs zgui

This is primarily a **p99 / jitter** win: it removes allocator variance from the tail, which is what
lets Proposals 1–4 hit *both* p50 and p99 below zgui (the competitive gate requires both). zgui's strong
locality implies it already avoids per-frame allocation; matching that is table stakes for the tail.

### (d) What could go wrong / honest limits

- **Peak memory retention**: a reused arena keeps its high-water size. Add a shrink policy under memory
  pressure / after churn (workload 10 requires gauges return to baseline).
- **Correctness of reuse**: a partially-cleared buffer must be generationally fenced; never trust
  leftover bytes. This is the same discipline as the two-epoch swap.

### (e) Gate

Property/counter/hover/scroll workloads assert `allocations == 0` in steady state after warmup. Idle/park
(workload 1) already requires zero. Churn/teardown (workload 10): live gauges and arena high-water return
to a statistically stable baseline. **Non-vacuity:** a genuinely new structure (first mount of a screen)
shows the allocations, proving the counter is live.

---

## 6 · Tape dedup: interned span/lowering cache + SIMD diff

### (a) Mechanism

The tape diff already carries 128-bit rolling content hashes per span keyed by `#[track_caller]` +
`.key(...)` (05 §4). Add a window-level **intern table: span content-hash → `LoweredSpan`**
(canonicalized style, shaped runs, paint picture). When two spans hash-equal — across sibling rows *or*
across reruns — the second reuses the first's lowered result after a **mandatory memcmp confirm** (never
trust the hash alone). 10k identical list rows lower once; repeated chrome lowers once.
**Hasher mandate** ([datastructures.md](datastructures.md) Flag B): this intern table — and every
hot map keyed by an already-computed hash, an interned slot, or a `TypeId` (the handler router) —
must use an identity or Fx-class hasher, never std's default SipHash; the lab measured SipHash
re-hashing dominating the intern probe (~116 ns/span), and the memcmp confirm, not the hasher, is
the collision defense. This is
common-subexpression elimination for the presentation pipeline. Identical subtrees can further share a
single retained picture referenced by multiple placements (transform-instanced), feeding the composite
path. SIMD the hash over bump-page opcode bytes and the confirm memcmp (vectorized compare, radix bucket
by hash prefix).

### (b) Zero simple-path DX cost

Internal. The author's `List` rows, `.children(&vec)`, or repeated helper chrome just get deduped. No
spelling; `.key()` stays deleted (identity is slots / embedding site).

### (c) Perf mechanics vs zgui

zgui lowers per fragment. Dedup collapses the describe→lower cost of repeated structure to one — a
structural win zgui does not advertise — directly attacking the 10k-static-siblings counter workload (2)
and kitchen-sink. SIMD makes the diff itself proportional-and-fast over description bytes.

### (d) What could go wrong / honest limits

- **Hash collision** must be caught by the memcmp confirm before any reuse — this is a correctness
  invariant, fuzzed by forcing collisions.
- **Intern-table memory**: LRU-budgeted; a churny UI evicts. Counted.
- **Shared-picture immutability**: a per-instance difference (transform, hover variant) must be an
  overlay on the shared picture, never a mutation of it — else two instances corrupt each other.

### (e) Gate

10k-static-sibling (2) and 100k-table (5): `spans_lowered` ≪ `spans_described`. Collision fuzz forces a
hash collision and asserts memcmp rejects the false reuse (renders correctly). Shared-picture
immutability oracle: two instances with different transforms/variants render independently. Intern table
respects its byte budget under churn.

---

## 7 · Layout fast paths beyond Taffy (leaf-metric, memo, parallel, single-pass seam)

### (a) Mechanism

Persistent Taffy stays (03 §2.4). Add pre-Taffy short-circuits, in order of cheapness:

1. **Leaf-metric-only change** (the counter workload): a text label whose new shaped run has the *same
   advance* as the cached one (fixed digit width / tabular nums / same glyphs) marks paint-only and
   never enters layout. Detected by comparing shaped advances.
2. **Translation-only**: no layout, transform slot only (Proposal 1).
3. **Layout memo**: cache each Taffy subtree's result keyed by `(style_hash, incoming_constraint)`.
   Unchanged style + same constraint → reuse cached size, skip re-solve. This *is* the "layout visits
   the dependency closure, not the whole window" promise made in performance.md, made concrete.
4. **Parallel / SIMD intrinsic sizing** for wide independent sibling rows, on the idle lane or a worker
   via the immutable stage-snapshot mechanism scheduler.md already defines for pure stages ("Framework-
   owned pure stages may lower a large dirty batch into an immutable owned stage snapshot and use
   workers above an adaptive threshold").
5. **Layout-adapter seam**: if persistent Taffy still loses to Rosin's published single-pass number on
   the 10k-node fixture (L8), swap in a single-pass constraint solver for the flexbox subset *behind the
   seam* — no public-vocabulary change, exactly the escape performance.md already reserves.

### (b) Zero simple-path DX cost

Internal; authors write the same `.w/.h/.gap/.flex` style chain. The memo, the leaf short-circuit, and
the engine swap are all invisible.

### (c) Perf mechanics vs zgui / Rosin

Matches zgui's "no clean-tree traversal" via the memo (dependency-closure relayout). Attacks Rosin's
~2.3× on 10k nodes via the single-pass seam if Taffy can't match it. The leaf short-circuit makes the
counter workload's layout cost literally zero when digit width is fixed (an existing complexity gate:
"leaves layout zero when digit width is fixed").

### (d) What could go wrong / honest limits

- **Memo invalidation correctness**: `style_hash` must include *everything* affecting size (font, text
  content, wrap width, constraints). A missed input → stale layout. Validated against the from-scratch
  relayout oracle scheduler.md already requires.
- **Intrinsic-size coupling**: a child's intrinsic size feeds the parent; naive per-node memo is wrong.
  Key the memo on the *resolved constraint closure*, not just local style.
- **Parallel join overhead** on small trees → threshold; only parallelize above measured node counts.
- **Single-pass solver** is a real engineering project and may not cover the full flexbox/grid feature
  set — the seam must fall back to Taffy for unsupported subtrees.

### (e) Gate

nodes-visited / nodes-measured slopes (perf table) stay in the dependency closure. Counter workload (2)
shows layout == 0 for fixed-digit. Rosin 10k fixture (L8) is beaten/matched or the seam is used. The
from-scratch relayout oracle validates every memo hit. Kitchen-sink (8) catches any accidental root walk
(a counter already exists: "accidental root walks").

---

## 8 · Frame pipelining + latency lanes

### (a) Mechanism

Two-stage pipeline over the existing double-buffered two-epoch model: while the GPU renders
`FramePacket N` (fence not retired), the CPU may describe/settle/lower `N+1` into a separate
presentation draft. Throughput roughly doubles for sustained-animation workloads. The **latency lane**
is Proposal 1's fast loop: a scroll/hover patch bypasses the pipeline and recomposites the *current*
frame instead of queuing behind `N+1`, driven by the existing `Input` priority lane. Deadline scheduling
from the injected clock (already present for media/animation) decides when `N+1` must be ready; if the
CPU can't make it, drop `N+1` (skip reason counted) rather than block input.

### (b) Zero simple-path DX cost

Internal. Pipelining and lane assignment are scheduler policy; the author's handlers and views are
unchanged.

### (c) Perf mechanics vs zgui

zgui parks when idle and settles between events. Pipelining is a *throughput* add for sustained
animation; the latency lane keeps input-to-photon low even under animation load — precisely the "p99
input-to-present latency under saturated idle/animation work" the explicit-priority-lanes gate measures.

### (d) What could go wrong / honest limits

- **Pipeline coherence**: `N+1` must not observe `N`'s in-flight GPU resources — the fence-owned lease
  rule already prevents this ("Frame packets own referenced GPU resources until fences retire").
- **Wasted work**: `N+1` invalidated by input that arrives after it started. Bound by only pipelining
  when no input is pending, and by keeping `N+1` cheap to discard (reversible presentation draft).
- **Complexity**: two in-flight presentation drafts stress the pin cap; scheduler.md's per-window pin
  cap and escalation apply.

### (e) Gate

Animation tiers (9) + a saturated-idle workload: p99 input-to-present under sustained animation stays
below one frame; frames skipped are counted with reason; the two-epoch fault oracle proves no torn frame
between pipelined stages. **Non-vacuity:** disabling the pipeline regresses animation throughput but not
input latency (proving the lane split).

---

## 9 · Text shaping cache + speculative atlas warming

### (a) Mechanism

GPUI already has sum-tree text and retains shaped paragraphs. Add a **shaping cache** keyed by
`(run_bytes_hash, font/style/features/direction, wrap_width)` → shaped glyph run (glyph ids, advances,
positions), so an edit reshapes only the true dependency range (already promised) and repeated labels
hit the cache. **Glyph atlas warming**: on font load / first paint, speculatively rasterize the ASCII +
recently-used glyph set on the idle lane so the first real paint doesn't stall on rasterization
(cold-start workload 11). **Digit-width canonicalization**: tabular-nums labels get a fixed advance so a
counter increment is paint-only (feeds Proposal 7.1).

### (b) Zero simple-path DX cost

Internal; `.font(Family::…)`, `text(...)`, tabular-nums style are unchanged. The cache and warming are
invisible.

### (c) Perf mechanics vs zgui

zgui retains shaped paragraphs (parity). The cross-label cache reuse + atlas warming reduce cold-start
and first-glyph latency (an explicit workload zgui's own numbers don't isolate), and the fixed-advance
path makes counter/label updates paint-only.

### (d) What could go wrong / honest limits

- **Cache key completeness**: must include locale/features/direction or it returns wrong shaping. Bounded
  by an LRU budget.
- **Wasted warming**: atlas warming rasterizes glyphs that may go unused — bounded to a small predicted
  set (ASCII + MRU), counted.
- **Sum-tree interaction**: the cache must invalidate on the true edit range, not the whole document
  (the existing "one paragraph cannot hide a document reshape" counter guards this).

### (e) Gate

Editor workload (7): shaped-bytes / lines-rebroken counters stay in the edit's dependency range;
cache-hit rate on repeated labels. Cold/warm start (11): first-glyph latency improves with warming, with
warming work idle-only. Counter workload (2): layout == 0 with tabular nums.

---

## 10 · Speculative pre-describe of hot variant states (lowest confidence)

### (a) Mechanism

During idle-lane time, pre-populate the presentation deltas for likely-next interaction states: ensure
the hover/active/focus variant table (Proposal 1) is filled for the node under a *stationary* pointer,
and optionally pre-describe the *other* arm of a `.switch`/`.when`/`.open` region so a toggle is instant.
Strictly speculative: never affects correctness, discarded on the next real turn, budgeted to idle time,
cancelled by real input. This mostly *guarantees* Proposal 1's table is warm before the first crossing.

### (b) Zero simple-path DX cost

Internal; keys off the same `.hover`/`.switch`/`.when`/`.open` declarations. No spelling.

### (c) Perf mechanics vs zgui

Shaves the *first* crossing / first toggle cost; marginal beyond Proposal 1's amortization. Lower
confidence because pre-describing arbitrary arms can be wasteful when the toggle never happens.

### (d) What could go wrong / honest limits

- **Wasted idle work / atlas pollution** for states that never occur — hard-budget to idle, cap the atlas
  warming, and never let speculation delay input (Input lane strictly preempts).
- **Cancellation correctness**: a real turn must discard speculative drafts cleanly (reversible draft;
  same machinery as Proposal 8's `N+1`).

### (e) Gate

Assert speculative work is idle-only and never delays input (priority-lane p99 unaffected under
speculation); assert it is discarded correctly by the next real turn (two-epoch oracle); assert toggling
a pre-described `.switch` shows reduced first-frame latency. **Non-vacuity:** under input saturation,
speculative pre-describe count drops to zero (proving preemption).

---

## Cross-cutting: how these compose, and the honest overall risk

Proposals 1+2 create and warm the fast loop; 3+4+5 make every patch/damage/turn cheap and jitter-free;
6+7+9 collapse the describe/lower/layout/shape stages for repeated and static content; 8 pipelines
throughput while the latency lane protects input; 10 warms the first interaction. The zgui-specific
targets map cleanly: **44 µs scroll → 1 (transform slot) + 2 (warmed layer) + 4 (exposed-band damage)**;
**202 µs hover → 1 (precomputed variant replay) + 3 (SoA writes) + 4 (two-rect damage) + 5 (no alloc)**.

The single load-bearing risk under everything is **fast-lane classification soundness**: the entire
input loop is correct only because the ladder already forbids machine-rate events from reaching
`fn(&mut Owner)` and already resolves variants/cascade at publish time. If any pointer-rate input can
reach owner Rust, or any variant can change layout, the epoch must escalate that case to the normal
loop. The design must therefore be *conservative by default* (escalate unless proven epoch-local) and
*counter-backed* (every escalation is measured, so a regression that silently swallows a real event
fails the non-vacuity half of the gate). That is the difference between "we beat zgui" and "we shipped a
stale-frame bug that looks fast."

Every proposal above is disprovable early per performance.md §6: if fast-lane bookkeeping costs more
than the avoided settle on common crossings, if warmed layers thrash, if the layout memo walks the clean
tree, or if damage collapses to full-surface routinely — stop and replace the mechanism, keeping the
locality/UX goals. None adds a fifth noun or a second authoring language; all hints stay runtime-owned.
