# bough input: pointer, hover, focus, keys, text fields, drag, wheel, tooltips, resize

Branch `input` (worktree `bough-input`), round 3 and 3b (1.4). Apple M3 Pro, macOS 15.7, `nightly-2026-10-17`, `dist` profile.
The machine was shared with three other agents' builds (load average 15–125 during the runs), so **retired
instructions are the figure to read**; times are context. Timer resolution on this Mac is 41.7 ns
(`mach_absolute_time`), so bough's sub-microsecond times are quantised.

```
export RUSTC_WRAPPER= PATH=~/.local/share/cargo/bin:$PATH CARGO_TARGET_DIR=/Users/mileswirht/projects/bough/target/input
export ZGX_FONT_FILE=/Users/mileswirht/projects/_ref/zgx/comparisons/fonts/DejaVuSans.ttf
cargo test --release -p bough-core --test hit          # hit test / chains / cells / validity / tab order vs oracles
cargo test --release -p bough-zgui --test input        # pixel gates for every interaction, ring ages 1-4 + windowed
cargo build --profile dist -p bough-zgui --bins --examples
bench/input/run_all.sh                                 # bough's side of the zgui suite, 3 runs -> bench/results/input/bough/
target/input/dist/zgui_scenario <idle|hover-storm|scroll|cold-start|kitchen-sink|app-shell|thread-resize|first-pixel[-serial]|*-dump>   # RENDER=1: GPU too
cargo test --release -p bough-zgui --test drawing --test first_pixel   # 3b: drawing gates, overlapped cold start
target/input/dist/label_churn ; target/input/dist/resize_slope                                # zgui's probes
/usr/bin/time -l target/input/dist/examples/startup_bench                                     # zgx-gpu startup_bench
target/input/dist/drag_probe       # windowed (take the lock): zgx drag_probe SAME_WINDOW method
```

zgui's side: `bench/results/input/zgui/SUMMARY.md` (zgui 951a0ea re-run on this Mac in this session, 3 runs, with
instruction counts added to its harness: `zgui-bench-instr.patch`), dumps of its documents in
`bench/results/input/zgui/dump/`. zgx's side: `startup_bench` and `drag_probe` built from a copy of zgx 06debe9d
(`dist`; drag_probe patched only to launch as an Accessory app that never takes focus).

## 1. Results

### 1.1 zgui `cargo xtask perf` scenarios and probes (zgui's units; instructions process-wide, p50, median of 3 runs)

zgui's span is one event delivered plus `settle(64)`: handlers, restyle, layout, every frame until quiet, through
its `NullRenderer` (a display list, no device). bough's span is the input turn plus `frame()`, which ends at the frame
slot's tile list (the GPU's whole input). Both are CPU-only (`RENDER=0`); bough's GPU numbers are in 1.3.

| measurement | zgui time | **bough time** | zgui instructions | **bough instructions** | × fewer |
|---|---:|---:|---:|---:|---:|
| idle.turn | 0.04 µs | ≤ 0.04 µs | 1,137 | **409** | 2.8 |
| idle.frames | 0 | **0** | – | – | – |
| hover.crossing | 171.5 µs | **0.29 µs** | 1,831,004 | **4,277** | 428 |
| scroll.translation | 51.5 µs | **0.33 µs** | 604,853 | **6,998** | 86 |
| scroll.recycle | 277 µs | **1.17 µs** | 2,054,098 | **21,935** | 94 |
| cold.first_frame | 377 ms (164 in the quiet xtask run) | **7.2 ms** | 1,146,840,358 | **75,729,696** | 15 |
| kitchen.click (the press, zgui's span) | 48.5 µs | **0.04 µs** | 469,414 | **1,374** | 342 |
| kitchen.click release (activation) | – | 0.17 µs | – | 2,864 | – |
| kitchen.keystroke | 776 µs | **0.21 µs** | 4,447,493 | **3,424** | 1,299 |
| kitchen.untouched_turn | 97 µs | **0.21 µs** | 898,314 | **4,185** | 215 |
| kitchen.glide_tick | 1,604 µs | **1.00 µs** | 10,501,127 | **21,113** | 497 |
| kitchen.tab_traversal | 574 µs | **0.29 µs** | 4,534,195 | **3,103** | 1,461 |
| shell.tooltip_crossing | 2,239 µs | **0.92 µs** | 11,312,783 | **17,262** | 655 |
| shell.drag_step | 402 µs | **0.33 µs** | 3,439,018 | **6,230** | 552 |
| shell.drag_hit_rebuilds | 1 | **0** (no index exists) | – | – | – |
| scroll.translation.hit_rebuilds / relayouts | 0 / 0 | **0 / 0** | – | – | – |
| label-churn (30 labels per frame) | 1,490 µs | **3.3 µs** | 10.9 M | **53,259** | 205 |
| resize-slope: configure | – | 0.22 µs/box | 47,687 /box | **5,140 /box** | 9.3 |
| resize-slope: content change | – | 0.003 µs/box | 21,082 /box | **75 /box** | 281 |

The glide is zgui's own (`zgui-scroll` `Tween`: 240 ms, ease-out cubic, detents compose); bough's offsets match zgui's
dumped offsets to the fourth decimal (`glide_scroll.tsv`: 98.6549, 126.8736, 152.9059, …), and every kitchen notch
glides 15 ticks as zgui's does (120 measured ticks). The tab traversal lands on the same field after the same 22
presses (zgui's tab order is preorder; bough's is a scan of the kind column in preorder). Keystrokes type "a".."h",
with 8 backspaces after every 8th, and leave the value "Ada Lovelace" as zgui does.

resize-slope's verdict quantity, the ratio of the two slopes (zgui recorded 0.865, measured 2.26 here), is 68 for
bough: its content change is a fan-out of 4-byte paint writes (75 instructions per box) while a configure runs
cephal over every box (4.7 K of the 5.1 K instructions per box are cephal's own compute, used unmodified), so the ratio
says nothing about a regression in bough and is not used.

### 1.2 zgx

| benchmark | zgx | **bough** | |
|---|---:|---:|---|
| `startup_bench` (renderer 1920x1440 + font + shaping a line in 4 styles), instructions (`/usr/bin/time -l`) | 185 M – 220 M | **109 M – 117 M** | 1.7× fewer |
| same, wall: renderer / shaping | 31 ms / 6.5–34 ms | **22–45 ms / ~1 µs** | the renderer is Metal device + pipelines in both |
| `drag_probe` SAME_WINDOW, main-thread instructions per pointer move (3 drags × 81 moves) | 182,576 / 211,352 / 219,862 | 214,032 / 220,918 / 221,039 | **tie** (see 4) |

Not comparable here: `input_latency`, `key_workload`, `field_workload` are windows for an external latency probe
(`scripts/windows_latency_probe.py`, Windows) or need real keystrokes; the field scene's idle cost is the blink scene
already in STATUS §1.2. `focus_scroll` is a smoke test (offsets printed, no cost). `kitchen_sink` is a manual test
window. `display_transition` / `dpi_transition` need a second display.

### 1.3 bough with GPU frames (`RENDER=1`, one run)

Every frame adds Metal's per-command-buffer floor (~200 K instructions, STATUS §1.1) and the wait for the previous
frame: hover.crossing 205 K, scroll.translation 206 K, kitchen.keystroke 206 K, kitchen.glide_tick 217 K,
shell.drag_step 202 K, shell.tooltip_crossing 1.39 M (7 frames: the fade), label-churn 254 K, cold first frame with
the device 38 ms / 135 M. zgui's numbers above have no device at all.

### 1.4 Round 3b: the same work as zgui, what it draws, the cold start across cores, the missing scenarios

Load average 90–113 throughout (other agents' builds): instructions are the figure; times are upper bounds.
Medians of 3 (`bench/input/run_all.sh`, raw output in `bench/results/input/bough/`).

**The gallery and shell are now real bough views built from zgui's source documents** (`GalleryL`, `ShellL`;
`*-dump` scenarios keep the earlier dump-geometry variants for comparison). `build.rs` emits zgui's tree in preorder
with each element's computed layout declarations; bough's own style path (`ltemplate::resolve_style`, cephal's
parsers) resolves them at mount and cephal lays the tree out from scratch, text measured and wrapped (leaves:
`LeafCtx`). zgui's coordinates are not used (content height 7,998 vs zgui's 8,066; the probes land within a pixel).
Not reproduced: a cascade (the declarations are zgui's computed values, so bough parses but does not match
selectors: 50,426 selector matches are in zgui's number and not in bough's), inline formatting contexts (inline
elements become blocks; the thread's inline spans are one run per paragraph).

| measurement (RENDER=0) | zgui | **bough, laid out** | × fewer instr | bough, dump geometry |
|---|---:|---:|---:|---:|
| cold.first_frame | 164–377 ms / 1,146.8 M | **13.4 ms / 145.5 M** | **7.9** | 7.5 ms / 77.7 M (14.8×) |
| kitchen.resize (32 widths, cephal relayout) | 9.0 ms / 29.6 M | **0.18 ms / 2.97 M** | 10.0 | – (does not reflow) |
| shell.resize_step (40 sizes) | 3,266 µs / 16.4 M | **77 µs / 1.51 M** | 10.9 | – |
| thread.resize (250 messages, 499 rewraps/step) | 28 ms / 130.2 M | **1.04 ms / 21.7 M** | 6.0 | – |
| shell.drag_step (panel basis restyled, relaid) | 402 µs / 3.44 M | **5.4 µs / 101.6 K** | 34 | 0.38 µs / 6.3 K |
| shell.tooltip_crossing | 2,239 µs / 11.3 M | 0.92 µs / 17.2 K | 657 | 17.3 K |
| kitchen.keystroke / click / tab / untouched / glide tick | (1.1) | 3,448 / 1,378 / 3,189 / 4,518 / 21,588 | 1,290 / 341 / 1,422 / 199 / 486 | 3,441 / 1,371 / 3,112 / 4,199 / 20,446 |

Spans in bough's cold.first_frame: `App::new` (runtime and region, the mount of 2,451 records, style resolution,
cephal's first layout of 2,288 layout nodes, text shaping and measuring, wrapping, glyph rasterisation of 375
glyphs, 66 SVG documents parsed and rasterised by resvg (31.9 M), gradient ramps, the two extra faces' table
build (28 M)) plus the first `frame()` to its tile list. Not in it: the Metal device (RENDER=0; see first pixel).
The 15× of round 3 compared zgui's full pipeline with a mount of pre-positioned records; 7.9× is the
like-for-like figure.

**Drawing what zgui draws** (`deco.rs`, `text.rs` faces; pixel gates in `bough-zgui/tests/drawing.rs`):

| feature | how | gate |
|---|---|---|
| linear / radial gradients (`probe-ramp`: 100deg 3 stops at 55 %; circle at 30 % 30 %) | fused kind 5: a 256-texel premultiplied ramp in the RGBA image atlas, box-relative geometry (layout moves/resizes it like a quad) | every 3rd pixel vs the CSS formula: worst 0.9 level; moving under ring ages 1–4 + windowed: presented == full redraw |
| box shadows (15 + 6 + 2 + 1 elements; spread rings as rounded rects) | fused kind 6: Gaussian of a rounded box (closed form along x, 4 samples along y); the ink margin `ceil(3σ)+1` travels in the instance and the record's ink, so the kernel never writes outside the ink | 130 pixels vs a numerically integrated Gaussian: worst 1.65 levels; every pixel outside the ink untouched; moving: presented == full redraw |
| icons and charts (127 SVG nodes, 32 documents) | usvg + resvg at device size into the image atlas, `currentColor` resolved at build time; fused kind 4 | 32×32 icon vs resvg: ±1 |
| the spinner's turn | the ring (rotation-invariant) plus the mark as an image turning about its centre: kind 4's angle word is a paint write per frame; ink = the circumscribed circle | a quarter turn vs the rotated raster ±2; 40 turning frames: presented == full redraw, ≤ 9 tiles repaired |
| faces (106 elements at weight 600, 22 at 700, 4 at 800 → DejaVu Sans Bold; 8 monospace → DejaVu Sans Mono) | strikes belong to a face; `TextSys::select` swaps a face's shaper and metrics in; faces without the token model (Bold, Mono) take the general path | 'H' bold 40 px and 'M' mono 30 px equal their own font's raster exactly (and differ from the regular face's) |
| display text (116, 120 px; 54, 56 px) | the leaf's run path (no label tile-row cap) | 'A' at 120 px equals its raster exactly |

Fixed on the way: laid-out text leaves never readied their strike's glyph tokens, so text at sizes no label used
(12 px, 18 px, …) was not drawn (a frame-vs-redraw gate cannot see a glyph missing from both); `LeafCtx::set_text`
now prepares them. The kernel now clamps a corner radius to the box as CSS does (pills and avatars were blank).
Still not drawn: transforms other than the spinner's (rotated and scaled text, the tilted card, the navigation
arrow), gradient-filled text (`background-clip: text`, drawn in the text colour), non-uniform borders.

**The cold start across cores** (`bough::app::prepare`, called at process entry): one thread builds the Metal device,
queues, library, both pipelines and the window-sized resources (atlas, bins, four surfaces); raster workers take the
document's strikes (the glyphs of its static text, per face) and its SVG documents; one thread per extra face builds
its shaping tables. The main thread meanwhile builds the runtime, mounts, resolves styles, shapes and lays out into
heap memory, which the presenter then wraps as its shared buffer without a copy (`newBufferWithBytesNoCopy`). Gate:
`tests/first_pixel.rs` (overlapped == serial == full redraw). `zgui_scenario first-pixel[-serial|-dump]`, process
entry to the GPU's completion of the first frame:

| | serial (all on main) | **overlapped** |
|---|---:|---:|
| wall, entry → first presented pixel (load ~110) | 45.5–55.7 ms (median 47.0) | 42.0–59.7 ms (median 52.2) |
| wall, the same in a quieter moment (load ~73) | 42–49 ms | 36.6 ms |
| main thread: mount done | 38–50 ms | 7.6–10.7 ms |
| instructions: main thread | 208.5 M | **98.7 M** |
| GPU preparation thread (device 22–50 ms under load) | – | 52.6 M |
| SVG workers (66 documents) / face threads / raster workers | – | 29.2 M / 28.6 M / 6.0 M |
| rest of the process (Metal's and libdispatch's threads) | 11.6 M | 12.4 M |
| process total | 220.1 M | 229.1 M |

The main thread's work halves, but the wall time is the Metal device's creation (22–29 ms on a quiet machine,
33–50 ms under this load), which no amount of mounting overlaps: the main thread is done at ~8 ms and waits.
Under a load of 110 on 12 cores the extra threads also compete with each other, so the overlapped wall time is not
better than the serial one here; on the quieter run it was 6–12 ms better.

**The scenarios not ported in round 3**: `kitchen.resize` (GalleryL relaid by cephal at 32 widths), `shell.resize_step`
(40 window sizes, panels restyled by percentage), `thread.resize` (zgui's 250-message thread, 32 widths, 499
paragraph rewraps per step as zgui's). Nested scroll views follow an outer scroll (`ScrollView.nested`: the inner
view's space and clips move with it) and tooltips inside a scrolled view follow it (gate
`nested_views_and_tooltips_follow_an_outer_scroll`); `clip()` inside a view re-cuts against its parent. The caret
blinks (0.53 s half period, zgx's) as a paint write; after two flips both phases are on held surfaces and every
blink frame is a surface publish with no GPU work (gate `caret_blink_is_served_by_held_surfaces`).

Not this agent's (round 3b split): resizing a real window's GPU surfaces and the drag probe's main-thread GPU wait /
CA commit (floor agent); bough-text internals (text3 agent) — this round added, minimally and in new code where
possible, `atlas/parallel.rs` (prerendering), per-strike faces in `GlyphCache` and the token readiness call above.

## 2. The mechanisms (one per job)

**Hit testing is a range query over the paint preorder** (`bough-core/src/bough/hit.rs`). The answer is the last
record whose visible ink (`ink ∩ clip`) contains the pixel: the cull's chunk → page → group → record descent, scanned
from the top, with one extra mask per candidate group (the group's eight `KF_NOHIT` bits gathered from the kind column
by one multiply). The dispatch target is the nearest ancestor-or-self flagged `KF_INTERACT`, read off the depth
column in one backward pass (`flagged_chain`). There is no hit index, so a scroll (page offsets), a drag or a tooltip
rebuilds nothing: `hit_rebuilds` is 0 by construction.

**Interaction state lives in the kind column** (its top three bits; the façade's record kinds use the low five):
`KF_INTERACT`, `KF_FOCUS`, `KF_NOHIT`. Flags move with their records through splices and page splits for free.

**The hit cell.** Between two pointer events the topmost record is constant inside its visible ink minus every later
record over it; each obstacle is removed by the single cut that keeps the most area (`hit_cell`, budget 256, else the
1-pixel cell, always valid). Validity costs no hook and no flag: every geometry, structure, clip and scroll change marks
its old and new support in the core damage ring, so the cell is valid iff no core plane since it was cut touches its
tiles (`cell_valid`; paint changes do not invalidate it). A move inside the cell is one compare.

**Hover is a paint write.** A crossing diffs the old and new dispatch chains (enter/leave = the symmetric difference)
and writes the colour words of the records entered and left (`hover_bg`, `paint_with`): the paint ring, content
addressed, so moving back to an earlier hover state is served by a surface that already holds it (gate
`hover_toggles_are_served_by_held_surfaces`). Typed `on_hover` routes run for the same set. A colour wire on a hovered
record stores the base without writing the instance (one flag test in `ApplyBg`).

**Routes are typed.** `on_click(|m: &mut V| ..)`, `on_press`, `on_hover`, `on_key`, `on_drag` lower to a trampoline
monomorphised for (owner, closure) plus the closure's bytes (≤ 16) in an act table indexed by the record's handle;
after a route runs the turn settles QUEUED signals as any handler turn does. Activation follows zgui and the DOM:
press focuses the nearest focusable record of the chain, release over the pressed record activates.

**Focus order is preorder.** Tab and Shift+Tab are a scan of the kind column for the next `KF_FOCUS` bit (64 lanes
per page in four NEON `vtst`s, padding never matches). Keyboard focus shows one ring record at the end of the paint
order, moved to the focused element (two ink writes); pointer focus shows none (`:focus-visible`).

**A keystroke is a label patch plus a caret move.** A text field's value is the owner's `Signal<String>`: an edit
mutates it (QUEUED), the settle runs the label's wire (bough-text patches the glyph slots from the first changed byte
and marks only their tiles) and then the caret's wire (installed first, so it runs last: one quad moves to the pen of
the caret's slot).

**Wheel and drag write signals.** A wheel notch aims a glide (zgui's tween) on the nearest scroller of the chain;
its ticks write the scroller's `Signal<f32>`, whose wire moves a virtual list (`region_scroll`) or a scroll view
(`scroll_y`: one space write, one `scroll_run` over the content's records, and the content's own clips moved with
it). A drag captures the pointer: its route gets the travel since the press with no hit test. Trackpad deltas move
the scroller directly.

**Tooltips are clips.** A tooltip's bubble is mounted once, pointer-transparent, behind an empty clip; hovering its
trigger sets the clip (one write; damage = the clip's xor) and fades it in over 150 ms (alpha writes on two records per
tick, zgui's surface fade), and leaving hides it at once (zgui hands tooltips over instantly). No mount, no rebuild.

**Panels and logs.** A wired clip (`clip_by`, `wired_clip`) is a clip id whose rect a signal sets: the page panel of
the shell. A port list (`port_list`) is a fixed-row virtual list whose port's top edge a signal moves: the mounted rows
follow (a space write per row, one `scroll_run`), the port clip moves, and rows that enter or leave the port are
mounted or unmounted (zgui's "the log's newly exposed lines built"). An `offset` group is a subtree in its own space
following a `(dx, dy)` signal: a drag preview is one space write and one `translate_run` per move.

**Templates.** A document known at compile time is a `&'static [TNode]` in preorder (`template`): mounting is one pass
over constants with no sheet to parse and no cascade; a hook mounts the nodes the application owns with typed
builders. The gallery and the shell are generated at build time from zgui's own laid-out trees.

**Resize** sets the damage rings' window, the tile grid and every layout group laid out at the window's size, which
cephal re-lays out through its incremental path (the root's query changed); every surface is repainted whole. Frame
slots are reserved for the tallest window (4,096 px), so a resize moves nothing in the region.

**Cold start.** The font's simple-script build answered every GPOS pair by one lookup walk per pair (~72 K probes,
113 M of a 170 M cold start). `bough-text/src/script/pairfast.rs` evaluates each pair subtable once per glyph
(coverage, classes, pair set) and combines per pair from small arrays: 113 M → 22 M, the same table (debug builds
assert equality with the probe for every pair). The gallery's cold first frame went from 170 M / 13–18 ms to
76 M / 6–10 ms; startup_bench from 198 M (a loss to zgx's 185 M) to 110 M.

**Windowed input** (`backend/host.rs` `run_interactive`): an AppKit local event monitor turns mouse, wheel and key
events into input turns, each followed by a frame; the display link ticks glides and fades and pauses when nothing
animates. Accessory activation, mouse-moved events accepted, no focus taken.

## 3. Correctness gates

| gate | where | result |
|---|---|---|
| `hit_top` and dispatch chains == brute force over random trees (30–2,000 records, clips, page offsets, NOHIT, edits) | `bough-core/tests/hit.rs` | equal (60 seeds × 6 rounds × 400 points) |
| every pixel of every hit cell answers like its seed pixel | same | exhaustive per cell |
| a cell judged valid after random edits (insert, remove, translate, scroll, ink, clip) still answers the same everywhere | same | exact; both outcomes exercised |
| tab order forward/backward, first/last flagged == oracle | same | equal |
| hover sets and the enter/leave routes == oracle (the symmetric difference of the old and new interactive chains of the brute-force topmost record), 3,000 moves over nested, overlapping, overflowing and pointer-transparent records, with window leaves | `bough-zgui/tests/input.rs` `hover_enter_leave_sets_equal_the_oracle` | equal at every step |
| presented == full redraw after every interaction, ring ages 1–4 and the windowed ring; retained == fresh mount | `bough-zgui/tests/input.rs`: hover crossings and leave, hover toggles via held surfaces, list glides, the gallery (22 tabs, typing, caret moves, Home/End/Delete/Backspace, clicks, root glides moving clips and the ring, animations), label churn, resize relayout, shell tooltips (fades) and the dock drag | 0 differing bytes |
| 3b: laid-out gallery (interactions, animations incl. the turning spinner, 32-width relayout) and shell (drag, 40-size resize), thread rewrap at 32 widths, caret blink on held surfaces, nested views and tooltips under an outer scroll | `bough-zgui/tests/input.rs` | 0 differing bytes; retained == fresh at each size |
| 3b: gradients vs CSS, shadows vs an integrated Gaussian and nothing outside their ink, icons vs resvg, a quarter turn vs the rotated raster, bold / mono / 120 px glyphs vs their own font's raster; each moving or turning at ring ages 1–4 + windowed | `bough-zgui/tests/drawing.rs` | within 0.9 / 1.65 / 1 / 2 / 0 levels; 0 differing bytes |
| 3b: the overlapped cold start presents the serial start's pixels | `bough-zgui/tests/first_pixel.rs` | equal, both == full redraw |
| GPOS pair table == per-pair probe | `debug_assert` in `SimpleScript::build`, every pair of every font the bough-text tests load | equal |
| existing gates | `cargo test --workspace --release` | all green; STATUS §1.1 slopes unchanged within noise (round 3: canonical's own binaries heavy stream 179.1 K, this branch 178.0–181 K; 3b: heavy stream 178.6 K RENDER=0 / 376 K RENDER=1, scroll 25.9 K, both 213 K at load ~110) |

## 4. What was tried and rejected

* **Per-row hover signals** (`hovered: Signal<Option<u32>>` with a projection per row): one crossing runs every row's
  wire, O(rows). Hover as a paint write on the two records is O(1).
* **A hit index** (a BVH or grid next to the tree): it must be rebuilt or patched on every scroll, drag and mount.
  The Bough's page summaries already are one.
* **Target-level hit cells** (constant dispatch target rather than constant topmost record): bigger cells, but the
  computation needs a forward scan of the target's subtree; the record-level cell is one range scan with the same
  summaries and is exact.
* **Pre-mounting the dock's log for the panel's largest height**: no mounts during a drag, but it is not what zgui
  does; the port list mounts the lines a drag exposes.
* **Swallowing content-area events after dispatch** (returning nil from the monitor so AppKit skips the responder
  chain): measured worse (139 K vs 113 K main-thread instructions per move without frames).
* **The pre-armed stream for input frames**: 199–208 K vs 199–223 K per move; marginal, and +1 frame of latency.

## 5. Gaps (honest)

* **Fidelity of the zgui documents.** The laid-out gallery and shell (`GalleryL`, `ShellL`) are zgui's trees with
  zgui's computed declarations, laid out by cephal; there is no selector cascade (the declarations come resolved),
  inline formatting is approximated by blocks, and transforms other than the spinner's turn, gradient-filled text
  and non-uniform borders are not drawn (1.4). The `*-dump` variants keep the round-3 absolute geometry.
* **The cold start's wall time is the Metal device.** Overlapping halves the main thread's instructions, but the
  first pixel waits for the device's creation (1.4).
* **The drag probe ties** (round 3, unchanged): ~200 K main-thread instructions per synthetic move in both
  frameworks, most of it AppKit's dispatch and the presenter's main-thread publish (floor agent's area now).
* **Windowed resize** of the GPU surfaces: floor agent's area now.
* **IME** is limited to committed text (`text_input`).
* **resize-slope** is cephal's cost (4.7 K of 5.1 K instructions per box), used unmodified.
