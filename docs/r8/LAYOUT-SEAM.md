# The layout seams (round 8): proposal v0 from the layout lane

Status: **v0 as agreed on 2026-11-12, with section 0 added at the end of the round: two of its claims were
measured and refuted. Section 0 wins where it differs from the sections after it.**

Original status line: proposal, 2026-11-12, not agreed. Answers the expr lane's five points and two questions, the lead's
two pushes (changed rows only; exact extents with wrapped text), ROUND8 Amendment 2 (the author keeps the
façade's builder calls), and proposes the boundary with the text lane. Nothing is built on it yet.

## 0. Where this ended (2026-11-12, measured; `docs/lanes/layout/JOURNAL.md` section 4)

Two claims of sections 1 and 2 are **refuted**:

* "A monomorphised `place` for a row of fixed and flex children is tens of instructions, cheaper than
  computing and comparing a key": it is 25,486 instructions for the sheet row against 203 to copy a kept
  slice, and one row type cost up to 204 s and 2.7 MB to compile. "There is no shape memo" is withdrawn.
* "The answer is not an owned `&[Rect]`": for rows whose layout does not read row data it should be exactly
  that. **The expr lane's slice compared by identity stays for `Hk::Fixed` row types.**

The design the numbers support, and what `crates/bough-lay` holds:

1. An element built by the façade's calls emits its layout as a flat array of 24-word elements (`Shape`,
   `Nd`): a few stores per element, 0.2 to 0.5 s and 3.5 to 17 KB of compile cost per row type.
2. One layout function for every row type (`shape::place`): the flex arithmetic instantiated once over that
   array, checked against `vendor/cephal`. It keeps nothing per row.
3. A list keeps the rects of the last shape it laid out, with the interval of widths they hold over
   (`Memo`). Same shape, width inside the interval: a copy.
4. The row type says how its height comes about (`Shape::HK`: `Fixed`, `Content`, `Wraps`), which picks the
   extent (`Uniform`, or `Heights`) as in section 4.

Open, stated plainly: the shape of a row is a hash of its words and the compiler does not fold it (2,677
instructions for the sheet row), so checking the shape per row costs more than the copy; a `Fixed` list checks
it once per pass, which is unsound if the author varies a value per row. And the layout function is no faster
than cephal with a kept tree (52,730 against 21,600 for the sheet row; 91,359 against 87,079 for a chat row);
what it buys is no retained tree per row.

The seam with the expr lane as it stands after this: `Out::put(k, [x0, y0, x1, y1])` into the lanes on a miss;
`Memo::kept()` (a slice and a height) on a hit; `Valid` per list for `Fixed` row types and per exposed row
otherwise; `Extent` with 64-bit logical coordinates and `window(y, h)` giving a viewport-local frame
(`Uniform::new` refuses a height that is not positive, which is where the divide by zero in list exposure is
to be stopped).

Checked before writing (not assumed):

* `const trait` / `const impl` / `[const]` bounds compile and evaluate on the pinned nightly-2026-10-17: one
  generic `place` over tuples and arrays was evaluated both in a `static` initialiser and at run time from the
  same source (scratch test, not in the tree). So a layout function can be one body for the compiler and the
  program. It turns out not to be the main path (section 1).
* The façade's builder (`crates/bough/src/layout.rs`): `LBox<Bg, Ch, X>` carries the children in its type
  (`Ch` is a tuple, `(Ch, E)` after `.child`, or `Each<I>`), and every style value in one plain `Spec` field
  that `.w()`, `.h()`, `.gap()`, `.layout(Flex)` mutate. The structure is already a type; the values are not.

## 1. How the builder calls become a shape type and a layout function (Amendment 2)

What the type system can know with the author changing nothing, and what it cannot:

1. **Structure: yes.** `Ch` is the tree. A trait `Lay` implemented for `LBox<_, Ch, _>`, tuples, `Text<S>`,
   `On<E, O>` recurses over it, so `place` for a row is straight-line code for that row's tree, with no style
   tree, no node ids, no cache and no interpreter. Row and container kinds stop being a `Spec` flag
   (`row()` / `column()` / `div()` return different marker types), so no flex run tests its direction.
2. **Which properties are set, and from what: yes, by typestate.** Nobody names an element's type (`impl
   Element`), so builder methods may return a different type without the author seeing it: `.h(22.0)` sets a
   `Fixed` marker for the height, `.grow(1.0)` a `Grow` marker, `.w(sig!(..))` or `.w_op(..)` a `State` marker,
   no call leaves `Auto`. The dependence structure of a layout is then const data of the type: whether the
   outer size depends on content, on available space, on state (`READS`); whether any descendant can shrink or
   wrap. That is what decides, at compile time, how much of the algorithm is instantiated (a row whose
   children are all `Fixed` on the main axis and none `Grow` has no free-space distribution in its code at all).
3. **The values: no.** `.w(110.0)` and `.w(self.width)` have the same type. So the strict `Const` stage
   ("baked into the template at compile time") is **not derivable by the type system under Amendment 2**. What
   happens instead: the row closure builds the element and `place` reads it in one inlined function, so
   literal values are constants to LLVM and fold (the sheet row should compile to stores of immediates: to be
   read in the disassembly, kill criterion in the journal). A layout that is constant in the strict sense is
   reached two ways that need no new author syntax: a `const` recipe the author already writes
   (`const BAR: Flex = ..`, const fn builders), and the build-time first frame (reformulation 3), where the
   same `place` runs in the build step.
4. **Stages, as they exist after this:** *Value* (plain values: folded when literal, per-row when computed
   from row data; the type does not say which), *Turn* (reads a block word; `READS` names it). `Const` vs
   `Mount` is a property of the call site that LLVM sees and the type does not.

Consequence for the design: **there is no shape memo.** Round 7's memo (738 shapes for 39,029 containers)
existed because one flex run cost about 4,900 instructions. A monomorphised `place` for a row of fixed and flex
children is tens of instructions, cheaper than computing and comparing a key, so the memo's job (do not run the
general algorithm per row) is done by not having a general algorithm on the path. What remains worth keeping
per row is only the answer's validity (section 3), which is what makes resize a range check.

What does not fit without a decision from the lead (API, not mine to change silently):

* `each(iter)` *inside* a row (`row(each((0..COLS).map(cell)))`, an editor line's tokens): the count is not in
  the type, and the items are flex items of the enclosing row. Layout handles it (a loop over the item type
  inside the row's flex run). Records need the expr lane's nested list (a row that holds a run of records);
  until that exists the fixtures use arrays with a count.
* An opaque row closure cannot be asked for a row's height without being called (section 4).

## 2. Answer to the expr lane's proposal

| point | answer |
|---|---|
| 1. one rect per record, depth-first order of the row type, i32 device pixels, half open, relative to the row / the space | **Accepted**, with the form of 2 below. `Lay::LEAVES == Row::LEAVES` is a const assert at the list call. |
| 2. an answer is an immutable `&[Rect]`, identity is equality, the memo owns slices | **Countered.** True for a shape shared by all rows; wrong as the general form. Rows whose layout reads row data (an editor line's token widths, a message) have one answer per *row*, so slices would be per row, owned by a memo, for rows that only need them while exposed. The answer is written **straight into the page's rect lanes** by the monomorphised function: `fn place<O: Out>(&self, at, avail, u, out: &mut O) -> Valid`, `Out::put(k, Rect)`, implemented by the sink over its sites. No intermediate slice, no memo, nothing to own or keep alive. (This is also the answer to question b: SoA, because the lanes are the destination.) |
| 3. rects are CPU stage; a changed layout re-lays every exposed row and damages the clip | **Accepted for the stage; countered for the cost**, see section 3. |
| 4. `Extent { top, row_at, total, shape }`, `Uniform` constant-folded, a prefix sum otherwise | **Accepted without `shape`** (placement is `Lay::place`, per row, not the extent's). Added: `set(row, h)`, `insert(row, h)`, `remove(row)`, and the scroll anchor of section 4. One row type per list: accepted. |
| 5. a measured text size is an input of the layout, produced by the text side; expr sees rects and a paint operand | **Accepted**; the boundary is section 5. |
| a. validity interval with the answer? | **Yes**: `place` and `size` return `Valid { lo, hi }` (the half-open interval of available inline size over which the answer is identical). Stored per exposed row as two lanes of the list's ring, and as one pair per list when the row type says rows cannot differ in it. |
| b. SoA lanes? | Yes, as above. |

The contract as code (crate `bough-lay`, depends on `bough-expr`; `Rect` moves to `bough-expr` so neither lane
depends on the other for it):

```rust
pub struct Avail { pub w: f32, pub h: f32 }          // device pixels; NaN = indefinite
pub struct Valid { pub lo: f32, pub hi: f32 }        // avail.w in [lo, hi): same answer
pub trait Out { fn put(&mut self, k: usize, r: Rect); }
pub trait Lay {
    const LEAVES: usize;
    const READS: u64;                                // block words the layout reads (Turn dimensions)
    const ROW_H: Height;                             // Fixed | Content | Wraps: from the typestate, see 4
    /// Outer size only: what an extent needs for a row that is not exposed. No rects.
    fn size(&self, avail: Avail, u: &U) -> (f32, f32, Valid);
    /// Every record's rect, into the lanes.
    fn place<O: Out>(&self, at: (i32, i32), avail: Avail, u: &U, out: &mut O) -> Valid;
}
pub trait Extent {
    fn top(&self, row: u32) -> i32;
    fn row_at(&self, y: i32) -> u32;
    fn total(&self) -> i32;
    fn set(&mut self, row: u32, h: i32);
    fn insert(&mut self, row: u32, h: i32);
    fn remove(&mut self, row: u32);
}
```

`Sink::row` takes the element (`s.row(|| element)`), not `(rects, row)`: the sink calls `place` against the
enclosing space. `Sink::list` takes the extent and the row function; a row value is both `Row` (operands) and
`Lay` (layout), which is what a façade element is.

Arithmetic: `f32` device pixels inside, edges rounded cumulatively to `i32` on output, in cephal's operation
order for the cases types express, so that the differential oracle (the type's style tree given to
`vendor/cephal`, `lay_diff`'s method) can demand equal rects, not near ones. If ties make that unattainable I
report the mismatch rate rather than loosen the test.

## 3. What a resize or an insert costs: the changed rows only

Three classes of exposed row after any layout-relevant change, decided without calling the row function:

* **Holds**: the new available width is inside the row's `[lo, hi)` and its top did not move. Nothing.
  The check is a compare over two lanes of the ring, 64 rows at a time. For a list whose row type cannot
  depend on available width (`ROW_H` and the typestate say so: every editor line, every sheet row at or above
  its content width) it is one compare for the list.
* **Moved**: the answer holds but an earlier row changed height, or a row was inserted or removed above. The
  rects are translated: `y0 += dy; y1 += dy` over the page's lanes, a vector add per page. No row function, no
  operand compare. Damage is the strip `old ∪ new`, plus a hint the raster may use to copy instead of repaint:
  `(rect, dy)`. That hint is the one thing this asks of the raster seam.
* **Changed**: outside its interval. The row function is called for that row, `place` rewrites its lanes, the
  row is damaged.

What the expr side has to change for this to hold end to end:

1. `list_plan` today resets the window when the shape slice changes (`(olo, ohi) = (0, 0)`, clip damaged): that
   becomes the three-way classification above.
2. A fixed row (`Sink::row`) keeps its `Valid` too; a window resize re-places the fixed rows whose interval
   fails and no others.
3. Insert and remove: with `row mod cap` addressing, every exposed row after the insert point changes slot, so
   its operands are rewritten whatever layout does. For an insert to cost one row, the ring needs an order
   lane for the exposed window (slot of the k-th exposed row, at most `cap` bytes): an insert is one row
   written, one small `memmove`, and the translation above. The expr lane's decision; layout's side is ready
   for either.

## 4. Extents: what is exact, what is estimated, and why the scrollbar is stable

The hard fact first: **with the façade's API a row is an opaque closure, and its height cannot be known
without calling it**, which builds the element (the sheet's gutter does `format!("{}", r + 1)`). "Nothing
unexposed exists" and "the extent is exact" therefore conflict for a row type whose height is not provably
uniform. The resolution, in three tiers chosen by the row type's `ROW_H`:

1. **`Fixed`** (the row's block size is set by the author and no descendant can change it: sheet rows, editor
   lines, tree rows, minimap blocks, terminal lines, sidebar rows): the extent is `Uniform`, `n * h`, with `h`
   taken from the first row placed. `top`, `row_at` and `total` are a multiply, a divide and a multiply. The
   author may still pass a different value per row; every row that is placed is checked against `h` (one
   compare), and a row that disagrees demotes the list to tier 3. So it is exact whenever it is true and
   self-correcting when it is not; no row is called to find out.
2. **`Content`, not wrapping** (height from children that cannot wrap): as tier 3, but a row's height holds
   for every width, so it is measured once.
3. **`Wraps`** (a chat message): heights live in a blocked prefix sum (64 rows per block: block sums in one
   array, in-block heights in one line each, so `row_at` is two chained lines), exact for every row that has
   been measured. A row never measured counts as the running mean of the measured ones. Measuring is
   `Lay::size` only (the row function is called, the paragraph's height-for-width is asked of the text lane;
   no records, no rects, no glyph positions), and it runs for all rows in slices off the interaction path
   (the threads lane's epochs); for a list under a row budget it runs inside mount. After the sweep the extent
   is exact. I will report both numbers for the chat: first paint, and extent exact.

**Scrollbar and content stability while estimates are being corrected: the scroll position is
`(row, offset within row)`, not pixels.** The space's origin is derived at settle: `base - (extent.top(row) +
offset)`. A correction above the anchor row then moves nothing on screen and writes nothing to the
application's state; only `total` changes, so the thumb's length drifts by the estimate's error and converges
as the sweep completes. This asks of the expr seam: a list-scrolled space takes its origin from the list's
extent (a Turn-stage value in the space table of SEAM 6.1 item 8), not from a raw block word of pixels.

A resize with wrapped rows: each measured row has `[lo, hi)`; blocks keep `max lo` and `min hi`, so a width
change is one compare per block of 64 rows, and only rows outside their interval are re-measured (exposed
ones re-placed). During a drag in one direction each row is re-measured once per line-count change, not once
per step.

## 5. The boundary with the text lane: what layout asks of a text leaf

A text leaf is a type implementing this; the calls are monomorphised into `place` and `size`:

```rust
pub trait Measure {
    /// The leaf cannot break: `block` ignores the width, and min == max content.
    const WRAPS: bool;
    /// Block words the measured size reads (a numeric label whose digit count follows a state word).
    const READS: u64;
    /// Min-content and max-content inline size, in 26.6 fixed device pixels.
    fn inline(&self, u: &U) -> (i32, i32);
    /// Block size at inline size `w`, the first baseline from the top, and the half-open interval of `w` over
    /// which this answer (and the line structure behind it) is the same. All 26.6.
    fn block(&self, w: i32, u: &U) -> Block;         // { h, baseline, lo, hi }
}
```

* Layout never sees glyphs, runs, fonts or strings. It asks at most these two questions per leaf per answer,
  and stores nothing of the text's except what is in `Valid`.
* A leaf whose size never feeds layout (a number in a box of fixed size: every sheet cell) is never asked:
  the box is the rect, the text lane aligns inside it. The typestate decides this at compile time, which is
  what makes a number write cost no layout at all.
* A one-line label whose width does feed layout (status bar: `Ln 12` then `Col 4` in a row with a gap) is
  `WRAPS = false` with `READS` naming the word: the row's `place` is re-run when that word changes (Turn),
  which is arithmetic.
* What layout hands back to the text lane: the leaf's rect (the content box), through the same rect lanes as
  everything else, and the width it was laid out at. The line structure for that width is the text lane's
  (it produced the interval).
* Asked of the text lane: `inline` and `block` must be callable for a row that is not exposed without creating
  glyph instances (tier 3 above measures 2,000 messages this way), and `block` must be cheap for a width inside
  the last interval (a range check).

## 6. Where the type system cannot carry a layout, and the residue

* Values (section 1, item 3): folded by LLVM, not known to the type.
* Content-sized wrapping text inside flex: the container's size depends on a run-time function of width. The
  residue is the `Measure::block` call and its interval; the flex arithmetic around it stays monomorphised.
* A percentage of an indefinite size, and an aspect ratio: both resolve against a size only known after the
  parent's pass. For one level they are arithmetic in `place` (the parent passes `Avail`); a cycle (a
  percentage child of a content-sized parent) follows CSS (treated as auto) and is the general solver's case.
* Flex wrap (multi-line flex), grids with content-sized tracks, min/max clamps that freeze items in rounds:
  expressible as loops in a monomorphised function but no longer straight-line; these go to `vendor/cephal`
  through the same `Lay` trait (`Solved<T>`: the type's style tree handed to the solver, its rects written
  through `Out`), which is also the oracle path. The fallback is per subtree, not per application.
* Opaque row closures (section 4): the residue is one `i32` per measured row of a `Wraps` list, plus two for
  its interval.
