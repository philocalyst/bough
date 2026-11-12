# The text lane's seams (round 8)

Status: **v0, 2026-11-12.** Part A answers the layout lane's proposal (`wt/r8-layout/docs/r8/LAYOUT-SEAM.md`
section 5): accepted, with five amendments. Part B is the text lane's proposal to the raster lane for the
glyph-instance column, the glyph coverage paint and the atlas. Part C is the seam with the surface lane
(selection and editing for accessibility) and the threads lane. Nothing here is built unless a line says so;
the measurements that decide the open points are in `docs/lanes/text/JOURNAL.md`.

## A. Layout: what layout asks of a text leaf

**Accepted as proposed:** layout never sees glyphs, runs, fonts or strings; it asks `inline` and `block`; the
answer carries the half-open interval of available width over which it holds; a leaf whose size never feeds
layout is never asked, decided by typestate; 26.6 fixed device pixels; layout hands back the content rect and
the width it laid out at; the line structure for that width is the text lane's.

Answers to the three questions, then the amendments.

### A1. Can a size-only measurement be far cheaper than shaping?

It depends on the script, and the honest answer has two halves.

* **Text the table shaper covers** (ASCII and Latin-1 without a ligature trigger: nearly all UI text, all code):
  shaping *is* a sum of advances and pair kerns, 7.0 instructions per byte recorded (`bough-text/RESULTS.md`
  line 214). There is nothing cheaper to find below it; what the size-only path saves is the stores and the
  memory: it writes no glyph ids, no positions, no atlas entries, only the **break table**.
* **Everything else** (complex scripts, ligature lines, fallback fonts): the advance is only known after real
  shaping (GSUB and GPOS decide it). There is no advance-only shortcut that is correct. The size-only path
  runs the shaper and keeps only the break table; the glyphs are dropped and re-made if the row is exposed
  (a memo hit if it happens soon).

The break table is the thing to cache, per (font, features, size-independent text key): for each break
opportunity `k`, two prefix sums in font units, `end[k]` (pen at the end of the segment, trailing space
excluded) and `next[k]` (pen at the start of the following segment), plus per-table `min` (the widest
unbreakable segment) and `max` (the widest hard line). Eight bytes per opportunity, about a byte and a half per
character of prose. With it:

* `inline()` is two loads.
* `block(w)` for a new width is, per line, one `partition_point` over `end` (monotone): `lines * log2(breaks)`
  probes, not a walk of every word. A 60-word, 4-line message: about 24 probes. The interval falls out of the
  same loop: `lo = max over lines of the line's own width`, `hi = min over lines of the width the line would
  need to take its next segment`. Both are rounded inwards (`lo` up, `hi` down) so the interval is never wider
  than the truth.
* `block(w)` for a width inside the interval is not called at all (layout holds the interval).

Prediction (journal T2): `block(w)` on a memoised table costs under 150 instructions for the chat's median
paragraph, against about 1,950 recorded for the façade's re-wrap at a new width and 187 M for its whole resize
step; a first measurement of an uncovered paragraph costs what shaping costs.

### A2. What `READS` means for text whose content is a state expression

Two different things reach a text leaf and only one of them is a block word.

* **Content that is a word of the block** (a number; a choice between constant strings): `READS` is the
  source expression's `READS`, joined with the `READS` of every other operand that feeds *shaping or
  breaking* (size, weight, slant, features, axes, tracking, line height, wrap). Paint operands (colour,
  decoration, background, opacity, palette) are excluded by construction: they are fields of the paint, not of
  the measure. The type computes it: `const READS: u64 = Src::READS | Size::READS | ...`.
* **Content that is not a word** (a `String` field, a `&str` from a row): it is not in the block and has no
  `READS`. It reaches the leaf as a *record operand*: the text key, two words (a 64-bit content hash salted
  with the font and features). The framework notices a change the way it notices any opaque state: the row is
  re-described and its operands compared (or the relation's journal names the row). A different key is a
  different leaf value, the row is in the layout lane's "Changed" class, and `Measure` is asked again with the
  new `&self`. So `READS == 0` for free text is correct and is not a hole.

A proportional number whose box is content-sized is `WRAPS = false`, `READS` = its word, and `inline` is a loop
over its digits with a ten-entry advance table (no memo, no shaper). A tabular one is `digits * cell`. Either
in a box of definite size is never asked.

### A3. Baselines and line metrics for mixed fonts and inline boxes

`Block { h, baseline, lo, hi }` survives mixed fonts as it stands, because the text lane computes `h` as the
sum of line boxes, each line box being the union of its runs' (ascent + half-leading, descent + half-leading)
and of its inline boxes placed by their own baseline and `vertical-align`. Layout does not need per-line
metrics. Two things do not fit and are the first two amendments.

### Amendments

1. **`last` baseline.** `Block { h, first, last, lo, hi }`. A multi-line text aligned by baseline inside a row
   is aligned by its first line; an inline-block and `align-items: last baseline` use the last. One more word,
   computed by the same loop.
2. **A text with inline boxes is a container, not a leaf.** The boundary runs both ways:
   * text asks layout for each inline box's shrink-to-fit size and its baseline (`Lay::size` at max-content,
     the layout lane's existing call), once, before breaking; the box is then one unbreakable item with
     (advance, ascent, descent);
   * after placement text hands layout each box's rect, relative to the text's content box, and layout places
     the box's subtree there (`Lay::place`).
   The interval is exact when the boxes' sizes do not depend on the line width. A box sized as a percentage
   of the line collapses the interval to `[w, w + 1)`: always re-measured, never wrong.
3. **`guess(w)`: an O(1) estimate from the text's own length.** For a row that has never been measured the
   layout lane proposes the running mean of measured rows. The text lane can do better for free:
   `ceil(bytes * mean_advance / w) * line_height`, with `mean_advance` a per-font constant. It needs the byte
   length and nothing else. Proposed as `fn guess(&self, w: i32) -> i32` with a default of "no opinion".
   Prediction (journal T2b): on the chat's 2,000 messages its total-extent error before the sweep is under 3 %
   and its per-row error under one line for 90 % of rows; kill: no better than the running mean.
4. **The interval promises the line structure and `h`, not glyph positions.** Under centre, end or justified
   alignment the glyphs move with `w` inside the interval. That is placement, on the text side, and cheap:
   a line is a record whose rect origin shifts; justification spreads `w - natural` over the line's gaps, which
   is arithmetic over one word per line if each glyph carries its gap ordinal (part B). Layout's "Holds" class
   must therefore still tell the text leaf when its rect's *width* changed, even though it does not re-measure.
5. **A measurement may be pending.** `block` on the interaction path must not shape a long uncovered
   paragraph. For text not yet in the memo the text lane answers `guess` immediately and queues the shaping
   (threads lane: lands as an epoch with the exact `Block`, which the layout lane treats as a correction, the
   same path as its sweep). For covered text under a byte budget (to be measured; about 4 KiB at 7 per byte is
   28,000 instructions) it answers exactly, inline.

### What the text lane keeps, so layout does not have to

Break tables live in the shape memo and are resident by exposure: a row scrolled far away keeps only what
layout holds (`h`, `lo`, `hi`: three words); its break table may be dropped and is rebuilt from the text if its
interval ends. Layout stores nothing of the text's.

## B. Raster: the glyph-instance column, the coverage paint, the atlas

`bough-paint` today has `Glyph<B, T, C>`: one record per glyph, rect lanes as the glyph's box, one `Rec` word
for the texel, a colour operand. That is a correct floor and the oracle. The proposal below replaces "a glyph
is a record" with "a text line is a record that names a range of a shared glyph column", and asks the raster
lane for one composed paint and one binning rule.

### B1. Why not one record per glyph

* **Mount.** A record per glyph is 16 bytes of rect, 4 of texel and the colour operand's words per glyph,
  written at every mount and translated at every placement. A memoised run (the same label 500 times in a tree;
  `}` in code; every digit string) is re-expanded each time.
* **Colour.** A paragraph with nine coloured spans becomes nine archetypes or nine colour words per glyph.
* **Selection, caret, decorations, justification** are properties of a line, not of a glyph.
* **Build-time text.** A constant string's glyphs can be rodata only if they are position-independent; a record
  rect is in the store's space.

### B2. The proposal

```rust
/// One line of text (or one unbreakable run): the record. Its rect lanes are the line's ink box.
#[repr(C)]
pub struct Line<G, P> {
    glyphs: G,      // GlyphSrc: where the glyphs come from
    paint: P,       // the style table: colours (and decoration colours) as expressions, indexed by glyph style
}

pub trait GlyphSrc: Words {
    /// The k-th glyph of this record under the block: position relative to the record's origin, atlas texel,
    /// box, style index. Pure, integer.
    fn glyph(&self, col: &[GlyphWord], u: &U, k: u32) -> GlyphAt;
    fn count(&self, u: &U) -> u32;
}

/// Shaped text: a range of the shared glyph column. Mount or Turn stage. Two record words.
pub struct Shaped(Rec<u32> /* first */, Rec<u32> /* count, baseline offset */);
/// A number: the glyphs are a function of a value. Any stage the value has, Frame included. No column.
pub struct Digits<const D: u8, V>(V /* Expr<Out = i32> */, Rec<u32> /* strike's digit strip */);
```

* **The glyph column** is one append-only array per window, shared by every text record, position-independent:
  three words per glyph, `[dx: i16, dy: i16] [texel: u32] [w: u8, h: u8, style: u8, gap: u8]`. `dx`, `dy` are
  whole device pixels from the line's origin (the subpixel choice was made when the run was placed, B4); `style`
  indexes the record's paint table; `gap` is the glyph's justification gap ordinal. A constant string's range
  is rodata mapped into the same address space (the column is two segments: image, then heap). A two-word
  alternative (6 bytes: `x` and a per-strike entry id, as the façade's pool has) saves half the bytes for one
  more load from a small hot table; the journal measures both (G1) before this is fixed.
* **`Digits`** needs no column: glyph `k` from the right is digit `(v / 10^k) % 10`, its texel is
  `strip + digit * cell`, its position `right - (k + 1) * cell_w`; the decimal point, sign, grouping and affixes
  are further terms of the same arithmetic. It is the same paint with a different `GlyphSrc`, so numbers are
  not a paint kind of their own. This requires of the font what the type promises: digits of one advance with
  no kerning between them (checked when the strike is made; otherwise the `tnum` feature, otherwise a
  synthesised cell of the widest digit).
* **Paint.** `p_glyph(c, x, y, cov8)` as it exists, with `c` taken from the record's paint table at the
  glyph's style index. A table entry is an expression of any stage, so a theme switch or a hover is a block
  write and the glyph column is untouched.
* **Damage** is `window(old) ∪ window(new)` of the line record, as for every paint. For `Digits` the window is
  the box (fixed) so a value write damages one rect.

### B3. What this asks of the raster lane

1. **A sink method for a ranged primitive**: `fn glyphs(&mut self, src: impl GlyphSrc, ...)`, i.e. a paint that
   stages a data-dependent number of `K_GLYPH` commands from one record. On the CPU this is a loop inside the
   record's kernel (no dispatch: the loop body is `p_glyph`). 
2. **Binning by sub-range.** Glyphs of a line are sorted by `dx`. A tile column that a line crosses needs only
   the glyphs whose boxes meet it: `partition_point` on `dx` with the strike's widest box as slack gives a
   `(first, count)` per tile. So a tile entry is (record, sub-range) and a pixel thread on the GPU loops over
   the few glyphs of its tile, not the line's. This is the one new thing in the tile list.
3. **A second buffer binding**, the glyph column, beside the atlas (binding 8 today). Read-only, append-only,
   upload ranges by the same page bitmask protocol as columns.
4. **`Digits` leaves in `ops.rs`**: `x_digit(v, k)`, `x_ndigits(v)`, `x_pow10(k)`: integer, Frame stage.

If (1) and (2) are refused, the fallback is the existing one-record-per-glyph `Glyph` for free text and a
fixed-arity `Digits<N>` row of `N` glyph records each with a Frame-stage texel operand
(`strip + x_digit(v, k) * cell`), which needs nothing new from the raster and still makes a number write one
word. The journal measures that form first because it runs on today's `bough-x`.

### B4. Positioning, hinting, gamma: decided once, the same on CPU and GPU

* **The atlas holds linear coverage, 8 bits, unhinted outlines** (the façade's `hint(false)`), rasterised at
  the device-pixel size. No gamma is baked into the atlas. The blend is `p_glyph`'s integer blend, one leaf,
  the same source for both executors, so exactness is the raster lane's existing gate. A contrast or gamma
  adjustment, if the user wants one (question in `TEXT-API.md`), is a 256-entry table applied to coverage
  inside `p_glyph`: integer, one leaf, still exact.
* **Horizontal subpixel positioning: four variants, chosen at placement on the CPU.** The pen's fractional
  quarter selects one of four rasterisations of the glyph (the atlas key is (strike, glyph, quarter)); `dx` is
  then whole pixels. Neither executor resamples, so nothing can differ between them. Vertical: the baseline is
  snapped to a whole pixel, one variant.
* **Scroll and translation are integers** (SEAM 6.1 item 3), so a moved line shows the same texels. A
  Frame-stage *fractional* motion or a scale (animated size, zoom) cannot reuse quarter-pixel masters exactly;
  those records are a different archetype (B5).

### B5. Large and animated text: coverage profiles and distance fields

Prediction-first, not built. A text whose size operand reads `Time` (or whose scale is Frame stage) cannot
rasterise a strike per frame (gpui-ce does exactly that: its atlas key holds the exact `f32` size). Options to
measure, in order: (a) scale the nearest master with a two-tap integer filter, exact on both executors, with a
re-rasterised master when the animation rests; (b) a signed distance field per glyph at one master size,
shaded with the raster's existing edge leaves (`Cost::Edge`); (c) analytic coverage from outlines in the
kernel for very large glyphs, where an atlas wastes memory (a 200 px digit is 40,000 texels, its outline is
30 segments). The type decides: a size operand that is `Frame` selects the scalable archetype, and the author
writes nothing. Kill for (b): visible corner rounding at body sizes when compared with the master at the 1:1
size, or a per-pixel cost above twice `p_glyph`'s.

### B6. Atlas residency

The atlas is a reserved, never-moving byte arena (the columns' memory rule), read in place by both executors,
committed on demand. A glyph is rasterised when a run that uses it is first *exposed*, not when it is shaped
or measured: size-only measurement (part A) creates no texels. A miss on the interaction path does not wait:
the raster of a new glyph is queued (threads lane) and the line paints when it lands; the ASCII set of a
strike that is in use is filled in the background ahead of need. Arenas are per strike, so a strike with no
exposed run for some epochs is released whole (`madvise`); glyph ranges that named it are invalidated by the
strike's generation in the memo key.

## C. Surface and threads

* **Surface (accessibility, events).** Text exposes, through the same view driven by the surface lane's sink:
  the string (`TextSrc::with_str`), its role (static text, link, text field, multi-line), and for editable
  text the `TextState`'s selection and composition ranges in UTF-16 at the boundary. Geometry queries the
  platform asks (`bounds_for_range`, `character_index_for_point`) are the same two functions as the author's
  `cx.text(..)` (index to rects, point to index), answered from the line records and the glyph column of
  exposed lines, and from the break table for unexposed ones. A selection scope reports one selection across
  its elements. Open with that lane: which of these are pushed as deltas and which are pulled.
* **Threads.** Three kinds of text work leave the interaction path and land as epochs: shaping of uncovered or
  long text (lands as a `Shaped` range and a `Block`), glyph raster (lands as atlas bytes and a generation), and
  the layout lane's measuring sweep. Each is a pure function of bytes the worker owns (the text is copied into
  the memo's key arena when queued), so no application state is borrowed across threads.
