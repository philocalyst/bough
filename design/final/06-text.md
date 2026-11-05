# 06 — Text and the Editor Kernel

Scope: the inline-formatting-context text model (CSS-complete spacing/decoration/atoms), the
`text_input` standard control with its transaction/interceptor/IME laws, and the editor kernel —
typed coordinate spaces, the display-map transform stack, `code(&document)` as a mounted element,
`MultiBuffer`, and N viewports over one document.

Depends on: [02-elements.md](02-elements.md) §1.2 (phase closures), §7 (placement); [03-ports.md](03-ports.md)
§3 (`Impact`); [05-async.md](05-async.md) (the display pipeline's async wrap stage). Feeds:
[07-platform.md](07-platform.md) (IME as a platform protocol), [10-quality.md](10-quality.md)
(text gates Text-0–Text-7 and the platform bar T1–T6, 10 §1).

---

## 1 · Text is one inline formatting context

> Glyphs, spacing, decorations, and replaced atoms occupy the same line boxes, share the same
> caret/hit-test, and damage through the same obligation mask. A chip is a placeholder in that
> context. It is never an overlay.

An absolutely-positioned label on top of shaped text misses wrap, bidi, skip-ink, copy, and
screen-reader order — the anti-pattern this model exists to make unrepresentable. Style splits
into three sheets, each field carrying the same `Impact` obligation metadata as any other port
(03 §3):

| Sheet | Lives on | Invalidates | Examples |
|---|---|---|---|
| **Font/shape** | character/mark | reshape | family, size, weight, features, variations, kerning, writing-mode |
| **Paragraph/wrap** | block/IFC root | rewrap, realign | white-space, wrap-style, hyphens, indent, line-height, align |
| **Paint/decoration** | decorating box or mark | repaint (skip-ink samples coverage) | fill, stroke, shadow, underline family, emphasis, run background |

Plus one layout object that is not a style field: **atom/placeholder** (width, height, baseline,
`vertical-align`, side bearings, atomic break, skip-decoration) — one U+FFFC per replaced inline,
lives on `character` for invalidation purposes, and rewraps plus lays out its child.

**Order of operations**: transform (presentation-only, byte-length-preserving with a mapping
table — never a buffer mutation) → white-space collapse/trim → shape (features, variations,
kerning, bidi) → letter/word spacing, autospace, spacing-trim → line breaking (hyphenation,
wrap-style; atoms are unbreakable unless marked) → hanging punctuation + indent → alignment/
justification → paint. Kerning is inside shaping; tracking is a separate post-shape step — they
must never be implemented as the same attribute.

**Decorations are layers, not a single optional underline.** Find-in-page, IME composition
underline, link underline, spelling wavy, and a user highlight can all cover the same bytes; they
stack in a documented order (document style < semantic highlight < IME < selection < find).
Skip-ink interrupts a decoration where it would intersect glyph ink; an atom with
`skip_decoration` punches a hole in an ancestor decoration across its placeholder width — the CSS
reason a mention chip does not wear the paragraph's underline unless asked.

## 2 · Atoms: object replacement, not overlays

```rust,ignore
struct Placeholder {
    width: Px,
    height: Px,
    baseline: Px,                 // distance from the placeholder bottom to the alphabetic baseline
    align: InlineAlign,           // Baseline | Middle | TextTop | TextBottom | Super | Sub | Offset(Px)
    break_mode: AtomBreak,        // Atomic (default) | BreakAfter
    pad_inline: (Px, Px),         // inline-axis bearings; letter-spacing does not apply inside
    skip_decoration: bool,        // text-decoration-skip-box
}
```

The child's layout size *is* the placeholder; a paint-only chip update does not rewrap. Atoms are
`List` slots when the document is a `List<Inline>` — the same identity ladder as any row: identity
is the slot, never `.key()`; a `Vec<Inline>` of chips is a located error naming `List`. One
document offset per atom; the caret lands before or after, never inside; selection may include the
atom, and copy asks it for a payload. FlowIndex (the cross-element selection projection, §4 below)
counts a replacement as one unit. IME never composes inside an atom.

```rust,ignore
impl View for Composer {
    fn view(&self) -> El<'_> {
        rich(&self.pieces).atom(Composer::chip).align(TextAlign::Start).wrap(Wrap::Pretty)
    }
}

impl Composer {
    fn chip(&self, piece: &Piece) -> Option<El<'_>> {
        match piece {
            Piece::Mention(id) => Some(mention_pill(&self.users[id]).atomic().skip_decoration()),
            Piece::Emoji(id)   => Some(emoji_atom(*id)),
            Piece::Text(_)     => None,   // kernel paints text
        }
    }
}
```

`.atom(view_fn)` is `list.each(f)` for the IFC: one mapper, `None` meaning "this slot is text."
Plain text pieces carry marks via interval annotations (`self.pieces.mark(range, Mark::emphasis()
.color(accent))`), never a nested span tree — a document is ordinary ladder state, not a second
recursive element tree the framework diffs.

## 3 · `text_input(&Signal<String>)` — a standard control, and two-way means two-way

```rust,ignore
text_input(&self.query)
    .placeholder("Search…")
    .target(self.input)                  // Target<TextInput>: focus, selection, tests
    .on_edit(Search::filter_input)        // optional interceptor
```

> **The signal is the document of record, and an edit is one transaction in the same turn
> machinery as every other write.** A keystroke, paste, drop, or IME commit stages an edit
> against the signal's current value, runs the interceptor *inside* that turn, publishes once,
> and every consumer — the input's own presentation, a filtered list, the a11y projection —
> observes the same committed value in the same epoch. There is no event-then-writeback round
> trip, no frame where the box shows text the signal doesn't hold, and no second "trigger
> reactivity" step for `on_edit` to forget.

`on_edit` is an **interceptor**, not a change listener: `fn(&mut Self, Edit<'_>) -> EditVerdict`
where `EditVerdict` is `Accept`, `Amend(replacement)`, or `Reject`. It sees the typed edit (range,
inserted text, source: keyboard/IME/paste/a11y), may consult `&mut self`, and decides —
validation, masking, auto-formatting — *before* publication. `Reject` leaves signal, box, and
history untouched, coherently. What it cannot do is observe the new value and then write the
signal, because it runs before there is a published new value — the lagging/fighting controlled
input failure mode is unspellable.

**IME is a sub-transaction.** Composition/preedit lives in the input's private presentation
state; the signal sees only commits. **Focus loss first settles the composition (commit or
cancel per platform convention) as its own transaction, then delivers focus-out** — blur can
never drop composed text, and the a11y node for the input derives from the same committed epoch
as paint, so a stale semantic value cannot outlive a frame. History groups a composition as one
undoable transaction (01 §10.2's `Coalesce`/`Silent` tags).

Selection, caret affinity, grapheme motion, word/paragraph motion, bidi caret placement, and
platform text shortcuts ship in the control; the full IME protocol (surrounding-text revision,
UTF-8/UTF-16 range agreement, replacement range, candidate rectangle, preedit styling) is driven
by `text_input` itself, not left as capability documentation an application must implement
(07-platform §4 owns the platform-level wire protocol this control speaks).

## 4 · Selection across elements, and the document byte-offset law

Plain UI text supports cross-element selection: select from the middle of one label to the middle
of another, copy, get verbatim text. A **FlowIndex** — an incrementally maintained, sum-tree-
shaped projection over the manifest in reading order — makes this cheap without a second
selection tree: `.selectable(Select::Contain)` scopes a selection root; nested
`.selectable(Select::Exclude)` punches one flow atom to the parent (a nested editor is one atom
to its container, never a leak of its internals). Selection endpoints are live and clamp under
splices; copy and screen-reader text take a `StaticRange` snapshot at the turn boundary so a
structural splice never mutates an in-flight clipboard payload. A window with no `.selectable()`
allocates no FlowIndex — this and the other cross-cutting projections (grids, trees, live
regions, gestures) are catalogued in [07-platform.md §5](07-platform.md).

**Document indices are always bytes**, not display clusters. `text-transform`, hyphenation, and
bidi reordering keep a mapping table rather than a second string APIs accept — the honest version
of "uppercase but preserve length." An editor's `text-transform` is a view of the document, never
a buffer mutation.

## 5 · The editor kernel

The single hardest surface in GPUI is its own editor, and it is not an escape hatch from the
model above — it is the same `El<'f>` and the same phase-capability surface (02 §1.2) at full
strength, with typed coordinate spaces underneath.

### 5.1 Coordinate spaces are branded newtypes

Every position is `Pos<Space>` where `Space` is a distinct marker — `Buffer`, `Inlay`, `Fold`,
`Tab`, `Wrap`, `Block`, `Crease`, `Display`, plus the excerpt-composed `Multi` (§5.4) — and `Px`
is its own type. A raw `usize` never flows as a position;
the only way across spaces is the display map:

```rust,ignore
let caret: Pos<Buffer>  = self.selection.head();
let row:   Pos<Display> = self.map.to_display(caret);   // the ONLY path across spaces
let y:     Px           = self.map.line_top(row.line());
```

`self.map.line_top(caret)` — passing a `Pos<Buffer>` where `Pos<Display>` is required — does not
type-check. Revision safety across frames uses a scoped lifetime proof, because stable Rust
cannot mint a new nominal type per runtime revision:

```rust,ignore
pipeline.with_snapshot(|map: DisplayMapRef<'m>| {
    let row: DisplayPos<'m> = map.to_display(anchor);
    map.line_top(row);   // row cannot escape or be consumed by another snapshot
});
```

Across frames the application retains only durable `(ExcerptId, Anchor<Buffer>)`. Kernel code that
must queue a numeric display offset pairs it with a runtime-checked `MapRevision`; event lowering
converts it back to an anchor against the same committed presentation snapshot before the turn
begins — an honest split between compile-time and runtime identity, and the same `Scope<'id, K>`
brand primitive as everywhere else in the kernel (02 §8).

### 5.2 The display map is a mounted, async-maintained projection

`code(&document)` is an element; its retained mount owns the display projection:

```rust,ignore
struct Editor {
    document: Entity<Document>,        // sources — the only things the app names
    selection: Signal<Selections>,
    scroll: Signal<ScrollAnchor>,
    hints: Signal<Vec<InlayHint>>,
}

fn view(&self) -> El<'_> {
    code(&self.document)
        .selection(&self.selection)    // wires, not observations
        .scroll(&self.scroll)
        .child(inlay_hints(&self.hints))
        .child(minimap(&self.document).follows(&self.scroll).align(TopRight))
}
```

Everything a hand-authored "display pipeline" object used to do is now done by the mount itself:

- **Viewport identity is mount identity.** The minimap is a second element over the same
  document, with its own wrap width, geometry, damage, and provisional snapshots because it is
  its own mount — N viewports fall out for free, including nesting (§5.4).
- **Wrap width comes from layout.** The mounted projection reads its width from its own laid-out
  box, not a number the application transcribed.
- **Readiness is asked where everything else is asked**: `is_pending(&self.document)` at entity
  grain, or on any wire derived from it (05 §5) — no `.snapshot()` accessor pair.
- **Escape hatch preserved**: code that truly needs the projection as a value (tests, a status
  bar showing wrap column) asks the element's target for a reader
  (`self.editor_target.display()`), the same way tests query any committed state through targets
  (04 §4).

Within the projection, the layer stack is `multi → inlay → fold → tab → wrap → block → crease →
display`, each layer a `Transform` with typed adjacent spaces so the stack composes only when the
spaces line up:

```rust,ignore
trait Transform {
    type Above;
    type Below;
    fn down(&self, p: Pos<Self::Above>) -> Pos<Self::Below>;      // exact
    fn up(&self, p: Pos<Self::Below>) -> Range<Pos<Self::Above>>; // a fold maps to a range
}
```

Font-metric-dependent wrapping is async: layout emits a `WrapDemand { projection,
document_revision, font_revision, width, dirty_ranges }`; the epoch barrier — not the layout
phase closure itself — coalesces it and arms a durable wrap producer through the ordinary
[05-async.md](05-async.md) admission machinery. While a revision wraps, the viewport keeps a
coherent exact-or-provisional snapshot descended from the prior revision; admission rejects a
result whose document, font, width, viewport, or projection generation is stale. Only an admitted
next-flush result advances the public projection revision.

### 5.3 Blocks and inlays are ordinary elements at typed positions

A block is an `El<'f>` slotted at a `Pos<Display>` (or between two display rows); an inlay is an
`El<'f>` at an intra-line anchor. Both go through the same `ChildSeq` sink and the same measure
phase as any `div` — the editor row stream is "a shaped line **or** a child element," so
interleaving diagnostics, git blame, and inline assists with text is not bespoke layout code:

```rust,ignore
canvas(&self.map)
    .rows(|row: Pos<Display>| match self.map.content(row) {
        Content::Text(line)  => shaped_line(line),          // borrowed glyph run
        Content::Block(slot) => self.blocks.get(slot),      // ordinary El<'f>
    })
    .inlays(&self.inlays, Inlay::view)
    .paint(|paint: Cx<'_, Paint>| paint.glyphs(&self.shaped))
    .hit(Editor::hit)
```

Only the frame-local child is an `El<'f>`; the retained projection stores `BlockId`, an anchor,
an owned renderer descriptor, and committed `BlockMetrics` — neither the display-map snapshot,
a cache, nor a wrap future ever contains an `El<'f>`.

`.paint(|paint| paint.glyphs(&self.shaped))` **borrows** `self.shaped` for the frame and streams it
straight into the scene arena — the same zero-copy append `text(&s)` performs for a label, now at
glyph-run granularity — while the commit diff still promotes only the scene ranges that actually
changed (the atlas already caches the glyphs themselves).

### 5.4 `MultiBuffer` and N viewports

`MultiBuffer` is an excerpt composition over many buffers with one semantic API: an
`Anchor<Buffer>` is a rope sum-tree identity (bias + edit timestamp) that rebases across edits —
durable identity, not an offset. `MultiBuffer` is a `List<Excerpt>` projection; each excerpt owns
a stable identity, a source `Entity<Buffer>`, an anchored source range, and its summary.
`Pos<Multi>` resolves to `(ExcerptId, Anchor<that buffer>)`; edits invalidate only affected
excerpt summaries. The one-buffer editor uses a specialized singleton representation but the
identical excerpt/position contract — search results, diffs, and ordinary files share selection,
display-map, serialization, and collaboration logic without pretending there is one global
buffer.

A `Viewport<Document>` owns local scroll, scale, visible range, selection presentation, focus/
targets, and layout obligations, while sharing the document, `MultiBuffer`, document-invariant
logical snapshots, shaping cache, and atlas by revision. A main editor, a split, a peek, and a
minimap are peers even when one viewport's element is nested inside another; they never
accidentally share width-specific wraps, selections, scroll, or hit geometry, and reuse requires
a compatible viewport spec.

## 6 · Text raster and measurement

Text raster policy — hinting, antialiasing, atlas sampling filter, pixel snapping — is an
application/window default with subtree override, using `Prefer`/`Require` fallback like every
other capability request (07-platform §1):

```rust,ignore
TextRaster::default()
    .metrics_hinting(Hinting::Full)
    .antialias(Antialias::BiLevel)
    .atlas_filter(Sampling::Nearest)
    .snap(PixelSnap::BaselineAndStems)
```

`TextMeasure` is a read-only service (text, style, constraints, locale/direction, scale in;
ink/logical bounds, baselines, line metrics, clusters, carets, overflow out); retained
`TextLayout` handles expose the same metrics after commit. Measurement never secretly mounts an
element or mutates the font cache from a paint callback.
