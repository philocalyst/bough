# threads lane: final (r8/threads 392d90a133, code at 57bfea8094, on r8/expr 054b83c929)

Checks run: at 57bfea8094 `cargo check --workspace --all-targets` clean on the Mac (journal section 16); `cargo test
--release -p bough-x -p bough-mem` green on the Mac (bough-mem 7 + align 1; bough-x anim 2, events 9, image 5, publish
4, reflect 6, sheet 4, table 1) and, per the commit message, on mothership. Mutants: `mutant_publish` fails the sheet
gate (published != working, 832,907 pixels), `mutant_torn` the reader gate (86,395 of 834,408 reads saw two
publishes); the real reader: 2,000,000 publishes, 4,949,911 reads, 3.55 retries a read, 0 torn. ilo-prod last ran
the tests at 81da84ea75, before the port. Not on r8/expr a2a2687662, and not merged into main at the close: 19
conflicting hunks in bough-x's world, store, sink and lib against the integrated core; its O02 fix (bough-mem)
was taken onto main alone (28e8d36f22). 392d90a133 adds documents only.

Numbers (dist; mothership = Xeon E-2236, load about 1, user-mode counters, pinned; Mac counts include the kernel's):
- sheet 10,000 rows, 764 records, mothership at 81da84ea75, warm: idle 48 instructions; hover 10,238 / 3,194 cycles;
  theme 36,094 / 11,961; exposed edit 2,164 / 842; scroll a page 44,187 / 16,043 (4.5 us). L3 cold: hover 12,342
  cycles, 473 last-level misses; edit 8,769; theme 23,117. Full frame, reference raster: 9.44 M cycles, 2.12 ms.
- pointer path over the column table (8cbbb91460, mothership, three interleaved runs; unchanged cells moved up to
  15 %): warm 10,888 -> 10,094 instructions; L3 cold 12,160 -> 9,890 cycles, 407 -> 315 misses; lines 93 -> 70.
- port under expr's table against 054b83c929 (Mac): hover, theme, idle +0.0 %; scroll three rows +1.9 %; a page
  +0.3 %; exposed edit +2.5 %. Every turn 0 allocations, 0 faults; mount 61 allocations, 21 faults (both hosts).
- start-up (mothership, wall with system calls): mount 41.3 us, map the image 25.5, map and adopt 40.8 (final tree
  45.4 / 27.0 / 44.9). A 16 MB shared buffer touched once 5,965 us and 4,000 faults; from a cached file 42 us.

Kept: the scene in one reserved mapping, working and published halves addressed by offsets, under expr's ColEnt;
dirty notes published under an epoch, a frame is (block, published half, plan length), nobody writes a page a frame
reads; readers on other threads through atomics with a Snapshot that cannot be kept; the scene image (save, map
copy-on-write, adopt by re-description; 3,000 corruptions on ilo-prod: 762 refused, 252 repaired, 1,986 harmless,
none out of bounds); the pointer path with no store walk or allocation; the counters recipe (MEASURE.md).
Refuted: five dependent loads to two cheapens a cold turn (12,245 against 12,342 cycles); record pages are half a
cold hover's lines (18 of 93); code is 150-250 of them (86); mapping under a fifth of mount (0.62x, saves ~15 us);
retries under 1 % a read (0.2-0.9 at 1.4 M publishes a second); a global allocator under 2 % of mount (5.9 %);
mount under 40 allocations (61); an epoch-retried plain read is sound (data race, P02); nothing in a turn is worth
a thread as a general claim (one scene); perf stat slopes within 5 % of the in-process counters (ilo-prod: +2.4 to
+28 %, the damage read-out lies outside the bracket); flamegraphs of microsecond cells (93 % the bracket's calls).
Review: closed with tests: P01 and P04 for the safe API, P02 (atomics), P03 (plan length from the last publish), O02
(the review's probe is `bough-mem/tests/align.rs`). Standing: P05 (publish waits for the frame in flight); inside
the crate `Ar` is a raw base (rest of P01); size-and-mtime identity is only a filter (rest of P04).
Open: frames as immutable versions (large; before any worker); products and the chat scene (large); first-frame
pixels from the image's file as the first wl_shm pool (medium); banded kernels on raster's `shade_into` (medium;
predicted >= 4.8x on six cores); one interaction block, per-page summaries, theme without a rect scan (`mark_rects`
is 46 % of theme cycles), a lifetime handle in the crate, a floor cell that leaves damage (small each). No worker.
Questions for the user: none recorded by the lane.
Lesson: two million clean publishes did not make a plain-memory read sound; the memory model decides, not the run.
