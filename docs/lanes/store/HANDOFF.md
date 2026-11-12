# Lane `store`: handoff (2026-11-12, wrap-up)

Companion files: `cards/2026-11-11-store.md` (the card), `BASELINE.md` (every baseline cell and the first
profile), `DESIGN.md` (what the design became and why). All numbers are instructions retired on the calling
thread (`bough_metal::perf::tinstr`), M3 Pro, release, machine under heavy load (load average 85 to 130:
instruction counts do not care; the one wall-time number says so).

## State

* Branch `lane/store/round6`, head `281f6c3c6d` plus this handoff's commit. Working tree clean.
* Contains `lead/core-r6` up to `4f4c469ef5` (merged twice; the last merge is `281f6c3c6d`).
* **Not merged into `round6`** by me: the lead takes the branch. Everything below is on the lane branch only.
* Commits, in order (m = mechanical / rename-like, s = structural):
  `e7f8fc5` card · `eb156f9` baseline, harness modes, ledger, stage 0a sibling chain (s; later replaced) ·
  `5e0459a` stage 0b carved columns (m) · `1f8c5ff` stage 1 one index space (s) · `ff1f090` bough-text
  `move_label` (s, additive) · `3ae78a4` label commit "a laid-out one-line text is a retained label" (s) ·
  `6f237e8`, `b68c141` Opus A1a, A1c/A1d cherry-picks · `21d108e` cursor fix + integer-seal mutant ·
  `a27229f` children as id arrays (s; undoes 0a's chain) · `bf845c9` stage 2a per-id columns (s) ·
  `281f6c3` merge + `Region::heap` as a mapping + two tests.

Stages of STORE-R6 reached: **0** (storage that never moves: done for every façade table; per-node `Vec`s
gone for `DNode.kids`, `Node.series`; NOT done for `DynItem.tape`, `Scope`'s vectors, `TShape::Lines`'
vectors, `LeafCtx`'s), **1** (one index space: done), **2** (hot/cold columns: done for `Node`; NOT done for
`LInfo`, `DNode`, `TextNode`), **3** (side pages on the core's column plan, fixed point: not started; lane
core has not landed the trait), **4** (parallel mount: not started).

## Numbers

Commands: `E=target/release/examples/redescribe_cost` (`cargo build --release -p bough --example
redescribe_cost --example signal_cost`). Each cell was run once to three times; instruction cells repeat
within ~1 % except where noted.

| cell | command | baseline (b083564) | now (281f6c3) | verified |
|---|---|---:|---:|---|
| an empty App (new + first turn) | `MOUNT=1 $E 100` (stderr line) | 13.0 to 13.3 M | **1.2 to 1.4 M** | 4 runs each; guarded by `tests/startup.rs` |
| mount per application row, 1,000 rows | `MOUNT=1 $E 1000` | 28,044 | 26,173 | 3 runs; +-1.5 K with allocator state |
| mount per application row, 6,000 rows | `MOUNT=1 $E 6000` | 26,715 | 25,032 before the last merge, **42,171 after it** | regression is lane core's far-ink path, see below |
| mount per bare box, 6,000 rows | same | 4,720 | 3,610 before the merge, 9,969 after | same |
| re-description per row (owned / static / bare), 1,000 rows | `$E 1000` | 645 / 628 / 230 | **298 / 305 / 122** | 3 runs; 2/3 of the gain is the Opus experiments A1a, A1c/A1d |
| churn (remove + insert) per row, 1,000 rows | `CHURN=1 $E 1000` | 16,446 | **12,075** | 3 runs |
| bytes per row (allocator live + touched column bytes), 6,000 rows | `HEAP=1 $E 6000` | 4,807 | **3,296** | 2 runs; per-table ledger printed |
| allocator calls per row | same | 15.1 | 14.1 (not re-read after stage 2a) | 1 run |
| element state per row (`Node` x 3) | ledger | 655 B | **36 B** (hot + generation, 12 B per id) | ledger |
| colour applier alone | `APPLIERS=1 $E` | (not measured at base) 162 at stage 1 | 159 | 3 runs |
| digit tick on a laid-out label, applier alone | same | (path did not exist) | 257 | 3 runs |
| signal: idle turn, once, 1 / 16 / 256 rows | `signal_cost 1 16 256` | 218 / 848 / 10,928 | 249 / 332 / 249 | the lead's write log; mine only merged it |
| signal: colour write, once, 1 / 256 rows | same | 455 / 11,128 | 640 / 597 | 1 run after merge |
| signal: number write, once, 1 / 16 / 256 rows | same | 3,269 / 3,172 / 19,239 | **850 / 785 / 1,218** | 2 runs; see "which change" below |
| signal: string write, once, 256 rows | same | 30,935 | 15,559 | slope unstable (width changes re-lay the column) |
| signal: redescribe mode, 16 rows, colour | same | 31,147 | 6,319 | the lead's wired seals + A1a |
| child iteration, 4,096 children, per child | removed probe (see Experiments) | n/a | chain 3.4 ns / 6.6 ns after churn; array 1.2 / 1.0 ns | wall time, best of 2,000, under load |

**Which change removed the row-count scaling of the number write (1,218 here against 8,756 on
`lead/core-r6` at 256 rows):** the label commit ("a laid-out one-line text is a retained label"), not the
child arrays and not the columns. Before it, every text change of a `text()`/`label()` in flow re-shaped the
leaf's paragraph, set `tdirty`, and, for a sized label, either restyled or invalidated the leaf, so the
write-back re-pointed its line windows and the containing column was re-laid out whenever the size compare
said so. Now the label patches itself in the glyph pool and layout is touched only if its *width* changes
(`Rt::label_set` returns "width changed"; `lay_text_changed` returns early otherwise). Measured at stage 1,
before the label commit: 19,235 at 256 rows against an idle 10,928 (8,307 for the write); after it: 11,729
against 10,928 (801). The residual growth with rows (850 -> 1,218) is the turns where the digit count
changes: the label is re-sized, cephal marks up through the fixed-size row and re-runs the root column over
N cached children (~58 instructions per child). That part is still there and is the layout lane's.

Regression to know about: **mount per row is not flat any more past ~1,000 rows of 32 px** (26.2 K at 1,000,
39.0 K at 3,000, 42.2 K at 6,000) since merging `4f4c469ef5`. Profile at 6,000 rows: `Bough::slot_wide`
10.7 %, `Bough::reencode` 8.6 % (lane core's exact-coordinate inks, content beyond 32,767 px). Reported to
the lead; not fixed here.

Frame-path cells (`bench/integrate/slope.py`, `bench/layout/slope.py`): **not run at any stage.**

## Assembly of the colour applier (the chain the card is about)

Before (stage 0, `patch_sig::<ApplyBg>`): `nodes[node]` (96 B stride: colour at +0x30, kind at +0x5c, run at
+0x10) -> `runs[run].first` -> `inst[first]`, each load waiting for the previous one, three bounds checks,
then `nodes[node].handle` -> `htab` -> page.
After (stage 2a, `runtime/lanes/store/asm/colour-stage2.s`, first 22 instructions after the prologue):

```
ldr  w19, [x1, #0x24]          ; w.node
ldr  x8,  [x0, #0x2d38]        ; hot.ptr
add  x9,  x8, x19, lsl #3      ; &hot[node]            (8 B stride)
ldrb w8,  [x9, #0x1]           ; flags
tbnz w8,  #0x1, <cold path>    ; HF_MOD
ldr  w8,  [x0, #0x30c0]        ; ibase
add  w8,  w8, w19              ; instance index = ibase + node   (no load depends on hot[node])
ldrb w9,  [x9]                 ; kind
umaddl x8, w8, w11, x10        ; &inst[ibase + node]   (48 B stride)
cmp  w9,  #0x3 ; csel x9, 0x14, 0x10   ; color2 or color, a select, not a branch
ldr  w23, [x8, x9] ; cmp w23, w20 ; b.eq <done> ; str w20, [x8, x9]
```

`hot[node]` and `inst[ibase + node]` are both addressed from `node` alone, so they load side by side; the
store is reached with zero dependent loads (it was two). Three bounds checks of mine survive (hot, inst,
hgen); the remaining ~110 of the applier's 159 instructions are bough-core's `visible_ink_handle` (scalar
`NRect` translate / intersect twice, with repacks), the paint ring mark and the paint log.

## Unsafe invariants added, and their tests

* `Rt::new`: `hot`, `hgen`, `aux` columns `set_len` to capacity over mapped zero memory (all-zero is a valid
  `Hot` / `u32` / `f32`): covered by every test that mounts; `bough-mem`
  `heap_region_is_zero_aligned_and_lazy` tests the mapping itself (zero, aligned, writable at both ends, 4 GiB
  without being cleared).
* `bough_mem::Region::heap` (`mmap` / `munmap` in `Drop`): the same test.
* `store::Spans::floats{,_mut}` (a `u32` span viewed as `f32`): `store.rs` tests
  `floats_view_the_same_words`, `spans_reuse_by_class_and_never_overlap` (a scalar model of live spans).
* `GlyphPool::move_label`: `bough-text/tests/pool.rs` `moved_label_patches_as_one_allocated_there`; the
  mutant that keeps the old pens fails it (run by hand).
* Seal mutants: `BOUGH_SEAL_MUTANT=1` (text) and `=int` (integer operands, new) both fail the gate at their
  step (`tests/seals.rs`).
* No column plan exists yet, so the "mutant that forgets one column in a split" test does not exist either.

## Not verified / known broken

* **The comparison suites were never run** on this branch (`bough-suite`, `-awagy`, `-zgui`, `-vlist`,
  `-heavy`, `-scenes`), nor the frame-path cells. Only `cargo check --workspace --all-targets` and
  `cargo test --release -p bough` (15 binaries, with `BOUGH_SETTLE_AUDIT=1` on the head), `bough-mem`,
  and `bough-text --test pool` (one test). This is the largest gap: stage 1, the label change and stage 2a
  all touch paths the suites exercise (templates, decorations, hover, tooltips, text fields, lists).
* Paths changed without a dedicated test: a label whose new text leaves the label kernel's coverage
  (representation change through `Deep::set_ctx`); hover / opacity / translate on a quad (the `HF_MOD` cold
  record, made lazily from the instance); `Quad::update` and `LBox::update` restyle branches under `HF_MOD`
  (they rewrite the instance as before and now also the cold record's colour); the tooltip keeping an
  element's own handle.
* A cold record is released only when its element id is reissued (`node_at` / `drop_node`): bounded, but a
  hovered-once quad keeps 80 B until then.
* `StoreCaps`: hard capacities derived from `Caps::pages` (ids = 2 x pages x 64). Exceeding them panics with
  a message. The instance column is now `[Caps::instances | ids]`, so a GPU region is larger by 48 B x ids
  of address space (not resident until written; **not checked on a real MTLBuffer**).
* The layout width of a label whose text has trailing spaces and ligatures uses the whole width (the
  paragraph path excluded trailing spaces there too); retained == fresh holds, absolute layout may differ.
* `ltemplate`'s no-wrap text still uses the measured-leaf path (not the label).
* `rustfmt` was not run on the changed files.
* The `Cargo.lock` change is one `libc` edge for `bough-mem`.

* **The sibling-cursor defect the layout lane found** (`tests/seals.rs` failing at "remove middle" on
  `b68c141937`: the fast loop of the Opus A1a experiment did not advance the sibling cursor) existed only on
  the two cherry-pick commits `6f237e8` and `b68c141`. It was fixed in the next commit (`21d108e`), and the
  cursor itself was removed in `a27229f` (children are index-addressed arrays again; the fast loop advances
  `m.kid`). `tests/seals.rs` passes on the head, with both mutants caught. Anyone bisecting should skip those
  two commits. The layout lane's differential test (`crates/bough/tests/lay_diff.rs` on `lane/layout/round6`
  at `76817a1af1`) is **not** on this branch: it was written against the chain form and should be taken
  when the lanes are integrated (its one-line fix is not needed here).

## Experiments (everything tried)

| experiment | result | kept? |
|---|---|---|
| Layout children as a sibling chain through the nodes (stage 0a) | removed a `Vec` per container, but child iteration became a dependent chain: 3.4 ns per child in mount order, 6.6 ns after churn, against 1.2 / 1.0 ns for an array (4,096 children; instructions equal, 9 to 10) | **killed**; replaced by id arrays in a span arena (`a27229f`) |
| Tables in an allocator-zeroed block | an empty App went 13 M -> 428 M instructions (the block is cleared eagerly) | killed; `mmap` |
| The same finding applied to the GPU region (`Region::heap`) | an empty App 13.3 M -> 1.36 M | **kept** |
| Per-strike label setup remembered (`GlyphCache::label_strike`) | label mount lost ~95 readiness tests + 10 box unions per label; mount cell moved within its noise | kept |
| `SpecTable::intern` checks the two most recent specs before hashing | first-turn cost per row 3,150 -> ~2,350 (with the other changes; not isolated) | kept |
| Node mirrors removed (compare with the instance, Opus F9) | colour applier 162 -> 158: the chain shortened but the count barely moved, because ~110 of it is the core's | kept (for the memory: 288 -> 36 B per row) |
| Integer seal with the text mutant | the text mutant no longer caught the gate at its step (zebra stripes turn a shifted row into a paint difference, which is updated in full) | split into two mutants |
| Mount cell order dependence | the 6,000-row owned cell moves +-1.5 K with allocator state (Vec growth of remaining tables) | noted; goes away when the remaining Vecs do |

Not started (the three the amendment asks for, with predictions, are under "Next").

## Underexplored and exotic avenues

Rough, in the order I would rank them. "Buys" numbers are estimates unless marked measured.

1. **Position is identity for the layout tables (`LInfo` + `DNode` -> pass-grouped per-id columns).** Designed,
   not built: `LayA {style, kids, nkids, flags, kclass, bits, kind}` 16 B (the line a cephal visit reads),
   `lrect [f32; 4]`, `overflow [f32; 4]`, `LayB {parent, spec_ix, order, space}` 16 B, cache word 8 B, and the
   border/padding/margin of cephal's 92 B `Layout` interned per style (they are style constants unless
   percentages are used). `LInfo.ax..ah` can go entirely: "moved" is "own location changed (a bit set in
   `set_unrounded_layout`, which sees old and new) or an ancestor moved" and needs no stored absolute rect;
   `touched` becomes a bit; `ctx` moves into the text state. Buys 528 -> ~228 B per row and one line per
   visited child instead of three. Cost: ~130 call sites. Kill: if cephal's incremental scheduler reads the
   interned parts on a hot path (it reads `unrounded_layout` in `schedule/mod.rs:345,380` and `oof.rs`).
2. **The 360 B answer cache per container** (`cephal::NodeCache`, boxed on first store) is now the single
   largest per-row item after text. In place (vendor rule): one inline entry instead of three plus the final
   one, or entries in a per-tree arena with (offset) references. Buys ~250 B per row. Kill: count how often
   entries 2 and 3 are hit over the suites.
3. **Text state**: `TextNode` 232 B + a boxed `Label` (256 B, cache-line aligned) + its text box per label =
   850 B per row of two labels, the largest table. The label's hot line (head, base, widths) is 64 B; pens,
   tile caches and the digit cell are derivable or per strike. Labels in a carved column of 64 B hot records
   plus a cold column; `TextNode` reduced to `{label/shape, colour, strike, handle, ox, oy}` ~32 B; no
   `String` at mount (`TextSrc::with_str` already exists). Buys ~700 B per row and 4 allocator calls per row.
4. **Seal columns** (the lead's target: idle re-description under 30 instructions per row). Fixed-stride
   word records per fixed-shape item type (Opus F2 measured 135 to 159 per row for build + compare), stored
   as a column of the part; then the part-level form: build the new column, one streaming compare, first
   mismatch = first changed item. Under 30 per row needs the *description itself* not to be rebuilt for
   unchanged rows, which no compare can give: it needs either owners-as-blocks (only changed owners
   re-describe) or a per-row input fingerprint (the row's source bytes, e.g. `(id, label ptr, len, selected
   == id)`) compared before the closure runs. Kill for the column form alone: if build + compare does not go
   below 120 per row for the application row.
5. **Direct wires** (lead's experiment): a wire whose effect is one column store plus a damage rect lowered at
   mount to `(column address, record id)`. With stage 2a the store address is already `inst_base + (ibase +
   id) * 48 + 16|20`, computable at mount and stable for the element's life (instances at ids never move);
   what is left per write is the damage (the core's ~110) and the paint log. So the write itself can do the
   store; the damage can be an append of the id to a per-turn paint list resolved once before the frame
   (Opus F10). Predicted: colour write ~25 instructions at the write + ~60 per distinct record at frame time.
   Kill: if a frame-time pass over the paint list costs more than the per-write mark for typical lists (< 8).
6. **The turn's fixed cost** (249 idle now). Read in the disassembly: `admit`, `settle*`, `effects`,
   `redescribe`, `lay_flush`, `sync`, `reclaim` are out-of-line calls that each return at once; `lay_flush`
   scans every list region for `dirty` each turn. Inline guards (`producers.is_empty()`, `pubs.is_empty()`,
   `pending/ready/dirty.is_empty() && regs.is_empty()`), a dirty-region counter instead of the scan, `prof`
   behind a feature. Predicted: idle turn 249 -> ~60. I had the edits planned (wrappers in the callee
   modules so `App::turn` itself is not touched) and did not get to them.
7. **Bit-packing of per-id columns.** `Hot` is 8 B: kind 4 bits, flags 2 bits, clip 16 bits, link 32 bits ->
   it fits 7 B but more usefully `clip` is also in the core's page and `link` is only for text and cold
   elements; a quad needs 1 byte (kind + flags). `hgen` duplicates `htab[i].generation`. With the core's
   8-byte handle word (Opus F1) the per-id state of a painted box is 1 byte + its 48 B instance; in bits: 8.
8. **Side tables still synchronised by hand** (the amendment's rule 6): `Scope` (wires, texts, insts, runs,
   series, lroots, dyns, lregs, prods: seven `Vec`s per dynamic item; derivable from the removed run's records
   for texts and instances), `DynItem.tape` (the tape entry of an element is the handle of the record at the
   walk's position: derivable with a page cursor), `DynItem.nrec/nkid` (constants of the item type, Opus F11),
   `reflect.types` (16 B per id: a `&'static str` per element; an interned u16 type id), `lay.specs`
   (HashMap for boxes with size operands), `lay.eng.abs` (HashMap), `input.acts` by handle, `lists.root_reg`
   / `root_h` by root id, `LInfo.clip` duplicating `Hot.clip` and the page's clip, `LInfo.space` duplicating
   the instance's and the text's, the engine's `spec_ids` HashMap beside `SpecTable`.
9. **Mount as a bulk column append** (Opus F6): with instance = id and hot = 8 B, a fixed-shape row's mount
   image is `{3 RecordInit, 3 Hot, 1 Instance, 2 text records}`; ids allocated as a contiguous block
   (`new_handles(n)` in the core) make the image a memcpy plus an id rebase. Predicted: element part of the
   mount ~9 K -> ~1.5 K per row (text shaping and the label's pool writes remain). Needs the core's bulk
   handle allocation and `insert_run_with`.
10. **Removal as tombstone-free compaction.** Today removal = `remove_run` (core) + `release_scope` (cut now,
    reclaim later, lists of ids). With resources at ids, a removed run's resources are its handles' ids:
    releasing is "these ids are free" (the core already pushes them on its free list), text states and cold
    records are found from `hot[id]` when the id is reissued (already how cold records are released). No
    scope, no reclaim queue. The glyph blocks of labels need the same rule (release when the text id is
    reissued) or a per-page text-id column.
11. **Memory per element in bits**, where it stands for a painted box: core record 23 B + htab 24 B + hot 8 +
    hgen 4 + instance 48 + `LInfo` 48 + `DNode` 128 (+ 360 cache if a container that stored) + reflect 16 =
    299 B without the cache. After avenues 1, 7 and the core's 8-byte handle: 23 + 8 + 1 + 48 + ~76 = 156 B;
    with fixed-point geometry shared between layout and instance (the instance's pos/size *are* the laid-out
    rect for an untransformed box): ~120 B.
12. **A from-scratch store, knowing this.** One id space handed out in contiguous blocks per mounted run;
    every per-element fact a column at the id (so mount is an append and nothing ever moves on a structure
    edit: the opposite choice from side pages); the record tree holds ids, not payloads (payload = id for
    single-instance kinds, which is what `ibase + id` already is); streaming passes follow the record
    pages' handle column, which is sequential in ids for anything mounted together (measured basis: the
    array-of-ids probe at 1.0 ns per child even after churn). Side pages (STORE-R6 stage 3) buy contiguity
    under churn at the price of moving ~100 B per record on every split; ids-at-columns buy stability and
    zero-hop addressing from wires at the price of gather under heavy churn. The experiment that decides:
    subtree translation of 10,000 records after churn, gather through the handle column against a contiguous
    add (predicted ~12 against ~6 instructions per record; both are under the 20 target).
13. **Sized leaf size as node data** (lead's cold-layout card): every distinct label width interns its own
    cephal `Style` today (`style_id` hashes 84 B, and the style table grows with distinct widths). A
    `LayoutTree::leaf_size(node)` hook in cephal (vendored, in place) read by its cheap-leaf path removes the
    hash per label and keeps one style per call site. Predicted: -400 instructions per label at mount, and
    `StyleTable` stops growing.
14. **Layout of identical rows** (Opus F5 trace, or the memo keyed by subtree shape): cephal's cold pass is
    ~8,600 of the ~23,000 instructions per mounted row. Not started; the Opus layout lane has it.
15. **A container re-run over cached children** costs ~58 instructions per child (the residual row-count
    scaling of a width-changing text write). A box whose own width and height are definite and that nobody
    measures through cannot change its parent's layout except through scrollable overflow, which bough does
    not read: the mark could stop there. Risk: `overflow: clip` semantics differ (automatic minimum size), so
    it must be a scheduler rule, not a style change.
16. **Masked ids instead of bounds checks**: the per-id columns have power-of-two capacity, so `i & (ids - 1)`
    with an unchecked load is in bounds for any `i` (the trick the GPU masks already use). Removes three
    compare-and-branch pairs from every applier without `unsafe` at call sites. ~6 instructions per applier.
17. **`NRect` scalar ops as NEON** (lane core; Opus A4/#8): the colour applier spends ~50 instructions in two
    scalar intersects and a translate with repacks; `smax.4h` forms are ~3 each. Predicted: colour applier
    159 -> ~110.
18. **Kernel time is invisible to the instruction metric.** The baseline profile had 10 % of mount wall time
    in `vm_copy`/`madvise` under `realloc`; stage 0b removed it for the façade tables but no cell shows it.
    A wall-time mount cell under the measurement lock would.
19. **Handles for static elements are still avoided** by keeping a spare-handle list; with position identity
    in the core (Opus F1) the distinction between static and dynamic elements disappears.
20. **Label for `ltemplate` no-wrap text and for the tooltip / text field** (all one-line text one
    representation); and paragraphs' line windows in the span arena instead of two `Vec`s per text.

## Where I think the lead or the plan is wrong

* **STORE-R6's stage 3 (side pages) is not obviously the destination.** Stage 2a shows the cheaper identity:
  element id = handle index = instance index, with columns at the id. It gives zero-hop addressing from a
  wire (no `htab` load before the store), nothing moves on a split, and mount is an append. Side pages only
  win on contiguity after churn, and the measured gather cost there is small. I would run avenue 12's
  experiment before building the column plan into the core's page operations.
* **The mount target (2,500 per row) is not a storage target.** Of ~23,000: cephal's cold pass ~8,600,
  write-back ~1,700, first re-description ~2,300, text shaping and label pool writes ~3,000. Storage proper
  is the rest. The targets should be split by owner or the card will be judged on other lanes' work.
* **"Colour write under 100" end to end** is bounded below by the core's damage path (~110 in the applier
  alone today); it needs avenue 5 (damage resolved per turn) or avenue 17, not more store work.
* **The cell `redescribe_cost MOUNT=1` at 6,000 rows now measures the far-ink path** (192,000 px of content);
  keep a 1,000-row cell beside it or the storage work is invisible.
* Lanes merging `lead/core-r6` mid-refactor cost me two full conflict resolutions and ~2 hours of builds
  under load; a single integration point per day would be cheaper than "merge at every stage boundary".

## Next (what I would do first, with predictions)

1. **Run the comparison suites and the frame-path cells on `281f6c3`** before anything else (they have never
   run on this branch). Prediction: failures, if any, in hover / tooltip / decoration paths (`HF_MOD`), not
   in layout or text.
2. **The turn's fixed cost** (avenue 6): idle turn 249 -> ~60, colour write 640 -> ~450, a day of work.
3. **Layout tables as per-id columns** (avenue 1) together with the text state (avenue 3): bytes per row
   3,296 -> ~1,300, allocator calls per row 14 -> ~4, mount per row -1,500.

The three experiments of my own the amendment asks for, not yet run:

* **E1, ids in contiguous blocks**: a mounted run takes its handles as one block, so the handle column of its
  pages is an arithmetic sequence and a pass can prove "these 64 slots are ids k..k+63" with one compare.
  Predicted: write-back and subtree translation gather at the cost of a contiguous load (6 against 12
  instructions per record). Kill: if after 50 % churn fewer than half the pages keep the property.
* **E2, the paint list** (avenue 5): colour writes append the id; damage and the paint log are resolved once
  per turn over distinct ids. Predicted: 159 -> ~25 at the write, ~60 per distinct record at frame time, and
  a hover-in-hover-out within a turn costs no damage. Kill: lists longer than 64 in the heavy scenes.
* **E3, row input fingerprints** (avenue 4): a dynamic part keeps, per item, the bytes the item's closure
  read from the owner (recorded once through a tracking borrow at mount); an idle re-description compares
  those bytes and never calls the closure. Predicted: idle re-description 298 -> ~25 per row. Kill: if the
  tracked read set cannot be made sound for closures that capture by reference (it is the fine-grained
  reactivity problem in another coat).
