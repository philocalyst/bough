# Lane `platform`, round 7: journal

Goal (the user): bough fully cross-platform as soon as possible, so it runs in zortax's gui-bench matrix on the
Linux host ilo-prod (fakeland, a headless Wayland compositor, pixman; lavapipe is the only Vulkan there).
Milestones, each run on ilo-prod: (1) the workspace builds on Linux behind a clean backend boundary; (2) a Wayland
backend presenting from a CPU executor shows a correct frame under fakeland; (3) a bough app in gui-bench passing
the layout check, then a quick run; (4) the Vulkan path through `bough-gpu` on lavapipe, with the compute lane.

Branch `r7/platform` (worktree `wt/r7-platform`), from `round6` 3b0b00c698. Remote work in
`ilo-prod:/root/work/r7/platform` (`env.sh` there sources the zortax nix env read-only and sets this lane's own
toolchain copy, cargo home and target dir).

## The backend boundary (milestone 1; permanent architecture)

Everything above the boundary is portable: bough-core, bough-mem, bough-text (pure Rust: own tables, swash,
vendored parley grafts), cephal, and the façade's runtime, elements, layout, input routing, damage and the surface
ring's policy (`app.rs`). Below it, three kinds of backend, chosen per platform (`crates/bough/src/backend/mod.rs`):

* **Executors** turn a frame into pixels. Their contract is the new crate **`bough-frame`**: the 48-byte instance,
  the region layout the kernels read (`Layout`, masks, column table), the payload encodings (`PAY_RUN`, `PAY_IRUN`),
  the frame slot of damaged tile rows (`write_slot`) and the scroll copy (`SlotCopy`). It used to live inside the
  Metal presenter, so nothing that reads a frame could build without Metal. Executors: bough-metal (macOS),
  bough-gpu (wgpu-hal; Metal now, Vulkan next), and the CPU executor (`cpu_raster.rs`, every platform).
* **Surfaces** hold the pixels the CPU executor writes and show them: the trait `backend::Surfaces` (count, size,
  in_use, pixels, present with damage, resize). Implementations: `MemSurfaces` (offscreen memory: every pixel test
  runs on any platform), the Wayland host's `wl_shm` ring, and (not wired yet) the Metal presenter's IOSurfaces,
  which already have the same operations (`write_begin`/`show_held`).
* **Hosts** own the window and event loop and feed the portable input API: AppKit (`backend::host`, macOS) and
  Wayland (`backend::wayland` over the new crate **`bough-wayland`**, Linux). The portable entry for harnesses is
  `backend::run(title, make_cfg, view, Run { seconds, start, every, linked, report })`, plus `run_window` and
  `run_window_linked` (the workload drivers'), implemented on both hosts.

OS services that are not a window (instruction counters, process memory, machine identity) are the new crate
**`bough-sys`** (macOS: `proc_pid_rusage`, `thread_selfcounts`; Linux: `perf_event_open` user-space counters and
procfs). bough-metal re-exports its old `perf` module and every moved frame type under the old paths, so other
lanes' code keeps compiling unchanged on macOS.

Text needed no new path: bough-text never asked the OS for fonts. Its only platform dependency was fontique's
`system` feature (CoreText on macOS, fontconfig on Linux), enabled but unused; it is now off for the library (the
tests' twin layouts keep it). Font discovery is the host's: `ui::default_font()` reads `BOUGH_FONT`, then
`ZGX_FONT_FILE`, then the platform's sans-serif (macOS Arial; Linux `fc-match sans-serif`, which under the zortax
env is DejaVu Sans, the face gui-bench's spec names).

## Log

### 2026-11-12, M1: the boundary, and the workspace checks on Linux

* New crates `bough-frame`, `bough-sys`, `bough-wayland`; `bough-metal` empty off macOS (its deps are macOS-only);
  façade deps split per target; comparisons use `bough::sys::perf` / `bough::frame` / `bough::backend::run_window`
  instead of bough-metal's paths; two Metal-only binaries (`kernel_bench`, `drag_probe`) are stubs elsewhere.
* `App` gained `target: Option<Box<dyn Surfaces>>`: where there is no GPU presenter, `App::new(.., gpu: true)` makes
  offscreen `MemSurfaces`, and every frame goes through the CPU executor (`draw_target`: the scroll copy on the CPU,
  the repair, `present` with the tiles that differ from the surface on screen). `presented_pixels`,
  `reference_pixels` (a CPU full redraw), `bins_overflowed`, `reset_bins`, `stream` are portable.
* The ring may repair the surface on screen in place when its target says the compositor released it (age 1, the
  smallest repair); the scroll copy is then a memmove in the same surface (`copy_rect` orders rows by the move).
* Checked: `cargo check --workspace --all-targets` on macOS (passes); `cargo check --target
  x86_64-unknown-linux-gnu --workspace` on macOS (passes; lib and bins). Linux build and tests on ilo-prod: next.
* Found on ilo-prod: the round-6 `platform-host` lane's unpushed branch (`/root/work/bough-r6/bough`,
  `lane/platform-host/round6`, 8 commits on `main`): the portable crates' tests passing on x86-64 (card 1) and
  bough-gpu's kernels on wgpu-hal Vulkan with exactness gates on lavapipe (card 2). Pulled as a bundle (sha256
  verified) to `refs/ilo/platform-host-r6`; card 1 is folded into this branch, card 2 is the starting point of M4.
