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
