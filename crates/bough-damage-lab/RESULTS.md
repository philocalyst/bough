# bough damage lab: results

Question: which damage representation minimises **total frame cost** = CPU to track + CPU to cull + GPU work
to repaint, across zgx's comparison workloads, on an Apple M3 Pro, for buffer ages 1–3?

Everything here was built and measured in `crates/bough-damage-lab` (self-contained, no dependency on the
other bough crates). Raw data: `results/*.csv`, `results/*.txt`; full tables: `results/tables_full.md`.
Reproduce: see the last section.

## 0. Headline

**Winner: `hw32c`.** Exact symmetric-difference support, tracked at the GPU's own unit of cost (one bit per
32×32 hardware tile, one `u128` per tile row), history as a ring of 4 such planes OR-ed over the age window,
culled by a NEON scan of the NRect column. It is consumed by the GPU as **format C**: each instance is
clipped to runs of damaged hardware tiles, so the tile-based GPU never loads or stores an undamaged tile.

| workload | age | oracle tiles | formulation | CPU instr/frame | CPU µs | hw tiles | prims | GPU µs | total µs |
|---|---:|---:|---|---:|---:|---:|---:|---:|---:|
| heavy-stream | 1 | 762 | zgx (+ retained copy) | 519,232 | 37.4 | 2,583 | 2,092 | 338.8 | 376.1 |
| heavy-stream | 1 | 762 | bbox | 139,499 | 13.0 | 3,465 | 2,694 | 107.0 | 120.0 |
| heavy-stream | 1 | 762 | exact4 | 601,169 | 32.4 | 3,420 | 1,217 | 102.9 | 135.3 |
| heavy-stream | 1 | 762 | hw32 | 335,261 | 22.0 | 3,420 | 1,696 | 103.9 | 125.9 |
| heavy-stream | 1 | 762 | hw32c | 586,334 | 35.8 | 789 | 2,977 | 40.7 | 76.5 |
| heavy-stream | 3 | 847 | zgx | 613,598 | 37.6 | 2,590 | 2,099 | 83.9 | 121.5 |
| heavy-stream | 3 | 847 | bbox | 139,509 | 10.9 | 3,465 | 2,694 | 107.0 | 117.9 |
| heavy-stream | 3 | 847 | exact4 | 605,963 | 39.9 | 3,459 | 1,381 | 104.2 | 144.1 |
| heavy-stream | 3 | 847 | hw32 | 338,281 | 14.2 | 3,459 | 1,767 | 105.0 | 119.2 |
| heavy-stream | 3 | 847 | hw32c | 589,476 | 25.3 | 915 | 3,138 | 44.2 | 69.5 |
| heavy-scroll | 1 | 990 | zgx (+ retained copy) | 40,264 | 2.5 | 990 | 1,053 | 296.9 | 299.3 |
| heavy-scroll | 1 | 990 | bbox | 39,930 | 2.4 | 990 | 1,053 | 41.9 | 44.3 |
| heavy-scroll | 1 | 990 | exact4 | 534,495 | 30.9 | 990 | 1,053 | 41.9 | 72.8 |
| heavy-scroll | 1 | 990 | hw32 | 71,519 | 4.6 | 990 | 1,054 | 41.9 | 46.4 |
| heavy-scroll | 1 | 990 | hw32c | 132,078 | 13.3 | 990 | 1,967 | 43.7 | 57.0 |
| heavy-scroll | 3 | 967 | zgx | 40,516 | 4.7 | 990 | 1,053 | 41.9 | 46.5 |
| heavy-scroll | 3 | 967 | bbox | 39,975 | 2.4 | 990 | 1,053 | 41.9 | 44.2 |
| heavy-scroll | 3 | 967 | exact4 | 542,290 | 23.2 | 990 | 1,053 | 41.9 | 65.0 |
| heavy-scroll | 3 | 967 | hw32 | 72,338 | 4.7 | 990 | 1,054 | 41.9 | 46.6 |
| heavy-scroll | 3 | 967 | hw32c | 132,529 | 6.7 | 990 | 1,968 | 43.7 | 50.4 |
| heavy-both | 1 | 1,612 | zgx (+ retained copy) | 552,224 | 34.7 | 3,510 | 2,714 | 363.2 | 397.9 |
| heavy-both | 1 | 1,612 | bbox | 140,411 | 12.7 | 3,510 | 2,714 | 108.2 | 120.8 |
| heavy-both | 1 | 1,612 | exact4 | 1,082,186 | 51.4 | 3,510 | 2,066 | 106.9 | 158.3 |
| heavy-both | 1 | 1,612 | hw32 | 373,669 | 22.9 | 3,510 | 2,527 | 107.8 | 130.7 |
| heavy-both | 1 | 1,612 | hw32c | 637,872 | 52.5 | 1,640 | 4,409 | 64.8 | 117.3 |
| heavy-both | 3 | 1,655 | zgx | 629,750 | 38.4 | 3,510 | 2,714 | 108.2 | 146.5 |
| heavy-both | 3 | 1,655 | bbox | 140,979 | 9.3 | 3,510 | 2,714 | 108.2 | 117.5 |
| heavy-both | 3 | 1,655 | exact4 | 1,083,943 | 48.5 | 3,510 | 2,201 | 107.2 | 155.7 |
| heavy-both | 3 | 1,655 | hw32 | 376,172 | 34.2 | 3,510 | 2,583 | 107.9 | 142.1 |
| heavy-both | 3 | 1,655 | hw32c | 638,555 | 39.1 | 1,741 | 4,551 | 67.6 | 106.7 |
| lab-stream | 1 | 50 | zgx (+ retained copy) | 22,886 | 1.3 | 432 | 603 | 199.1 | 200.5 |
| lab-stream | 1 | 50 | bbox | 22,753 | 1.3 | 432 | 603 | 27.0 | 28.3 |
| lab-stream | 1 | 50 | exact4 | 138,312 | 8.0 | 408 | 68 | 25.3 | 33.3 |
| lab-stream | 1 | 50 | hw32 | 75,085 | 5.1 | 408 | 146 | 25.5 | 30.6 |
| lab-stream | 1 | 50 | hw32c | 103,765 | 9.4 | 50 | 258 | 16.8 | 26.1 |
| lab-stream | 3 | 53 | zgx | 22,933 | 0.1 | 432 | 603 | 27.0 | 27.1 |
| lab-stream | 3 | 53 | bbox | 22,473 | 0.0 | 432 | 603 | 27.0 | 27.0 |
| lab-stream | 3 | 53 | exact4 | 139,652 | 5.9 | 408 | 75 | 25.4 | 31.3 |
| lab-stream | 3 | 53 | hw32 | 75,752 | 4.6 | 408 | 155 | 25.5 | 30.1 |
| lab-stream | 3 | 53 | hw32c | 105,019 | 5.6 | 53 | 271 | 16.9 | 22.5 |
| lab-scroll | 1 | 1,468 | zgx (+ retained copy) | 18,233 | 1.1 | 1,508 | 451 | 225.7 | 226.8 |
| lab-scroll | 1 | 1,468 | bbox | 17,902 | 0.7 | 1,508 | 451 | 53.6 | 54.3 |
| lab-scroll | 1 | 1,468 | exact4 | 252,391 | 11.0 | 1,508 | 451 | 53.6 | 64.6 |
| lab-scroll | 1 | 1,468 | hw32 | 32,522 | 1.7 | 1,508 | 451 | 53.6 | 55.3 |
| lab-scroll | 1 | 1,468 | hw32c | 56,433 | 2.9 | 1,508 | 963 | 54.6 | 57.5 |
| lab-scroll | 3 | 1,468 | zgx | 18,430 | 0.8 | 1,508 | 451 | 53.6 | 54.4 |
| lab-scroll | 3 | 1,468 | bbox | 18,082 | 1.1 | 1,508 | 451 | 53.6 | 54.7 |
| lab-scroll | 3 | 1,468 | exact4 | 257,269 | 14.0 | 1,508 | 451 | 53.6 | 67.6 |
| lab-scroll | 3 | 1,468 | hw32 | 33,184 | 1.9 | 1,508 | 451 | 53.6 | 55.5 |
| lab-scroll | 3 | 1,468 | hw32c | 56,947 | 2.9 | 1,508 | 963 | 54.6 | 57.5 |
| animate | 1 | 84 | zgx (+ retained copy) | 34,908 | 2.4 | 125 | 53 | 190.4 | 192.7 |
| animate | 1 | 84 | bbox | 5,142 | 0.3 | 265 | 79 | 21.8 | 22.1 |
| animate | 1 | 84 | exact4 | 23,414 | 1.4 | 265 | 37 | 21.7 | 23.1 |
| animate | 1 | 84 | hw32 | 9,152 | 0.5 | 265 | 38 | 21.7 | 22.2 |
| animate | 1 | 84 | hw32c | 13,223 | 0.7 | 84 | 81 | 17.3 | 17.9 |
| animate | 3 | 97 | zgx | 52,501 | 3.5 | 147 | 60 | 18.8 | 22.3 |
| animate | 3 | 97 | bbox | 5,232 | 0.3 | 269 | 80 | 21.9 | 22.2 |
| animate | 3 | 97 | exact4 | 25,292 | 1.3 | 269 | 42 | 21.8 | 23.1 |
| animate | 3 | 97 | hw32 | 9,913 | 0.5 | 269 | 43 | 21.8 | 22.3 |
| animate | 3 | 97 | hw32c | 14,158 | 0.7 | 97 | 89 | 17.6 | 18.3 |
| blink | 1 | 2 | zgx (+ retained copy) | 1,316 | 0.1 | 2 | 2 | 187.2 | 187.2 |
| blink | 1 | 2 | bbox | 1,100 | 0.0 | 2 | 2 | 15.1 | 15.1 |
| blink | 1 | 2 | exact4 | 2,982 | 0.2 | 2 | 2 | 15.1 | 15.2 |
| blink | 1 | 2 | hw32 | 1,850 | 0.1 | 2 | 2 | 15.1 | 15.1 |
| blink | 1 | 2 | hw32c | 2,896 | 0.1 | 2 | 3 | 15.1 | 15.2 |
| blink | 3 | 2 | zgx | 1,577 | 0.1 | 2 | 2 | 15.1 | 15.1 |
| blink | 3 | 2 | bbox | 1,162 | 0.2 | 2 | 2 | 15.1 | 15.2 |
| blink | 3 | 2 | exact4 | 3,359 | 0.2 | 2 | 2 | 15.1 | 15.2 |
| blink | 3 | 2 | hw32 | 2,514 | 0.1 | 2 | 2 | 15.1 | 15.2 |
| blink | 3 | 2 | hw32c | 3,560 | 0.2 | 2 | 3 | 15.1 | 15.2 |
| vlist-scroll | 1 | 1,745 | zgx (+ retained copy) | 62,942 | -5.3 | 2,250 | 1,544 | 217.8 | 212.5 |
| vlist-scroll | 1 | 1,745 | bbox | 62,942 | -4.8 | 2,250 | 1,544 | 74.3 | 69.5 |
| vlist-scroll | 1 | 1,745 | exact4 | 859,675 | 35.0 | 2,228 | 1,544 | 73.8 | 108.8 |
| vlist-scroll | 1 | 1,745 | hw32 | 132,901 | 8.9 | 2,250 | 1,544 | 74.3 | 83.2 |
| vlist-scroll | 1 | 1,745 | hw32c | 207,043 | 11.4 | 2,250 | 3,313 | 77.9 | 89.3 |
| vlist-scroll | 3 | 1,959 | zgx | 63,772 | -0.5 | 2,250 | 1,544 | 74.3 | 73.8 |
| vlist-scroll | 3 | 1,959 | bbox | 63,452 | -0.1 | 2,250 | 1,544 | 74.3 | 74.2 |
| vlist-scroll | 3 | 1,959 | exact4 | 865,625 | 35.6 | 2,240 | 1,544 | 74.1 | 109.7 |
| vlist-scroll | 3 | 1,959 | hw32 | 133,535 | 7.9 | 2,250 | 1,544 | 74.3 | 82.3 |
| vlist-scroll | 3 | 1,959 | hw32c | 208,167 | 14.9 | 2,250 | 3,312 | 77.9 | 92.8 |
| vlist-fling | 1 | 1,831 | zgx (+ retained copy) | 65,915 | 2.6 | 2,250 | 1,619 | 217.9 | 220.5 |
| vlist-fling | 1 | 1,831 | bbox | 65,479 | 2.7 | 2,250 | 1,619 | 74.5 | 77.2 |
| vlist-fling | 1 | 1,831 | exact4 | 890,892 | 53.3 | 2,231 | 1,619 | 74.0 | 127.3 |
| vlist-fling | 1 | 1,831 | hw32 | 142,926 | 9.0 | 2,250 | 1,619 | 74.5 | 83.5 |
| vlist-fling | 1 | 1,831 | hw32c | 219,970 | 11.3 | 2,250 | 3,313 | 77.9 | 89.2 |
| vlist-fling | 3 | 1,937 | zgx | 66,093 | 6.0 | 2,250 | 1,619 | 74.5 | 80.4 |
| vlist-fling | 3 | 1,937 | bbox | 65,591 | 4.4 | 2,250 | 1,619 | 74.5 | 78.9 |
| vlist-fling | 3 | 1,937 | exact4 | 896,704 | 38.2 | 2,236 | 1,619 | 74.1 | 112.3 |
| vlist-fling | 3 | 1,937 | hw32 | 143,783 | 10.6 | 2,250 | 1,619 | 74.5 | 85.1 |
| vlist-fling | 3 | 1,937 | hw32c | 221,066 | 13.6 | 2,250 | 3,314 | 77.9 | 91.5 |
| vlist-jump | 1 | 1,786 | zgx (+ retained copy) | 64,287 | 4.7 | 2,250 | 1,572 | 217.8 | 222.5 |
| vlist-jump | 1 | 1,786 | bbox | 63,818 | 3.3 | 2,250 | 1,572 | 74.4 | 77.7 |
| vlist-jump | 1 | 1,786 | exact4 | 896,251 | 37.4 | 2,237 | 1,572 | 74.1 | 111.5 |
| vlist-jump | 1 | 1,786 | hw32 | 142,167 | 10.0 | 2,250 | 1,572 | 74.4 | 84.4 |
| vlist-jump | 1 | 1,786 | hw32c | 217,538 | 15.3 | 2,250 | 3,347 | 77.9 | 93.3 |
| vlist-jump | 3 | 1,999 | zgx | 64,066 | 2.4 | 2,250 | 1,572 | 74.4 | 76.8 |
| vlist-jump | 3 | 1,999 | bbox | 63,682 | 2.5 | 2,250 | 1,572 | 74.4 | 76.9 |
| vlist-jump | 3 | 1,999 | exact4 | 904,001 | 41.7 | 2,244 | 1,572 | 74.3 | 115.9 |
| vlist-jump | 3 | 1,999 | hw32 | 142,924 | 9.4 | 2,250 | 1,572 | 74.4 | 83.8 |
| vlist-jump | 3 | 1,999 | hw32c | 217,990 | 11.5 | 2,250 | 3,346 | 77.9 | 89.4 |


Columns: CPU = track + cull, with the tree-update baseline subtracted. Instructions come from
`ri_instructions` and are the primary metric. µs is wall time, min of 3–5 runs, with ±2–8 µs noise on
small costs (it can go negative after baseline subtraction). "hw tiles" = 32×32 tiles the GPU touches. "prims" = instances submitted. GPU µs comes from the
calibrated model in §1.4. zgx at age 1 is zgx as shipped: it repairs a retained texture and blits it whole to
the drawable each frame (+255 µs at 2560×1600, measured on the quiet machine; 336 µs under load), and at ages 2–3 the row shows its region algebra
with buffer-age union and no copy. "oracle tiles" = hardware tiles whose pixels actually differ between the
surface's old content and the new frame: the floor for any formulation.

What the numbers say:

1. **On a tile-based GPU, damage precision is worthless unless geometry is clipped to it.** Apple's GPU
   loads and stores exactly the 32×32 tiles that binned geometry touches. An empty load/store pass costs
   15 µs, the same pass with one 16 px quad also 15 µs, clear+store 132–163 µs; scissor clips before
   binning; a full-screen quad whose every fragment is discarded still costs 94 µs (§1.4).
   - So "scissor the bbox, mask the region" (format S, what zgx and every rect or tile damage system does)
     touches 3,420 of 4,000 tiles on heavy-stream even when the region is 250 K px (exact4): the card
     panels and the background are redrawn whole under the mask.
   - Clipping geometry to damaged tile runs (format C) cuts that to 789 tiles, against an oracle of 762.
   - Modelled GPU time: 103 µs → 41 µs. This one consumption change is worth more than every tracking
     refinement combined.
2. **Tracking finer than the hardware tile buys nothing on the GPU.** Under format C, exact4, exact8,
   exact16 and exact32 touch the same 789 tiles. Finer tracking only reduces fragments, which overlap tile
   bandwidth. The 32 px plane is 800 B against exact4's 160 KB, so OR, clear and test are a handful of
   word operations.
3. **Exact symmetric difference beats bounds 2–9× on the GPU.**
   - heavy-stream, age 1: 789 vs 1,523 tiles (bounds at the same tile size).
   - lab-stream: 50 vs 432 tiles. In pixels, 12.5 K exact vs 404 K bounds against a 10.2 K oracle: the GPUI
     campaign's 34× gap, reproduced.
4. **A scrolled subtree is one primitive.** Per-slot exactness for moved content costs O(moved): 0.3–0.9 M
   instructions per frame on the scroll traces. `hw32` marks clip ∩ (old ∪ new) content bounds instead,
   O(1).
   - heavy-scroll: 8 K tracking instructions, at the oracle's 990 tiles.
   - Losing regime, kept: vlist's uniform rows leave 22% of tiles unchanged, which the group rule repaints
     (2,250 vs 1,745 tiles: about +13 µs GPU against −25 µs CPU).
5. **Net-change exactness (content signatures) is oracle-exact but costs 3–10× the CPU.**
   - `sig32` hits exactly 762 tiles on heavy-stream and 0 for blink at age 2 (A→B→A).
   - It costs 0.6–2.9 M instructions per frame.
   - Computed on the GPU instead: 0 CPU instructions and a bit-identical tile set (0 mismatches over 57 K
     tile rows). But at UI frame sizes the passes are dominated by dispatch latency at idle clocks: 13–49 µs
     for signatures alone, 76–284 µs with binning.
6. **Against zgx.** Totals are CPU + modelled GPU µs, age 1, `hw32c` vs zgx as shipped:

   | trace | `hw32c` | zgx | ratio |
   |---|---:|---:|---:|
   | heavy-stream | 77 | 376 | 4.9× |
   | heavy-both | 117 | 398 | 3.4× |
   | lab-stream | 26 | 201 | 7.7× |
   | animate | 18 | 193 | 11× |
   | blink | 15 | 187 | 12× |

   Most of zgx's GPU cost is its per-frame retained-texture copy (255 µs at 2560×1600, measured). Against
   zgx's damage algebra alone (age 3, no copy), the margin is 1.2–1.7×:

   | trace | `hw32c` | zgx (no copy) |
   |---|---:|---:|
   | heavy-stream | 70 | 122 |
   | heavy-both | 107 | 147 |
   | lab-stream | 22 | 27 |
   | animate | 18 | 22 |

   On the scroll traces everything is oracle-bound (the whole scrolled clip changes): `hw32` ties zgx
   without its copy (heavy-scroll 46 vs 47 µs), and `hw32c` is 4–10 µs behind.

   **CPU is not decisively better.** In instructions per frame, track + cull:

   | trace | `hw32` (format S) | `hw32c` (format C) | zgx region algebra |
   |---|---:|---:|---:|
   | heavy-stream | 335 K | 586 K | 519 K |
   | lab-stream | 75 K | 104 K | 23 K |
   | animate | 9 K | 13 K | 35 K |
   | vlist-scroll | 133 K | 207 K | 63 K |

   Exact per-glyph diffing and format C emission cost CPU that the GPU repays several times over, but the
   CPU line is not a win. §5.4 gives the next structural step: GPU-side run expansion, which brings format C
   down to `hw32`'s CPU cost. zgx's own measured damage phase, tree walks included, is 1.4 M instructions
   (DESIGN.md §0); only its region algebra and cull are modelled here.

## 1. Method

### 1.1 Workloads as change traces

Each workload is a pure function `tick → full scene` (`src/workloads.rs`). The geometry, text and motion are
copied from zgx's `comparisons/workload/src/lib.rs` (`sample`, `tile_value`, `card_value`, `bar_height`,
`row_*`, `status`, the log format, `dot_opacity`, `shimmer_alpha`, and vlist's `hash`/`lines`/`text`/`delta`/
`jump`). The layouts are copied from the zgx adapters (`heavy_view.rs`, `component_workload.rs`,
`animation_view.rs`, `blink_workload.rs`, `vlist_view.rs`). The window is @2x device pixels, and all
geometry is snapped to device pixels.

Text uses a fixed advance of 0.577 em (DejaVu Sans 13 px ≈ 7.5 px/char). It has one glyph primitive per
non-space char, with box = advance × 1.2 em.

The scene is a **fixed, paint-ordered slot array**:

- Virtual lists recycle a ring of row-slot groups: row r goes to group r mod ring, as a recycling list does.
- Text nodes have phantom glyph capacity; trailing empty slots are trimmed from events.

Diffing consecutive scenes yields the write-time events a retained framework knows:

- **Node events:** old and new state of each slot.
- **Group events:** a scroll container's content moved by (dx, dy), listing the moved slots.

| trace | window px | live prims | frames | node events/frame (slots) | moved slots/frame |
|---|---|---:|---:|---:|---:|
| heavy-stream | 2560×1600 | 2763 (of 3840 slots) | 600 | 799.7 (1751) | 0 |
| heavy-scroll | 2560×1600 | 2770 (of 3840 slots) | 600 | 8.2 (57) | 1078 |
| heavy-both | 2560×1600 | 2762 (of 3840 slots) | 600 | 807.5 (1805) | 1081 |
| lab-stream | 1920×1440 | 1084 (of 1536 slots) | 600 | 1.0 (734) | 0 |
| lab-scroll | 1920×1440 | 483 (of 1536 slots) | 600 | 2.0 (35) | 465 |
| animate | 1920×1440 | 88 (of 128 slots) | 600 | 35.0 (35) | 0 |
| blink | 1920×1440 | 2 (of 64 slots) | 120 | 1.0 (1) | 0 |
| vlist-scroll | 1440×1600 | 1666 (of 26368 slots) | 600 | 1.2 (276) | 1566 |
| vlist-fling | 1440×1600 | 1666 (of 26368 slots) | 600 | 2.1 (367) | 1677 |
| vlist-jump | 1440×1600 | 1666 (of 26368 slots) | 600 | 2.5 (388) | 1643 |

### 1.2 Formulations

All formulations get the same events and the same retained substrate. The substrate is an NRect `(x0, y0,
−x1, −y1)` i16 coverage column in paint order, plus 64-slot page summaries (union coverage). Maintaining it
is the tree's job; its cost is measured as stage A and subtracted.

| name | support | representation | history (buffer age) | cull |
|---|---|---|---|---|
| `full` | everything | — | — | all |
| `zgx` | node old ∪ new bounds | zgx `add_damage_as`: 4-entry containment cache, merge-on-intersect list; >64 rects → 16 px `DamageGrid` (zgx's 8 pt cell), `rects()` run-merge at flush | union of per-frame lists | zgx-gpu `cluster_damage` (≤16 groups, cells doubling from 32 px) + `merge_damage` (NEAR_PIXELS) → one scissored batch per region |
| `zgx-vp` | same | list, >64 → collapse to viewport (the older zgx) | same | same |
| `bbox` | bounds | one NRect per frame (Chromium `DamageAccumulator`) | union of `age` boxes | NRect test |
| `tileT` T=4..32 | bounds | bitplane, rows padded to u64 words, tile bbox | ring of 4, OR | page summary → slot NRect → span bits |
| `tile2L` / `exact2L` | bounds / exact | 128 px super-tiles over 16 px tiles | ring, OR both | coarse then fine |
| `exactT` | **exact symmetric difference** per slot (incl. moved groups) | bitplane | ring, OR | as tileT |
| `stamps16` | exact slots → *stamps* | per-slot u32 epoch + per-page max (never cleared); old support → per-epoch departure plane | query "stamp > e" scan + OR departures | as tileT |
| `sigT` | net content | per-tile additive u64 signature of what each slot paints in the tile; per-surface snapshot | touched planes ∪ → compare signatures | as tileT |
| `xgT` | exact nodes, **group = one primitive** | bitplane | ring, OR | as tileT |
| `xgT c`, `xs32`, `xb32`, `tstamp32`, `bands32`, `hw32`, `hw32c` | see §2 (the reformulation chain) | | | |

**Exact symmetric difference** (`forms::exact_slot`). Each rule is exact for the oracle renderer's pixel
functions (§1.5):

- **Content change** (key, kind or radius): old coverage ∪ new coverage, or once if they are equal.
- **Uniform rect geometry change:** `old ⊕ new` as at most 4 strips per side.
- **Glyph geometry change:** old ∪ new if the content origin moved; `cov ⊕ cov` if only the clip changed.
- **Rounded rect geometry change:** `old ⊕ new` ∪ the four r×r corner squares of old and of new.

### 1.3 CPU measurement

`src/bench.rs` replays a trace single-threaded at QoS user-interactive. It reads
`proc_pid_rusage(getpid(), RUSAGE_INFO_V4).ri_instructions` and `mach_absolute_time` around the loop over
all presented frames (frames ≥ ring size; the first `ring` frames are full draws), and subtracts the
counter pair's own cost. There are three stages:

- **A** = tree apply only.
- **B** = A + `record`.
- **C** = B + `repair` (region for the surface of age = ring size, plus cull to a paint-ordered draw list).

Track = B − A and cull = C − B, each the min of N reps, divided by presented frames. Instruction counts are
reproducible to about ±1%. ns at the 1–5 µs scale carries about ±2 µs of noise, which is why some tiny
values come out negative.

### 1.4 GPU cost model, calibrated

`calib/metal_calib.swift` and `calib/metal_calib2.swift` measure command-buffer GPU time
(`gpuEndTime − gpuStartTime`, median of 60–80) on a 2560×1600 BGRA8 texture:

| experiment | µs | reading |
|---|---:|---|
| load/store pass, no draws | 15.3–22 | pass fixed cost; untouched tiles are not loaded or stored |
| same + one 16 px quad | 16.3 | one tile touched ≈ free |
| clear/store pass, no draws | 132–163 | a clear must write every tile |
| full-screen quad (load/store) | 103 (quiet) – 337 (SDF shader, loaded) | ~4,000 tiles; depends on content and shader |
| full-screen quad ×4 overdraw | 325 (vs 337 for ×1) | fragments overlap bandwidth |
| 1,000 × 256 px quads (65 M fragments) | 1,456 | ~22 ps/fragment when fragment-bound |
| 4,000 1 px quads, one per 32 px cell | 70–74 (quiet) / 108–156 (loaded) | ≈ 14 ns per touched tile (flat content) |
| 4,000 16 px quads in 32 px cells vs 32 px quads | 278 vs 267 | **hardware tile = 32×32** |
| 16,000 1 px quads, 4 per 32 px cell | 96–100 (quiet) | extra instance ≈ 2 ns |
| 100,000 off-screen instances | 212 | ≈ 2 ns per culled instance |
| full-screen quad, scissor 64×64 | 15.0 | **scissor clips before binning** |
| full-screen quad, every fragment discarded | 92–117 | **discard does not un-touch a tile** |
| noise content, 4,000 tiles touched | 210–213 (quiet) | incompressible: ≈ 49 ns/tile |
| blit 2560×1600 (zgx's retained-texture copy) | 255 (quiet) – 336 | paid by zgx every frame |
| blit 256×256 | 5.25 | 82 ns per 32 px tile: a copy costs more than a redraw |

Model, from three quiet-machine runs (`results/metal_calib2_quiet*.txt`): `gpu = 15 (if anything is drawn) +
max(0.025 × tiles, 0.000005 × fragments) + 0.002 × instances` (µs). Tile cost ranges from 14 ns (flat content)
to 49 ns (noise); §5.7 gives the sensitivity. Two consumption formats are evaluated for every formulation from the verifier's exact counts:

- **S:** one draw (zgx: one per region) under scissor = region bbox, with fragments masked to the region.
  Tiles = tiles under each drawn prim's coverage ∩ scissor.
- **C:** geometry clipped to runs of damaged 32 px tiles. Tiles = region tiles; instances = (prim, run)
  pairs, or one unclipped instance when a prim's whole span is damaged.

### 1.5 Correctness: differential, literal, exhaustive over the traces

`src/verify.rs`, run in lockstep per (trace, ring):

1. Every frame, render the reference with the dumb oracle (`render_full`: every primitive into a software
   framebuffer of 32-bit hash colours). Pixel functions:
   - **SOLID:** `key` (opaque).
   - **TINT:** `mix(below, key)`.
   - **ROUND:** uniform interior `mix(below, key)`; corner squares `mix(below, key ^ h(dx, dy))`.
   - **GLYPH:** `mix(below, key ^ h(x − ink.x0, y − ink.y0))`.

   `mix` is order-sensitive, so a missing, extra, duplicated or reordered primitive changes the pixel.
2. For every formulation, **literally repair its own ring surface of age `ring`**: rasterise its region to
   a pixel mask, clear the region to background, and redraw its draw list batch by batch, clipped to batch
   scissor ∩ mask. Then compare the whole surface to the reference with `memcmp`. A mismatch is a bug; the
   surface is resynced so later frames are checked independently.
3. Compute the true change set Δ = {p : ref_n(p) ≠ ref_{n−ring}(p)} (the "oracle").

Coverage: 10 traces × 600 frames (blink 120) × ages 1, 2, 3 × 28 formulations. **0 failures**
(`results/verify_v1.log`, `verify_v2.log`, `verify_v3.log`, `verify_v4.log`). The verifier was itself
validated by mutation: deliberately broken formulations are all rejected on frame 1–2.

| mutant | caught on |
|---|---|
| new bounds only | heavy-both, ring 1 |
| rounded-corner squares dropped | heavy-both, ring 1 |
| glyph moves treated as XOR | lab-scroll, ring 1; vlist-fling |
| age history ignored | heavy-both, lab-scroll, animate, vlist, ring 2 |
| last draw-list entry dropped | all |

A mutant that culls by bbox only (a superset) correctly passes. The GPU-computed signatures were checked
bit-for-bit against the CPU `sig32` tile sets: 0 mismatches over 57,000 tile rows.

## 2. The reformulation chain (what each attempt bent)

Each attempt changes the representation or the domain, not constants. Losers are kept.

Numbers are heavy-stream at age 1 unless stated: instructions per frame as track + cull, then hardware tiles
(the oracle is 762).

**A0. First full comparison** (`verify_v1`, `bench_final`). The set was full, zgx, zgx-vp, bbox, tile4/8/16/32,
tile2L, exact4/8/16, stamps16 and sig16, scored with the brief's "pixels × overdraw" GPU model.

| | pixels | instructions |
|---|---|---|
| exact4 (pixel winner) | 250 K px, oracle 185 K | 308 K + 293 K |
| bbox (CPU winner) | 3.4 M px | 45 K + 95 K |
| zgx | 2.5 M px | 381 K + 138 K |

Rules the winners bend:

- **exact:** "damage = union of touched bounds" becomes "damage = where coverage differs" (symmetric
  difference: strips for resized uniform rects, changed glyphs only, colour change once).
- **bbox:** precision is traded for one NRect `min` per change.

**A1. Calibrate the GPU: the unit of cost is the 32×32 hardware tile.** This bent the brief's own cost model.
Metal measurements (§1.4) showed that an Apple TBDR pass costs ≈ 15 µs + 14–49 ns per *touched* tile, plus
~2 ns per instance. Fragments overlap bandwidth, scissor clips before binning, and discard or stencil does not
un-touch a tile. Re-scoring A0 under that model collapses every formulation together: in "scissor the bbox,
mask the region" (format S), exact4's 250 K px region still touches 3,420 of 4,000 tiles, because each drawn
primitive is binned whole.

**A2. Consume damage as clipped geometry (format C).** Rule bent: "the draw list is a list of primitives".
It becomes (primitive, damaged-tile-run) instances, with a primitive left unclipped when its whole tile
span is damaged.

| trace | format S tiles | format C tiles | oracle | GPU µs (S → C) |
|---|---:|---:|---:|---|
| heavy-stream | 3,420 | 789 | 762 | 104 → 41 |
| lab-stream | 408 | 50 | 50 | 25.5 → 16.8 |
| animate | 265 | 84 | 84 | 21.7 → 17.3 |

Price: emission on the CPU, measured on the final plane as 176 K + 411 K (C) vs 176 K + 159 K (S). Large
primitives under scattered damage split into one instance per row run: 2,977 instances for 1,696 prims.

**A3. Track at the hardware tile (exact32).** Rule bent: "finer damage is better". Under format C, exact4,
exact8, exact16 and exact32 all touch 789 tiles. The 32 px plane is 800 B against exact4's 160 KB, and
tracking is cheaper: 279 K vs 334 K.

**A4. A moved subtree is one primitive (xg32).** Rule bent: "exactness is per primitive". Per-slot exactness
for scrolled content costs O(moved slots). The group rule marks clip ∩ old and clip ∩ new content bounds,
O(1).

| trace | track instr (exact32 → xg32) | tiles: exact32 / xg32 / oracle |
|---|---|---|
| heavy-scroll | 299 K → 8 K | 990 / 990 / 990 |
| lab-scroll | 123 K → 5 K | 1,468 / 1,508 / 1,468 |
| vlist-scroll | 453 K → 45 K | 1,745 / 2,250 / 1,745 |

Losing regime, kept: in vlist, uniform row backgrounds and image placeholders leave 22% of the window's tiles
unchanged under scroll. The group rule repaints them for about +20 µs GPU against about −25 µs CPU, a wash
on total. A finer group rule that stays O(1) (moved group minus its uniform interior) is open.

**A5. Versions, not flags, at two granularities.**

- **`stamps16`:** per-slot u32 epoch + per-page max, never cleared, with old support in per-epoch departure
  planes. Rule bent: "a frame's damage is a set" becomes "every record carries its epoch". Write time is
  cheaper (160 K vs exact16 283 K), but the query scans pages and stamps and marks the *whole* new coverage
  (329 K vs 207 K), and that coverage is not the symmetric difference: 1,096 tiles vs 789. **Loses on CPU
  and GPU.**
- **`tstamp32`:** a u32 epoch per hardware tile; write is a store, never cleared, any number of consumers.
  The query scans all tiles: blink 18 K instructions vs 1.2 K for planes, heavy +50 K. **Loses** as the
  frame path. It is the right API for unbounded consumers (inspector, accessibility, remote mirror).

**A6. Net change: content signatures, then on the GPU.** Rule bent: "damage is a log of writes" becomes
"damage is a difference of states". Each tile keeps an additive 64-bit hash of what each slot paints in it,
updated incrementally; each surface keeps the signatures it was drawn with.

- `sig32` = oracle exactly: 762 tiles on heavy, **0 on blink at age 2** (A→B→A).
- It costs 0.6–2.9 M instructions per frame (3–10× hw32) for hashing every tile under every changed slot.
- **GPU-computed (`calib/gpu_damage.swift`):** "the GPU already has the instance buffer", so it hashes all
  instances per (instance, tile row), atomic-adds two 32-bit lanes, compares per tile, and compacts
  (instance, run) pairs in paint order for one indirect draw.
  - Exactness: 0 CPU instructions, and the tile set is bit-identical to the CPU's (0 mismatches over
    57,000 tile rows, five traces, ages 1–2).
  - Cost: GPU 13–49 µs for the signature pass alone, 76–284 µs with binning. These are dominated by
    per-dispatch latency at idle clocks; even 88 instances (animate) cost 78 µs. **Loses on total at UI
    scale.** It wins CPU% outright.

**A7. Interval sets per scanline band (bands32).** Rule bent: "damage is a tile set" becomes "exact
x-intervals per 32 px band" in min/max algebra. Exact x saves only fragments, which are cheap on TBDR. The
8-interval cap fuses on heavy (1,960 tiles vs 789), and tracking costs 2–3× xg32. **Loses.**

**A8. Cull as prefix counts (xs32).** Rule bent: "the cull tests bits" becomes an O(1) "any damaged tile in
this rect?" from a summed-area table. **Loses:** the table build is ~40 K per frame at heavy's full-screen
bbox, and the four-load query is no cheaper than a 1–2-row bit test (cull 291 K vs 196 K).

**A9. A byte per tile (xb32).** Rule bent: bit becomes byte, so a span test is one unaligned u64 load and a
mask, no cross-word shifts. Cull −12%, track +10%. **Wash.**

**A10. Bend the range, then widen (hw32).** Three moves on the plane:

- **Domain bent deliberately to ≤ 4096×4096 px:** a tile row is one `u128`, so a mark is a 64-bit mask OR-ed
  into the span's rows and a span test is one AND per row.
- **NEON cull:** `vld4q_s16` deinterleaves 8 NRects; the bbox test is two `smax` and two saturating `sqadd`.
- **Widen to delete the fixup (Joffe move 3):** drop dirty-row tracking and OR/clear the whole ≤ 2 KB plane
  instead. That cut tracking 17–25% (heavy 212 K → 176 K, animate 4.4 K → 3.3 K) and cost +0.4 K only on
  blink.

Result on heavy-stream: hw32 = 176 K + 159 K, against xg32 277 K + 201 K, exact4 308 K + 293 K and zgx 381 K +
138 K.

**Rejected by calibration, not built: copy-forward / scroll blit.** The idea was to repaint only the age-1
region and copy the rest of the age window, or a scrolled clip, from the freshest surface. On this GPU a blit
costs 82 ns per 32 px tile (blit 256×256 = 5.25 µs) against 27–63 ns to repaint a touched tile, and the
full-surface blit is exactly zgx's 255–336 µs per frame. Repainting OR(age) into the surface of that age is
cheaper.


## 3. Per-workload tables (ages 1–3)

Key formulations below; every formulation is in `results/tables_full.md`. Columns: instructions and ns per
presented frame (track, cull), repaint region px, prims drawn (format S), hardware tiles under S and C,
format C instances, GPU µs (model) for S and C, and the formulation's memory.


#### heavy-stream


age 1 — oracle Δ: 184,635 px, 761.8 hw tiles (32 px), 1,817.1 tiles@16, 15,599.4 tiles@4

| form | track instr | cull instr | track ns | cull ns | region px | prims | hw tiles S | hw tiles C | inst C | GPU µs S | GPU µs C | mem B |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| full | -589 | 95,234 | -11,371 | 4,781 | 4,096,000 | 2,769 | 4,000 | 4,000 | 5,509 | 120.5 | 126.0 | 0 |
| zgx | 381,248 | 137,984 | 25,760 | 11,594 | 2,545,352 | 2,092 | 2,583 | 2,583 | 4,463 | 83.8 | 88.5 | 18432 |
| zgx-vp | 168,949 | 95,018 | -2,897 | 4,429 | 4,096,000 | 2,769 | 4,000 | 4,000 | 5,509 | 120.5 | 126.0 | 2304 |
| bbox | 44,881 | 94,618 | 6,896 | 6,097 | 3,403,777 | 2,694 | 3,465 | 3,465 | 5,335 | 107.0 | 112.3 | 88 |
| tile16 | 165,516 | 208,236 | 8,675 | 8,904 | 1,048,492 | 1,946 | 3,465 | 1,523 | 4,130 | 105.5 | 61.3 | 12000 |
| tile32 | 151,865 | 204,321 | -2,098 | 5,058 | 1,559,161 | 2,022 | 3,465 | 1,523 | 4,231 | 105.7 | 61.5 | 4000 |
| exact4 | 308,001 | 293,168 | 25,225 | 7,147 | 250,507 | 1,217 | 3,420 | 789 | 2,426 | 102.9 | 39.6 | 160000 |
| exact32 | 276,076 | 201,309 | 2,604 | 8,494 | 808,067 | 1,696 | 3,420 | 789 | 2,977 | 103.9 | 40.7 | 4000 |
| stamps16 | 166,195 | 328,990 | -3,106 | 17,520 | 739,962 | 1,593 | 3,420 | 1,096 | 3,658 | 103.7 | 49.7 | 25200 |
| sig32 | 670,830 | 247,809 | 22,769 | 5,024 | 780,071 | 1,696 | 3,420 | 762 | 2,883 | 103.9 | 39.8 | 67200 |
| xg32 | 276,148 | 201,558 | 3,450 | 9,752 | 808,067 | 1,696 | 3,420 | 789 | 2,977 | 103.9 | 40.7 | 4000 |
| tstamp32 | 198,712 | 251,169 | -265 | 14,185 | 808,067 | 1,696 | 3,420 | 789 | 2,977 | 103.9 | 40.7 | 16000 |
| bands32 | 564,529 | 539,208 | 16,933 | 54,665 | 1,838,685 | 2,277 | 3,600 | 1,960 | 4,137 | 109.6 | 72.3 | 8500 |
| hw32 | 175,822 | 159,439 | 11,184 | 10,849 | 808,067 | 1,696 | 3,420 | 789 | 2,977 | 103.9 | 40.7 | 4000 |
| hw32c | 175,542 | 410,792 | 16,633 | 19,150 | 808,067 | 1,696 | 3,420 | 789 | 2,977 | 103.9 | 40.7 | 4000 |

age 2 — oracle Δ: 224,365 px, 824.0 hw tiles (32 px), 2,017.0 tiles@16, 18,575.3 tiles@4

| form | track instr | cull instr | track ns | cull ns | region px | prims | hw tiles S | hw tiles C | inst C | GPU µs S | GPU µs C | mem B |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| full | 198 | 94,938 | -629 | 5,699 | 4,096,000 | 2,769 | 4,000 | 4,000 | 5,509 | 120.5 | 126.0 | 0 |
| zgx | 381,902 | 185,591 | 27,910 | 21,721 | 2,549,622 | 2,096 | 2,587 | 2,587 | 4,469 | 83.9 | 88.6 | 18432 |
| zgx-vp | 169,867 | 95,019 | 9,763 | 5,117 | 4,096,000 | 2,769 | 4,000 | 4,000 | 5,509 | 120.5 | 126.0 | 2304 |
| bbox | 44,750 | 94,243 | 4,346 | 3,633 | 3,403,777 | 2,694 | 3,465 | 3,465 | 5,335 | 107.0 | 112.3 | 88 |
| tile16 | 165,696 | 208,624 | 10,304 | 15,586 | 1,075,110 | 1,974 | 3,465 | 1,547 | 4,185 | 105.6 | 62.0 | 12000 |
| tile32 | 152,290 | 205,248 | 6,584 | 7,950 | 1,584,229 | 2,035 | 3,465 | 1,547 | 4,252 | 105.7 | 62.2 | 4000 |
| exact4 | 307,865 | 293,805 | 18,765 | 16,282 | 318,533 | 1,346 | 3,446 | 871 | 2,645 | 103.8 | 42.1 | 160000 |
| exact32 | 277,827 | 203,164 | 15,952 | 10,228 | 891,465 | 1,750 | 3,446 | 871 | 3,093 | 104.6 | 43.0 | 4000 |
| stamps16 | 167,581 | 344,901 | 11,054 | 27,407 | 809,189 | 1,665 | 3,446 | 1,172 | 3,789 | 104.5 | 51.9 | 25200 |
| sig32 | 670,567 | 253,973 | 26,682 | 12,718 | 843,735 | 1,728 | 3,446 | 824 | 2,960 | 104.6 | 41.5 | 99200 |
| xg32 | 276,607 | 203,696 | 12,677 | 10,526 | 891,465 | 1,750 | 3,446 | 871 | 3,093 | 104.6 | 43.0 | 4000 |
| tstamp32 | 199,565 | 254,855 | 11,215 | 13,009 | 891,465 | 1,750 | 3,446 | 871 | 3,093 | 104.6 | 43.0 | 16000 |
| bands32 | 564,899 | 577,768 | 26,534 | 44,879 | 1,934,938 | 2,336 | 3,600 | 2,049 | 4,249 | 109.7 | 74.7 | 8500 |
| hw32 | 175,396 | 161,413 | 9,554 | 11,378 | 891,465 | 1,750 | 3,446 | 871 | 3,093 | 104.6 | 43.0 | 4000 |
| hw32c | 175,555 | 413,052 | 9,447 | 25,177 | 891,465 | 1,750 | 3,446 | 871 | 3,093 | 104.6 | 43.0 | 4000 |

age 3 — oracle Δ: 227,551 px, 847.2 hw tiles (32 px), 2,053.5 tiles@16, 18,766.7 tiles@4

| form | track instr | cull instr | track ns | cull ns | region px | prims | hw tiles S | hw tiles C | inst C | GPU µs S | GPU µs C | mem B |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| full | -61 | 94,864 | 813 | 4,047 | 4,096,000 | 2,770 | 4,000 | 4,000 | 5,509 | 120.5 | 126.0 | 0 |
| zgx | 381,473 | 232,125 | 25,294 | 12,272 | 2,553,056 | 2,099 | 2,590 | 2,590 | 4,474 | 83.9 | 88.7 | 18432 |
| zgx-vp | 170,343 | 94,707 | 14,746 | 4,888 | 4,096,000 | 2,770 | 4,000 | 4,000 | 5,509 | 120.5 | 126.0 | 2304 |
| bbox | 44,946 | 94,563 | 4,451 | 6,471 | 3,403,777 | 2,694 | 3,465 | 3,465 | 5,335 | 107.0 | 112.3 | 88 |
| tile16 | 166,098 | 208,553 | 13,852 | 4,983 | 1,088,229 | 1,981 | 3,465 | 1,558 | 4,195 | 105.6 | 62.3 | 12000 |
| tile32 | 152,654 | 205,023 | 8,621 | 7,398 | 1,595,693 | 2,036 | 3,465 | 1,558 | 4,253 | 105.7 | 62.5 | 4000 |
| exact4 | 308,241 | 297,722 | 21,864 | 18,041 | 350,974 | 1,381 | 3,459 | 915 | 2,711 | 104.2 | 43.3 | 160000 |
| exact32 | 276,749 | 203,815 | 14,390 | 7,566 | 936,998 | 1,767 | 3,459 | 915 | 3,138 | 105.0 | 44.2 | 4000 |
| stamps16 | 167,611 | 351,115 | 11,533 | 38,048 | 836,506 | 1,689 | 3,459 | 1,207 | 3,823 | 104.8 | 52.8 | 25200 |
| sig32 | 671,808 | 256,570 | 32,632 | 12,084 | 867,523 | 1,728 | 3,459 | 847 | 2,961 | 104.9 | 42.1 | 131200 |
| xg32 | 277,901 | 204,383 | 32,448 | -2,445 | 936,998 | 1,767 | 3,459 | 915 | 3,138 | 105.0 | 44.2 | 4000 |
| tstamp32 | 201,096 | 256,049 | 16,533 | 10,556 | 936,998 | 1,767 | 3,459 | 915 | 3,138 | 105.0 | 44.2 | 16000 |
| bands32 | 567,466 | 618,157 | 41,879 | 73,458 | 1,979,223 | 2,358 | 3,600 | 2,091 | 4,299 | 109.7 | 75.9 | 8500 |
| hw32 | 175,720 | 162,561 | 5,818 | 8,407 | 936,998 | 1,767 | 3,459 | 915 | 3,138 | 105.0 | 44.2 | 4000 |
| hw32c | 175,573 | 413,903 | 1,246 | 24,096 | 936,998 | 1,767 | 3,459 | 915 | 3,138 | 105.0 | 44.2 | 4000 |

#### heavy-scroll


age 1 — oracle Δ: 854,889 px, 990.0 hw tiles (32 px), 3,759.9 tiles@16, 53,864.9 tiles@4

| form | track instr | cull instr | track ns | cull ns | region px | prims | hw tiles S | hw tiles C | inst C | GPU µs S | GPU µs C | mem B |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| full | -83 | 95,591 | 39 | 4,544 | 4,096,000 | 2,791 | 4,000 | 4,000 | 5,845 | 120.6 | 126.7 | 0 |
| zgx | 899 | 39,365 | 1,468 | 992 | 989,312 | 1,053 | 990 | 990 | 1,966 | 41.9 | 43.7 | 320 |
| zgx-vp | 776 | 39,261 | 53 | 1,778 | 989,312 | 1,053 | 990 | 990 | 1,966 | 41.9 | 43.7 | 320 |
| bbox | 650 | 39,280 | 500 | 1,910 | 989,312 | 1,053 | 990 | 990 | 1,966 | 41.9 | 43.7 | 88 |
| tile16 | 3,616 | 82,820 | 1,500 | 3,485 | 996,864 | 1,054 | 990 | 990 | 1,967 | 41.9 | 43.7 | 12000 |
| tile32 | 2,253 | 81,995 | 471 | 2,551 | 1,013,760 | 1,054 | 990 | 990 | 1,967 | 41.9 | 43.7 | 4000 |
| exact4 | 440,460 | 94,035 | 20,267 | 10,642 | 861,838 | 1,053 | 990 | 990 | 1,966 | 41.9 | 43.7 | 160000 |
| exact32 | 299,470 | 81,966 | 14,065 | 2,648 | 1,013,760 | 1,054 | 990 | 990 | 1,967 | 41.9 | 43.7 | 4000 |
| stamps16 | 151,332 | 184,926 | 6,475 | 8,768 | 996,864 | 1,054 | 990 | 990 | 1,967 | 41.9 | 43.7 | 25200 |
| sig32 | 820,574 | 135,623 | 34,471 | 5,922 | 1,013,760 | 1,054 | 990 | 990 | 1,967 | 41.9 | 43.7 | 67200 |
| xg32 | 7,936 | 82,319 | 267 | 3,457 | 1,013,760 | 1,054 | 990 | 990 | 1,967 | 41.9 | 43.7 | 4000 |
| tstamp32 | 9,155 | 138,398 | 478 | 8,580 | 1,013,760 | 1,054 | 990 | 990 | 1,967 | 41.9 | 43.7 | 16000 |
| bands32 | 10,623 | 201,050 | 2,687 | 7,413 | 996,864 | 1,054 | 2,640 | 990 | 1,967 | 83.1 | 43.7 | 8500 |
| hw32 | 4,109 | 67,410 | 65 | 4,503 | 1,013,760 | 1,054 | 990 | 990 | 1,967 | 41.9 | 43.7 | 4000 |
| hw32c | 4,130 | 127,948 | 697 | 12,581 | 1,013,760 | 1,054 | 990 | 990 | 1,967 | 41.9 | 43.7 | 4000 |

age 2 — oracle Δ: 824,429 px, 990.0 hw tiles (32 px), 3,889.1 tiles@16, 51,526.8 tiles@4

| form | track instr | cull instr | track ns | cull ns | region px | prims | hw tiles S | hw tiles C | inst C | GPU µs S | GPU µs C | mem B |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| full | -319 | 96,009 | -1,522 | 7,537 | 4,096,000 | 2,791 | 4,000 | 4,000 | 5,845 | 120.6 | 126.7 | 0 |
| zgx | 868 | 39,406 | -250 | 1,268 | 989,312 | 1,053 | 990 | 990 | 1,966 | 41.9 | 43.7 | 320 |
| zgx-vp | 859 | 39,299 | -463 | 891 | 989,312 | 1,053 | 990 | 990 | 1,966 | 41.9 | 43.7 | 320 |
| bbox | 637 | 39,344 | -585 | 2,504 | 989,312 | 1,053 | 990 | 990 | 1,966 | 41.9 | 43.7 | 88 |
| tile16 | 3,666 | 83,194 | -327 | 4,105 | 996,864 | 1,054 | 990 | 990 | 1,967 | 41.9 | 43.7 | 12000 |
| tile32 | 2,195 | 82,434 | -759 | 2,665 | 1,013,760 | 1,054 | 990 | 990 | 1,967 | 41.9 | 43.7 | 4000 |
| exact4 | 440,710 | 97,856 | 17,707 | 6,757 | 989,312 | 1,053 | 990 | 990 | 1,966 | 41.9 | 43.7 | 160000 |
| exact32 | 298,643 | 82,358 | 11,006 | 3,626 | 1,013,760 | 1,054 | 990 | 990 | 1,967 | 41.9 | 43.7 | 4000 |
| stamps16 | 151,240 | 185,172 | 5,339 | 8,085 | 996,864 | 1,054 | 990 | 990 | 1,967 | 41.9 | 43.7 | 25200 |
| sig32 | 820,775 | 135,273 | 38,454 | 675 | 1,013,760 | 1,054 | 990 | 990 | 1,967 | 41.9 | 43.7 | 99200 |
| xg32 | 7,814 | 82,268 | -1,187 | 4,787 | 1,013,760 | 1,054 | 990 | 990 | 1,967 | 41.9 | 43.7 | 4000 |
| tstamp32 | 9,309 | 137,541 | 702 | 4,878 | 1,013,760 | 1,054 | 990 | 990 | 1,967 | 41.9 | 43.7 | 16000 |
| bands32 | 10,242 | 204,040 | -736 | 10,628 | 996,864 | 1,054 | 2,640 | 990 | 1,967 | 83.1 | 43.7 | 8500 |
| hw32 | 4,123 | 67,553 | -87 | 2,239 | 1,013,760 | 1,054 | 990 | 990 | 1,967 | 41.9 | 43.7 | 4000 |
| hw32c | 4,189 | 128,021 | -693 | 6,399 | 1,013,760 | 1,054 | 990 | 990 | 1,967 | 41.9 | 43.7 | 4000 |

age 3 — oracle Δ: 760,496 px, 967.3 hw tiles (32 px), 3,495.6 tiles@16, 48,361.3 tiles@4

| form | track instr | cull instr | track ns | cull ns | region px | prims | hw tiles S | hw tiles C | inst C | GPU µs S | GPU µs C | mem B |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| full | 27 | 95,628 | -185 | 5,303 | 4,096,000 | 2,791 | 4,000 | 4,000 | 5,846 | 120.6 | 126.7 | 0 |
| zgx | 972 | 39,544 | 934 | 3,756 | 989,312 | 1,053 | 990 | 990 | 1,967 | 41.9 | 43.7 | 320 |
| zgx-vp | 947 | 39,429 | 259 | 1,363 | 989,312 | 1,053 | 990 | 990 | 1,967 | 41.9 | 43.7 | 320 |
| bbox | 546 | 39,429 | 222 | 2,171 | 989,312 | 1,053 | 990 | 990 | 1,967 | 41.9 | 43.7 | 88 |
| tile16 | 3,540 | 83,612 | -13 | 3,170 | 996,864 | 1,054 | 990 | 990 | 1,968 | 41.9 | 43.7 | 12000 |
| tile32 | 2,301 | 82,393 | 3 | 2,831 | 1,013,760 | 1,054 | 990 | 990 | 1,968 | 41.9 | 43.7 | 4000 |
| exact4 | 440,435 | 101,855 | 19,044 | 4,130 | 989,312 | 1,053 | 990 | 990 | 1,967 | 41.9 | 43.7 | 160000 |
| exact32 | 298,896 | 83,186 | 12,002 | 4,844 | 1,013,760 | 1,054 | 990 | 990 | 1,968 | 41.9 | 43.7 | 4000 |
| stamps16 | 151,703 | 186,274 | 7,815 | 8,182 | 996,864 | 1,054 | 990 | 990 | 1,968 | 41.9 | 43.7 | 25200 |
| sig32 | 821,165 | 135,409 | 43,669 | 202 | 1,013,760 | 1,054 | 990 | 990 | 1,968 | 41.9 | 43.7 | 131200 |
| xg32 | 8,124 | 82,364 | 1,311 | 5,999 | 1,013,760 | 1,054 | 990 | 990 | 1,968 | 41.9 | 43.7 | 4000 |
| tstamp32 | 9,259 | 137,802 | 357 | 5,598 | 1,013,760 | 1,054 | 990 | 990 | 1,968 | 41.9 | 43.7 | 16000 |
| bands32 | 10,480 | 206,257 | 318 | 9,701 | 996,864 | 1,054 | 2,640 | 990 | 1,968 | 83.1 | 43.7 | 8500 |
| hw32 | 4,284 | 68,054 | 1,205 | 3,495 | 1,013,760 | 1,054 | 990 | 990 | 1,968 | 41.9 | 43.7 | 4000 |
| hw32c | 4,402 | 128,127 | 473 | 6,199 | 1,013,760 | 1,054 | 990 | 990 | 1,968 | 41.9 | 43.7 | 4000 |

#### heavy-both


age 1 — oracle Δ: 992,763 px, 1,612.3 hw tiles (32 px), 5,242.0 tiles@16, 65,857.3 tiles@4

| form | track instr | cull instr | track ns | cull ns | region px | prims | hw tiles S | hw tiles C | inst C | GPU µs S | GPU µs C | mem B |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| full | -76 | 96,086 | -560 | 10,625 | 4,096,000 | 2,789 | 4,000 | 4,000 | 5,841 | 120.6 | 126.7 | 0 |
| zgx | 417,903 | 134,321 | 27,422 | 7,286 | 3,491,840 | 2,714 | 3,510 | 3,510 | 5,667 | 108.2 | 114.1 | 18432 |
| zgx-vp | 170,447 | 95,111 | 14,966 | 2,063 | 4,096,000 | 2,789 | 4,000 | 4,000 | 5,841 | 120.6 | 126.7 | 2304 |
| bbox | 45,324 | 95,087 | 8,897 | 3,760 | 3,449,680 | 2,714 | 3,510 | 3,510 | 5,667 | 108.2 | 114.1 | 88 |
| tile16 | 169,433 | 209,851 | 8,476 | 12,558 | 1,865,976 | 2,649 | 3,510 | 2,217 | 5,485 | 108.0 | 81.4 | 12000 |
| tile32 | 154,983 | 209,752 | 7,825 | 16,362 | 2,270,350 | 2,723 | 3,510 | 2,217 | 5,585 | 108.2 | 81.6 | 4000 |
| exact4 | 825,291 | 256,895 | 42,208 | 9,193 | 1,054,633 | 2,066 | 3,510 | 1,640 | 3,885 | 106.9 | 63.8 | 160000 |
| exact32 | 618,720 | 209,371 | 27,237 | 10,609 | 1,678,981 | 2,527 | 3,510 | 1,640 | 4,409 | 107.8 | 64.8 | 4000 |
| stamps16 | 342,675 | 415,373 | 31,459 | 13,601 | 1,651,061 | 2,439 | 3,510 | 1,947 | 5,113 | 107.6 | 73.9 | 25200 |
| sig32 | 1,610,421 | 301,738 | 75,745 | 6,432 | 1,650,985 | 2,527 | 3,510 | 1,612 | 4,314 | 107.8 | 63.9 | 67200 |
| xg32 | 333,094 | 209,617 | 16,655 | 13,998 | 1,678,981 | 2,527 | 3,510 | 1,640 | 4,409 | 107.8 | 64.8 | 4000 |
| tstamp32 | 251,652 | 284,984 | 15,680 | 13,191 | 1,678,981 | 2,527 | 3,510 | 1,640 | 4,409 | 107.8 | 64.8 | 16000 |
| bands32 | 710,062 | 551,507 | 36,532 | 47,681 | 1,840,825 | 2,456 | 3,600 | 1,960 | 4,398 | 109.9 | 72.8 | 8500 |
| hw32 | 202,366 | 171,303 | 11,584 | 11,308 | 1,678,981 | 2,527 | 3,510 | 1,640 | 4,409 | 107.8 | 64.8 | 4000 |
| hw32c | 202,709 | 435,163 | 12,188 | 40,271 | 1,678,981 | 2,527 | 3,510 | 1,640 | 4,409 | 107.8 | 64.8 | 4000 |

age 2 — oracle Δ: 994,143 px, 1,663.4 hw tiles (32 px), 5,545.6 tiles@16, 65,969.2 tiles@4

| form | track instr | cull instr | track ns | cull ns | region px | prims | hw tiles S | hw tiles C | inst C | GPU µs S | GPU µs C | mem B |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| full | 161 | 95,352 | 923 | 4,043 | 4,096,000 | 2,789 | 4,000 | 4,000 | 5,841 | 120.6 | 126.7 | 0 |
| zgx | 417,991 | 172,115 | 20,835 | 11,085 | 3,491,840 | 2,714 | 3,510 | 3,510 | 5,667 | 108.2 | 114.1 | 18432 |
| zgx-vp | 169,991 | 95,688 | 11,377 | 5,024 | 4,096,000 | 2,789 | 4,000 | 4,000 | 5,841 | 120.6 | 126.7 | 2304 |
| bbox | 45,077 | 95,178 | -3,484 | 5,226 | 3,449,680 | 2,714 | 3,510 | 3,510 | 5,667 | 108.2 | 114.1 | 88 |
| tile16 | 169,818 | 209,188 | 10,780 | 152 | 1,891,362 | 2,675 | 3,510 | 2,239 | 5,537 | 108.1 | 82.1 | 12000 |
| tile32 | 154,717 | 210,831 | 11,835 | 18,566 | 2,293,223 | 2,734 | 3,510 | 2,239 | 5,604 | 108.2 | 82.2 | 4000 |
| exact4 | 824,491 | 256,327 | 30,592 | 15,942 | 1,237,985 | 2,176 | 3,510 | 1,705 | 4,075 | 107.1 | 65.8 | 160000 |
| exact32 | 621,008 | 209,873 | 36,266 | 15,490 | 1,746,368 | 2,570 | 3,510 | 1,705 | 4,509 | 107.9 | 66.7 | 4000 |
| stamps16 | 343,728 | 427,912 | 30,499 | 26,715 | 1,708,500 | 2,494 | 3,510 | 2,007 | 5,219 | 107.7 | 75.6 | 25200 |
| sig32 | 1,614,423 | 305,881 | 111,692 | -2,025 | 1,703,310 | 2,552 | 3,510 | 1,663 | 4,382 | 107.9 | 65.3 | 99200 |
| xg32 | 333,595 | 210,145 | 17,560 | 12,602 | 1,746,368 | 2,570 | 3,510 | 1,705 | 4,509 | 107.9 | 66.7 | 4000 |
| tstamp32 | 252,221 | 289,041 | 18,384 | 26,021 | 1,746,368 | 2,570 | 3,510 | 1,705 | 4,509 | 107.9 | 66.7 | 16000 |
| bands32 | 712,446 | 590,037 | 57,713 | 43,119 | 1,935,779 | 2,510 | 3,600 | 2,050 | 4,506 | 110.0 | 75.3 | 8500 |
| hw32 | 202,575 | 172,007 | 11,690 | 10,035 | 1,746,368 | 2,570 | 3,510 | 1,705 | 4,509 | 107.9 | 66.7 | 4000 |
| hw32c | 202,372 | 435,009 | 10,711 | 23,715 | 1,746,368 | 2,570 | 3,510 | 1,705 | 4,509 | 107.9 | 66.7 | 4000 |

age 3 — oracle Δ: 928,431 px, 1,655.1 hw tiles (32 px), 5,169.6 tiles@16, 62,671.2 tiles@4

| form | track instr | cull instr | track ns | cull ns | region px | prims | hw tiles S | hw tiles C | inst C | GPU µs S | GPU µs C | mem B |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| full | -310 | 96,591 | 1,899 | 7,276 | 4,096,000 | 2,789 | 4,000 | 4,000 | 5,841 | 120.6 | 126.7 | 0 |
| zgx | 418,520 | 211,230 | 28,406 | 9,959 | 3,491,840 | 2,714 | 3,510 | 3,510 | 5,667 | 108.2 | 114.1 | 18432 |
| zgx-vp | 169,798 | 96,758 | 13,597 | 6,469 | 4,096,000 | 2,789 | 4,000 | 4,000 | 5,841 | 120.6 | 126.7 | 2304 |
| bbox | 45,384 | 95,595 | 2,780 | 6,542 | 3,449,680 | 2,714 | 3,510 | 3,510 | 5,667 | 108.2 | 114.1 | 88 |
| tile16 | 169,556 | 210,254 | 7,270 | 8,416 | 1,904,139 | 2,681 | 3,510 | 2,250 | 5,547 | 108.1 | 82.3 | 12000 |
| tile32 | 154,464 | 210,054 | 7,148 | 13,415 | 2,304,036 | 2,735 | 3,510 | 2,250 | 5,604 | 108.2 | 82.5 | 4000 |
| exact4 | 825,178 | 258,765 | 48,976 | -433 | 1,263,694 | 2,201 | 3,510 | 1,741 | 4,132 | 107.2 | 66.8 | 160000 |
| exact32 | 618,255 | 209,763 | 26,198 | 8,760 | 1,782,498 | 2,583 | 3,510 | 1,741 | 4,551 | 107.9 | 67.6 | 4000 |
| stamps16 | 341,899 | 432,345 | 15,051 | 38,484 | 1,730,170 | 2,510 | 3,510 | 2,033 | 5,245 | 107.8 | 76.3 | 25200 |
| sig32 | 1,611,225 | 304,279 | 88,055 | -8,976 | 1,718,094 | 2,547 | 3,510 | 1,678 | 4,380 | 107.8 | 65.7 | 131200 |
| xg32 | 333,535 | 210,056 | 38,141 | 2,058 | 1,782,498 | 2,583 | 3,510 | 1,741 | 4,551 | 107.9 | 67.6 | 4000 |
| tstamp32 | 250,776 | 288,982 | 13,687 | 25,568 | 1,782,498 | 2,583 | 3,510 | 1,741 | 4,551 | 107.9 | 67.6 | 16000 |
| bands32 | 709,665 | 626,020 | 46,493 | 41,027 | 1,982,696 | 2,527 | 3,600 | 2,094 | 4,552 | 110.1 | 76.5 | 8500 |
| hw32 | 202,107 | 174,065 | 25,277 | 8,920 | 1,782,498 | 2,583 | 3,510 | 1,741 | 4,551 | 107.9 | 67.6 | 4000 |
| hw32c | 202,736 | 435,819 | 12,257 | 26,803 | 1,782,498 | 2,583 | 3,510 | 1,741 | 4,551 | 107.9 | 67.6 | 4000 |

#### lab-stream


age 1 — oracle Δ: 10,216 px, 49.8 hw tiles (32 px), 101.3 tiles@16, 784.3 tiles@4

| form | track instr | cull instr | track ns | cull ns | region px | prims | hw tiles S | hw tiles C | inst C | GPU µs S | GPU µs C | mem B |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| full | 45 | 36,688 | 999 | 977 | 2,764,800 | 1,084 | 2,700 | 2,700 | 2,320 | 84.7 | 87.1 | 0 |
| zgx | 283 | 22,603 | 434 | 897 | 403,886 | 603 | 432 | 432 | 1,218 | 27.0 | 28.2 | 320 |
| zgx-vp | 305 | 22,701 | 42 | 1,806 | 403,886 | 603 | 432 | 432 | 1,218 | 27.0 | 28.2 | 320 |
| bbox | 109 | 22,644 | 85 | 1,214 | 403,886 | 603 | 432 | 432 | 1,218 | 27.0 | 28.2 | 88 |
| tile16 | 843 | 47,201 | 65 | 1,879 | 438,272 | 603 | 432 | 432 | 1,218 | 27.0 | 28.2 | 7200 |
| tile32 | 390 | 46,879 | -116 | 1,761 | 442,368 | 603 | 432 | 432 | 1,218 | 27.0 | 28.2 | 1800 |
| exact4 | 55,082 | 83,230 | 2,634 | 5,362 | 12,548 | 68 | 408 | 50 | 163 | 25.3 | 16.6 | 115200 |
| exact32 | 53,463 | 45,928 | 2,244 | 1,962 | 50,954 | 146 | 408 | 50 | 258 | 25.5 | 16.8 | 1800 |
| stamps16 | 44,464 | 57,286 | 2,045 | 2,489 | 25,928 | 95 | 408 | 50 | 191 | 25.4 | 16.6 | 12000 |
| sig32 | 57,323 | 48,433 | 2,771 | 1,762 | 50,954 | 146 | 408 | 50 | 258 | 25.5 | 16.8 | 44640 |
| xg32 | 53,456 | 45,818 | 2,185 | 1,750 | 50,954 | 146 | 408 | 50 | 258 | 25.5 | 16.8 | 1800 |
| tstamp32 | 43,030 | 64,553 | 2,374 | 2,970 | 50,954 | 146 | 408 | 50 | 258 | 25.5 | 16.8 | 10800 |
| bands32 | 54,437 | 81,019 | 2,931 | 5,386 | 19,230 | 97 | 480 | 50 | 190 | 27.2 | 16.6 | 7650 |
| hw32 | 40,970 | 34,115 | 2,363 | 2,709 | 50,954 | 146 | 408 | 50 | 258 | 25.5 | 16.8 | 3600 |
| hw32c | 40,948 | 62,817 | 2,395 | 6,977 | 50,954 | 146 | 408 | 50 | 258 | 25.5 | 16.8 | 3600 |

age 2 — oracle Δ: 11,184 px, 51.5 hw tiles (32 px), 106.6 tiles@16, 848.5 tiles@4

| form | track instr | cull instr | track ns | cull ns | region px | prims | hw tiles S | hw tiles C | inst C | GPU µs S | GPU µs C | mem B |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| full | -102 | 36,695 | -446 | 1,736 | 2,764,800 | 1,084 | 2,700 | 2,700 | 2,320 | 84.7 | 87.1 | 0 |
| zgx | 273 | 23,087 | -43 | 2,462 | 403,886 | 603 | 432 | 432 | 1,218 | 27.0 | 28.2 | 320 |
| zgx-vp | 167 | 22,761 | -399 | 1,909 | 403,886 | 603 | 432 | 432 | 1,218 | 27.0 | 28.2 | 320 |
| bbox | 180 | 22,499 | 344 | 392 | 403,886 | 603 | 432 | 432 | 1,218 | 27.0 | 28.2 | 88 |
| tile16 | 957 | 47,301 | 497 | 1,574 | 438,272 | 603 | 432 | 432 | 1,218 | 27.0 | 28.2 | 7200 |
| tile32 | 303 | 46,955 | -291 | 2,004 | 442,368 | 603 | 432 | 432 | 1,218 | 27.0 | 28.2 | 1800 |
| exact4 | 55,619 | 83,758 | 4,708 | 3,359 | 13,576 | 72 | 408 | 52 | 169 | 25.3 | 16.6 | 115200 |
| exact32 | 53,359 | 45,979 | 1,968 | 1,867 | 52,759 | 151 | 408 | 52 | 264 | 25.5 | 16.8 | 1800 |
| stamps16 | 44,491 | 57,427 | 2,094 | 2,384 | 27,279 | 100 | 408 | 52 | 197 | 25.4 | 16.7 | 12000 |
| sig32 | 57,246 | 48,630 | 2,570 | 1,948 | 52,759 | 151 | 408 | 52 | 264 | 25.5 | 16.8 | 66240 |
| xg32 | 53,504 | 45,963 | 3,757 | 1,544 | 52,759 | 151 | 408 | 52 | 264 | 25.5 | 16.8 | 1800 |
| tstamp32 | 42,866 | 64,643 | 1,780 | 2,795 | 52,759 | 151 | 408 | 52 | 264 | 25.5 | 16.8 | 10800 |
| bands32 | 54,217 | 84,074 | 2,144 | 5,777 | 21,051 | 102 | 480 | 52 | 197 | 27.2 | 16.7 | 7650 |
| hw32 | 40,925 | 34,366 | 3,664 | 378 | 52,759 | 151 | 408 | 52 | 264 | 25.5 | 16.8 | 3600 |
| hw32c | 40,846 | 63,439 | 1,407 | 3,816 | 52,759 | 151 | 408 | 52 | 264 | 25.5 | 16.8 | 3600 |

age 3 — oracle Δ: 12,152 px, 53.3 hw tiles (32 px), 111.8 tiles@16, 912.8 tiles@4

| form | track instr | cull instr | track ns | cull ns | region px | prims | hw tiles S | hw tiles C | inst C | GPU µs S | GPU µs C | mem B |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| full | 259 | 36,448 | 1,055 | 684 | 2,764,800 | 1,084 | 2,700 | 2,700 | 2,320 | 84.7 | 87.1 | 0 |
| zgx | 101 | 22,832 | -630 | 695 | 403,886 | 603 | 432 | 432 | 1,218 | 27.0 | 28.2 | 320 |
| zgx-vp | 395 | 22,769 | 1,066 | 1,355 | 403,886 | 603 | 432 | 432 | 1,218 | 27.0 | 28.2 | 320 |
| bbox | -18 | 22,491 | -945 | 979 | 403,886 | 603 | 432 | 432 | 1,218 | 27.0 | 28.2 | 88 |
| tile16 | 706 | 47,548 | -946 | 1,773 | 438,272 | 603 | 432 | 432 | 1,218 | 27.0 | 28.2 | 7200 |
| tile32 | 468 | 46,933 | 1,066 | 864 | 442,368 | 603 | 432 | 432 | 1,218 | 27.0 | 28.2 | 1800 |
| exact4 | 54,981 | 84,671 | 1,997 | 3,906 | 14,604 | 75 | 408 | 53 | 174 | 25.4 | 16.7 | 115200 |
| exact32 | 53,467 | 46,026 | 2,395 | 1,931 | 54,566 | 155 | 408 | 53 | 271 | 25.5 | 16.9 | 1800 |
| stamps16 | 44,514 | 57,668 | 2,500 | 2,509 | 28,630 | 104 | 408 | 53 | 203 | 25.4 | 16.7 | 12000 |
| sig32 | 57,500 | 48,776 | 3,506 | 2,218 | 54,566 | 155 | 408 | 53 | 271 | 25.5 | 16.9 | 87840 |
| xg32 | 53,569 | 45,939 | 2,783 | 2,152 | 54,566 | 155 | 408 | 53 | 271 | 25.5 | 16.9 | 1800 |
| tstamp32 | 43,043 | 64,571 | 2,295 | 2,611 | 54,566 | 155 | 408 | 53 | 271 | 25.5 | 16.9 | 10800 |
| bands32 | 54,417 | 86,990 | 2,851 | 5,693 | 22,874 | 108 | 480 | 53 | 204 | 27.2 | 16.7 | 7650 |
| hw32 | 40,940 | 34,812 | 1,954 | 2,668 | 54,566 | 155 | 408 | 53 | 271 | 25.5 | 16.9 | 3600 |
| hw32c | 40,885 | 64,134 | 2,092 | 3,550 | 54,566 | 155 | 408 | 53 | 271 | 25.5 | 16.9 | 3600 |

#### lab-scroll


age 1 — oracle Δ: 924,800 px, 1,468.0 hw tiles (32 px), 4,701.0 tiles@16, 58,300.0 tiles@4

| form | track instr | cull instr | track ns | cull ns | region px | prims | hw tiles S | hw tiles C | inst C | GPU µs S | GPU µs C | mem B |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| full | 6 | 17,936 | -15 | 837 | 2,764,800 | 483 | 2,700 | 2,700 | 1,058 | 83.5 | 84.6 | 0 |
| zgx | 556 | 17,677 | 388 | 666 | 1,472,000 | 451 | 1,508 | 1,508 | 963 | 53.6 | 54.6 | 320 |
| zgx-vp | 417 | 17,877 | 24 | 1,163 | 1,472,000 | 451 | 1,508 | 1,508 | 963 | 53.6 | 54.6 | 320 |
| bbox | 233 | 17,669 | 59 | 670 | 1,472,000 | 451 | 1,508 | 1,508 | 963 | 53.6 | 54.6 | 88 |
| tile16 | 2,342 | 37,266 | 223 | 2,211 | 1,514,496 | 451 | 1,508 | 1,508 | 963 | 53.6 | 54.6 | 7200 |
| tile32 | 1,100 | 35,952 | 1,140 | 176 | 1,544,192 | 451 | 1,508 | 1,508 | 963 | 53.6 | 54.6 | 1800 |
| exact4 | 203,222 | 49,169 | 8,196 | 2,811 | 932,800 | 451 | 1,508 | 1,468 | 963 | 53.6 | 53.6 | 115200 |
| exact32 | 122,566 | 36,090 | 5,022 | 1,281 | 1,503,232 | 451 | 1,508 | 1,468 | 963 | 53.6 | 53.6 | 1800 |
| stamps16 | 68,614 | 83,414 | 3,013 | 3,251 | 1,514,496 | 451 | 1,508 | 1,508 | 963 | 53.6 | 54.6 | 12000 |
| sig32 | 533,046 | 117,540 | 22,024 | 5,708 | 1,503,232 | 451 | 1,508 | 1,468 | 963 | 53.6 | 53.6 | 44640 |
| xg32 | 4,705 | 36,086 | -14 | 1,498 | 1,544,192 | 451 | 1,508 | 1,508 | 963 | 53.6 | 54.6 | 1800 |
| tstamp32 | 7,189 | 100,227 | 423 | 5,029 | 1,544,192 | 451 | 1,508 | 1,508 | 963 | 53.6 | 54.6 | 10800 |
| bands32 | 7,259 | 47,116 | 157 | 1,991 | 1,530,880 | 451 | 1,560 | 1,508 | 963 | 54.9 | 54.6 | 7650 |
| hw32 | 2,686 | 29,836 | 174 | 1,540 | 1,544,192 | 451 | 1,508 | 1,508 | 963 | 53.6 | 54.6 | 3600 |
| hw32c | 2,627 | 53,806 | 175 | 2,735 | 1,544,192 | 451 | 1,508 | 1,508 | 963 | 53.6 | 54.6 | 3600 |

age 2 — oracle Δ: 1,472,000 px, 1,508.0 hw tiles (32 px), 5,916.0 tiles@16, 92,000.0 tiles@4

| form | track instr | cull instr | track ns | cull ns | region px | prims | hw tiles S | hw tiles C | inst C | GPU µs S | GPU µs C | mem B |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| full | -56 | 17,945 | -216 | 742 | 2,764,800 | 483 | 2,700 | 2,700 | 1,058 | 83.5 | 84.6 | 0 |
| zgx | 516 | 17,833 | 276 | 929 | 1,472,000 | 451 | 1,508 | 1,508 | 963 | 53.6 | 54.6 | 320 |
| zgx-vp | 410 | 17,967 | -85 | 1,688 | 1,472,000 | 451 | 1,508 | 1,508 | 963 | 53.6 | 54.6 | 320 |
| bbox | 134 | 17,863 | 174 | 1,220 | 1,472,000 | 451 | 1,508 | 1,508 | 963 | 53.6 | 54.6 | 88 |
| tile16 | 2,466 | 36,979 | 322 | 1,020 | 1,514,496 | 451 | 1,508 | 1,508 | 963 | 53.6 | 54.6 | 7200 |
| tile32 | 896 | 36,215 | -199 | 1,298 | 1,544,192 | 451 | 1,508 | 1,508 | 963 | 53.6 | 54.6 | 1800 |
| exact4 | 203,594 | 51,323 | 8,990 | 3,022 | 1,472,000 | 451 | 1,508 | 1,508 | 963 | 53.6 | 54.6 | 115200 |
| exact32 | 122,475 | 36,436 | 4,454 | 2,669 | 1,544,192 | 451 | 1,508 | 1,508 | 963 | 53.6 | 54.6 | 1800 |
| stamps16 | 68,414 | 84,406 | 2,585 | 4,757 | 1,514,496 | 451 | 1,508 | 1,508 | 963 | 53.6 | 54.6 | 12000 |
| sig32 | 533,370 | 118,858 | 24,071 | 5,566 | 1,544,192 | 451 | 1,508 | 1,508 | 963 | 53.6 | 54.6 | 66240 |
| xg32 | 4,641 | 36,236 | -191 | 1,044 | 1,544,192 | 451 | 1,508 | 1,508 | 963 | 53.6 | 54.6 | 1800 |
| tstamp32 | 7,132 | 99,937 | -281 | 4,483 | 1,544,192 | 451 | 1,508 | 1,508 | 963 | 53.6 | 54.6 | 10800 |
| bands32 | 7,206 | 49,014 | -296 | 1,954 | 1,530,880 | 451 | 1,560 | 1,508 | 963 | 54.9 | 54.6 | 7650 |
| hw32 | 2,672 | 30,187 | 234 | 1,727 | 1,544,192 | 451 | 1,508 | 1,508 | 963 | 53.6 | 54.6 | 3600 |
| hw32c | 2,630 | 53,989 | 40 | 2,642 | 1,544,192 | 451 | 1,508 | 1,508 | 963 | 53.6 | 54.6 | 3600 |

age 3 — oracle Δ: 924,800 px, 1,468.0 hw tiles (32 px), 4,701.0 tiles@16, 58,300.0 tiles@4

| form | track instr | cull instr | track ns | cull ns | region px | prims | hw tiles S | hw tiles C | inst C | GPU µs S | GPU µs C | mem B |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| full | 6 | 17,934 | 14 | 866 | 2,764,800 | 483 | 2,700 | 2,700 | 1,058 | 83.5 | 84.6 | 0 |
| zgx | 319 | 18,111 | 93 | 735 | 1,472,000 | 451 | 1,508 | 1,508 | 963 | 53.6 | 54.6 | 320 |
| zgx-vp | 410 | 18,055 | 174 | 848 | 1,472,000 | 451 | 1,508 | 1,508 | 963 | 53.6 | 54.6 | 320 |
| bbox | 88 | 17,994 | -161 | 1,307 | 1,472,000 | 451 | 1,508 | 1,508 | 963 | 53.6 | 54.6 | 88 |
| tile16 | 2,266 | 37,479 | -275 | 1,652 | 1,514,496 | 451 | 1,508 | 1,508 | 963 | 53.6 | 54.6 | 7200 |
| tile32 | 958 | 36,337 | 99 | 1,298 | 1,544,192 | 451 | 1,508 | 1,508 | 963 | 53.6 | 54.6 | 1800 |
| exact4 | 203,656 | 53,613 | 8,957 | 5,063 | 1,472,000 | 451 | 1,508 | 1,508 | 963 | 53.6 | 54.6 | 115200 |
| exact32 | 122,555 | 36,309 | 5,197 | 1,051 | 1,544,192 | 451 | 1,508 | 1,508 | 963 | 53.6 | 54.6 | 1800 |
| stamps16 | 68,490 | 84,552 | 3,126 | 3,619 | 1,514,496 | 451 | 1,508 | 1,508 | 963 | 53.6 | 54.6 | 12000 |
| sig32 | 535,444 | 120,128 | 49,796 | 14,730 | 1,544,192 | 451 | 1,508 | 1,508 | 963 | 53.6 | 54.6 | 87840 |
| xg32 | 4,846 | 36,373 | 2,754 | 3,774 | 1,544,192 | 451 | 1,508 | 1,508 | 963 | 53.6 | 54.6 | 1800 |
| tstamp32 | 7,308 | 100,310 | 1,855 | 8,765 | 1,544,192 | 451 | 1,508 | 1,508 | 963 | 53.6 | 54.6 | 10800 |
| bands32 | 7,359 | 51,396 | 2,013 | 5,919 | 1,530,880 | 451 | 1,560 | 1,508 | 963 | 54.9 | 54.6 | 7650 |
| hw32 | 2,627 | 30,557 | 152 | 1,752 | 1,544,192 | 451 | 1,508 | 1,508 | 963 | 53.6 | 54.6 | 3600 |
| hw32c | 2,627 | 54,320 | 171 | 2,712 | 1,544,192 | 451 | 1,508 | 1,508 | 963 | 53.6 | 54.6 | 3600 |

#### animate


age 1 — oracle Δ: 29,070 px, 83.7 hw tiles (32 px), 216.0 tiles@16, 1,949.1 tiles@4

| form | track instr | cull instr | track ns | cull ns | region px | prims | hw tiles S | hw tiles C | inst C | GPU µs S | GPU µs C | mem B |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| full | -10 | 3,389 | -477 | 792 | 2,764,800 | 88 | 2,700 | 2,700 | 207 | 82.7 | 82.9 | 0 |
| zgx | 17,741 | 17,167 | 1,065 | 1,288 | 87,669 | 53 | 125 | 125 | 101 | 18.2 | 18.3 | 5952 |
| zgx-vp | 17,792 | 17,293 | 2,277 | 1,352 | 87,669 | 53 | 125 | 125 | 101 | 18.2 | 18.3 | 5952 |
| bbox | 1,984 | 3,158 | 150 | 149 | 237,001 | 79 | 265 | 265 | 154 | 21.8 | 21.9 | 88 |
| tile16 | 7,409 | 7,157 | 338 | 306 | 55,303 | 37 | 265 | 84 | 79 | 21.7 | 17.2 | 7200 |
| tile32 | 6,480 | 6,489 | 296 | -161 | 85,690 | 38 | 265 | 84 | 81 | 21.7 | 17.3 | 1800 |
| exact4 | 9,051 | 14,363 | 513 | 873 | 31,185 | 37 | 265 | 84 | 78 | 21.7 | 17.2 | 115200 |
| exact32 | 5,769 | 6,512 | 820 | 731 | 85,690 | 38 | 265 | 84 | 81 | 21.7 | 17.3 | 1800 |
| stamps16 | 1,726 | 11,202 | -385 | 2,079 | 55,303 | 37 | 265 | 84 | 79 | 21.7 | 17.2 | 6280 |
| sig32 | 32,257 | 11,334 | 3,411 | 892 | 85,690 | 38 | 265 | 84 | 81 | 21.7 | 17.3 | 44640 |
| xg32 | 5,768 | 6,490 | 877 | 587 | 85,690 | 38 | 265 | 84 | 81 | 21.7 | 17.3 | 1800 |
| tstamp32 | 4,594 | 26,305 | 759 | 2,405 | 85,690 | 38 | 265 | 84 | 81 | 21.7 | 17.3 | 10800 |
| bands32 | 8,074 | 13,776 | 1,150 | 1,541 | 51,345 | 36 | 648 | 84 | 77 | 31.3 | 17.2 | 7650 |
| hw32 | 3,319 | 5,833 | 149 | 350 | 85,690 | 38 | 265 | 84 | 81 | 21.7 | 17.3 | 3600 |
| hw32c | 3,319 | 9,904 | 170 | 513 | 85,690 | 38 | 265 | 84 | 81 | 21.7 | 17.3 | 3600 |

age 2 — oracle Δ: 31,108 px, 90.3 hw tiles (32 px), 231.1 tiles@16, 2,078.5 tiles@4

| form | track instr | cull instr | track ns | cull ns | region px | prims | hw tiles S | hw tiles C | inst C | GPU µs S | GPU µs C | mem B |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| full | 85 | 3,293 | -334 | 606 | 2,764,800 | 88 | 2,700 | 2,700 | 207 | 82.7 | 82.9 | 0 |
| zgx | 17,755 | 25,967 | 1,074 | 1,788 | 97,175 | 57 | 136 | 136 | 108 | 18.5 | 18.6 | 5952 |
| zgx-vp | 17,854 | 26,311 | 2,022 | 3,224 | 97,175 | 57 | 136 | 136 | 108 | 18.5 | 18.6 | 5952 |
| bbox | 1,986 | 3,202 | 150 | 157 | 238,901 | 79 | 267 | 267 | 155 | 21.8 | 22.0 | 88 |
| tile16 | 7,476 | 7,271 | 438 | 205 | 59,172 | 40 | 267 | 90 | 83 | 21.7 | 17.4 | 7200 |
| tile32 | 6,499 | 6,700 | 521 | -81 | 92,463 | 41 | 267 | 90 | 85 | 21.7 | 17.4 | 1800 |
| exact4 | 9,059 | 15,260 | 467 | 804 | 33,256 | 40 | 267 | 90 | 83 | 21.7 | 17.4 | 115200 |
| exact32 | 5,791 | 6,636 | -239 | 564 | 92,463 | 41 | 267 | 90 | 85 | 21.7 | 17.4 | 1800 |
| stamps16 | 1,745 | 11,360 | 88 | 1,014 | 59,172 | 40 | 267 | 90 | 83 | 21.7 | 17.4 | 6280 |
| sig32 | 32,293 | 11,824 | 2,711 | 362 | 92,463 | 41 | 267 | 90 | 85 | 21.7 | 17.4 | 66240 |
| xg32 | 5,789 | 6,664 | 715 | -57 | 92,463 | 41 | 267 | 90 | 85 | 21.7 | 17.4 | 1800 |
| tstamp32 | 4,611 | 26,430 | 591 | 2,618 | 92,463 | 41 | 267 | 90 | 85 | 21.7 | 17.4 | 10800 |
| bands32 | 8,099 | 18,312 | 977 | 1,843 | 55,234 | 39 | 649 | 90 | 82 | 31.3 | 17.4 | 7650 |
| hw32 | 3,322 | 6,213 | 151 | 351 | 92,463 | 41 | 267 | 90 | 85 | 21.7 | 17.4 | 3600 |
| hw32c | 3,322 | 10,370 | 150 | 554 | 92,463 | 41 | 267 | 90 | 85 | 21.7 | 17.4 | 3600 |

age 3 — oracle Δ: 33,149 px, 96.9 hw tiles (32 px), 246.2 tiles@16, 2,208.2 tiles@4

| form | track instr | cull instr | track ns | cull ns | region px | prims | hw tiles S | hw tiles C | inst C | GPU µs S | GPU µs C | mem B |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| full | 6 | 3,340 | -143 | -105 | 2,764,800 | 88 | 2,700 | 2,700 | 207 | 82.7 | 82.9 | 0 |
| zgx | 17,774 | 34,727 | 1,067 | 2,471 | 107,102 | 60 | 147 | 147 | 115 | 18.8 | 18.9 | 5952 |
| zgx-vp | 17,882 | 34,956 | 691 | 5,448 | 107,102 | 60 | 147 | 147 | 115 | 18.8 | 18.9 | 5952 |
| bbox | 1,987 | 3,245 | 163 | 142 | 240,622 | 80 | 269 | 269 | 156 | 21.9 | 22.0 | 88 |
| tile16 | 7,421 | 7,502 | 345 | 354 | 63,040 | 42 | 269 | 97 | 88 | 21.8 | 17.6 | 7200 |
| tile32 | 6,577 | 6,736 | 46 | 61 | 99,232 | 43 | 269 | 97 | 89 | 21.8 | 17.6 | 1800 |
| exact4 | 9,067 | 16,225 | 472 | 875 | 35,331 | 42 | 269 | 97 | 87 | 21.8 | 17.6 | 115200 |
| exact32 | 5,822 | 6,788 | -255 | 629 | 99,232 | 43 | 269 | 97 | 89 | 21.8 | 17.6 | 1800 |
| stamps16 | 1,756 | 11,696 | -318 | 1,225 | 63,040 | 42 | 269 | 97 | 88 | 21.8 | 17.6 | 6280 |
| sig32 | 32,579 | 12,483 | 3,190 | 753 | 99,232 | 43 | 269 | 97 | 89 | 21.8 | 17.6 | 87840 |
| xg32 | 5,791 | 6,961 | 784 | 451 | 99,232 | 43 | 269 | 97 | 89 | 21.8 | 17.6 | 1800 |
| tstamp32 | 4,783 | 26,596 | 646 | 2,578 | 99,232 | 43 | 269 | 97 | 89 | 21.8 | 17.6 | 10800 |
| bands32 | 8,189 | 22,962 | 455 | 4,414 | 59,128 | 41 | 650 | 97 | 86 | 31.3 | 17.6 | 7650 |
| hw32 | 3,324 | 6,589 | 172 | 361 | 99,232 | 43 | 269 | 97 | 89 | 21.8 | 17.6 | 3600 |
| hw32c | 3,324 | 10,834 | 153 | 578 | 99,232 | 43 | 269 | 97 | 89 | 21.8 | 17.6 | 3600 |

#### blink


age 1 — oracle Δ: 160 px, 2.0 hw tiles (32 px), 3.0 tiles@16, 10.0 tiles@4

| form | track instr | cull instr | track ns | cull ns | region px | prims | hw tiles S | hw tiles C | inst C | GPU µs S | GPU µs C | mem B |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| full | 6 | 971 | 0 | 41 | 2,764,800 | 2 | 2,700 | 2,700 | 46 | 82.5 | 82.6 | 0 |
| zgx | 232 | 1,084 | 7 | 50 | 160 | 2 | 2 | 2 | 3 | 15.1 | 15.1 | 320 |
| zgx-vp | 232 | 1,084 | 7 | 67 | 160 | 2 | 2 | 2 | 3 | 15.1 | 15.1 | 320 |
| bbox | 81 | 1,019 | 3 | 44 | 160 | 2 | 2 | 2 | 3 | 15.1 | 15.1 | 88 |
| tile16 | 208 | 1,351 | 103 | -24 | 768 | 2 | 2 | 2 | 3 | 15.1 | 15.1 | 7200 |
| tile32 | 186 | 1,230 | 100 | 122 | 2,048 | 2 | 2 | 2 | 3 | 15.1 | 15.1 | 1800 |
| exact4 | 362 | 2,620 | 18 | 134 | 160 | 2 | 2 | 2 | 3 | 15.1 | 15.1 | 115200 |
| exact32 | 240 | 1,229 | 27 | 56 | 2,048 | 2 | 2 | 2 | 3 | 15.1 | 15.1 | 1800 |
| stamps16 | 169 | 1,864 | 22 | 79 | 768 | 2 | 2 | 2 | 3 | 15.1 | 15.1 | 6020 |
| sig32 | 384 | 1,401 | 29 | 42 | 2,048 | 2 | 2 | 2 | 3 | 15.1 | 15.1 | 44640 |
| xg32 | 237 | 1,229 | 8 | 58 | 2,048 | 2 | 2 | 2 | 3 | 15.1 | 15.1 | 1800 |
| tstamp32 | 165 | 18,497 | 4 | 1,843 | 2,048 | 2 | 2 | 2 | 3 | 15.1 | 15.1 | 10800 |
| bands32 | 241 | 1,300 | 110 | -40 | 256 | 2 | 120 | 2 | 3 | 18.0 | 15.1 | 7650 |
| hw32 | 208 | 1,642 | 8 | 81 | 2,048 | 2 | 2 | 2 | 3 | 15.1 | 15.1 | 3600 |
| hw32c | 208 | 2,688 | 10 | 132 | 2,048 | 2 | 2 | 2 | 3 | 15.1 | 15.1 | 3600 |

age 2 — oracle Δ: 0 px, 0.0 hw tiles (32 px), 0.0 tiles@16, 0.0 tiles@4

| form | track instr | cull instr | track ns | cull ns | region px | prims | hw tiles S | hw tiles C | inst C | GPU µs S | GPU µs C | mem B |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| full | 6 | 971 | 1 | 41 | 2,764,800 | 2 | 2,700 | 2,700 | 46 | 82.5 | 82.6 | 0 |
| zgx | 229 | 1,217 | -2 | 56 | 160 | 2 | 2 | 2 | 3 | 15.1 | 15.1 | 320 |
| zgx-vp | 229 | 1,217 | 8 | 55 | 160 | 2 | 2 | 2 | 3 | 15.1 | 15.1 | 320 |
| bbox | 81 | 1,050 | -7 | 47 | 160 | 2 | 2 | 2 | 3 | 15.1 | 15.1 | 88 |
| tile16 | 208 | 1,446 | -1 | 74 | 768 | 2 | 2 | 2 | 3 | 15.1 | 15.1 | 7200 |
| tile32 | 186 | 1,300 | 8 | 62 | 2,048 | 2 | 2 | 2 | 3 | 15.1 | 15.1 | 1800 |
| exact4 | 363 | 2,808 | 8 | 148 | 160 | 2 | 2 | 2 | 3 | 15.1 | 15.1 | 115200 |
| exact32 | 240 | 1,301 | 114 | 124 | 2,048 | 2 | 2 | 2 | 3 | 15.1 | 15.1 | 1800 |
| stamps16 | 169 | 1,916 | 107 | 187 | 768 | 2 | 2 | 2 | 3 | 15.1 | 15.1 | 6020 |
| sig32 | 384 | 359 | 123 | 37 | 0 | 0 | 0 | 0 | 0 | 0.0 | 0.0 | 66240 |
| xg32 | 237 | 1,300 | 111 | 124 | 2,048 | 2 | 2 | 2 | 3 | 15.1 | 15.1 | 1800 |
| tstamp32 | 165 | 18,577 | 103 | 1,308 | 2,048 | 2 | 2 | 2 | 3 | 15.1 | 15.1 | 10800 |
| bands32 | 241 | 1,477 | 9 | 67 | 256 | 2 | 120 | 2 | 3 | 18.0 | 15.1 | 7650 |
| hw32 | 208 | 1,974 | 9 | 93 | 2,048 | 2 | 2 | 2 | 3 | 15.1 | 15.1 | 3600 |
| hw32c | 208 | 3,020 | 8 | 147 | 2,048 | 2 | 2 | 2 | 3 | 15.1 | 15.1 | 3600 |

age 3 — oracle Δ: 160 px, 2.0 hw tiles (32 px), 3.0 tiles@16, 10.0 tiles@4

| form | track instr | cull instr | track ns | cull ns | region px | prims | hw tiles S | hw tiles C | inst C | GPU µs S | GPU µs C | mem B |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| full | 6 | 971 | -2 | 95 | 2,764,800 | 2 | 2,700 | 2,700 | 46 | 82.5 | 82.6 | 0 |
| zgx | 227 | 1,350 | -8 | 74 | 160 | 2 | 2 | 2 | 3 | 15.1 | 15.1 | 320 |
| zgx-vp | 227 | 1,350 | 32 | 118 | 160 | 2 | 2 | 2 | 3 | 15.1 | 15.1 | 320 |
| bbox | 81 | 1,081 | -12 | 183 | 160 | 2 | 2 | 2 | 3 | 15.1 | 15.1 | 88 |
| tile16 | 208 | 1,541 | 89 | 161 | 768 | 2 | 2 | 2 | 3 | 15.1 | 15.1 | 7200 |
| tile32 | 186 | 1,371 | 19 | 142 | 2,048 | 2 | 2 | 2 | 3 | 15.1 | 15.1 | 1800 |
| exact4 | 363 | 2,996 | 5 | 164 | 160 | 2 | 2 | 2 | 3 | 15.1 | 15.1 | 115200 |
| exact32 | 241 | 1,371 | -76 | 66 | 2,048 | 2 | 2 | 2 | 3 | 15.1 | 15.1 | 1800 |
| stamps16 | 170 | 1,968 | -65 | 77 | 768 | 2 | 2 | 2 | 3 | 15.1 | 15.1 | 6020 |
| sig32 | 384 | 1,541 | -57 | 54 | 2,048 | 2 | 2 | 2 | 3 | 15.1 | 15.1 | 87840 |
| xg32 | 238 | 1,391 | -62 | 77 | 2,048 | 2 | 2 | 2 | 3 | 15.1 | 15.1 | 1800 |
| tstamp32 | 165 | 18,438 | -79 | 1,754 | 2,048 | 2 | 2 | 2 | 3 | 15.1 | 15.1 | 10800 |
| bands32 | 241 | 1,654 | 24 | 156 | 256 | 2 | 120 | 2 | 3 | 18.0 | 15.1 | 7650 |
| hw32 | 208 | 2,306 | 9 | 107 | 2,048 | 2 | 2 | 2 | 3 | 15.1 | 15.1 | 3600 |
| hw32c | 208 | 3,352 | 9 | 159 | 2,048 | 2 | 2 | 2 | 3 | 15.1 | 15.1 | 3600 |

#### vlist-scroll


age 1 — oracle Δ: 1,313,118 px, 1,744.8 hw tiles (32 px), 6,328.7 tiles@16, 86,597.1 tiles@4

| form | track instr | cull instr | track ns | cull ns | region px | prims | hw tiles S | hw tiles C | inst C | GPU µs S | GPU µs C | mem B |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| full | 107 | 63,090 | 423 | 3,223 | 2,304,000 | 1,544 | 2,250 | 2,250 | 3,313 | 74.3 | 77.9 | 0 |
| zgx | -120 | 63,062 | -8,278 | 2,964 | 2,304,000 | 1,544 | 2,250 | 2,250 | 3,313 | 74.3 | 77.9 | 320 |
| zgx-vp | 428 | 63,447 | -1,598 | 5,430 | 2,304,000 | 1,544 | 2,250 | 2,250 | 3,313 | 74.3 | 77.9 | 320 |
| bbox | 329 | 62,613 | -7,295 | 2,469 | 2,304,000 | 1,544 | 2,250 | 2,250 | 3,313 | 74.3 | 77.9 | 88 |
| tile16 | 3,959 | 125,164 | -7,061 | 7,268 | 2,304,000 | 1,544 | 2,250 | 2,250 | 3,313 | 74.3 | 77.9 | 8000 |
| tile32 | 2,133 | 124,705 | -1,761 | 8,864 | 2,304,000 | 1,544 | 2,250 | 2,250 | 3,313 | 74.3 | 77.9 | 2000 |
| exact4 | 727,342 | 132,333 | 20,482 | 14,553 | 1,385,553 | 1,544 | 2,228 | 1,745 | 3,298 | 73.8 | 65.2 | 96000 |
| exact32 | 452,996 | 124,012 | 22,414 | 5,984 | 1,786,665 | 1,544 | 2,228 | 1,745 | 3,299 | 73.8 | 65.2 | 2000 |
| stamps16 | 253,923 | 288,352 | 12,518 | 18,330 | 2,304,000 | 1,544 | 2,250 | 2,250 | 3,313 | 74.3 | 77.9 | 113520 |
| sig32 | 1,386,206 | 230,159 | 57,094 | 15,041 | 1,786,665 | 1,544 | 2,228 | 1,745 | 3,299 | 73.8 | 65.2 | 37600 |
| xg32 | 45,089 | 124,768 | -1,195 | 9,032 | 2,304,000 | 1,544 | 2,250 | 2,250 | 3,313 | 74.3 | 77.9 | 2000 |
| tstamp32 | 43,514 | 209,497 | -2,624 | 11,151 | 2,304,000 | 1,544 | 2,250 | 2,250 | 3,313 | 74.3 | 77.9 | 9000 |
| bands32 | 63,065 | 161,736 | -2,473 | 9,003 | 2,304,000 | 1,544 | 2,250 | 2,250 | 3,313 | 74.3 | 77.9 | 8500 |
| hw32 | 26,392 | 106,509 | 1,160 | 7,738 | 2,304,000 | 1,544 | 2,250 | 2,250 | 3,313 | 74.3 | 77.9 | 4000 |
| hw32c | 26,453 | 180,590 | 1,459 | 9,961 | 2,304,000 | 1,544 | 2,250 | 2,250 | 3,313 | 74.3 | 77.9 | 4000 |

age 2 — oracle Δ: 1,492,948 px, 1,911.8 hw tiles (32 px), 7,253.2 tiles@16, 102,300.9 tiles@4

| form | track instr | cull instr | track ns | cull ns | region px | prims | hw tiles S | hw tiles C | inst C | GPU µs S | GPU µs C | mem B |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| full | 49 | 63,713 | -120 | 5,809 | 2,304,000 | 1,544 | 2,250 | 2,250 | 3,312 | 74.3 | 77.9 | 0 |
| zgx | 656 | 62,917 | 66 | 2,305 | 2,304,000 | 1,544 | 2,250 | 2,250 | 3,312 | 74.3 | 77.9 | 320 |
| zgx-vp | 573 | 63,676 | 1,490 | 4,045 | 2,304,000 | 1,544 | 2,250 | 2,250 | 3,312 | 74.3 | 77.9 | 320 |
| bbox | -117 | 63,377 | -175 | 11,724 | 2,304,000 | 1,544 | 2,250 | 2,250 | 3,312 | 74.3 | 77.9 | 88 |
| tile16 | 4,053 | 125,345 | -594 | 5,110 | 2,304,000 | 1,544 | 2,250 | 2,250 | 3,312 | 74.3 | 77.9 | 8000 |
| tile32 | 1,637 | 124,333 | 239 | 4,261 | 2,304,000 | 1,544 | 2,250 | 2,250 | 3,312 | 74.3 | 77.9 | 2000 |
| exact4 | 728,780 | 132,979 | 99,385 | -53,930 | 1,663,401 | 1,544 | 2,235 | 1,912 | 3,300 | 74.0 | 69.4 | 96000 |
| exact32 | 451,147 | 124,856 | 20,105 | 6,660 | 1,957,666 | 1,544 | 2,235 | 1,912 | 3,301 | 74.0 | 69.4 | 2000 |
| stamps16 | 252,589 | 287,515 | 10,981 | 12,245 | 2,304,000 | 1,544 | 2,250 | 2,250 | 3,312 | 74.3 | 77.9 | 113520 |
| sig32 | 1,385,056 | 236,014 | 58,411 | 15,327 | 1,957,666 | 1,544 | 2,235 | 1,912 | 3,301 | 74.0 | 69.4 | 55600 |
| xg32 | 45,498 | 124,466 | 7,294 | 6,335 | 2,304,000 | 1,544 | 2,250 | 2,250 | 3,312 | 74.3 | 77.9 | 2000 |
| tstamp32 | 43,738 | 208,690 | 1,769 | 8,704 | 2,304,000 | 1,544 | 2,250 | 2,250 | 3,312 | 74.3 | 77.9 | 9000 |
| bands32 | 63,232 | 165,327 | 2,083 | 7,656 | 2,304,000 | 1,544 | 2,250 | 2,250 | 3,312 | 74.3 | 77.9 | 8500 |
| hw32 | 26,691 | 106,984 | 1,603 | 11,591 | 2,304,000 | 1,544 | 2,250 | 2,250 | 3,312 | 74.3 | 77.9 | 4000 |
| hw32c | 26,613 | 180,999 | 2,551 | 9,942 | 2,304,000 | 1,544 | 2,250 | 2,250 | 3,312 | 74.3 | 77.9 | 4000 |

age 3 — oracle Δ: 1,688,146 px, 1,958.9 hw tiles (32 px), 7,627.4 tiles@16, 113,242.0 tiles@4

| form | track instr | cull instr | track ns | cull ns | region px | prims | hw tiles S | hw tiles C | inst C | GPU µs S | GPU µs C | mem B |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| full | -237 | 63,687 | -654 | 4,532 | 2,304,000 | 1,544 | 2,250 | 2,250 | 3,312 | 74.3 | 77.9 | 0 |
| zgx | 509 | 63,263 | -2,497 | 1,969 | 2,304,000 | 1,544 | 2,250 | 2,250 | 3,312 | 74.3 | 77.9 | 320 |
| zgx-vp | 1,326 | 62,688 | 6,813 | 23,653 | 2,304,000 | 1,544 | 2,250 | 2,250 | 3,312 | 74.3 | 77.9 | 320 |
| bbox | 288 | 63,164 | -3,278 | 3,141 | 2,304,000 | 1,544 | 2,250 | 2,250 | 3,312 | 74.3 | 77.9 | 88 |
| tile16 | 4,599 | 125,557 | -1,081 | 4,488 | 2,304,000 | 1,544 | 2,250 | 2,250 | 3,312 | 74.3 | 77.9 | 8000 |
| tile32 | 1,576 | 124,433 | 208 | 3,910 | 2,304,000 | 1,544 | 2,250 | 2,250 | 3,312 | 74.3 | 77.9 | 2000 |
| exact4 | 728,194 | 137,431 | 28,008 | 7,590 | 1,880,513 | 1,544 | 2,240 | 1,962 | 3,303 | 74.1 | 70.6 | 96000 |
| exact32 | 450,467 | 124,754 | 17,338 | 4,921 | 2,008,708 | 1,544 | 2,240 | 1,962 | 3,303 | 74.1 | 70.6 | 2000 |
| stamps16 | 252,188 | 288,658 | 10,501 | 14,723 | 2,304,000 | 1,544 | 2,250 | 2,250 | 3,312 | 74.3 | 77.9 | 113520 |
| sig32 | 1,385,216 | 236,785 | 59,451 | 9,692 | 2,006,095 | 1,544 | 2,240 | 1,959 | 3,303 | 74.1 | 70.6 | 73600 |
| xg32 | 44,990 | 125,094 | 2,695 | 6,198 | 2,304,000 | 1,544 | 2,250 | 2,250 | 3,312 | 74.3 | 77.9 | 2000 |
| tstamp32 | 44,076 | 208,717 | 2,307 | 10,722 | 2,304,000 | 1,544 | 2,250 | 2,250 | 3,312 | 74.3 | 77.9 | 9000 |
| bands32 | 63,618 | 168,497 | 4,553 | 6,468 | 2,304,000 | 1,544 | 2,250 | 2,250 | 3,312 | 74.3 | 77.9 | 8500 |
| hw32 | 26,616 | 106,919 | 1,198 | 6,722 | 2,304,000 | 1,544 | 2,250 | 2,250 | 3,312 | 74.3 | 77.9 | 4000 |
| hw32c | 26,514 | 181,653 | 1,193 | 13,735 | 2,304,000 | 1,544 | 2,250 | 2,250 | 3,312 | 74.3 | 77.9 | 4000 |

#### vlist-fling


age 1 — oracle Δ: 1,399,519 px, 1,831.4 hw tiles (32 px), 6,845.7 tiles@16, 93,501.9 tiles@4

| form | track instr | cull instr | track ns | cull ns | region px | prims | hw tiles S | hw tiles C | inst C | GPU µs S | GPU µs C | mem B |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| full | 150 | 65,190 | 426 | 5,098 | 2,304,000 | 1,619 | 2,250 | 2,250 | 3,313 | 74.5 | 77.9 | 0 |
| zgx | 586 | 65,329 | -8 | 2,592 | 2,304,000 | 1,619 | 2,250 | 2,250 | 3,313 | 74.5 | 77.9 | 320 |
| zgx-vp | 451 | 65,550 | 199 | 4,211 | 2,304,000 | 1,619 | 2,250 | 2,250 | 3,313 | 74.5 | 77.9 | 320 |
| bbox | 442 | 65,037 | -131 | 2,862 | 2,304,000 | 1,619 | 2,250 | 2,250 | 3,313 | 74.5 | 77.9 | 88 |
| tile16 | 4,995 | 129,803 | 1,268 | 4,225 | 2,304,000 | 1,619 | 2,250 | 2,250 | 3,313 | 74.5 | 77.9 | 8000 |
| tile32 | 1,574 | 129,819 | 95 | 5,495 | 2,304,000 | 1,619 | 2,250 | 2,250 | 3,313 | 74.5 | 77.9 | 2000 |
| exact4 | 755,126 | 135,766 | 36,108 | 17,237 | 1,501,488 | 1,619 | 2,231 | 1,833 | 3,304 | 74.0 | 67.4 | 96000 |
| exact32 | 477,812 | 129,547 | 20,095 | 4,922 | 1,876,941 | 1,619 | 2,231 | 1,833 | 3,304 | 74.0 | 67.4 | 2000 |
| stamps16 | 267,869 | 297,111 | 12,779 | 14,935 | 2,304,000 | 1,619 | 2,250 | 2,250 | 3,313 | 74.5 | 77.9 | 113520 |
| sig32 | 1,433,192 | 235,626 | 88,060 | -13,685 | 1,876,941 | 1,619 | 2,231 | 1,833 | 3,304 | 74.0 | 67.4 | 37600 |
| xg32 | 56,108 | 129,421 | 4,014 | 4,063 | 2,304,000 | 1,619 | 2,250 | 2,250 | 3,313 | 74.5 | 77.9 | 2000 |
| tstamp32 | 52,811 | 214,028 | 3,072 | 17,699 | 2,304,000 | 1,619 | 2,250 | 2,250 | 3,313 | 74.5 | 77.9 | 9000 |
| bands32 | 78,791 | 166,200 | 10,837 | 7,149 | 2,304,000 | 1,619 | 2,250 | 2,250 | 3,313 | 74.5 | 77.9 | 8500 |
| hw32 | 32,419 | 110,507 | 962 | 8,029 | 2,304,000 | 1,619 | 2,250 | 2,250 | 3,313 | 74.5 | 77.9 | 4000 |
| hw32c | 32,497 | 187,473 | 1,110 | 10,234 | 2,304,000 | 1,619 | 2,250 | 2,250 | 3,313 | 74.5 | 77.9 | 4000 |

age 2 — oracle Δ: 1,530,828 px, 1,900.5 hw tiles (32 px), 7,187.9 tiles@16, 101,002.0 tiles@4

| form | track instr | cull instr | track ns | cull ns | region px | prims | hw tiles S | hw tiles C | inst C | GPU µs S | GPU µs C | mem B |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| full | -274 | 65,332 | -1,273 | 4,078 | 2,304,000 | 1,619 | 2,250 | 2,250 | 3,313 | 74.5 | 77.9 | 0 |
| zgx | 403 | 65,739 | -209 | 3,622 | 2,304,000 | 1,619 | 2,250 | 2,250 | 3,313 | 74.5 | 77.9 | 320 |
| zgx-vp | 139 | 65,539 | -1,402 | 3,429 | 2,304,000 | 1,619 | 2,250 | 2,250 | 3,313 | 74.5 | 77.9 | 320 |
| bbox | 19 | 65,491 | -749 | 4,316 | 2,304,000 | 1,619 | 2,250 | 2,250 | 3,313 | 74.5 | 77.9 | 88 |
| tile16 | 4,325 | 130,309 | -436 | 4,680 | 2,304,000 | 1,619 | 2,250 | 2,250 | 3,313 | 74.5 | 77.9 | 8000 |
| tile32 | 1,547 | 129,664 | -669 | 4,844 | 2,304,000 | 1,619 | 2,250 | 2,250 | 3,313 | 74.5 | 77.9 | 2000 |
| exact4 | 754,582 | 138,706 | 40,816 | -486 | 1,703,700 | 1,619 | 2,234 | 1,924 | 3,305 | 74.1 | 69.7 | 96000 |
| exact32 | 478,078 | 129,837 | 25,764 | 16,263 | 1,970,561 | 1,619 | 2,234 | 1,924 | 3,306 | 74.1 | 69.7 | 2000 |
| stamps16 | 268,015 | 298,294 | 14,413 | 13,308 | 2,304,000 | 1,619 | 2,250 | 2,250 | 3,313 | 74.5 | 77.9 | 113520 |
| sig32 | 1,435,382 | 238,687 | 73,436 | 6,657 | 1,961,863 | 1,619 | 2,234 | 1,916 | 3,306 | 74.1 | 69.5 | 55600 |
| xg32 | 55,815 | 130,150 | 2,302 | 8,640 | 2,304,000 | 1,619 | 2,250 | 2,250 | 3,313 | 74.5 | 77.9 | 2000 |
| tstamp32 | 52,767 | 214,870 | 4,631 | 15,620 | 2,304,000 | 1,619 | 2,250 | 2,250 | 3,313 | 74.5 | 77.9 | 9000 |
| bands32 | 78,978 | 169,416 | 5,450 | 6,450 | 2,304,000 | 1,619 | 2,250 | 2,250 | 3,313 | 74.5 | 77.9 | 8500 |
| hw32 | 32,459 | 110,234 | 2,055 | 6,353 | 2,304,000 | 1,619 | 2,250 | 2,250 | 3,313 | 74.5 | 77.9 | 4000 |
| hw32c | 32,523 | 187,868 | 2,764 | 9,203 | 2,304,000 | 1,619 | 2,250 | 2,250 | 3,313 | 74.5 | 77.9 | 4000 |

age 3 — oracle Δ: 1,588,298 px, 1,936.7 hw tiles (32 px), 7,364.2 tiles@16, 104,375.9 tiles@4

| form | track instr | cull instr | track ns | cull ns | region px | prims | hw tiles S | hw tiles C | inst C | GPU µs S | GPU µs C | mem B |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| full | 61 | 65,141 | 238 | 3,152 | 2,304,000 | 1,619 | 2,250 | 2,250 | 3,314 | 74.5 | 77.9 | 0 |
| zgx | 331 | 65,762 | 566 | 5,386 | 2,304,000 | 1,619 | 2,250 | 2,250 | 3,314 | 74.5 | 77.9 | 320 |
| zgx-vp | 455 | 65,636 | 326 | 4,109 | 2,304,000 | 1,619 | 2,250 | 2,250 | 3,314 | 74.5 | 77.9 | 320 |
| bbox | 63 | 65,528 | -174 | 4,581 | 2,304,000 | 1,619 | 2,250 | 2,250 | 3,314 | 74.5 | 77.9 | 88 |
| tile16 | 4,652 | 130,613 | 699 | 4,792 | 2,304,000 | 1,619 | 2,250 | 2,250 | 3,314 | 74.5 | 77.9 | 8000 |
| tile32 | 1,607 | 130,186 | 3,705 | 4,109 | 2,304,000 | 1,619 | 2,250 | 2,250 | 3,314 | 74.5 | 77.9 | 2000 |
| exact4 | 754,636 | 142,068 | 33,174 | 4,979 | 1,806,657 | 1,619 | 2,236 | 1,975 | 3,307 | 74.1 | 71.0 | 96000 |
| exact32 | 477,362 | 130,225 | 19,816 | 5,451 | 2,022,260 | 1,619 | 2,236 | 1,975 | 3,307 | 74.1 | 71.0 | 2000 |
| stamps16 | 267,735 | 300,064 | 15,768 | 10,874 | 2,304,000 | 1,619 | 2,250 | 2,250 | 3,314 | 74.5 | 77.9 | 113520 |
| sig32 | 1,430,923 | 243,133 | 62,777 | 27,536 | 2,013,776 | 1,619 | 2,236 | 1,967 | 3,307 | 74.1 | 70.8 | 73600 |
| xg32 | 55,607 | 130,567 | 4,363 | 7,070 | 2,304,000 | 1,619 | 2,250 | 2,250 | 3,314 | 74.5 | 77.9 | 2000 |
| tstamp32 | 53,054 | 214,179 | 4,846 | 11,681 | 2,304,000 | 1,619 | 2,250 | 2,250 | 3,314 | 74.5 | 77.9 | 9000 |
| bands32 | 78,657 | 173,366 | 4,323 | 9,097 | 2,304,000 | 1,619 | 2,250 | 2,250 | 3,314 | 74.5 | 77.9 | 8500 |
| hw32 | 32,619 | 111,164 | 2,389 | 8,195 | 2,304,000 | 1,619 | 2,250 | 2,250 | 3,314 | 74.5 | 77.9 | 4000 |
| hw32c | 32,579 | 188,487 | 2,391 | 11,187 | 2,304,000 | 1,619 | 2,250 | 2,250 | 3,314 | 74.5 | 77.9 | 4000 |

#### vlist-jump


age 1 — oracle Δ: 1,356,158 px, 1,786.4 hw tiles (32 px), 6,506.9 tiles@16, 89,399.7 tiles@4

| form | track instr | cull instr | track ns | cull ns | region px | prims | hw tiles S | hw tiles C | inst C | GPU µs S | GPU µs C | mem B |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| full | -294 | 63,420 | -670 | 5,384 | 2,304,000 | 1,572 | 2,250 | 2,250 | 3,347 | 74.4 | 77.9 | 0 |
| zgx | 460 | 63,827 | -38 | 4,700 | 2,304,000 | 1,572 | 2,250 | 2,250 | 3,347 | 74.4 | 77.9 | 320 |
| zgx-vp | 323 | 63,142 | 177 | 2,668 | 2,304,000 | 1,572 | 2,250 | 2,250 | 3,347 | 74.4 | 77.9 | 320 |
| bbox | 406 | 63,412 | 378 | 2,934 | 2,304,000 | 1,572 | 2,250 | 2,250 | 3,347 | 74.4 | 77.9 | 88 |
| tile16 | 4,521 | 126,324 | -118 | 4,514 | 2,304,000 | 1,572 | 2,250 | 2,250 | 3,347 | 74.4 | 77.9 | 8000 |
| tile32 | 1,087 | 126,305 | -1,078 | 5,389 | 2,304,000 | 1,572 | 2,250 | 2,250 | 3,347 | 74.4 | 77.9 | 2000 |
| exact4 | 764,644 | 131,607 | 30,937 | 6,486 | 1,442,520 | 1,572 | 2,237 | 1,791 | 3,335 | 74.1 | 66.5 | 96000 |
| exact32 | 469,903 | 126,391 | 17,206 | 5,993 | 1,834,446 | 1,572 | 2,237 | 1,791 | 3,335 | 74.1 | 66.5 | 2000 |
| stamps16 | 266,355 | 291,819 | 15,099 | 32,390 | 2,304,000 | 1,572 | 2,250 | 2,250 | 3,347 | 74.4 | 77.9 | 113520 |
| sig32 | 1,446,486 | 230,204 | 74,788 | -6,152 | 1,834,446 | 1,572 | 2,237 | 1,791 | 3,335 | 74.1 | 66.5 | 37600 |
| xg32 | 58,486 | 126,299 | 1,524 | 5,787 | 2,304,000 | 1,572 | 2,250 | 2,250 | 3,347 | 74.4 | 77.9 | 2000 |
| tstamp32 | 55,925 | 210,586 | 11,193 | 14,114 | 2,304,000 | 1,572 | 2,250 | 2,250 | 3,347 | 74.4 | 77.9 | 9000 |
| bands32 | 85,040 | 162,072 | 8,825 | 1,506 | 2,304,000 | 1,572 | 2,250 | 2,250 | 3,347 | 74.4 | 77.9 | 8500 |
| hw32 | 34,421 | 107,746 | 2,243 | 7,789 | 2,304,000 | 1,572 | 2,250 | 2,250 | 3,347 | 74.4 | 77.9 | 4000 |
| hw32c | 34,233 | 183,305 | 2,135 | 13,198 | 2,304,000 | 1,572 | 2,250 | 2,250 | 3,347 | 74.4 | 77.9 | 4000 |

age 2 — oracle Δ: 1,537,117 px, 1,954.4 hw tiles (32 px), 7,426.0 tiles@16, 105,072.1 tiles@4

| form | track instr | cull instr | track ns | cull ns | region px | prims | hw tiles S | hw tiles C | inst C | GPU µs S | GPU µs C | mem B |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| full | 62 | 63,362 | 298 | 2,961 | 2,304,000 | 1,572 | 2,250 | 2,250 | 3,347 | 74.4 | 77.9 | 0 |
| zgx | 417 | 63,991 | 142 | 5,645 | 2,304,000 | 1,572 | 2,250 | 2,250 | 3,347 | 74.4 | 77.9 | 320 |
| zgx-vp | 454 | 63,782 | 31 | 3,323 | 2,304,000 | 1,572 | 2,250 | 2,250 | 3,347 | 74.4 | 77.9 | 320 |
| bbox | 166 | 63,593 | 184 | 3,328 | 2,304,000 | 1,572 | 2,250 | 2,250 | 3,347 | 74.4 | 77.9 | 88 |
| tile16 | 4,627 | 126,875 | 653 | 6,040 | 2,304,000 | 1,572 | 2,250 | 2,250 | 3,347 | 74.4 | 77.9 | 8000 |
| tile32 | 1,825 | 126,405 | 6,936 | 16,171 | 2,304,000 | 1,572 | 2,250 | 2,250 | 3,347 | 74.4 | 77.9 | 2000 |
| exact4 | 763,823 | 135,479 | 29,387 | 6,177 | 1,731,504 | 1,572 | 2,241 | 1,964 | 3,337 | 74.2 | 70.8 | 96000 |
| exact32 | 470,973 | 126,503 | 21,099 | 6,353 | 2,010,861 | 1,572 | 2,241 | 1,964 | 3,337 | 74.2 | 70.8 | 2000 |
| stamps16 | 265,944 | 292,802 | 11,865 | 12,733 | 2,304,000 | 1,572 | 2,250 | 2,250 | 3,347 | 74.4 | 77.9 | 113520 |
| sig32 | 1,445,657 | 238,757 | 60,218 | 14,224 | 2,010,861 | 1,572 | 2,241 | 1,964 | 3,337 | 74.2 | 70.8 | 55600 |
| xg32 | 59,265 | 126,303 | 4,617 | 11,345 | 2,304,000 | 1,572 | 2,250 | 2,250 | 3,347 | 74.4 | 77.9 | 2000 |
| tstamp32 | 56,242 | 210,751 | 3,365 | 11,715 | 2,304,000 | 1,572 | 2,250 | 2,250 | 3,347 | 74.4 | 77.9 | 9000 |
| bands32 | 84,692 | 166,740 | 4,073 | 7,990 | 2,304,000 | 1,572 | 2,250 | 2,250 | 3,347 | 74.4 | 77.9 | 8500 |
| hw32 | 34,515 | 107,750 | 2,215 | 6,654 | 2,304,000 | 1,572 | 2,250 | 2,250 | 3,347 | 74.4 | 77.9 | 4000 |
| hw32c | 34,665 | 183,045 | 3,008 | 8,738 | 2,304,000 | 1,572 | 2,250 | 2,250 | 3,347 | 74.4 | 77.9 | 4000 |

age 3 — oracle Δ: 1,727,635 px, 1,999.3 hw tiles (32 px), 7,787.3 tiles@16, 115,639.5 tiles@4

| form | track instr | cull instr | track ns | cull ns | region px | prims | hw tiles S | hw tiles C | inst C | GPU µs S | GPU µs C | mem B |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| full | -73 | 63,254 | -741 | 2,736 | 2,304,000 | 1,572 | 2,250 | 2,250 | 3,346 | 74.4 | 77.9 | 0 |
| zgx | 450 | 63,616 | -476 | 2,843 | 2,304,000 | 1,572 | 2,250 | 2,250 | 3,346 | 74.4 | 77.9 | 320 |
| zgx-vp | 342 | 63,982 | -660 | 5,041 | 2,304,000 | 1,572 | 2,250 | 2,250 | 3,346 | 74.4 | 77.9 | 320 |
| bbox | 44 | 63,638 | -806 | 3,306 | 2,304,000 | 1,572 | 2,250 | 2,250 | 3,346 | 74.4 | 77.9 | 88 |
| tile16 | 4,604 | 127,354 | -43 | 7,296 | 2,304,000 | 1,572 | 2,250 | 2,250 | 3,346 | 74.4 | 77.9 | 8000 |
| tile32 | 1,475 | 126,233 | -1,086 | 4,945 | 2,304,000 | 1,572 | 2,250 | 2,250 | 3,346 | 74.4 | 77.9 | 2000 |
| exact4 | 764,637 | 139,364 | 30,442 | 11,254 | 1,949,339 | 1,572 | 2,244 | 2,015 | 3,339 | 74.3 | 72.1 | 96000 |
| exact32 | 471,095 | 126,201 | 19,562 | 4,788 | 2,063,840 | 1,572 | 2,244 | 2,015 | 3,339 | 74.3 | 72.1 | 2000 |
| stamps16 | 266,233 | 294,101 | 11,387 | 12,500 | 2,304,000 | 1,572 | 2,250 | 2,250 | 3,346 | 74.4 | 77.9 | 113520 |
| sig32 | 1,446,911 | 240,376 | 69,437 | 14,719 | 2,061,757 | 1,572 | 2,244 | 2,013 | 3,339 | 74.3 | 72.0 | 73600 |
| xg32 | 59,944 | 126,109 | 4,390 | 5,012 | 2,304,000 | 1,572 | 2,250 | 2,250 | 3,346 | 74.4 | 77.9 | 2000 |
| tstamp32 | 56,123 | 210,593 | 1,844 | 9,563 | 2,304,000 | 1,572 | 2,250 | 2,250 | 3,346 | 74.4 | 77.9 | 9000 |
| bands32 | 84,712 | 171,247 | 3,241 | 22,230 | 2,304,000 | 1,572 | 2,250 | 2,250 | 3,346 | 74.4 | 77.9 | 8500 |
| hw32 | 34,435 | 108,489 | 2,478 | 6,939 | 2,304,000 | 1,572 | 2,250 | 2,250 | 3,346 | 74.4 | 77.9 | 4000 |
| hw32c | 34,285 | 183,705 | 1,770 | 9,733 | 2,304,000 | 1,572 | 2,250 | 2,250 | 3,346 | 74.4 | 77.9 | 4000 |


## 4. Hot loops: assembly (aarch64, `--release`, thin LTO)

Symbols are kept out-of-line with `#[unsafe(no_mangle)]` wrappers (`src/asm.rs`) over the same generic code
the measured paths inline. Read with `objdump --disassemble-symbols=_lab_hw_mark target/damage/release/lab`.

**Mark (`Hw::mark`, the write-time op).** The tile span takes 9 ALU ops. A span inside one 64-bit word (the
common case) costs `lsr`/`lsl` for the mask, then one load–OR–store per row (unrolled ×4 for tall spans).
There is no bbox or dirty-row bookkeeping.

```asm
ldp   w9, w8, [x1]          ; x0, y0
asr   w12, w9, #5           ; tx0
asr   w11, w8, #5           ; ty0
ldp   w8, w10, [x1, #0x8]   ; x1, y1
sub   w13, w8, #0x1
asr   w8, w13, #5           ; (x1-1)>>5
add   w14, w8, #0x1         ; tx1
sub   w8, w10, #0x1
asr   w8, w8, #5            ; ty1-1
asr   w10, w9, #11          ; word of tx0
cmp   w10, w13, asr #11     ; same word as tx1-1?
b.ne  <two-word path>
...
mov   x14, #-0x1
lsr   x12, x14, x12         ; !0 >> (64 - len)
lsl   x9, x12, x9           ; << (tx0 & 63)
...                         ; per row (short spans):
ldr   x11, [x10]
orr   x11, x11, x9
str   x11, [x10], #0x10     ; next u128 row
subs  x8, x8, #0x1
b.ne  <loop>
```

**Cull (`cull_hw_neon`), 8 slots per step.** The vector block deinterleaves, tests against the bbox, and
reduces to a lane bitmask. Only hit lanes reach the scalar span test (one word load + AND per row, then
push).

```asm
ld4.8h  { v0, v1, v2, v3 }, [x8]   ; 8 NRects -> x0[8], y0[8], -x1[8], -y1[8]
smax.8h v4, v0, v4                 ; max(x0, q.x0)
smax.8h v5, v2, v5                 ; max(-x1, -q.x1)
sqadd.8h v4, v4, v5                ; x0' + (-x1') (saturating: EMPTY stays >= 0)
smax.8h v5, v1, v5
smax.8h v6, v3, v6
sqadd.8h v5, v5, v6
and.16b v4, v5, v4
cmlt.8h v4, v4, #0                 ; both < 0  <=>  non-empty intersection
and.16b v4, v4, v5                 ; & {1,2,4,..,128}
addv.8h h4, v4                     ; lane bitmask
fmov    w8, s4
cbz     w8, <next 8>
sshr.8h v4, v0, #5                 ; tx0
sshr.8h v5, v1, #5                 ; ty0
sshr.8h v4, v2, #5 ; mvn.16b       ; !(-x1 >> 5) = (x1-1) >> 5
sshr.8h v0, v3, #5 ; mvn.16b
...                                ; per hit lane: rbit/clz, ldrsh spans, one-word AND per row, push
```

The compiler keeps the four broadcast query vectors and the lane-bit constant on the stack, reloading them
with `ldr q` per step: 5 loads per 8 slots, harmless. The scalar per-hit part (~25 instructions + push) is
where heavy's cull goes, since nearly every slot passes the bbox there.


## 5. Recommendation for bough

The DESIGN.md §2 plan ("16×16 tiles, per-record stamps, OR of planes, stencil/tile-masked draw") is right in
spirit and wrong in three places that the measurements settle:

- the tile is the GPU's 32×32;
- per-record stamps lose on both CPU and GPU;
- a mask does not save a tiled GPU anything; clipped geometry does.

### 5.1 Data layout

- **Damage plane** = `[u128; ceil(h/32)]`, one bit per 32×32 device-pixel hardware tile.
  - Domain bent to ≤ 4096 px per side, so a row is one `u128`. Use two planes side by side for wider
    windows, or widen the row type.
  - No dirty-row mask and no bbox field: whole-plane clear and OR are ≤ 2 KB of word ops, cheaper than
    tracking on every mark (A10).
  - 800 B at 2560×1600.
- **History** = ring of 4 planes indexed by epoch, i.e. buffer age ≤ 3 + current: 3.2 KB per window. Each
  presentation surface records the epoch it was last drawn at; age = `epoch_now − epoch_surface`.
  - Age 0, unknown, or > 3: full repaint.
- **Bough records** keep `ink: NRect i16x4` in paint order with 64-slot page summaries (union ink), as
  designed. That is all the cull needs.
- **Drop `stamp` from the damage path.** Per-record versions were measured (`stamps16`): cheaper writes,
  dearer queries, and whole-coverage support instead of the symmetric difference (+39% tiles). Keep a stamp
  column only if a non-frame consumer needs record-level "changed since e". For unbounded consumers
  (inspector, accessibility, remote mirror), keep **per-tile u32 epochs** (`tstamp32`, 16 KB at 2560×1600):
  a write is a store, never cleared, and the query is one scan.

### 5.2 Write-time ops (in each binding's patch fn, before the instance write)

`mark(r)`:

```rust
let (tx0, ty0, tx1, ty1) = (r.x0 >> 5, r.y0 >> 5, ((r.x1 - 1) >> 5) + 1, ((r.y1 - 1) >> 5) + 1);
let w = tx0 >> 6;
if w == (tx1 - 1) >> 6 {
    let m = (!0u64 >> (64 - (tx1 - tx0))) << (tx0 & 63);
    for ty in ty0..ty1 {
        plane.word(ty, w) |= m;
    }
} else {
    // two-word mask
}
```

That is ~9 ALU ops plus one load–OR–store per tile row (asm in §4). The support per change, exact for the
pixel model verified in §1.5:

| change | marks |
|---|---|
| content only (colour, opacity, glyph id in the same box, image) | coverage, once |
| uniform rect resized or moved (bars, fills, backgrounds) | `old ⊕ new` as ≤ 4 strips per side (a bar h₁→h₂: one strip) |
| glyph / image / gradient moved | old and new coverage |
| rounded rect resized or moved | `old ⊕ new` ∪ the r×r corner squares of old and of new |
| text run rewritten | compare the old and new runs glyph by glyph (id, x), as SoA columns so it vectorises; mark each changed glyph box once |
| subtree translated (scroll, transform, layout move of a container) | clip ∩ old content bounds and clip ∩ new content bounds: **O(1), never per moved record** |
| mount / unmount of a run | the run's page-summary unions |

### 5.3 Query ops (per presented surface of age a)

1. `acc = plane[e] | plane[e−1] | … | plane[e−a+1]`: at most 3 × 50 `u128` ORs.
2. Cull (`cull_hw_neon`):
   - page summaries: NRect `max` test against acc's pixel bbox;
   - surviving pages, 8 records per step: `vld4q_s16`, 2× `smax`, 2× `sqadd`, `cmlt`, `addv` → lane mask;
   - hit lanes: span test, one u64 AND per tile row.

Measured: 159–171 K instructions per frame on heavy (2,770 live records, nearly all hit), 34 K on
lab-stream, 2–7 K on animate and blink. The lab's phantom glyph slots inflate the cull (3,840 slots for
2,770 live in heavy, 26 K for 1.7 K in vlist). Bough pages hold live records plus page padding only.

### 5.4 GPU consumption: format C (clipped instance stream)

- The cull emits, in paint order, one entry `(instance, clip: i16x4)` per drawn primitive:
  - `clip = coverage` when every tile of its span is damaged (the common case; it touches only damaged
    tiles);
  - otherwise one entry per run of damaged tiles in each tile row it spans.
- One instanced draw over the entries. The vertex shader intersects the instance quad with `clip`.
- `loadAction = .load` into the surface of age a. No scissor, no stencil, no discard, no per-region passes.
- Order: entries never overlap across different runs, so paint order only has to hold within a tile, and the
  list order guarantees it.

Why: Apple's TBDR loads and stores exactly the tiles binned geometry touches, and a mask does not un-touch a
tile. On heavy-stream, format C touches 789 tiles against 3,420 for "scissor + mask" (oracle 762). Modelled
GPU time: 41 µs vs 104 µs.

Measured CPU price: +250 K instructions on heavy for the run splitting of large primitives. **Next
structural step**, not measured here: emit format S (instance only) on the CPU and let the vertex shader
expand the runs from the 800-byte plane bound as a buffer. Draw each partially damaged instance with K quads
(K = max runs per row, about 8); the shader picks run k of its row span by bit scan and degenerates the rest.
CPU cost becomes hw32's 159 K cull, and GPU cost becomes ~2 ns per degenerate quad.

### 5.5 Presentation

- Own a ring of 3 IOSurface-backed textures with exact epochs, and repaint `OR(age)` into the surface being
  presented.
- **Never copy a retained texture.** zgx's full-surface blit costs 255–336 µs per frame (measured), more
  than bough's entire modelled frame on every workload. A blit (82 ns per tile) also costs more than
  repainting the tile (14–49 ns), so copy-forward and scroll-blit lose on this GPU.
- If nothing changed (empty acc), do not encode, present or wake. Idle frames cost zero.

### 5.6 What not to build (measured losers)

| mechanism | measured |
|---|---|
| pixel-precise damage (4–16 px tiles, bands, interval sets) | identical GPU cost under format C, and 1.2–3× the tracking CPU |
| content signatures as the general mechanism | oracle-exact, but 3–10× hw32's CPU; GPU-computed, 13–284 µs of dispatch latency at UI scale |
| summed-area or byte-map culls | no gain at UI scale |
| per-record stamps for damage | see §5.1 |

Keep `sig32` in mind for one niche: content that oscillates within the age window (a caret at age 2 repaints
nothing). If the caret matters, compute signatures only for the tiles the age window touched (hashing cost ∝
damage), not for every write.

### 5.7 Sensitivity

The tile cost was measured at 14 ns (flat content, partial write) to 49 ns (incompressible content). Totals
are CPU + modelled GPU µs:

| tile cost | heavy-stream age 1: zgx as shipped / `hw32c` | heavy-stream age 3 (no copy) | heavy-both age 1 | lab-stream age 1 |
|---|---|---|---|---|
| 14 ns | 348 / 68 (5.1×) | 93 / 59 (1.6×) | 359 / 102 (3.5×) | 196 / 26 (7.6×) |
| 25 ns (the tables) | 376 / 77 (4.9×) | 122 / 70 (1.7×) | 398 / 117 (3.4×) | 201 / 26 (7.7×) |
| 49 ns | 438 / 95 (4.6×) | 184 / 92 (2.0×) | 482 / 157 (3.1×) | 211 / 27 (7.7×) |

Fragments: ≤ 0.4 ps marginal at 4× overdraw, 22 ps when blending-bound at 16× overdraw. The model uses
5 ps under `max()` with tile bandwidth; at 20 ps, heavy-both becomes fragment-bound for every formulation,
which narrows the gap without changing the order.


## 6. Reproduce

```
cd ~/projects/bough
export CARGO_TARGET_DIR=$PWD/target/damage
~/.local/share/cargo/bin/cargo build --release -p bough-damage-lab
B=target/damage/release/lab
$B verify --frames 600 --tag _x            # all forms in forms::registry::V1 (override with --forms a,b)
$B bench  --frames 600 --reps 5 --tag _x   # single-threaded; keep the machine idle
$B dump   --frames 120 --traces heavy-both # for calib/gpu_damage.swift
python3 crates/bough-damage-lab/report.py key verify_x.csv -- bench_x.csv
swiftc -O crates/bough-damage-lab/calib/metal_calib2.swift -o /tmp/c2 && /tmp/c2
swiftc -O crates/bough-damage-lab/calib/gpu_damage.swift -o /tmp/g && /tmp/g crates/bough-damage-lab/results/dump_heavy-both.bin
```
