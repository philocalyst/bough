# lorlei (formerly bough)

The project is now called lorlei; crates, paths and most documents still say bough. `AGENTS.md` is the entry point for
contributors: the state, the contribution policy and the whole roadmap.

A from-scratch Rust GUI framework built toward the GPUI-rework design (`designs/reactivity/final`), in the repository
that preserves the clean-room GPUI rework it continues. **Humans and AI-assisted contributors are invited to take a
lane** (`docs/LANES.md`). The aim is a framework that keeps bough's measured efficiency while reaching every platform
and the design's whole surface.

**Status (2026-11-13):** the façade runs on macOS (Metal, AppKit) and Linux (Wayland, CPU executor); a second,
typed-dataflow core (`bough-x` and its sibling crates) holds the order-of-magnitude numbers and is not yet what the
façade runs on. What stands, what was refuted and every open avenue are in `docs/r8/final/REPORT.md`; the roadmap is
in `AGENTS.md`. The whole-workspace suite on the integration branch passed (209 targets, 401 passed, 0 failed).

Where it is measured, it is efficient. Each of these numbers holds on its own workload, machine and baseline; none is
a general ranking:

* zero work when idle;
* 6–17× fewer instructions per frame than zgx on zgx's desktop scenes (`STATUS.md`);
* on the areweactuallyguiyet scroll workload: at the 120 Hz display limit with 6× less CPU than zgx and 24× less than
  BUI, the lowest memory, and the smallest binary of the frameworks measured (`bench/awagy/`).

It loses GPU time on glyph-dense scenes, the dashboard's worst stall, and p95 steadiness against iced.

## Running tally

`docs/TALLY.md` is the running list of everything tried, what each attempt showed (kept, refuted with the number
that killed it), and what is still open, ordered by expected size. It is updated as work lands. The current
round's design is `docs/r8/ROUND8.md`: one abstraction (typed expressions whose stage the compiler infers,
columns per view call site, damage as evaluate-and-compare over exposed pages) under an unchanged author API.
The public scoreboard is zortax's gui-bench under fakeland. As of 2026-11-13 the benchmarked façade binary is the
round-7 runtime (`r7/platform` `74e665786c`, now on main); the round-8 core is on main too (`crates/bough-x` with its
element layer, and gui-bench's app on it, `comparisons/bough-guibench-x`, which passes the runner's layout and
behaviour checks in all eight scenarios). Round 8's closing report is
[`docs/r8/final/REPORT.md`](docs/r8/final/REPORT.md), with each lane's final report beside it.

## The question

Can one framework do all of the following at once?

* (a) Implement the design's model: plain-Rust state, signals, lists, entities and readers; re-description with exact
  delta delivery; no ambient context.
* (b) Run on macOS, Windows, Linux and the web, with the full platform bar: IME, accessibility, clipboard, drag and
  drop, menus, dialogs, multi-window, DPI.
* (c) Keep or beat the efficiency bough measures today: idle at zero, per-frame work proportional to damage,
  constant-time property updates.

A contribution moves one of these without regressing the others, and says which it moved. Related work counts and is
welcome, but doesn't by itself discharge the question: a faster kernel on one GPU, a new widget, a port of another
framework's benchmark.

## Directions to pursue

Detailed in [`docs/LANES.md`](docs/LANES.md): each lane's open question, first bounded task, required output and
success criterion. [`docs/AVENUES.md`](docs/AVENUES.md) is the exhaustive backlog behind them: every open item with
its AUDIT ID and state, a per-platform realisation table (macOS, Windows, Linux, web, mobile: presenter, shaders,
IME, accessibility, clipboard, drag and drop, menus, dialogs, DPI, pacing, CI), and the design's 31 reference
workloads mapped to lanes. The table is in order of leverage. bough is already fast on one platform, so most of the distance
to a usable framework is **platform reach** and **the design's surface**.

| lane | question | evidence required |
|---|---|---|
| `platform-host` | Can the region-in-a-buffer, binned-compute presenter run through wgpu on Windows, Linux and the web, on a winit host, without losing the macOS wins? | Same scene snapshots and reference images on every backend; zero idle wakeups; Metal-through-wgpu within 5 % of direct Metal |
| `platform-capabilities` | IME (the design's bidirectional protocol), AccessKit accessibility, clipboard, drag and drop, menus, dialogs, cursors, multi-window, DPI and appearance, as typed capabilities with one cross-platform API | Gates T1–T6 on two platforms; scripted-IME and screen-reader tests |
| `surface-api` | The design's spelling and ladder (`&self.x` wires, owner-free `El<'f>`, `ChildSeq`, `Handoff`, `Target`, `Delta`, commands) at no cost | The design's examples compile; a compile-fail message corpus; STATUS and zgx suite within 5 % |
| `text` | Parley `Layout` under the façade (fallback, bidi, rich spans, emoji), an editor, hinting and snapping | Twin gates; CJK/Arabic/emoji pixel gates; text_bench parity |
| `gpu` | Close the GPU-time loss, and move every kernel to wgsl-rs | ABAB GPU pairs ≤ 1.0 against zgx on every scene; identical pixels |
| `pacing` | At most one present per refresh, no lost input, bounded input-to-present p99 | 10 AWAGY runs: worst gap ≤ 2 frames, p95 ≤ 8.4 ms; a measured p99 |
| `memory` | Virtualization of any container in core; a per-window memory ledger | O(visible) mount and memory at 1 M rows; footprint ≤ the best measured |
| `verification` | Run the suite; CI; scene-description snapshots plus a CPU reference raster; design-level test API; a stable build | Same gate verdicts on every platform's CI |
| `independent-review` | Do accepted claims replay from a clean checkout? | A replay receipt per accepted claim |
| `async-entities` | Questions, actions and streams (`map_async`, `.action`, `Remote`), the entity kernel, atomic publication, priority lanes | Race and entanglement property tests; fault-matrix workloads |
| `style-motion-media` | Tokens and cascade, variants, analytic motion, the image/video pipeline | A palette swap touching only consumers; animation tiers by phase |
| `components` | A full component set on the prelude alone | Builds on `bough::prelude` only; a friction log |
| `developer-loop` | Compile time, incremental rebuilds, binary size | Gates tracked in CI |

**What is portable today, and what isn't.**

* **Portable, plain Rust:**
  * the record tree and damage planes (`bough-core`);
  * text shaping into the glyph pool (`bough-text`, vendored parley);
  * cephal layout;
  * the reactive façade (`bough`).
* **macOS-only:**
  * the direct-Metal presenter (`bough-metal`: MSL compiled at load, IOSurface surfaces, a shared-memory region, a
    pre-armed command stream);
  * the AppKit host (`crates/bough/src/backend/host.rs`);
  * the wgpu-hal presenter (`bough-gpu`), which currently builds Metal only. Its kernels are wgsl-rs, so naga can
    target SPIR-V and HLSL; that build, and a winit host (vendored, unused so far), are the first platform tasks.

## Reuse what has already been learned

Read [`docs/SETTLED.md`](docs/SETTLED.md) before reopening a search. It lists the routes measured and closed in
bough, in the archive's clean-room GPUI, and by the design itself. `research/AUDIT.md` §15 lists the failed work that
merits a retry and why, for example:

* tile-flip presentation, which zgx ships;
* 2D binning with a parallel scatter;
* choosing copy or redraw per frame;
* a mount-time text cache.

The archive's 833 refs are indexed in [`archive/`](archive/README.md). Each dossier states its disposition;
preservation is not adoption.

## A relay with an independent gate

1. **Pick a lane** and an issue, and name the platform, target, baseline and predecessor commit.
2. **Freeze a card** ([`docs/CARD.md`](docs/CARD.md)): the change, baseline, workloads, thresholds, controls, budget
   and stop conditions.
3. **Produce a candidate or a useful negative result.** Preserve failures, timeouts and counterexamples.
4. **Hand the evidence to an independent replay.** A lane doesn't accept its own result.
5. **Submit a PR** with the gates, reproduction commands, raw measurements, provenance and exact claim scope
   ([`CONTRIBUTING.md`](CONTRIBUTING.md)). Integration and public claims are reviewed separately.

States are kept distinct, from `LEAD`, `CARD` and `CANDIDATE` up to `INTEGRATED`. `REJECTED`, `METHOD_BOUNDARY` and
`UNKNOWN` are useful, durable outcomes. Lane reports live in `docs/lanes/<lane>/`; local state, scratch and handoffs
live in the ignored `runtime/` (agents: `AGENTS.md`).

## Start reading

1. `research/AUDIT.md` §0: the state in fifteen items.
2. `designs/reactivity/final/00-model.md`: the model, the laws L1–L14, the rejection list.
3. `BRIEF.md`: where everything is, what was measured, how it is built.
4. `crates/bough/src/lib.rs` (the two surfaces), then `crates/bough/examples/counter.rs`:

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

`sig!` is an interim spelling; the design binds `&self.count` directly (AUDIT D2, lane `surface-api`).

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

## What is where

| path | what it is |
|---|---|
| `crates/` | bough's crates. `bough`: the façade, with `bough::prelude` (the design surface: views, signals, lists, entities, readers, elements in flow, `Application::run`) and `bough::raw` (the scene-level surface the benchmarks use). `bough-core`: the paint-preorder record tree (64-slot SoA pages, negated-max rects, tile damage planes, hit testing). `bough-text`: shaping into the GPU glyph pool, over vendored parley. `bough-metal`: the fused binned-compute presenter on Metal. `bough-gpu`: the same presenter on vendored wgpu-hal with wgsl-rs kernels (`--features wgpu`). `bough-mem`: region columns. `bough-damage-lab`: damage-representation experiments |
| `comparisons/` | Benchmark ports: zgx's suite, scenes and apps (`bough-suite`, `-heavy`, `-scenes`, `-vlist`, `-text-bench`, `-app-*`, `-widgets`); zgui's (`bough-zgui`); the areweactuallyguiyet workloads (`bough-awagy`, the only ones written on `bough::prelude` so far) |
| `bench/` | Harnesses and raw results. `bench/awagy/` is a macOS port of areweactuallyguiyet.mespin.me with bough, zgx, BUI and Vizia measured and 18 frameworks reported ([page](bench/awagy/results/bough-on-awagy.html)) |
| `vendor/` | Plain-source copies of parley, fontique, harfrust, skrifa, wgpu, wgpu-hal, naga, winit, wgsl-rs and cephal, each with every local edit listed in `vendor/README.md` |
| `docs/` | **Where to direct effort.** `LANES.md` (thirteen lanes: question, first task, success criterion), `AVENUES.md` (the exhaustive backlog and the per-platform plan), `SETTLED.md` (what not to reopen), `CARD.md` (the experiment card), `lanes/<lane>/` (each lane's public reports and cards) |
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

## Working together

* One coordinator integrates into `main` and keeps README, AUDIT, BRIEF and STATUS current.
* Lanes run in parallel, and one experiment has one owner.
* Settled results and source revisions are shared, so work builds on what is known instead of repeating it.
* Authorship, inherited ideas, new mechanisms and independent verification are credited separately.
* PR volume, model mentions and account counts don't measure progress; gates and replayed measurements do.

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
