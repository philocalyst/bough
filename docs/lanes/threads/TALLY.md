# Threads lane: lines for docs/TALLY.md (round 8)

Hosts: mothership = Xeon E-2236, quiet, user-mode counters; ilo-prod = Ryzen 7 3700X, busy; Mac = M3 Pro,
instructions including the kernel's. Profile `dist`. Sheet at 10,000 rows unless said. Files in
`bench/results/r8/threads/`; commits on `r8/threads`.

## Kept

* **The scene in one reserved mapping: a working half and a published half of one layout, offsets only**
  (`arena.rs`), under expr's `ColEnt` table. Turn cost against expr `054b83c929` (Mac): hover, theme, idle
  +0.0 %; scroll three rows +1.9 %; a page +0.3 %; exposed edit +2.5 %.
* **Publishing with dirty notes under an epoch; a frame is (block, published half, plan length).** Nobody
  writes a page a frame reads. One note per list per pass. Gates: published == working after every step of
  every sheet script; a frame in flight undisturbed by later turns; `mutant_publish` fails by pixels.
* **Readers on other threads by atomics** (`World::reader`): 2,000,000 publishes, no mixed scene (Mac,
  ilo-prod at the first version; Mac and mothership at the atomic version); `mutant_torn` fails.
* **The scene image**: save, map copy-on-write, adopt by re-description with replayed allocation; a stale
  state is repaired by ordinary damage, another shape or backend refused; 3,000 corruptions (ilo-prod): 762
  refused, 252 repaired, 1,986 harmless, none out of bounds.
* **The pointer path over the column table, no store walk, no allocation**: hover warm 10,888 -> 10,094
  instructions; L3 cold 12,160 -> 9,890 cycles, 407 -> 315 last-level misses (mothership, three interleaved
  runs each; unchanged cells moved up to 15 % between runs).
* **The cost model as a recipe**: `hot.sh` (warm and each level cold: instructions, cycles, branch misses, L1
  and L2 misses, lines from L3 and memory, wall), `lines.sh` (which lines, by address and by function),
  `alloc_x` (allocations and faults), on three hosts; mothership set up as the counters host for a non-root
  user.
* **Cold is the cost of an interaction**: hover 3,100 cycles warm, 9,300 to 12,300 with L3 cold; an exposed
  edit 840 and 7,300 (mothership).
* **Turns allocate nothing and fault nothing** (every turn cell, both hosts).
* **Independent review, batch 5**: P01 and P04 closed for the safe API, P02, P03 and O02 closed; tests added.

## Refuted, with the number

* **Shortening the chain of dependent loads from a block write to a page makes a cold turn cheaper**: five
  loads against two, L3 cold: 12,245 against 12,342 cycles, 470 against 473 lines from memory. No effect.
  (So: no computable column bases.)
* **Record pages are at least half of a cold hover's data lines**: 18 of 93. Half was the event path's
  bookkeeping (store tree 24, heap 24).
* **Code is 150 to 250 of a cold hover's lines from memory**: 86 (60 after the pointer path change).
* **Mapping the image is under a fifth of mounting**: 25.5 us against 41.3 us wall (0.62x); map and adopt
  equals mount. The scene's share of start-up on this core is about 40 us either way.
* **A warm theme switch stays in L1**: 283 to 411 L1 fills; the pages differentiated (about 100 KiB) live in
  L2, as they should.
* **A reader retries under 1 % of its reads**: 0.2 to 0.9 retries per read, because the test publishes 1.4
  million times a second; the prediction named no publish rate.
* **A global allocator choice buys under 2 % of mount**: 5.9 % of user instructions under a bump allocator,
  0 % of cycles; and nothing on turns.
* **Mount makes under 40 allocations**: 61.
* **An epoch-retried read of plain memory is sound** (my first reader): a data race in Rust's model (review
  P02); replaced by atomics.
* **"Nothing in a turn is worth a thread"** as a general claim: measured on one scene with arithmetic layout
  and no shaped text (review P05); stands only for that scene (largest turn 16,000 cycles, a frame 9.4
  million).
* **perf stat slopes and the in-process counters agree within 5 %** (ilo-prod): instructions differ by 600 to
  1,000 per repetition (28 % of an edit, 2.4 % of a scroll); branch and L1 misses agree. Cause: the loop's
  damage read-out is outside the bracket and absent from the floor cell. The in-process table is the cell.
* **`setsid nohup` from an `rx.sh` script starts a remote job**: it did not; `bg.sh` does.
* **Flamegraphs of microsecond cells under the counting harness**: 93 % `[unknown]` (the bracket's system
  calls); usable only for cells of tens of microseconds and up.

## Open, by expected size

* **Frames as immutable versions** (publish one root atomically, readers pin, pages reclaimed): removes the
  wait of a publish on the frame in flight (review P05); the precondition for any worker. Large.
* **The first frame's pixels from the image's file as the first `wl_shm` pool**: first-buffer cost 5,965 us ->
  about 42 us at 2560 x 1600 on a warm page cache (mothership, ext4); nothing on the Mac. Needs e2e's
  first-commit path. Medium.
* **Banded CPU kernels on raster's `shade_into`**, bands of equal weight owned by a core: predicted at least
  4.8x on six cores against raster's 3.4x; kill within 5 % of its number. Not started. Medium.
* **One interaction block** (what a pointer event and a block write touch, in about 16 lines): predicted
  -10 % cold hover cycles. Small.
* **A per-page summary so a block write differentiates only pages that can change**: predicted -40 % warm
  hover instructions, about -8 % cold. Small to medium (expr's delta kernels).
* **Theme: no rect scan for a page whose every record changed** (`mark_rects` is 46 % of a theme switch's
  cycles, ilo-prod flamegraph). Small (expr).
* **Scene bookkeeping (store tree vectors, table, damage bitmap) into the arena; a bump arena per epoch for
  scratch and bulk products**: at most -6 % of mount's instructions; matters when bulk products exist. Medium.
* **A lifetime-carrying arena handle inside the crate** (what is left of review P01). Small.
* **Prefetch at input arrival**: re-predicted to at most 10 % of cold cycles; after the layout items. Small.
* **A floor cell that leaves the same damage** (so perf stat slopes are comparable) and a bracket-off switch
  for flamegraphs of small cells. Small.
* **Product columns and their landing as an epoch; the chat scene; the latency distribution**: not started.
  Large.
