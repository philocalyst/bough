# Round 7: lead's notes from reading the code (2026-11-12)

Written for the core, compute and async lanes. Hypotheses with the code they come from; none is measured yet.

## 1. Damage lives in the wrong space

`bough-core/src/damage.rs`: damage is a screen-space bitmap, one bit per 32 px tile, one `u128` per tile row,
a ring of 4 planes. Every write projects itself to tiles immediately (`mark`/`add`), which is where most of a
colour applier's instructions go, and the `u128` row is the 4096 px window limit (a known panic).

It is also why damage does not generalise. A tile bit cannot say *what* changed, so layout dirtiness, GPU
instance uploads, accessibility deltas, media tiles and async holds each keep their own record of the same
write. A blur or a shadow has to special-case its expansion at mark time.

Proposal: **damage is record-space first, screen-space derived.** A write sets one bit in its page's dirty
plane for its class (paint, geometry, text, media, structure): one `or` into a word the write already has in
cache. At the end of the turn one pass projects dirty records to tiles (old rect and new rect, expanded by the
record's effect radius, which is data in a lane, not a branch). Every other consumer reads the same planes:
layout reads `geometry|structure`, the uploader reads `paint|geometry` to get instance ranges, accessibility
reads `text|structure`. The tile bitmap becomes an output of the frame, sized to the window (no 128 limit),
and the ring of planes can be a ring of record-space words where that is smaller.

Things to test: instructions per colour write (now 606 at 256 rows, predicted under 60); cost of projection per
dirty record with rect lanes gathered 8 at a time; whether buffer-age repair is cheaper as "OR of the last k
dirty planes, projected once" than as k tile planes.

## 2. The frame as a simdjson-style pipeline over pages

simdjson's shape: stage 1 turns every 64-byte block into bitmasks with no data-dependent branch; stage 2 visits
only the set bits. Ours, with the 64-row page as the block:

1. classify: `work = dirty_any | (animated & visible)` per page, one word.
2. project: for set bits, rect lanes -> tile spans -> OR into tile rows.
3. bin: tiles -> per-tile command lists (prefix sums, as Vello does).
4. raster: CPU bands or GPU.

Every stage is a map over pages whose result merges by OR or by concatenation, so splitting pages across cores
needs no lock and no second code path: the one-core loop and the many-core loop are the same loop with a
different range. A page with `work == 0` costs one load and one compare; an idle frame is a scan of zero words.

## 3. Transitions cost a turn per frame today

`bough/src/transition.rs`: the frame clock (`App::tick`) evaluates each active transition on the CPU and
applies the value through the ordinary applier, so every animated frame is a write, a damage mark and, for a
layout property, a relayout.

Proposal: **a transition is data in the record, evaluated by the kernel.** The instance carries
`(from, to, t0, duration, ease)` for the animated lane; the presenter supplies `now`. No turn runs while it
plays. The `animated` plane says which records need a frame; their damage is the swept bounds of from and to,
known when the transition starts. The CPU executor evaluates the same expression (one kernel source), so
exactness holds. Interruption rewrites `from` with the value at `now`, which the CPU can compute with the same
function, keeping the C0/C1 continuity the current code has.

For layout properties: solve the end layout once, keep old and new rects, and let the kernel interpolate rects
(text and images that would reflow differently mid-way are the exception; they can opt in to per-frame layout).
One relayout per transition instead of one per frame.

User shaders fit the same slot: a shader node is a record whose kernel is user-supplied, with `time` as a
parameter; `animated` is set while it declares itself time-dependent.

## 4. Async holds as a plane

If a pending resource is a bit in a `pending` plane and a withheld journal names the pages it touches, "is this
subtree held" is a mask test in stage 1, not a walk, and releasing a world is OR-ing its journal into the dirty
planes. Torn frames are then unrepresentable: stage 1 only ever sees applied journals.
