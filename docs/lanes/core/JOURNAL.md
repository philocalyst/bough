# Core lane, round 7: journal

Branch `r7/core` (worktree `wt/r7-core`), based on `round6` `3b0b00c698`. Brief: `bough/runtime/lead/ROUND7.md`,
section "core". This file is the lane's running record: what was tried, with the prediction and the kill
criterion written before the run, the numbers with their provenance, and what is next. Newest entries last.

Provenance of every number unless a line says otherwise: host = the shared M3 Pro (aarch64, macOS 15.6), under
load from the other lanes (load average 15 to 35; instruction counts do not depend on it), toolchain
nightly-2026-10-17, profile `release` (thin LTO, the workspace default) for the working numbers and `dist` (fat
LTO, one codegen unit) where a line says so. Unit: instructions retired on the calling thread
(`bough_metal::perf::tinstr`), by slope over two repeat counts, as the examples print them.

Commands:

```sh
export PATH=$HOME/.local/share/cargo/bin:$PATH RUSTC_WRAPPER= CARGO_BUILD_JOBS=3 CARGO_TARGET_DIR=$PWD/target
cargo build --release -p bough --example signal_cost --example redescribe_cost
target/release/examples/signal_cost 1 64 256
target/release/examples/redescribe_cost 64 256 1000
MOUNT=1 target/release/examples/redescribe_cost 1000
```

## 0. Baseline on `3b0b00c698` (release, one run each; reproduces the checkpoint's section 3)

| cell | 1 row | 64 rows | 256 rows |
|---|---:|---:|---:|
| idle turn, once | 247 | 247 | 247 |
| colour write, once | 649 | 618 | 606 |
| number write, once | 3,490 | (harness underflow) | 8,715 |
| string write, once | 4,399 | (harness underflow) | 20,857 |
| idle turn, re-describing | 1,244 | 35,684 | 140,219 |
| colour write, re-describing | 1,713 | 35,950 | 140,487 |

Re-description per row (`redescribe_cost`, one plain field changed per turn), owned / static / bare:
721 / 714 / 221 at 64 rows, 660 / 635 / 182 at 256, 656 / 616 / 173 at 1,000. Mount per row at 1,000 rows:
28,123 owned, 28,189 static, 4,578 bare; an empty App 1,337,036.

What the base already is, in the terms of the brief (read from the source, not assumed):

* A signal write already "appends one fixed-size entry and does nothing else": `signal::queue` sets QUEUED on the
  head and logs the head's address (thread-local, 32 entries, an epoch on overflow). The per-write bookkeeping is
  not in the write, it is in what settle runs for it: the wire chain walk (`run_head`: address check, dead check,
  held check, counter, indirect call), the applier's chain `nodes[node] -> runs[run].first -> inst[first]` with
  three bounds checks, and `paint_write` (handle table, generation and live checks, page, scalar rect algebra,
  window clip, tile span, plane mark, paint log).
* The idle turn's 247 are seven out-of-line calls that each return at once (`sync`, `admit`, `settle` with its
  thread-local access and a 256-byte stack array, `effects`, `redescribe`, `lay_flush`, the reclaim test) plus the
  profiler marks.
* A re-describing owner rebuilds every row's description (three 84-byte specs, operands) and streams it against
  a seal word by word with a length check per word and a spec-table lookup per spec.

## Plan and order

The order is by how much each experiment says about the others.

1. **E1, the turn is its journal** (façade, real cells): idle guard; paint writes journaled and resolved once
   before the frame; settle without the general machinery for the common case. Tells whether "derive at the end"
   beats "do at the write" when a turn has one write (the common case), which decides the shape of everything else.
2. **E2, planes** (bough-core, real pages and damage planes): the journal's resolve by page with bit-planes; the
   many-dependents case (one write, 10,000 records) as page-at-a-time vector work against the per-record path.
3. **E3, the view as compiled data**: stamping a template into pages (mount) and operand-only seals (idle
   re-description).
4. **E4/E5, borrowing and the static schedule**: taken as they fall out of 1 to 3, listed with the dynamic cases.

## E1. The turn is its journal (façade; 2026-11-12)

Baseline re-measured with frame columns added to `signal_cost` (the same example source built against
`3b0b00c698` in a second worktree; release; one run): at 1 / 64 / 256 rows, once mode: idle 264, colour
658 / 627 / 615, colour + frame 1,743 / 1,444 / 965, idle + frame 322. (The extra 17 on the idle turn against
section 0 is the example's larger match.)

**Predictions, written before the runs.** (a) Inline guards on every stage of a turn: idle turn 247 -> at most
60. (b) Paint writes journaled (an append at the write, damage and paint log derived before the frame): the
colour applier loses its ~110 of damage work; colour turn at 256 rows at most 606 - 110 + 15 = 510 without (a),
at most 320 with it; turn + frame not worse than base. (c) A settle that skips the general machinery for one
logged cell: colour turn at most 200. Kill for (b): if turn + frame is worse than the base's, the deferral by
itself is not a win and only a cheaper projection can make it one.

**Results** (release, one run each, 256 rows unless said; all once mode):

| step | idle | colour | colour + frame | idle + frame |
|---|---:|---:|---:|---:|
| base `3b0b00c698` | 264 | 615 | 965 | 322 |
| (a) guards + (b1) journal as `Vec<(key, old, new)>`, netted per record at the frame | 101 | 514 | 1,091 | 161 |
| (a) + (b2) record-space plane in the core (`dirty.rs`), projected at the frame | 101 | 493 | 995 | 177 |
| (b2) with the projection's cold path out of line and a one-byte pending test + (c) one settle wave | 101 | 435 | 918 | 162 |

At 1 / 64 rows the last line is: colour 436 / 436 (base 658 / 627: the one-row regression of the write log is
gone, the cell no longer depends on the row count), colour + frame 1,684 / 1,399 (base 1,743 / 1,444).

What the numbers say:

* **(a) held less than predicted: 101, not 60.** The idle turn is now a straight line of independent tests
  (disassembly read: 75 instructions in `turn` itself, 13 of them the thread-local access for the write log,
  the rest one or two loads per mechanism: reclaim queue, orphans, profiler flag, five `pubs` stores, producers,
  audit flag, rescan flag, list ends, publications, committed readers, `once`, three layout queues, the list
  regions' dirty scan). Each mechanism still owns its own "have I work" word. One word that every producer
  raises would make it about 20; that is a change to every queue's push site, not to the turn.
* **(b1) failed its kill criterion: colour + frame 965 -> 1,091.** A generic deferred resolve (a 768-byte
  netting table cleared per frame, `Vec` bookkeeping, the same scalar rect algebra as before) costs more than
  marking at the write when a frame has one write. Deferral is not a win by itself.
* **(b2) is a wash for one write, by measurement: about -95 at the write, about +100 at the frame.** The
  applier went from ~159 to ~65 (attribution spans, ±30 under load) and the projection of one record costs
  ~100: the tile-plane update itself (span arithmetic, the plane's occupied-row range, the row loop) is half of
  it and does not get cheaper by being done later. The lead's "110 of 159 are damage bookkeeping" is right about
  where the instructions are and wrong that deferring removes them; for a single write they move.
* What record-space-first does buy, measured: the turn no longer touches frame structures (colour turn
  615 -> 435, with (a) and (c)), N writes to one record project once, and the plane is there for other readers.
  What it does not buy yet: the frame's fixed cost (colour + frame is 918 against 965).
* **(c)**: the second settle wave (another thread-local access, a 256-byte array cleared, a scan of nothing) is
  gone; -58.
* **Still there at 435**: idle 101 + the write 48 (compare, store, QUEUED, the log's thread-local) + settle
  ~220 (log read, rank lookup, the chain walk with a 56-byte wire copy, address / dead / held checks, a counter,
  the indirect call) + the applier ~65 (node, run and instance bounds checks, the hover and border selects, the
  opacity multiply, the paint log). "Under 60" is not reachable on this base without the write applying its
  own store (surface-api's direct wires measure 62 above idle); the general form of that is in section E4.

**At the `dist` profile** (fat LTO, one codegen unit; commit `32aee86bce` against `3b0b00c698`, both built
with the same example source; three runs each, identical to the instruction; Mac under load average 45;
`data/r7/dist-and-verify-32aee86bce.txt`), once mode, base -> head:

| rows | idle | colour | colour + frame | idle + frame |
|---|---|---|---|---|
| 1 | 188 -> 95 | 546 -> 366 | 1,672 -> 1,657 | 259 -> 170 |
| 64 | 188 -> 95 | 514 -> 366 | 1,358 -> 1,355 | 259 -> 170 |
| 256 | 188 -> 95 | 503 -> 365 | 870 -> 865 | 259 -> 170 |

The shipping profile says it more plainly than `release` did: the turn is 27 to 33 % cheaper and the idle turn
is half, but a colour write counted with its frame is unchanged (-5 of 870). What left the write arrived at
the frame. Re-describing mode is unchanged throughout (154 K at 256 rows; 623 per row at 1,000), as it should
be: nothing here touches the walk. The description-build floor at `dist` is 53 per row (operands 19).

Correctness: the paint ring is read in four places (`repair_rows`, `copy_plan`, `frame_as`, `advance`), all
behind `frame_as`, which projects first. A projection after a scroll marks the record where it is now; that is
the exact rule (the copy brought the record's old pixels to the new place, and the old place holds copied
content). Records released since their write are skipped; their removal damaged them.

## E2/E3 model: the page pipeline (`crates/bough-pages`, no dependencies; Mac and ilo-prod)

A measuring instrument for the lead's notes 1 and 2 and for items 2 and 3 of the brief, in the smallest real
form: pages of 64 records with rect lanes and bit-planes; stage 1 (classify pages) and stage 2 (project set
bits to a tile bitmap of any window size, with an effect-radius lane); the same projection over sub-ranges on
workers, merged by OR; the page as it is (an insert shifts every column and re-points handles) against a page
with stable slots and a 64-byte selection vector for paint order; a directory that instances shared template
page images; operand lanes compared a page at a time as the seal. Four tests hold the variants equal to each
other (`cargo test` in the crate).

**Predictions, written before the first run** (instructions unless said; aarch64):

1. The write (`mark`): at most 8 per write at any size.
2. Stage 1 by scanning every page word: about 1.5 per page, so ~2,300 per idle frame at 100,000 records and
   ~23,000 at 1,000,000. Kill: more than 1,000 at 100,000 records means a scan cannot be the idle path and a
   summary level is required; predicted to be killed. Stage 1 by summary: under 60 per idle frame at any size.
3. Stage 2, one record of a list: at most 45 per record on the scalar path. The lane path costs ~250 per page
   whatever is set, so it loses below about 6 to 8 set bits per page and wins above.
4. Stage 2, every record of a page set (a theme switch): at most 14 per record with lanes against ~40 scalar.
   Kill for the lane path: less than 2x at full pages. One column of a 10-column table (6.4 bits per page):
   about equal.
5. Workers: at 1,000,000 dirty records 8 threads give at least 4x in wall time; below ~20,000 records no gain
   (scoped threads cost 50 to 100 µs to start). Kill: 16 threads under 3x at 1,000,000.
6. Selection-vector page: an insert + remove pair at most a third of the shifting page's at fill 32 (about 300
   -> 80); an ordered walk of a permuted page at most 1.5x the direct walk per record; an append-only page walks
   at 1.0x. Kill: the permuted walk above 2x.
7. Mount of 100,000 rows of 3 records: per record ~40 per row in the model (the real `insert_run` is 84 per
   record, so the model flatters the baseline); copied page images at most 12 per row; shared images under 1.
8. Idle re-description over an operand lane: at most 6 per row at 1,000 rows. Kill: above 30.

### Results of the model

Hosts: Mac = M3 Pro, aarch64, under load (instructions and cycles of the calling thread; no miss counters);
ilo = ilo-prod, Ryzen 7 3700X (Zen 2), x86-64, load average 17 to 20 during the run (a busy host), counters by
`perf_event_open` in process, user mode, pinned to CPU 2 for the single-thread cells; "cache misses" on this PMU
are L2 misses. Builds: release with fat LTO (the crate's own profile); ilo twice, the default x86-64 target
(SSE2) and `-C target-cpu=znver2` (AVX2). Minimum of five samples. Raw files: `data/r7/pages-*.{csv,txt}`.
Model commit: the one that adds this section.

| # | cell | Mac | ilo SSE2 | ilo AVX2 | prediction | verdict |
|---|---|---|---|---|---|---|
| 1 | the write (`mark`), per write incl. the loop's index load | 14 | 16 (0.001 L2 misses) | 16 | at most 8 | missed: ~10 for the mark itself; flat from 64 to 1,000,000 records |
| 2 | stage 1 by scan, idle frame, 100,000 records (1,563 pages) | 5,645 | 14,435 | 4,770 | ~2,300; kill above 1,000 | killed, as predicted: 3 to 9 per page |
| 2 | the same, 1,000,000 records | 55,049 | 143,411 (113 L2 misses) | 46,732 (254) | | |
| 2 | stage 1 by summary, idle, 100,000 / 1,000,000 | 160 / 1,260 | 209 / 1,749 | 209 / 1,749 | under 60 | missed at scale: the summary itself is scanned; a top word (or one pending byte, as `dirty.rs` has) makes it one load |
| 3 | stage 2, one record of a list, scalar path, per call | 203 | 231 | 214 | at most 45 | missed: ~120 fixed per call + ~80 per record |
| 3 | the same through lanes | 477 | 643 | 366 | ~250 per page | the page costs 280 to 560 whatever is set |
| 3 | 8 of 64 bits: scalar / lanes, per record | 92 / 97 | 110 / 123 | 108 / 87 | crossover at 6 to 8 | crossover is at 8 (AVX2) to ~16 (NEON, SSE2) bits |
| 4 | every record of every page (theme switch, 100,000): lanes / scalar | 18.4 / 51.2 | 22.5 / 60.8 | 17.6 / 61.9 | 14 / 40; kill under 2x | 2.7x to 3.5x: holds. 3.2 to 5.5 cycles per record |
| 4 | one column of a 10-column table (10,000 of 100,000; 6.4 bits per page): lanes / scalar | 69 / 58 | 95 / 69 | 51 / 67 | about equal | scalar wins on NEON and SSE2, lanes on AVX2 |
| 5 | stage 2 on workers, 1,000,000 dirty, wall ms at 1 / 2 / 4 / 8 / 16 threads | 3.24 / 2.44 / 1.58 / 1.04 / n/a | 4.20 / 1.94 / 1.16 / 1.10 / 0.89 | 3.53 / 2.10 / 1.48 / 1.06 / 1.14 | 4x at 8; kill 16 under 3x | 3.1x to 3.8x at 8, 4.7x at 16 on a busy host: not killed, under the prediction |
| 5 | the same at 100,000 dirty, wall µs at 1 / 2 / 16 | 102 / 221 / n/a | 141 / 180 / 373 | 143 / 214 / 440 | no gain below ~20,000 | no gain below ~500,000: starting scoped threads costs 78 µs (2) to 384 µs (16) per call |
| 6 | insert + remove pair, fill 32: shifting page / selection page | 927 / 188 (70 / 11.6 ns) | 668 / 158 (68 / 13.7 ns; 1.65 / 0.13 branch misses) | 648 / 154 | a third | 4.2x to 4.9x in instructions, 5x to 6x in time |
| 6 | ordered walk per record: direct / selection, append-only / selection, permuted | 5.6 / 5.7 / 12.6 (0.40 / 0.40 / 0.55 ns) | 6.4 / 6.5 / 13.0 (0.53 / 0.53 / 0.64 ns) | | 1.0x and at most 1.5x; kill above 2x | 2.3x in instructions (killed by the letter), 1.2x to 1.4x in time: the loads are independent |
| 7 | mount 100,000 rows of 3 records, per row: per record / copied images / shared images | 249 / 17.9 / 1.68 (13.3 / 1.65 / 0.05 ns) | 196 / 10.5 / 2.04 (27.8 / 0.85 / 0.22 ns; 0.61 / 0.07 / 0 L2 misses) | same | 40 / 12 / under 1 | per record is 83 per record, the same as the real `insert_run` (84); copy and share a little over the prediction |
| 8 | idle re-description over an operand lane, per row | 4.5 to 4.7 (0.19 ns) | 17.9 | 7.2 to 7.4 | at most 6; kill above 30 | holds on NEON and AVX2 |
| | scroll: translate one page (frame word) / re-cull one page (lanes) | 3.9 / 271 | 4.0 / 266 (1.2 L2 misses) | 1.7 / 134 (2.1) | | 4 instructions per record to re-derive visibility |

In the façade, on the same question as cell 8 (`BUILD=1 redescribe_cost`, release, Mac): building the
application row's description (three specs, tint, integer label, borrowed string label) costs **60 instructions
per row**, and its operands alone (id, label slice, tint) 19, against 650 to 664 per row for the whole idle
re-description. So ~590 of the ~650 are the lockstep walk and the stream-form seal check, not the description.
The store lane's handoff put the floor at "the cost of rebuilding the description"; that floor is 60, not 300.

### What the model says about the lead's notes

1. **"Damage lives in the wrong space."** Half right. Record-space first is cheap at the write (10
   instructions, no dependence on size) and gives every other consumer the same planes. It does not make the
   one-write frame cheaper: projecting a record to tiles costs 80 to 100 whenever it is done (E1: -95 at the
   write, +100 at the frame). The win is in fan-out: a page with every bit set projects at 18 per record
   instead of 51 to 100, and N writes to one record project once.
   * "The ring of planes can be a ring of record-space words": no. A geometry change needs the old rect, which
     is gone after the write unless it is kept; the tile plane *is* the compact record of "where it was". The
     ring stays in tile space; record-space is the epoch's working form. (For paint-only epochs a record-space
     ring would work, but then the record must not have moved since, which is the same condition the paint log
     already tests.)
   * The 4,096 px limit: the model's tile bitmap is rows of `stride` words and costs nothing extra in the
     single-word case (the projection's tests hold at 5,120 x 2,880). That part of the note stands.
   * Effects: an effect-radius lane makes "a blur damages more than it covers" one saturating add in the
     projection, no branch (in the model and its test).
2. **"The frame as a simdjson pipeline."** The shape works and the tests hold the scalar, lane and worker
   forms equal. Three corrections from the numbers:
   * Stage 1 must not scan. One load and one compare per page is 3 to 9 instructions per page in practice
     (5,645 per idle frame at 100,000 records); idle has to be one word. Summaries (a word per 64 pages, and one
     above it) make stage 1 proportional to touched pages. simdjson scans because every byte of its input is
     new; ours is almost entirely unchanged from the last frame, so the right analogue of its stage 1 is the
     *write* setting the bit, not a pass finding it.
   * Lanes win only on dense pages: the page-at-a-time span computation costs 280 to 560 instructions, so below
     8 to 16 set bits the per-bit path is cheaper. Both forms are needed, chosen by popcount.
   * "The one-core loop and the many-core loop are the same loop" is true and, for projection, useless: a
     million dirty records project in 3 to 4 ms on one core and nothing real has a million; at 100,000 every
     thread count loses. A persistent pool would remove the 78 to 384 µs start but not change the conclusion.
     The pages-as-unit-of-work argument should be spent where a page costs microseconds, not nanoseconds: mount
     (shaping, layout), raster and binning. Stage 2 stays on one core.

### The reformulation the measurements point to

Four of the five items of the brief and both of the lead's notes reduce to one change of representation:

**A record's identity is its place: (page, slot), and a slot never moves.** Paint order inside a page is a
64-byte selection vector; between pages it is the directory. Then:

* the handle table (24 B per record, a dependent load on every write through a handle, a re-point for every
  record an insert shifts) does not exist: 4.2x to 4.9x fewer instructions per structural edit, measured;
* every per-record column outside the page (instances, state planes, dirty planes, operand lanes) is addressed
  by the same index and never has to be carried by an insert, a split or a merge: the store lane's "ids at
  columns" and the core lane's "state planes carried through every page-moving site" become the same thing and
  the carrying code is deleted;
* dirty planes are in the pages' own space, so a dense page projects as lanes with no gather (cell 4), and
  stage 1's summary is the directory's;
* a page that was only appended to is its own order (the walk is 1.0x); a permuted page costs 1.2x to 1.4x in
  time for an ordered consumer. The GPU's bin pass is the consumer that matters and needs one byte load per
  record on permuted pages only: the compute lane has to agree to that.

**A page image is shared until it is written.** The directory entry carries the image, the offset and the first
id. A run of identical-shape rows (a list, a table, a grid) is `lcm(k, 64)` records of template tiled once and
one directory entry per page: mount of the geometry is 1.7 to 2 instructions per row and no cache miss, against
196 to 249 and 0.6 L2 misses. What a row really owns is its operands, in lanes; those lanes are also the seal
(cell 8) and the instance colours the GPU reads. The façade's tape, scopes, per-item seals arena, node mirrors
and the per-record handle allocation are bookkeeping for the fact that today every row is stored as if it were
unique.

What has to be true in the façade for that: a description is (shape, operands). The builder types already fix
the shape; the description value is 60 instructions to build, of which 19 are operands. Nothing has to be
proved about the row closure for the idle case: computing the operands and comparing them with the lane is 4.5
to 7 per row (cell 8) plus the 19. The static part does not have to be hoisted by a macro to reach "under 30":
it has to stop being *compared*, which the shape's type already licenses for everything that is a literal in
the builder chain. That needs the builders to tell literal from operand: a `const` spec per call site
(`&'static Spec`, compared by address) is enough and is expressible today with an inline `const { }` block.

### E4, borrowing: where a run-time check exists only because the API hides what was provable

Read in the disassembly of the colour applier and the settle (base, 159 + ~220 instructions):

| check, per write | what would prove it instead |
|---|---|
| `nodes[node]`, `runs[run]`, `inst[i]` bounds (3 compares and branches, 3 panic paths) | ids that are positions in columns reserved to capacity: a mask, or no check (stable slots) |
| handle table: bounds, live, generation, null page, slot under 64 | identity = place; a slot is live by the page's live plane; no generation because nothing is re-pointed |
| wire: address check (`w.head == addr`), dead flag, 56-byte copy of the wire to call through it | wires of a mounted shape are one contiguous table released as a range with its block; a `Signal` moved out of its owner is the one dynamic case and is caught when the block is released, not on every run |
| HELD test on every wire run | a held cell is a bit in a plane beside QUEUED; settle masks once per word |
| hover flag, border select, opacity multiply inside the colour applier | variants of the *shape* (three appliers chosen at mount), not branches at the write |
| paint log capacity and overflow | content addressing by lane compare of the operand lane against its epoch-start copy, per touched page |
| per-turn: seven mechanisms each asking "have I work" | one word every producer raises; the turn tests it once |
| the thread-local for the write log (13 instructions at the write, 13 per settle) | writes through `&mut Turn`; kept only for `DerefMut` convenience |

The one borrow-level fact not used at all: a turn whose handler was handed `&V` and not `&mut V` cannot have
changed plain fields. `App::poll()` (async admission) and every read-only handler re-describe today
(140,219 instructions at 256 rows) for nothing. A handler type that asks for `&V` plus the turn's writer for
signals makes the idle re-description disappear by type, not by compare.

### E5, the static schedule: what is static and what needs a fallback

Static per shape (known when the view type is monomorphised, or at its first mount): the set of wired cells of
an owner type and their order (class rank, already computed at mount), the wires of each cell that come from
the fixed part of the shape and their targets as offsets from the block's base. For those, settle is: for each
set bit of the owner's QUEUED word, in bit order, run a contiguous run of (applier, target offset) entries.
No log, no sort, no chain, no per-wire flags.

Dynamic, each needing its own small mechanism:

1. **Dynamic parts (`each`, `Option`, `Vec`) whose items wire cells of an outer owner** (the shape of
   `signal_cost`: every row wires `self.tint[i]`). The wire set of the cell changes when items mount and
   unmount. Fallback: per cell, a second run of entries appended at mount and compacted when a block is
   released; the settle loop is the same. Size: the entries themselves.
2. **List rows and entities as owners**: each row is an instance of a class with a static schedule; what is
   dynamic is *which* rows have a queued bit. One summary bit per row page (the list already stores rows in
   64-item chunks).
3. **Conditional shapes that change the wire set** (`Option<E>` flipping): the same as 1.
4. **Async holds**: a cell held by a pending cohort runs only its `latest` wires. A HELD plane beside QUEUED;
   release = OR the held bits back into QUEUED.
5. **Wires that queue other cells** (derived values, `View::changed`): order by rank makes a forward pass
   sufficient when a wire only queues higher ranks; a wire that queues a lower rank needs a second pass, which
   is today's wave loop. Field declaration order is a topological order for derived cells declared after
   their sources; anything else is detectable at mount (rank of target < rank of source) and can keep the loop.
6. **Signals moved between owners at run time** (`mem::swap` of two `Signal`s): the head must not carry the
   other owner's schedule. Today a per-wire address check on every run; instead the head can hold its own rank
   and the settle indexes the owner's table by offset, so a moved head is simply a different cell.

### Generalising damage (Amendment 1)

One notion: **a class bit on a record, in the epoch's planes, projected by one pass that reads lanes.** What
each consumer the amendment names needs from it:

| consumer | class bit | lane the projection or reader uses | status |
|---|---|---|---|
| paint (colour, alpha) | PAINT | ink, clip | real (`dirty.rs`, `project_paint`) |
| geometry (move, resize) | GEOM | old rect kept at first touch + new rect; edge bands | designed; the core's `set_ink*` family still marks tiles at the write |
| text | TEXT | the label's row masks (bough-text computes them) | designed |
| media tiles | MEDIA | the tile's rect within the record | designed |
| effects (blur, shadow) | any | effect-radius lane: grow before clipping | model, tested |
| time-driven shaders, transitions | ANIMATED plane (not an epoch bit: stays set while it runs) | swept bounds lane, written when it starts | model (`anim & visible` is work with no turn) |
| native views | GEOM on the hole's record | ink | nothing special: a hole is a record |
| accessibility | TEXT, STRUCT | none: reads set bits | designed |
| async holds | HELD plane | none: masks stage 1 | designed (with the async lane) |
| instance upload ranges | PAINT, GEOM | none: set bits are the ranges | designed |

A transition as a kernel parameter touches the core in two places only: the record keeps `(from, to, t0,
duration, ease)` in an operand lane the kernel reads, and its ANIMATED bit and swept-bounds lane are written
once when it starts. No turn runs while it plays; stage 1 ORs `animated & visible` into the work word, which
the model does.

## State at the end of this session (2026-11-12)

* Branch `r7/core`, two commits on `round6` `3b0b00c698`. Nothing pushed. Not rebased onto the integrated base
  (no word from the lead that it is ready).
* In the tree and real: the guarded turn, the record-space paint plane and its projection (`bough-core`
  `dirty.rs`, `Bough::{mark_paint, paint_pending, project_paint}`, `Rt::{paint_write, paint_resolve}`), the
  single settle wave, `tests/dirty.rs`, the frame cells of `signal_cost`, `BUILD=1 redescribe_cost`, the `attr`
  feature's spans (noisy under load: ±30 to ±100 per span; good for "which half", not for a number).
* In the tree and a model: `crates/bough-pages` (its own workspace; `cargo test` and
  `cargo run --release --bin pages_bench -- [cell prefixes]`). On ilo-prod: `/root/work/r7/core` (`env.sh`, a
  hard-linked toolchain, `src/bough-pages`, results in `out/`).
* Not done from the brief: geometry, text and media classes of record-space damage (the core's `set_ink*`
  family still marks tiles at the write); damage planes of any window size in the real core (model only);
  stable-slot pages and shared images in the real core (model only); the single work word for the idle turn;
  x86-64 twins of the core's NEON kernels (the new projection has a portable fallback, built on ilo-prod only
  as part of the model, not as `bough-core`); perf counters on `bough-core` itself with the bench lane's recipe.

## Failed or refuted, in one place

1. Deferring damage with a generic journal and a netting resolve: colour + frame 965 -> 1,091. Killed.
2. "A colour write under 60 including the append" on this base: 435. The damage half moved to the frame at
   equal cost; the rest is settle machinery and the applier's chain, which only a write that applies its own
   store, or the static schedule, removes.
3. Idle turn under 60 by guards alone: 101.
4. Stage 1 as a branch-free scan of every page: 3 to 9 instructions per page; killed for the idle path.
5. Stage 2 on many cores: no gain below ~500,000 dirty records.
6. Projecting one record in 45 instructions: 80 to 100, in the model and in the real core.
7. The selection-vector page's ordered walk by instruction count: 2.3x (1.2x to 1.4x in time).
8. "The floor of idle re-description is rebuilding the description" (store handoff): the build is 60 of 650.
9. A ring of record-space planes instead of tile planes for buffer-age repair: refuted by argument (old rects).

## Next, in order

1. **Phase B on the integrated base, smallest first**: the guarded turn and the record-space paint plane
   (conflicts: `rt.rs` `paint_write`, `app.rs` `turn` / `frame_as`; surface-api's `direct_flush` becomes a
   `mark_paint` per dirty entry and loses its `old` field if content addressing moves to lane compare). Then the
   single work word (prediction: idle turn 101 -> at most 25).
2. **Stable slots in the real core** behind the existing `Bough` API, as an experiment with the differential
   suite as its oracle: `Page` gains `ord` and `live`, `insert_in_page` stops shifting, `fixup_handles` is
   deleted, the handle becomes (page id, slot). Predictions: insert8-front at 1e3 3,961 -> under 1,500;
   set-ink-handle loses the handle-table load (387 -> ~340); cull and hit on append-only pages within 1.00x; on
   permuted pages hit +10 % at most. Kill: any spatial cell on append-only pages above 1.02x, or the shader's
   bin pass needing more than one extra load per record (ask the compute lane first).
3. **Shared page images**: directory entries carry (image, offset, first id); `insert_run` of a tiled template
   writes entries; first write to a shared page copies it. Prediction: the storage part of mounting 100,000
   three-record rows from ~250 to under 20 instructions per row; resident bytes per row of the core's pages
   from 69 B to under 2 B until rows diverge. Needs the façade to hand the core a template, which is 4.
4. **(shape, operands) in the façade**: specs as `&'static` per call site compared by address; operand lanes as
   the seal and as the instance's colour column. Prediction for the idle re-description of the application row:
   650 -> at most 100 with today's builders (60 to build + lane compare), at most 30 with static specs.
5. **Geometry class in record space** with the old rect kept at first touch, then text (labels hand their row
   masks to the projection), then the tile bitmap as rows of `stride` words (removes the 4,096 px panic).
6. Cache-miss counters on `bough-core` with the bench lane's recipe (`bench/perf/pstat.sh`, r7/bench), at 1,
   64, 256 and 100,000 records, before and after 2.

# Phase B (from 2026-11-12, on the integrated base)

Rebased onto `round6` `2db93d2120` (core and store lanes merged). One conflict (`redescribe_cost.rs`, both
sides kept). On the new base, release, same example source built against `2db93d2120` in a second worktree,
once mode, 1 / 64 / 256 rows: idle 266 -> 101; colour 649 / 618 / 606 -> 432 / 432 / 431; colour + frame
1,729 / 1,432 / 954 -> 1,677 / 1,390 / 911; idle + frame 323 -> 161. The colour applier alone
(`APPLIERS=1 redescribe_cost`) is 45 instructions (store's columns and the deferred damage together; it was
159). Idle re-description 299 per row at 1,000 rows (store's seal work), description build 60.

**A measurement error of phase A, found here.** `signal_cost` writes row `7k mod n`; at 256 rows only the 25
rows inside the 800 px window are visible, so nine writes in ten damage nothing and their frame returns at
once. The 256-row "colour + frame" cell is therefore mostly idle frames, and the phase A sentence "a colour
write counted with its frame is unchanged (870 -> 865)" understated the frame: the one-row cell, where every
write is visible, is the real one-record frame, 1,245 instructions above the turn on the rebased head.

## B0. Derived geometry in rank order (a correctness defect; outranked the performance steps)

Reported by the bench lane (r7/bench `0dd77a69b2`, `comparisons/bough-workloads/tests/defects.rs`): two nested
`clip_by`s and an `offset` wired to one signal; after the third write retained != fresh mount.

**Mechanism, verified in the code and by four failing tests** (`crates/bough/tests/geometry_order.rs`,
headless, comparing every record's ink and clip rect with a fresh mount):

1. `Rt::add_wire` prepends, so a cell's wires run newest first. `patch_clip_by` computed the clip's absolute
   rect at once from the parent clip's *current* rect and the space's *current* origin. The inner clip's wire
   ran before the outer's and before the offset's: stale parent, stale origin. (The repro's first difference,
   x = 45 against 50, is the inner clip cut to the outer's previous rect.)
2. The same staleness without any ordering: a `clip_by` whose own signal was not written did not follow an
   `offset` that moved its space, nor a parent `clip_by` that moved (nothing recomputed it).
3. A `clip_by` inside an `anchor` was computed at mount with the anchor's space at (0, 0); the layout's
   placement moved the space and the view's own clips, not the wired one (the second report: confirmed).
4. Found while fixing: a constant `clip(..)` under an `offset` or a `clip_by` was an absolute rect fixed at
   mount, so it followed neither.

**Fix, by construction.** A clip whose inputs can change is a *derived* value with three inputs: its local
rect, its space's origin, its parent clip's rect (`template::WClip`). Nothing writes its absolute rect
directly. A wire stores the local rect; a space move (`set_space` / `shift_space`) and a change of a clip
through the façade (`Rt::set_clip`) raise the lowest rank that reads them (one load: a per-space and a
per-clip "lowest dependent rank"); `Rt::clips_settle` recomputes from the lowest raised rank upward in one
forward pass. **Rank is mount order**: a clip's parent clip and the space it is in (and the spaces that one
is nested in) are always mounted before it, so the pass sees every input final, whatever order the writes
came in. It runs when the wires of a cell have run (`run_head`) and after a layout flush has placed its boxes;
it is one compare when nothing was raised. A constant `clip` under a moving parent takes the same form (its
rank goes on the re-description tape).

Negative control: the pass in reverse rank order fails three of the four tests.

Not covered, stated: derived-clip entries are never released (the table grows with mounts, like the existing
`views` / `offsets` / `wclips` tables); a constant clip inside a virtual-list row is still an absolute rect
(making it derived needs the release, or the entries grow with every scrolled row); tooltip clips are set
without raising; list port clips (`region_set_port`) read their parent clip at the write and are raised to
dependents but are not themselves derived entries. The general form (every piece of derived geometry, ports
and view clips included, as an entry of one ranked table released with its block) is step 2's business.

## B1. The directory as a piece table (model, before the real core)

The page model proved the stable-slot page; it did not say what happens at a page boundary. If a record keeps
`(page, slot)` for life, a full page cannot split by moving half its records, so the directory splits
instead: an entry is a **piece**, a run of one page's slots in paint order (`crates/bough-pages/src/pieces.rs`,
`Pieces`), identity pieces as `(page, start, len)`, permuted ones with a 64-byte ord block of their own (no two
pieces share one, so no edit of a piece fixes another). `Shift` is today's directory in the same harness
(position-ordered pages, column shifts, handle re-pointing, split in half). A differential test puts both
through 6,000 random edits against a plain vector and checks that every live record of the piece table is
still at the identity it was given. Each page carries a **seal** (bumped by any change to what it shows) for
the compute lane's staged commands; a second test checks it moves exactly when its page does. The compute
lane's two requests are in the design: paint order is the directory and the piece, never a position; the seal
is per page.

**Predictions, written before the first run** (instructions per operation; ilo-prod and the Mac):

1. An insert of 8 + a remove of 8 at random positions, 100,000 records: the piece table at most a third of the
   shifting directory.
2. Front inserts (8 at position 0, repeated): the same ratio or better (the shifting page re-points up to 63
   handles per insert; a piece prepends 8 ord bytes).
3. After churn that replaces every record four times over (N/2 pairs of remove 8 / insert 8 at random
   positions): pieces per page in use at most 2.0; average piece length at least 16 records; bytes per record
   at most 1.5x the shifting directory's (which carries a handle table); at least half of the pieces permuted.
4. Ordered walk of every record after that churn: at most 1.5x the shifting directory's time per record.
5. Building by appends: equal within 10 %.

Kill: after the churn, an average piece shorter than 8 records or more than 2x the bytes per record means the
piece table needs compaction (relocation with notification) designed in before the real core takes it.

### Results (ilo-prod, Zen 2, x86-64 SSE2 build, pinned to CPU 2, load average 8 to 10; minimum of five)

Raw: `data/r7/dir-pieces-vs-shift-ilo-prod.txt`, `data/r7/dir-rows-pieces-vs-shift-ilo-prod.txt`. Instructions
per operation, piece table / shifting directory; time and L2 misses where they differ in kind.

Adversarial matrix (pages built full, then inserts and removes of 8 at random record positions):

| cell | 1,000 | 100,000 | 1,000,000 |
|---|---|---|---|
| insert 8 + remove 8 at the front | 946 / 1,156 | 1,080 / 1,258 | 1,080 / 1,258 |
| insert 8 + remove 8 at random positions, fresh | 2,376 / 2,010 | 2,863 / 2,956 | 5,138 / 5,637 |
| the same after the churn | 2,292 / 1,968 | 3,764 / 3,038 | 20,691 / 8,607 |
| ordered walk per record, fresh | 13.6 / 9.4 | 12.9 / 9.2 (2.3 / 0.8 ns) | 11.3 / 9.1 |
| ordered walk per record, after the churn | 13.4 / 9.5 | 13.2 / 9.3 (2.5 / 0.9 ns) | 13.2 / 9.3 (10.3 / 3.7 ns) |
| build by appends, per record | 86 / 121 | 204 / 235 | 358 / 388 |
| after the churn: records per entry | 9.5 / 20.4 | 10.5 / 23.8 | 10.5 / 23.7 |
| after the churn: page fill | 0.24 / 0.32 | 0.26 / 0.37 | 0.26 / 0.37 |
| after the churn: permuted pieces | 75 % | 80 % | 79 % |

Row-shaped workloads (rows of 8 records, pages built with slack at 48 of 64):

| cell | 1,000 | 100,000 | 1,000,000 |
|---|---|---|---|
| replace a row in place | 1,164 / 1,782 | 1,868 / 2,328 (10 / 21 L2 misses) | 5,147 / 5,615 |
| scroll (a row leaves the front, one arrives at the end) | 940 / 1,544 | 2,006 / 2,536 | 5,187 / 5,679 |
| insert a row, remove another | 1,533 / 1,699 | 2,418 / 2,697 (13 / 46 L2 misses) | 5,382 / 5,844 |
| the same after n/2 such pairs | 1,534 / 1,699 | 2,540 / 2,622 | 9,979 / 6,550 |
| ordered walk per record after them | 12.1 / 9.3 | 12.2 / 9.3 (1.33 / 0.84 ns) | 12.2 / 9.2 (5.8 / 2.5 ns) |
| records per entry / fill after them | 20.8 / 29.4; 0.41 / 0.46 | 17.8 / 28.5; 0.37 / 0.45 | 17.8 / 28.6; 0.37 / 0.45 |

(Both sides grow with n because the model's block search is a linear scan of block counts; that is the
harness, equal on both sides, and the reason the 1,000,000 column should be read as "same order". The
shifting side's bytes per record are not comparable: its handle table in this model never reuses a handle.)

**Verdict against the predictions.**

1. "At most a third on random inserts": **refuted.** Parity on fresh trees (0.97x at 100,000), 1.24x *worse*
   after churn at 100,000 and 2.4x worse at 1,000,000 (twice the entries to search and shift).
2. "Front inserts the same ratio or better": **refuted.** 0.86x.
3. Shape after churn: records per entry 10.5 (predicted at least 16: missed; not under the kill line of 8),
   fill 0.26 against 0.37, 80 % permuted (predicted).
4. "Ordered walk at most 1.5x": 1.42x in instructions, **2.8x in time at 1,000,000** (1.6x on the row
   workload at 100,000): missed where it matters, in the cache.
5. Appends: 0.87x to 0.92x: holds.

Why the page model's 4x to 5x did not survive: an edit's cost is the directory, not the page. A point inside
a full page costs the shifting directory one split and the piece table one split *and* a new page (two
directory inserts, a page that then sits at 8 of 64 until an insert lands beside it). A piece table only wins
where the slots it frees are the slots the next insert needs: row replaced in place (0.80x), scroll (0.79x),
row inserted and another removed (0.90x), with a third to a half of the L2 misses. Those are real GUI shapes
and the gain is real, but it is 10 to 25 %, not a factor.

**What this means for the plan.** Stable slots do not pay for themselves in structural edits. If they are
taken, it is for what identity = place deletes (the handle table: 24 B a record and a dependent load on every
write through a handle; the handle column; re-pointing) and for sharing page images, and they need a
compaction story for the ordered walk (the bin pass) after heavy churn, which the model does not have. The
store lane's identity already gives the two properties the rest of this lane's design needs: per-id columns
that never move, and planes addressable by id. So the order of work should not start with the directory.

**Decision (lead, on these numbers):** the piece table is not built; identity is the store lane's ids. The
model and its harness stay (`pages_bench dir`). The one case that reopens it: the handle table showing up in
the mount, fan-out or resize numbers of the standing scenes (its load on writes through a handle, its 24 B a
record, or its re-pointing on structural edits) at more than about a tenth of a phase.

## B2. One table for derived geometry; the page seal; transitions owned by their scope

**Derived geometry, generalised** (after the first fix, the lead asked for the gaps that can show wrong
pixels). Each failed first in `tests/geometry_order.rs`, then passed:

| case | what was wrong |
|---|---|
| a constant `clip` in a virtual-list row | an absolute rect fixed at mount: the row scrolled, the clip did not (and every newly mounted row took a fresh clip id until the clip column was full) |
| a tooltip under an `offset` | its clip rect was absolute; the bubble moved, the clip did not |
| a virtual list or port list under an `offset` or a `clip_by` | the port clip and the rows' spaces were absolute |
| a keyed `List` port under both | held already (its placement re-reads its space and parent clip) |
| a scroll view (raw `scroll_y`, or a laid-out `.scroll` box) under an `offset`, with a clip and a nested view in its content | the view kept an absolute origin: its next scroll put the content back where it was mounted |

What exists now instead of four mechanisms (the wired-clip list in mount order, each scroll view's own clip
list re-cut on scroll, the layout clips' parent-to-children map with its recursive re-cut, tooltip rects moved
by hand with their view):

* **Every clip that can change is one kind of entry** (`template::WClip`: local edges in a space, the space,
  the parent clip, hidden or not) in a slab, **scheduled by level** (parent clip's level + 1), released with
  the scope that mounted it; released clip ids are reused. Writers store inputs only (a wire: the local rect; a
  layout placement: the box; a tooltip: shown or hidden) and raise a level; `Rt::clips_settle` runs levels in
  order. Only a constant clip in the root space under a clip that cannot change stays a shared absolute rect.
* **Spaces nest, so position is carried, not re-derived**: a scroll view's content space, an anchor's, a
  transformed subtree's and a virtual list's rows' spaces are nested in the space they were mounted in; a view
  keeps its port origin local to that space. Moving a space (an offset, an outer scroll, a placement) carries
  everything under it in one walk; nothing keeps an absolute origin that could go stale. `shift_nested`,
  `view_clips`, `recut`, `clip_kids`, `ScrollView::{clips, nested, tips}` and `Tip::rect` are deleted.

**The page seal** (for the compute lane's staged commands): `Bough::page_seal_at(pos)` and `page_seals()`
give `(page id, generation)`; the generation moves on any write to the page's columns, on a paint write to one
of its records once projected, and on text content (`seal_touch`, called by the three text write paths). What
it does not cover is stated in its doc comment: clip rects and space origins, which a consumer keys on
separately. Paint order is the directory's, never part of a seal. Test: `bough-core/tests/dirty.rs`
(passes on the Mac's ilo-prod twin: x86-64, `cargo test -p bough-core --lib --tests`).

**Transitions** (the bench lane's second defect): `Transitions.items` was a grow-only vector keyed by element
id; unmounting a row killed its wires and not its transitions, so a running one kept restyling a dead layout
node (the panic in `App::resize`) and would have animated whatever element was next given that id. Now a
transition belongs to the `Scope` that mounted its element (`Scope::trans`), is released with it, and its slot
is reused; the frame clock takes a transition out of its slot while its applier runs, so an applier that
unmounts elements can release the very transition being stepped. This is also the shape a kernel-evaluated
transition needs: the record owns it. Tests: `tests/transition_owner.rs`.
