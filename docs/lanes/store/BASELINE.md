# Lane `store`: baseline (commit b083564, branch `lead/core-r6`, before any storage change)

All cells are instructions retired on the calling thread (`bough_metal::perf::tinstr`), from
`crates/bough/examples/redescribe_cost.rs`, M3 Pro, release profile, machine under load (load average ~85:
instruction counts do not care). The example gained a counting `#[global_allocator]` for `HEAP=1` (a load and
a store per allocator call); the instruction cells were taken before and after it and agree within run noise.

## Mount (`MOUNT=1 redescribe_cost 100 1000 6000`: `App::new` + first turn, minus the same for 0 rows)

| rows | application row (box + integer label + string label) | static row | bare box |
|---:|---:|---:|---:|
| 100 | 59,798 | 57,578 | 4,055 |
| 1,000 | 28,044 | 28,001 | 5,116 |
| 6,000 | 26,715 | 26,717 | 4,720 |
| slope 100 -> 1,000 | 24,516 | 24,715 | 5,234 |
| slope 1,000 -> 6,000 | 26,450 | 26,460 | 4,641 |

An empty App (new + first turn, headless): 13.0 to 13.3 M instructions.

Phases of the 6,000-row application mount (`PHASES=1`; the engine's share with `BOUGH_LPROF=1`, the split of
the rest from the time profile below): `App::new` ~23,300 per row (elements ~10,500, cephal's cold layout
~9,000, write-back ~3,500), first turn (the first re-description, which writes every seal) ~3,150.

## Re-description (`redescribe_cost 10 100 1000`, per row per turn)

| rows | application | static | bare | once (control, whole turn) |
|---:|---:|---:|---:|---:|
| 10 | 1,274 | 1,297 | 555 | 173 |
| 100 | 679 | 701 | 259 | 173 |
| 1,000 | 645 | 628 | 230 | 173 |

## Churn (`CHURN=1 redescribe_cost 100 1000`: remove row 2 + insert it back, per row)

| rows | per pair | per row |
|---:|---:|---:|
| 100 | 1,637,072 | 16,370 |
| 1,000 | 16,446,590 | 16,446 |

## Heap (`HEAP=1 redescribe_cost 1000 6000`: live bytes of the Rust heap, application rows)

| rows | B/row after `App::new` | B/row after the first turn | allocator calls per row |
|---:|---:|---:|---:|
| 1,000 | 3,733 | 3,922 | 15.3 |
| 6,000 | 4,681 | 4,807 | 15.1 |

(Lane `memory` measured ~2.5 KiB per row by length; these are live allocator bytes, so they include every
`Vec`'s doubling slack: the 6,000-row figure is the worse because more tables sit just past a doubling.)

Per table at 6,000 rows (`Rt::store_ledger`: `size_of` x capacity plus the heap capacity reachable from each
entry), bytes per row (a row is three elements):

| table | entries | B/row |
|---|---:|---:|
| core: directory, prefix, `htab`, planes | 18,001 | 201 |
| `rt.nodes` (`Node`, 120 B) | 18,002 | 655 |
| `rt.runs` | 6,002 | 16 |
| `rt.texts` (`TextNode` + `String` + line vectors) | 12,000 | 594 |
| `lay.info` (`LInfo`, 60 B) | 18,002 | 284 |
| `lay.eng.n` (`DNode`, 152 B) | 18,002 | 830 |
| `DNode.kids` (a `Vec` per container) | 6,001 | 22 |
| cephal answer caches (a 360 B box per container that stored) | 6,001 | 360 |
| `lay.eng.ctxs` (`LeafCtx` + its vectors + the paragraph) | 12,000 | 1,070 |
| engine scratch | | 22 |
| `dyns.parts` (`DynItem`, its tape, its `Scope`) | 6,000 | 343 |
| `dyns.seals` | | 126 |
| `reflect.types` | 18,002 | 87 |
| sum | | 4,611 |

## Time profile of the 6,000-row mount (`MOUNT=1 SPIN=200 redescribe_cost 6000`, `sample <pid> 4`)

Self time, teardown excluded (2,609 samples; raw file `runtime/lanes/store/base/mount6000.sample.txt`):

| share | where |
|---:|---|
| 21.1 % | cephal: `flex::fused::compute<Row>` 10.3, `Questions::ask_cheap_leaf` 7.5, `compute_flexbox_layout` 3.3 |
| 9.4 % | `memmove` (growing `Vec`s: `LeafCtx`, `DNode`, `TextNode`, `Node`, `DynItem`; struct moves) |
| 10.1 % | kernel: `mach_msg2_trap` (`vm_copy` under `realloc`) 5.3, `madvise` 4.8 |
| 5.5 % | `lay_writeback_ready` (with `place_text` inlined) |
| 5.4 % | `LBox::mount` (self) |
| 5.1 % + 1.7 % + 3.4 % | `SpecTable::intern`, `hash_one::<SpecKey>`, `memcmp` (spec identity by hashing 84 bytes) |
| 7.6 % | malloc / free / `bzero` |
| 2.8 % + 2.3 % | `Deep::add`, `Vec<DNode>::resize_with` |
| 2.1 % | `TextSys::select` |
| 1.2 % | `Para::shape_range` (the shaping itself) |

By subtree: element mount 47 % of `App::new` (two fifths of it inside `realloc`), cephal's cold pass 35 %,
write-back 15 %.

## The signal path (`crates/bough/examples/signal_cost.rs`, the lead's harness; added to the gate cells)

One signal written per turn among N rows, instructions per turn (slope over repeat counts), measured after
stage 0b (which does not touch the wire path: storage addresses only):

| rows | mode | colour | number | string | idle turn |
|---:|---|---:|---:|---:|---:|
| 16 | once | 1,091 | 3,172 | 3,928 | 848 |
| 64 | once | 3,076 | 886 (*) | (*) | 2,864 |
| 256 | once | 11,128 | 19,239 | 30,935 | 10,928 |
| 16 | redescribe | 31,147 | 33,751 | 34,892 | 31,583 |
| 64 | redescribe | 122,426 | 122,024 | 120,245 | 124,639 |
| 256 | redescribe | 493,085 | 503,135 | 517,802 | 500,224 |

(*) the slope of these cells is not stable: a digit-count change re-lays the row out in some turns only.

What the table says: an idle turn costs ~43 instructions per row (14 per wired cell: `settle` scans every
wired cell of the owner for its QUEUED bit), so a colour write is ~240 instructions on top of an O(cells)
scan; a re-describing owner pays ~1,930 per row per turn because a row bound to signals does not seal.
Targets (lead): a colour write under 100 instructions end to end, a numeric label write under 400.
