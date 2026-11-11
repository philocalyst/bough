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

Results (2026-11-11, M3 Pro, 120 Hz) are in `results/`. `results-8fw-partial.json` is the interrupted run with eight
frameworks; bough, egui, iced and Slint are complete in it.
