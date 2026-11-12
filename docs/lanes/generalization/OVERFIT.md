# Overfitting map: gui-bench and an adversarial matrix on `bough::prelude`

Lane `generalization`, round 6. Framework sources are `lead/round6-base` (aa9943e7ae), unchanged on this branch
(`git diff aa9943e7ae -- crates vendor` is empty); `lead/core-r6` is not applied. Crate: `comparisons/bough-guibench`
(standalone workspace; gui-bench 13f3cb61 `spec.rs`/`cli.rs` verbatim). M3 Pro, macOS, direct Metal, release, thin LTO,
nightly-2026-10-17, machine under load from other lanes.

**Numbers.** Instructions per operation, calling-thread counter (`bough_metal::perf::tinstr`), one process per cell:
two warm-up operation/undo pairs, then three blocks of eight pairs; the cell is the median block. Block spread
(hi - lo) / median is above 5 % in 63 of 405 R0 cells: the ticker cells (up to 76 %, cause not
isolated) and structural list cells with all rows mounted (up to 25 %); lo and hi are in the sweep JSON. **R0** = input turn + frame through the GPU's input, no
Metal; **R1** = with the Metal frame and present wait (CPU side only). An operation and its undo are averaged.
`select` is two host events (press, release). No wall time, no GPU time and no lock were used.
**Gates** (4 repeats, surface rings 4 and 1, after each of 8 transitions; rings 2 and 3 on a 12-cell subset at
N = 1,000, all passing): presented == full redraw; retained ==
fresh mount of a copy of the final state; the model against gui-bench's spec; gui-bench's layout points.
*Presented == full redraw passed in every cell that ran, including every cell with wrong pixels below.*
**Exponent** = log-log slope of R0 against N (all sizes / last step); a cell is OVERFIT above ideal + 0.2.
**Negative control**: `--mutant` (select additionally reads every row id, ~13 instructions per row) is flagged
(0.35 / 0.75) where plain select is 0.01 / 0.00, so an O(N) term of 13 instructions per row is visible at N = 10,000.

Cell codes: `S` stale scroll-box content (finding 1) · `H` stale hover after a scroll (8) · `R` route ran a stale
closure (5) · `D` content beyond the i16 pixel domain (3) · `G` / `I` / `SP` column full: glyphs / instances / spaces
(4); prefix `m` = at mount with default caps, `r` = during the measured run · `†` = labelled follow-up with
`glyphs = 1,048,576, spaces = 16,384` (N = 10,000) or `spaces = 8,192` (mounted, N = 5,000) · `ⁱ` = follow-up with
one idle frame after each operation. Default caps are `ui.rs::APP_CAPS`.

## Findings, ranked by how much of a real application they affect

| # | finding | numbers | mechanism (file :: function) | general mechanism that removes it | owner |
|---:|---|---|---|---|---|
| 1 | **Plain state inside a scroll box is never re-described: wrong pixels.** Every coarse table, coarse ticker and search cell, at every N from 1 row up. | retained != fresh by 215 bytes (ticker, K = 1) to 154,509 bytes (search); the same views in a plain box pass. Costs in those cells (select 9.7k flat) are the cost of doing nothing. | `template.rs :: ScrollBox::mount` wraps the content in `describe::mount_opaque`; `ScrollBox::update` is `describe::update_opaque` only. | The scroll view keeps its tape and updates its children like any box (scroll offset and clip are already signals). | surface-api |
| 2 | **A re-description that runs costs ~3,300 instructions per row per handler, and every handler runs one over the whole root**: clicks, keys, timer turns, hover crossings of a `hover_bg`, handlers that change nothing. | coarse plain: select 6.56M at 1,000 rows, 65.7M at 10,000 (exp 0.98); ticker with one row changed 3.29M at 1,000, 32.8M at 10,000; swap 3.31M at 1,000; no-op key 17.7M at 10,000 children of 3 elements (590 per element); hover crossing 25k / 45k / 68k at 40 / 80 / 128 sibling cells; one signal with 10,000 paint dependents 1.80M by wires alone, 19.5M through a handler. Positional realign makes one remove/insert at index 2 cost 23.4k per row (23.5M at 1,000, 234M at 10,000): every later row's two texts are re-shaped. One wrapped row growing in a plain Vec: 6.46M at 1,000. | `input.rs :: settle_input` and `app.rs :: turn` call `describe::redescribe` unconditionally (`pointer_move` does when `update_hover` changed the chain). Per element `layout.rs :: LBox::update` / `Para::update` call `layout::restyle` -> `eng.style_id`, which hashes and compares the whole `Spec`: `SpecKey::eq` + `hash_one` + `sized_spec` + `restyle` are 49 % of samples, malloc/free/memmove 26 % (profile of select at 1,000). `describe.rs :: update_dyn` realigns by position; `each` has no key (`Element::dyn_key` exists only for entity views). | Owner boundaries that skip a subtree whose inputs did not change (dependency-tracked or compared operands), so a turn costs what changed; a retained-spec identity check before hashing; hover paint that does not schedule a re-description; keyed dynamic parts spliced as runs. | surface-api |
| 3 | **Content further than 32,766 device px from its scroll origin is drawn wrong after scrolling** (i16 rects). gui-bench `rows` cannot be scrolled past row 1,023 at scale 1, row 511 at scale 2. | rows in a scroll box, scrolled to the end: 1,000 and 1,020 rows exact; 1,030 rows: 100 of 800 probed pixel rows wrong; 1,100 and 2,100 rows: 428 of 800 wrong (every non-white pixel of the column is missing); scale 2: 500 rows exact, 520 rows 2,065,646 bytes differ. One 40,000 px block: the window is blank after the scroll. Fine table with all rows mounted, swap of rows 1 and N-2: exact at 1,020, wrong at 1,030 and 10,000. A fresh mount at the same offset is exact. | `bough-core/src/nrect.rs :: NRect` is `[i16; 4]` (domain +-32,766, constructor clamps); records store absolute device-px inks; `bough.rs :: scroll_run` shifts the saturated lanes. | Inks in the coordinates of their space (content-local, 32-bit at page level), with the window test done after the space offset; i16 only for the window-relative form. | core |
| 4 | **Fixed columns, and reclamation only when idle.** (a) No non-virtual gui-bench scenario mounts at 10,000 rows with default caps. (b) Handlers that remove and add subtrees exhaust the columns although the live set is constant. | (a) rows / table / ticker / spinner / typing / search / resize at 10,000: `column glyph_x is full (capacity 524288, need 524320)` at mount (52 glyph slots per row); a list with >= 4,095 mounted rows: `space column full: raise Caps::spaces` (one space per row). (b) fine table, 10 rows, all mounted: F6/F7 (append / truncate 1,000) panics with `glyph_x` full within 10 pairs; with one idle frame per operation it runs (18.0M per op). Replace of 1,000 mounted rows, or anything at 5,000, still overflows with idle frames (budget 4,096 nodes per frame); coarse clear at 5,000: `instance column full (65536)`. | `rt.rs :: alloc_space` / `alloc_run`, `bough-mem :: Column::grow_to` (carved columns cannot grow); `listview.rs :: mount_row` takes a space per row; `rt.rs :: reclaim` is called only from `app.rs :: turn` and from an idle `frame_as`, `RECLAIM_BUDGET = 4096`; `input.rs :: settle_input` never reclaims. | Growable columns (reserve address space, commit on demand); reclaim on allocation pressure before a column grows, not on idleness; rows positioned without a space each. | memory, core |
| 5 | **A row cannot be clicked the documented way.** Routes inside `list()` rows panic; routes on re-described rows keep their first closure. | `block(..).on_click(..)` in a list row: `click inside a list row or an entity view: routes run on the app's root owner` (executed; `hover_bg` in a row works). Coarse plain table, replace then click: model assertion fails at every N (the route selects the id captured at mount). | `input.rs :: root_owner_only` (line 467); `input.rs :: On::update` updates its child, not the route. | Routes carry their owner (class + address) and are refreshed by the description that re-states them. | surface-api |
| 6 | **A window wider or taller than 4,096 device px is refused.** A maximised window on a 5K or 6K display (5,120 or 6,016 px) cannot open. | 4,096 x 800 and 1,280 x 4,096: exact. 4,128 x 800, 1,280 x 4,128, 6,016 x 3,384: `window up to 4096x4096` at mount. | `bough-core/src/damage.rs :: DamageRing::new` (128 x 128 tiles of 32 px, one `u128` per tile row); `rt.rs :: MAX_WINDOW`. | Tile rows of as many words as the window needs (or a second tile level), sized at resize. | core, gpu |
| 7 | **`list()` in a scroll box whose port sits at the window origin mounts no rows** until the list changes (the first insert mounts them; a scroll was not tried). "The whole window is one scrolling list" shows an empty window. | 100 rows: 4 records with the port at (0, 0) (also with a footer below it); 119 to 125 records with a 1 pt or 40 pt block or a text above it; 122 in a plain root box. Six row shapes, same result. Retained == fresh passes (both empty). | Not fully traced. `listview.rs :: lr_place_region` reads the rows' clip (`lr_visible`) in `lists_place`, before `layout.rs :: lay_flush` writes the port's clip back; `template.rs :: move_view` re-places the regions only when the port moved from its initial (0, 0). | The region's visible rect is a dependency of the clip: writing a clip dirties the regions that read it. | surface-api, core |
| 8 | **Hover is stale after a scroll**: the row that was under the pointer stays highlighted; the row now under it is not. | rows and virtual, scroll 16 pt or a jump: retained != fresh by 198,048 bytes at every N (100 to 1,000,000). With the pointer re-delivered at the same point every one of those cells passes, at 1,000,000 rows too. | `input.rs :: wheel_points`, `wheel`, `tick` call `update_hover` before the scroll write and not after. | Hover is a function of (pointer, tree epoch): re-evaluated when either changes. | surface-api |
| 9 | **A structural list edit costs O(rows after it)** even when nothing visible moves. | fine table, 138 records mounted: swap / remove / insert 73k to 102k at 1,000 rows, 260k to 283k at 10,000 (exp last step 0.7 to 0.8). Front insert: 54k per insert at 10 rows, 75k at 1,000, 257k at 10,000, 2.08M at 100,000 (20 instructions per row); random move 27k / 148k / 1.37M. All rows mounted: swap of rows 1 and N-2 1.71M at 1,000, 16.0M at 10,000 (1,600 per row between); remove / insert 1.97M at 10,000; select with the list outside a scroll box 13k -> 69k at 10,000 (`LList::update` sums the rows' record counts, ~3 per row per re-description). | `listview.rs :: lr_replay` -> `mlist.rs :: HeightIndex::assign_from` + `rebuild` rewrite the height suffix (1,095 of the 1,188 samples under `lr_replay` at 100,000 rows); `listview.rs :: lw_permute` / `lw_remap` rewrite the records of the whole span; `listview.rs :: LList::update`. | An order-statistic tree over heights and record counts (insert / move in O(log N)), structure edits as motion of runs. | core |
| 10 | **Resize re-lays every mounted row.** | rows: 1.61M at 100, 16.1M at 1,000, 152M at 10,000 (16k per row, exp 0.98); R1 at 10 rows 3.63M (full redraw at the new size). | `layout.rs :: lay_flush` computes and writes back the whole content group (`place_text` per row) when the width changes. | Layout of subtrees outside the exposed rect deferred until exposed; rows whose result does not depend on the width skipped. | core |
| 11 | **No font fallback; the pixel gates cannot see it.** | Arial: a CJK line, an emoji line and an unassigned codepoint draw the same four boxes (ink 200, same extent). Arial Unicode: CJK draws (ink 964), emoji is still the missing glyph, never coloured. Combining sequences match the precomposed forms (ink 411 = 411). presented == full passes in all of them. Right-to-left order not verified. | `bough-text` shapes with the one configured face; no fallback chain. | Per-run font fallback and colour glyphs; a text oracle that is not the renderer itself. | surface-api (text) |
| 12 | **A description nested d levels deep costs O(d^2).** | 10 / 50 / 100 / 200 nested boxes: hover 9.5k / 54k / 157k / 533k, key 8.4k / 51k / 153k / 520k (last step exp 1.77); `_platform_memmove` is 1,270 of ~1,620 main-thread samples at 200. Pixels exact. The 200-level type takes minutes to compile. | `elem.rs :: Element::update(self, ..)` / `mount(self, ..)` take the description by value: `layout.rs :: LBox::update` moves the remaining nested struct at every level. | Walk the description by reference. | surface-api |
| 13 | **Scrolling a scroll box touches every record in it.** | rows scroll R0: 11.9k at 100, 20.2k at 1,000, 92.8k at 10,000, 180k at 20,000 (9 per row). | `template.rs :: patch_view` -> `bough.scroll_run` over the whole subtree, `view_clips`. | The space offset alone moves the content; records outside the port are not visited. | core |
| 14 | **Nested auto-height lists are not laid out to a fixed point at mount.** | List rows that each own a list, 20 levels: retained != fresh by 30 bytes; 50: 1,392 growing to 3,261 over four turns; 200: 8,592 to 32,064. Each later turn makes the retained boxes taller; cost per toggle at 200 levels 443k to 11.2M. 10 levels exact. | `layout.rs :: lay_flush` and `listview.rs :: lists_place` stop after 8 passes; a height needs one pass per level. | Heights propagate bottom-up in one ordered pass (children before parents). | core |
| 15 | **Scroll offsets are f32 points.** | Virtual list of 600,000 / 1,000,000 / 4,000,000 rows at its end: a 1 pt wheel step is lost (1,487 instructions, offset unchanged; 34.7k at 100,000 rows where it moves). Pixels at the end are exact up to 4,000,000 rows (128M px). | `input.rs :: wheel_points`: `Signal<f32>` offset + delta. | Offset as integer device px (i64) or (row, remainder). | surface-api |

Not defects, recorded: a wheel at the end of an inner scroll box does not continue into the outer one. A live scale
change has no host entry point (`backend/host.rs` reads the scale once); scale 2 as a fresh configuration passes both
gates (rows hover and scroll, fine select and update10). Prelude gaps are in [FRICTION.md](FRICTION.md); new this
stage: a tuple of more than 8 elements is not an `Element`.

## Where it holds

Per-row signals and `List` slots do what the card asks, independent of N and of how many rows are mounted: fine select
12.5k at 10 to 10,000 rows (13.2k with all 10,000 mounted), ticker K = 1 13.6k, spinner 5.1k to 5.5k, typing 6.9k to
7.2k, hover 4.4k to 6.7k (8.5k over 100,000 mounted records). Virtual list: mount 20M, hover 4.6k, scroll 33.5k, jump
0.92M, flat from 100 to 1,000,000 rows. Signal writes are linear: 3,360 per write with the row mounted (10 to 10,000
writes in one turn), 180 with it unmounted; one signal with N paint dependents 180 per dependent by wires alone.
Nested scroll boxes scrolling in the same frame are exact. A 100,000-record tree mounts and hovers with raised caps.

## gui-bench scenarios

`table` variants: `fine` = `List<FineRow>` with per-field signals (rows mounted by exposure: 138 records at any N);
`--mounted` = the same with `overscan = rows` (every row mounted); `coarse` = one `Vec<Row>`, re-described;
`--plain` = the rows in a plain box instead of the scroll box. Cells are `R0 / R1 [codes]`.

**rows**

| operation | ideal | N=10 | N=100 | N=1,000 | N=10,000 | exponent R0 (all / last step) | verdict |
|---|---|---:|---:|---:|---:|---|---|
| hover | O(1) | 4.4k / 52.7k | 4.5k / 52.8k | 4.9k / 51.3k | 6.7k / 55.4k [mG]† | 0.06 / 0.13 | ok; mG |
| scroll | O(visible) |  | 11.9k / 60.0k [H] | 20.2k / 71.4k [H] | 92.8k / 143k [mG,H]† | 0.45 / 0.66 | OVERFIT; H,mG |

**table coarse**

| operation | ideal | N=10 | N=100 | N=1,000 | N=5,000 | N=10,000 | exponent R0 (all / last step) | verdict |
|---|---|---:|---:|---:|---:|---:|---|---|
| select | O(1) | 8.8k / 9.1k [S] | 8.9k / 9.2k [S] | 9.7k / 10.0k [S] | 9.7k / 10.1k [S] | 9.8k / 10.1k [mG,S]† | 0.02 / 0.01 | WRONG PIXELS (stale; cost not meaningful); mG |
| update10 | O(N/10) | 5.2k / 5.3k [S] | 12.6k / 12.7k [S] | 86.3k / 86.5k [S] | 419k / 415k [S] | 835k / 835k [mG,S]† | 0.76 / 0.99 | WRONG PIXELS (stale; cost not meaningful); mG |
| swap | O(1) | 4.4k / 4.6k [S] | 4.4k / 4.6k [S] | 4.9k / 5.0k [S] | 4.9k / 5.1k [S] | 4.9k / 5.1k [mG,S]† | 0.02 / 0.01 | WRONG PIXELS (stale; cost not meaningful); mG |
| remove | O(1) | 20.6k / 76.4k [S] | 21.7k / 69.4k [S] | 29.1k / 77.6k [S] | 47.3k / 93.7k [S] | 97.2k / 155k [mG,S]† | 0.20 / 1.04 | WRONG PIXELS (stale; cost not meaningful); mG |
| insert | O(1) | 20.6k / 72.2k [S] | 10.0k / 60.7k [S] | 17.2k / 73.9k [S] | 47.3k / 94.5k [S] | 88.9k / 135k [mG,S]† | 0.22 / 0.91 | WRONG PIXELS (stale; cost not meaningful); mG |
| replace | O(N) | 22.3k / 22.4k [S] | 174k / 175k [S] | 1.70M / 1.71M [S] | 8.52M / 8.55M [S] | 17.1M / 17.1M [mG,S]† | 0.97 / 1.00 | WRONG PIXELS (stale; cost not meaningful); mG |
| append | O(1,000) | 872k / 951k [S] | 873k / 926k | 871k / 911k | 863k / 910k | 875k / 935k [mG]† | -0.00 / 0.02 | WRONG PIXELS (stale; cost not meaningful); mG |
| truncate | O(1,000) | 876k / 935k [S] | 873k / 923k | 862k / 913k | 861k / 914k | 874k / 933k [mG]† | -0.00 / 0.02 | WRONG PIXELS (stale; cost not meaningful); mG |
| clear | O(N) | 28.9k / 77.8k [S] | 105k / 153k [S] | 871k / 935k [S] | 4.28M / 4.34M [S] | 8.60M / 8.66M [mG,S]† | 0.84 / 1.01 | WRONG PIXELS (stale; cost not meaningful); mG |
| replace-select | O(N) | 20.0k / 20.3k [R] | 95.9k / 96.8k [R] | 861k / 862k [R] | 4.27M / 4.27M [R] | 8.55M / 8.59M [mG,R]† | 0.89 / 1.00 | ok; R,mG |

**table coarse --plain**

| operation | ideal | N=10 | N=100 | N=1,000 | N=5,000 | N=10,000 | exponent R0 (all / last step) | verdict |
|---|---|---:|---:|---:|---:|---:|---|---|
| select | O(1) | 76.6k / 126k | 666k / 726k | 6.56M / 6.64M | 32.8M / 33.0M | 65.7M / 66.0M [mG]† | 0.98 / 1.00 | OVERFIT; mG |
| update10 | O(N/10) | 53.2k / 106k | 471k / 532k | 4.68M / 4.74M | 23.4M / 23.4M | 46.7M / 47.0M [mG]† | 0.98 / 1.00 | ok; mG |
| swap | O(1) | 70.8k / 120k | 365k / 415k | 3.31M / 3.37M | 16.4M / 16.5M | 33.0M / 33.0M [mG]† | 0.90 / 1.01 | OVERFIT; mG |
| remove | O(1) | 220k / 276k | 2.32M / 2.37M | 23.5M / 23.5M | 116.8M / 116.8M | 234.6M / 234.4M [mG]† | 1.01 / 1.01 | OVERFIT; mG |
| insert | O(1) | 240k / 295k | 2.33M / 2.39M | 23.4M / 23.5M | 116.7M / 117.0M | 234.1M / 234.1M [mG]† | 1.00 / 1.00 | OVERFIT; mG |
| replace | O(N) | 228k / 281k | 2.09M / 2.14M | 21.1M / 21.1M | [G,rG] | [mG,G,rG]† | 0.98 / 1.00 | ok; G,rG,mG |
| append | O(1,000) | 21.3M / 21.4M | 21.8M / 21.8M | 28.1M / 28.6M | 57.9M / 58.4M | 95.3M / 95.5Mⁱ [mG,rI]† | 0.20 / 0.72 | OVERFIT; mG,rI |
| truncate | O(1,000) | 21.3M / 21.6M | 22.0M / 22.0M | 28.5M / 28.3M | 58.3M / 58.5M | 95.3M / 95.5Mⁱ [mG,rI]† | 0.21 / 0.71 | OVERFIT; mG,rI |
| clear | O(N) | 252k / 302k | 2.13M / 2.11M | 20.3M / 20.6M | [rI] | [mG,I,rI]† | 0.95 / 0.98 | ok; rI,mG,I |
| replace-select | O(N) | 193k / 241k [R] | 1.73M / 1.81M [R] | 17.1M / 17.2M [R] | [R,rG] | [mG,R,rG]† | 0.97 / 1.00 | ok; R,rG,mG |

**table fine**

| operation | ideal | N=10 | N=100 | N=1,000 | N=5,000 | N=10,000 | exponent R0 (all / last step) | verdict |
|---|---|---:|---:|---:|---:|---:|---|---|
| select | O(1) | 11.7k / 59.3k | 11.8k / 59.2k | 12.5k / 59.6k | 12.5k / 60.8k | 12.5k / 60.3k | 0.01 / 0.00 | ok |
| update10 | O(N/10) | 21.2k / 70.6k | 79.3k / 132k | 169k / 217k | 570k / 627k | 1.08M / 1.12M | 0.54 / 0.92 | ok |
| swap | O(1) | 29.6k / 81.1k | 53.7k / 112k | 72.8k / 125k | 156k / 224k | 260k / 318k | 0.29 / 0.74 | OVERFIT |
| remove | O(1) | 60.5k / 110k | 83.0k / 138k | 102k / 153k | 164k / 222k | 283k / 337k | 0.20 / 0.79 | OVERFIT |
| insert | O(1) | 61.1k / 122k | 63.3k / 130k | 82.4k / 137k | 163k / 216k | 263k / 319k | 0.20 / 0.69 | OVERFIT |
| replace | O(N) | 364k / 425k | 1.08M / 1.11M | 2.91M / 2.94M | 11.0M / 11.1M | 21.1M / 21.1M | 0.57 / 0.94 | ok |
| append | O(1,000) | 1.32M / 1.39M | 1.05M / 1.11M | 1.03M / 1.08M | 1.03M / 1.08M | 1.05M / 1.10M | -0.03 / 0.02 | ok |
| truncate | O(1,000) | 1.31M / 1.38M | 1.05M / 1.10M | 1.03M / 1.09M | 1.03M / 1.08M | 1.05M / 1.11M | -0.03 / 0.02 | ok |
| clear | O(N) | 220k / 270k | 580k / 626k | 1.49M / 1.54M | 5.52M / 5.60M | 10.6M / 10.6M | 0.55 / 0.94 | ok |
| replace-select | O(N) | 193k / 258k | 560k / 613k | 1.46M / 1.51M | 5.51M / 5.56M | 10.6M / 10.7M | 0.57 / 0.94 | ok |

**table fine --mounted**

| operation | ideal | N=10 | N=100 | N=1,000 | N=5,000 | N=10,000 | exponent R0 (all / last step) | verdict |
|---|---|---:|---:|---:|---:|---:|---|---|
| select | O(1) | 11.8k / 58.4k | 11.8k / 61.2k | 12.7k / 60.7k | 13.0k / 65.6k [mSP]† | 13.2k / 63.4k [mSP]† | 0.02 / 0.03 | ok; mSP |
| update10 | O(N/10) | 21.2k / 72.8k | 223k / 272k | 2.19M / 2.24M | 10.9M / 11.0M [mSP]† | 22.0M / 22.1M [mSP]† | 1.00 / 1.01 | ok; mSP |
| swap | O(1) | 29.7k / 79.5k | 185k / 244k | 1.71M / 1.77M | 8.07M / 8.12M [mSP,D]† | 16.0M / 16.1M [mSP,D]† | 0.92 / 0.98 | OVERFIT; mSP,D |
| remove | O(1) | 60.5k / 114k | 79.3k / 137k | 251k / 341k | 999k / 1.07M [mSP]† | 1.97M / 2.06M [mSP]† | 0.51 / 0.98 | OVERFIT; mSP |
| insert | O(1) | 60.8k / 111k | 60.2k / 112k | 232k / 307k | 997k / 1.06M [mSP]† | 1.98M / 2.03M [mSP]† | 0.52 / 0.99 | OVERFIT; mSP |
| replace | O(N) | 366k / 420k | 3.44M / 3.45M | [G,rG] | [mSP,G,rG]† | [mSP,G,rG]† | 0.97 / 0.97 | ok; G,rG,mSP |
| append | O(1,000) | 18.0M / 18.0Mⁱ [rG] | 18.0M / 18.1Mⁱ [rG] | 17.6M / 17.7Mⁱ [rG] | [mSP,G,rG]† | 18.7M / 18.8Mⁱ [mSP,G,rG]† | 0.00 / 0.03 | ok; rG,mSP,G |
| truncate | O(1,000) | 18.0M / 18.1Mⁱ [rG] | 18.1M / 18.0Mⁱ [rG] | 17.6M / 17.7Mⁱ [rG] | [mSP,G,rG]† | 18.8M / 18.8Mⁱ [mSP,G,rG]† | 0.00 / 0.03 | ok; rG,mSP,G |
| clear | O(N) | 212k / 272k | 1.80M / 1.79M | 17.9M / 18.0Mⁱ [rG] | [mSP,G,rG]† | [mSP,G,rG]† | 0.96 / 1.00 | ok; rG,mSP,G |
| replace-select | O(N) | 204k / 246k | 1.78M / 1.79M | 17.9M / 18.0Mⁱ [rG] | [mSP,G,rG]† | [mSP,G,rG]† | 0.97 / 1.00 | ok; rG,mSP,G |

**table fine --mounted --plain**

| operation | ideal | N=10 | N=100 | N=1,000 | N=5,000 | N=10,000 | exponent R0 (all / last step) | verdict |
|---|---|---:|---:|---:|---:|---:|---|---|
| select | O(1) | 12.3k / 72.1k | 12.8k / 60.6k | 18.6k / 67.5k | 40.8k / 101k [mSP]† | 68.6k / 119k [mSP]† | 0.24 / 0.75 | OVERFIT; mSP |
| update10 | O(N/10) | 21.5k / 76.3k | 223k / 291k | 2.19M / 2.24M | 11.0M / 11.1M [mSP]† | 22.0M / 22.2M [mSP]† | 1.00 / 1.01 | ok; mSP |
| swap | O(1) | 24.1k / 78.3k | 126k / 178k | 1.10M / 1.17M | 5.27M / 5.34M [mSP,D]† | 10.5M / 10.6M [mSP,D]† | 0.89 / 0.99 | OVERFIT; mSP,D |
| remove | O(1) | 64.6k / 121k | 89.7k / 137k | 264k / 328k | 1.03M / 1.10M [mSP]† | 2.01M / 2.07M [mSP]† | 0.50 / 0.97 | OVERFIT; mSP |
| insert | O(1) | 65.4k / 128k | 76.0k / 126k | 252k / 358k | 1.03M / 1.10M [mSP]† | 2.00M / 2.06M [mSP]† | 0.51 / 0.96 | OVERFIT; mSP |
| replace | O(N) | 355k / 430k | 3.51M / 3.35M | [G,rG] | [mSP,G,rG]† | [mSP,G,rG]† | 1.00 / 1.00 | ok; G,rG,mSP |
| append | O(1,000) | [rG] | [rG] | [rG] | [mSP,G,rG]† | [mSP,G,rG]† | - | not measurable; rG,mSP,G |
| truncate | O(1,000) | [rG] | [rG] | [rG] | [mSP,G,rG]† | [mSP,G,rG]† | - | not measurable; rG,mSP,G |
| clear | O(N) | 221k / 270k | 1.78M / 1.77M | [rG] | [mSP,G,rG]† | [mSP,G,rG]† | 0.90 / 0.90 | ok; rG,mSP,G |
| replace-select | O(N) | 200k / 249k | 1.75M / 1.75M | [rG] | [mSP,G,rG]† | [mSP,G,rG]† | 0.94 / 0.94 | ok; rG,mSP,G |

**table fine --mutant**

| operation | ideal | N=10 | N=100 | N=1,000 | N=10,000 | exponent R0 (all / last step) | verdict |
|---|---|---:|---:|---:|---:|---|---|
| select | O(1) | 11.9k / 70.0k | 13.1k / 61.0k | 25.5k / 73.2k | 143k / 191k | 0.35 / 0.75 | OVERFIT |

**table fine --plain**

| operation | ideal | N=10 | N=100 | N=1,000 | N=5,000 | N=10,000 | exponent R0 (all / last step) | verdict |
|---|---|---:|---:|---:|---:|---:|---|---|
| select | O(1) | 12.3k / 65.9k | 12.4k / 59.6k | 13.1k / 61.8k | 13.1k / 59.9k | 13.1k / 63.1k | 0.01 / 0.00 | ok |
| update10 | O(N/10) | 21.5k / 72.4k | 79.6k / 129k | 169k / 221k | 569k / 621k | 1.08M / 1.12M | 0.54 / 0.92 | ok |
| swap | O(1) | 24.2k / 80.5k | 55.1k / 107k | 74.5k / 124k | 158k / 211k | 263k / 315k | 0.32 / 0.74 | OVERFIT |
| remove | O(1) | 64.6k / 114k | 96.5k / 147k | 112k / 167k | 181k / 248k | 297k / 358k | 0.19 / 0.72 | OVERFIT |
| insert | O(1) | 65.4k / 120k | 81.1k / 132k | 101k / 158k | 181k / 235k | 281k / 346k | 0.20 / 0.63 | OVERFIT |
| replace | O(N) | 363k / 413k | 1.08M / 1.09M | 2.89M / 2.91M | 11.0M / 11.0M | 21.1M / 21.2M | 0.57 / 0.95 | ok |
| append | O(1,000) | 1.33M / 1.39M | 1.06M / 1.11M | 1.05M / 1.11M | 1.05M / 1.12M | 1.06M / 1.11M | -0.03 / 0.01 | ok |
| truncate | O(1,000) | 1.34M / 1.39M | 1.06M / 1.11M | 1.05M / 1.10M | 1.05M / 1.10M | 1.07M / 1.11M | -0.03 / 0.03 | ok |
| clear | O(N) | 225k / 264k | 570k / 622k | 1.48M / 1.53M | 5.54M / 5.59M | 10.6M / 10.7M | 0.55 / 0.93 | ok |
| replace-select | O(N) | 197k / 242k | 542k / 605k | 1.45M / 1.50M | 5.51M / 5.56M | 10.6M / 10.6M | 0.56 / 0.94 | ok |

**ticker coarse --updates 1**

| operation | ideal | N=10 | N=100 | N=1,000 | N=10,000 | exponent R0 (all / last step) | verdict |
|---|---|---:|---:|---:|---:|---|---|
| ticker | O(K) | 9.4k / 55.1k [S] | 9.8k / 56.3k [S] | 9.8k / 61.0k [S] | 9.9k / 64.9k [mG,S]† | 0.01 / 0.00 | WRONG PIXELS (stale; cost not meaningful); mG |

**ticker coarse --updates 1 --plain**

| operation | ideal | N=10 | N=100 | N=1,000 | N=10,000 | exponent R0 (all / last step) | verdict |
|---|---|---:|---:|---:|---:|---|---|
| ticker | O(K) | 45.2k / 94.4k | 341k / 392k | 3.29M / 3.34M | 32.8M / 33.0M [mG]† | 0.96 / 1.00 | OVERFIT; mG |

**ticker coarse --updates 100**

| operation | ideal | N=10 | N=100 | N=1,000 | N=10,000 | exponent R0 (all / last step) | verdict |
|---|---|---:|---:|---:|---:|---|---|
| ticker | O(K) | 9.6k / 55.6k [S] | 11.7k / 61.5k [S] | 11.5k / 60.7k [S] | 11.5k / 60.8k [mG,S]† | -0.00 / 0.00 | WRONG PIXELS (stale; cost not meaningful); mG |

**ticker coarse --updates 100 --plain**

| operation | ideal | N=10 | N=100 | N=1,000 | N=10,000 | exponent R0 (all / last step) | verdict |
|---|---|---:|---:|---:|---:|---|---|
| ticker | O(K) | 71.7k / 120k | 634k / 695k | 3.59M / 3.66M | 33.1M / 33.3M [mG]† | 0.86 / 0.97 | OVERFIT; mG |

**ticker fine --updates 1**

| operation | ideal | N=10 | N=100 | N=1,000 | N=10,000 | exponent R0 (all / last step) | verdict |
|---|---|---:|---:|---:|---:|---|---|
| ticker | O(K) | 13.2k / 63.5k | 13.6k / 63.9k | 13.6k / 64.3k | 13.6k / 63.8k | 0.00 / 0.00 | ok |

**ticker fine --updates 1 --mounted**

| operation | ideal | N=10 | N=100 | N=1,000 | N=10,000 | exponent R0 (all / last step) | verdict |
|---|---|---:|---:|---:|---:|---|---|
| ticker | O(K) | 13.3k / 59.4k | 13.6k / 60.7k | 13.7k / 59.6k | 13.8k / 64.4k [mSP]† | 0.01 / 0.00 | ok; mSP |

**ticker fine --updates 100**

| operation | ideal | N=10 | N=100 | N=1,000 | N=10,000 | exponent R0 (all / last step) | verdict |
|---|---|---:|---:|---:|---:|---|---|
| ticker | O(K) | 45.2k / 94.0k | 121k / 169k | 44.7k / 95.3k | 33.8k / 81.1k | -0.28 / -0.12 | ok |

**ticker fine --updates 100 --mounted**

| operation | ideal | N=10 | N=100 | N=1,000 | N=10,000 | exponent R0 (all / last step) | verdict |
|---|---|---:|---:|---:|---:|---|---|
| ticker | O(K) | 45.2k / 94.9k | 378k / 430k | 394k / 446k | 336k / 398k [mSP]† | -0.03 / -0.07 | ok; mSP |

**spinner**

| operation | ideal | N=0 | N=10 | N=100 | N=1,000 | N=10,000 | exponent R0 (all / last step) | verdict |
|---|---|---:|---:|---:|---:|---:|---|---|
| spinner | O(1) | 5.1k / 51.8k | 5.2k / 52.5k | 5.1k / 51.1k | 5.5k / 54.2k | 5.5k / 58.6k [mG]† | 0.01 / 0.00 | ok; mG |

**typing**

| operation | ideal | N=10 | N=100 | N=1,000 | N=10,000 | exponent R0 (all / last step) | verdict |
|---|---|---:|---:|---:|---:|---|---|
| typing | O(1) | 6.9k / 54.3k | 6.9k / 54.5k | 7.2k / 56.2k | 7.2k / 62.1k [mG]† | 0.01 / 0.00 | ok; mG |

**search**

| operation | ideal | N=10 | N=100 | N=1,000 | N=10,000 | exponent R0 (all / last step) | verdict |
|---|---|---:|---:|---:|---:|---|---|
| filter | O(N shown/hidden) | 6.9k / 53.2k [S] | 6.9k / 53.9k [S] | 7.2k / 55.8k [S] | 7.3k / 59.5k [mG,S]† | 0.01 / 0.00 | WRONG PIXELS (stale; cost not meaningful); mG |
| filter-nochange | O(1) + query scan | 6.9k / 54.0k [S] | 6.9k / 53.5k [S] | 7.2k / 54.0k [S] | 7.3k / 57.1k [mG,S]† | 0.01 / 0.00 | WRONG PIXELS (stale; cost not meaningful); mG |

**search --plain**

| operation | ideal | N=10 | N=100 | N=1,000 | N=10,000 | exponent R0 (all / last step) | verdict |
|---|---|---:|---:|---:|---:|---|---|
| filter | O(N shown/hidden) | 231k / 277k | 1.98M / 2.04M | 19.8M / 19.8M | [I,rI]† | 0.97 / 1.00 | ok; I,rI |
| filter-nochange | O(1) + query scan | 15.2k / 65.3k | 85.8k / 137k | 794k / 848k | 7.89M / 7.94M† | 0.91 / 1.00 | OVERFIT |

**virtual**

| operation | ideal | N=10 | N=100 | N=1,000 | N=10,000 | N=100,000 | N=1,000,000 | exponent R0 (all / last step) | verdict |
|---|---|---:|---:|---:|---:|---:|---:|---|---|
| mount | O(visible) | 19.7M | 20.2M | 20.3M | 20.2M | 20.6M | 23.8M | 0.01 / 0.06 | ok |
| hover | O(1) | 4.5k / 50.6k | 4.6k / 52.9k | 4.6k / 54.1k | 4.6k / 57.1k | 4.6k / 53.5k | 4.6k / 62.4k | 0.00 / 0.00 | ok |
| scroll | O(visible) | 3.5k / 3.7k | 33.2k / 83.8k [H] | 33.6k / 83.8k [H] | 33.4k / 82.8k [H] | 33.6k / 84.1k [H] | 33.7k / 81.9k [H] | 0.00 / 0.00 | ok; H |
| jump | O(visible) | 3.5k / 3.7k | 923k / 954k [H] | 923k / 977k [H] | 923k / 952k [H] | 922k / 980k [H] | 938k / 955k [H] | 0.00 / 0.01 | ok; H |

**resize**

| operation | ideal | N=10 | N=100 | N=1,000 | N=10,000 | exponent R0 (all / last step) | verdict |
|---|---|---:|---:|---:|---:|---|---|
| resize | O(visible) | 171k / 3.63M | 1.61M / 5.07M | 16.1M / 19.5M | 152.0M / 155.7M [mG]† | 0.98 / 0.98 | OVERFIT; mG |

Notes on the tables. Coarse append / truncate pass their gates at N >= 100 only because the changed rows are below
the viewport. `filter-nochange` in a plain box is the port's own `spec::matches` scan plus the re-description
(790 per row), not a list cost. Fine `update10` at 10,000 rows is 1,000 label writes on unmounted rows (1.08k each);
mounted it is 21.9k per label (re-shape and row layout). Fine replace / clear are ~2.1k per row for rows that are never
mounted (slot and signal construction in the port plus the list log).

## Per-row cost of mounting (R0, default caps, finite difference 1,000 -> 4,000 rows)

| view | instructions per row | resident bytes per row | footprint bytes per row |
|---|---:|---:|---:|
| rows (scroll box, 5 records per row) | 39,950 | 7,570 | 5,270 |
| table coarse, plain box | 39,940 | 7,220 | 5,100 |
| table fine, all rows mounted | 34,760 | 8,090 | 5,590 |
| table fine, exposure (138 records) | 74 | 295 | 300 |
| virtual (`index_list`) | 127 | ~0 | ~0 |

Memory is a level difference read once per size under load, not a locked measurement. 20,000 rows (100,002 records,
raised caps): mount 833M instructions, 200 MB resident.

## Adversarial matrix

Predictions were written in `adv.py` before the runs. "exact" = presented == full, retained == fresh and the case's
independent pixel probes all pass.

| case | prediction | result |
|---|---|---|
| window 4,128 x 800; 6,016 x 3,384; 1,280 x 4,128 | far edge not repaired, or refused | refused: `window up to 4096x4096` (finding 6). 4,096 wide and 4,096 high exact. |
| content 40,000 / 70,000 / 16.78M / 40M px in a scroll box | placement saturates | wrong pixels from 32,767 px (finding 3). 20,000 px exact. |
| 1,030 to 2,100 real rows scrolled to the end | same | wrong pixels (finding 3); 1,020 rows exact. |
| virtual list, 100,000 to 4,000,000 rows, to the end | precision lost past 2^24 px | pixels exact at every size; 1 pt steps lost from 600,000 rows (finding 15). |
| 100,002 mounted records (20,000 rows) | default caps overflow | `glyph_x` full at mount. Raised (glyphs 2,097,152, instances and runs 262,144, spaces 32,768; glyphs alone is enough to mount): hover 8.5k / 57.5k, scroll 180k / 233k, gates pass (pointer re-delivered). |
| unequal-height wrapping rows, one grows by lines | O(mounted rows) | exposure list: 57k flat to 10,000 rows, exact. All mounted: 61k at 100, 221k at 1,000 (160 per row below), exact; 4,000 mounted: `glyph_x` full at mount. Plain Vec: 693k at 100, 6.46M at 1,000 (finding 2). |
| two nested scroll boxes, both wheeled before one frame | repair fails | exact: inner 8.2k, outer 10.5k, both 17.1k. No scroll chaining at the inner end. |
| 200-level tree, static description | depth field or recursion fails | exact at 10 to 200 levels; cost quadratic (finding 12). |
| 200-level tree of list rows owning lists | spaces / clips exhausted | mounts; retained != fresh from 20 levels (finding 14). |
| 10,000 signals written in one turn, rows mounted | linear, small constant; spaces overflow | default caps: `space column full` at 10,000 (4,000 mounted rows run: 13.3M for 4,000 writes). Raised: 6.8k / 35.6k / 323k / 3.39M / 33.6M for 1 / 10 / 100 / 1,000 / 10,000 writes (3,360 per write, exp 1.0), exact. Unmounted rows: 1.81M for 10,000 writes. |
| one signal, N = 10 to 10,000 dependents | O(N) | text dependents 4.80M at 1,000, 47.8M at 10,000 (3.0k per dependent by wires alone, 4.8k through a handler); paint dependents 1.98M / 19.5M (180 by wires alone). Exact. |
| owner with 10,000 children, one plain field changed | O(N) walk | 17.8M; a handler that changes nothing 17.7M (finding 2). |
| CJK, Arabic, emoji, combining marks | no fallback; gates blind | finding 11. |
| front insert x 1,000; random moves x 1,000 | per-op cost grows with N | finding 9. Exposure list exact after 1,000 of each at 10 to 100,000 rows; mounted exact at 10 and 1,000. Plain Vec front insert: 388k per insert at 10 rows, 6.02M at 1,000 (finding 2), exact. |
| scale 1 -> 2 -> 1 | no entry point | not runnable live; scale 2 as a fresh configuration exact. |
| list in a scroll box at the window origin (found while building the matrix) | - | no rows mounted (finding 7). |

## Fixes made so the suite runs

None in the framework. In the crate: the driver's layout points and fresh-mount pointer replay now scale with the
configuration's scale (they assumed scale 1); adversarial list views carry an 8 pt block above their scroll box
because of finding 7.

## Reproduce

```sh
cargo build --release --manifest-path comparisons/bough-guibench/Cargo.toml --bins          # add --features deep for the static deep tree
python3 comparisons/bough-guibench/sweep.py --binary $T/release/headless --out OUT --batch control|table|rest
python3 comparisons/bough-guibench/adv.py --adversarial $T/release/adversarial --headless $T/release/headless --out OUT
python3 comparisons/bough-guibench/report.py RUNTIME_DIR                                     # the tables above
$T/release/headless --scenario table --variant coarse --rows 1000 --plain --op select --repeats 8 --blocks 3
GUIBENCH_REHOVER=same $T/release/headless --scenario rows --rows 100 --op scroll --gate --repeats 4     # finding 8
GUIBENCH_IDLE=1 $T/release/headless --scenario table --variant fine --rows 10 --mounted --op append --repeats 8 --blocks 3   # finding 4b
$T/release/adversarial --case tall-rows --op end --n 1030        # finding 3
$T/release/adversarial --case listbox --op x --n 100 --k 1       # finding 7 (k = 5: with a header)
$T/release/adversarial --case row-interact --op x                # finding 5
```

Earlier files in this directory (`HANDOFF.md`, `TABLE-CELLS.*`, `MEASUREMENTS.csv`, the cards) are the previous
session's record; this file supersedes their tables. Its three headline cells reproduce here: coarse plain select
6.56M at 1,000 and 32.8M at 5,000, fine select 12.5k.
