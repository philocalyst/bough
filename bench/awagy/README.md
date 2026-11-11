# areweactuallyguiyet.mespin.me on macOS

The site's two workloads (marc2332/areweactuallyguiyet, `apps/`) measured on macOS, because its harness is Linux-only
(X11, xdotool, MangoHud) and bough runs only on macOS (Metal).

* `comparisons/bough-awagy`: bough's scroll and dashboard apps, written only with `bough::prelude`.
* `tools/build_apps.sh <dir> <app>...`: builds the site's apps (cloned under `<dir>/src/repo`) with the harness's
  feature selection.
* `tools/present_observer.m`: replaces MangoHud. It is injected with `DYLD_INSERT_LIBRARIES` and timestamps every frame
  handed to the window server: a CAMetalLayer drawable taken, a `CGLFlushDrawable`, or an IOSurface set as layer
  contents.
* `tools/awagy_input.swift`: replaces xdotool. It runs the harness's input schedule:
  * the pointer is moved over the window, and wheel and clicks are real HID input routed by the window server;
  * keys are posted to the app's process only;
  * `activate` makes the app active and verifies its window is uncovered (covered windows are throttled, so their
    runs are rejected).
* `tools/run.py`: the site's protocol and `frames.py` window rules. CPU and memory come from `proc_pid_rusage`.
* `tools/gen_report.py` + `report_template.html`: the evaluation page (`results/bough-on-awagy.html`).
* `tools/sizes.sh`: the production-profile executable sizes.
* `apps/zgx`, `apps/bui`: two frameworks the site doesn't register, ported to the same two workloads and measured with
  the same protocol. **zgx** is Zeron's (github.com/zeronsh/zgx, pinned at `0f0a08af`, built as its
  getting-started guide says, including its `[patch.crates-io]` set). **BUI** is Blender's UI as a standalone library
  (github.com/ppmpreetham/BUI, pinned at `d6be0397`, crate `blenderui`); its apps are the site's own `apps/gpui`
  sources with the changes listed in the report. Build with `tools/build_zgx.sh <work>` and `tools/build_bui.sh <work>`
  (BUI needs `cmake`; set `CMAKE_BIN` if it isn't on `PATH`).
  * zgx doesn't present through CAMetalLayer drawables. It flips IOSurface tiles of a plain layer, and the observer
    counts `-[CALayer setHidden:NO]` on IOSurface-backed layers (`AWAGY_OBSERVE_TILES=1`, tag 4). This was checked
    against zgx's own `ZGX_PRESENT_LOG`: 319 of 319.
  * BUI has no GPU renderer on macOS: it draws a CPU raster into an NSView. The observer counts
    `-[NSView displayIfNeeded]` on its view (`AWAGY_OBSERVE_BUIVIEW=1`, tag 5). This was checked against an app-counted
    animation: 85.6 against 85–88 per second. BUI draws one frame per wheel event without waiting for the display, so
    its presents per second can exceed the refresh rate.
  * `tools/fill_missing.py` re-runs rejected (covered-window) repetitions slot by slot.

Results (2026-11-11, M3 Pro, 120 Hz) are in `results/`: `results.json` (bough, Vizia, Vizia optimized),
`results-extra.json` (zgx, BUI; `results-extra-contaminated-1.json` is a discarded batch that overlapped a build),
`sizes*.json`, `verdict.html` and the page `bough-on-awagy.html`. The page's first version said bough dropped 13 of 480
clicks in one run. That was wrong: the driver sent 467 in that run (`events_sent`), and bough counted all of them. `results-8fw-partial.json` is the interrupted run with eight
frameworks; bough, egui, iced and Slint are complete in it.
