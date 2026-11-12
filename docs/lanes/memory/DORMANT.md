# Dormant runs: mounting driven by exposure, for every dynamic part

Lane `memory`, round 6, branch `lane/memory/dormant`. Card: `cards/2026-11-11-dormant-runs.md`.
Code: `crates/bough/src/dormant.rs` (the mechanism), `describe.rs` (entries), `lay_deep.rs` (the run's leaf),
`app.rs` (`App::expose`), `input.rs` (focus traversal, hover after a scroll). Gates: `crates/bough/tests/dormant.rs`.
Numbers: `crates/bough/examples/dormant_scale.rs`, `redescribe_cost.rs`, `comparisons/bough-guibench` (copied
from lane `generalization`).

## What it is

`each(source)` keeps its spelling. When the source can give item `i` without the others and the container is one a
run can stand in, the part is retained as **entries** covering its items in order: realised items (exactly what
`describe.rs` kept before) and **dormant runs** (`DynItem::run = n`): one paint-less `REC_BOX` record and one
layout leaf that measures as the run's extent. A dormant item has no description, record, instance, glyph or
layout node. One million rows mount as eight seed items and one run.

* **Source.** `dormant::Source` is implemented for every iterator by specialisation on std's
  `TrustedRandomAccessNoCoerce`: ranges, slice iterators and `map` / `enumerate` / `zip` / `cloned` / `copied` /
  `skip` over them are indexed; `filter`, `rev`, `chain`, a `Vec<E>` of non-`Copy` elements and hand-written
  iterators are not, and mount every item as before. `iter.clone().nth(i)` is **not** used: `Map::nth` runs the
  closure for every skipped item (it is O(i), contrary to the brief's assumption).
* **The two representation functions** (to be re-pointed at a dedicated record kind): `Rt::dorm_leaf` (make a
  run: a handle, a layout node, its `LInfo`) and `dormant::update_src` (replace a sub-range of a run by realised
  items and the reverse). Neither touches `Rt::nodes` or `Run` ids.
* **The leaf's size is a side value** of its layout node (`Deep::set_dormant` / `resize_dormant`, answered by
  `measure`), not a style: an interned style per extent would never be returned.
* **Extent model per part**: an estimate (the mean measured outer size, whole px, moved only when off by more than
  1/8) plus remembered sizes of released items as **run-length groups** (first, count, size); items of the
  estimated size are not stored. A uniform list of any length stores nothing.

## The loop (`App::expose`, after every layout flush of a turn, and at mount)

1. `Rt::dorm_stale` (inlined into the turn): per part, (clip rect ∩ window, space origin, layout generation)
   against what it last scanned, from three columns of `Dorms`; no part state is loaded when nothing changed.
2. `Rt::dorm_scan`: for a part whose key changed, its entry boxes (columns `tops` / `bots`, rebuilt only when the
   layout or the entries changed) are tested against the visible range ± overscan, the keep range and the visible
   range as bit masks; exposed runs become demands (item ranges, mapped through the extent model), items outside
   the keep range for `HOLD` passes and holding no focus, hover or press become releases.
3. One re-description of the root (`describe::redescribe`): `update_src` steps over runs without asking the source,
   splits them where items are demanded (one `insert_run` and one layout pass per range) and collapses released
   items into runs. Realised items are positional and go through their seals and the shift detection.
4. Layout; **anchoring**: the first visible item of a part in a scroll view is kept at its screen position by
   moving the view's offset signal (so a correction above the viewport is a scroll).
5. Repeat until a scan asks for nothing (bound 12; `DormStats::unconverged` stays 0 in every test).

Every structural change happens inside a walk, so an enclosing item's record count is recomputed on the way out
and nested parts need no parent links.

## Rules

* **Qualifies**: root owner, re-describing, flex column, no wrap, start-justified, content-sized main axis, items
  in flow that do not grow, not inside an element a re-description steps over. Checked at mount (container) and
  when an item is first laid out; a violation realises the whole part for good (`DormStats::fell_back`).
* **Realise**: runs meeting the visible rect of the part's clip ± `Dorms::over` (64 pt).
* **Release**: outside ± `Dorms::keep` (256 pt) for `HOLD` (3) exposure passes, not containing the focused,
  hovered or pressed record. A held item stays at any distance (bough-core's exact inks; tested at 720,000 px).
* **Count changes**: realised items are positional; dormant ranges only track count; a difference in length is
  applied at the tail. With an index-only source an insertion above the window is "every realised item changed
  by one", which the shift detection turns into one removal at one end and one realisation at the other.
* **Dormant items** are not hittable and not scanned by focus traversal; `Tab` realises the nearest item of a run
  on its way (`Rt::dorm_run_toward`, `dorm_pin`), and `App::scroll_to_item(part, i, offset)` is the entry point
  for scroll-to-item and find.

See the report at the end of the session's hand-off for the measured cells.
