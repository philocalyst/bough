# A scale change without a remount (design note, platform lane, round 7)

**Problem.** A window moved to a display of another scale (gui-bench's `scale-switch`: 2.0, 1.0, 2.0, ...) must
draw at `logical x scale` device pixels. bough fixes `Rt::scale` at `App::new` and bakes device pixels in at
mount: layout styles are interned per (spec, scale), glyph strikes are per pixel size, quad instances hold pixel
geometry, radii and border widths, spaces, clips and scroll views hold pixel offsets, images are resampled to
pixel size, and bough-core's inks and damage tiles are device pixels. A remount at the new scale with signal heads
reset would lose identity and state (handles, focus, hover, scroll positions in flight, transitions, held async
cells, list slots), so it is not the mechanism.

**Proposal: scale is a property of the root space; a change is a relayout plus a full repaint.** Nothing that
is identity or state changes: records, handles, wires, signals, owners, lists, readers, scroll offsets (they are
points), transitions (point-space values advanced by the clock) and the focus all stay. What is re-derived is
exactly what is a function of `(points, scale)`:

1. **Layout.** The root's scale is an input of every group, as the root's size already is on resize: set it,
   mark every group queued, `lay_flush`. The interned style ids stop including the scale (the engine applies it
   when it lays out), so no node is restyled. Place hooks then write every instance's pixel geometry, every
   record's ink, every space, clip and scroll port, as they do after any relayout.
2. **Paint quantities baked at mount** (corner radius, border width, shadow sigma, ink margins): stored in points
   on the node next to the paint data they produce, and multiplied by the scale where the instance is written
   (the place hook already writes the instance). This is the one representation change; it removes a class of
   "px stored at mount" values rather than adding a re-derivation path for each.
3. **Text.** Shaping is in font units (pool slots, `RunHeader` pens) and does not change. Each run header's strike
   (pixel size), scale and origin are re-derived from (point size, scale); the atlas rasterises the new strikes on
   demand (or ahead, through `prepare`'s raster threads). A label never re-shapes.
4. **Images** are resampled to their new pixel size from the decoded source (kept by content key, as media already
   wants: decode once, resample per exposed scale).
5. **Damage and surfaces**: the window's pixel size changes as on a resize (`App::resize` path: damage rings,
   tile grid, surfaces) and the frame is a full repaint.

**Cost and order.** One relayout, one restrike, one full frame: the same work as the first frame, without the
mount. Items 1 and 3 are what a resize already does; 2 is the real change and belongs to the core/store lanes'
column work (radius, border and margin as point-valued columns). Until 2 exists, a re-description pass with a
"rescale" flag can re-apply the baked values for re-describing views (`LBox::update` recomputes the instance from
the spec times the scale), which covers the prelude; mount-once scene ports would stay at the old scale.

**Test.** Retained after a switch 1.0 -> 2.0 -> 1.0 equals a fresh mount at the final scale with the same state,
pixel for pixel; and state survives the switch (scroll offset, focus, a held reader).
