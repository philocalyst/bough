# iced port of the heavy scenes: deviations and features used

Files: `overlay/apps/iced/src/main.rs` (upstream's plus the `scenes` spec module and the pre-dispatch; the apps'
module is `heavy`, mapped to `scenes/mod.rs`, because the spec's module already takes the name `scenes`),
`overlay/apps/iced/src/scenes/{mod,sheet,canvas}.rs`. `Cargo.toml` is unchanged (iced 0.14.0 with `advanced`,
`canvas`, `lazy`, `tokio`; the build resolved iced 0.14.2 widgets). Both scenes are Elm applications
(`iced::application(boot, update, view)`), reuse the existing app's undecorated 1280x800 window (`page::window`),
`rows::scrollbar()` (6 px bar in its own 1 px gutter), and the `Wheel` wrapper (wheel detents to pixels).

## Common

* Theme: state holds `theme: usize`; `Application::theme` returns `Theme::Light`/`Theme::Dark` from it, and
  `Application::style` the page colour. Fills and inks come from the spec by index, through `container::Style`
  closures that capture the state's values (`style(|theme| ...)`); no widget reads iced's palette.
* Keys: `keyboard::listen().filter_map(...)` (as the table app binds F1-F8) turns `t`, `e` and `m` character
  presses into `Message::Key`. Modifiers are ignored.
* Hover: `mouse_area` `on_enter`/`on_exit` messages. `on_exit(c)` clears the hot item only if it is still `c`,
  because moving to a neighbour can be heard as enter-new before exit-old.
* Fonts: iced's default font (system sans-serif through cosmic-text; DejaVu Sans on ilo-prod). No font is loaded.
* Text weight/metrics differ from the other frameworks, as the spec allows.

## Sheet

* Body: `scrollable(Column::with_children(all rows))`, the plain way of the table app's coarse variant. Not
  `lazy`/`keyed_column`, and not the windowing trick of upstream's `virtual_rows` (`on_scroll` plus spacers). Reason:
  the brief asks for a scrollable of a column of rows, and iced has no virtualised list in its core. Every message
  (hover, `t`, `e`) re-runs `view`, which rebuilds every row of every cell (110,000 widgets at 10,000 rows) and iced
  diffs it. That is where the latencies come from (hover/theme/edit about 165-185 ms at 10,000 rows, against about
  30 ms at 1,000). A `lazy` row keyed by (theme, hot column, edit version) would only help `e`; hover and `t` change
  every row's dependencies, so it was not used.
* Cells are `container(text)` with a fixed size, padding `TEXT_PAD`, vertical centre, background and the 2 px
  `SELECT` border (`Border` inside the container's rectangle) on (0, 0). The model keeps each cell's formatted
  text, updated when `e` edits the cell, so `view` borrows strings instead of formatting 100,000 numbers; the totals
  are kept and adjusted by the delta of the edit.
* The scrollbar is `rows::scrollbar()`: it reserves a 15 px gutter at the right edge of the body (over PAGE), so
  the body's rows are 15 px narrower than the window; the cells end at x = 1156 either way.
* The strip right of the last header cell is CHROME (the spec leaves it open); the body to the right of the cells is
  PAGE through the window background.
* Row height and column widths are exact (`Length::Fixed`), so the layout check and the probe regions match.
* Startup is long at 10,000 rows (the first layout of 110,000 widgets): the app's toplevel is mapped late in the
  startup report (about 10 s at 1,000 rows on a loaded host; not investigated).

## Canvas

* Choice: a stack of positioned containers, not a `canvas::Program` with `Cache`s. `canvas::Frame` can draw paths,
  gradients, strokes, text and rectangular clips (`with_clip`), but not a blurred shadow; a container's
  `container::Style::shadow` is a real one (blur radius and offset), so the spec's `Shadowed` nodes and the frame
  shadow are drawn as specified. Gradients are `Background::Gradient(Linear::new(Radians::PI))` (iced's angle 0
  points up; PI runs top to bottom). The cost is that there is no cache: any message rebuilds the whole tree
  (5,000 nodes are 5,000 `pin(container(space))` elements) and iced diffs and redraws all of it, where a `canvas`
  cache would keep the geometry when only the selection moves. The canvas variant with approximated shadows was not
  built.
* Positioning: iced has no absolute positioning besides `pin` (one widget at a point of its parent) in a `stack`.
  A node is `pin(container).x().y()`; a frame is a `container` (frame fill, radius 6, shadow `FRAME_SHADOW`) around
  a `stack` of its nodes at the frame's size; the surface is a `stack` of titles and frames at the spec's coordinates
  (window coordinates minus the panel and the toolbar).
* Clips: `Stack::clip(true)` on each frame and on each group (a 70 x 80 stack that holds one grouped node), so a node
  is clipped to its frame and a grouped node also to its group. The clip is rectangular: iced has no rounded clip,
  so nodes show square corners at the frame's rounded corners (6 px radius). A `Stack` draws its first child without
  a layer of its own, so each clipping stack starts with an empty base child to make the node layers clip.
  A node's shadow is clipped with the node (as the spec's "nodes clipped to the frame" implies).
* Z order (deviation): the selected node (node 0 of frame 0) is drawn last, on top of its frame, not first as
  "drawn in index order" has it. Reason: under the other 249 nodes of a frame at 5,000 nodes it is covered, `m`
  changes almost no pixel of the probe's region (`FRAME_*`), and the edit probe timed out on all 4 iterations in a
  first run with the spec's order (the 1,000-node cell passed either way). A selection drawn over its frame is also
  how an editor shows it. The inspector and the position are as in the spec. This should be settled in the spec for
  all frameworks (the probe cannot see a covered node).
* The selection outline: 1 px `SELECT_OUTLINE` border, radius 8, around the node with 2 px between (the ring is
  2 to 3 px outside the node's edge), a container with padding 2 around the node's fill (radius 6).
* `Bordered` nodes: `fill` with alpha 0x66 as the background and a 1 px opaque `Border` of `border` (inside the
  rectangle). `Instance`: radius 4 fill with a 1 px `0x33000000` border (iced draws `Border` inside, as the spec's
  "1 px border" asks). `Label`: a `text` of 10 px at the rectangle's corner, not clipped to the rectangle, wrapping
  off.
* Frame title: 11 px MUTED text vertically centred in a 16 px box whose top is at `frame_rect.y - 16`
  (the spec's "baseline area" is ambiguous).
* Layers panel: `scrollable` of a `Column` of 525 rows of 24 px (5 components, then per frame a "Frame n" row at
  x 12 and 25 "Layer g" rows at x 24; with fewer than 25 nodes per frame only those). Component rows are
  `mouse_area`s with `ROW_HOT` when hot. Swatch at x 12, name at x 32 (12 px swatch, 8 px spacing).
* Inspector: 260 px wide (to the window's right edge), 12 rows of 24 from y 56, label at x 1032, value at x 1140.
* Layout follows the window: the panels have fixed widths, the surface takes the rest, frames keep their
  coordinates (the surface clips).

## Things in the spec or scripts that got in the way

* The spec's `scenes` module name collides with the `src/scenes/` directory the brief names for the apps;
  `main.rs` maps `#[path = "scenes/mod.rs"] mod heavy;`.
* The edit probe cannot see a node that is covered (above).
* On a shared host the `wake_lateness` check fails in some measurements (other lanes building or measuring on
  ilo-prod); they still write reports. Numbers are from single quick cells (2 measured iterations).

**Spec amended (lead, after the ports):** the selected node is now drawn last in frame 0 by the spec itself, so this is no longer a deviation; all four ports draw it last.
