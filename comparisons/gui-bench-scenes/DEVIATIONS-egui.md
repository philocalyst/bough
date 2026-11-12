# egui port of the heavy scenes: deviations and the features used

Files: `overlay/apps/egui/src/main.rs` (upstream's plus the dispatch), `overlay/apps/egui/src/scenes/{mod,sheet,canvas}.rs`.
`Cargo.toml` is upstream's, unchanged (eframe 0.37 and fontique only; no `egui_extras`).

## Framework features, per part of the spec

* Window, font, input path: upstream's `window::run`, `font::definitions` (system sans-serif first) and the
  1:1 wheel scroll (`scroll::Scroll` + `wheel`, fed from `App::raw_input_hook`), unchanged, so the scenes scroll as the other egui
  scenarios do. The scenes use the root `Ui` that `eframe::App::ui` receives (eframe 0.37).
* Panels: eframe/egui 0.37 has no `TopBottomPanel`/`SidePanel` any more (they became `egui::Panel::top/bottom/left/right`, shown with
  `.show(ui, ..)` on the root ui). The toolbar, header, footer, layers panel and inspector are `Panel`s with `exact_size`, a flat
  `Frame::new().fill(CHROME)` and no separator line; the sheet body and the canvas are the `CentralPanel` (`no_frame`).
* Sheet body: `ScrollArea::show_rows` (egui's own virtualisation), not `egui_extras::TableBuilder`, because the app's Cargo.toml has no
  `egui_extras` and the brief says to prefer what is already there. `show_rows` needs one row height, which the sheet has (22 with
  `item_spacing` 0). Each visible row is a `ui.horizontal` of cells; a cell is `allocate_ui_with_layout` (centred left-to-right) with a
  painter fill and a `Label` (`RichText` size and colour). Only the rows in the viewport are built, each pass.
* Sheet hover: the header cells are `allocate_exact_size(.., Sense::hover())` and `Response::hovered()` sets the hot column, which the
  body (built later in the same pass) reads. The selection border is `Painter::rect_stroke(.., StrokeKind::Inside)`.
* Keys: `ctx.input(|i| i.key_pressed(Key::T / E / M))`.
* Theme: one `Visuals` per `Theme` (`ctx.set_visuals_of`, egui's light/dark with the spec's chrome, ink and extreme fill colours) and
  `ctx.set_theme`; the body colours come from the spec's tables, indexed by `ctx.theme()`.
* Data: the app owns plain data (the sheet's values and per-column totals, the canvas's node list built once at start). The `e` edit
  changes the value and recomputes that column's total (one column sum on the key press, not per frame); the `m` edit flips a bool.
* Canvas: `Panel::left` (layers, rows in `ScrollArea::show_rows`, row hover through `Response::hovered()`), `Panel::right` (inspector),
  `CentralPanel` with `ui.painter_at(rect)`. Frames and nodes are `epaint` shapes: `Shadow::as_shape` (a blurred `RectShape`) for the frame and
  `Kind::Shadowed` shadows, `RectShape` with rounding and stroke for frames, bordered boxes, instances and the selected node, a `Mesh` with
  vertex colours for gradients, `Painter::with_clip_rect` for the frame clip and the group clips, `Painter::text` for labels.

## Deviations from the spec

1. **Rounded clip**: egui clips to rectangles only. Nodes are clipped to the frame's rectangle, not its 6 px rounded rectangle, so a node
   touching a frame corner can show outside the rounded corner (at most 2 px at the corner).
2. **Shadows**: egui's `Shadow` is a blurred rounded rectangle (`blur` is a `u8`, offset an `i8`, so fractional values truncate; the spec's
   offsets and blurs are whole numbers). Its falloff is not a gaussian of the given radius; large blurs (up to 40 px) show a wider, slightly different
   penumbra and faint streaks near sharp corners. The shadow of a node is clipped to the node's frame/group clip, as the node is.
3. **Gradient**: interpolated in gamma space (vertex colours of a two-triangle mesh), as egui does everywhere.
4. **Selected node's outline**: a 1 px stroke centred 2 px outside the rect (`StrokeKind::Middle` on the rect expanded by 2, radius 8); the
   spec does not say whether "2 px outside" is the gap or the outer edge.
5. **Module name**: the shared spec is `mod heavy` in `main.rs` (`#[path = "../../shared/scenes.rs"]`), not `mod scenes`, because
   `src/scenes/` (the brief's file layout) is this app's own module.
6. **Text**: labels are egui `Label`s / `Painter::text` in the system sans-serif; the vertical placement is egui's (centred in the cell by the
   layout, or `Align2::LEFT_CENTER`), which may differ by a pixel from other renderers. The canvas frame titles use `Align2::LEFT_TOP` at
   `frame_rect.y - 16` (the spec's "baseline area" is ambiguous).
7. **Scroll bars**: egui's solid scroll bar (6 px bar, 4 px margin) takes its width from the scroll area; in the layers panel it sits inside
   the panel's right edge (the rows are 10 px narrower); in the sheet it is at the window's right edge over PAGE, as the spec allows.
8. **Layers panel width**: egui's `exact_size` for the inspector is computed from the window width (`max_rect.right() - 1020`), so it follows
   a resize as the spec asks.

## Spec issues found

* **Canvas edit probe at 5,000 nodes (250 per frame) measures nothing.** The selected node (node 0 of frame 0) is drawn first and "later
  on top", so at 250 nodes per frame it is covered completely (confirmed on the screenshot) and moving it changes fewer than the
  probe's 0.001 x ROI pixels. The probe waits its 5 s timeout on every repeat and writes a report with no latency samples
  (`edit.json`: `records` is set, no `latency`); the cell still lists `edit` under `done` and `errors` is empty, so only the missing latency
  shows it. The egui port follows the spec (index order); any port would show the same. A fix in the spec: draw the selected node (or just
  its outline) last, as a design tool draws a selection.
* `valid=False failed_checks=['wake_lateness']` appears on most probes of every cell (fakeland counts wake-ups later than 2 ms on a loaded
  host; egui under llvmpipe uses 50 to 480 ms of CPU per frame). The latencies are still reported.

**Spec amended (lead, after the ports):** the selected node is now drawn last in frame 0 by the spec itself, so this is no longer a deviation; all four ports draw it last. The egui port was changed by the lead to draw it last (`canvas.rs`: the frame loop draws the `Selected` node after the others); the 5,000-node edit probe then measures (122 ms total p50 in a quick cell).
