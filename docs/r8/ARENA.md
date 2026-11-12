# The arena, the plan, publishing and the image (threads lane, round 8)

Status: **built on `r8/threads`** (`crates/bough-x/src/arena.rs`, and `store.rs` / `sink.rs` / `world.rs` ported
onto it; merged with `r8/expr` `8af3f228d0`). Offered to the expr, raster and e2e lanes through the lead. It
answers three requests at once: the flat world-wide column table (expr, SEAM 9), who may write a page a frame
is reading (raster, SEAM 6.1 "Memory"), and the mapped scene image (e2e, ROUND8 amendment 5).

## 1. Layout

One reserved range of address space per world; memory is committed by first touch.

```text
base    shared   head | dirty bitmap | ranges                     256 KiB
        W        the working half: read and written by the turn thread only
        F        the published half: read by a frame and by every other reader; written only by `publish`

half    plan     64 bytes per column, indexed by column number     MAX_COLS = 1024
        order    column numbers in paint order (u16)
        spaces   clip, base, scroll word per space (32 bytes)      MAX_SPACES = 512
        live     live words of every store                         MAX_LIVE = 2048 words
        columns  pages, packed at 64-byte alignment
```

The halves have one layout: an offset means the same in both, and which copy a pass reads is one base pointer
(`Half`). Nothing in a half is an address: pages are lines (64 bytes) from the half's base; kernels and the
descriptor are 32-bit offsets from an anchor in the binary.

**A plan entry** (`arena::Entry`, one cache line): `reads`, `skip` (the words that move its space), the delta
and paint kernels and the descriptor (offsets), `pages`, `nrec`, `live`, `cap`, `space`, `flags` (TIMED), `rid`
(first dirty-range id), and the page shape (`page_lines`, `rec_off`, `rec_size`) so a reader can walk records
without the archetype's code.

**A column's number** is its index in the plan, given when the column is created and never changed. It is
what `Tiles::sweeps` names a column by, and what a pick id's `col` field should be (it does not change when a
later column is created, which a paint-order number would). Paint order is the `order` array.

## 2. Passes

`store::delta`, `store::paint`, `store::stats` are loops over the plan (paint through `order`). The store tree
(`Store`, `RawCol`, `Site`) is what the sink uses to find its sites; no pass over records reads it.

Dependent loads from the block to a record page: the plan entry (its address is the half's base plus a
constant), then the page (`entry.pages`). Two, at any nesting; the space and the live words are reached from
the same entry, in parallel with the page.

### Where this departs from what the lead asked ("a column's base computable from its table index, VM-page
aligned"), and why

Not built that way; this is the point to agree.

* *Computable base* needs a fixed span of address space per column, so every column starts on its own VM page
  in each half. A scene of many small columns (every fixed store has several, of one page of 1.3 KiB each)
  then costs one VM page and one TLB entry per column per half: at 16 KiB pages, 1,000 columns are 16 MiB a
  half against 1.3 MiB packed, and a full pass walks 1,000 TLB entries against about 80.
* What it would buy is one miss overlapped instead of chained on a cold pass: the page's address would not
  wait for the entry. But a pass must load the entry before it knows whether to touch the page at all
  (`reads & changed`), and after the first column the entries stream ahead of the pages anyway.
* Measured, on the quiet host with everything evicted (`hot-mothership-*.tsv`): going from **five** dependent
  loads (the store tree) to **two** (the plan) changed a cold hover from 12,245 to 12,342 cycles, 470 to 473
  lines from DRAM. No difference. A cold turn pays for about 210 data lines and its own code, not for the
  chain; removing one more link cannot show.

So: packed spans, base in the entry. If a GPU backend needs each column as its own buffer, that is the reason
to revisit; with one buffer per half it is not needed. *Never moved*: a list's columns are sized exactly when
its row type is first described and never move. A fixed store's column can move once, when a later row of the
same store's first description joins it; that is before the store's first publish, so no frame, reader or
image has seen the old place. *Committed on demand*: by page fault; the mapping is `MAP_NORESERVE`.

## 3. Who may write a page a frame is reading: nobody

`publish()` copies the ranges the turn dirtied from W to F. A frame is a pure function of (the block it was
given, F). Rules:

1. The turn thread writes W whenever it likes; it never waits for a frame or a reader.
2. `publish` runs between frames: when no frame is reading F. One frame in flight. If the previous frame is
   not done, the turn thread does not wait: it keeps taking turns into W (the block is its own copy, relation
   edits are logs) and publishes when the frame slot is free. Latest state wins; nothing queues.
3. A turn that writes no column (hover, theme, time, the pointer: every Frame-stage write) dirties nothing and
   publishes nothing; the frame gets the new block and the same F.

This is the raster lane's copy-on-write per dirty page, with the copy made into a fixed place: no indirection
on any pass, no remap call, and each half stays one contiguous buffer. On unified memory F is the GPU's
buffer; on a discrete GPU F need not exist on the CPU at all: the dirty ranges *are* the upload list, and
`publish` is the upload.

**What a writer must do**: note what it wrote. `Site.rid + (slot >> 6)` is the dirty-range id of the page a
record is in; `Ar::dirty(id)`. The sinks do it: per site per list pass for rows written whole, per record in
`replace`, per site at a fresh fixed row. Live words, spaces, the plan and the order are ranges too.

**Readers that are not the frame** (accessibility, the inspector, a worker reading operands): `Ar::read(|half,
epoch| ..)` runs the closure on F and keeps its answer only if no publish ran meanwhile (the epoch is odd
during a publish). They write nothing shared. The closure must tolerate torn bytes: copy plain words, bound
every index (`Ar::in_half`), follow no code offset.

Gates (`tests/publish.rs`, and one assertion added to `tests/sheet.rs`): the published half equals the working
half after every step of the sheet script; turns after a publish do not change what a frame in flight reads; a
reader thread over 2,000,000 publishes saw no scene of two publishes (Mac and ilo-prod). Mutants:
`mutant_publish` (forgets every other page) and `mutant_torn` (no epoch check) fail them.

Cost on the turn (mothership, x86-64, instructions): hover and theme unchanged or lower (10,381 -> 10,238;
36,168 -> 36,094); scroll three rows 6,561 -> 7,173 (+9 %: the dirty notes of 22 sites and the live words);
scroll a page +1.7 %; edit an exposed cell -2.5 %. The publish itself is one copy of each dirty page per frame.

## 4. Open, for the other lanes

* **expr**: `discover` no longer allocates; `seal()` after a row's positions are known does (called from
  `row_new`). Kernels are `unsafe fn(Half, &Entry, ..)` and get their column through `store::col_view`.
  `Store::space` is private with `space()` / `set_space()`; live words are `live_mut()` / `live_dirty()`.
  The order lane (SEAM 7.5) and `Valid` lanes will be more ranges of the same kind. Limits are asserts today
  (1,024 columns, 512 spaces, 131,072 row slots, 16,384 column pages, 64 MiB a half).
* **raster**: a frame takes `(U, Half, ncols)`; `Tiles::sweeps` names columns by number. If the GPU wants the
  dirty set as upload ranges, `publish` can return them instead of a count (it iterates exactly that list).
  Kernels index nothing with record words unchecked: that is what lets an image's column pages go unverified.
* **surface**: accessibility and inspector walks are `Ar::read` over the plan; an entry carries the page
  shape, so a typed query needs the archetype only to interpret the words.
