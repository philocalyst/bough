# The arena, publishing and the image, under expr's column table (threads lane, round 8)

Status: **built on `r8/threads`**, on top of `r8/expr` `054b83c929`. `crates/bough-x/src/arena.rs`; changes in
`store.rs`, `sink.rs`, `world.rs`. This replaces the first version of this document (the arena with its own
plan as the turn's entry): the lead decided that expr's `ColEnt` table is the entry, and the independent
review (`recent-20261112`, P01 to P05) changed what the API may offer. What stands, what was closed, and what
is still open are each said below.

## 1. What is where

```text
base    shared   head | page bitmap | list bitmap | ranges | column descriptions | list descriptions   256 KiB
        W        the working half: what the turn thread reads and writes
        F        the published half: what a frame and readers on other threads read; written only by publish

half    plan     64 bytes per column in paint order: the offset form of ColEnt (arena::Entry)
        spaces   clip, base, scroll word per fixed store (a list uses its parent's slot)
        live     live words of every store
        columns  pages, packed at 64-byte alignment
```

* `store::ColEnt` (expr's, unchanged) is what a turn uses: absolute pointers into W, rebuilt when the tree
  changes shape. The half's plan is the same table with offsets in place of pointers, written by the same
  rebuild (`World::retable` -> `replan`); only a frame, a reader and an image use it.
* Column pages, live words and space slots are allocated in W: `Store::new` takes the arena; `discover` no
  longer allocates, `Store::seal` does, once per row described (so a list's columns are sized exactly and
  never move; a fixed store's column can move once during its first description, before anything was
  published). `Store.row_live` is a `LiveWords` (derefs to `[u64]`), so expr's code reads and writes it as
  before.
* Column bases are **not** computable from a table index (the lead agreed). The measurement: with everything
  evicted, five dependent loads against two changed a cold hover from 12,245 to 12,342 cycles and 470 to 473
  lines from memory (mothership, `hot-mothership-tree-*` against `-plan-*`): a cold turn pays for its data
  lines and its code, not for the chain. Computable bases would cost a VM page and a TLB entry per column per
  half.

## 2. Publishing

`World::publish(&mut self)` copies what the turn dirtied from W to F and records what a frame may paint (the
plan length and the block of that moment). `World::paint_front(&self, u, b)` paints F; before the first
publish it paints nothing and returns `false`.

What a writer notes, and what it costs (against `054b83c929` built apart, Mac, instructions, expr's `sheet_x`):

| what was written | the note | cost |
|---|---|---|
| a list wrote rows whole, or moved its window | one mask per list per pass (`Store::rows_dirty`): bit `p` = row slots `64p..`; publish expands it to the page of every site of every column, and the live words | scroll three rows +1.9 %, a page +0.3 % |
| one record replaced (`sink::replace`) | its page's bit (`Site.rid + slot / 64`) | edit an exposed cell +2.5 % |
| a fixed row written fresh, a space, the plan | page bits (cold) | hover, theme, idle: +0.0 % |

A turn that writes no column publishes nothing: the frame gets the new block and the same F.

## 3. The review's findings

| finding | state | how |
|---|---|---|
| **P01** handles escape ownership; aliased `&mut Head`; `Send + Sync` handles; unchecked ids | **closed for the safe API** | `Ar`, `Half`, `Head`, `Entry`, `Arena` are crate-private; a `World` has no public handle. `Ar` is neither `Send` nor `Sync`. `Ar::head` returns `&Head` and every field the turn changes is a `Cell`, so no `&mut` can alias. Bitmap and description indices are masked into their tables. The mapping is an `Arc` held by the arena and by every `Reader`, so nothing outlives it (`a_reader_outlives_its_world`). What is **not** closed: inside the crate `Ar` is still a raw base with accessors whose invariants are comments ("stored only in the world that owns the arena or its stores"); a lifetime-carrying handle would make those types, and is the first thing to do before any worker exists. |
| **P02** an epoch retry does not make plain concurrent reads and writes sound | **closed** | `publish` stores every word of F through `AtomicU64`; `Reader` loads every word through `AtomicU64` / `AtomicU32` and nothing else, bounds every offset it derives from loaded words, and hands the closure a `Snapshot` it cannot keep (`for<'a> FnMut(&Snapshot<'a>) -> R`). The epoch check now only discards mixed values; it is no longer what makes the access defined. The closure may run more than once and must only compute an answer (documented; not enforced). A frame's reads of F are plain, and are excluded from `publish` by `&self` against `&mut self`. |
| **P03** `paint_front` could run an unpublished plan | **closed** | the frame's plan length comes from `World.front`, set by `publish` (and by `adopt` after it has matched an image's plan); `nothing_is_painted_before_the_first_publish`. |
| **P04** image identity weaker than the code offsets it authorises; not bound to the backend | **closed for the safe API** | the head carries a hash of the backend type (`another_backend_is_refused`); `Arena::open` follows no code offset and no descriptor; the safe path paints an image only after `World::adopt` has compared every plan entry with the one this binary builds for the view. Painting before that is `unsafe fn paint_image` with the contract stated. What stands: the file-size-and-mtime identity is only a filter, and a build id would be better; the checksum guards against accident, not forgery. |
| **P05** a fixed published half serialises publication with frame completion | **stands** | true: `publish` needs the frame done. Closing it is immutable frame versions (THREADS-DESIGNS.md section 4); not built. The sentence "nothing in a turn is worth a thread" is withdrawn as a general claim: it was measured on a sheet whose layout is arithmetic and whose text is formatted while shading. |
| **O02** `Region::carve` misaligns over-aligned types | **closed** | `bough-mem`: an alignment above a VM page is applied to the address, not the offset; the review's probe is `crates/bough-mem/tests/align.rs`. |

## 4. Gates

`tests/publish.rs` (4), `tests/image.rs` (5), one assertion in `tests/sheet.rs` (published == working after
every step of every script), expr's and surface's own (anim 2, events 9, reflect 6, table 1, sheet 4).
Mutants: `mutant_publish` (forgets every other column page) fails the sheet gate by pixels; `mutant_torn` (the
reader does not re-check the epoch) fails the reader gate with tens of thousands of mixed scenes.

## 5. For the other lanes

* **expr**: the changes to your files are the allocation (`Store::new(ar, ..)`, `new_list`, `seal`, `LiveWords`,
  `sync_space` after a space is assigned), the notes (three lines in `sink.rs`), `HitFn` taking `&ColEnt`, and
  the pointer path in `world.rs` running over the table (`hits`, `kinds_of`, scratch vectors) instead of
  walking the store tree and allocating: a cold hover went from 12,160 to 9,890 cycles and a warm one from
  10,888 to 10,094 instructions.
* **raster**: a frame is `(block, published half, plan length)`; nobody writes it while it is painted. Kernels
  must index nothing with record words unchecked: an image's column pages are not verified.
* **surface**: accessibility and inspector reads from another thread are `World::reader()` -> `Reader::read`;
  `Snapshot::{columns, column, live, record_word, rect}`.
