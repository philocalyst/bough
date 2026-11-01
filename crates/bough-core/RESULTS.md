# bough-core: results

Iteration 2 (Opus): structural reformulations of the Sonnet prototype, measured every time. The prototype's
results are kept verbatim as the appendix (iteration 1). Machine: Apple M3 Pro, `nightly-2026-10-17`, release
profile (thin LTO). Primary metric: retired instructions (`ri_instructions`, deterministic to ~1%); other agents
were compiling on this machine throughout, so ns are indicative only.

```
export CARGO_TARGET_DIR=/Users/mileswirht/projects/bough/target/core RUSTC_WRAPPER= PATH=~/.local/share/cargo/bin:$PATH
cargo test -p bough-core --release            # differential suite (8 profiles), kernels, shapes  (~10 s)
cargo test -p bough-core                      # same in debug, with domain debug_asserts          (~40 s)
cargo run  -p bough-core --release --example core_bench     # Bough vs ArenaTree (zgx model), sections 1 and 6
cargo run  -p bough-core --release --example trace_bench    # the damage lab's 10 zgx-derived traces, section 2
cargo run  -p bough-core --release --features stats --example cull_stats   # cull work counters
cargo run  -p bough-core --release --example mark_probe | batch_probe       # per-mark / batched-write probes
```

## 0. API changes (for the integration agent)

Names are stable where the semantics survived; everything below changed. All of it follows from adopting the
damage lab's measured representation (`crates/bough-damage-lab/RESULTS.md`, DESIGN.md section 2 as revised).

| change | before | after |
|---|---|---|
| tile size, plane, ring | 16 px tiles, `Plane { bbox, rows: [u128; 96] }`, ring of 8, window <= 2048x1536 | 32 px tiles (the GPU's load/store unit), `Plane { rows: [u128; 128], y0, y1, solid, n }`, ring of 4 (`repair(age)`, age 1..=4), window <= 4096x4096. **`Plane::bbox` is gone.** |
| `repair_into(age, &mut Plane)` | returned `()` | returns the repaired plane's pixel bbox `NRect` (EMPTY when clean); pass it to `cull_tiles` |
| frame cull | `cull(pl.bbox, None, out)` (bbox superset) | **`cull_tiles(&plane, bbox, out)`**: every record whose (visible) ink touches a damaged tile, paint order, the lab's format-S list. Also `cull_repair(age, &mut scratch, out)`. |
| `cull(bbox, Some(plane), out)` | ink meets bbox and touches a tile | same, but `bbox` is clipped to the window first (invisible ink never draws) |
| per-record stamps | `Record.stamp`, `stamp_at`, page `max_stamp` | **removed** (the lab measured them losing on CPU and GPU). `changed_since(e, out)` is now tile-granular: positions of records whose visible ink touches a tile damaged after `e`; every record if `e` is older than the ring |
| `set_ink` damage | old and new ink, always | nothing if unchanged; `old XOR new` strips for kinds marked `set_kind_uniform(kind, true)`; old and new otherwise |
| `translate_run` damage | old/new union per page chunk | old/new union of the whole run (2 marks). New `scroll_run(start, len, dx, dy, clip)` marks `clip ∩ old` and `clip ∩ new` |
| clips | none | `RecordInit.clip` / `Record.clip: u16` (`RecordInit::new` sets 0, `.with_clip(id)`), `new_clip(rect) -> u16`, `clip_rect`, `set_clip_rect(id, rect)` (damage `old XOR new`), `set_clip_run(start, len, id)`. Visible ink = ink ∩ clip everywhere (cull, hit, every mark) |
| new writes | | `damage_inside(r)` (pre-clipped coverage: no clip work), `damage_xor(old, new)`, `set_ink_nodamage(pos, ink)`, batched `set_inks(&[(pos, ink)], damage)` and `set_inks_handles(&[(Handle, ink)], damage)` |
| new queries | | `ancestors(pos, &mut Vec)`, `children_into(pos, &mut Vec)`, `kind_at(pos)` |
| snapshots | | `freeze_into(&self, dst: &mut Bough) -> pages_copied`: `dst` becomes a read-only replica (render thread) |
| `Page` | 1536 B: depth, ink, stamp, kind, payload, handle | 1472 B: depth, ink (**page-relative**: absolute = `ink + off`), gink (8 group summaries), kind, payload, handle, clip; header cgroups, version, id, dir, dirty, off/offl, wgen. `Page::read` returns absolute ink |
| `ArenaTree` | | `set_bounds(id, rect)` (the zgx-model layout write, baseline for `set_inks`) |
| Cargo | | dev-dependency on `bough-damage-lab` (trace replay); feature `stats` |

## 1. Headline: prototype (iteration 1) vs final (iteration 2) vs the Arena (zgx model), instructions

The frame rows changed meaning in the Bough's favour: the prototype's "cull" was a **bbox superset** over 16 px
tiles; the final `cull_tiles` returns the lab's **tile-exact** draw list over 32 px tiles (every record whose
visible ink touches a damaged tile, nothing else), which is what the GPU's format-S consumption needs.
Arena = lean zgx `Scene` model (lower bound on zgx). Ratio = Arena / final.

| operation | shape | prototype | **final** | Arena | final vs Arena |
|---|---|---:|---:|---:|---:|
| single paint write + repair(1) + cull | dashboard | 966 | **1,052** | 4,309 | 4.1x |
| single paint write + repair(1) + cull | list | 845 | **921** | 6,314 | 6.9x |
| single paint write + repair(1) + cull | flat100k | 3,776 | **1,217** | 139,133 | 114x |
| single paint write + repair(1) + cull | deep40 | 1,193 | **1,234** | 13,908 | 11.3x |
| 80 scattered writes + cull (tile-exact; prototype: bbox superset) | dashboard | 29,618 | **54,802** | 244,130 | 4.5x |
| 80 scattered writes + cull | flat100k | 50,774 | **100,619** | 545,144 | 5.4x |
| translate a ~1,000-node subtree + cull | dashboard | 25,318 | **9,869** | 506,027 | 51x |
| translate 1,000 nodes + cull | flat100k | 29,343 | **9,704** | 836,453 | 86x |
| scroll step: 20 rows scroll in their viewport + insert/remove a row + cull | list | 10,290 | **11,710** | 142,592 | 12x |
| cull(full window) | dashboard | 18,730 | **7,789** | 170,745 | 22x |
| cull(full window) | flat100k | 40,959 | **13,237** | 348,259 | 26x |
| hit test | flat100k | 2,640 | **417** | 84,038 | 202x |
| hit test | dashboard | 344 | **357** | 3,998 | 11x |
| layout pass: 200 `set_ink` (one call each) / `set_inks` batch | dashboard | - | 61,747 / **43,294** | 104,016 (`set_bounds` x200) | 2.4x |
| splice: insert 8 + remove 8 at the front | dashboard | 3,051 | **3,343** | 4,697 | 1.4x |
| parent(pos) | dashboard | 105 | 105 | 26 | 0.25x (loses) |
| ancestors(pos) | flat100k | - | 1,188 | 51 | 0.04x (loses) |

Write path alone (touch_handle + next_epoch): 194 -> **164**; + `repair_into(1)`: **412** (the lab-style
whole-plane repair cost 731 per frame before the row range).

## 2. Damage-lab traces: track + cull per frame (instructions), in-process against the lab's own forms

`examples/trace_bench.rs` replays the lab's ten zgx-derived traces (2560x1600 and 1920x1440 windows,
600 frames) through the Bough: tree update = one batched `set_inks` per frame of every changed slot's new
coverage; track = the lab's exact symmetric-difference rules (`exact_nodes_group_bounds`) marking through
`damage_inside`; cull = `repair_into(age)` + `cull_tiles`. **Every presented frame's draw list was checked equal
to the lab's `hw32` cull** (all 10 traces x ages 1 and 3, 11,966 frames) before timing. The lab's `hw32` and
`zgx` forms run in the same process on the same machine state. "diff rules" = the lab's exact-difference
computation alone (shared by both; it is the binding layer's work, not the core's).

| trace | age | marks/frame | diff rules | core track | **core cull** | **core track+cull** | lab hw32 track+cull | lab zgx track+cull | vs hw32 | vs zgx |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| heavy-stream | 1 | 1324 | 144,955 | 229,057 | **102,413** | **331,470** | 337,203 | 500,049 | 1.02x | 1.51x |
| heavy-stream | 3 | 1324 | 144,571 | 228,773 | **102,969** | **331,742** | 340,172 | 594,734 | 1.03x | 1.79x |
| heavy-scroll | 1 | 57 | 2,068 | 5,501 | **8,580** | **14,081** | 72,128 | 40,334 | 5.12x | 2.86x |
| heavy-scroll | 3 | 57 | 2,456 | 5,526 | **8,911** | **14,437** | 72,272 | 40,340 | 5.01x | 2.79x |
| heavy-both | 1 | 1915 | 150,774 | 275,021 | **90,575** | **365,596** | 374,375 | 531,994 | 1.02x | 1.46x |
| heavy-both | 3 | 1915 | 151,287 | 274,426 | **91,333** | **365,758** | 376,109 | 609,974 | 1.03x | 1.67x |
| lab-stream | 1 | 18 | 50,381 | 45,582 | **20,845** | **66,427** | 74,982 | 22,909 | 1.13x | 0.34x |
| lab-stream | 3 | 18 | 50,390 | 45,942 | **20,418** | **66,360** | 75,764 | 23,293 | 1.14x | 0.35x |
| lab-scroll | 1 | 32 | 1,149 | 3,223 | **4,298** | **7,521** | 32,809 | 18,168 | 4.36x | 2.42x |
| lab-scroll | 3 | 32 | 1,459 | 3,214 | **4,640** | **7,853** | 33,138 | 18,460 | 4.22x | 2.35x |
| animate | 1 | 35 | 1,842 | 3,825 | **5,332** | **9,157** | 9,152 | 31,170 | 1.00x | 3.40x |
| animate | 3 | 35 | 1,861 | 3,832 | **5,438** | **9,270** | 9,893 | 48,782 | 1.07x | 5.26x |
| blink | 1 | 1 | 70 | 136 | **561** | **697** | 1,850 | 1,318 | 2.66x | 1.89x |
| blink | 3 | 1 | 70 | 136 | **584** | **720** | 2,514 | 1,577 | 3.49x | 2.19x |
| vlist-scroll | 1 | 281 | 19,781 | 37,382 | **12,716** | **50,097** | 133,189 | 63,425 | 2.66x | 1.27x |
| vlist-scroll | 3 | 282 | 18,607 | 36,198 | **12,698** | **48,896** | 133,165 | 63,470 | 2.72x | 1.30x |
| vlist-fling | 1 | 362 | 23,548 | 46,047 | **13,746** | **59,794** | 145,129 | 67,342 | 2.43x | 1.13x |
| vlist-fling | 3 | 360 | 23,076 | 45,418 | **12,980** | **58,398** | 145,274 | 66,396 | 2.49x | 1.14x |
| vlist-jump | 1 | 390 | 21,992 | 45,207 | **12,641** | **57,849** | 142,427 | 67,564 | 2.46x | 1.17x |
| vlist-jump | 3 | 391 | 20,325 | 44,420 | **13,173** | **57,593** | 142,760 | 64,191 | 2.48x | 1.11x |

Reading it:

* **The cull is 1.6-7.6x cheaper than the lab's `hw32` cull** on every trace (heavy-stream 102 K vs 160 K;
  vlist 13 K vs 107 K; heavy-scroll 8.6 K vs 67 K), with identical output.
* Track + cull is **1.5-5.3x below zgx** on heavy-stream, heavy-both, heavy-scroll, lab-scroll, animate and
  blink, and 1.1-1.3x below on the vlist traces.
* **Losing regime: lab-stream** (one rewritten log line per frame): 66 K vs zgx's 23 K. 50 K of it is the
  per-glyph exact-difference rules (binding-layer work, the lab's own `hw32` pays the same); the core's part is
  ~16 K of marks + 21 K of cull. zgx unions coarse bounds and pays for it on the GPU (432 vs 50 tiles, the
  lab's section 0). The next step belongs to the text layer: per-glyph diffs as SoA compares (the lab's
  recommendation) rather than the core.
* Tree update (not in the totals; the lab subtracts it too): heavy-stream 175 K/frame batched (353 K one
  `set_ink_nodamage` at a time). The lab's scene update is 157 K (array stores + 64-wide page re-unions).

## 3. Recommended core design (what the code now is)

**Damage (the lab's representation, plus a row range).** A plane is one bit per 32 x 32 device-pixel tile,
`[u128; 128]` rows (windows <= 4096 px a side), plus `[y0, y1)`, the band outside which every row is zero.
Ring of 4 planes by epoch; `repair_into(age)` ORs the band of the last `age` planes into a caller plane in one
fused pass that also returns the pixel bbox and the **solid certificate** (the damaged tiles are exactly the
bbox). No per-record stamps. Write-time marks are exact symmetric differences supplied by the caller
(`damage_inside` for pre-clipped coverage, `damage_rect` clips in tile units, `damage_xor`, `set_ink` with
uniform kinds), translations mark `clip ∩ old` and `clip ∩ new` (`scroll_run`), never per moved record.

**Records.** 64-slot SoA pages in paint preorder from a type-stable pool (1,472 B: depth, page-relative ink,
8 group summaries, kind, payload, handle, clip id; header with the offset lanes, dir index, write generation).
A page's inks are relative to its offset, so a translated run's interior pages move by changing one field.
Clip ids index an absolute clip table; visible ink = ink ∩ clip, applied at query and mark time.

**Directory.** Flat SoA (page pointer, absolute union ink, min depth, length) + a chunk level (union of 64
pages) + blocked prefix counts. Every spatial query is a 4-level SIMD filter: chunk -> page -> 8-record group
-> record, each one `lt_mask`/`lt8` of an `NRect` column against one threshold, with the page offset and clip
folded into the threshold by one saturating lane subtract per surviving page.

**Frame cull** (`cull_tiles`): solid certificate -> bbox filter only; otherwise surviving groups cut their
clipped tile span out of the plane as `u16` row windows: none -> drop, all -> keep, partial -> 8 records tested
in NEON lanes. Emission is table-driven compaction (`emit::compact8`: two `tbl`, two stores, a popcount bump).
Output: the paint-ordered, tile-exact payload list the GPU consumes (format S + vertex-shader clipping, DESIGN
section 2).

**Writes.** Handles hold page pointers (a write is two loads to its record). Batched ink writes (`set_inks`)
settle each page's summaries once, when the walk leaves the page. Structure edits are run splices as before.

**Readers.** `freeze_into` keeps a replica by copying only pages whose write generation changed; the replica
is a plain `Bough` (lock-free reads). Its directory pass must become O(dirty chunks) before 100 k-node trees.

## 4. The attempts (rule bent, representation change, numbers, verdict)

Every attempt below kept the differential suite green: `tests/differential.rs` compares the Bough with the
dumb oracle after every op (records, every repair plane row by row and its bbox, `cull_tiles` for every age,
general culls, hits, `subtree_end`/`parent`/`children`/`children_into`/`ancestors`, handles, `changed_since`,
and a `freeze_into` replica), over 8 profiles: small window, long sequences, a 2600 px window (tile columns
past 64, spans straddling the u128 word boundary), the 4096 x 4096 maximum, far translates (page offsets in
the thousands), type stability, a 20 k bulk mount/peel, and bursts of 300 marks per epoch. New mechanisms
were mutation-checked (dropping the clip in `group_visible`, dropping the write-generation bump: both caught).
Kernels: `lt8`/`union8` vs scalar on 200 k random groups, `compact8` exhaustively over all 256 masks.
Columns "v1..v8" are snapshots measured along the way; the final full tables are in section 6.

### Attempt 0 (prerequisite): adopt the lab's damage plane, then fix its sparse-frame cost

*Rule bent:* "damage is a 16 px plane plus a bbox". *Representation:* the lab's `hw32` plane (one bit per 32 px
hardware tile, a `u128` per row, ring of 4, no bbox, no stamps) plus one field the lab rejected: a **row range
`[y0, y1)`** maintained by `mark` with a `min` and a `max`.

The lab dropped dirty-row tracking because whole-plane OR/clear is cheaper than tracking at 1,300 marks per
frame. At one mark per frame it is the other way round: the lab-style whole-plane repair (fused OR + bbox over
25 rows) cost **731 instructions per repair** (v1 `repair3`), more than the rest of the single-write frame. An
exact row-occupancy `u128` cost ~10 instructions per mark; the row range costs ~4 and keeps repair/clear/OR to
the damaged band. Measured per mark on heavy-stream rects in isolation: lab `Hw::mark` 40-43, ours 55
(`damage_inside`: pre-clipped coverage, the binding path) or 72 (`damage_rect`: clip to the window, done in tile
units after the shift). The ~12 over the lab are the range update and emptiness test; they buy the
sparse-frame repair. **Kept.**

### Attempt 1: make culling disappear for the common case

*Rule bent:* "every frame culls". Three formulations:

1. **Exactness certificate (kept).** `repair_into` computes, in the same pass, whether the damaged tiles are
   exactly their bounding rectangle (`Plane::solid`). Then the bbox test *is* the exact tile test and
   `cull_tiles` runs the cheap bbox cull with no tile work at all. One write marks a rectangle, so the single
   write frame gets a tile-exact draw list at the cost of the prototype's bbox superset: dashboard
   **947** (v4) vs prototype bbox-superset 966; 100 k nodes **1,112** vs 3,776.
2. **Version-keyed persistent draw lists patched by writes (analysed, not built).** A per-handle cache of
   "records touching my tiles", valid while a global geometry version is unchanged, would make a warm
   paint-only frame a copy (~50 instructions instead of ~900). It was not built because on the lab's ten
   zgx-derived traces every frame of every workload changes geometry somewhere (the tree-update stage, which
   only runs for changed coverage, is non-zero on every presented frame: text rewrites change glyph slots,
   scrolls move content, blink toggles the caret's coverage), so a geometry-versioned cache would be cold on
   every measured frame, while costing memory per handle and a merge for multi-write frames. Spatially keyed
   invalidation (per-tile geometry epochs) is the version that could work; it is the same machinery as the
   tile cull itself, which after attempts 2/9 costs ~100 instructions per surviving page.
3. **Page-granular draw lists expanded by the GPU (rejected by the lab's data).** Emitting (page, mask) pairs
   would remove per-record emission, but the lab measured that the GPU's cost is tiles touched, and format S
   (exact instance list) + vertex-shader clipping is what keeps tiles touched at the oracle. So the CPU must
   produce the exact list; attempt 9a made that emission nearly free instead.

### Attempt 2: coarse tile signatures, done as group tile windows

*Rule bent:* "a record's tile test is per record, in pixels". The prompt's version (a 64-bit window-tile
signature per page or 8-record group, maintained at write time) taxes every geometry write and every
translate (signatures are absolute). The version built reads the plane instead, at cull time:

* **8-record groups** (`gink[8]` per page, attempt 9b) are the unit; a group's tile span is clipped to the
  damage bbox (tiles outside it are clear), and its rows are cut out of the plane as one `u16` window per row.
* `none` (every window 0) drops the group, `all` (every window full) keeps every bbox-passing record with no
  per-record work, `partial` tests the 8 records at once in NEON lanes: per lane a column mask
  `(1 << lx1) - (1 << lx0)` (`ushl`) and a row mask; per row one `tst` of the broadcast window against all 8
  column masks. Container-sized groups (span > 16 x 16 tiles) fall back to a per-record span test.

Cull counters (`cull_stats`, per frame): dashboard single write: 4 pages, 5.7 groups, all of them `all`, 9
outputs. 80 scattered writes: 24 pages, 185 groups (42 none, 0.5 all, 142.5 partial), 938 records refined.

| row (Bough instructions) | v1: span loop per group/record | v2: spans clipped to the damage bbox | v3: group windows + NEON lanes |
|---|---:|---:|---:|
| write1+cull dashboard | 2,550 | 1,732 | 1,442 |
| write1+cull deep40 | 8,449 | 2,420 | 2,211 |
| write80+cull dashboard | 83,609 | 84,927 | 48,770 |
| write80+cull flat100k | 153,218 | 156,184 | 90,266 |

**Kept** (group windows); write-time signatures **rejected** without building (the cost lands on every write,
the benefit only on scattered frames, and the plane already holds the answer).

### Attempt 3: quantised 8-bit ink lanes (analysed, not built)

*Rule bent:* "ink is exact i16 in the hot column". Once groups exist, the per-record bbox test is `lt8`: 4
loads, 4 `cmgt`, 3 `uminp`, a narrow and an `addv`, ~15 instructions per 8 records (~1.9 each), and it only
runs inside surviving groups. u8 lanes relative to the group origin would halve the compares but add a
conservative round-out, a second exact pass on survivors (which is every output record), and a second column
to maintain on every ink write and translate. The ceiling is under 1 instruction per *surviving* record, below
the emission cost; not worth a second representation.

### Attempt 4: a third directory level, crossover included

*Rule bent:* "two levels". *Representation:* `c_ink`, the union of each run of 64 directory entries, kept exact
on every summary change (monotone growth is one `min`; a shrink recomputes its chunk). Every scan descends
chunk -> page -> group -> record. For trees of <= 64 pages there is one chunk: the cost is one extra 1-entry
test (~10-15 instructions), measured inside the dashboard/list/deep40 rows. At 100 k nodes (1,563 pages):

| row | prototype | after |
|---|---:|---:|
| hit (1 point) flat100k | 2,640 | 375 |
| write1 + cull flat100k | 3,776 | 1,112 |

**Kept.** The chunk recompute is O(pages/64 x 64) on splits/merges, like the existing directory shift.

### Attempt 5: clip chains and scroll containers

*Rule bent:* "absolute ink is rewritten when a subtree moves". Two representation changes:

1. **Page offsets folded into the query threshold.** Each page stores inks relative to `off` (absolute = stored
   + off); `offl = (dx, dy, -dx, -dy)` as i16 lanes. A translated run's interior pages change only `off` and
   their summary (O(1) per page); the <= 2 partial end pages are normalised and rewritten. Queries move into
   page coordinates instead: one saturating lane subtract per surviving page for the threshold (and the clip),
   one wrapping lane add for spans (`NRect::add_lanes`: stored + off is in the domain, so wrapping is exact even
   for offsets that are not). Writes store `abs - off` with a checked conversion (normalise on overflow);
   |off| <= 32767 is kept by normalising instead of overflowing. Translate + cull: dashboard 1,016-node
   subtree 15,274 -> 9,155 (v4 -> v5), flat100k 1,000 nodes 14,616 -> 8,894; the Arena rewrites every node
   (506 K / 836 K).
2. **Clip ids.** A record stores unclipped ink and a `clip: u16` into a clip table (absolute rects, composed by
   the caller: a scroll container's viewport). Visible ink = ink ∩ clip is applied at query and mark time;
   summaries stay unclipped unions (valid filters). A per-page `cgroups` mask means clip-free groups never
   touch the clip column; a clipped group's 8 visible inks are materialised out of line (one broadcast clip,
   4 `smax`, a lane-wise canonicalise when the group shares one clip). A scroll step is
   `scroll_run(rows, 0, dy, viewport)`: O(pages) record work and the lab's exact scroll damage
   `viewport ∩ old ∪ viewport ∩ new`, never per moved record.

*Cost, honestly:* the hot loops carry the offset and clip handling: v4 -> v8 on unclipped shapes, culls are
~5-12% more instructions (dashboard write1 947 -> 1,053, cull(full window) 6,769 -> 7,765); on the list
shape, whose rows are now clipped by their scroll container (as in a real UI), the hit test went 331 -> ~800
and the full-window cull 2,532 -> ~5,700. That is the price of clip correctness and O(pages) scrolling;
**kept**. The exact damage of a scroll step with copy-forward (repaint only the exposed strip) was not built:
the lab measured blits (82 ns/tile) costlier than repainting (14-49 ns/tile) on this GPU.

### Attempt 6: structure operations

*Rule bent:* "every query starts with a locate".

| row (instructions) | dashboard | list | flat100k | deep40 |
|---|---:|---:|---:|---:|
| `children(pos)` (prototype: `subtree_end` hops) | 208 | 123 | 152 | 164 |
| `children_into` (one forward depth scan, pages skipped by `min_depth`) | 172 | 119 | 170 | 194 |
| `ancestors(pos)` (one backward pass) | 488 | 351 | 1,188 | 754 |
| Arena: parent-pointer chain | 86 | 61 | 51 | 262 |

`children_into` wins a little on short subtrees and loses on deep40 (long in-page scans); `ancestors` still loses
4-23x to pointer chasing. **Losing regime kept and recorded.** The structural fix is a per-record
`parent_delta`/`skip` column; its maintenance cost is the problem: inserting k records at the top of an
n-child list changes the parent distance of all n-1 later siblings (O(siblings) per splice), and a `skip`
column needs every ancestor updated per insert/remove (O(depth) locates, ~40 x 30 instructions on deep40).
Neither was built. `move_run` by page relinking and order labels for pages were not built either: the
measured splice rows (2.8-3.9 K, 1.2-2.1x better than the Arena) are dominated by in-page memmoves, not by
the O(pages) reindex, which only runs on splits/merges.

### Attempt 7: batch application

*Rule bent:* "a write settles its summaries before returning".

* **Batched ink writes (kept).** `set_inks(&[(pos, ink)], damage)` walks sorted positions: one locate per page
  (not per write), stale group bits accumulated, each page's summary and chunk settled once, the moment the walk
  leaves the page. One mechanism, cheapest in both regimes (`batch_probe`, instructions per write):

  | | dashboard, no damage | dashboard, damage | flat100k (1 write/page), no damage | flat100k, damage |
  |---|---:|---:|---:|---:|
  | eager `set_ink` | 114 | 232 | 167 | 215 |
  | first batch (dirty list, settle at end) | 54 | 165 | 232 | 269 |
  | final batch (settle when the walk leaves a page) | 48 | 156 | 173 | 210 |

  Layout pass of 200 writes (`layout200`, v8 table), and the trace replay's tree update on heavy-stream:
  353 K/frame one write at a time -> 170 K batched (`BATCH=0` vs default). `set_inks_handles` keeps the
  dirty-list form (handles arrive in any order).
* **Batched mark rasterisation (rejected).** Marks buffered per epoch and rasterised 8 at a time (`ld4`,
  lane-wise validity/clamps, scalar row ORs): 64 vs 72 instructions per mark in isolation, but the frame
  protocol repairs *before* closing the epoch, so the open epoch's marks are rasterised twice (into the repair
  output, then into the ring): heavy-stream track -3 K, cull +2.5 K, net zero, plus a stale `plane(0)`.
  Reverted (not shipped).

### Attempt 8: epoch snapshots for a reader thread

*Rule bent:* "the render thread reads the UI thread's structure". `freeze_into(&self, dst)` makes `dst` an
exact replica; a page is copied only if its write generation (`wgen`, bumped by every mutable access) differs
from the replica's. The replica is an ordinary `Bough`, so the render thread runs the same cull/hit code
lock-free between freezes.

| per turn (instructions) | dashboard | list | flat100k | deep40 |
|---|---:|---:|---:|---:|
| touch_handle + freeze (0 pages copied) | 2,095 | 1,456 | 81,542 | 1,019 |
| 80 scattered `set_inks` + freeze | 39,462 (23.4 pages) | 27,755 (8.0) | 127,508 (35.0) | 23,620 (4.0) |
| 80 scattered `set_inks`, no freeze | 30,329 | 23,736 | 35,379 | 21,181 |

Page copies cost ~150-400 instructions each (1.5 KiB). The directory rebuild is O(pages): ~1.5 K on small
trees, 81 K at 1,563 pages: **kept for small trees, not yet acceptable at 100 k.** Next step (not built):
a per-directory-entry `d_wgen` column (compared 4 lanes at a time instead of reading page headers) and a
directory generation so `d_ptr` is rebuilt only after structural edits; that makes an idle freeze O(pages/4)
NEON compares (~1 K at 100 k) and a busy one O(dirty pages).

### Attempt 9: inventions

* **9a. Emission is a table lookup.** The cull's output step was a `ctz`/clear/push loop (~10 instructions per
  emitted record; it was most of the prototype's full-window cull). `emit::COMPACT[mask]` is the byte shuffle
  that moves the set lanes' payloads to the front: two `tbl` over 32 payload bytes, two 16-byte stores, a
  pointer bump by `popcount`, no per-record branch. cull(full window): dashboard 18,730 -> 6,769 (v4),
  flat100k 40,959 -> 11,566.
* **9b. The 8-record group is the filter unit; the page is only the storage unit.** `gink[8]` per page (64 B,
  maintained exactly by `union8`, 3 `smin`); every scan tests 8 group summaries (one `lt8`) before any record.
  A page that survives only because of one container record costs ~15 instructions for the other 7 groups.
* **9c. Exactness certificate** (`Plane::solid`, attempt 1).
* **9d. Row-range occupancy** (attempt 0) and **tile-space clipping** of marks (the window clip after the
  shift, in tile units).
* **9e. Offsets and clips folded into thresholds with saturating/wrapping lane arithmetic** (attempt 5).

## 5. Losing regimes and what to try next

| regime | final | best competitor | why / next |
|---|---:|---:|---|
| `parent` / `ancestors` | 105 / 351-1,188 | Arena pointer 26 / 51-262 | no parent pointers; a `parent_delta` column costs O(later siblings) per splice, `skip` costs O(depth) locates per splice. Next: answer bubbling from the hit test itself (the hit descent already passes every ancestor's page: collect ancestors on the way down) |
| `children` iteration | 119-208 | Arena slice 24 | same cause; `children_into` only helps short subtrees |
| lab-stream trace | 66 K | zgx 23 K | 50 K is per-glyph exact diffing (text layer, shared with the lab); zgx pays on the GPU instead (8.6x more tiles) |
| tile-exact scattered culls vs the prototype's bbox superset | 54.8 K | 29.6 K (superset) | the list is exact now (the lab: tiles touched dominate frame time); 938 records/frame need lane tests on dashboard |
| clipped groups on the hit path | list hit 811 | 331 unclipped | visible inks materialised per clipped group; next: per-page clip when a page's records share one clip |
| `freeze_into` on 100 k nodes | 81 K idle | - | O(pages) directory rebuild; next: `d_wgen` column + directory generation |
| per-mark cost | 55 (`damage_inside`) | lab 40-43 | the row-range update; the single-write repair it enables is worth more |
| offsets + clips in the cull loop | +5-12% on unclipped shapes vs v4 | v4 | the price of O(pages) scrolling and clip correctness |

## 6. Raw output, final build (`core_bench`, `trace_bench`, `cull_stats`)

```
dashboard  write1   out/frame     9.1 tiles/frame    3.0 pages 4.0 groups 5.7 g_none 0.0 g_all 0.0 g_partial 0.0 refined 0.0 rows_g 0.0 rows_r 0.0
dashboard  write80  out/frame   453.7 tiles/frame  202.1 pages 24.0 groups 185.0 g_none 42.0 g_all 0.5 g_partial 142.5 refined 937.6 rows_g 562.0 rows_r 98.0
list       write1   out/frame     9.5 tiles/frame    3.4 pages 2.7 groups 4.4 g_none 0.0 g_all 0.0 g_partial 0.0 refined 0.0 rows_g 0.0 rows_r 0.0
list       write80  out/frame   235.6 tiles/frame  136.6 pages 8.0 groups 58.0 g_none 22.1 g_all 0.2 g_partial 35.7 refined 280.1 rows_g 192.0 rows_r 117.0
flat100k   write1   out/frame    10.6 tiles/frame    2.4 pages 3.3 groups 9.0 g_none 0.0 g_all 0.0 g_partial 0.0 refined 0.0 rows_g 0.0 rows_r 0.0
flat100k   write80  out/frame   731.1 tiles/frame  172.9 pages 40.5 groups 318.0 g_none 67.0 g_all 0.0 g_partial 251.0 refined 2000.9 rows_g 401.8 rows_r 742.5
deep40     write1   out/frame    37.5 tiles/frame    2.4 pages 2.6 groups 13.0 g_none 0.0 g_all 0.0 g_partial 0.0 refined 0.0 rows_g 0.0 rows_r 0.0
deep40     write80  out/frame   160.0 tiles/frame   43.9 pages 3.0 groups 20.0 g_none 0.0 g_all 0.0 g_partial 20.0 refined 160.0 rows_g 420.0 rows_r 990.0
```

#### core_bench (optimized)
loop baseline: 5.0 instr, 2.09 ns per empty iteration (subtracted from all rows)
page-start prefix: blocked (2-level), eager

#### Per-operation cost (Bough vs ArenaTree), loop baseline subtracted

##### write1+cull(repair1)

| shape | Bough instr | Bough ns | Bough cyc | Arena instr | Arena ns | Arena cyc | instr ratio (Arena/Bough) | ns ratio |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| dashboard | 1052 | 97 | 284 | 4314 | 641 | 1351 | 4.1x | 6.6x |
| list | 921 | 103 | 242 | 6325 | 639 | 1722 | 6.9x | 6.2x |
| flat100k | 1217 | 156 | 370 | 139146 | 13132 | 40929 | 114.3x | 84.0x |
| deep40 | 1234 | 163 | 320 | 13962 | 1625 | 4013 | 11.3x | 10.0x |

variants:

| shape | variant | instr | ns | cyc |
|---|---|---:|---:|---:|
| dashboard | write+repair_into (no cull) | 412 | 17 | 63 |
| dashboard | touch(pos) instead of handle | 1091 | 92 | 303 |
| dashboard | bbox-only cull (superset) | 1053 | 91 | 255 |
| dashboard | write only (touch_handle+next_epoch) | 164 | 16 | 26 |
| dashboard | ARENA write only (mark_paint+flush) | 360 | 53 | 100 |
| list | write+repair_into (no cull) | 419 | 74 | 68 |
| list | touch(pos) instead of handle | 944 | 116 | 264 |
| list | bbox-only cull (superset) | 928 | 66 | 225 |
| list | write only (touch_handle+next_epoch) | 166 | 12 | 25 |
| list | ARENA write only (mark_paint+flush) | 332 | 19 | 49 |
| flat100k | write+repair_into (no cull) | 409 | 17 | 61 |
| flat100k | touch(pos) instead of handle | 1313 | 238 | 414 |
| flat100k | bbox-only cull (superset) | 1219 | 167 | 328 |
| flat100k | write only (touch_handle+next_epoch) | 163 | 20 | 31 |
| flat100k | ARENA write only (mark_paint+flush) | 320 | 30 | 55 |
| deep40 | write+repair_into (no cull) | 414 | 40 | 68 |
| deep40 | touch(pos) instead of handle | 1255 | 111 | 351 |
| deep40 | bbox-only cull (superset) | 1251 | 127 | 321 |
| deep40 | write only (touch_handle+next_epoch) | 164 | 10 | 27 |
| deep40 | ARENA write only (mark_paint+flush) | 553 | 121 | 229 |

##### write80+cull(repair1)

| shape | Bough instr | Bough ns | Bough cyc | Arena instr | Arena ns | Arena cyc | instr ratio (Arena/Bough) | ns ratio |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| dashboard | 54802 | 3593 | 10051 | 243578 | 45311 | 61169 | 4.4x | 12.6x |
| list | 27043 | 1838 | 4523 | 136126 | 8407 | 30073 | 5.0x | 4.6x |
| flat100k | 100619 | 12782 | 21247 | 547088 | 135352 | 125745 | 5.4x | 10.6x |
| deep40 | 29413 | 3862 | 5425 | 93181 | 38481 | 39188 | 3.2x | 10.0x |

variants:

| shape | variant | instr | ns | cyc |
|---|---|---:|---:|---:|
| dashboard | bbox-only cull (superset) | 18849 | 1006 | 3600 |
| list | bbox-only cull (superset) | 16035 | 2193 | 3294 |
| flat100k | bbox-only cull (superset) | 24247 | 5687 | 5928 |
| deep40 | bbox-only cull (superset) | 12330 | 7534 | 2936 |

##### translate1016

| shape | Bough instr | Bough ns | Bough cyc | Arena instr | Arena ns | Arena cyc | instr ratio (Arena/Bough) | ns ratio |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| dashboard | 9869 | 791 | 2186 | 500334 | 34842 | 98155 | 50.7x | 44.1x |

variants:

| shape | variant | instr | ns | cyc |
|---|---|---:|---:|---:|
| dashboard | translate only (no cull) | 3036 | 226 | 481 |
| dashboard | ARENA translate only (no cull) | 263198 | 29656 | 56793 |

##### layout200: set_ink x200 (one call each), sorted positions

| shape | Bough instr | Bough ns | Bough cyc | Arena instr | Arena ns | Arena cyc | instr ratio (Arena/Bough) | ns ratio |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| dashboard | 61747 | 7379 | 14732 | 104016 | 6203 | 16727 | 1.7x | 0.8x |
| list | 61077 | 4971 | 13591 | 103197 | 4329 | 15502 | 1.7x | 0.9x |
| flat100k | 61536 | 4813 | 17234 | 51645 | 4937 | 8666 | 0.8x | 1.0x |
| deep40 | 52288 | 37882 | 13026 | 121267 | 21882 | 54156 | 2.3x | 0.6x |

variants:

| shape | variant | instr | ns | cyc |
|---|---|---:|---:|---:|
| dashboard | set_inks batch (one call) | 43294 | 2560 | 8331 |
| list | set_inks batch (one call) | 40208 | 1810 | 6475 |
| flat100k | set_inks batch (one call) | 61068 | 13632 | 22502 |
| deep40 | set_inks batch (one call) | 35750 | 1601 | 5749 |

##### splice8-front (insert 8 + remove 8 at the front, no cull)

| shape | Bough instr | Bough ns | Bough cyc | Arena instr | Arena ns | Arena cyc | instr ratio (Arena/Bough) | ns ratio |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| dashboard | 3343 | 249 | 672 | 4695 | 755 | 1855 | 1.4x | 3.0x |
| list | 3288 | 385 | 672 | 4710 | 530 | 1669 | 1.4x | 1.4x |
| flat100k | 3879 | 340 | 765 | 6924 | 676 | 2221 | 1.8x | 2.0x |
| deep40 | 3268 | 1070 | 781 | 4752 | 930 | 1973 | 1.5x | 0.9x |

##### hit

| shape | Bough instr | Bough ns | Bough cyc | Arena instr | Arena ns | Arena cyc | instr ratio (Arena/Bough) | ns ratio |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| dashboard | 357 | 45 | 60 | 3983 | 273 | 784 | 11.2x | 6.1x |
| list | 811 | 158 | 280 | 5390 | 254 | 910 | 6.6x | 1.6x |
| flat100k | 417 | 21 | 77 | 84187 | 4951 | 17687 | 201.8x | 234.6x |
| deep40 | 521 | 56 | 156 | 12152 | 1631 | 2746 | 23.3x | 29.0x |

variants:

| shape | variant | instr | ns | cyc |
|---|---|---:|---:|---:|
| dashboard | ARENA reverse early-exit hit | 1310 | 103 | 263 |
| list | ARENA reverse early-exit hit | 2002 | 260 | 366 |
| flat100k | ARENA reverse early-exit hit | 63389 | 10997 | 11810 |
| deep40 | ARENA reverse early-exit hit | 7017 | 923 | 1820 |

##### repair3

| shape | Bough instr | Bough ns | Bough cyc | Arena instr | Arena ns | Arena cyc | instr ratio (Arena/Bough) | ns ratio |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| dashboard | 1018 | 53 | 133 | 3659 | 296 | 676 | 3.6x | 5.6x |
| list | 1127 | 63 | 161 | 3812 | 211 | 685 | 3.4x | 3.4x |
| flat100k | 1054 | 61 | 148 | 3634 | 163 | 585 | 3.4x | 2.7x |
| deep40 | 1128 | 57 | 168 | 2654 | 172 | 485 | 2.4x | 3.0x |

##### repair3+cull

| shape | Bough instr | Bough ns | Bough cyc | Arena instr | Arena ns | Arena cyc | instr ratio (Arena/Bough) | ns ratio |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| dashboard | 39314 | 2584 | 6597 | 53516 | 3765 | 13102 | 1.4x | 1.5x |
| list | 16751 | 736 | 2598 | 44069 | 3611 | 11019 | 2.6x | 4.9x |
| flat100k | 78033 | 12476 | 16588 | 751487 | 488215 | 219255 | 9.6x | 39.1x |
| deep40 | 19574 | 1575 | 3769 | 32953 | 2751 | 7599 | 1.7x | 1.7x |

##### cull(full window)

| shape | Bough instr | Bough ns | Bough cyc | Arena instr | Arena ns | Arena cyc | instr ratio (Arena/Bough) | ns ratio |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| dashboard | 7789 | 911 | 1920 | 170881 | 18601 | 34434 | 21.9x | 20.4x |
| list | 5706 | 282 | 1013 | 55444 | 3566 | 8066 | 9.7x | 12.6x |
| flat100k | 13237 | 2365 | 3079 | 348240 | 13538 | 48767 | 26.3x | 5.7x |
| deep40 | 1199 | 235 | 315 | 19291 | 952 | 3083 | 16.1x | 4.0x |

##### subtree_end

| shape | Bough instr | Bough ns | Bough cyc | Arena instr | Arena ns | Arena cyc | instr ratio (Arena/Bough) | ns ratio |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| dashboard | 101 | 12 | 16 | 117 | 5 | 15 | 1.2x | 0.4x |
| list | 89 | 3 | 13 | 189 | 36 | 34 | 2.1x | 11.0x |
| flat100k | 145 | 15 | 41 | 5316 | 327 | 959 | 36.6x | 21.9x |
| deep40 | 90 | 5 | 9 | 81 | 2 | 7 | 0.9x | 0.3x |

##### parent

| shape | Bough instr | Bough ns | Bough cyc | Arena instr | Arena ns | Arena cyc | instr ratio (Arena/Bough) | ns ratio |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| dashboard | 105 | 5 | 18 | 25 | 0 | 0 | 0.2x | 0.0x |
| list | 102 | 8 | 19 | 25 | 0 | 0 | 0.2x | 0.0x |
| flat100k | 231 | 23 | 63 | 25 | 0 | 0 | 0.1x | 0.0x |
| deep40 | 111 | 4 | 14 | 25 | 0 | 0 | 0.2x | 0.0x |

##### children(count) of a deep-ish node

| shape | Bough instr | Bough ns | Bough cyc | Arena instr | Arena ns | Arena cyc | instr ratio (Arena/Bough) | ns ratio |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| dashboard | 208 | 16 | 50 | 23 | 0 | 0 | 0.1x | 0.0x |
| list | 123 | 12 | 23 | 23 | 0 | 0 | 0.2x | 0.0x |
| flat100k | 152 | 63 | 37 | 23 | 0 | 0 | 0.2x | 0.0x |
| deep40 | 164 | 23 | 33 | 23 | 0 | 0 | 0.1x | 0.0x |

variants:

| shape | variant | instr | ns | cyc |
|---|---|---:|---:|---:|
| dashboard | children_into (one forward depth scan) | 172 | 7 | 26 |
| list | children_into (one forward depth scan) | 119 | 3 | 12 |
| flat100k | children_into (one forward depth scan) | 170 | 15 | 37 |
| deep40 | children_into (one forward depth scan) | 194 | 20 | 27 |

##### ancestors(pos) (event bubbling path)

| shape | Bough instr | Bough ns | Bough cyc | Arena instr | Arena ns | Arena cyc | instr ratio (Arena/Bough) | ns ratio |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| dashboard | 488 | 97 | 145 | 86 | 9 | 11 | 0.2x | 0.1x |
| list | 351 | 108 | 73 | 61 | 2 | 4 | 0.2x | 0.0x |
| flat100k | 1188 | 315 | 317 | 51 | 1 | 3 | 0.0x | 0.0x |
| deep40 | 754 | 248 | 405 | 262 | 36 | 99 | 0.3x | 0.1x |

##### epoch snapshot: touch_handle + freeze_into (UI turn handing a frozen replica to a render thread)

| shape | Bough instr | Bough ns | Bough cyc | Arena instr | Arena ns | Arena cyc | instr ratio (Arena/Bough) | ns ratio |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| dashboard | 2095 | 133 | 440 | 0 | 0 | 0 | 0.0x | 0.0x |
| list | 1456 | 159 | 346 | 0 | 0 | 0 | 0.0x | 0.0x |
| flat100k | 81542 | 9910 | 21908 | 0 | 0 | 0 | 0.0x | 0.0x |
| deep40 | 1019 | 108 | 264 | 0 | 0 | 0 | 0.0x | 0.0x |

variants:

| shape | variant | instr | ns | cyc |
|---|---|---:|---:|---:|
| dashboard | 80 scattered set_inks + freeze_into | 39462 | 2475 | 8114 |
| dashboard | 80 scattered set_inks, no freeze | 30329 | 3534 | 6746 |
| list | 80 scattered set_inks + freeze_into | 27755 | 5496 | 5806 |
| list | 80 scattered set_inks, no freeze | 23736 | 2203 | 5295 |
| flat100k | 80 scattered set_inks + freeze_into | 127508 | 17964 | 48860 |
| flat100k | 80 scattered set_inks, no freeze | 35379 | 2639 | 9468 |
| deep40 | 80 scattered set_inks + freeze_into | 23620 | 1326 | 4613 |
| deep40 | 80 scattered set_inks, no freeze | 21181 | 2123 | 4498 |

##### scroll-step(ins8+rm8)+cull

| shape | Bough instr | Bough ns | Bough cyc | Arena instr | Arena ns | Arena cyc | instr ratio (Arena/Bough) | ns ratio |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| list | 6492 | 416 | 1257 | 10027 | 1244 | 2121 | 1.5x | 3.0x |

##### scroll-step+translate+cull

| shape | Bough instr | Bough ns | Bough cyc | Arena instr | Arena ns | Arena cyc | instr ratio (Arena/Bough) | ns ratio |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| list | 11710 | 945 | 2521 | 143066 | 9283 | 23792 | 12.2x | 9.8x |

##### insert row at top + remove last (structure+damage only, no cull)

| shape | Bough instr | Bough ns | Bough cyc | Arena instr | Arena ns | Arena cyc | instr ratio (Arena/Bough) | ns ratio |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| list | 4429 | 613 | 888 | 5555 | 758 | 1153 | 1.3x | 1.2x |

##### translate1000

| shape | Bough instr | Bough ns | Bough cyc | Arena instr | Arena ns | Arena cyc | instr ratio (Arena/Bough) | ns ratio |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| flat100k | 9704 | 550 | 1967 | 835427 | 43022 | 141620 | 86.1x | 78.3x |

variants:

| shape | variant | instr | ns | cyc |
|---|---|---:|---:|---:|
| flat100k | translate only (no cull) | 3589 | 346 | 579 |
| flat100k | ARENA translate only (no cull) | 417244 | 54599 | 83749 |

##### translate156

| shape | Bough instr | Bough ns | Bough cyc | Arena instr | Arena ns | Arena cyc | instr ratio (Arena/Bough) | ns ratio |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| deep40 | 4106 | 208 | 746 | 63274 | 10950 | 11687 | 15.4x | 52.6x |

variants:

| shape | variant | instr | ns | cyc |
|---|---|---:|---:|---:|
| deep40 | translate only (no cull) | 1883 | 624 | 340 |
| deep40 | ARENA translate only (no cull) | 33345 | 2972 | 6234 |

##### Mount (build whole tree from specs)

| shape | nodes | Bough instr/node | Bough ns/node | Arena instr/node | Arena ns/node |
|---|---:|---:|---:|---:|---:|
| dashboard | 1474 | 151 | 10 | 565 | 27 |
| list | 459 | 195 | 34 | 554 | 37 |
| flat100k | 100001 | 152 | 12 | 397 | 45 |
| deep40 | 160 | 215 | 12 | 751 | 59 |

##### Retained memory

| shape | nodes | Bough pages | page fill | Bough bytes/node | Arena bytes/node (lean model) | arena size_of Slot |
|---|---:|---:|---:|---:|---:|---:|
| dashboard | 1474 | 24 | 96% | 34.4 | 163.5 | 96 B |
| list | 459 | 8 | 90% | 57.8 | 137.8 | 96 B |
| flat100k | 100001 | 1563 | 100% | 23.9 | 144.5 | 96 B |
| deep40 | 160 | 3 | 83% | 99.5 | 193.6 | 96 B |


## Appendix: iteration 1 (Sonnet prototype), verbatim

Crate: `crates/bough-core`. Everything below was produced by the code in this directory on an Apple M3 Pro
(aarch64, NEON), toolchain `nightly-2026-10-17` (no nightly *features* are needed: the NEON kernels use
`core::arch::aarch64`; `portable_simd` was tried and dropped).

```
export CARGO_TARGET_DIR=/Users/mileswirht/projects/bough/target/core
# NB: this box has RUSTC_WRAPPER=kache and a stable rustc ahead of rustup on PATH; for `cargo` to use the
# pinned nightly end to end I ran with:  RUSTC_WRAPPER= PATH=~/.local/share/cargo/bin:$PATH
cargo test  -p bough-core                 # debug   (differential suite ~30 s on a quiet machine, 2 min when busy)
cargo test  -p bough-core --release       # release (~15 s)
cargo run   -p bough-core --release --example core_bench      # tables below
cargo run   -p bough-core --profile dist --example core_bench # fat-LTO build, same numbers +-4%
cargo test  -p bough-core --release --features flat-prefix    # alternative prefix implementations (see 4.)
cargo test  -p bough-core --release --features lazy-prefix
```

The example is called `core_bench` (file `examples/bench.rs`) because other crates in the workspace also have an
example named `bench` and they collide in a shared target dir.

### 1. What was built

| piece | file | notes |
|---|---|---|
| `NRect = [i16;4]` negated-max form, scalar + NEON (2 rects / 128-bit op) | `src/nrect.rs` | intersect = lane max, union = lane min, `is_empty` = saturating `x0 + (-x1)` sign test, `contains_point`, translate, `Thresh` (one `lane < thresh` compare answers all four edge conditions). Domain: coordinates in [-32766, 32766]; canonical empty `[i16::MAX; 4]` is the `union` identity, absorbed by `intersect`, and rejected by every `Thresh` compare, which is what makes padding slots free. Slice kernels: `lt_mask` (64 rects -> u64 in 94 instructions), `union_all`, fused `translate_union`. |
| 64-slot SoA pages, type-stable pool | `src/page.rs` | 1,536 B, 64-aligned: depth u16 / ink NRect / stamp u32 / kind u8 / payload u32 / handle u32 columns (23 B per slot) + version + id. Padding slots hold the sentinels (depth 0xFFFF, EMPTY ink, stamp 0). Freed pages are reset and go to a free list, never to the allocator; `version` only grows (bumped on alloc, free and every structural edit). |
| Directory (two levels), position addressing | `src/bough.rs`, `src/prefix.rs` | SoA summaries: page ptr, id, union ink, max_stamp, min_depth, len. Global index -> (page, slot) through prefix counts: see section 4 for the three implementations that were measured. |
| Operations | `src/bough.rs` | `insert_run` (bulk, end/front-biased splits keeping `min(2*MIN, CAP/2)` = 16 on the receiving side, balanced for middle inserts), `remove_run` (+ merge of small neighbours), `move_run` / `reparent_run`, `translate_run` (SIMD add on contiguous ink columns), `set_ink` (old ink captured to damage before overwrite, new after), `touch` (+ `touch_handle`), `subtree_end`, `parent`, `children`, `hit`, `cull`, `changed_since`, stable `Handle {idx, generation}` table fixed up from the moved run's `handle` column. |
| Damage | `src/damage.rs` | `Plane` = 128x96 tile bitmap (16x16 px tiles, one `u128` per tile row, window up to 2048x1536) + bounding NRect; row mask `(MAX >> (128-tx1)) & (MAX << tx0)` OR-ed into rows `ty0..ty1`. Ring of 8 planes, `repair(age)` = OR of the last `age` planes (only over the bbox's rows), `changed_since(epoch)` = SIMD compare of page `max_stamp` then of record stamps. |
| Oracle | `src/oracle.rs` | `Vec<Record>` with naive O(n) / per-tile implementations of everything. |
| Baseline | `src/arena.rs` | `ArenaTree`, a lean model of zgx's `Scene` (section 3). |
| Shapes | `src/shapes.rs` | dashboard 1,474 nodes, list (20 rows x 8 nodes in a 300-node chrome, 459 nodes), flat 100,001 nodes, deep nesting 40 (160 nodes). |

Damage semantics (what the oracle checks, tile for tile): `touch` damages the record's ink; `set_ink` damages old and
new ink as two rects; `insert_run` damages the union of each group of 64 input records; `remove_run`, `move_run`
damage the union of the ink in each *page-sized chunk* of the run (whole pages use the page summary), and
`translate_run` damages old and new union per chunk. Chunk granularity is the pre-op page boundaries, so the oracle takes
`page_starts()` as input. Everything is then clamped to the window.

### 2. Correctness

* `tests/nrect.rs`: the NRect algebra against a naive normal-form implementation. Exhaustive over every pair of
  canonical rects with coordinates in -5..=5 (release; -3..=3 in debug) for intersect/union/empty/threshold test on the
  scalar *and* NEON-2 paths, every point in -6..6 against every rect in -4..4, every raw lane quadruple in -8..8 for
  `is_empty`, saturation extremes, and 3M random full-range pairs (200k in debug) plus random slice-kernel checks
  (`lt_mask`, `union_all`, `translate_union`, scalar twin vs NEON vs naive).
* `tests/differential.rs`: randomised op sequences (insert subtree / remove subtree / move+reparent / translate run /
  set_ink / touch / handle ops / next_epoch) against the oracle, **after every op**: full record sequence (including
  stamps and handles), `check_invariants()` (sentinels, canonical ink, exact summaries, prefix, handle table, depth
  well-formedness), all 8 `repair(age)` planes row-by-row (bbox and every tile bit including phantom bits), handle
  positions and stale-handle rejection after index reuse, sampled `subtree_end` / `parent` / `children` / `hit` /
  `cull` (bbox and tile-plane) / `changed_since`. Four profiles (balanced, append-heavy, scroll-step, prepend-heavy).
  Release: 3,000 sequences x 40 ops (small window), 60 x 600 ops, 150 x 60 on a 1400x800 window (tiles >= 64 need the
  high half of the `u128` rows), 12 x 60 on the full 2048x1536 window; plus a 20,000-record bulk mount peeled off in
  odd-sized runs. Debug runs scaled-down counts (400 / 8 / 20 / 2 sequences).
* `tests/shapes.rs`: Bough and ArenaTree agree on cull results and hit targets for every benchmark shape (otherwise
  the comparison would compare different work), before and after a translate; `list` scroll steps keep invariants.
* Sanity of the tests themselves: five hand-made mutations of `bough.rs` (skip a handle fix-up after a split, skip the
  fix-up of the kept page, merge pages that do not fit, skip the exact page-ink recompute on `set_ink`, ...) were each
  caught by the differential suite; the one that survived (front-biased split sizing -> balanced) is
  behaviourally equivalent by design.
* All of the above pass in debug and release, and with each of the three prefix implementations.

### 3. Baseline: `ArenaTree`

Read from zgx `docs/architecture.md` ("Retained fibers and layout", "Damage and composition") and
`crates/zgx/src/scene.rs`: arena of `Slot {generation, Option<Node>}`, `NodeId(slot, generation)`, parent id,
`Vec<NodeId>` children, **absolute** f32 bounds (a moved subtree rewrites every descendant), a lazily computed cached
*subtree ink* (`Cell`, cleared up the ancestor chain until an already-invalid ancestor, exactly `invalidate_with`),
dirty flags + a deduplicated dirty queue drained by `flush`, `damage_node` = own bounds clipped by every clipping
ancestor (the ancestor walk), `add_damage` = outward-quantised, viewport-clamped, "recent rect holds it" fast path,
join with every overlapping rect (one scan per join), **more than 64 rects collapse to the viewport** (the task's
rule; real zgx spills to a cell grid instead), a 8-frame damage history for `repair(age)`, an explicit-stack paint
walk that skips subtrees whose cached ink misses the damage, a forward hit walk collecting hits (last wins) with the
same prune (as `hit_test_walk`); a reverse early-exit hit is reported as a variant.

Where the model is *kinder* to zgx than reality: the node is lean (96 B per slot; the real zgx `Node` carries style,
measurement caches, effects, transform, ... so its footprint and cache misses are much larger), layout/measure cost
is not charged to the insert/remove/scroll operations, and the >64-rects policy is cheaper than the grid. Every
Arena number is therefore a lower bound on zgx's.

### 4. Results

Method: `ri_instructions` / `ri_cycles` from `proc_pid_rusage(getpid(), RUSAGE_INFO_V4)`, single thread, the cost of
an empty loop iteration (5 instructions) subtracted. Each cell is a batch of >= 15 ms repeated 7 times;
instructions = median (they are deterministic to ~1%), ns and cycles = best batch. **ns is noisy**: other agents were
compiling on this machine while these ran, so trust instructions and treat ns ratios as indicative. Each "frame"
includes `next_epoch()` / `end_frame()`. Bough writes go through handles (what a binding holds); the `touch(pos)`
variant is in the raw output. Cull in the headline rows is against the repair region's bounding box; the tile-plane
variant is in the raw output (section 7).

#### 4.1 Headline: instructions and ns per operation, release build

(Arena/Bough ratio in bold = how many times fewer instructions Bough retires; < 1 means Bough loses.)

| operation | shape | Bough instr | Arena instr | instr ratio | Bough ns | Arena ns | ns ratio |
|---|---|---:|---:|---:|---:|---:|---:|
| single paint write, then cull(repair 1) | dashboard | 965 | 4,302 | **4.5x** | 50 | 499 | 10.1x |
| single paint write, then cull(repair 1) | list | 854 | 6,380 | **7.5x** | 179 | 1,124 | 6.3x |
| single paint write, then cull(repair 1) | flat100k | 3,782 | 139,310 | **36.8x** | 205 | 14,393 | 70.3x |
| single paint write, then cull(repair 1) | deep40 | 1,194 | 13,929 | **11.7x** | 87 | 1,325 | 15.3x |
| 80 scattered paint writes, then cull(repair 1) | dashboard | 29,388 | 241,459 | **8.2x** | 2,408 | 23,689 | 9.8x |
| 80 scattered paint writes, then cull(repair 1) | list | 17,989 | 136,032 | **7.6x** | 2,166 | 11,358 | 5.2x |
| 80 scattered paint writes, then cull(repair 1) | flat100k | 50,796 | 546,364 | **10.8x** | 5,031 | 52,520 | 10.4x |
| 80 scattered paint writes, then cull(repair 1) | deep40 | 13,401 | 91,030 | **6.8x** | 866 | 12,184 | 14.1x |
| translate a 1,016-node subtree + cull | dashboard | 25,142 | 500,121 | **19.9x** | 1,897 | 27,640 | 14.6x |
| splice: insert 8 + remove 8 at the front (no cull) | dashboard | 3,029 | 4,683 | **1.5x** | 181 | 352 | 1.9x |
| splice: insert 8 + remove 8 at the front (no cull) | list | 2,980 | 4,725 | **1.6x** | 292 | 315 | 1.1x |
| splice: insert 8 + remove 8 at the front (no cull) | flat100k | 3,315 | 6,924 | **2.1x** | 225 | 593 | 2.6x |
| splice: insert 8 + remove 8 at the front (no cull) | deep40 | 2,931 | 4,722 | **1.6x** | 165 | 466 | 2.8x |
| hit test (1 point) | dashboard | 343 | 3,995 | **11.6x** | 57 | 338 | 5.9x |
| hit test (1 point) | list | 363 | 5,373 | **14.8x** | 42 | 408 | 9.8x |
| hit test (1 point) | flat100k | 2,641 | 83,606 | **31.7x** | 182 | 6,778 | 37.2x |
| hit test (1 point) | deep40 | 351 | 12,140 | **34.6x** | 31 | 924 | 29.5x |
| repair(3) alone | dashboard | 1,000 | 3,671 | **3.7x** | 54 | 206 | 3.8x |
| repair(3) alone | list | 1,183 | 3,821 | **3.2x** | 113 | 329 | 2.9x |
| repair(3) alone | flat100k | 1,082 | 3,641 | **3.4x** | 60 | 184 | 3.1x |
| repair(3) alone | deep40 | 1,158 | 2,667 | **2.3x** | 84 | 195 | 2.3x |
| repair(3) + cull | dashboard | 17,577 | 53,511 | **3.0x** | 1,454 | 4,491 | 3.1x |
| repair(3) + cull | list | 7,501 | 44,124 | **5.9x** | 615 | 3,497 | 5.7x |
| repair(3) + cull | flat100k | 36,840 | 764,636 | **20.8x** | 1,636 | 59,374 | 36.3x |
| repair(3) + cull | deep40 | 3,766 | 32,874 | **8.7x** | 274 | 1,991 | 7.3x |
| cull(full window) | dashboard | 18,733 | 170,854 | **9.1x** | 1,222 | 14,069 | 11.5x |
| cull(full window) | list | 7,457 | 55,404 | **7.4x** | 496 | 4,608 | 9.3x |
| cull(full window) | flat100k | 40,989 | 348,696 | **8.5x** | 2,452 | 19,659 | 8.0x |
| cull(full window) | deep40 | 2,713 | 19,275 | **7.1x** | 185 | 944 | 5.1x |
| subtree_end(pos) | dashboard | 101 | 116 | **1.2x** | 8 | 8 | 1.1x |
| subtree_end(pos) | list | 89 | 189 | **2.1x** | 6 | 11 | 1.9x |
| subtree_end(pos) | flat100k | 145 | 5,307 | **36.6x** | 12 | 389 | 32.0x |
| subtree_end(pos) | deep40 | 90 | 81 | **0.9x** | 10 | 4 | 0.4x |
| parent(pos) | dashboard | 105 | 26 | **0.2x** | 11 | 1 | 0.1x |
| parent(pos) | list | 101 | 26 | **0.3x** | 16 | 1 | 0.0x |
| parent(pos) | flat100k | 231 | 26 | **0.1x** | 18 | 1 | 0.0x |
| parent(pos) | deep40 | 111 | 26 | **0.2x** | 5 | 1 | 0.2x |
| children(pos) iteration (count) | dashboard | 210 | 24 | **0.1x** | 31 | 3 | 0.1x |
| children(pos) iteration (count) | list | 123 | 24 | **0.2x** | 9 | 0 | 0.1x |
| children(pos) iteration (count) | flat100k | 152 | 24 | **0.2x** | 13 | 1 | 0.0x |
| children(pos) iteration (count) | deep40 | 164 | 24 | **0.1x** | 19 | 0 | 0.0x |
| virtual-list scroll step: insert row (8) at top + remove last row + cull | list | 4,335 | 9,914 | **2.3x** | 404 | 1,364 | 3.4x |
| scroll step + translate the 20 mounted rows + cull | list | 10,303 | 142,880 | **13.9x** | 553 | 9,684 | 17.5x |
| virtual-list scroll step, structure+damage only (no cull) | list | 3,503 | 5,434 | **1.6x** | 203 | 647 | 3.2x |
| translate a 1,000-node subtree + cull | flat100k | 29,348 | 835,786 | **28.5x** | 1,928 | 82,094 | 42.6x |
| translate the 156-node level-1 subtree + cull | deep40 | 6,526 | 63,035 | **9.7x** | 426 | 3,189 | 7.5x |

Reading it:

* Everything that is a **scan or a streaming write** wins by 4-37x in instructions: a single paint write + repair-1
  cull is 0.85-1.2 K instructions on 459-1,474-node trees (4.5-11.7x), 3.8 K on 100 k nodes (37x, because
  the Arena must rebuild the cached ink of the 1,000-child group and the 100-child root on every write); 80 scattered writes 6.8-10.8x;
  translating a ~1,000-node subtree 20-28x (10x for the 156-node one); hit test 12-35x.
* The **virtual-list scroll step** (the task's headline list shape) is 2.3x fewer instructions with the cull and
  1.6x without it (3.5 K vs 5.4 K): structure editing in Bough is memmoves of six columns + summary refreshes +
  prefix deltas; the Arena's is a few allocations and a `Vec::insert(0)`. Translating the mounted rows along with it
  (what a real scroll does when rows are positioned absolutely) is 14x.
* **Where Bough loses**: `parent(pos)` is ~105 instructions against a pointer load (26): 4x worse; `children` iteration
  (re-derived with `subtree_end` hops, ~120-210 instructions for a handful of children) loses to walking a materialised `Vec`;
  `subtree_end` on the 40-deep chain is a tie (90 vs 81). These are the costs of "no pointers": every query starts with a
  binary search for (page, slot).

#### 4.2 Page-start prefix counts: three implementations, measured (Bough instructions)

`prefix.rs` has the flat array maintained eagerly with a NEON suffix add (`--features flat-prefix`), the same array
rebuilt lazily from a watermark on the next lookup (`--features lazy-prefix`), and the default two-level
`BlockedPrefix` (block starts + block-local starts: a length change touches <= 32 + n/32 entries). Release, instr/op:

| operation | shape | **blocked (default)** | flat eager | flat lazy | Arena |
|---|---|---:|---:|---:|---:|
| splice: insert 8 + remove 8 at the front (no cull) | dashboard | 3,029 | 2,951 | 3,332 | 4,683 |
| splice: insert 8 + remove 8 at the front (no cull) | list | 2,980 | 2,903 | 3,163 | 4,725 |
| splice: insert 8 + remove 8 at the front (no cull) | flat100k | 3,315 | 8,402 | 25,001 | 6,924 |
| splice: insert 8 + remove 8 at the front (no cull) | deep40 | 2,931 | 2,846 | 3,038 | 4,722 |
| virtual-list scroll step, structure+damage only (no cull) | list | 3,503 | 3,334 | 3,372 | 5,434 |
| virtual-list scroll step: insert row (8) at top + remove last row + cull | list | 4,335 | 4,159 | 4,206 | 9,914 |
| translate a 1,016-node subtree + cull | dashboard | 25,142 | 25,381 | 24,735 | 500,121 |
| translate a 1,000-node subtree + cull | flat100k | 29,348 | 29,421 | 28,976 | 835,786 |
| subtree_end(pos) | flat100k | 145 | 142 | 142 | 5,307 |
| subtree_end(pos) | dashboard | 101 | 99 | 99 | 116 |
| hit test (1 point) | flat100k | 2,641 | 2,647 | 2,651 | 83,606 |
| single paint write, then cull(repair 1) | flat100k | 3,782 | 3,802 | 3,805 | 139,310 |

Conclusions: lazy loses outright for edits because every edit begins with a locate (insert needs `locate(pos-1)`), so
each edit pays a full O(pages) rebuild (25 K instructions per splice on 1,563 pages versus 8.4 K eager); queries are
the same in all three (lookup is a ~11-step branchless binary search either way). Flat eager is fine up to a few
dozen pages but its suffix add costs ~1.7 K instructions per length change on 1,563 pages and made the 100 k front splice *lose*
to the Arena (8.4 K vs 6.9 K). The two-level version fixes that (3.3 K, 2.1x better than the Arena) and costs 75-170
instructions (3-5%) more than flat-eager on the small trees' splice/scroll rows, so it is the default only because the
100 k case is worth far more than that. Page *insertion/removal* (split/merge) is still O(pages)
(directory column shifts, `page_dir` reindex, prefix rebuild); it happens once per ~32-56 inserted records.

#### 4.3 `dist` (fat LTO, codegen-units=1) vs `release` (thin LTO), Bough/Arena instructions

| operation | shape | release Bough | dist Bough | release Arena | dist Arena |
|---|---|---:|---:|---:|---:|
| single paint write, then cull(repair 1) | dashboard | 965 | 962 | 4,302 | 4,213 |
| single paint write, then cull(repair 1) | list | 854 | 842 | 6,380 | 6,171 |
| single paint write, then cull(repair 1) | flat100k | 3,782 | 3,779 | 139,310 | 135,956 |
| single paint write, then cull(repair 1) | deep40 | 1,194 | 1,176 | 13,929 | 13,725 |
| 80 scattered paint writes, then cull(repair 1) | dashboard | 29,388 | 28,097 | 241,459 | 242,613 |
| 80 scattered paint writes, then cull(repair 1) | list | 17,989 | 17,393 | 136,032 | 136,988 |
| 80 scattered paint writes, then cull(repair 1) | flat100k | 50,796 | 48,249 | 546,364 | 544,180 |
| 80 scattered paint writes, then cull(repair 1) | deep40 | 13,401 | 13,295 | 91,030 | 92,830 |
| translate a 1,016-node subtree + cull | dashboard | 25,142 | 24,338 | 500,121 | 506,571 |
| splice: insert 8 + remove 8 at the front (no cull) | dashboard | 3,029 | 3,031 | 4,683 | 4,545 |
| splice: insert 8 + remove 8 at the front (no cull) | list | 2,980 | 2,970 | 4,725 | 4,582 |
| splice: insert 8 + remove 8 at the front (no cull) | flat100k | 3,315 | 3,322 | 6,924 | 6,782 |
| splice: insert 8 + remove 8 at the front (no cull) | deep40 | 2,931 | 2,933 | 4,722 | 4,586 |
| hit test (1 point) | dashboard | 343 | 344 | 3,995 | 3,914 |
| hit test (1 point) | list | 363 | 362 | 5,373 | 5,271 |
| hit test (1 point) | flat100k | 2,641 | 2,646 | 83,606 | 80,854 |
| hit test (1 point) | deep40 | 351 | 351 | 12,140 | 11,698 |
| repair(3) alone | dashboard | 1,000 | 998 | 3,671 | 3,884 |
| repair(3) alone | list | 1,183 | 1,179 | 3,821 | 4,041 |
| repair(3) alone | flat100k | 1,082 | 1,084 | 3,641 | 3,867 |
| repair(3) alone | deep40 | 1,158 | 1,156 | 2,667 | 2,804 |
| repair(3) + cull | dashboard | 17,577 | 16,504 | 53,511 | 53,125 |
| repair(3) + cull | list | 7,501 | 7,140 | 44,124 | 44,158 |
| repair(3) + cull | flat100k | 36,840 | 34,672 | 764,636 | 742,537 |
| repair(3) + cull | deep40 | 3,766 | 3,613 | 32,874 | 32,800 |
| cull(full window) | dashboard | 18,733 | 17,511 | 170,854 | 170,043 |
| cull(full window) | list | 7,457 | 7,013 | 55,404 | 55,459 |
| cull(full window) | flat100k | 40,989 | 38,452 | 348,696 | 347,039 |
| cull(full window) | deep40 | 2,713 | 2,553 | 19,275 | 19,284 |
| subtree_end(pos) | dashboard | 101 | 102 | 116 | 109 |
| subtree_end(pos) | list | 89 | 90 | 189 | 157 |
| subtree_end(pos) | flat100k | 145 | 146 | 5,307 | 3,739 |
| subtree_end(pos) | deep40 | 90 | 91 | 81 | 79 |
| parent(pos) | dashboard | 105 | 106 | 26 | 24 |
| parent(pos) | list | 101 | 102 | 26 | 24 |
| parent(pos) | flat100k | 231 | 227 | 26 | 24 |
| parent(pos) | deep40 | 111 | 111 | 26 | 24 |
| children(pos) iteration (count) | dashboard | 210 | 201 | 24 | 22 |
| children(pos) iteration (count) | list | 123 | 122 | 24 | 22 |
| children(pos) iteration (count) | flat100k | 152 | 151 | 24 | 22 |
| children(pos) iteration (count) | deep40 | 164 | 158 | 24 | 22 |
| virtual-list scroll step: insert row (8) at top + remove last row + cull | list | 4,335 | 4,201 | 9,914 | 9,656 |
| scroll step + translate the 20 mounted rows + cull | list | 10,303 | 10,019 | 142,880 | 142,888 |
| virtual-list scroll step, structure+damage only (no cull) | list | 3,503 | 3,444 | 5,434 | 5,263 |
| translate a 1,000-node subtree + cull | flat100k | 29,348 | 28,346 | 835,786 | 844,387 |
| translate the 156-node level-1 subtree + cull | deep40 | 6,526 | 6,378 | 63,035 | 63,409 |

`dist` removes ~3-7% of instructions on the cull-heavy rows (fewer spills in the page loops) and changes nothing
structurally.

#### 4.4 Memory and mount

Bytes retained per node (heap_bytes / nodes: pages, directory columns, prefix, `page_dir`, handle table, the 8 damage planes = 12 KiB
fixed) and cost of mounting the whole tree. Arena figure = slots + children `Vec` capacity + queues for the *lean* model.

##### Mount (build whole tree from specs)

| shape | nodes | Bough instr/node | Bough ns/node | Arena instr/node | Arena ns/node |
|---|---:|---:|---:|---:|---:|
| dashboard | 1474 | 151 | 8 | 547 | 28 |
| list | 459 | 185 | 20 | 537 | 45 |
| flat100k | 100001 | 148 | 9 | 369 | 55 |
| deep40 | 160 | 230 | 12 | 747 | 59 |

##### Retained memory

| shape | nodes | Bough pages | page fill | Bough bytes/node | Arena bytes/node (lean model) | arena size_of Slot |
|---|---:|---:|---:|---:|---:|---:|
| dashboard | 1474 | 24 | 96% | 37.1 | 163.5 | 96 B |
| list | 459 | 8 | 90% | 63.6 | 137.8 | 96 B |
| flat100k | 100001 | 1563 | 100% | 25.1 | 144.5 | 96 B |
| deep40 | 160 | 3 | 83% | 120.4 | 193.6 | 96 B |


Per node Bough is 23 B of columns (24 B with page slack at 100% fill) + 0.6 B of directory; the 12 KiB of damage planes
dominate the small trees (deep40: 120 B/node = 160 nodes in 3 pages + 12 KiB of planes).

### 5. Hottest inner loops (assembly)

`cargo rustc -p bough-core --release --lib -- --emit asm` (non-inlined `probe::*` entry points exist for this).

**`lt_mask` for a 64-record page (cull, hit, directory scan): 94 instructions, 1.5 per record, no tail.** Per 8 records: 2 `ldp`,
4 `cmgt` (record lane < threshold lane, i.e. all four edge conditions of the intersection or the point test at once),
3 `uminp` (fold the four lane results of each rect into one mask lane); the eight resulting vectors are narrowed
with `uzp1`, weighted, and folded by four `addp` into the 64-bit mask:

```
	dup.2d	v0, x1                     ; broadcast the 4-lane threshold
	ldp	q1, q2, [x0]               ; 4 rects
	cmgt.8h	v1, v0, v1                 ; thresh > ink  (lane-wise, 2 rects per op)
	cmgt.8h	v2, v0, v2
	ldp	q3, q4, [x0, #32]
	cmgt.8h	v3, v0, v3
	cmgt.8h	v4, v0, v4
	uminp.8h	v1, v1, v2         ; AND the 4 lanes of each rect ...
	uminp.8h	v2, v3, v4
	uminp.8h	v1, v1, v2         ; ... -> one u16 per rect, 8 rects per vector
	  ... (x4 blocks of 16 rects) ...
	uzp1.16b	v0, v5, v0         ; narrow to bytes
	and.16b	v0, v0, v1                 ; bit weights 1,2,4..128
	addp.16b	v1, v2, v3         ; 4 pairwise adds = movemask
	addp.16b	v0, v4, v0
	addp.16b	v0, v1, v0
	addp.16b	v0, v0, v0
	fmov	x0, d0                     ; 64 result bits
	ret
```
(The first version did one `addv` + shift/or per 8 rects: ~125 instructions; the `uzp1`/`addp` movemask replaced it.)

**`translate_run` inner loop (ink columns, 2 rects per iteration, fused with both union reductions): 11 instructions per pair.**
`cmeq` finds canonical-empty rects (all lanes == i16::MAX) so they stay empty; LLVM expands the select into
`bic/bic/orr` instead of one `bsl`:

```
LBB88_15:
	ldr	q4, [x1]
	sqadd.8h	v5, v4, v0         ; ink + (dx, dy, -dx, -dy), saturating
	cmeq.8h	v6, v4, v1                 ; empty? (lane == MAX)
	bic.16b	v5, v5, v6
	bic.8h	v6, #128, lsl #8
	orr.16b	v5, v6, v5                 ; select(empty, MAX, moved)
	str	q5, [x1], #16
	smin.8h	v3, v3, v4                 ; union of OLD ink  (damage)
	smin.8h	v2, v2, v5                 ; union of NEW ink  (damage + page summary)
	subs	x8, x8, #1
	b.ne	LBB88_15
```

**`union_all` over a page (page summary refresh): 64 instructions for 64 rects** (`ldp` + 2x `smin.8h` per 4 rects, no
horizontal reduction until the end).

**Structure scan (`subtree_end` / `parent`, page-local part)**: one aligned 8-lane block per step, the first masked to lanes >= `from`,
first match through a byte-narrow + `rbit/clz` (no `addv`):

```
	ldr	q1, [x13, x15]             ; 8 depths
	cmhs.8h	v1, v0, v1                 ; depth[j] <= depth[pos]
	xtn.8b	v1, v1
	fmov	x16, d1
	lsr	x16, x16, x15              ; drop lanes before `from` (shift by 8*lane)
	lsl	x15, x16, x15
	cbz	x15, next_block
	rbit	x9, x15 ; clz x9, x9 ; add x0, x8, x9, lsr #3   ; slot of first hit
```

**`Plane::add` (damage rasterisation)**: the row mask is branch-free (two 128-bit shifts via `lsl/lsr/csel` pairs on GPR
pairs, ~25 instructions), the loop over tile rows is 5 instructions per row (`ldp; orr; orr; stp; subs/b.ne`):

```
LBB127_13:
	ldp	x12, x13, [x11]
	orr	x13, x13, x9
	orr	x12, x12, x10
	stp	x12, x13, [x11], #16
	subs	x8, x8, #16
	b.ne	LBB127_13
```
The scalar `NRect` intersect/union with the window and the plane bbox cost ~35 instructions of `cmp/csel` because the
rects arrive in GPRs; a 64-bit NEON `smax/smin` would halve that (not done; see 8.).

### 6. What surprised me

1. **Lazy prefix is a loss, not a win.** Every edit starts with a locate, so "lazy" just moves the rebuild next to the
   edit and does it twice per insert+remove (25 K vs 8.4 K instructions). The right fix was not laziness but a
   two-level representation: 3.3 K and *better* than the Arena at 1,563 pages.
2. **Cull cost is per surviving page, not per record.** At 94 instructions a page and 1.1 per directory entry, a single
   paint write is ~190 instructions of write + ~800 of cull: the cull is 80% of the frame. In dashboard/list, page 0 (root background
   + header) always survives, and the damage *bounding box* of scattered writes is nearly the whole window, so 80
   writes cost ~15 K of writes plus ~14 K of cull over ~1,000 surviving records (a full-window cull is 19 K). The precise answer, the tile plane,
   costs 4x more in per-record tile tests (121 K for the same frame). Precision and speed pull in different
   directions until the plane is used as the *index* (see 8.1).
3. **Pointers win exactly one query.** `parent` is 4x worse than a pointer load; everything else that walks (subtree
   skip, hit, cull, translate, 100 k `subtree_end` where the Arena scans siblings) is far better. The Arena's wide-container weakness
   (zgx's own doc admits it) is dramatic: one paint write under a 1,000-child group is 139 K instructions
   to re-union the cached ink.
4. **Exact tile equality is testable only if damage chunking is defined.** "Union the page summary" (the design's
   removal rule) makes the damage tile set depend on page boundaries; the oracle therefore needs `page_starts()`.
   That is a feature (cheaper than per-record) but it must be part of the contract.
5. **The canonical-empty trick pays for itself everywhere**: padding slots, `translate` (a single `cmeq` against MAX),
   page summaries and the cull test need no special case, and the "any lane == MAX" property is only valid because
   the coordinate domain is clipped to +-32766 (otherwise `-x1` could collide with MAX).
6. **Toolchain trap:** `RUSTC_WRAPPER=kache` invokes a `rustc` from PATH (stable), so `#![feature]` silently fails
   under the pinned-nightly `cargo`; and `cargo test` rebuilds the *examples* with the test feature set, which left a
   stale lazy-prefix `core_bench` binary behind a default `cargo build` (caught by printing the prefix variant in the
   bench header).

### 7. Raw benchmark output (release, default features)

#### Per-operation cost (Bough vs ArenaTree), loop baseline subtracted

##### write1+cull(repair1)

| shape | Bough instr | Bough ns | Bough cyc | Arena instr | Arena ns | Arena cyc | instr ratio (Arena/Bough) | ns ratio |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| dashboard | 965 | 50 | 178 | 4302 | 499 | 1247 | 4.5x | 10.1x |
| list | 854 | 179 | 227 | 6380 | 1124 | 1934 | 7.5x | 6.3x |
| flat100k | 3782 | 205 | 657 | 139310 | 14393 | 42211 | 36.8x | 70.3x |
| deep40 | 1194 | 87 | 239 | 13929 | 1325 | 3974 | 11.7x | 15.3x |

variants:

| shape | variant | instr | ns | cyc |
|---|---|---:|---:|---:|
| dashboard | touch(pos) instead of handle | 1004 | 111 | 235 |
| dashboard | tile-plane cull | 1889 | 155 | 450 |
| dashboard | write only (touch_handle+next_epoch) | 194 | 8 | 27 |
| dashboard | ARENA write only (mark_paint+flush) | 354 | 26 | 87 |
| list | touch(pos) instead of handle | 875 | 140 | 242 |
| list | tile-plane cull | 1930 | 235 | 446 |
| list | write only (touch_handle+next_epoch) | 198 | 30 | 42 |
| list | ARENA write only (mark_paint+flush) | 329 | 41 | 71 |
| flat100k | touch(pos) instead of handle | 3865 | 353 | 800 |
| flat100k | tile-plane cull | 4172 | 280 | 785 |
| flat100k | write only (touch_handle+next_epoch) | 193 | 13 | 27 |
| flat100k | ARENA write only (mark_paint+flush) | 315 | 15 | 54 |
| deep40 | touch(pos) instead of handle | 1207 | 105 | 280 |
| deep40 | tile-plane cull | 9807 | 615 | 1711 |
| deep40 | write only (touch_handle+next_epoch) | 194 | 9 | 27 |
| deep40 | ARENA write only (mark_paint+flush) | 545 | 81 | 199 |

##### write80+cull(repair1)

| shape | Bough instr | Bough ns | Bough cyc | Arena instr | Arena ns | Arena cyc | instr ratio (Arena/Bough) | ns ratio |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| dashboard | 29388 | 2408 | 5714 | 241459 | 23689 | 52192 | 8.2x | 9.8x |
| list | 17989 | 2166 | 3675 | 136032 | 11358 | 34185 | 7.6x | 5.2x |
| flat100k | 50796 | 5031 | 10088 | 546364 | 52520 | 130251 | 10.8x | 10.4x |
| deep40 | 13401 | 866 | 2603 | 91030 | 12184 | 28028 | 6.8x | 14.1x |

variants:

| shape | variant | instr | ns | cyc |
|---|---|---:|---:|---:|
| dashboard | tile-plane cull | 121153 | 5613 | 20077 |
| list | tile-plane cull | 56744 | 4801 | 11535 |
| flat100k | tile-plane cull | 230287 | 21884 | 41575 |
| deep40 | tile-plane cull | 36386 | 3577 | 6618 |

##### translate1016

| shape | Bough instr | Bough ns | Bough cyc | Arena instr | Arena ns | Arena cyc | instr ratio (Arena/Bough) | ns ratio |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| dashboard | 25142 | 1897 | 4876 | 500121 | 27640 | 90066 | 19.9x | 14.6x |

variants:

| shape | variant | instr | ns | cyc |
|---|---|---:|---:|---:|
| dashboard | translate only (no cull) | 11825 | 960 | 2281 |
| dashboard | ARENA translate only (no cull) | 263101 | 16179 | 45588 |

##### splice8-front (insert 8 + remove 8 at the front, no cull)

| shape | Bough instr | Bough ns | Bough cyc | Arena instr | Arena ns | Arena cyc | instr ratio (Arena/Bough) | ns ratio |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| dashboard | 3029 | 181 | 595 | 4683 | 352 | 824 | 1.5x | 1.9x |
| list | 2980 | 292 | 738 | 4725 | 315 | 924 | 1.6x | 1.1x |
| flat100k | 3315 | 225 | 730 | 6924 | 593 | 1378 | 2.1x | 2.6x |
| deep40 | 2931 | 165 | 554 | 4722 | 466 | 951 | 1.6x | 2.8x |

##### hit

| shape | Bough instr | Bough ns | Bough cyc | Arena instr | Arena ns | Arena cyc | instr ratio (Arena/Bough) | ns ratio |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| dashboard | 343 | 57 | 83 | 3995 | 338 | 801 | 11.6x | 5.9x |
| list | 363 | 42 | 101 | 5373 | 408 | 1008 | 14.8x | 9.8x |
| flat100k | 2641 | 182 | 586 | 83606 | 6778 | 19875 | 31.7x | 37.2x |
| deep40 | 351 | 31 | 80 | 12140 | 924 | 2210 | 34.6x | 29.5x |

variants:

| shape | variant | instr | ns | cyc |
|---|---|---:|---:|---:|
| dashboard | ARENA reverse early-exit hit | 1305 | 99 | 280 |
| list | ARENA reverse early-exit hit | 2008 | 122 | 383 |
| flat100k | ARENA reverse early-exit hit | 63391 | 5989 | 12701 |
| deep40 | ARENA reverse early-exit hit | 6985 | 548 | 1577 |

##### repair3

| shape | Bough instr | Bough ns | Bough cyc | Arena instr | Arena ns | Arena cyc | instr ratio (Arena/Bough) | ns ratio |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| dashboard | 1000 | 54 | 158 | 3671 | 206 | 660 | 3.7x | 3.8x |
| list | 1183 | 113 | 203 | 3821 | 329 | 757 | 3.2x | 2.9x |
| flat100k | 1082 | 60 | 193 | 3641 | 184 | 605 | 3.4x | 3.1x |
| deep40 | 1158 | 84 | 196 | 2667 | 195 | 500 | 2.3x | 2.3x |

##### repair3+cull

| shape | Bough instr | Bough ns | Bough cyc | Arena instr | Arena ns | Arena cyc | instr ratio (Arena/Bough) | ns ratio |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| dashboard | 17577 | 1454 | 3545 | 53511 | 4491 | 13928 | 3.0x | 3.1x |
| list | 7501 | 615 | 1544 | 44124 | 3497 | 10246 | 5.9x | 5.7x |
| flat100k | 36840 | 1636 | 5827 | 764636 | 59374 | 214180 | 20.8x | 36.3x |
| deep40 | 3766 | 274 | 668 | 32874 | 1991 | 7095 | 8.7x | 7.3x |

##### cull(full window)

| shape | Bough instr | Bough ns | Bough cyc | Arena instr | Arena ns | Arena cyc | instr ratio (Arena/Bough) | ns ratio |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| dashboard | 18733 | 1222 | 3554 | 170854 | 14069 | 31362 | 9.1x | 11.5x |
| list | 7457 | 496 | 1364 | 55404 | 4608 | 8129 | 7.4x | 9.3x |
| flat100k | 40989 | 2452 | 6521 | 348696 | 19659 | 56842 | 8.5x | 8.0x |
| deep40 | 2713 | 185 | 489 | 19275 | 944 | 3097 | 7.1x | 5.1x |

##### subtree_end

| shape | Bough instr | Bough ns | Bough cyc | Arena instr | Arena ns | Arena cyc | instr ratio (Arena/Bough) | ns ratio |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| dashboard | 101 | 8 | 21 | 116 | 8 | 21 | 1.2x | 1.1x |
| list | 89 | 6 | 18 | 189 | 11 | 31 | 2.1x | 1.9x |
| flat100k | 145 | 12 | 39 | 5307 | 389 | 924 | 36.6x | 32.0x |
| deep40 | 90 | 10 | 16 | 81 | 4 | 12 | 0.9x | 0.4x |

##### parent

| shape | Bough instr | Bough ns | Bough cyc | Arena instr | Arena ns | Arena cyc | instr ratio (Arena/Bough) | ns ratio |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| dashboard | 105 | 11 | 25 | 26 | 1 | 3 | 0.2x | 0.1x |
| list | 101 | 16 | 21 | 26 | 1 | 2 | 0.3x | 0.0x |
| flat100k | 231 | 18 | 61 | 26 | 1 | 2 | 0.1x | 0.0x |
| deep40 | 111 | 5 | 18 | 26 | 1 | 2 | 0.2x | 0.2x |

##### children(count) of a deep-ish node

| shape | Bough instr | Bough ns | Bough cyc | Arena instr | Arena ns | Arena cyc | instr ratio (Arena/Bough) | ns ratio |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| dashboard | 210 | 31 | 66 | 24 | 3 | 3 | 0.1x | 0.1x |
| list | 123 | 9 | 27 | 24 | 0 | 2 | 0.2x | 0.1x |
| flat100k | 152 | 13 | 39 | 24 | 1 | 2 | 0.2x | 0.0x |
| deep40 | 164 | 19 | 37 | 24 | 0 | 2 | 0.1x | 0.0x |

##### scroll-step(ins8+rm8)+cull

| shape | Bough instr | Bough ns | Bough cyc | Arena instr | Arena ns | Arena cyc | instr ratio (Arena/Bough) | ns ratio |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| list | 4335 | 404 | 1067 | 9914 | 1364 | 2162 | 2.3x | 3.4x |

##### scroll-step+translate+cull

| shape | Bough instr | Bough ns | Bough cyc | Arena instr | Arena ns | Arena cyc | instr ratio (Arena/Bough) | ns ratio |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| list | 10303 | 553 | 1965 | 142880 | 9684 | 26804 | 13.9x | 17.5x |

##### insert row at top + remove last (structure+damage only, no cull)

| shape | Bough instr | Bough ns | Bough cyc | Arena instr | Arena ns | Arena cyc | instr ratio (Arena/Bough) | ns ratio |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| list | 3503 | 203 | 720 | 5434 | 647 | 1011 | 1.6x | 3.2x |

##### translate1000

| shape | Bough instr | Bough ns | Bough cyc | Arena instr | Arena ns | Arena cyc | instr ratio (Arena/Bough) | ns ratio |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| flat100k | 29348 | 1928 | 5378 | 835786 | 82094 | 160163 | 28.5x | 42.6x |

variants:

| shape | variant | instr | ns | cyc |
|---|---|---:|---:|---:|
| flat100k | translate only (no cull) | 10682 | 795 | 2137 |
| flat100k | ARENA translate only (no cull) | 416747 | 21153 | 69473 |

##### translate156

| shape | Bough instr | Bough ns | Bough cyc | Arena instr | Arena ns | Arena cyc | instr ratio (Arena/Bough) | ns ratio |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| deep40 | 6526 | 426 | 1177 | 63035 | 3189 | 11365 | 9.7x | 7.5x |

variants:

| shape | variant | instr | ns | cyc |
|---|---|---:|---:|---:|
| deep40 | translate only (no cull) | 3431 | 188 | 623 |
| deep40 | ARENA translate only (no cull) | 33355 | 2460 | 5816 |

#### Mount (build whole tree from specs)

| shape | nodes | Bough instr/node | Bough ns/node | Arena instr/node | Arena ns/node |
|---|---:|---:|---:|---:|---:|
| dashboard | 1474 | 151 | 8 | 547 | 28 |
| list | 459 | 185 | 20 | 537 | 45 |
| flat100k | 100001 | 148 | 9 | 369 | 55 |
| deep40 | 160 | 230 | 12 | 747 | 59 |

#### Retained memory

| shape | nodes | Bough pages | page fill | Bough bytes/node | Arena bytes/node (lean model) | arena size_of Slot |
|---|---:|---:|---:|---:|---:|---:|
| dashboard | 1474 | 24 | 96% | 37.1 | 163.5 | 96 B |
| list | 459 | 8 | 90% | 63.6 | 137.8 | 96 B |
| flat100k | 100001 | 1563 | 100% | 25.1 | 144.5 | 96 B |
| deep40 | 160 | 3 | 83% | 120.4 | 193.6 | 96 B |


### 8. Five structural changes to try next (in the spirit of research/04-reformulation-method.md)

1. **Damage plane as the index, not a post-filter (domain change: geometry test -> bitset AND).** Give every page a
   coarse tile signature (e.g. a 16x12 macro-tile bitset = 3 u64, OR of its records' tile footprints, maintained with
   the same min/max discipline as `ink`) and keep the damage plane's coarse level next to it. Cull becomes
   `page_sig & plane_coarse != 0` per page (3 AND/ORR instead of 1.1-1.5 instructions of rect compare *and* it is exact
   for scattered damage). This kills the "bbox of 80 scattered writes = whole window" blow-up (29 K -> a few K), makes the
   tile-plane cull cheap (only records of signalled pages are tile-tested), and reuses the plane built at write time.
   Rule bent: "cull tests rectangles".
2. **The value was there all along: version-keyed draw-list patching.** A paint write already knows (page, slot, payload).
   Keep the last draw list per surface keyed by the page versions + epoch it was built from; a stamp-only write
   leaves every version untouched, so the draw list is still valid and the frame needs *no cull at all* (just the damage
   plane): 190 instructions instead of ~1,000. Structure/geometry edits bump page versions and rebuild only the
   chunks of the list for versions that changed. Rule bent: "repair = cull".
3. **Quantised 8-bit ink lanes for the first-pass cull: 4 rects per 128-bit op instead of 2.** Store a second ink column
   in 16-px tile units relative to the page's summary origin (conservative round-out, u8x4 per record = 256 B/page);
   test 16 records per `cmgt.16b` pair, then confirm only survivors against the exact i16 ink. Halves the 94-instruction
   page scan, doubles directory-scan width. Rule bent: "ink is exact in the hot column" (exact i16 stays in the cold column).
4. **Skip links instead of depth scans for structure queries (representation change: depth+scan -> depth+relative skip).**
   Add a u16 `skip` column (subtree length, saturating with an "ask the depth scan" escape) and `parent_delta` (u8/u16 back-distance),
   maintained on run insert/remove by touching only the ancestor chain (<= depth records, found by the same SIMD scans).
   `subtree_end` and `parent` become `locate + 1 load` (~50 instructions instead of ~100) and `children` becomes
   the Arena's pointer-chase over a contiguous run; costs 2-4 B/record and an ancestor walk per structural edit.
5. **Splice whole pages, address by (page label, slot).** `move_run`, large `insert_run`/`remove_run` currently copy records
   through a staging buffer and renumber everything after them (directory shifts, `page_dir` reindex, prefix rebuild
   are O(pages)). Pages are position-independent: relink whole pages in the directory without touching their records
   (handles name page ids, so no fix-up), give each page a 64-bit order-maintenance label so ordering needs no prefix
   counts, and derive global indices only on demand with a SIMD prefix sum over `len` (or not at all for consumers that
   hold (label, slot)). Frozen/"departed" pages for transitions fall out of the same mechanism. Rule bent: "a position is
   an integer index".

Smaller things already visible in the asm: 64-bit NEON `smax/smin` for scalar `NRect` ops (halves `ring.add`), handle
table storing the page pointer directly (drops the `page_dir` hop on the write path), SIMD prefix-sum rebuild for the
rare structural case, and avoiding the six `copy_within` memmove calls for short shifts in `insert_in_page`.

### 9. Known gaps in this prototype

No clip chains / inherited-state stack yet (ink is stored final, absolute); no third directory level (100 k nodes = 1,563
pages scans at ~1.1 instr/page = the 3.8 K single-write cost; a 64-page super-summary would cut it to ~1 K); `move_run`
is copy-based; single-threaded (`Bough` is `!Sync` through `UnsafeCell`/raw pointers); only aarch64 has NEON kernels
(scalar fallbacks compile elsewhere but were not run); damage outside the window is dropped by design.
