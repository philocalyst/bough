# Designs left by the threads lane, with their predictions (round 8, at wind-down)

None of this is built unless a line says so. Each section says what was measured, what the design is, what it
is predicted to give and what would kill it. Numbers are mothership (Xeon E-2236, quiet, `dist`, user-mode
counters) unless a line says otherwise; files in `bench/results/r8/threads/`.

## 1. The cold interaction path: where the lines are, and the plan to remove them

**Why it is the number that matters.** An input arrives after other code has run. With every cache level cold
a hover on the sheet costs 9,300 to 12,300 cycles against 3,100 warm; an exposed edit 7,300 against 840.
Warm numbers under-state an interaction by 3x to 9x.

**Measured** (`bench/r8/perf/lines.sh`: every L1-missing load's address, every L3-missing load's address and
every L2-missing instruction fetch, sampled during the measured repetitions only and folded by
`World::regions`; hover a header, 10,000-row sheet):

| what the lines are | data lines loaded, L1 cold: store-tree hit test | pointer path over the table (built) |
|---|---:|---:|
| store tree (stores, sites) | 24 | 5 |
| heap (event scratch, allocator state) | 24 | 18 |
| record pages: records | 13 | 12 |
| record pages: rect lanes | 5 | 4 |
| driver state (`World`) | 9 | 8 |
| damage bitmap | 7 | 9 |
| column table and reader lists | 4 | 6 |
| model, stack, live words | 7 | 8 |
| **total** | **93** | **70** |
| code lines fetched from L3 or memory | 86 | 60 |

Two predictions of the journal died here. Record pages are not "at least half" of a cold hover's data: they
are a fifth. Half was bookkeeping of the event path (a walk of the store tree for the hit test, a recount of
the tree per event, heap allocations on every pointer event). And code is as large as data: 60 to 86 lines.

**Built from it**: the pointer path over the column table with reused scratch (no store walk, no allocation).
Three interleaved runs each, L3 cold: 12,160 -> 9,890 cycles, 407 -> 315 last-level misses; warm 10,888 ->
10,094 instructions. Control cells whose code did not change moved by up to 15 % between runs, so the cold
gain is "about a fifth", not a three-digit number.

**The plan for the rest** (70 data lines, 60 code lines), in the order of what each is predicted to remove:

1. *One interaction block.* The 18 heap lines and 8 driver lines are scratch vectors, the hover set, the
   pointer, the table's vectors' headers, each its own allocation. Put what a pointer event and a block write
   touch (the shown block, the hit list, the hover set, the per-word reader lists, the scratch) in one
   allocation of about 16 lines laid out in the order they are read. Predicted: 26 -> about 12 lines; cold
   hover -10 % cycles. Kill: under 4 %.
2. *Differentiate only pages that can change.* A hover differentiates every page of the columns that read the
   hover word (12 record lines) to change two. Per page and per word read, keep a 64-bit summary of which
   record-constant classes the page holds (for `Eq`-like expressions: a small Bloom mask of the constants);
   the delta skips a page whose summary cannot match the old or the new value. Predicted: record lines 12 ->
   4, rect lanes unchanged, cold hover -8 % cycles, warm hover -40 % instructions (it is 550 instructions a
   page). Kill: summaries cost more than 2 % on scroll-a-page (they are written with the rows).
3. *Code.* 60 lines for a hover: the handler closure's monomorphised body (18), two delta kernels (16),
   `mark_rects`, `Tiles::mark`, `World::delta`. A delta kernel is about 12 lines because both sides are
   evaluated for all 64 records; with (2) the sparse path is the common one and is shorter. Sharing one
   outlined body for `changed_mask`'s packing would remove about 3 lines per archetype. Predicted: 60 -> 45.
   Kill: any warm cell dearer by 2 %.
4. *Prefetch at input arrival* (prediction L4, not run). When the platform hands over an event, before the hit
   test: prefetch the interaction block, the entries of the hot words' readers and the record lanes of their
   pages (addresses known from the last event of that kind). The misses of a cold hover already overlap
   almost fully (about 19 cycles per line from memory), so this is predicted to give at most 10 %, less than
   the journal's 25 %; it is listed last on purpose. bbtree's rule against explicit prefetch (fix layout
   first) applies: do 1 to 3 first.

**The numbers every lane should report**: per interaction, cycles and lines from memory with L3 cold
(`hot.sh`, level `l3`: `cycles`, `fill_dram`), next to the warm instructions; and for a path being worked on,
`lines.sh`'s table. Recipe in `docs/lanes/threads/MEASURE.md`.

## 2. Allocation: one reservation, arenas per epoch

**Measured** (`examples/alloc_x.rs`, a counting global allocator; `bump_x.rs`; mothership, 4 KiB pages):

| step | heap allocations | page faults |
|---|---:|---:|
| model built (10,000 rows) | 1 | 98 |
| world made (the arena reserved) | 3 | 4 |
| first mount (764 records) | 61 | 21 |
| first publish | 0 | 14 |
| first frame, reference raster (16 MB buffer) | 0 | 4,000 |
| every turn cell (idle, hover, theme, edits, scrolls, insert and remove, re-describe) | 0 | 0 |

* Turns allocate nothing and fault nothing (after the pointer path stopped allocating). So a global allocator
  has nothing to be fast on in a turn.
* At mount the whole heap traffic is 61 allocations. Under a bump global allocator that never frees (the
  cheapest possible: an add and a compare) world-and-mount went from 90,117 to 84,830 user instructions
  (-5.9 %) and from 52,134 to 52,199 cycles (nothing). So **a `#[global_allocator]` choice buys at most 6 %
  of mount's instructions and no time**; the journal predicted under 2 % and a kill at 10 %.
* What start-up pays for is page faults: 39 for the scene (4 + 21 + 14) and 4,000 for the first frame's
  buffer. The Mac's "mount rose to 290 K instructions" was this: the arena's reservation and release are
  131 K instructions there with the kernel's, mount itself did not rise (section 11 of the journal).

**Design.**

1. *One reservation at start* (built: the arena). Extend it rather than add allocations: the store tree's
   vectors, the column table, the reader lists and the damage bitmap become spans of the shared part, sized
   at discovery. That removes the 61 allocations and, more usefully, puts the turn's bookkeeping in the lines
   of section 1.1. Predicted: mount -6 % instructions (the bump result is the ceiling), cold interaction as
   1.1. Kill: nothing measurable cold.
2. *A bump arena per epoch* for what a turn or a frame produces and nobody keeps: the list pass's plan, the
   event chain, sweeps, staging for a frame, a decode's scratch. Two of them, flipped at publish, so a frame
   can keep reading what the turn that made it produced. Reset is one store. This is bumpalo's shape with the
   lifetime tied to the epoch instead of a scope; nothing is returned to a general allocator on a hot path
   because nothing is allocated from one. Today the scratch that exists is kept vectors, which is the same
   thing without the shared reset; the arena matters when bulk products exist.
3. *Typed slabs for the few long-lived things* that come and go (stores of lists that appear and disappear,
   media handles): fixed-size slots with a free bitmap, per type, in the shared part.
4. *First-touch control.* The scene's 39 faults: the plan, spaces and live regions each start a page (their
   maximum sizes space them out). Packing a small scene's first 16 KiB as one "front page" (64 spaces, 256
   live words, 128 entries) would make that 3 faults into 1 per half. The 14 at first publish disappear if the
   first frame reads W (nothing is being written yet). Predicted: 39 -> about 20. Not worth more: the frame
   buffer is 4,000.
5. *Huge pages.* Without privileges Linux gives transparent huge pages to an aligned anonymous mapping if
   `madvise(MADV_HUGEPAGE)` is allowed by the system's setting; the arena's column region aligned to 2 MiB
   would then be one TLB entry instead of up to 512. For a scene of 150 KiB it changes nothing measurable; for
   the frame buffer it would (4,000 faults -> 8), but that buffer is `wl_shm`'s and shm huge pages are a
   mount option, not ours to set. Predicted: no effect on any turn of the five scenes; the right lever for the
   first buffer is section 3. Not run.

## 3. The first frame's pixels from the image (for e2e)

e2e's facts: the first `wl_buffer` may be backed by a file the application did not write; that buffer is then
read-only for ever; the host draws its own first buffer while idle and compares; a size or scale mismatch
drops the image; and the first buffer's cost is the kernel's first touch of its pages, the same for an empty
client. Its question: would a pixel section mapped from a file avoid those faults when the file is in the page
cache?

**Yes, on Linux, by two orders of magnitude; no on the Mac.** `examples/pixels_x.rs`, one 2560 x 1600 frame
(16,384,000 bytes), every page touched once:

| | wall | minor faults |
|---|---:|---:|
| anonymous shared mapping, written (a client's first shm buffer), mothership | 5,965 us | 4,000 |
| file in the page cache, mapped shared read-only, read (what a compositor does to a pool made from the image's file), mothership, ext4 | 42 us | 11 |
| the same with `MAP_POPULATE` | 47 us to map, 25 us to read | 11 |
| anonymous, Mac (16 KiB pages) | 930 to 1,140 us | 1,000 |
| file, Mac | 690 to 900 us | 1,000 |

The file's pages are already allocated and filled; mapping them is page-table work only, and this kernel maps
them in large folios (11 faults for 4,000 pages). The client touches nothing; the faults that remain are the
compositor's. Conditions: the file is in the page cache (after a cold boot it is a read of 16 MB instead) and
on a filesystem with large folios; on tmpfs or another filesystem the count will differ and has to be
measured there.

**Format** (reserved in the head, `sections[0]`; not written yet): after the half, at a 64 KiB boundary, the
frame as XRGB8888 rows with stride `4 * width`, for the surface size in `head.size` and the block in
`head.shown`. The loader hands e2e the image's file, the section's offset and length, and the size; e2e makes
the pool from that descriptor. Validity is e2e's compare; the pixels are not covered by the checksum (a wrong
first frame is replaced by the host's own, it cannot break memory safety).

**What I need from e2e to build it**: the scale and buffer transform the pixels must already be in; whether
the pool may be the whole file with the buffer at an offset, or must start at 0 (then the section is its own
file); and who writes it (the host after its first idle draw is the natural writer: `World::save` gains a
pixel slice).

Predicted: first-buffer cost 6 ms -> under 0.1 ms on a warm page cache at this size; kill: a compositor that
copies the pool on attach (then nothing is saved and the section is dropped).

## 4. Frames as immutable versions (the review's priority-0 answer to P05)

Today: one published half, written between frames. A long frame delays the next publish, so a small
interaction waits behind a full raster. The review's answer, which I agree with: a writer edits another
version and publishes one root atomically; readers borrow a snapshot; a stalled reader is bounded by
backpressure.

In this arena's terms: the published side becomes N page-granular versions sharing pages. A publish copies
only dirty pages into free page slots (as now) and writes a new *plan* whose entries point at the new pages
for dirty columns and the old pages for the rest; the root is one atomic word (plan offset, epoch). A reader
(frame or other) pins the root it loaded by an epoch-slot counter; pages are reclaimed when no pinned root
names them. Cost against today: the plan's page offsets become per page instead of per column (a level of
indirection for a frame: one line per 16 pages of a column), and a free-page list per page size. Predicted: a
publish no longer waits for a frame; the turn's cost unchanged (it never touched F); frame paint +1 dependent
line per column. Bounded memory: at most (frames in flight + 1) copies of the pages dirtied meanwhile; a
reader stalled past a limit forces a full copy for the writer or is dropped (it retries). This is the first
thing to build before workers, with Loom or Shuttle on the root and the pin counters.

## 5. Banded CPU kernels on the raster lane's `shade_into` (not started)

Raster measured the layered canvas at 316 ms on one core and 93 ms on six (3.4x) and exposes
`shade_into(&self, u, tiles, out, stride, t: &mut Tile)`, `Scene::worker() -> Tile`, `tile_weight(t)`.

Design: the surface is cut into bands of whole tile rows of equal summed `tile_weight`; band `k` belongs to
worker `k` for as long as the weights hold (re-cut only when a band's weight is off by more than a quarter,
moving one boundary by whole tile rows, so most tiles keep their owner). Each worker keeps its `Tile` scratch
and shades its band's damaged tiles straight into the surface; the scene is read by all (it is the published
half, which nobody writes). Damage under the plan's `cpu_tiles` is shaded by the caller, never queued. The
split is decided from column constants: a store's paints' cost classes times the surface's tiles give the
frame's weight at compile time up to the area factor.

Predictions (P1 of the journal, unrun): at least 4.8x on six cores on the canvas; L2 misses per frame at least
20 % under a rotating assignment; no gain on the sheet's scroll beyond memory bandwidth (16 MB written per
frame). Kill: within 5 % of raster's own six-thread number: the gap to linear is then not placement but
bandwidth or the serial staging, and that is where to look instead. Requires section 4 first if an interaction
is not to queue behind a frame: with versions, a hover publishes while a full frame is being shaded and its
few tiles are shaded by the caller.
