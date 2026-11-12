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
