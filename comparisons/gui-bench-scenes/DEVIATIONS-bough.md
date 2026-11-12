# bough in gui-bench: deviations and features used

## Sheet (`bough_workloads::sheet::Sheet`, the standing workload itself)

* The data (`spec::sheet::value`) and keys (`t`, `e`) follow the spec; the root holds an `.on_key` route and the
  window host focuses it (`bough-guibench-scenes/src/lib.rs`).
* Body: a flow `.scroll(..)` box with every row mounted (`each` over all rows): bough's flow surface has no
  virtualised list; its exposure-driven mounting (the dormant lane) is not on this branch.
* Every cell's fill and text colour read one `look` signal (theme bit | hovered column) through `.map`: a hover
  re-evaluates all 2 x cells wires (the fan-out), a theme switch changes them all.
* Column totals are kept by the edit handler (no derived values in bough).
* The scroll box shows no scrollbar.
* Application mode: re-described after every handler (no `Config::once`).

## Canvas (`comparisons/bough-guibench-scenes/src/canvas.rs`)

Verified locally by its author (headless bough, GPU offscreen, application mode and `once`, 1,000 and 5,000 nodes):
the layout-check points, the probes' regions, presented == full redraw. Then run by the lead under fakeland on
ilo-prod: the cells at 1,000 and 5,000 nodes pass the layout check and write every report.

### Features used for each part of the spec

* Toolbar, layers panel, inspector, root: flow layout (`row`, `column`, `div`, `text`, `block`), `.bg(sig!(look).map(..))`
  for the theme, `.w_pct/.h_pct` root, panels with fixed widths and a growing canvas box.
* Layers panel: a `.scroll(sig!(layers))` box (bough's flow scroll box; all 525 rows are mounted, there is no
  virtualised list in the flow surface), `.on_hover` on the five component rows, which sets the hot component.
* Hover fan-out and theme: one `Signal<u32>` `look` (theme bit | hovered component); every component instance's fill is
  `sig!(self.look).map(INST[k])` (1,000 wires at 5,000 nodes; 200 repaint per hover), plus the swatches, row backgrounds,
  frame fills, canvas background, chrome and label colours.
* Keys: `.on_key` on the root (the window host focuses it): `t` toggles the theme bit, `m` toggles the selected node's x.
* Move: the selected node is a flow box (`div().abs(..)`) with a `.translate(sig!(sel_x).map(..))` operand, and its
  outline another with the same operand; the inspector's x is `text(sig!(sel_x).num(..))`.
* Frames, nodes: raw elements at window coordinates (the spec's geometry is in window coordinates): `deco_box` for box
  shadows and the vertical gradients, `quad` for bordered boxes and instances, `clip` per frame and a nested `clip` per
  group (static, so nesting is exact), `text` (raw) for the frame titles.

### Deviations

* Frame corners: bough clips to rectangles, so the frame's nodes are clipped to the frame's rectangle, not its rounded
  rect; a node at a frame corner pokes past the 6 px radius.
* Frame fill and shadow: a `deco_box` always paints its fill and has no operands, so the frame is a transparent
  `deco_box` (its shadow only) under a `quad` that follows the theme.
* Shadow blur: bough's `Shadow::blur` is CSS's blur radius (2 sigma); the spec's blur radius (2 to 40) is passed as is.
  Shadows are `&'static [Shadow]`: a static table of the 39 spec shadows (offset y = round(blur / 4)).
* Gradient stops are `&'static`: a static table of the 64 palette pairs.
* Labels: a raw `text` has a constant colour, and the label's ink follows the theme, so labels are flow `text` boxes
  with a colour operand and `.abs(..)` (positioned from the canvas box's corner, which is at (240, 44)). Their 12 px
  line height is bough's (the spec gives the font size only).
* The selected node's outline: "1 px outline 2 px outside" is drawn as a 1 px bordered box 3 px larger on every side
  (radius 9), a ring from 2 to 3 px outside the node.
* Component instances use a 1 px border of 0x33000000 and radius 4 as specified; bordered boxes have radius 0 (the spec
  gives none).
* The application is described by the owner and re-described after every handler (no `Config::once`); the 5,000 nodes
  are re-described on each key and hover (stepped over where bough can: `deco_box`es are
  opaque to a re-description, quads and flow boxes are compared).
* Node list: `Element` is not object safe, so the mixed list of nodes is mounted through a small object-safe `Mount`
  trait (the `El` wrapper in canvas.rs); not an application-facing feature of bough.
* Text: bough's own font (`bough::ui::default_font`), not `sans-serif` by name.
* The selected node is drawn last in frame 0 (the spec, amended).
* Renderer: under fakeland bough draws with its CPU executor into `wl_shm` buffers (the platform lane's host), one
  thread; the other frameworks draw through wgpu or Skia on lavapipe, Mesa's software Vulkan, which rasterises on
  several threads. A full repaint of the canvas (theme: every frame; hover: the 1,000 instances spread over all 20
  frames) re-rasterises 5,000 nodes with blurred shadows on one core: about 1 s at 5,000 nodes, the same with
  `BOUGH_ONCE=1` (mount once), so the cost is the executor's, not the re-description's.
