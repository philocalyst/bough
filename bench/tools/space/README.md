# bench/tools/space: benchmark windows that never disturb the user

`bough-space` orders a window we created in so that it never activates the app, never becomes key, ignores the
mouse and is never on the user's screen, while its view's display link keeps ticking at the display's rate.

Every window bough opens goes through `bough_metal::app::run_sized` (the windowed workloads, `run_window`,
`run_window_linked`, `run_interactive`, the probes), which calls `bough_space::show` instead of
`orderFrontRegardless`. Benchmark windows are placed by `BOUGH_BENCH_SPACE` (default `other`) and are borderless;
`bough::ui::Application::run` keeps an application's window on the user's Space, titled, unless the variable is set.

| `BOUGH_BENCH_SPACE` | where | display link (measured, macOS 15 / Darwin 24.6, M3 Pro, 120 Hz panel) |
|---|---|---|
| `other` (default) | a Space this process creates with `CGSSpaceCreate` and never shows; the window keeps its frame on the main display inside it | 120.0 ticks/s; `animate_workload` presents 240 frames in 2 s (241 GPU frames), as on the active Space |
| `offscreen` | outside every display | **never ticks** (the window has no screen): display-link-paced runs stall and never reach their exit timer |
| `active` | the user's current Space (the old behaviour) | the display's rate; disturbs the user |

The window server reports the `other` window as not on screen (`kCGWindowIsOnscreen` false); it is never key and the
app is never active (`crates/bough-metal/examples/space_probe.rs` checks all three). This Mac had one user Space,
so no user Space is ever touched: the private Space belongs to the process's window-server connection and goes
away with it.

How it is done (no permission prompt, no UI scripting, no input events): the app is an accessory
(`NSApplicationActivationPolicyAccessory`, set by `run_sized`); the window gets `ignoresMouseEvents` and leaves the
window cycle; it is ordered in outside every display, moved with SkyLight's `CGSSpaceCreate`,
`CGSAddWindowsToSpaces` and `CGSRemoveWindowsFromSpaces` (looked up with `dlsym`, so a missing symbol falls back to
`offscreen` instead of failing to load), then put back at its own frame inside the private Space.

```sh
BOUGH_BENCH_SPACE=other cargo run --profile gate -p bough-metal --example space_probe   # placement + ticks/s
BOUGH_SPACE_REPORT=1 ...                                                               # any windowed bench: print the placement
```

From other code (a Swift or C probe, another crate): call `bough_space::show(&window, bough_space::mode_from_env(
bough_space::Mode::Other))` once, on the main thread, instead of ordering the window in; or repeat the four calls
above on your own window number.
