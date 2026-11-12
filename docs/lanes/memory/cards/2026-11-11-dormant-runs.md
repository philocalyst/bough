# Card: dormant runs. Mounting driven by exposure for every dynamic part (2026-11-11, lane `memory`)

State: FROZEN before building. Branch `lane/memory/dormant` (from `lead/core-r6` + `lane/surface-api/round6`
cherry-picked, because a scroll container must re-describe its content for any of this to work).

## Hard cases, decided before coding

| case | decision | general mechanism it is a case of |
|---|---|---|
| a source that cannot give item `i` in O(1) (`filter`, `rev`, `Vec<E>` of non-`Copy` items, any hand-written iterator) | eager, as today; the inspector says so | "indexable" is a property of the iterator's type: std's `TrustedRandomAccessNoCoerce` (`Range`, `slice::Iter`, `Map`, `Enumerate`, `Zip`, `Cloned`, `Copied`, `Skip`), chosen by specialisation. `Map::nth` is **not** O(1) (it must call the closure for skipped items), so `iter.clone().nth(i)` is not used |
| nested parts (a dormant run inside a realised item inside a dormant run) | supported: a part is dormant-capable wherever its owner is the root and re-describes; an inner part lives and dies with the outer item's scope | a part is registered while its scope lives (`cut_scope` unregisters it) |
| parts inside list rows / entity views / `Config::once()` | eager | realisation re-runs `view()` of the root owner; other owners have no walk to the part yet (lane `surface-api`'s owners-as-blocks removes the limit) |
| row containers, wrapping, grids, `justify` other than start, a container with a definite main size, items that grow, absolute items | eager (decided at mount from the container's style and at each realisation from the item's; a violation found later realises the whole part and marks it eager for good) | a run stands for its items only if the container's result depends on them through *the sum of their outer main sizes*: true for a start-justified, content-sized column |
| items of wildly different sizes | extent model per part: estimate (mean of measured pitches, moved only when it is off by more than 1/8) + remembered measured pitches of released items (sparse, by index); anchoring keeps the visible item still while extents are corrected | CSS `contain-intrinsic-size: auto` + scroll anchoring |
| a clipped ancestor | exposure = the run's box ∩ `visible(clip)` (one function shared with `listview`); an empty clip exposes nothing | the clip chain is already composed in bough-core's clip rects |
| a transformed ancestor | the part's own space affine is inverted on the visible rect (bounding box, conservative: realises a superset); deeper transform chains: not verified | exposure is computed in the part's local frame |
| two scroll views over one part | cannot be described (a part has one place in the tree); two parts over one source are independent | none needed |
| an item realised because it holds focus / hover / capture, scrolled away | stays realised while within the i16 ink domain (±12,000 px of the visible rect); beyond it is released (the hold breaks). | OVERFIT finding 3 (absolute i16 inks) is core's; this is its visible edge |
| the count changes while a run is dormant | realised items are positional (index `i` shows `source[i]`, updated through their seals and the shift detection); dormant ranges only track count; any difference in length is applied at the tail. With an index-only source an insertion above the window is therefore "every realised item changed by one", which the shift detection turns into one removal and one realisation | positional identity is all an index source has; keyed deltas (`List<T>`) are the next card |
| f32 | cephal lays out in f32: positions above 2^24 device px lose integer precision (1 M rows of 20 px is 20 M px: row tops stay exact only when the pitch is a multiple of 4). Reported, not fixed here | layout in content-local frames (core) |

## Change
`each(iter)`: indexable source + qualifying container => seed items + one dormant run (paint-less `REC_BOX` + a
layout leaf sized by the run's extent, through a measured-leaf side value, not an interned style). After every
layout flush: scan parts (early-out on an unchanged (visible rect, space origin, layout generation)), turn exposed
run ranges (± overscan px) into demands, re-describe the root once (dormant ranges are skipped in the source),
lay out, anchor, repeat to a fixed point (bound 8). Release: items outside the keep margin, not held; cut now,
reclaimed by the frames that release (not only idle ones).

## Gates and thresholds
1. Oracle: presented == full redraw of retained == full redraw of a fresh `App` (same state, same offset) ==
   full redraw of a fresh `App` with `Config::eager_parts` (today's path), rings 1..4, random scroll / resize /
   edit sequences. Variable sizes: compared at the same anchor (item, offset in item).
2. O(exposed): 100 / 10,000 / 1,000,000 items: records, mount instructions, scroll-frame instructions within 5 %.
   1 M mounts at default caps.
3. Mutants (`BOUGH_DORM_MUTANT=short|norelease|noanchor`) fail the pixel gate / the record bound / the anchor test.
4. No tax: `redescribe_cost 10` per-row numbers within 1.02x of the base; suites pass.
5. gui-bench `rows` 10,000 mounts at default caps and passes both pixel gates.

Negative control: the three mutants. Stop: a gate that cannot be made exact is reported as failing, not loosened.
