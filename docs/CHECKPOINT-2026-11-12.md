# bough round 6: checkpoint, 2026-11-12

Written by the lead after every lane was told to stop and hand off. Nothing has been pushed; every branch named
here is local. Each lane's own `HANDOFF.md` is the detailed record (numbers with commands, every experiment,
every avenue); this file is the map across them, the state of integration, and the list of what to do next.
Numbers are instructions retired on the calling thread unless a unit is given. "Reported" means the lane measured
it and nobody replayed it; "verified by the lead" is said where it applies.

## 1. Summary

* **Integrated (`round6`, head `b6860109e1`):** exact-rect inks end to end, the core lane's coordinate contract
  and prefix fix, the write log and wired seals (`4f4c469ef5`), and the memory lane's `a1873e83fa` (ledger, region
  views, CPU repair backend switched off by default). Tests: `bough` + `bough-suite` with the settle audit on,
  65 passed, 0 failed, on the merge before memory's. Addendum: the whole-workspace run on `b6860109e1`
  (`BOUGH_SETTLE_AUDIT=1 cargo test --release --workspace`, GPU mode) finished after this was written: 242 passed,
  0 failed, one run.
* **Built on lane branches, not integrated:** per-id columns (store), the gapped directory, state planes and
  vector translate (core), exposure-driven mounting (dormant), owner grain, flags and direct wires (surface-api),
  the overflow cut in cephal (layout), reflection as a walk plus the testing API and agent socket (composition),
  row certificates and reworked kernels (gpu). Most of these have never run the comparison/pixel suites.
* **Designed only:** async (a complete study of Solid's mechanisms and a bough design; nothing built), native
  view interleaving (design only), the wgsl-rs port of the new kernels (not started), SDF experiments (not
  started), cross-framework comparison ports (harness only, no competitor measured).
* **The three findings that change the plan:** (1) a Metal frame costs 72-90 K instructions whatever it draws, so
  small repairs must not use the GPU at all; (2) once storage is positional, most of what is left in a signal
  write is damage bookkeeping in the core, which should be resolved once per turn, not per write; (3) the common
  application mode is re-describing, and both async correctness and the idle cost there are weaker than the
  mount-once numbers suggest.

## 2. Branches

| lane | branch | head | in `round6` | tree | last gate the lane ran on its head |
|---|---|---|---|---|---|
| integration | `round6` | `b6860109e1` | n/a | clean | whole workspace, audit on: 242 passed, 0 failed (one run) |
| lead | `lead/core-r6` | `4f4c469ef5` (+ this file) | yes | clean | bough + bough-suite, audit on: 0 failed |
| core | `lane/core/round6` | `2c77f0a1ed` | only `e8f29816ad`, `ffe1b961d5` | clean | `cargo test -p bough-core`; no façade or pixel suite ever |
| store | `lane/store/round6` | `fea9e15453` (code `281f6c3c6d`) | no | clean | workspace check; `-p bough` with audit (15 binaries); comparison suites never |
| layout | `lane/layout/round6` | `83bc9b0f83` (code `76817a1af1`) | no | clean | seals, layout, lay_diff; cephal suite on ilo-prod; pixel suites never |
| surface-api | `lane/surface-api/round6` | `9885d4a04c` | no (this session) | clean | workspace check; own test binaries; full bough suite only on `51d297ffd0` |
| dormant | `lane/memory/dormant` | `31c0d2769f` | no | clean | `--test dormant` (12); suites not re-run after last merge; bare no-tax gate fails (1.07x) |
| composition | `lane/composition/round6` | `8e86055eba` | no | clean except the user's design-session files | probe 15, a11y 6, devtools 2, 25 others; workspace check |
| gpu | `lane/gpu/round6` | `f8e125164c` (gated code `82b83dd072`) | no | clean | heavy/scenes/vlist 31 passed; 4 mutants caught; head not re-gated |
| memory | `lane/memory/round6` | `26db824b83` | `a1873e83fa` yes; WIP `98834fc3bf` no | clean | GPU mode 174/0; CPU mode 152/22 |
| async | `lane/async-entities/round6` | `68f7466f8d` | no (docs only; source == `lead/core-r6`) | clean | none (nothing built); side branch `lane/async-entities/prev-wip` `4812f6afdb` never compiled |
| research | `lane/research/round6` | `8d6f0bea66` | no | clean (`target-cmp/` untracked build dir) | n/a |
| pacing | `lane/pacing/round6` | `344ce4135b` | no (21 commits) | clean | lane's report; crash fix `de79af5` wanted |
| verification | `lane/verification/round6` | `ee86a3b561` | no (2 commits) | clean | bough-suite base failures fixed |
| iteration | `lane/iteration/round6` | `a2d3848ff0` | no | parked WIP, never built | none |
| developer-loop | `lane/developer-loop/round6` | `dc5481c396` | no | parked WIP, never built | none |
| directory | `lane/core/directory` | `842518f85b` | no | WIP, superseded by core's gapped directory | none |
| opus collaborators | `opus/tree` `c20c360a08`, `opus/gpu` `29d93caa8d` | | idea docs only | clean | probes, ungated |
| a11y, generalization, hygiene | | | reports only | clean | |

Remote: `ilo-prod:/root/work/zortax` holds gui-bench (`0fd45107`) and fakeland (`71695e7c`) with all eleven
framework builds; a quick matrix was still running when the runner stopped (how to collect:
`wt/research/bench/r6/zortax/HANDOFF.md`). `/root/work/cmp` has the comparison harness copy;
`/root/work/layout-r6` has cephal test logs. ilo-prod renders with software Vulkan (no 3D GPU).

## 3. Numbers that matter (provenance in brackets)

**Signal path, mount-once, per turn** (`signal_cost`; lead, reproduced by async and comparisons lanes within 0.1 %):

| cell, 256 rows | round start | `round6` | best on a lane branch |
|---|---:|---:|---|
| idle turn | 10,925 | 247 | store plans ~60 (inline guards, not built); surface-api's head regressed it to 356 |
| colour write | 11,097 | 606 | direct wire: 62 above an idle turn, but turn + frame only 644 -> 514 [surface-api, one run] |
| number write | 19,236 | 8,756 | 1,218 with retained labels [store] |
| string write | 31,399 | 20,854 | 15,559 [store]; layout share 38,443 -> 10,625 with the overflow cut [layout, one run] |
| colour write, 1 row | 455 | 640 | **regression from the write log; not fixed** |

**Re-describing mode** (every real application): idle turn at 256 rows 511,585 -> 140,662 on `round6` (still ~550
per row); one colour write per turn 510,970 -> 745 with owner grain [surface-api]; per-row re-description
645/628/230 -> 298/305/122 [store, with the Opus seal experiments].

**Other headline cells:** empty App 13.3 M -> 1.36 M [store; same fix is in `round6` via memory]. Plain
`each(..)` flat from 100 to 30 M rows at 77 records, 27 K per scroll frame, 0 bits per dormant item, breaks above
~33 M rows [dormant]. Front insert at 1e6 records 556,844 -> 4,472 [core, 3 runs]. Whole-page translate 109 -> 32
per page [core]. Element state per row 655 -> 36 B, bytes per row 4,807 -> 3,296 [store]. GPU kernel time: vlist
0.42-0.44, lab stream 0.51, heavy 0.77-0.82 of before; end-to-end milliseconds unchanged; p50 write-to-pixels
460-870 µs at a 4 ms cadence before and after [gpu]. Signal-write frame 3.7 K without render, 107.5 K with [gpu].
Resident: counter 122.5 MiB active, of which 76 is the driver's and 32 is three surfaces [memory, 3 runs].

**Losses on record:** store's mount per row at 6,000 rows 25.0 K -> 42.2 K after merging exact coordinates (fixed
on core's branch `be7e104440`, not integrated); gapped directory at 1e3: removals +7-20 %, freeze-one +20 %,
state-any +26 %; GPU copy slower on small rects, vlist repair up to 1.17 on short lists; surface-api idle turn
302 -> 356; dormant scroll cells +8 %, bare no-tax 1.07x; written-out shader arithmetic +1.2-1.3 % on an
SDF-dense scene.

## 4. Against what the user asked for

| ask | where it stands |
|---|---|
| Damage tracking foundational, so applications never think about virtualisation | Built for plain `each` (dormant lane), not integrated; 2^24 px float ceiling; `List<T>` still has its own window; not yet extended to grids, trees, text |
| bbtree lessons (cache lines over instructions) | Applied in places: gapped directory (positional only inside a block), carved columns, zero dependent loads to the instance. **No lane ever counted cache lines or misses**; several bbtree items unapplied (core avenue 19) |
| Improve on Ripple's propagation | Write log (settle proportional to writes), wired seals, owner grain, direct wires, compile-time ordering designed. Direct damage, derived cells and contained owners not built |
| Solid's async, adapted | Study complete; design sketched; nothing built; two suspected defects in the existing reader |
| Shaders in wgsl-rs on vendored wgpu, every trick, no platform API lost | **Not started.** All new kernel work is hand-written Metal source; the lane lists the wgsl-rs gaps and argues to port after the kernels settle (decision needed, section 9) |
| SDF experiments | None started; three proposed with predictions |
| Interleaved native views and reflection next | Reflection built with testing API, agent socket, replay, a11y projection (not integrated). Native views: design only |
| Bookkeeping that compiles away | Reflection tables deleted; node mirrors removed; list of remaining hand-synchronised side tables in store avenue 8 |
| Memory tricks, bit packing | 655 -> 36 B element state; packing candidates listed (store 7, 11; memory 7); none of the deeper ones built |
| Latency, parallelism, SIMD, GPU | NEON in the core (translate, rank); no x86 twins; no parallel mount/layout/raster built; latency measured for the first time (gpu) |
| Resident memory, binary size, startup, shutdown | Ledger exists; startup 10x; queue release measured (-69 MiB) but not applied; binary-size lane parked unbuilt; shutdown unmeasured |
| Accessibility | Projection and AccessKit export behind a feature; no macOS adapter |
| Composing/overriding external components | Parts re-derived by the walk; flags as the override point proposed; no part-addressing from outside yet |
| zortax benchmarks, other frameworks on the charts | Eleven builds done on ilo-prod; quick run unfinished; no competitor numbers; comparison harness written, unbuilt on Linux |
| Charts page | Version 3 published; not updated since; known data issues (section 8) |
| Overfitting control | Each lane reported losses; generalisation review map exists; several results rest on single runs under load |

## 5. Integration order and known conflicts

Recommended order, each step gated by the full suites (GPU mode) with `BOUGH_SETTLE_AUDIT=1`, read at the `dist`
profile for any 1 % threshold:

1. Finish verifying `round6` `b6860109e1` (workspace run).
2. **core** `lane/core/round6`: vector translate, state planes, gapped directory, far-write fix. Before merging:
   carve `dir_id`/`dir_ink` at 2x and lift the shader's `n_dir` clamp; `gpu_bounds.rs` must walk `dir_len()`;
   build the two façade call sites the lane edited blind; re-run the three-run matrix; first façade and pixel
   suite run this code has ever had.
3. **store** `lane/store/round6`: run the comparison suites first (never run). Conflicts: `Region::heap` mmap
   exists in both store and memory (keep one); `rt.rs`/`elem.rs` with later lanes.
4. **layout** `lane/layout/round6`: branched from an older store commit; rebase; bring `lay_diff.rs` (it found
   two real defects) and adapt it to id arrays.
5. **surface-api**: touches `signal.rs`/`rt.rs` on top of the write log (bit 29 of the head tags a direct head,
   `HEAD_IDX` is 29 bits, `Signal::set` fast path, plain-read recorder in `deref`), `layout.rs` with store.
   Take back its idle-turn regression first. Decide the `Flagged` blanket trait (section 9).
6. **composition**: keep its deletion of `Reflect::types` over store's column; fold `Route.base` into
   surface-api's `OwnerRef`; `nodes_by_handle` becomes the identity on store's index space.
7. **dormant**: needs surface-api's scroll re-description (it cherry-picked it); fix or re-scope the bare no-tax
   gate; integrate after 3-5.
8. **gpu** `82b83dd072` then **memory** WIP: re-apply memory's written-out `f_sd` and fmas on the new kernels;
   add the certificate hook (a CPU repair must zero or write `FCert` for tiles it touches) before the CPU
   backend can be anything but opt-in.
9. **pacing** (21 commits, crash fix `de79af5`), **verification** (2 commits): small, independent.

## 6. Defects and unverified claims to clear

* CPU repair backend: 22 pixel failures, whole-colour differences. Suspects: held/paint-only surfaces assuming a
  GPU frame; CPU-written IOSurface used as a GPU copy source without coherence. One command names the cause:
  `BOUGH_CPU_VERIFY=1 cargo test --release -p bough-scenes --test correctness blink` on memory's WIP.
* Async reader (suspected, untested): torn frames in re-describing mode (run `tests/reader.rs` without
  `.once()`); a pending wire declared before its producer misses the hold (`make_latest` does not move the wire).
* Windows over 4,096 px panic (128-bit tile rows through core damage planes, `FRow.bits`, the tile pass).
* Pre-armed GPU batch dies with a command-buffer timeout about 1 s after the process is suspended (2 of 9 heavy
  runs); repro drafted, unbuilt.
* Dormant: float ceiling at 2^24 px; `DormStats::inexact` counted, never acted on; gui-bench `rows` do not seal.
* `signal_cost` at 64 rows prints an underflowed slope for number and string cells (lead's harness).
* Write log costs +185 at one row.
* Probe coverage: templates, virtual lists, `list_with`/`index_list` rows are unattributed in reflection.
* Layout: the zero-layout write-back hole may exist on template and list-row paths (unchecked).
* Store: hover/opacity/translate cold records, labels leaving the label kernel, tooltips: changed, no dedicated test.
* `tests/far.rs` negative control never confirmed.
* An earlier pacing probe window received real pointer events before it was made mouse-transparent.
* Dashboard: a group titled "gates pass" includes cells whose pixel gate fails; base-versus-round6 points mix
  reported and measured; memory holders sum to 119.7 against 122.5 MiB; `build.py` emits a duplicate heading.

## 7. The avenue catalogue

Every underexplored or exotic avenue the lanes recorded, grouped by mechanism, with the handoff that holds the
detail (mechanism, estimate, risk, cheapest experiment). Abbreviations: C = core, S = store, L = layout,
SA = surface-api, D = dormant, CO = composition, G = gpu, M = memory, A = async, R = comparisons; numbers are the
avenue numbers in that handoff. Estimates are the lanes' predictions, not measurements.

### 7.1 Propagation (the improvement on Ripple)

* **Direct damage** (SA1): the direct entry carries the record's tile span; the write ORs the paint plane itself.
  Colour write + frame 957 -> ~400 at 256 rows. Needs per-page epochs from the core.
* **The paint list** (S5, S-E2; C13): a write appends the record id; damage and the paint log resolve once per
  turn over distinct ids, with independent loads. Colour applier 159 -> ~25 at the write. Hover-in-hover-out in
  one turn costs no damage. These two are rivals; one experiment should pick.
* **Derived cells in declaration order** (SA2): recompute wires run in field order, which is a topological order
  known at compile time; glitch-free with no runtime sort. Diamond test with a run counter.
* **Row input fingerprints** (S-E3, S4): per item, remember the owner bytes its closure read; an idle
  re-description compares bytes and never calls the closure. 298 -> ~25 per row. The soundness of the tracked
  read set is the open question.
* **Seals as fixed-stride page columns** (C6, S4, SA6): an unchanged page of 64 rows is one vector compare; ~12
  per row estimated. Bounded below by the cost of rebuilding the description, which is why fingerprints or
  owners-as-blocks are needed as well.
* **Contained views as owners** (SA7): `part!(self.editor)` proves an offset like `sig!`; a handler re-describes
  only that owner.
* **By-reference `Element::update`** (SA8): removes the move of the description and the quadratic term in depth
  (200 levels 533 K -> ~25 K predicted). ~104 mechanical sites.
* **Turn fixed cost** (S6, SA-next-1): inline guards instead of out-of-line calls that return at once, a
  dirty-region counter instead of a scan; idle 249 -> ~60. A single "nothing happened" bit per turn.
* **Batched list flags as position runs** (SA3, SA5): bitsets by position; a 10,000-row range select 305 K -> ~10 K.
* **Flags in core state planes** (SA4, C5, A12): subtree and range flips become mask writes; "is anything
  selected" and "loading" become summary tests.
* **Variant-driven layout** (SA10); **selection helpers** (SA13); **hover as flags only** (SA12);
  **compile-time proof of no plain reads** (SA11, speculative).
* **Fixed-advance numeric cells as direct glyph stores** (L6, SA-next-2): number write ~800 -> ~150 over idle.

### 7.2 Storage, identity, packing

* **Ids at columns versus side pages** (S12, S-E1): the open design question for the store. One experiment
  decides: translate a 10,000-record subtree after churn, gather through the handle column versus contiguous add
  (predicted ~12 against ~6 per record, both under target). Ids in contiguous blocks per mounted run make the
  gather sequential.
* **Position is identity in the core** (C8): the handle table becomes opt-in; saves 24 B per record and the
  handle fixups on every split.
* **Layout and text state as per-id columns** (S1, S3): bytes per row 3,296 -> ~1,300; allocator calls 14 -> ~4.
* **The 360 B layout answer cache per container** (S2); **sized-leaf size as node data** (S13, L7): stops a new
  interned style per distinct label width.
* **Mount as a bulk column append** (S9): a fixed-shape row's mount image is a memcpy plus an id rebase;
  element part of mount ~9 K -> ~1.5 K.
* **Removal without scopes or reclaim queues** (S10); **hand-synchronised side tables to delete** (S8: `Scope`,
  `DynItem.tape`, `reflect.types`, three HashMaps, duplicated clip and space fields).
* **Bit packing** (S7, S11, M7): per-id hot state to one byte; page `handle` and `payload` sharing a word; inks
  as tile-relative bytes under a page origin; run headers 32 -> 16 B; instances 48 -> 24 B; glyph slots carry
  ~170 bits per drawn glyph against ~8 of information; pens derivable by prefix sum.
* **Masked indices instead of bounds checks** (S16); **NEON `NRect`** (C14, S17): colour applier 159 -> ~110.
* **Derived instead of stored** (M6): the ring's spare surfaces, per-label layout state, CPU atlas copies, three
  font copies.

### 7.3 Core tree and directory

* **Page-relative coordinates as the only form** (C3): deletes the near/far split; scroll independent of
  distance. Cheapest test: force the wide path and run the matrix. With **far translate vectorised** (C4).
* **Summary word per page and block** (C5): any-state at 1e6 250 K -> ~600; freeze-one 490 K -> ~3 K.
* **Block cut without the spike** (C1: block-id indirection or a shader order array; C2: PMA at 75 % fill).
* **Deferred per-page settle** (C13): the geometry write 573 -> under 150.
* **Column plan implementation** (C7); **hot/cold page split by measured access** (C9); **`dir` out of the page
  header into a side array** (C10); **rank without the search** (C17); **removal bookkeeping** (C18).
* **Damage planes for any window size** (C15): design ready; fixes the 4,096 px panic with G14.
* **bbtree items not applied** (C19): last-leaf append cache, template-sized bulk build, type-stable directory
  storage, node plans, interleaved lookups for batches of handle writes.
* **x86-64 twins of every NEON kernel** (C20).

### 7.4 Layout

* **Cheaper flex run for small containers** (L1): 10.3 K -> ~3 K per one-row relayout; cold mount share ~9 K -> ~3 K.
* **Memo by seal, then parametric trace replay** (L2, L3): uniform rows under 500 per row at cold mount.
* **Offsets as blocked prefix sums; moved rows as one `translate_run`** (L4, L5): a height change costs per
  page, not per following row.
* **Scheduler parent/depth from columns** (L12); **parallel subtree layout** (L8: cephal has the executor,
  bough does not enable it); **SIMD prefix sums over uniform runs** (L9, order-of-summation caveat);
  **constraint cache keyed by seal** (L10); **paragraph wrap as width intervals** (L11).
* **Semantics question** (L13): fixed-size boxes defaulting to a zero minimum size (section 9).

### 7.5 Damage as virtualisation

* **Virtual coordinates** (D1): i64 base per part; lifts the ceiling to 100 M+ rows.
* **`List<T>` on dormant runs** (D2): one mechanism; deletes listview's window, height index and per-row spaces.
* **Walk-free realisation** (D3): a row entering costs its wake only.
* **Runs beyond lists** (D4, D7): grids, trees, text documents, spatially indexed canvases, hidden tabs as a run
  of one.
* **Stated extents** (D5); **compressed remembered sizes** (D6); **parallel realisation** (D10);
  **velocity-proportional overscan** (D12); **dormant kind in the core so the cull returns runs** (D11, C16);
  **a11y, find and focus through the source, never the records** (D9).

### 7.6 GPU, shaders, SDF

* **Tile fingerprints** (G5): skip shading and writing a tile whose staged records hash equal; lab stream
  36 -> ~15 µs. Certificates generalise into it (G16).
* **Dense copy plan and a short-list path** (G4): vlist 83 -> ~50 µs; removes both regressions.
* **SDF experiments** (G9): analytic merged coverage per tile row; glyphs as distance bands instead of atlas
  texels; one distance evaluation for rect + border + shadow.
* **Glyph coverage per pixel, not per record** (G8); **per-run row test** (G6); **text as one entry per run** (G7).
* **GPU-side damage** (G10): a changed-tile bitmap written by the tile pass; an oracle for over-damage.
* **Occlusion** (CO9): the last opaque full-tile cover makes earlier records irrelevant to both GPU and damage.
* **wgsl-rs port and its gaps** (G13): `subgroup_min`, count-trailing-zeros, half reinterpretation, byte stores,
  64-bit addresses and resource ids in a buffer, run-time texture selection, fma contraction control.
* **Metal-specific paths to keep reachable** (G12): bindless ids, shared events, residency sets, simdgroup
  reductions, indirect dispatch, IOSurface textures.
* **Atlas dirty-band upload** (G15): 1.14 M -> 36.8 K per new glyph.
* **Heavy stream's lever is over-damage** (G17): 34 % of repaired tiles are unchanged.

### 7.7 The frame path, executors, latency

* **Two executors with a cost model** (G1, M1, M4): CPU repair for small frames; 107 K -> 20-40 K and p50
  470 -> under 50 µs predicted. The model should include wake state, occlusion, thermal and battery.
* **Completion without the event signal** (G3): 109 K -> 91 K per GPU frame.
* **Coalesced frames per encoder** (G2); **phase-split work that can be split** (G11: the CPU work feeding the
  slot, and CPU tiles).
* **Lockstep scrolling with native views** (CO8).

### 7.8 Memory, startup, shutdown, size

* **A process that never submits GPU work** (M2): counter 122.5 -> ~25 MiB.
* **Queue release when calm** (M3): -69 MiB per window; +3.1 ms on the next GPU frame.
* **One reference surface plus residual layers** (M5); **font file mapped once** (M-X3).
* **Steady-state allocations as a gate** (M11): a counting allocator asserted per frame.
* **Startup** (M9): glyph rasterisation and font parsing for strikes the first frame does not draw.
* **Binary size**: lane parked unbuilt (`lane/developer-loop/round6`); CPU executor's code size unmeasured.
* **Kernel time is invisible to instruction counts** (S18): a wall-time mount cell under the measurement lock.

### 7.9 Async

* **Worlds by swapping** (A2): choose the visible world per phase of the turn by swapping held cells with their
  retained values; closes the re-description gap at O(held cells).
* **Pending as one bit; publication through the write log** (A1, A4): start cost independent of dependents;
  zero allocations per landing.
* **Wake-driven admission** (A5): 1,000 pending futures cost an idle turn nothing.
* **Cascades as one publication** (A3); **optimistic writes as reversed staging** (A6); **netting a write back
  to what is shown** (A7); **`.alone()` escape from turn membership** (A8); **streams via `AsyncIterator`** (A9);
  **`Send` only at the worker boundary, executor as a type parameter** (A10); **errors as the future's output**
  (A11); **prefetch as a question without sinks** (A13).
* **Retained == full redraw from published state as the async oracle** (A14), over random schedules.

### 7.10 Reflection, testing, hot reload, inspector, accessibility

* **Finish the zero-cost proof** (CO1): reachable-but-idle build; then `Location` in every builder.
* **Selectors** (CO2): by geometry, by builder type, by source line. **Query algebra for agents** (CO14).
* **Turn logs as files; replay across builds; bisect a pixel diff** (CO3). **State transplant** (CO4).
* **Inspector** (CO5): pick mode, highlight record, style pins. **Stable identity across reloads** (CO15).
* **Incremental accessibility from the write log** (CO6); macOS adapter.
* **Parts' guards folded into route chains** (CO12); **owner-aware routes in embedded views** (CO13).

### 7.11 Native views and platforms

* **`Native<T>` that derefs to the real handle** (CO11); Wayland subsurfaces and DirectComposition visuals as
  the same host contract.
* **Translucent content by splitting the paint order into planes** (CO7); **input and focus handoff** (CO10).

### 7.12 Ideas of the lead's not in any handoff

* **One journal per turn.** The write log, the paint log, state-plane flips, list delta logs, the reflection turn
  log and async publication are six records of "what changed this turn". One append-only journal that settle,
  damage, accessibility deltas, replay, hot reload and publication all read would delete five mechanisms and
  make every consumer proportional to change. Cheapest test: express the paint list (S-E2) and the a11y delta
  (CO6) as readers of the existing write log and count what is left over.
* **The description as data.** A view's static shape (kinds, specs, constant operands) is known at compile time;
  only operand bytes vary. Lowering a view to a constant image plus an operand table would make mount a memcpy
  (S9), a seal the operand bytes (S-E3), direct wires table entries, and the reflection walk a table read. This
  is the common root of four lanes' top avenues and nobody owns it.
* **Queued bits as a plane.** Keep the QUEUED bits of an owner's cells in one bitset beside the owner; settle is
  a count-trailing-zeros loop, in rank order by construction, with no log, no overflow epoch and no sort. Would
  also remove the one-row regression. Compare against the 32-entry log on 1, 16, 256 and 4,096 cells.
* **Animation without turns.** A record carrying (from, to, start, curve) evaluated by the kernel from a time
  uniform costs the CPU nothing per frame; damage is the swept bound, known at start.
* **Scroll in the compositor.** A surface larger than the viewport, moved by a layer transform, makes a scroll
  frame cost no repaint until new content is exposed; dormant runs supply the exposure.
* **A caller for `freeze_into`.** Either a render thread reading a frozen snapshot (latency), async worlds, or
  time-travel in the inspector; or the cells leave the gate (core's objection).
* **Hardware counters.** No lane measured cache misses or branch misses. The portable crates (bough-core,
  cephal, bough-text) can run under `perf` on ilo-prod once source can get there (section 9).
* **Layout of the owner struct.** Signal heads are hot, values are cold; a derive could group heads contiguously
  so a settle touches one line per eight cells.
* **PGO and BOLT** on the turn and frame paths; never tried.
* **A Linux build of bough** on the wgpu-hal path: without it no competitor number can share an axis with ours.
* **Energy** as a metric beside latency: the GPU clocks down, so cycles removed show up as joules, not milliseconds.

## 8. Benchmarks and fairness

* Our instruction counts are "up to the GPU's input" on aarch64; zortax's numbers are whole-process CPU time
  under software rendering on x86-64. They must not share an axis (R1).
* Only favourable shapes are charted for signals; replace (2.9 M at 1,000 rows, 11 M at 5,000), append and a
  100-update ticker must be charted too (R5).
* Binary size and memory comparisons need stripped, feature-equivalent competitor builds (R6).
* Direct wires look 5.7x better without the frame and 1.25x with it: report turn and turn + frame for every
  signal cell (surface-api).
* Gates at the 1 % level are noise at the `release` profile (untouched cells moved up to 4 %, one 24 %); read
  them at `dist`, medians of five (composition). GPU-time slopes are unusable under load (A/A spread 0.63-1.91);
  use saturated benches (memory).
* Headless test drivers exist for floem, freya, gpui, zgui, egui and iced; vizia needs a hand-built one (R).

## 9. Decisions that are the user's

1. **Getting source to ilo-prod**: accept its host key on the Mac, or allow pushing work branches. Blocks
   hardware-counter work and Linux competitor runs.
2. **wgsl-rs timing**: the direction is wgsl-rs now. The GPU lane argues for porting once after fingerprints and
   the copy plan, because runtime-compiled Metal source gives a seconds-long experiment loop. Alternative that
   honours both: give wgsl-rs the same edit-and-run loop first.
3. **CPU/GPU exactness**: dropped as a requirement; the memory and GPU lanes both found it cheap to keep and
   useful (it caught an fma contraction and two ordering mutants), and mixed executors on one surface ring need
   it or a full repaint at every switch. Keep per kind where it is free?
4. **Gate policy**: "every cell within 1.02x" blocks foundational changes on bookkeeping noise (core, dormant).
   Proposal: a weighted set with the signal-path cells first, absolute thresholds for small cells.
5. **Scale target**: with dormant runs, trees stay at 1e3-1e4 records; is 1e6 all-mounted still a target (core)?
6. **`min-size: 0` for fixed-size boxes** (layout): a behaviour change that makes them true layout boundaries.
7. **Scroll offset type** (f64 or i64) for virtual coordinates: an API change.
8. **The `Flagged` blanket trait** claims `.hover`, `.selected`, `.flag`, ... on every type in the prelude.

## 10. Where the lanes say the lead got it wrong

* Too many re-plans: the core lane's queue was reordered eight times in a session; three items started and
  parked. Stopping and restarting the layout lane cost a full context rebuild.
* Merging `lead/core-r6` into lanes mid-refactor cost hours of conflict resolution and builds under load; one
  integration point a day would be cheaper.
* Briefs written from unmeasured decompositions: "bin scanning is 47-64 %" (false), "relayout boundaries
  derived from Spec" (wrong cause: it was overflow propagation), "58 instructions per cached child"
  (misattributed), "under 10 K for a GPU frame" (not reachable by call tuning). An hour of floors first.
* Targets assigned to the wrong owner: store's mount target is mostly layout, write-back and text.
* An Opus experiment was committed to a branch other lanes based on without passing that lane's gates.
* Async treated as a second-wave lane twice, though re-describing correctness is a precondition for any real app.
* Reflection, native views, hot reload and accessibility in one lane left native views unstarted.
* Whole-suite gates at every stage under load cost about an hour each; a small fixed gate set between stages and
  full suites at merge would have doubled what was measured.
* Lanes' own measurement locks were overlapped at least once (memory over core, 15:11).

## 11. What to focus on next, in order

1. **Integrate** in the order of section 5, one lane at a time, suites green at each step. Nothing new is worth
   more than getting columns, the directory, grain/flags/direct wires and reflection onto one branch.
2. **Small frames off the GPU**: fix the 22 CPU-mode failures, add the certificate hook, measure the signal
   frame. This is the largest user-visible number available (107 K -> 20-40 K; p50 0.47 ms -> under 0.05 ms).
3. **Damage once per turn**: run the paint-list-versus-direct-damage experiment, with deferred per-page settle
   and NEON rects in the core. Target: colour write + frame under 150.
4. **Re-describing mode as the headline**: owner grain everywhere, row fingerprints or contained owners, seal
   columns. Target: idle re-description under 30 per row, and every chart shows this mode first.
5. **Async built** on worlds-by-swapping, starting with the two suspected defects and the full-redraw oracle.
6. **Virtualisation finished**: virtual coordinates, `List<T>` on runs, then grids and text.
7. **wgsl-rs port, fingerprints, dense copy plan, SDF experiments**, in whichever order decision 2 gives.
8. **Native views** as their own lane; macOS accessibility adapter.
9. **Fair comparisons**: a Linux build of bough or clearly separated axes; competitor ports; losing cases charted.
10. **The description as data** and **one journal per turn** as design studies by the lead before anyone builds
    more per-mechanism logs.

## 12. Where to read more

`wt/<lane>/docs/lanes/<lane>/HANDOFF.md` for core, store, layout, surface-api, composition, gpu, memory,
async-entities; `wt/dormant/docs/lanes/memory/dormant/HANDOFF.md`; `wt/research/bench/r6/{comparisons,zortax}/HANDOFF.md`.
Design notes: `wt/async/docs/lanes/async-entities/SOLID-ASYNC.md`; `wt/core/docs/lanes/core/COLUMN-PLAN.md`,
`TABLE-R6*.md`; `wt/store/docs/lanes/store/DESIGN.md`; `wt/dormant/docs/lanes/memory/DORMANT.md`;
`wt/composition/docs/lanes/composition/{DESIGN,EXPERIMENTS,REFLECTION-SESSION}.md`;
`wt/memory/docs/lanes/memory/{LEDGER,CPU-REPAIR,EXPERIMENTS}.md`; Opus idea documents
`wt/opus-tree/docs/lanes/core/IDEAS-OPUS.md`, `wt/opus-gpu/docs/lanes/gpu/IDEAS-OPUS.md`. Process and the binding
amendments: `bough/runtime/lead/ROUND6.md` (ignored by git). Dashboard source: `wt/research/bench/r6/dashboard/`.
