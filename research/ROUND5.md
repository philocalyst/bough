# Round 5: common brief (2026-11-04)

Read this first, then `BRIEF.md` (where everything is, the numbers, the outstanding work) and the design spec in
`designs/reactivity/final/` (00-model, 02-elements, 03-ports, 04-reactions, 08-runtime and `examples/progressive.rs` at
least). ROUND3/ROUND4 rules still apply where this file does not override them.

## The user's direction

> Drive out the complete plan end to end, stop dragging your feet, keep pushing the numbers. Maintain the
> performance, but shift the benchmarks to not use the raw primitives, as that is definitely influencing the
> numbers. Use compute on ilo-prod through rex. Keep innovating and keep pushing the limits; there are still some
> HUGE things missing, especially around startup and memory. You're still not being clever or ambitious enough.
> Push the limits!

What follows from that:

* **Benchmarks are written the way users write apps.** `bough::prelude` is the design surface: flow layout by
  cephal, `text`, `button`, `div().layout(..).child(..)`, signals, routes as methods, `Application::run`. The
  `bough::raw` scene API (absolutely placed quads, `.at(x, y)`, templates) may not appear in any benchmark scene.
  The win must come from the framework making the design-shaped program cheap, not from hand-placed primitives.
  Every port must also be faithful to its zgx/zgui workload, so the comparison is honest.
* **Startup and memory are first-class targets**, measured against zgx and zgui on every scene:
  * cold time to first pixel, cold time to interactive, and instructions to first pixel;
  * `phys_footprint` steady and peak;
  * dirty memory by category (`footprint`, `vmmap --summary`).
* **Ambition.** The previous rounds won on CPU per frame. The remaining large costs are the ones nobody attacked
  structurally:
  * the ~22–29 ms Metal device creation on the startup path;
  * the GPU driver's ~95–105 MB first-submission footprint;
  * window surfaces at roughly 11 MB each;
  * AppKit's launch and dirty memory;
  * mount, layout and shaping redone on every launch.

  Treat each one as a representation problem (the Joffe method, `research/04-reformulation-method.md`). The question
  is what would make the cost vanish, not how to make it smaller.

## Environment (this machine was migrated; everything is restored)

```
cd /Users/philocalyst/projects/bough             # (= /Users/mileswirht/projects/bough via symlink)
export RUSTUP_HOME=~/.local/share/rustup CARGO_HOME=~/.local/share/cargo RUSTC_WRAPPER= PATH=~/.local/share/cargo/bin:$PATH
export CARGO_TARGET_DIR=/Users/philocalyst/projects/bough/target/<your branch>
export ZGX_FONT_FILE=$HOME/projects/_ref/zgx/comparisons/fonts/DejaVuSans.ttf
cargo check --workspace --all-targets && cargo test --workspace --release
```

* **No Xcode is installed** (Command Line Tools only), so there is no `xcrun metal`. bough-metal compiles
  `shader.metal` at load (`gpu::load_library`). bough-gpu builds an empty metallib, so its runtime MSL path is
  missing. Runtime shader compilation is now on the startup path; making it vanish is part of the startup work.
* **References** in `~/projects/_ref`:
  * `zgx` is pinned at 06debe9d, the commit our ports and baselines were built against;
  * `zgx-latest` is current main, 07ebcf01, to re-baseline against;
  * `zgui`, `bbtree`, and `cephal` (also vendored as `vendor/cephal`, 9e77f91).

  The GPUI rework archive is this repo's `origin`: its branches are the research history, and
  `origin/local/architecture-performance` has the reactivity work.
* **ilo-prod**: Ryzen 3700X, 16 threads, 62 GB, NixOS, x86_64. Use it for Linux CPU-only builds and instruction
  counts (`perf stat -e instructions:u`), in your **own** window of the rex session `bough`:

  ```
  rex -S ilo-prod run -s session:01a1aaa1-56f3-7ee4-bec4-421e412eeec8 --label <you> --keep-open --focus=false --json --wait -- sh -c '<cmd>'
  rex -S ilo-prod capture -b <block id from the json> --trim
  ```

  * Use `/root/work/env.sh` for cargo, and put your clones under `/root/work/bough-<you>`.
  * Never touch the other session on that server (`nudox-support-…`); it belongs to the user's other work.
  * Plain `ssh` to ilo-prod times out from here, so go through rex.
* **Load.** The Mac's load average is 40–60 from other work. Interleave A/B comparisons, read instruction counts,
  and treat times as indicative.

## Rules

* Work in your worktree and branch. Commit often, with the trailer
  `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Run `cargo check --workspace --all-targets` (zero
  warnings) and the full suite before each report. Never touch `canonical`; the lead merges.
* Every new mechanism has a gate:
  * pixels equal a full redraw at ring ages 1–4;
  * retained equals fresh mount;
  * differential tests against an oracle.

  For startup snapshots: the restored state's pixels and behaviour equal a cold mount.
* Use Haiku subagents for comparisons, baseline runs and literature or web research. Do the design and
  implementation yourself.
* Report: tables against zgx and zgui in their own units, with the exact commands; what each structural idea
  bought; what you tried and rejected; honest gaps.
