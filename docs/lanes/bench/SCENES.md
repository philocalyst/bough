# The standing workloads

Five application scenes the user asked for, in `comparisons/bough-workloads`, each driven through the same
phases by one harness, so every lane measures against the same work. Round 7, bench lane. The scenes are
written as an application author would write them with the current public API (`bough::prelude`, plus
`bough::raw` where the prelude has no spelling); what each scene could not say is a result and is listed in its
module doc and summarised below.

## Run

```
cargo build --profile dist -p bough-workloads --bin workload --bin workload_alloc
bench/scenes/run.sh out.tsv sheet                       # one scene, identity record first
SCENE_SCALE=0.1 REPS=16 target/dist/workload all        # every scene, a tenth of the size
bench/scenes/baseline.sh <dir>                          # the baseline set (four configurations + cold starts)
cargo test --profile dist -p bough-workloads --test pixels   # the pixel gate (macOS)
```

Settings (environment): `SCENE_SCALE` (default 1, the specified size), `REPS` (steps per phase, default 48),
`MODE` (`once`: mount once, wires only, the default; `redescribe`: the owner re-describes every turn), `RENDER`
(macOS default 1: the GPU draws offscreen and each step waits for it; 0: frames end at the tile list), `SCALE`
(device pixels per point, default 2), `PHASES`, `BOUGH_PMU` (extra counters, Linux), `STEPS=1` (print every
step). One cargo feature per scene (`--no-default-features --features ide`) builds a scene alone.

Output: one TSV row per phase: steps, instructions per step on the calling thread (p50, p95; counter bracket
subtracted), cycles p50, wall time per step (p50, p95, max), allocations per step (`workload_alloc` only),
footprint and peak resident memory after the phase. `#` lines carry the model and the run's settings.

## Phases

| phase | a step is | what it exercises |
|---|---|---|
| `cold` | build the model; `App::new`; the first frame | startup (`mount`, `first_frame` rows; `since_exec_ms`) |
| `steady` | the scene's own interaction (below) + frame | the common path: small writes, input, scrolling |
| `hover` | pointer onto / off the element with thousands of dependents + frame | fan-out from one signal |
| `theme` | a theme switch + frame | every colour in the scene changes |
| `resize` | a window size from a triangle sweep (60-125 % of 1280 x 800 in each axis) + frame | relayout, re-raster |
| `resize_anim` | the sweep while the scene's transitions run (started every 6 steps, clock +1/60 s per step) | cross-fades during resize |
| `anim` | `tick(1/60)` + frame, transitions started every 20 steps | animation as a stream of frames |
| `idle` | a turn that writes nothing + frame | the floor |
| `idle5` (opt-in) | five seconds asleep, then idle steps | the memory lane's idle-after-5-s point |

## The scenes (at scale 1)

| scene | model | fan-out (hover) | steady step | transitions |
|---|---|---|---|---|
| `sheet` | 10 x 10,000 = 100,000 numeric cells, header, row gutter, totals, selection outline; 240,055 records | column header hover: one `look` signal read by every cell's fill and text colour (200,000 wires; 20,000 change) | a value edit (+ its column total), a selection move, a wheel detent | none of its own (a selection jump) |
| `ide` | 12 tabs, 2,000-entry tree, 3,000-line editor (14,585 token spans), minimap of 3,000 blocks, 528-line terminal, status bar; 30,166 records | hovering `value` highlights its 2,189 occurrences (4,312 identifier spans read one signal) | a character typed; every 6th a terminal line; every 10th a wheel scroll and minimap sync | tab switch under a fading cover, folder expand (height transition), minimap viewport and tree selection glide |
| `dash` | 200 chart cards (bars, sparklines of 40 quads, trends of 2 x 18 dots, gauges, stacked bars), header, legend; 8,188 records | legend "B" dims every other series in 200 charts (3,484 marks) | a data tick of 20 cards; every 8th a range-button click | card cross-fades (veil opacity), utilisation-bar widths, card ring |
| `canvas` | 20 clipped frames, 5,000 layered nodes (shadows blur 2-40, gradients, borders, labels, SVG icons, nested group clips, 1,000 component instances), layers panel, inspector; 10,856 records | layers-panel row of component 3 highlights its 200 instances (1,000 wires) | a drag gesture (`on_drag`), selection change, wheel pan | 50 nodes "smart animate" (position, opacity), a frame cross-fade |
| `chat` | (see its module doc) | | | |

## Correctness

`tests/pixels.rs`: through every phase of every scene (steady 8, hover 4, theme 2, anim 12 ticks, resize with
transitions 8 sizes), the presented frame equals a full redraw of the same retained state (`reference_pixels`),
at ring ages 1-4 and on the windowed ring, mount-once, plus re-describing on ring 2, at `PIXEL_SCALE` 0.05. All
scenes pass on the r7/bench tree of 2026-11-12. `tests/defects.rs` holds the repro of a defect a scene found
(nested `clip_by`, retained != fresh mount after a pan); it is `#[ignore]`d while it fails.

## What the scenes could not say (cross-cutting; per-scene lists in each module doc)

* **Group opacity.** `.opacity` multiplies one box's own paint, never its children: no scene can cross-fade a
  card, a frame or an editor as a unit. Every cross-fade here is a veil or cover box, or one transition per box.
* **Shadows, borders and colours on what moves.** Shadows exist only on raw `deco_box`, which has no operands; a
  flow box (the only thing with `translate` and `opacity` operands) has no shadow; a border colour is a constant;
  raw `text` has a constant colour (so canvas labels ignore the theme).
* **Effects.** No layer blur, backdrop blur, blend modes, masks, rounded clips, subtree transforms (rotation,
  scale, zoom); the only blur is a box shadow's.
* **Operands.** No operand over two signals (every scene packs theme and highlight into one `u32` by hand); no
  proportional width operand; `quad` has no x operand; `offset` takes a signal pair only (no `.map`, no
  `.transition`); a `.map` captures at most 16 bytes.
* **State shape.** A wired signal must sit at an `offset_of!` path, so per-item state is `[Signal<T>; N]` arrays
  sized at compile time (200 cards, 100,000 cells), and `List<T>` rows cannot read their owner's signals, so a
  list cannot take part in a root-level fan-out (the chat and IDE chose fixed slot arrays for that).
* **Composition.** No conditional or heterogeneous element (`Either`, `AnyElement`): two scenes wrote their own
  over `Mounter`; raw content in flow needs `anchor`, which is fixed in size.
* **Layout.** No wrapping row (the dashboard cannot reflow to fewer columns); `clip_overflow` is not a paint
  clip; a label measures without its edge spaces; flow labels expose no glyph positions (caret x is estimated).
* **Text.** A text from `.map` is a `String` (an allocation per update); only `.num` formats without one.
* **Defects** (correctness): nested `clip_by` (repro in `tests/defects.rs`); `clip_by` inside `anchor` mounts
  without the anchor's origin (reported by the canvas agent, not yet reproduced).
* **Capacity.** Labels take glyph slots in blocks of 16 and double as they grow (~32-35 per label measured): a
  scene must size `Caps::glyphs` from its label count or panic at mount; instances stay under the format's 2^18.

## Baselines

See `bench/results/r7/workloads/` and the dashboard's "Standing workloads" section; numbers and provenance in
JOURNAL.md.
