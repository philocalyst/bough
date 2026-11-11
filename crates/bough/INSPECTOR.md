# The inspector, and what the API made harder (round 4, input agent)

`comparisons/bough-zgui/src/inspector.rs` is an ordinary bough app written against the public API only
(`bough::prelude`, `bough::inspect`). It is opened on another app (`&App<T>`) and today does:

* **pick by pointer**: `App::pick_record(x, y)`, which is the target's own hit testing (`Bough::hit_top`);
  transformed records answer through the inverse transform;
* **record tree** around the pick: its ancestors, then the records after it, each with depth, kind and
  position;
* **the picked record**: kind, ink, layout box, clip, and whether it is in a transformed subtree;
* **overlay** of the target window at 1/4 scale: the picked ink (red), its layout box (blue), and every
  tile the target's last frame repaired (orange), refreshed after each target frame;
* **state panel**: a list of `(name, value)` rows. It waits for the api agent's typed reflection, which
  supplies a tree of `(name, offset, TypeId, format/edit vtables)` nodes, and edits will go through the
  framework's `Edit` vtable, so they flow through ordinary wires and damage. No reflection was built here.

Gate: `tests/inspector.rs`. It picks the gallery's "Button" heading, checks the overlay pixels and that the
tree marks the pick, hovers a swatch in the target and checks that only that tile is marked. The
inspector's own frames must equal a full redraw.

## Friction log

Each entry says whether I fixed the API (it was mine) or who it is reported to.

| # | friction | resolution |
|---|---|---|
| 1 | There was no way to ask "which record is under this point". Worse, the obvious name `App::pick` was already taken by the frame's private surface pick. | **Fixed.** `App::pick_record(x, y)` (bough::inspect) picks through hit testing. |
| 2 | A record's layout box was not reachable. Layout nodes are keyed by the record's handle index, and their position is group-local plus the space's translation. Both facts were internal. | **Fixed.** `App::record_view(pos)` returns depth, kind, absolute ink, handle, clip, absolute layout box, and the transformed flag. |
| 3 | The last frame's repaired tiles were not exposed. When I exposed the row masks, a full repaint (a fresh surface) left them stale, so the overlay showed nothing for that frame. | **Fixed.** `App::last_repair()` returns the row masks, or every tile when the frame was full. |
| 4 | The kind byte's meaning (quad, text, box; interactive, focus, no-hit bits) had no public names. | **Fixed.** `RecordView::kind_name()`. |
| 5 | A quad has dynamic `width` and `span` (y, h) operands but no dynamic x. Moving an overlay box takes an `offset` group around it. | Reported to **api**: a rect operand (`.rect(sig)`), or `.at(sig)`. |
| 6 | A variable-length list driven by state (the tree rows) had to be a fixed pool of `TREE_ROWS` text elements, each projecting element `i` of one `Signal<Vec<String>>`. Any change re-runs every row's wire. | Reported to **api**: List / keyed rows from a collection signal. With virtualization-as-damage this becomes ordinary. |
| 7 | Transitions are identified by lowering order. A quad lowers its colour, then its width, so "the width transition" is id 1. | For **api** (reflection): a transition should be reachable by the property it drives. |
| 8 | Two apps in one process work headless (the inspector reads `&App<T>`). The windowed host runs one `App` per run loop, so there is no way to open the inspector's window next to the target's. | Reported to **floor** (vendored winit host): multi-window hosting. |
| 9 | `sig!` operands are owner fields, so all inspector state of variable shape is packed into `Vec` signals. | Same as #6. |
| 10 | Reading the target between its frames needs `&App<T>`. Writing to it (editing state) will need `&mut App<T>` plus reflection's `Edit` vtable. | Pending reflection (**api**). |
