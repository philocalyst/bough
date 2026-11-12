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
* Checked on macOS (M3 Pro, load 30-40): `cargo check --workspace --all-targets` passes; `cargo test --release -p
  bough -p bough-frame -p bough-sys`: 35 passed, 0 failed (7 min 27 s); `cargo check --target
  x86_64-unknown-linux-gnu --workspace` passes (lib and bins).
* **M1 on ilo-prod** (head b3c56c4cb4; Ryzen 7 3700X, load 15-24 from other tenants, `nice -n 10 -j6`):
  `cargo build --release -p bough` passes (1 min 44 s, cold except the copied registry); `cargo check
  --workspace` passes; `cargo test --release -p bough-mem -p bough-core -p bough-frame -p bough-sys -p
  bough-damage-lab -p bough-text`: 87 passed, 1 ignored (upstream's), 0 failed. Not run on Linux yet: the façade's
  and the comparisons' tests (they need the CPU target path exercised; next), `--all-targets` (the Metal-only
  examples of bough-metal/bough-gpu are not built off macOS by design).
* Found on ilo-prod: the round-6 `platform-host` lane's unpushed branch (`/root/work/bough-r6/bough`,
  `lane/platform-host/round6`, 8 commits on `main`): the portable crates' tests passing on x86-64 (card 1) and
  bough-gpu's kernels on wgpu-hal Vulkan with exactness gates on lavapipe (card 2). Pulled as a bundle (sha256
  verified) to `refs/ilo/platform-host-r6`; card 1 is folded into this branch, card 2 is the starting point of M4.

### 2026-11-12, toward M2

* The CPU executor (`cpu_raster.rs`) models every kind where it is the only executor (`all_kinds`, set on the
  `App::target` path): images (the spinner is one), gradients, shadows, transformed spaces, as the CPU twin of
  `f_shade_x`/`f_shade_t`. Off on macOS, so the Metal/CPU mixed-executor exactness is unchanged.
* On x86-64 the tile loop is compiled twice, the second for AVX2 + FMA + F16C, chosen at run time: Rust's `f16`
  arithmetic is otherwise a compiler-rt call per conversion. Same arithmetic in both; measured next.
* bough-wayland: `dispatch` read nothing while decoded events waited to be taken, so the ring's wait for a buffer
  release (inside a frame) could never see it: fixed (read without waiting). The caret's blink timer is armed only
  while a field is focused (no wake-ups in an idle window without one).
* gui-bench/fakeland: this lane's clones in `/root/work/r7/platform/{gui-bench,fakeland}` (from the zortax
  checkouts, same commits 0fd4510 / 71695e7), runner patched to know `bough` (`apps/bough`: a one-line binary over
  `comparisons/bough-guibench`, whose spec.rs and cli.rs are byte-identical to gui-bench's shared ones).

### 2026-11-12, M2 and the first gui-bench runs

* **M2 reached** (head b3c56c4cb4 then 6ad7fd59aa; ilo-prod, fakeland 71695e7 `serve --renderer pixman --output
  1280x800@60`, `bough-bench --scenario rows --rows 1000`): the first frame, a hover and a 200 px finger scroll
  under fakeland, screenshots probed with a stdlib PNG decoder: (640,16) #ffffff, (640,48) #f6f8fa, (640,31)
  #e5e7eb, hover (640,176) #cfe3ff. `ctl stats`: input to commit p50 6.8 ms over 4 events.
* gui-bench: this lane's clone of gui-bench and fakeland (same commits as `/root/work/zortax`), the runner patched
  to know `bough` (`apps/bough`, one line over `bough_guibench::run_cli`). `--quick --frameworks bough`, q1 (rows
  only) and q2 (all 8 scenarios, 14 cells, head ad451b1a10, load 3-15, nice 10): 13 cells complete with the layout
  check passing, 310 of 316 reports valid; spinner at 10,000 rows failed at mount (glyph column, see below).
* **Found by the runs, fixed** (each its own commit; the first two also on `r7/platform-fixes`, cut from `round6`
  for the integrator: a40713f01a, 72a87feefc):
  1. The software `f16::mul_add` on x86-64 (`__truncdfhf2`, an f64 FMA rounded in software): 73 % of a scroll
     frame. The CPU executor's half FMA is an f32 FMA rounded to half off aarch64 (aarch64 keeps the exact half
     FMA). Scroll frame 327 M -> 60 M instructions, first frame 336 M -> 69 M (rows, 100, guibench headless R1).
  2. `scroll_band` compared the record kind without masking `KF_MASK`, so a hoverable or scrolling box (the
     scroll port itself) was damaged whole on every scroll step, Metal included: rows scroll repaired tiles at ring
     age 4, 1000 -> 240; search 960 -> 80 (macOS R0, guibench headless).
  3. The prelude scroll box moved its content with `scroll_run` (damage = the port): now `Rt::scroll_copy`.
  4. The region reserves the record format's limits off macOS too (spinner 10,000 filled `glyph_x` at 2^19).
  5. **Generalization OVERFIT 8, hover stale after a scroll** (the 198,048-byte retained-vs-fresh failure of
     rows and virtual scroll): hover is now derived again at the still pointer after every settle and turn
     (`App::rehover`; the hit cell's cache makes it one compare when nothing under the pointer moved).
  6. **Generalization OVERFIT 1, plain state inside a scroll box never re-described** (every coarse table op
     timed out in gui-bench: the window never changed): `ScrollBox` tapes its view and updates port and content
     like any box instead of mounting opaque.
  Minimal repros of 5 and 6 are `crates/bough/tests/scroll_box.rs` (each fails without its fix, checked).
  The guibench gate matrix (66 gates: rows/virtual hover, scroll, jump; table fine/coarse x 9 ops; ticker; search
  x 4; spinner; typing; resize; rings 4 and 1) passes on macOS Metal with 5 and 6; before them rows/virtual
  scroll/jump and every coarse op failed.
* CPU executor speed (still `cpu_raster.rs`, until the compute lane's `bough-kernel` replaces it): binned entries
  per tile (CSR), each tile starts at its last opaque cover, AVX2 + F16C solid spans and store chosen at run time
  (the same bits as the scalar loop, checked by `tests/cpu_target.rs`, which also caught an off-by-one in the
  first version of the bins).
* **What the fine table's ~10 ms ops are**: framework cost, but not the reactive engine's. Fine remove at 1,000
  rows (guibench headless, ilo-prod, c2c001b1e5): 111 K instructions without rendering, 22.2 M with the CPU
  executor, which repairs 883 tiles because every row below row 2 moved up 32 px (89 % of the profile in the tile
  loop). A list splice is a rigid translation of the rows below it, the same as a scroll: a copy of the moved rows'
  pixels plus the exposed row would cut it to a few dozen tiles. The scroll record holds one clip and one move;
  generalising it to a copy rect (any run translated inside a clip) is a core-damage change (core lane).
* Stale build caught: after a negative control that reverted and restored source files, cargo's mtime freshness
  reused the reverted library and a passing test "failed"; the restored tree passed once rebuilt. From then on
  `cargo -Z checksum-freshness` (AGENTS.md).

### 2026-11-12, M3 comparison, M4 (Vulkan), and the scroll-copy condition

* **cmp-1** (gui-bench `--quick`, all 12 builds in one runner invocation on ilo-prod, 12:15-13:13Z, load 8-9 from
  other tenants, nice 10; bough f2335ebcb8 CPU executor into wl_shm, the others the zortax lavapipe builds; table in
  `results/cmp-1/headline.md`, identity and binary hashes in `results/cmp-1/IDENTITY.txt`): bough hover p50 11.5 ms
  (input to commit 0.16 ms at 100 rows), idle 34.5 MiB and 0.03 ms CPU/s, scroll 6.7 ms CPU/frame, resize 15.9 ms,
  spinner 10k 1.3 ms/frame, fine remove 14.6 ms p50; validity 44 of 47 reports (3 wake_lateness). Its typing and
  search cells predate select-all (0c062fbe29) and are not comparable: rerun as cmp-2.
* **M4(a), the Vulkan executor** (`bough-kernel/src/vulkan.rs`, after merging r7/compute): naga SPIR-V 1.3 of the one
  kernel source, wgpu-hal 29 Vulkan, five storage buffers, a fence per frame, `wrap` imports caller memory as the
  output (VK_EXT_external_memory_host). On ilo-prod lavapipe (llvmpipe, LLVM 21.1.8): `tests/exact.rs` 24 scenes,
  9,600 CPU tiles, 3,840 GPU tiles, 0 differing words, spec hash 892af7c19e24755b, identical to the Mac's; the Vulkan
  negative control (the shared OpConstant 16384 -> 16383) caught in 4 of 4 scenes.
* **First Vulkan gate run: 64 of 66 segfaults.** `KernelBackend` drops `gpu` before `targets`, and a Target
  destroyed its buffer and memory through a cloned `ash::Device` whose VkDevice was gone. Now the Gpu shares its
  hal device (Arc) with its targets (any drop order is sound), the App releases the GPU's imports before a resize
  frees the pages, and `vulkan_writes_wrapped_memory` covers `wrap` and the drop order.
* **The scroll-copy condition** (the lead's question after the integrator reverted 72a87feefc): a copy is exact for a
  record only if the executor's pixels of it are a function of the pixel's offset from the record. Untransformed
  records are; transformed ones are not (the kernels map a pixel through the affine in absolute coordinates). Found
  by instrumenting bough-zgui gallery-l ring 2 step 41: the one wrong pixel (157,511) belongs to a label in space 5,
  turned 20 degrees, moving with the scrolled run; the full redraw itself differs by one level between the label's
  two positions. The unmasked kind had marked the port's whole clip on every scroll, hiding it. `scroll_run_copy`
  now repaints run pages carrying a transform (old and new cover) and treats static records of transformed pages as
  band -1; untransformed pages cost nothing extra. Negative controls: each half alone fails the new core oracle test.
  The interaction flags play no part. cmp-1's rows-scroll numbers stand (no transformed content there; the gates were
  pixel exact).
