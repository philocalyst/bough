# bough, and the GPUI rework it continues

This repository holds three things, plus the indexes that tie them together:

* **bough**, the active implementation: a from-scratch Rust GUI framework built toward the GPUI-rework design. It
  lives at the repository root.
* **The design**: `designs/reactivity/`, normative.
* **The clean-room GPUI rework that came before bough**: `reactivity/`, preserved as a checkpoint, along with its
  833 research branches.

Everything here is experimental. Nothing is released.

## What is where

| path | what it is |
|---|---|
| `crates/` | bough's crates. `bough`: the façade, with `bough::prelude` (the design surface: views, signals, lists, entities, readers, elements in flow, `Application::run`) and `bough::raw` (the scene-level surface the benchmarks use). `bough-core`: the paint-preorder record tree (64-slot SoA pages, negated-max rects, tile damage planes, hit testing). `bough-text`: shaping into the GPU glyph pool, over vendored parley. `bough-metal`: the fused binned-compute presenter on Metal. `bough-gpu`: the same presenter on vendored wgpu-hal with wgsl-rs kernels (`--features wgpu`). `bough-mem`: region columns. `bough-damage-lab`: damage-representation experiments |
| `comparisons/` | Benchmark ports: zgx's suite, scenes and apps (`bough-suite`, `-heavy`, `-scenes`, `-vlist`, `-text-bench`, `-app-*`, `-widgets`); zgui's (`bough-zgui`); the areweactuallyguiyet workloads (`bough-awagy`, the only ones written on `bough::prelude` so far) |
| `bench/` | Harnesses and raw results. `bench/awagy/` is a macOS port of areweactuallyguiyet.mespin.me with bough, zgx, BUI and Vizia measured and 18 frameworks reported ([page](bench/awagy/results/bough-on-awagy.html)) |
| `vendor/` | Plain-source copies of parley, fontique, harfrust, skrifa, wgpu, wgpu-hal, naga, winit, wgsl-rs and cephal, each with every local edit listed in `vendor/README.md` |
| `research/` | **`AUDIT.md`, the work list** (start here), the round directions (`ROUND3–5.md`) and the surveys |
| `BRIEF.md`, `STATUS.md`, `API.md`, `DESIGN.md` | Where everything is and the numbers (`BRIEF.md`); the fused system's scene-by-scene results against zgx (`STATUS.md`); the API as built, round by round (`API.md`) |
| `designs/reactivity/` | The design. `final/` is the constitution and its statutes (read `final/00-model.md` first); `01-…19-*.md` are the passes it consolidates; `approaches/` holds the four rejected or partial architectures; `runtime/` the measured data-structure work; `gap-register.md` and `performance.md` carry inline notes that are requirements |
| `reactivity/` | The clean-room GPUI rework checkpoint (its own workspace, toolchain and `AGENTS.md` rules): [brief](reactivity/docs/rework-2026-10-26/BRIEF.md) |
| `archive/` | The index of every preserved ref in this repository (research `local/*`, `codex/*`, `production/*`, `wt/*`, `ns/*`, stashes) with captured commits and how to restore one ([README](archive/README.md), [research index](archive/RESEARCH_BRANCHES.md)) |

Branches:

* **`main`**: this one. Continue here.
* `bough/canonical`: bough's own history up to this merge.
* `bough/api`, `bough/floor`, `bough/text3`, `bough/input`, `bough/fusion`, `bough/integrate`, `bough/layout`: the
  agents' branches, all merged into `main`.
* The archive's refs (above).

## Start here

1. `research/AUDIT.md` §0. The state in fifteen items: what is strong, and what is missing against the design and
   against gpui-ce.
2. `designs/reactivity/final/00-model.md`. The model, the laws (L1–L14) and the rejection list.
3. `BRIEF.md`. Where everything is, what was measured, how the system is built.
4. `crates/bough/src/lib.rs` (the two surfaces), then `crates/bough/examples/counter.rs`.
5. Pick work from `research/AUDIT.md`:
   * §12 is the experiments in order;
   * §13 maps them to the design's phases;
   * §15 lists the failed work worth retrying.

The design's counter, as bough runs it:

```rust
use bough::prelude::*;

const BAR: Flex = Flex::row().align(Align::Center).gap(12.0);

pub struct Counter {
    pub count: Signal<u32>,
}

impl Counter {
    pub fn inc(&mut self) {
        *self.count += 1; // a publish: the label's port patches
    }
}

impl View for Counter {
    fn view(&self) -> impl Element<Self> + '_ {
        div()
            .layout(BAR)
            .p(24.0)
            .child(text(sig!(self.count).map(|n| format!("Count: {n}"))))
            .child(button("+").on_click(Counter::inc))
    }
}

fn main() {
    Application::run(Window::document(Counter { count: Signal::new(0) }).title("Counter"));
}
```

(`sig!` is an interim spelling: the design binds `&self.count` directly; see AUDIT D2 and experiment X1.)

## Build, test, run

macOS on Apple silicon only for now: the presenter is Metal (direct or through wgpu-hal), and the host is AppKit.

```sh
# rustup honours rust-toolchain.toml (nightly-2026-10-17). A system cargo that ignores it will fail on
# #![feature] (reflection uses type_info).
export PATH=$HOME/.cargo/bin:$PATH RUSTC_WRAPPER=
cargo test --workspace --release                       # every gate (pixel gates draw headless on the GPU)
cargo run --release -p bough --example counter         # the counter in a window
cargo build --release -p bough --features wgpu         # the wgpu-hal presenter instead of direct Metal
```

* The zgx comparison crates depend on `zgx-workload` and `zgx-cosmic-text` at a pinned zeronsh/zgx commit by git, so
  the first build fetches it. Fonts for every gate come from that crate (DejaVu Sans).
* Without Xcode (Command Line Tools only), shaders compile from source at startup: bough-metal reads `shader.metal`,
  and bough-gpu builds an empty metallib with a warning. Install Xcode for build-time metallibs.
* The root `.cargo/config.toml` belongs to `reactivity/`'s CI and also applies here: v0 symbol mangling,
  `--cfg tokio_unstable`, `MACOSX_DEPLOYMENT_TARGET=10.15.7`. None changes bough's code generation.
* **State of this tree**: `bough/api` and `bough/floor` were merged on 2026-11-11.
  * `cargo check --workspace --all-targets` passed before the last merge edits.
  * The full test suite has not been run on the merged tree.
  * Running it is the first thing to do. The two merge commit messages list exactly what was changed by hand.

Benchmarks: each `comparisons/*` crate has binaries and a README. `bench/*` holds the runners. Measure alone (no
builds or other benchmarks running), report every cell, and keep losing results, as `research/AUDIT.md` §10
describes.

## Where bough stands (details in `BRIEF.md` and `research/AUDIT.md`)

**Wins:**

* zero work when idle;
* per-frame instructions 6–17× below zgx on its desktop scenes (`STATUS.md`);
* constant-time signal-bound updates;
* areweactuallyguiyet scroll at the display limit with 6–24× less CPU than zgx and BUI;
* the lowest memory and the smallest binary of the frameworks measured.

**Losses and gaps:**

* GPU time on glyph-dense and list scenes (1.5–2.2×);
* the dashboard's worst stall (75.7 ms in one run of three);
* p95 steadiness against iced;
* no IME, accessibility, clipboard, menus or multi-window;
* macOS only;
* single-font text without fallback or bidi;
* a public surface that still differs from the design: `sig!`, owner-typed elements, no `El<'f>`.

---

## The GPUI rework archive (preserved checkpoint, unchanged)

### Original normative designs — start here

- [Original design packet and precedence](designs/reactivity/README.md)
- [Final constitution and statutes](designs/reactivity/final/00-model.md)
- [07 — The Ladder](designs/reactivity/07-the-ladder.md)
- [11 — Where07 Leaks, and the Algebra That Closes It](designs/reactivity/11-consolidations.md)
- [Implementation crosswalk and complete127-file index](reactivity/docs/rework-2026-10-26/DESIGN-CROSSWALK.md)

**One semantic model, proofs at boundaries, no third language, nothing ambient.** All original files are preserved byte-for-byte. The original README's final-statute precedence governs; the new brief does not replace it.

### Collected engineering checkpoint

Read the [expansive engineering brief](reactivity/docs/rework-2026-10-26/BRIEF.md) and [evidence/source index](reactivity/docs/rework-2026-10-26/README.md). This is an **experimental preservation checkpoint**, not a finished or performance-qualified release. It includes current architecture, measured and unmeasured results, failure histories, weaknesses, API gaps and a detailed research backlog.

Selected cloud code is `3c5cc3330cf6e6878489a55f1ab7218731a379af`. Host adaptations and alternatives are separately indexed. The actual target-damage consumer, complete ladder compatibility and matched realistic unified benchmarks remain open. Historical graphs are baseline-only.
