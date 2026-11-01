# bough input: pointer, hover, focus, keys, text fields, drag, wheel, tooltips, resize

Branch `input` (worktree `bough-input`), round 3. Apple M3 Pro, macOS 15.7, `nightly-2026-10-17`, `dist` profile.
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
target/input/dist/zgui_scenario <idle|hover-storm|scroll|cold-start|kitchen-sink|app-shell>   # RENDER=1: GPU too
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
| GPOS pair table == per-pair probe | `debug_assert` in `SimpleScript::build`, every pair of every font the bough-text tests load | equal |
| existing gates | `cargo test --workspace --release` | all green; STATUS §1.1 slopes unchanged within noise (canonical's own binaries re-measured today: heavy stream 179.1 K, this branch 178.0–181 K) |

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

* **Fidelity of the zgui documents.** The table, list and probes are zgui's fixtures rebuilt exactly. The gallery
  and the shell are generated from zgui's dumped trees (preorder, rects, colours, borders, radii, text, tab stops,
  clips, animations), but with absolute geometry: **bough's cold first frame does no layout and parses no CSS**
  (zgui's does both: that is part of the 15×). SVG icons are tinted squares, gradients one solid stop, box shadows,
  non-uniform borders and transforms are absent, the spinner's rotation is a colour phase over the same box, text is
  one line per node in one face (display text over 60 px is set at 60 px: the label path caches at most 4 tile rows).
  `kitchen.resize` is reported as `kitchen.resize_repaint_only` and not compared (the template does not reflow);
  `shell.resize_step` and `thread.resize` were not ported.
* **The drag probe ties.** Both frameworks spend ~200 K main-thread instructions per synthetic move. bough's own work
  per move is ~6 K (the headless span). Measured by elimination (`BOUGH_NOFRAME`, `BOUGH_NOPUBLISH`): 113 K per move is
  AppKit's dispatch of a posted event (frames suppressed), ~45 K is the Metal encode and commit, ~55 K is the
  presenter's publish (the wait for the GPU and the Core Animation transaction that puts the IOSurface on the layer, on
  the main thread). zgx presents through CAMetalLayer drawables, whose presentation is scheduled by Metal rather than
  committed by a main-thread transaction. Closing the gap needs the presenter to publish without a main-thread
  transaction (drawables, or a render thread): bough-metal's area, not built.
* **Windowed resize**: `App::resize` is headless-complete (damage, tiles, layout, slots) but the presenter's
  IOSurfaces keep their creation size, so a window cannot yet grow past it. Not built.
* **Nested scroll views** do not follow an outer scroll's space (inner clips do); tooltips' rects are absolute (a
  tooltip inside a scrolled view would not follow); `clip_by` inside a scroll view is not registered with it.
* **Caret blink** is not built (the caret is solid while focused); IME is limited to committed text (`text_input`).
* **Cold start remains** ~22 M in the simple-script build and ~3.6 ms rasterising 1,150 glyphs on one core (the
  label path prepares 95 glyphs per strike); rasterising strikes on other cores while the tree mounts was not built.
* **resize-slope** is cephal's cost (4.7 K of 5.1 K instructions per box), used unmodified.
