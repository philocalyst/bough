# Composition lane: experiments (predictions written before running)

Each entry: the mechanism in two sentences, the predicted number, what would kill it, then the measured result.
Instruction counts are `tinstr` slopes (deterministic per binary); profile `dist` (fat LTO, one codegen unit)
unless stated, because the `release` profile's codegen-unit partition moves cells by up to a few percent on
unrelated edits (A1's release cells, below).

## X1. Reflection that is reachable costs nothing when it is not running

Mechanism: reflection is a generic walk (`Element::probe`) and joins (`tree.rs`) instantiated only where an app
calls `App::tree`/`find`/actions; nothing is written at mount or in a turn. So a binary in which reflection is
reachable (one call behind an env check) differs from one in which it is not only by the code of the walk.

Prediction: `signal_cost` turn cells identical within 0.3 % (dist); binary `__TEXT` larger by 40-90 KiB when
reachable. Kill: any turn cell beyond 1 %, or `__TEXT` growth above 200 KiB.

Result: (pending)

## X2. What one reflection costs (can an inspector afford it every frame?)

Mechanism: `App::tree()` runs the view's description once more (a re-description without patching) plus one
pass over the records and the wire-free joins. So it should cost about one re-description plus a few hundred
instructions per record for strings and lookups.

Prediction: for `signal_cost`'s 256-row view, `tree()` at 1.5x-3x the 256-row re-describing turn (~0.5 M), i.e.
0.8-1.5 M instructions, linear in rows. Kill: above 5x a re-description (then the inspector needs a cache keyed by
the epoch and the walk needs pruning by dirty owners).

Result: (pending)

## X3. Damage that never repaints what an opaque native view covers

Mechanism: a tile wholly inside the visible rect of a `Mode::Under` foreign element, with no record painted
after the element meeting it, holds (0,0,0,0) in every surface drawn since the element last moved, whatever
changes beneath it. So such tiles are removed from the repair rows; the hole's own edge tiles still repaint.

Prediction: a background that animates every frame entirely under a 320x240 pt web view (2x scale: 20x15 = 300
tiles) repairs 0 tiles per frame instead of 300 (the animating rect), with presented == full redraw at every
ring age. On a frame where the element moved, the tiles repaint once. Cost with no foreign element: zero (the
check sits behind `foreign.live != 0`). Kill: any pixel gate failure, or a frame without foreign elements
paying anything.

Result: (pending)
