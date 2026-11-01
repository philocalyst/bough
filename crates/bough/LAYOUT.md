# bough layout: cephal in the façade, a measured variable-height list, zgx's vlist and vscroll-variable

Branch `layout` (worktree `bough-layout`). Apple M3 Pro, macOS 15.7, 120 Hz, `nightly-2026-10-17`, `dist` profile for every
number below (fat LTO, cgu 1). zgx = the 06debe9d `dist` binaries (`micro_bench`, `vlist_workload`) plus a headless vlist
driver built from the *same* tree/profile (see 5). The machine was loaded by other builds (load average 30-80), so
**instruction counts lead and CPU times are context**; nothing is taken from zgx's published tables.

```
export CARGO_TARGET_DIR=$PWD/target RUSTC_WRAPPER= PATH=~/.local/share/cargo/bin:$PATH
export ZGX_FONT_FILE=/Users/mileswirht/projects/_ref/zgx/comparisons/fonts/DejaVuSans.ttf
cargo test --release -p bough -p bough-vlist -p bough-heavy              # layout, text, vlist pixel/anchor/cold-layout gates (+ heavy regression)
cargo build --profile dist -p bough-vlist                                  # mirror (Variant A);  --features deep / deep-kids: Variant B
ZGX_MODE=scroll|fling|jump|probe RENDER=0|1 TICKS=900 target/dist/vlist_headless   # PHASES=1, PROBE_SPREAD=2000
ZGX_MODE=jump ZGX_SECONDS=10 target/dist/vlist_workload                    # windowed; prints workload_ticks=N on stderr
SCENE=vscroll-variable RENDER=0|1 target/dist/micro_bench                  # zgx's JSON shape
python3 bench/layout/slope.py bough|bough-deep|bough-deepkids|zgx 0|1      # slope method;  bench/layout/windowed.py <fw> <mode>
target/dist/layout_bench                                                   # NODES=100000: cold mount and edit costs at tree scale
```

## 1. What was built

| file | role |
|---|---|
| `crates/bough/src/layout.rs` | builders (`column`, `row`, `block`, `para`, `label`; `Styled`: `w h w_pct min_w pad p px py margin gap grow shrink items align_self justify abs relative clip_overflow`), `Spec` -> cephal `Style` in **device px**, `LeafCtx` (measured text), the group bridge (`Lay`: compute / write-back), the layout-path profiler |
| `src/lay_mirror.rs` | **Variant A**: bough's layout nodes mirrored into cephal's own `Tree` |
| `src/lay_deep.rs` | **Variant B**: cephal's `LayoutTree` / `CacheAccess` / `Incremental` over bough's storage (cargo feature `deep`; `deep-kids` adds a child-id cache) |
| `src/mlist.rs` | the measured variable-height list: Fenwick prefix index, anchored mount/measure/place loop, `measured_list(..)` element |
| `src/rt.rs`, `app.rs`, `text.rs`, `signal.rs`, `elem.rs` (additive hooks) | `Node::lnode`, `Scope::lroots`, `NK_LTEXT`, `Rt::lay`/`mregions`, `lay_flush` after the text phase, `TextSys::shape_run`, `Signal::set_silent`; **instance pool is now a buddy allocator, 32 K instances** (see 6) |
| `comparisons/bough-vlist` | the vlist scene (`VList`, `chat_row`), `vlist_workload` (windowed), `vlist_headless`, `vlist_shot`, `micro_bench` (vscroll-variable), `layout_bench`, `cephal_raw`, `text_bench`, `tests/correctness.rs` |
| `bench/layout/` | `slope.py`, `windowed.py` |
| `crates/bough-text/src/shape.rs` | one word: `Shaper::dense_tables` is `pub` (was `pub(crate)`); nothing else in the frozen crates changed |

cephal is used **unmodified**, by path (`/Users/mileswirht/Downloads/ilo-prod-work/cephal`), `flex` + `block` features. Nothing is vendored.

## 2. Design

**Units.** Layout runs in device pixels: every length is `points * scale` when a builder is lowered, so cephal's sizes, the shaper's
metrics (font units * `size_px / upem`) and bough's instances share one unit; rounding is off (positions may be fractional; text baselines
are rounded as before).

**Lowering.** A builder tree is mounted as before (`Element::mount`), now creating *layout nodes* beside the records: a `column` / `row` /
`block` / `para` / `label` adds a node (style from its `Spec`, shared per distinct spec) and, if painted (`bg`, `rounded`, `border`; text
always), a quad / text record with a handle, a pool instance run and a `Node` (so `ApplyBg` / `ApplyText` wires work unchanged). Records get
**provisional empty ink**. The outermost layout box of a mount is a **group**: one independent layout tree (the window's root, or one row of
a list), with its own available space (window size; or viewport width x max-content for rows) and its own space (instances are group-local px;
the space adds the origin).

**Compute, then write back.** `lay_compute_pending` lays each queued group out (cephal, incremental: a text edit calls `invalidate` on its
leaf; a cold group has empty caches). `lay_writeback` then writes results through bough-core's write path: a quad's `xy/wh` go into its
instance and `set_ink_handle` (old and new support to damage: exact repaint); a text node is re-wrapped and its instances regenerated
straight into its pool run. The write-back starts from exactly the nodes the engine reports as changed (plus the root of a never-written
group), computes each one's absolute position as the sum of the rects up its parent chain, and descends into children only if the node moved:
**an edit never scans its siblings**.

**Measured text.** A `LeafCtx` is cephal's measure callback state: the text is shaped once (NEON table shaper; rustybuzz if a GSUB/GPOS
trigger or non-ASCII byte needs it) into a `GlyphRun`; for paragraphs where glyph i = byte i, a **word table** (start byte, `x0`, `x1`
per word, found 8 bytes at a time with a SWAR zero-byte mask) turns `wrap_greedy` into a loop over ~65 words (606 instr vs 5,320 for a
338-byte paragraph, identical line starts and widths: `word_table_pipeline_equals_glyph_run_pipeline`, 12 K random (string, width,
size) cases incl. the instances). Max-content measures never wrap; min-content = longest word; one wrap result is cached per width. Glyph instances are
written by the glyph kernel **directly into the node's pool run** (a `Vec` view of the run; capacity >= glyph count, so it never grows) and
then stamped (space, clip) and bounded (ink) in one NEON pass (10.3 K -> 3.1 K instructions per row).

**The measured list** (`mlist.rs`). Rows are keyed by index; each mounted row is its own group in its own space. A **prefix index**
(Fenwick tree over device-px heights: `prefix(k)`, `row_at(y)`, point update, all O(log n); 100 k rows = 0.8 MB) gives row tops; unmeasured
rows count as the three-line estimate (106 pt, zgx's `ESTIMATE`). A scroll runs: *anchor* (first visible row + offset in it) -> range
(visible +- 2 rows, zgx's overscan) -> unmount leaving / mount entering rows -> lay the fresh rows out and take their heights -> if any
differ from their estimate, update the index and **re-derive the offset from the anchor**, repeat until the range is stable (1-3 passes) ->
place: one space write per mounted row and **one `translate_run` over the survivors' contiguous ink** (instances of surviving rows are never
rewritten). A **jump** is the same loop: the target is placed from estimates, the rows around it are mounted and measured *inside the same
frame*, the anchor keeps the target row at the top (zgx's `scroll_to_row` semantics, the list's own anchoring instead of zgx's
feedback passes). The list may move the offset itself while anchoring; the driver mirrors it back with `Signal::set_silent`.

## 3. The two cephal integrations

Same elements, same engine algorithms, same tests (hand-computed flex geometry, incremental == cold == fresh mount, vlist pixel gates): both pass everything.

| | **A: mirror** (`lay_mirror.rs`) | **B: deep** (`lay_deep.rs`, feature `deep`) |
|---|---|---|
| nodes | a cephal `Tree` node per layout node (id = slot) | **no second tree**: a layout node is a Bough record (paint-less boxes get an empty-ink `REC_BOX`), cephal id = handle index |
| structure | cephal's sibling links | `Incremental::parent/depth` = `Bough::parent/depth_at` (depth column); `children` = `Bough::children` page-skipping depth scans (`deep-kids`: ids cached in the side column on first use) |
| styles | `new_leaf(style)` interns (hash of the 300-byte `Style`) | `Spec` -> `StyleTable` id cached per distinct spec: one hash of 25 words, no `Style` built or hashed after the first |
| answer caches | `Vec<NodeCache>` in the tree | side column by handle index; `NodeCache` is one word until its first store, so cheap leaves (paint-only blocks, `leaf_is_cheap`) never store |
| dirtiness | `append_child` marks the parent dirty: a cold group first runs the scheduler over every new node | nothing marked at mount: caches are empty, the first `layout` computes cold |
| not supported | - | absolute descendants that are not direct children (`AbsoluteContext` storage not implemented; `keeps_placements` false) |

**Per-frame instructions (slope method, RENDER=0, vlist headless; median of 5, `TICKS` 600 vs 3600):**

| mode | A mirror | B deep | B deep-kids | zgx |
|---|---:|---:|---:|---:|
| scroll | 21,306 | 20,597 | **20,288** | 107,598 |
| fling | 29,768 | 28,423 | **28,135** | 176,855 |
| jump | 51,606 | 47,940 | **47,243** | 333,408 |

B is 3-8 % cheaper per frame (it only matters in modes that mount rows). Per mounted row (jump, before the text-path work below; instructions):
mount A 26.5 K / B 18.8 K; compute 28.3 K / 26.4 K; write-back 22.3 K / 22.2 K; unmount 7.0 K / 5.8 K, so B saves ~10 % of a row
(the mirror's `new_leaf` + dirty marking and style hashing; the depth scans eat most of the scheduler saving, `deep-kids` recovers the rest).

**At tree scale** (`layout_bench`: a row of N/2000 columns of 500 cards, 4 layout nodes each; one card holds a label, a wrapping paragraph and a block;
instructions; edit = the paragraph's text alternates one / two lines):

| nodes | | A mirror | B deep | B deep-kids |
|---|---|---:|---:|---:|
| 2 K | mount + cold layout + write-back, per node | 60.6 K* | 60.2 K* | 59.4 K* |
| 10 K | same | 17.5 K* | 17.2 K* | 16.1 K* |
| 100 K | same | 8.2 K* | 8.3 K* | **6.9 K*** |
| 2 K / 10 K / 100 K | idle turn | 94 / 94 / 94 | 96 | 96 |
| 2 K / 10 K / 100 K | **contained edit** (fixed-height card) | 72.3 K / 71.9 K / 73.6 K | 74.9 K / 75.9 K / 75.9 K | 71.5 K / 72.0 K / 71.4 K |
| 2 K / 10 K / 100 K | **propagating edit** (auto-height card: 500 siblings move) | 155.9 K / 155.7 K / 160.0 K | 329.7 K / 361.4 K / 430.8 K | 141.9 K / 142.7 K / 149.6 K |

(*) includes process start-up (font tables, ~150 M instructions: 1.5 K per node at 100 K nodes) and, at 2 K nodes, the cold glyph rasteriser.
**Edit cost does not depend on tree size** (2 K to 100 K nodes: same instructions) and an idle turn is 94 instructions (~25 ns). B without the child cache pays
for scanning 500 siblings' depth entries on every propagating edit; with it B is the cheapest at every size. **Verdict:** deep-kids wins on every
measure (-3 to -8 % per frame, -16 % cold mount at 100 K nodes, no second tree, 9 % cheaper edits than the mirror); the mirror is the simplest and
supports everything cephal does (absolute descendants at any depth). Both are kept behind `deep`.

**What cephal itself costs** (`cephal_raw`, cephal's own `Tree`, no bough): a *style* edit that only re-places an item (`align-self`) costs
**13.8 K instructions (~3 us), independent of size** (400 vs 4,000 cards: 13,799 / 13,557); a *content* edit of a measured leaf costs **1.4 K instructions per sibling**
in the edited container (563 K for 400 cards, 5.5 M for 4,000; it matches cephal's own README, 0.14 ms for a list-item text change in a 100 K-node tree):
a node that asked its measured child speculative questions reports `Change::Full` to its parent, which re-runs its algorithm over all children
(`overflow: clip` does not stop it). That is cephal's behaviour, not the bridge's (bough adds ~2x on top in the 500-card column: heavier card styles).

## 4. bough vs zgx

### vlist (100 k chat rows, 720 x 800 pt window, scale 2), per frame, headless, slope method (median of 3-5)

| mode | zgx RENDER=0 | **bough RENDER=0** | ratio | zgx RENDER=1 | **bough RENDER=1** | ratio |
|---|---:|---:|---:|---:|---:|---:|
| scroll (12 pt/tick) | 107,598 | **21,306** | 5.1x | 1,127,908 | **256,311** | 4.4x |
| fling | 176,855 | **29,768** | 5.9x | 1,308,040 | **279,368** | 4.7x |
| jump (every 30 ticks) | 333,408 | **51,606** | 6.5x | 1,325,747 | **294,368** | 4.5x |

(`RENDER=0`: up to a ready draw list and instance writes (bough) / layout + damage, no renderer (zgx); `RENDER=1`: plus the offscreen Metal render, bough's IOSurface ring + one
draw + commit, zgx's wgpu `GpuRenderer`. With deep-kids: 20,288 / 28,135 / 47,243 and jump 291,453 at RENDER=1.) zgx has no headless vlist driver upstream: the zgx side is
`vlist_headless`, the windowed `vlist_workload`'s list and tick logic copied into an example of an identical copy of the zgx tree (`bench/layout/slope.py`'s `ZGX_COPY`).
Where bough's 21 K per scroll frame goes (PHASES=1, scroll): update (wires + list update) 10.2 K, text 0.1 K, damage 9.0 K, cull + list 2.8 K; a mounted row costs ~75 K
(text set 15 K, glyph emit 10 K, engine compute 22 K, builders + records + inserts ~25 K, unmount 7 K), one every ~12 scroll frames.

### Windowed, 10 s, from outside (`proc_pid_rusage` at 1.5 s and 9 s; GPU and WindowServer not included)

| mode | zgx CPU % | **bough CPU %** | zgx instr/s | bough instr/s | zgx footprint | bough footprint (ring 4 / ring 3) | ticks zgx / bough |
|---|---:|---:|---:|---:|---:|---:|---|
| scroll | 2.49 | **1.46** | 69.9 M | **30.5 M** | 149.5 MB | 160.3 / **152.0** MB | 599 / 600 |
| fling | 2.64 | **1.30** | 81.0 M | **29.0 M** | 151.0 MB | 159.6 MB | 599 / 600 |
| jump | 2.51 | **1.60** | 76.6 M | **30.6 M** | 151.8 MB | 160.8 MB | 599 / 600 |

(CPU % is noisy under load: earlier same-method runs gave bough 1.30 / 1.64 / 1.69 % vs zgx 2.21 / 2.71 / 3.09 %; the ratio is 1.5-2.1x; instr/s is the stable column,
2.3-2.8x. Idle wakeups 0 in all. **Memory is the column bough loses at the default ring of 4** (+9-11 MB: 4 x 9.2 MB IOSurfaces); with a ring of 3 it is a tie within 2 MB, as for heavy.)

### micro_bench SCENE=vscroll-variable (10,000 rows, op = scroll 12 pt), CPU ns per op, process CPU clock, median of 3 repeats (p10 / p90 of the best-median repeat)

| | RENDER | zgx write | **bough write** | zgx frame | **bough frame** |
|---|---|---:|---:|---:|---:|
| ns | 0 | 7,560 (6,540 / 20,460) | **1,440** (720 / 1,920) | 8,100 (1,000 / 25,700) | **1,400** (700 / 3,300) |
| ns | 1 | 9,380 (7,980 / 16,000) | **700** (520 / 1,000) | 221,800 (113,400 / 363,100) | **118,000** (56,400 / 214,200) |

bough's "write" is the whole turn (set + wires + row mount / measure / place + text + layout write-back; state ready to cull), "frame" is write + damage + cull + draw list (+ Metal
encode + commit with RENDER=1); zgx's write is `offset.set` and its frame `prepare_frame` + `flush` (+ render). Shape of the JSON identical (`framework, scene, figure, render, clock, ops, batch, median_ns, p10_ns, p90_ns, min_ns, max_ns, batches`).
Times are inflated and noisy by the load (zgx's own earlier idle-machine medians: 7,140 / 7,100 / 213,800); with a GPU the frame is dominated by driver threads (bough 4.3x fewer instructions, 1.9x less CPU time).

## 5. Correctness

* **Row heights vs zgx**: `ZGX_MODE=probe` in both (zgx's `vlist::probe_report`): the 13 probe rows identical, and **2,000 rows spread over the list (`PROBE_SPREAD`) identical
  in every height** (0 differing; 18 distinct heights 66..322 pt). The windowed bough `vlist_workload ZGX_MODE=probe` prints the same line.
* **Presented image vs full redraw** (`tests/correctness.rs`, the ring surface read back, byte for byte): scroll (400 ticks), fling (300), jump (200, a jump every 30 ticks), and a down / up / sub-row / big-step
  sequence (anchoring while scrolling back through unmeasured rows): 0 differing bytes. **Retained frame == a fresh mount at the same offset with the same row heights**, for all three modes.
  Heavy's suite (5 tests) still passes after the pool change.
* **Layout vs a cold cephal layout**: every mounted row's incremental layout equals `clone_cold` + cold layout, node for node, bit for bit; each node's written absolute geometry equals the sum of its chain of locations.
  `tests/layout.rs`: flex geometry (padding, gap, grow 1:3, percentage, centre, space-between, absolute) against hand-computed numbers; paragraph height == `bough-text`'s own `wrap_greedy` line count x 20;
  60 random text edits (wrapping, growing, shrinking) after each of which incremental == cold layout **and** retained records / instances == a fresh mount of the same values.
* **Prefix index** vs a naive running sum (random point updates, `prefix`, `row_at`); heights are whole numbers of lines (+ image) for rows 0..300.
* Word-table text path == glyph-run path on 12 K random cases; both cephal integrations pass every test (`--features deep`, `deep-kids`).
* Visual: `results/bough_vlist_t0.png` (tick 0) and `bough_vlist_jump60.png` (after two jumps), 1440 x 1600.

## 6. Applying the reformulation method (what lost, what changed)

* **Text**: the first word-table version summed steps byte by byte and *lost* (10.4 K vs the NEON shaper's 2.6 K per paragraph): the shaper already has the numbers. Rule bent: "text is
  measured from a glyph run". Representation change: keep the NEON run, add a word table found with a SWAR mask, wrap over words (5.3 K -> 0.6 K). The fused shape-to-instances kernel
  also lost to run + `instances` (11.8 K vs 10.0 K), so it was not used. The per-row *copy* (10.3 K) was a value already in the pool: the kernel now writes into the run and one NEON pass stamps + bounds (3.1 K).
* **Write-back**: a DFS from the group root scanned every sibling of every ancestor of an edited node (70-130 K for a 500-card column); the write-back now starts from the changed set and visits nothing else
  (contained edit 162 K -> 73 K).
* **Pool**: a long jump run (2,000 jumps) exhausted the size-class free lists (fragmentation); the pool is a buddy allocator now (coalescing, splitting), 32 K instances.
* **Jump**: instead of zgx's feedback passes (mount by estimate, observe, re-place, repeat across frames), measurement is part of the list update and the anchor is a value (row, offset), so a jump lands in its frame.

## 7. Gaps

* **Memory**: +9-11 MB over zgx at ring 4 (tie at ring 3, whose compositor safety bough-metal measured at ~3 % of frames at 120 Hz; not verified visually here).
* **cephal property**: editing a measured leaf costs O(siblings) (1.4 K instr each); a style edit is 13.8 K flat. Not changeable without touching cephal (not modified).
* **Text**: only ASCII text wraps (non-ASCII, and strings that need GSUB/GPOS like `fi`, lay out on one line when glyph != byte); breaks only at spaces; no `\n` in a `para`, no bidi, hyphenation, justify, letter spacing; `label` is one line.
* **Layout**: style is fixed at mount (only text content is a reactive operand; no signal-driven widths / flex-grow yet); no grid builder; `clip_overflow` is a layout property only (nothing is clipped visually; the list clips rows through its own rect); a measured-list row must be exactly one layout group;
  a *reactive* text edit inside a mounted list row re-lays the row out and re-places it but does not yet update the prefix index (`Lay::touched_roots` records it); `Deep` has no absolute-context storage (absolute boxes must be direct children of their containing block).
* **List**: overscan fixed in rows; `ROWS` x estimate error shows up in the scrollbar-less total (as in zgx, content height is approximate until rows are measured); rows above the viewport measured later move the offset by the anchoring rule (zgx's), tested down / up / jump.
* **Platform**: instance pool 32 K instances (5.6 K live for the vlist), 512 spaces, 32 clips; `MAX_INST` doubled for everything on the façade (pool memory is shared + untouched pages are not resident).
* **Measurement**: no GPU time, WindowServer CPU or frame-interval percentiles; CPU-time columns are noisy under load; micro_bench `state` and `vscroll-fixed` are not ported (vscroll-variable only); zgx's headless vlist driver is mine (a copy of its windowed loop), not upstream's.
* The `ZGX_PARITY` handshake and `ZGX_INITIAL_TICKS` are not implemented for vlist (neither exists in zgx's vlist adapter).
