# gpui-kit port of the sheet and canvas scenes: features used and deviations

Files: `overlay/apps/gpui-kit/src/main.rs` (upstream's, plus `mod scenes;` and the pre-dispatch
`scenes::from_env()` before `cli::from_env`), `src/scenes/{mod,sheet,canvas}.rs`. `Cargo.toml` is upstream's
(gpui-kit 0.9.1, GPUI `gpui-pre` 0.3.12, wgpu renderer), unchanged.

Module layout: the spec is included from `scenes/mod.rs` (`#[path = "../../../shared/scenes.rs"] mod spec;`), because
`main.rs` already owns the name `scenes` for the port's own module; `scenes::from_env` and `scenes::Scene` are
re-exported from it.

## What was used for each part

Common
* Each scene is an entity (`Sheet`, `Canvas`) that implements `Render`; the window is opened with
  `gpui_kit::open_window` as in the other scenarios (server decorations, same app id).
* State (theme, hot column or component, edit or move) is view state. Every change goes through `cx.notify()`; the
  next render describes the tree again from the state. Nothing is cached or patched by hand.
* Keys: `actions!` plus `KeyBinding::new("t" | "e" | "m", Action, Some("Scene"))`, a `key_context("Scene")` root that
  holds a `FocusHandle` (`track_focus`, focused at construction), and `on_action(cx.listener(..))`, as the table
  scenario does for F1 to F8.
* Hover: `.on_hover(cx.listener(..))` on a `Stateful<Div>` (an element with an id). Entering sets the view's hot
  column or component to the element's index; leaving clears it only if it is still that index (so moving between
  neighbours is order independent). Both set the state and call `cx.notify()`; every cell or instance that depends
  on it reads the state when the view renders again (nothing is restyled with `.hover(|s| ..)`, which could not drive
  thousands of elements elsewhere in the tree).
* Layout: `div()` flex layout (`flex_col`, `flex_1`, `min_h_0`, `h_full`, fixed `px(..)` sizes); the panels keep
  their sizes and the remaining area takes the rest, so the resize probe needs no code.
* Font: `font_family("sans-serif")` on the root (the spec's `FONT_FAMILY`), sizes with `text_size(px(..))`.

Sheet
* The body is `uniform_list("sheet-body", rows, cx.processor(..))` with `track_scroll(&UniformListScrollHandle)`,
  as `virtual_rows` does: only the visible rows (about 33) are built on each render, at 1,000 and at 10,000 rows.
* A body row is a flex row of the gutter div and 10 cell divs (fixed 110 x 22, `bg` from `spec::fill(theme, hot, r)`);
  the header is 10 hoverable divs with ids; the footer shows the column totals; the toolbar is two absolutely
  placed texts.
* The selected cell (0, 0) is `.border(px(2.)).border_color(SELECT)` on its div (border-box, so the border is
  inside the 110 x 22 rectangle).
* Totals: the 10 column sums are computed once at construction; the edit adds `EDIT_DELTA` to its column's total.

Canvas
* The design surface is a tree of absolutely positioned divs, which is how GPUI's own examples draw such things
  (`examples/shadow.rs`: `.shadow(vec![BoxShadow::new(..).blur_radius(..)])`; `examples/gradient.rs`: `linear_gradient`
  as a background; `examples/opacity.rs` and the like). GPUI's `canvas(prepaint, paint)` element with `paint_quad`
  and paths is what `examples/painting.rs` uses for free-form drawing; it was not used, because every node here is
  a plain box and divs are what a GPUI user would write for it.
* Frame: `div().relative().rounded(6).bg(..).shadow(..).overflow_hidden()` with its nodes as absolute children;
  the title is a text above it. Shadowed node: `rounded(r).bg(fill).shadow(..)`. Gradient: `bg(linear_gradient(180.,
  linear_color_stop(top, 0.), linear_color_stop(bottom, 1.)))` (180 degrees is top to bottom). Bordered:
  `bg(rgba(fill << 8 | 0x66)).border_1().border_color(..)`. Label: `text_size(10)` text, `whitespace_nowrap`.
  Instance: `rounded(4).bg(comp(..)).border_1()` with `0x33000000`. Group clip: an absolute `overflow_hidden()` div at the
  group's rectangle holding the node.
* The layers panel is a second `uniform_list` (5 + 20 x 26 = 525 rows at 5,000 nodes) with its own scroll handle;
  component rows have ids and `on_hover`. The inspector is a column of flex rows.

## Deviations

1. **Selected node drawn last (z-order).** The spec draws every node in index order, later on top, and the selected
   node is node 0 of frame 0, so it is the bottom-most node of its frame. Under the 49 (1,000 nodes) or 249 (5,000)
   nodes drawn after it, it is hidden almost entirely: at 5,000 nodes the `m` probe's region (frame 0) did not change
   by 0.1 % of its pixels, and the `edit` probe timed out on all 4 presses (`timeouts` 4, 0 latency samples). With the
   selected node moved to the end of frame 0's children (`canvas.rs`, `Canvas::frame`) the probe measures normally
   (the work per frame is the same: same nodes, same count). This is a flaw of the spec for large node counts (the
   selected node should be on top, as in a design tool); the other ports will see the same unless the spec changes.
   To restore the spec order, delete the `if f == 0 { .. }` block in `Canvas::frame`; the cell at 5,000 then reports
   a failed edit probe.
2. **Shadow blur.** GPUI's `BoxShadow::blur_radius` is the standard deviation of the Gaussian (the wgpu shader uses it
   as sigma and reaches 3 sigma); the spec's blur radius is the CSS one (sigma = half of it). The port passes `blur / 2`
   so the shadow looks like the spec's. With the spec's number as sigma the frame shadow reached the layout check
   point (1013, 400) and the check failed (`#dbdbdb`, not `#e5e5e5`).
3. **Rectangular clips.** GPUI's `overflow_hidden()` clips to the element's rectangle (a content mask), not to its
   rounded outline; nodes inside a frame are clipped to its rectangle, so at the frame's rounded corners a node can
   show past the curve (up to 6 px at a corner). The spec allows this ("a rounded clip where the framework has one").
4. **Selected node outline.** GPUI has no outline property; the 1 px outline 2 px outside the node is a child div
   (absolute, inflated by 3 px, `border_1()`, radius 9), painted over the fill.
5. **Bordered nodes.** The 1 px border is painted by GPUI at full colour over a fill at 0x66 alpha (the border is
   inside the box, like the spec's). GPUI draws border and fill in one quad; the border is not blended with the fill
   the way an extra stroke would be.
6. **Scrolling.** Both lists use GPUI's default scroll handling (not the 1:1 `scroll_by` override that the `rows`
   and `virtual` scenarios install to compare input distances); the scroll probe measures the default glide. There
   is no scrollbar (a bare `uniform_list` draws none).
7. **Whole-view re-render.** The canvas is one view; a theme, hover or move change re-describes the whole tree
   (all frames and nodes). Splitting frames into cached entities (`entity.cached(..)`, as the table's `fine` variant
   does for rows) would let `m` rebuild only frame 0, but it is not what the spec's hover and theme (which touch every
   frame) reward, and it was not tried. The sheet is the same: one view, the visible rows rebuilt per notification.
8. **Selected cell text.** The selected cell has a 2 px border, so its left padding is `TEXT_PAD - 2` to keep the
   text at 6 px from the cell's left edge.
9. **Not in the spec, chosen:** the header row right of column J is CHROME; the layers panel's frame rows start at
   x 12 (component rows: swatch at 12, name at 32); the inspector is 260 px wide (window width minus 1020, fixed), so on
   a narrower window it follows the right edge only as far as the layout allows; the frame title is a 16 px tall box
   above the frame with 11 px text.
10. **Per-frame count below 25.** With fewer than 25 nodes per frame the layers panel lists only `per_frame` layers
    per frame (the spec says the first `LAYER_ROWS` nodes); not reachable at the sizes the cells run.

## Host notes (not deviations)

* Every probe of the dev cells except `idle`, `scroll` and some `edit` / `theme` runs is flagged `valid=false` by the
  runner's `wake_lateness` check (35 of 4,913 output wakes later than 2 ms); the reports are written and `done` holds all
  measurements. ilo-prod has no GPU (the reports list an empty GPU and driver), so GPUI renders in software; the
  canvas at 5,000 nodes costs about 1 s of CPU per frame.

**Spec amended (lead, after the ports):** the selected node is now drawn last in frame 0 by the spec itself, so this is no longer a deviation; all four ports draw it last.
