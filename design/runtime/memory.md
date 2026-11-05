# Memory: Minimal Footprint and Zero Steady-State Allocation

Status: **runtime contract**, peer to [fastpath.md](fastpath.md) and bound by the same
zero-tax law. The goal is **super-low absolute memory use** — bytes-per-node, bytes-per-cell,
bytes-per-row measured and minimized, one copy of truth everywhere — with zero allocations in
steady state. Budgets and the pressure cascade below are *backstops* that keep worst cases
honest, not the design center: a well-behaved app should sit far under every budget without
anyone tuning anything. The public surface never grows a memory annotation — pools are
runtime-owned, per the ladder ([../07-the-ladder.md](../07-the-ladder.md) §0). The one public
touchpoint is an *app policy*, not a perf hint: `gpui::run(view).memory_budget(..)` (optional
chrome on the App builder), defaulting adaptively from the platform's device class and
memory-pressure signals.

## 0 · Footprint: where the bytes are, and why there are few of them

The architecture is intrinsically low-footprint before any pool tuning, because state and
display each exist **exactly once**:

- **App state lives inline in the owner's struct.** `Signal<T>` is a field — the value plus
  an interned `u32` slot id and intrusive links; no `Rc`, no heap cell, no per-signal
  allocation, no side arena of values (contrast Solid/Leptos-style runtime arenas). `List<T>`
  is one slab. Promotion changes a type, not where the bytes live.
- **Elements are transient; only compact lowered records persist.** Describe output flows
  through reused bump pages, is diffed, and lowers into kind-segregated SoA slabs (a node's
  hot header is a few machine words; cold data is side-tabled). There is no retained tree of
  element *objects*, no closure forest, no per-element `Box` — the "element tree" you'd
  expect to dominate RSS does not exist at rest.
- **Repetition dedupes.** Identical rows share lowered spans and retained pictures
  (fastpath §6); strings intern (`SharedString`); wires are indexes into dense per-window
  arrays, not objects.
- **Big things spill or stream.** History pages spill to the FileStore beyond a small RAM
  window; media keeps encoded bytes and bounded decoded windows; atlases evict.

Per-target bytes-per-node / bytes-per-row / bytes-per-window constants are stored alongside
the perf baselines and regressions are gated (M1) — footprint is a counter, not a vibe.

## 1 · The three allocation classes (the law)

Every allocation in the pipeline must be classifiable as exactly one of:

1. **Steady-state: zero.** A running screen doing what it did last frame — re-describes,
   signal patches, hover, scroll, animation, typing into an existing field — allocates
   nothing. Not "little": zero, asserted by a debug allocator hook per phase.
2. **Novelty: page-granular, counted, amortized.** Structural novelty (first mount of a View
   shape, a promotion installing a wire, a cache miss) allocates from window-level
   **free-listed pages**, touching the OS allocator only when the free list is empty. Every
   novelty allocation is counted with provenance (which phase, which site).
3. **Budgeted caches with a correctness-safe miss path.** Every retained cache (shaping,
   layout memo, tape dedup intern, atlas, media variants, composite layers, history) has a
   byte/entry budget, eviction telemetry, and a miss path that is slower, never wrong.
   (Already a cross-cutting release rule in [../migration.md](../migration.md); this file
   makes it enforceable via the ledger below.)

Anything that fits none of the three classes is a design bug, not a tuning problem.

## 2 · The ledger, the cascade, and giving pages back

**One `MemoryLedger` per window (rolled up per app).** Every pool, arena, slab, and cache
*must* register: name, live bytes, committed bytes, budget, eviction callback. In debug
builds, an allocation that cannot be attributed to a registered pool is an error — the ledger
is *complete by construction*, so the inspector's memory map always sums to the truth and a
leak is attributable the day it appears, not after a heap-profiler session.

**The pressure cascade** (order is normative — cheapest-to-recreate evicts first):

1. speculative work: predictively warmed layers, warmed atlas glyphs, pre-described arms
2. dedup/intern caches (tape span intern, shared pictures)
3. shaping cache and layout memos
4. earned composite layers (demote to retained pictures — never below skip-paint, 07 §6)
5. media variant caches (decoded/scaled variants; encoded bytes kept)
6. arena high-water trim (below)

Pressure sources: the app budget, and platform signals (macOS memory-pressure dispatch
source, Linux PSI, Windows low-memory notifications) — wired once in the platform crates.
Budgets are expressed as fractions of a device-memory class, not absolute bytes, so the same
binary behaves on an 8 GB laptop and a 128 GB workstation.

**High-water trim.** Reset-not-free arenas retain their peak by design (fastpath §5). The
counterweight: on idle ticks, each arena trims its committed pages toward the p95 of its
recent per-frame usage and **decommits** (madvise `MADV_FREE`/`DONTNEED`, `VirtualFree
MEM_DECOMMIT`) so RSS actually falls after a burst — "capped" must be true in the OS's
accounting, not just our ledger's.

**Fragmentation** is bounded by kind-segregated slabs (fixed size classes per node/property
kind), tombstone reuse, and idle-lane compaction; the ledger tracks live/committed ratio per
pool and gate M6 bounds it.

## 3 · Stage by stage

| Stage | Steady state (class 1) | Novelty (class 2) | Budget (class 3) |
|---|---|---|---|
| **Describe / tape** | describe output written into reused bump pages per owner; same-shape output reuses the same pages; read-set diffs in fixed scratch | new View shapes take pages from the free list; **learned size classes**: the first describe of a View *type* records its page footprint, so later mounts of that type pre-size exactly and never realloc-grow | tape dedup intern (LRU bytes) |
| **Text** | port text lowers by `write!` into the port's **retained byte buffer** (see §4); shaping-cache hits; glyph atlas hits; fixed-advance counters skip shaping entirely | new run → shaping cache fill; new glyph → shelf-allocated atlas slot | shaping cache, atlas (shelf + eviction) |
| **Layout** | persistent Taffy nodes patched in place; measurement scratch reused; memo hits | new nodes from the node slab | layout memo table |
| **Scene / GPU** | fragment patches write persistent primitive slots; uploads go through **persistently mapped staging rings**; frame packets from a fixed pool ring (bounded in-flight already) | new primitives take free-listed slots | atlases, pipeline cache, composite layers (VRAM budget) |
| **Input fast lane** | resolver lookups + SoA slot writes + tile bitset — allocation-free by construction (fastpath §1/§3/§4) | resolver rebuild on epoch publish uses double-buffered reused storage | — |
| **Collections** | splices reuse tombstoned slab slots; arrangement outputs are pooled index vectors; virtualization recycles pooled rows (07 §18.7) | list growth is slab-chunked | arrangement caches |
| **Wires / cells** | interned dense slots; subscriber lists are **intrusive links** on the retained records — no per-cell `Vec`, so subscribe/unsubscribe is pointer surgery, zero alloc | first bind interns a slot (counted) | — |
| **Regions / closures** | region closures stored in 3-word inline storage (`size_of == 0` for fn items and capture-free producers — the common case); rerun executes in place | oversized captures spill to a pooled box, **counted** as a `closure_spill` — visible, not silent | — |
| **Effects / tasks** | the dispatcher sink is a fixed ring per dispatch; emit/announce/haptic write records in place; completion routing is slot-indexed | `spawn`/`Task::run` boxes its future **once** from a size-classed future pool | — |
| **History** | coalescing folds in place (oldest inverse + newest value — 08's no-per-keystroke-alloc rule); delta arena pages reused | tagged nodes take small-object arena slots | **RAM-capped with disk spill**: beyond its byte budget, cold history pages spill to the crate's FileStore — undo depth is bounded by disk, not RSS |
| **Media** | resident-set playback reuses plane buffers | decode into pooled buffers | encoded/decoded/variant caches (already budgeted) |
| **Inspector / counters** | release: compiled to no-ops; debug: fixed ring buffers | — | trace ring |

## 4 · The clever bits (what makes zero actually reachable)

- **`Display` into the retained buffer.** The classic leak of "reactive text" is a `String`
  per publish. `.project` is already generic over its output; a projection whose output is
  `impl Display` lowers as a *text wire that formats directly into the port's retained byte
  buffer* (`write!`, then byte-compare against the previous content for the consumer cutoff).
  Steady-state derived text — counters, "n selected", timestamps — allocates nothing and
  still patches precisely. Rung-0 `format!` in a describe stays legal; its allocation lands
  in the describe bump page (freed by page reuse, not the global allocator) — so even the
  floor's `format!` is arena-absorbed, not a heap churn.
- **Learned size classes per View type.** Describe-output shape is near-static per View
  type; recording the footprint on first describe makes every later mount of that type a
  single exact-fit page pull. Mount storms become `O(nodes)` bytes with a small constant and
  zero realloc-growth.
- **Intrusive everything.** Subscriber lists, dirty lists, eviction queues, free lists live
  as intrusive links inside the records they organize (the pattern migration.md Phase 4
  already names for owner/source removal) — no side `Vec<Box<_>>` anywhere on hot paths.
- **Pools own their spill story.** Every "usually inline" optimization (closure SSO, small
  child lists, SmallStr text holes) has a *counted* spill path — the inspector can answer
  "what spilled this session" so an inline-capacity mistake is data, not a mystery.
- **History spills, never grows.** The undo DAG is the one store users expect to grow
  forever; giving it a RAM budget with FileStore spill keeps "unbounded undo" true while
  keeping RSS capped — and Sudden Termination already required the disk path to exist.

## 5 · Verification: the debug allocator and the M-gates

A debug-build global allocator hook tags every allocation with the current pipeline phase
(describe / settle / layout / paint / composite / input-fast-lane / effect) and, for class-2,
the provenance site. That gives allocation the same treatment work already gets: counters
that can't lie, attributed in the inspector.

- **M0 — Steady-state zero.** Idle, hover storm, scroll, animation, typing-in-existing-field,
  and plain-`T` counter workloads run with `allocations == 0` in every phase after warmup
  (subsumes and extends fastpath §5's gate). *Non-vacuity:* first mount shows its class-2
  allocations with correct provenance.
- **M1 — Mount storm.** Opening N panels allocates `O(nodes)` bytes with the stored
  per-target constant; closing them returns live bytes to baseline **and** trim returns RSS
  (OS-measured) within the idle-trim window.
- **M2 — Ledger completeness.** Debug run of the full workload suite produces zero
  unattributed allocations; the inspector memory map sums to the allocator's totals.
- **M3 — Cascade order.** Under synthetic pressure, evictions occur in the normative order,
  never demote below skip-paint, and never evict something whose recreation was then needed
  the same frame (thrash counter bounded).
- **M4 — Soak.** The 24 h churn soak (mount/unmount/scroll/edit loop) holds every ledger
  line flat within noise — the leak gauge from Phase 0, now with attribution.
- **M5 — History cap.** A 10⁶-operation editing session holds history RAM at its budget with
  spill; undo across the spill boundary is correct and its latency printed.
- **M6 — Fragmentation.** live/committed per pool stays above the stored threshold under the
  churn matrix; compaction is idle-lane only.

## 6 · What still allocates, honestly

One boxed future per `spawn`/`Task::run` (pooled, size-classed, counted); class-2 novelty by
definition (first mounts, first binds, cache fills); whatever the OS, GPU driver, and
platform text/IME engines allocate internally (observed via RSS in M1/M4, not controllable);
and debug-only instrumentation. Nothing else — and the ledger is the proof, because in debug
an allocation with no pool to charge is an error.
