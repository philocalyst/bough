# bough-core: first prototype results

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

## 1. What was built

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

## 2. Correctness

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

## 3. Baseline: `ArenaTree`

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

## 4. Results

Method: `ri_instructions` / `ri_cycles` from `proc_pid_rusage(getpid(), RUSAGE_INFO_V4)`, single thread, the cost of
an empty loop iteration (5 instructions) subtracted. Each cell is a batch of >= 15 ms repeated 7 times;
instructions = median (they are deterministic to ~1%), ns and cycles = best batch. **ns is noisy**: other agents were
compiling on this machine while these ran, so trust instructions and treat ns ratios as indicative. Each "frame"
includes `next_epoch()` / `end_frame()`. Bough writes go through handles (what a binding holds); the `touch(pos)`
variant is in the raw output. Cull in the headline rows is against the repair region's bounding box; the tile-plane
variant is in the raw output (section 7).

### 4.1 Headline: instructions and ns per operation, release build

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

### 4.2 Page-start prefix counts: three implementations, measured (Bough instructions)

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

### 4.3 `dist` (fat LTO, codegen-units=1) vs `release` (thin LTO), Bough/Arena instructions

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

### 4.4 Memory and mount

Bytes retained per node (heap_bytes / nodes: pages, directory columns, prefix, `page_dir`, handle table, the 8 damage planes = 12 KiB
fixed) and cost of mounting the whole tree. Arena figure = slots + children `Vec` capacity + queues for the *lean* model.

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


Per node Bough is 23 B of columns (24 B with page slack at 100% fill) + 0.6 B of directory; the 12 KiB of damage planes
dominate the small trees (deep40: 120 B/node = 160 nodes in 3 pages + 12 KiB of planes).

## 5. Hottest inner loops (assembly)

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

## 6. What surprised me

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

## 7. Raw benchmark output (release, default features)

### Per-operation cost (Bough vs ArenaTree), loop baseline subtracted

#### write1+cull(repair1)

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

#### write80+cull(repair1)

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

#### translate1016

| shape | Bough instr | Bough ns | Bough cyc | Arena instr | Arena ns | Arena cyc | instr ratio (Arena/Bough) | ns ratio |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| dashboard | 25142 | 1897 | 4876 | 500121 | 27640 | 90066 | 19.9x | 14.6x |

variants:

| shape | variant | instr | ns | cyc |
|---|---|---:|---:|---:|
| dashboard | translate only (no cull) | 11825 | 960 | 2281 |
| dashboard | ARENA translate only (no cull) | 263101 | 16179 | 45588 |

#### splice8-front (insert 8 + remove 8 at the front, no cull)

| shape | Bough instr | Bough ns | Bough cyc | Arena instr | Arena ns | Arena cyc | instr ratio (Arena/Bough) | ns ratio |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| dashboard | 3029 | 181 | 595 | 4683 | 352 | 824 | 1.5x | 1.9x |
| list | 2980 | 292 | 738 | 4725 | 315 | 924 | 1.6x | 1.1x |
| flat100k | 3315 | 225 | 730 | 6924 | 593 | 1378 | 2.1x | 2.6x |
| deep40 | 2931 | 165 | 554 | 4722 | 466 | 951 | 1.6x | 2.8x |

#### hit

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

#### repair3

| shape | Bough instr | Bough ns | Bough cyc | Arena instr | Arena ns | Arena cyc | instr ratio (Arena/Bough) | ns ratio |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| dashboard | 1000 | 54 | 158 | 3671 | 206 | 660 | 3.7x | 3.8x |
| list | 1183 | 113 | 203 | 3821 | 329 | 757 | 3.2x | 2.9x |
| flat100k | 1082 | 60 | 193 | 3641 | 184 | 605 | 3.4x | 3.1x |
| deep40 | 1158 | 84 | 196 | 2667 | 195 | 500 | 2.3x | 2.3x |

#### repair3+cull

| shape | Bough instr | Bough ns | Bough cyc | Arena instr | Arena ns | Arena cyc | instr ratio (Arena/Bough) | ns ratio |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| dashboard | 17577 | 1454 | 3545 | 53511 | 4491 | 13928 | 3.0x | 3.1x |
| list | 7501 | 615 | 1544 | 44124 | 3497 | 10246 | 5.9x | 5.7x |
| flat100k | 36840 | 1636 | 5827 | 764636 | 59374 | 214180 | 20.8x | 36.3x |
| deep40 | 3766 | 274 | 668 | 32874 | 1991 | 7095 | 8.7x | 7.3x |

#### cull(full window)

| shape | Bough instr | Bough ns | Bough cyc | Arena instr | Arena ns | Arena cyc | instr ratio (Arena/Bough) | ns ratio |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| dashboard | 18733 | 1222 | 3554 | 170854 | 14069 | 31362 | 9.1x | 11.5x |
| list | 7457 | 496 | 1364 | 55404 | 4608 | 8129 | 7.4x | 9.3x |
| flat100k | 40989 | 2452 | 6521 | 348696 | 19659 | 56842 | 8.5x | 8.0x |
| deep40 | 2713 | 185 | 489 | 19275 | 944 | 3097 | 7.1x | 5.1x |

#### subtree_end

| shape | Bough instr | Bough ns | Bough cyc | Arena instr | Arena ns | Arena cyc | instr ratio (Arena/Bough) | ns ratio |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| dashboard | 101 | 8 | 21 | 116 | 8 | 21 | 1.2x | 1.1x |
| list | 89 | 6 | 18 | 189 | 11 | 31 | 2.1x | 1.9x |
| flat100k | 145 | 12 | 39 | 5307 | 389 | 924 | 36.6x | 32.0x |
| deep40 | 90 | 10 | 16 | 81 | 4 | 12 | 0.9x | 0.4x |

#### parent

| shape | Bough instr | Bough ns | Bough cyc | Arena instr | Arena ns | Arena cyc | instr ratio (Arena/Bough) | ns ratio |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| dashboard | 105 | 11 | 25 | 26 | 1 | 3 | 0.2x | 0.1x |
| list | 101 | 16 | 21 | 26 | 1 | 2 | 0.3x | 0.0x |
| flat100k | 231 | 18 | 61 | 26 | 1 | 2 | 0.1x | 0.0x |
| deep40 | 111 | 5 | 18 | 26 | 1 | 2 | 0.2x | 0.2x |

#### children(count) of a deep-ish node

| shape | Bough instr | Bough ns | Bough cyc | Arena instr | Arena ns | Arena cyc | instr ratio (Arena/Bough) | ns ratio |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| dashboard | 210 | 31 | 66 | 24 | 3 | 3 | 0.1x | 0.1x |
| list | 123 | 9 | 27 | 24 | 0 | 2 | 0.2x | 0.1x |
| flat100k | 152 | 13 | 39 | 24 | 1 | 2 | 0.2x | 0.0x |
| deep40 | 164 | 19 | 37 | 24 | 0 | 2 | 0.1x | 0.0x |

#### scroll-step(ins8+rm8)+cull

| shape | Bough instr | Bough ns | Bough cyc | Arena instr | Arena ns | Arena cyc | instr ratio (Arena/Bough) | ns ratio |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| list | 4335 | 404 | 1067 | 9914 | 1364 | 2162 | 2.3x | 3.4x |

#### scroll-step+translate+cull

| shape | Bough instr | Bough ns | Bough cyc | Arena instr | Arena ns | Arena cyc | instr ratio (Arena/Bough) | ns ratio |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| list | 10303 | 553 | 1965 | 142880 | 9684 | 26804 | 13.9x | 17.5x |

#### insert row at top + remove last (structure+damage only, no cull)

| shape | Bough instr | Bough ns | Bough cyc | Arena instr | Arena ns | Arena cyc | instr ratio (Arena/Bough) | ns ratio |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| list | 3503 | 203 | 720 | 5434 | 647 | 1011 | 1.6x | 3.2x |

#### translate1000

| shape | Bough instr | Bough ns | Bough cyc | Arena instr | Arena ns | Arena cyc | instr ratio (Arena/Bough) | ns ratio |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| flat100k | 29348 | 1928 | 5378 | 835786 | 82094 | 160163 | 28.5x | 42.6x |

variants:

| shape | variant | instr | ns | cyc |
|---|---|---:|---:|---:|
| flat100k | translate only (no cull) | 10682 | 795 | 2137 |
| flat100k | ARENA translate only (no cull) | 416747 | 21153 | 69473 |

#### translate156

| shape | Bough instr | Bough ns | Bough cyc | Arena instr | Arena ns | Arena cyc | instr ratio (Arena/Bough) | ns ratio |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| deep40 | 6526 | 426 | 1177 | 63035 | 3189 | 11365 | 9.7x | 7.5x |

variants:

| shape | variant | instr | ns | cyc |
|---|---|---:|---:|---:|
| deep40 | translate only (no cull) | 3431 | 188 | 623 |
| deep40 | ARENA translate only (no cull) | 33355 | 2460 | 5816 |

### Mount (build whole tree from specs)

| shape | nodes | Bough instr/node | Bough ns/node | Arena instr/node | Arena ns/node |
|---|---:|---:|---:|---:|---:|
| dashboard | 1474 | 151 | 8 | 547 | 28 |
| list | 459 | 185 | 20 | 537 | 45 |
| flat100k | 100001 | 148 | 9 | 369 | 55 |
| deep40 | 160 | 230 | 12 | 747 | 59 |

### Retained memory

| shape | nodes | Bough pages | page fill | Bough bytes/node | Arena bytes/node (lean model) | arena size_of Slot |
|---|---:|---:|---:|---:|---:|---:|
| dashboard | 1474 | 24 | 96% | 37.1 | 163.5 | 96 B |
| list | 459 | 8 | 90% | 63.6 | 137.8 | 96 B |
| flat100k | 100001 | 1563 | 100% | 25.1 | 144.5 | 96 B |
| deep40 | 160 | 3 | 83% | 120.4 | 193.6 | 96 B |


## 8. Five structural changes to try next (in the spirit of research/04-reformulation-method.md)

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

## 9. Known gaps in this prototype

No clip chains / inherited-state stack yet (ink is stored final, absolute); no third directory level (100 k nodes = 1,563
pages scans at ~1.1 instr/page = the 3.8 K single-write cost; a 64-page super-summary would cut it to ~1 K); `move_run`
is copy-based; single-threaded (`Bough` is `!Sync` through `UnsafeCell`/raw pointers); only aarch64 has NEON kernels
(scalar fallbacks compile elsewhere but were not run); damage outside the window is dropped by design.
