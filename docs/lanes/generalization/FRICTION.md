# gui-bench application friction

Baseline aa9943e7ae; findings from source inspection unless a report records a reproduced failure.

| ID | exact desired expression / behaviour | obstruction | smallest escape planned |
|---|---|---|---|
| G1 | `Win::demo(view).click(row).press(Key::F(1))` | No prelude headless host or pixel API (`lib.rs::prelude`). | Harness only: `bough::raw::{App, Config, Caps, Button}`; input methods shared with host, ordinary re-description retained. |
| G2 | `list(sig!(self.rows))` with row `.on_click(Row::select)` | `input.rs::root_owner_only` panics for row/entity owner classes. | Root `.on_press` records row index from shared fixed-height geometry and scroll; root `.on_click` edits selected slots. This delegates selection at root, and is labelled, not a native row-route result. |
| G3 | `.on_key(... Key::F(1) ...)` heard with no focused widget | Key has no F variant; `key_down` only dispatches along focus chain. | Current AppKit host delivers F1–F8 as `Key::Char` private-use Unicode U+F704–U+F70B. Root route matches these. Focus root during host startup with `set_focus`; no focus ring. Native event mapping remains to be verified. |
| G4 | `Application::run(Window::document(view))` plus initial root focus and bounded smoke-run exit | Window/Application expose no initialization or exit hook. | `bough::backend::host::run_interactive` (not re-exported by raw), with identical default application Caps, same view, and startup focus only. No custom event/update path. |
| G5 | `Application`'s default config in the headless harness | APP_CAPS and default_font are private; Config::new uses smaller defaults. | Mirror APP_CAPS exactly, assert against source in provenance checks; load the same env-selected/default Arial font. Report capacities. |
| G6 | measure current RSS/footprint and instructions | No prelude metric surface. | Harness-only `bough_metal::perf::snap`; no application state or view uses it. |

The comparison is a standalone workspace to respect lane ownership of the root manifest. Root integration is a
lead task; its existing workspace check does not cover this crate, so the crate gets its own locked check/tests.
Shared spec.rs and cli.rs retain the gui-bench source bytes and provenance. `resize` is absent from upstream cli;
the port will add an explicit alias in its own argument adapter, leaving the shared file unchanged.

No framework files changed at this stage. A plain Vec insert necessarily moves its tail: report its application
cost against the stronger structural ideal instead of attributing that entire cost to the renderer.

## Completion-stage escapes (registered before use)

| ID | desired code | obstruction | smallest escape |
|---|---|---|---|
| G7 | `timer.every(16ms, Self::tick)` owned by the view | no prelude timer registration | host adapter uses the same NSTimer/main-loop mechanism as backend::host; calls App::turn and frame_paced. Headless invokes the identical clock delivery function. |
| G8 | `block(24,24).rounded(3).rotate(angle).repeat(1s)` | flow boxes have translate/opacity, no rotation or repeating animation | raw anchor + svg_image.angle(signal); square SVG matches spec. Timer advances angle; this is explicitly a 16ms timer fallback, not native display-linked analytic animation. |
| G9 | `virtual_rows(count, row)` in the prelude without allocating a million values | list() requires List<T: View>; index_list and its Mounter builder are raw | raw index_list with prelude-built rows; same existing exposure engine, estimated 32px height, no custom virtualization. Pointer scrolling is supplied by prelude .scrolls with exact content extent. |
| G10 | `text_input(...).focus_on_mount()` | no prelude focus request | focus the mounted field through App host setup; headless uses the same method. No global synthetic input. |
| G11 | `app.scale_changed(2.0)` | App::resize changes dimensions only; no live-scale entry point | report scale switch unsupported. Do not replace with a new mount. |

Timer FFI safety follows existing host.rs: AppKit main-thread registration, a block owning an Rc of the live app,
run-loop-owned timer, no cross-thread mutation. Bounded real-window timer smoke asserts at least one callback;
that is the independent lifetime/main-thread integration check for this adapter.

## Round-6 completion stage

| ID | desired expression / behaviour | obstruction | what the suite does |
|---|---|---|---|
| G12 | a view tuple of nine or more children | `Element` is implemented for tuples up to arity 8 | nest tuples |
| G13 | `div().scroll(..).child(list(..))` as the whole window | no rows are mounted when the port is at the window origin (OVERFIT finding 7) | an 8 pt block above the port in the adversarial list views; the gui-bench table has its header there already |
| G14 | `each(rows.iter().map(row)).key(|r| r.id)` | `each` items have no key; `Element::dyn_key` is not reachable from the prelude | positional re-description, measured as such |
| G15 | a description of dynamic depth (`Box<dyn Element>` or a recursive element) | no type-erased element; a recursive tree is only expressible as `List` rows owning lists | static nesting by macro (`--features deep`) and the list form |
| G16 | `app.reclaim()` or reclamation inside input turns | `Rt::reclaim` runs only in `App::turn` and idle frames | none in measured cells; `GUIBENCH_IDLE` as a labelled diagnostic |
