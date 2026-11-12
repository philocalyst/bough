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

Result, half of it: the *unreachable* comparison was measured (A1 vs the same tree without A1, profile dist,
`runtime/lanes/composition/ab_dist.sh`): every `signal_cost` cell and every deterministic `redescribe_cost` cell
equal or within 0.25 %; `signal_cost` binary 3,055,840 -> 3,055,744 B, `redescribe_cost` 3,089,328 -> 3,072,720 B
(the per-mount reflection writes deleted). The *reachable-but-idle* variant (one `App::tree` behind an env
check in `signal_cost`) was not built: not measured.

## X2. What one reflection costs (can an inspector afford it every frame?)

Mechanism: `App::tree()` runs the view's description once more (a re-description without patching) plus one
pass over the records and the wire-free joins. So it should cost about one re-description plus a few hundred
instructions per record for strings and lookups.

Prediction: for `signal_cost`'s 256-row view, `tree()` at 1.5x-3x the 256-row re-describing turn (~0.5 M), i.e.
0.8-1.5 M instructions, linear in rows. Kill: above 5x a re-description (then the inspector needs a cache keyed by
the epoch and the walk needs pruning by dirty owners).

Result: not measured (the session was stopped for the checkpoint).

## X3. Damage that never repaints what an opaque native view covers

Mechanism: a tile wholly inside the visible rect of a `Mode::Under` foreign element, with no record painted
after the element meeting it, holds (0,0,0,0) in every surface drawn since the element last moved, whatever
changes beneath it. So such tiles are removed from the repair rows; the hole's own edge tiles still repaint.

Prediction: a background that animates every frame entirely under a 320x240 pt web view (2x scale: 20x15 = 300
tiles) repairs 0 tiles per frame instead of 300 (the animating rect), with presented == full redraw at every
ring age. On a frame where the element moved, the tiles repaint once. Cost with no foreign element: zero (the
check sits behind `foreign.live != 0`). Kill: any pixel gate failure, or a frame without foreign elements
paying anything.

Result: not built (the session was stopped for the checkpoint).

## A1's release-profile cells (a control, not an experiment)

The same A1 change measured at profile `release` (thin LTO, 16 codegen units) moved cells it does not touch:
`signal_cost` once/number +30 instr/turn (+1.0 % at 16 rows, +2.9 % at 64), redescribe 256 rows -3.8 %,
`redescribe_cost` bare -24 %. A control that restored `Rt`'s size with a padding field did not restore them, and
at profile `dist` (fat LTO, one unit) the same pair is equal within 0.25 %. So `release` cells carry codegen-
partition noise of a few percent on unrelated edits; gates should be read at `dist`. `redescribe_cost` at 100
rows also varies run to run by +-3 % in both binaries (heap-state dependent): report medians of 5 there.
