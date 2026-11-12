# Core lane, round 6: handoff (2026-11-12 checkpoint)

Supersedes the previous session's handoff (its design note is still `DESIGN-R6.md`; its harness is the one used
here). Everything below was produced by this session unless it says otherwise.

## State

* Branch `lane/core/round6`, head: the commit that adds this file (parent `be7e104440`). Working tree clean.
* `lead/core-r6` is merged in up to `8ded38964f` (the façade's exact inks). `lead/core-r6` at `4f4c469ef5` is
  **not** merged.
* In `round6` / `lead/core-r6` by the lead's word: fix (a) (`e8f29816ad`) and the coordinate contract
  (`ffe1b961d5`). **Not merged anywhere else:** vector translate (`5d82a89d80`), state planes (`1e5263d641`), the
  gapped directory (`3c0c5248fe`), the far-write fix (`be7e104440`), `COLUMN-PLAN.md`.
* Commits in order: `a5ef0766c5` matrix + attribution + harness check; `e8f29816ad` fix (a); `fcff4366c6` harness
  pre-touch; `77e2c80609` / `aedfb123fd` fix (b) rejected and reverted; `3f4554fc96` / `150b7d5db0` fix (c) rejected
  and reverted; `e615b042c6` column-plan contract; `ffe1b961d5` exact coordinates; `5d82a89d80` vector translate;
  `495d504e42` state-plane contract; `1e5263d641` state planes; `3c0c5248fe` gapped directory; `be7e104440` far
  writes.

## Numbers

Instructions per operation (`proc_pid_rusage`), minimum of five samples per run, median of three runs unless a
row says otherwise; machine under heavy load from other lanes (instruction counts do not care; the harness reads
every page header before an interval because the kernel's soft faults on aged pages are counted). Reproduce:

```sh
export PATH=$HOME/.local/share/cargo/bin:$PATH RUSTC_WRAPPER= CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=$PWD/target
cargo build --release -p bough-core --example core_bench --example trace_bench
target/release/examples/core_bench --cost-r6            # fresh tree;   R6_WARM=1 for warm columns
target/release/examples/trace_bench --cost-r6           # spatial cells
R6_FAR=1 target/release/examples/core_bench --cost-r6   # far-content cells
python3 docs/lanes/core/table-r6.py table <tag> | compare <base> <new>     # CSVs in docs/lanes/core/data/
cargo run --release -p bough-core --features attr --example r6_attr        # attribution (R6_WARM=1)
cargo run --release -p bough-core --example r6_check                       # independent loop
```

Tags in `data/`: `base` (round start + the lead's three defect fixes), `a`, `b`, `c`, `f3` (coordinates), `f13`
(vector translate), `st` (state planes), `g1` (gapped directory). Tables: `TABLE-R6.md` (base),
`TABLE-R6-g1.md`, `data/compare-*.md`.

**Baseline matrix, headline cells (warm columns), 1e3 / 1e4 / 1e5 / 1e6, top-decade exponent:**
insert8-front 3,961 / 8,878 / 58,698 / 556,844 (0.98, ideal 0); insert8-middle 3,557 / 7,545 / 45,860 / 428,952;
append1 546 / 577 / 602 / 300,808; remove64-front 4,714 / 9,793 / 59,788 / 559,450; build-append8 exponent 1.82;
bulk-build 98 k / 856 k / 8.3 M / 85 M (1.01); parent-last (wide tree) 229 / 364 / 1,800 / 15,900 (0.95);
freeze-idle 1,497 / 5,744 / 48,148 / 472,434 (1.0); hit-gap interleaved 306 -> 123,870; translate-half
2,503 -> 865,068; culls linear in output. Harness check: an independent loop agrees to 12 instructions at 1e3
and 10 at 1e6. Verified: 3 runs.

**Attribution** of a warm front insert of 8 at 1e6 (557,127): `Prefix::invalidate_from` 298,404 (same from the
front, middle and end: it ignored its index), `reindex_pages` 140,638, the five column shifts 76,868,
`fix_chunks` 38,418, page work 2,707. One run of three samples (`data/attr-base-warm.csv`).

| change | cells (warm, 1e3 / 1e6 unless said) | verified |
|---|---|---|
| (a) prefix honours its index | append1 1e6 300,808 -> 2,473; insert8-middle 428,952 -> 188,961; insert8-front -> 375,195; build-append8 3.23 G -> 187 M; every 1e3 cell <= 1.001x | 3 runs |
| exact coordinates (`f3` vs `a`) | no cell above 1.02x; set-ink-pos 421 -> 371, set-ink-handle 387 -> 337, touch-handle 131 -> 117, cull-100 5,928 -> 5,519, hit-present 325 -> 303 | 3 runs |
| vector whole-page translate | translate-half 2,485 -> 2,039 / 849,441 -> 271,499 (109 -> 32 instructions a page, disassembly read); translate64 unchanged | 3 runs |
| state planes, unused | no cell above 1.02x vs `f13` | 3 runs |
| state planes, used | flag flip through a handle 95; flip + flips + commit 215; range write 57 a page; any_state 11 a page (now ~16: see losses) | 1 run |
| gapped directory (`g1` vs `base`) | insert8-front 3,961 -> 3,215 / 556,844 -> 4,472; insert8-middle 3,557 -> 2,830 / 428,952 -> 3,614; append1 546 -> 538 / 300,808 -> 2,420; remove64-front 4,714 -> 4,973 / 559,450 -> 7,840; move64-middle 15,789 -> 13,843 / 569,674 -> 15,845; parent-last 229 -> 222 / 15,900 -> 743; subtree-root 205 -> 210 / 17,812 -> 671; freeze-idle 1,496 -> 753 / 472,434 -> 754; build-append8 3.23 G -> 178 M | 3 runs (before the live-count scan fix and the far fix; those two commits have one quick 1e3 run each, no cell moved by 1 %) |
| adversarial page splits (256 at the front) | amortised / worst: 7,929 / 29,967 at 1e3; 8,077 / 31,483 at 1e4; 9,087 / 80,694 at 1e5; 15,826 / 373,671 at 1e6 | 1 run |
| far writes (rows of 32 px, 1,000 / 3,000 / 10,000 / 100,000 rows) | writeback-row 1,828 / 26,184 / 35,127 / 38,603 -> 1,828 / 2,014 / 2,081 / 2,107; far-set-ink 280 / 5,039 / 3,993 / 6,153 -> 280 / 374 / 374 / 374 | 1 run |

**Losses and regressions that stand (limit 1.02x):**

* Gapped directory against the commit before it (`data/compare-g1.md`), at 1e3: remove8-* +7..9 % (1,062 -> 1,134),
  remove64-end 1,773 -> 2,136 (+20 %; +85 % at 1e6: 897 -> 1,656), remove64-front +11 %, move8-front +3 %,
  freeze-one +20 % (1,807 -> 2,176: more columns copied when anything changed), freeze-all +6 %, state-any
  +26..45 % (259 -> 328), state-run-half +5..9 %, subtree-root +5 (205 -> 210), translate-half +2.4 % at 1e4.
  hit +13 % and cull-1-band +6 % at 1e3 were from scanning 64 entries where 16 are live: fixed in the same commit
  (scan the block's live count), **not re-measured over three runs**.
* A far ink write is 374 against 280 near; a far page translates at ~315 instructions against ~35 for a plain page
  (scalar i32 path).
* The block cut is still O(pages after): 374 k worst at 1e6.
* Fresh (not warm) cells include the one-time reallocation of the directory columns; with the gapped directory
  the warm-up no longer grows blocks, so the first block growth of a tree shows up in the adversarial cells at
  small N (the 30 k worst at 1e3 is mostly ten `Vec` reallocations).

## Not verified / known broken

* **No façade or pixel suite was run by this lane on any commit.** `cargo check --workspace --all-targets` passed at
  `f3`; it failed after merging `lead/core-r6` at `59e26e2f4c` in `comparisons/bough-app-sketch-code` (the lead's
  side, since fixed there); not re-run since the gapped directory. The façade compiles against `dir_len()` in
  two places I edited (`app.rs` n_dir, `kernel_bench.rs`); not built.
* **Carved capacity:** `dir_id` / `dir_ink` are carved with `Caps::pages` entries and the shader clamps
  `n_dir` to `m_page + 1`. Holes eat into both: with blocks at 56 of 64 (and 32 after a cut) a tree can hit
  "column full" at roughly half to 7/8 of `Caps::pages` pages. Fix on the presenter side: carve 2x, clamp to the
  directory capacity.
* `comparisons/bough-heavy/tests/gpu_bounds.rs` walks `0..num_pages()` directory entries (now it should walk
  `dir_len()` and skip null ids); not changed, not run.
* Far transformed pages are unsupported (a transform on a page outside the domain keeps the old semantics).
* `rewrite_run` carries state bits only for records with handles; `RecordInit` has no initial state; KF_* kind
  bits are not stored as planes.
* A layout mutant (uneven block sizes in `relayout`) was not caught by the tests I ran (it may be a legal
  layout; not analysed). The exhaustive small-domain proof and the ilo-prod fuzzing the lead asked for were not
  done; nothing was run on ilo-prod. No x86-64 twin of any kernel was written.
* `freeze_into` skip relies on `mgen` being bumped wherever replicated state changes; covered by the differential
  suite's replica check and one mutant, but `set_window` does not bump it (the ring's window is not copied
  either: pre-existing).
* The three-run matrix was not re-run on the last two commits.

## Experiments (including the killed ones)

1. **(a) prefix honours its index.** Landed. 19 -> 7 instructions per page of the suffix only.
2. **(b) chunks as runs of directory entries over flat columns.** Killed: fix_chunks 38,417 -> 5,168 at 1e6 (7.4x,
   under the card's 10x) and 55 cells above 1.02x: finding the chunk of an index is a search (translate64 +6.8 %,
   remove64-end +49 %), the first insert into a full chunk splits it (+19 % at 1e4), freeze copies one more
   column. `77e2c80609`.
3. **(c) `Page::dir` as a lazily repaired hint.** Killed: reindex gone (insert8-front at 1e6 -> 234,578) but the
   repair (~35 instructions through a cold call) is dearer than the eager write (9): insert + pos_of on every
   page 1.57x at 1e6, 1.10x at 1e3; pos_of 24 -> 29. `3f4554fc96`. Conclusion that drove the directory: an index
   must be positional only inside a block.
4. **Exact coordinates** (page frames: plain / far / wide). Landed; then the store lane measured every far write
   going through `page_rects` + `reencode` (and the frame nearest zero making each next row re-frame its page):
   fixed in `be7e104440`.
5. **Vector whole-page translate.** Landed (32 instructions a page).
6. **State planes.** Landed. First version re-derived both summaries from 32 words on every write (262 per
   flip): replaced by per-plane incremental summaries (95).
7. **Gapped directory.** Landed. Sub-experiments: rank inside a block by NEON count over 64 u16 (kept: parent
   and locate did not get slower); `next_dir` by block length (6 instructions) vs by a null sentinel in the pointer
   column (kept: one load that the caller needs anyway); settle by running sum over all later blocks (3,683 at
   1e6) vs by one vector add of the delta (kept).
8. **Packed-memory-array rebalance instead of the block cut.** Killed at this fill: 256 page splits at the front
   of 1e6 records cost 52.7 k each amortised and 1.2 M worst (windows at 87 % density rebalance every few
   inserts) against the cut's 15.8 k and 374 k. The Opus collaborator's 1,162 is the directory part *without*
   rebalancing; with blocks filled to 48 and looser thresholds it may still win at 1e6 (see avenues).
9. **Harness artefacts found:** fresh trees pay a reallocation of the directory columns in their first split
   (half of insert8-front at 1e3); kernel soft faults on aged page headers inflate 1e6 cells by 25-40 % (pre-touch
   added); cargo reused a mutant's objects for the restored source once (the mutant script now touches the file).

## Underexplored and exotic avenues

Each: mechanism; what it could buy; cost or risk; cheapest experiment; dependencies.

1. **Blocks out of paint order (order array) or block-id indirection, to remove the cut's spike.** Pages store
   (stable block id, slot) and a 280-entry table maps block id to position; a cut inserts a block in the columns
   (streaming memmove, ~40 k at 1e6) without touching any page header. Buys: worst case 374 k -> ~45 k at 1e6,
   amortised 15.8 k -> ~8 k. Costs one dependent L1 load on `handle_loc` (pos_of 24 -> ~27). Experiment: add
   `r_pos[block]`, measure pos-of-handle and set-ink-handle; kill above +3. Alternative with zero lookup tax:
   the shader iterates an order array so blocks never move (needs lane gpu).
2. **PMA with 75 % fill.** Same code as the killed experiment (`relayout` over a doubling window exists in
   history of `3c0c5248fe`'s predecessor edits; ~40 lines) with `RUN_FILL = 48` and thresholds 62 -> 50. Predicted
   amortised ~8-10 k at 1e6, worst ~100 k (a 16-block window), cost: scans read 1.33x the entries (cull cells at
   capacity must stay within 1.05x), memory +33 % of 26 B per page. Experiment: flip the constants, run the
   adversarial cells and the spatial cells.
3. **Page-relative coordinates everywhere (no near/far branch).** Today plain pages store absolute (offset 0) and
   only far pages use their frame; the API still takes absolute 16-bit rects for the common path. The uniform
   form: every insert carries an i32 origin and 16-bit relative inks; a page's frame is always its own; the
   absolute form exists only as the clamped view at query time. Buys: deletes `F_FAR` and the 374-vs-280 gap,
   makes scroll never depend on distance, lets the façade pass space-local rects (STORE-R6's fixed-point
   geometry) without forming absolutes. Costs: every abs() is an i32 add + saturating narrow (3 NEON
   instructions instead of 1 for offset-0 pages: +2 on set-ink, hit and cull per page; estimated +0.5..1 %),
   and `RecordInit` changes shape (façade-wide). Experiment: make `Page::abs` always take the wide NEON path and
   run the matrix; if no cell passes 1.02x, the split has no reason to exist.
4. **Far page translate in vector form.** `translate_far` is scalar (315 a page). The same loop as
   `translate_pages` with the i32 offset add and the summary recomputed as `saturating_narrow(union + off)` is
   ~45 a page. One afternoon. Needed for a mounted 20,000-row column (scroll 2.7 M at 100,000 rows now).
5. **Summary word per page and per block (Opus F4).** One `u64` directory column: state `any` (16) + `changed`
   (16) + written-since-publish + has-clip + has-xf/far + has-text + has-hit-target; block OR next to
   `c_ink`. Buys: any_state over a subtree from 16 a page to one AND per block (1e6: 250 k -> ~600), hit and
   tab-order skipping, freeze copying only written blocks (freeze-one 1e6 490 k -> ~3 k), realisation scans for
   dormant runs. Costs: one more column moved by `dir_move` / `relayout` (the plumbing is in one place now) and
   an OR on writes that set a bit. Experiment: `d_word` with the state bits only, re-run state-any cells; kill
   if set-state-handle rises above 100.
6. **Seals as page columns (with store).** A fixed-stride seal (N words per record) in a side page moved by the
   column plan; an unchanged page of 64 rows is 64*N words compared with NEON (`vceqq` + `vminvq`), and the first
   mismatch lane is the first changed record. Buys: Opus measured 159 vs 256 instructions per row for a
   fixed-stride record; a page-at-a-time compare of 4-word seals should be ~12 instructions per row (estimated),
   i.e. 20x on the unchanged-row cost that dominates re-description. Costs: descriptions must produce a
   fixed-shape seal (variable text by (id, hash-free operand pointer) or interned string id); needs the column
   plan implemented (trait is in `COLUMN-PLAN.md`, not built). Experiment: a 64-row page of 4 x u64 seals compared
   against a rebuilt buffer in `r6_check` style, to get the per-row number before any façade work.
7. **Column plan implementation.** The hooks for state planes are now at every page-moving site
   (`insert_in_page`, `insert_split` + `st_copy_virtual`, trim, `merge_next`, `split_at`, free); making them
   `P::shift / copy / clear` with `Bough<P = ()>` is mechanical. Risk: monomorphising the whole core per plan;
   gate: disassembly of `insert_in_page::<()>` equal to today's.
8. **Position is identity (Opus F1).** With the gapped directory a record's (directory index, slot) is stable
   between structural edits in its block; a pass that streams pages needs no handle. The handle table could be
   opt-in (records that outside code names). Buys: `HEntry` 24 B per record and the `fixup_handles` loops on
   every split (page work is 2.2-2.7 k of a 3.2 k split at 1e3; fixups are a part of it not yet attributed).
   Experiment: attribute `fixup_handles` in `r6_attr` (one more slot) on a tree with all records bound.
9. **Hot/cold split of the page by measured access.** The 1,472 B page mixes columns: cull reads `gink`, `ink`,
   `payload` (and `clip` only for clipped groups); hit reads `gink`, `ink`, `kind`; depth scans read `depth`;
   structural edits touch all; `handle` (256 B) is read only by handle fixups and `handle_at`. Splitting
   `handle` + `depth` + header into a cold half makes a cull touch 8 lines of 128 B instead of up to 12.
   Not measured: no cache-line counter was built. Experiment: a counting shim (`mincore`-free: record the set
   of `addr >> 7` touched through wrapper accessors under a feature) for cull-1-band, hit, set-ink, insert8.
10. **Cache lines per operation.** Owed and not delivered. Analytical now: a block-local insert touches the
    block's six column ranges (<= 64 entries: 8 + 4 + 8 + 2 + 4 + 2 B each = 1,792 B, 14 lines of 128 B) + up to
    63 page headers (one line each, the worst part: scattered) + the block-start suffix (nr * 4 B). The header
    writes are the argument for keeping `dir` out of the page (a side array `dir_of[page id]` is 4 B per page,
    contiguous: 64 pages = 2 lines instead of 63). Experiment: move `dir` to a side array indexed by page id and
    measure `pos-of-handle` (one more load: page -> id -> dir) against the split cells' wall time, not
    instructions.
11. **SIMD over state planes.** `flips` over many pages is `bits ^ seen` per page then `ctz` per record; a
    consumer that applies a table row per flipped record could take 64-bit masks per page directly
    (`flips_pages(f, |page, mask, now_mask|)`) and write a column with a masked store. The structural moves of the
    32 words (`StatePage::shift / copy / clear`) are scalar loops LLVM may or may not vectorise: disassembly not
    read. `any_state` across 16 planes at once is one `u16` AND per page (already the layout).
12. **Parallel page walks.** Bulk build is 84 instructions per record at 1e6, all per-record `Page::write` +
    regroup: pages are disjoint, the directory is filled first, so `insert_split`'s loop over new pages can be
    split across threads above a grain (estimated grain: 64 pages = 350 k instructions = ~70 us; below that
    one thread). Same for `reencode` of a far column and for the façade's realisation. Not measured. Depends on
    a pool (cephal has one).
13. **Deferred settle (Opus F10) with per-page motion (F7).** A write stores the ink and sets a dirty group bit;
    page and block summaries and damage are settled once per dirty page per frame. Opus measured 185 -> 76 per
    write without marks, 241 -> 199 with per-record marks; the gain needs damage per page motion instead of per
    record. For the lead's two targets (visible_ink_handle ~110 of a colour applier's 162; geometry write 573):
    the colour write needs no ink at all if the paint log records the handle and resolves visible ink at settle
    in one batch (independent loads); predicted core share 110 -> ~25. The geometry write's 573 is two edge-band
    marks (~60 each) + 185 bookkeeping: with settle per page it is one store + a dirty bit (~20) and the marks
    move to settle. Not built.
14. **NEON rect algebra outside the kernels (Opus A8/b).** `NRect::intersect / union / translate` compile to
    scalar `cmp/csel` + repack (~14 instructions vs 3). `translate_in_domain` here is already the NEON form.
    Making `NRect` wrap an `int16x4_t` on aarch64 (and `__m128i` on x86-64) fixes every caller at once. Risk:
    `Eq`/`Hash` and const constructors. Predicted: set-ink-handle 342 -> ~300, visible_in 20 -> 8.
15. **Damage planes for any window size** (finding 6, not started). Design ready: rows of `stride` u64 words
    (2 for <= 4,096 px: the same bytes as today's u128), one phantom word after the last row so the cull's
    16-tile window is one unaligned 8-byte load + shift (cheaper than today's variable u128 shift), single-word
    marks unchanged, multi-word loops by stride, `shift_within` keeping the u128 path for stride 2. Crosses
    bough-text (`Marks::or_rows(u128)`), `app.rs` (`Vec<u128>` rows) and the frame slots (8-bit tile spans:
    8,192 px). Predicted: one-block cells within 1.00x (marks) and -3 % on tile culls.
16. **Dormant runs** (not started). With state planes: a dormant run is a record with `S_DORMANT` whose payload
    indexes a side table `(part, first, count)`; `realise` = `insert_run` after it + one ink/payload write;
    `release` = `remove_run` + extend the neighbour run; the cull variant tests `bits[S_DORMANT]` of pages with
    state. Page-level dormancy (a page with len 0 and a virtual count) was considered and not chosen: it forces a
    page boundary around every run (a 16-page UI with 20 small lists becomes 40+ sparse pages; per-page cull
    overhead is ~100 instructions) and a split of a run becomes a directory insert instead of a record insert.
    Read `wt/dormant` STATE.md first (I did not).
17. **Rank without the top-level search.** Block starts are searched by a branchless binary search (dependent
    loads). For <= 64 blocks (267 k records) a NEON count over `r_start` is 16 compares with no chain; above
    that a second level of 64-block groups. Predicted: locate at 1e5 -8 instructions, and the dependency chain
    shortened by 5 loads. Not measured.
18. **Removal path at 1e3.** The regressions above are bookkeeping: settle recomputes the whole block
    (local starts, union, min) after one page goes; it can recompute from the first changed slot and skip the
    union when the removed page's ink is strictly inside the block's. Predicted: remove64-end 2,136 -> ~1,850.
19. **bbtree items not applied:** last-leaf append cache (an append still locates: append1 538); bulk build
    that sizes pages from the template and fills them without the general insert path; type-stable storage that
    never reallocates for the directory columns (they are `Vec`s that double: the 30 k worst case at 1e3);
    node plans (the column plan is designed, not built); interleaved lookups for batches of handle writes
    (`set_inks_handles`: load all `HEntry`s, then all page headers, then write: independent misses overlap).
20. **x86-64 twins.** Every NEON block added this round (`translate_in_domain`, `translate_pages`, the rank
    count, `len_delta`, `block_min`) has only a scalar fallback. The platform-host lane measured 2-5x on the
    older kernels.

## Where I think the lead or the plan is wrong

* **"Handle_loc must not get slower" and "every structural edit <= 300 instructions at 1e6" cannot both hold**
  with flat GPU-visible columns: either an index is physical (and something moves on overflow) or it is
  derived (and every handle write pays a load). The plan should pick per scale: with dormant runs trees stay at
  1e3-1e4 records, where the cut costs 30 k once per 8-32 page splits; the 1e6 target matters only for
  all-mounted content.
* **The 1.02x rule on every cell at 1e3 blocks foundational changes for bookkeeping noise** (a removal that
  goes 1,062 -> 1,134). I kept reporting them as losses; a weighted set (the signal-path cells first, as the
  user said) would be a better gate than "every cell".
* **Too many re-plans.** This lane's queue was reordered eight times in one session; three items were started
  and parked (damage planes, dormant runs, column plan). The matrix and the two rejections were the valuable
  early output because nothing interrupted them.
* **The state-plane `seen` word doubles the state memory for one consumer.** If variant tables apply flips at the
  write (they know old and new there), `seen` and the flip lists are unnecessary; they exist because the
  contract said "iteration of flipped bits between two versions".
* **`freeze_into` has no caller.** Its cells shaped two decisions (mgen, the freeze-one regression). Either a
  reader lands or the cells should leave the gate.

## Next (in order, with predictions)

1. Re-run the three-run matrix on head and fix the removal bookkeeping (avenue 18): remove8-* back within
   1.02x, remove64-end 2,136 -> ~1,850; then summary words (avenue 5): state-any 328 -> ~120 at 1e3, 250 k ->
   ~600 at 1e6, freeze-one at 1e6 490 k -> ~3 k.
2. The colour and geometry write paths (avenue 13 + 14): visible-ink resolved at settle from the paint log,
   NRect as a vector type: core's share of a colour applier 110 -> under 40; `set_ink_handle_edges` 573 -> under
   150 with per-page settle.
3. Page-relative coordinates as the only form (avenue 3) together with the far vector translate (avenue 4):
   far-set-ink 374 -> 285, far-scroll 315 -> 45 a page, `F_FAR` deleted; then damage planes for any window size
   (avenue 15) since both touch the same absolute-view code.
