# Vendored crates

Plain source copies of upstream crates, with no history of their own. Each table names the upstream commit and published version; each "changes" list is every local edit, so a diff against the published crate of that version is bough's whole patch. The workspace excludes `vendor/`; crates resolve to these copies through `[patch]` tables or path dependencies.

## GPU and windowing stack (ROUND4 §8)

Copied verbatim from the crates.io sources of the versions below (the registry packages, `Cargo.toml` as published;
`.cargo_vcs_info.json` and `Cargo.toml.orig` dropped). The workspace resolves every use of these crates to these
copies through `[patch.crates-io]` in the root `Cargo.toml`, and `exclude = ["vendor"]` keeps them out of
`cargo test --workspace`.

| crate | version | upstream commit | used by |
|---|---|---|---|
| wgpu | 29.0.4 | gfx-rs/wgpu e99f5305ded96ff7006f0714d043a7f735bd45c2 | (not on the frame path; for the wgpu-core comparison) |
| wgpu-core | 29.0.4 | e99f5305 | (same) |
| wgpu-hal | 29.0.4 | e99f5305 | `crates/bough-gpu` (Metal backend, the frame path) |
| wgpu-types | 29.0.4 | e99f5305 | `crates/bough-gpu` |
| naga | 29.0.4 | e99f5305 | `crates/bough-gpu/build.rs` (WGSL -> MSL at build time); wgpu-hal's `metal` feature links it but bough never calls it at runtime |
| wgpu-naga-bridge | 29.0.4 | e99f5305 | wgpu-core |
| wgpu-core-deps-apple | 29.0.4 | e99f5305 | wgpu-core |
| winit | 0.30.13 | rust-windowing/winit e9809ef54b18499bb4f2cac945719ecc2a61061b | (the host port, not started) |
| wgsl-rs (+ -macros, -ir, -layout, -layout-macros) | 0.1.0 | schell/wgsl-rs b2de16a1b81e67d76bc265c296184f882864badb (the git checkout gpui-ce pins) | `crates/bough-gpu` shaders; `[patch."https://github.com/schell/wgsl-rs"]` |

Version choice: gpui-ce (`~/projects/gpui-ce3` origin/main) pins `wgpu = "29.0.3"` and `wgsl-rs` at
schell/wgsl-rs `b2de16a1b8`, whose naga is 29; 29.0.4 is that line's latest patch release. (The archive's
`codex/wgpu30` moved gpui to wgpu 30 with a newer wgsl-rs; not needed here.)

### Local changes

* **wgsl-rs: subgroup operations.** `subgroup_exclusive_add`, `subgroup_add`, `subgroup_max`, `subgroup_ballot` in
  `crates/wgsl-rs/src/std/subgroup.rs` (CPU model: one invocation per subgroup) and their WGSL names
  (`subgroupExclusiveAdd`, ...) in both name tables (`wgsl-rs-macros/src/builtins.rs`,
  `wgsl-rs-ir/src/render/builtin_lookup.rs`). naga 29 accepts them without an `enable` directive and emits
  `simd_prefix_exclusive_sum` / `simd_sum` / `simd_max` / `simd_ballot`. The binned kernels' ordered compaction is
  built on them (`crates/bough-gpu/src/shaders/compact.rs`, checked by `halprobe`).
* **wgsl-rs workspace manifest**: `members` trimmed to the five vendored crates (the examples and test crates are
  not vendored).
* wgpu, wgpu-core, wgpu-hal, wgpu-types, naga, winit: none. What the hal already exposes covers the first increment: `Device::device_from_raw`,
`Queue::queue_from_raw` / `as_raw`, `Device::buffer_from_raw` (the `storageModeShared` region the CPU writes in
place), `Device::texture_from_raw` (IOSurface-backed surfaces), `ShaderInput::MetalLib` (the build-time metallib),
`CommandEncoder::raw_command_buffer` (shared-event waits and signals between passes of a pre-armed batch), and
`Fence::raw_shared_event`.

Planned extensions (each a clean addition in `wgpu-hal/src/metal`, listed here when made):
* a bindless table: write surface / atlas resource ids into the region and bind only the region and the slot
  offset per encoder (today's hal bind group calls `setTexture` for every surface on every pre-armed encoder, which
  costs per frame and makes pending frames hold every IOSurface "in use");
* residency-set membership for wrapped resources (`addAllocation` on the hal queue's set);
* CAMetalLayer-free presentation of IOSurface contents (the publisher thread's transaction).

## Text stack (ROUND4 §7)

bough's text path is parley, vendored here so bough-text's performance mechanisms can be grafted into it
(research/ROUND4.md §7, crates/bough-text/RESULTS.md Part IV). These directories are forks: change them where a
graft needs it, and record each change below so every diff against upstream stays reviewable.

| crate | version | upstream commit | source | licence |
|---|---|---|---|---|
| `parley` | 0.11.1 | linebender/parley `eea3503dd6cf17130cbb07348e0ff2c918300e94` (path `parley`) | crates.io `parley-0.11.1` | Apache-2.0 OR MIT |
| `fontique` | 0.11.1 | linebender/parley `eea3503dd6cf17130cbb07348e0ff2c918300e94` (path `fontique`) | crates.io `fontique-0.11.1` | Apache-2.0 OR MIT |
| `harfrust` | 0.12.0 | harfbuzz/harfrust `60b28ea22b5261710018d69c168a762bcb28794c` | crates.io `harfrust-0.12.0` | MIT |
| `skrifa` | 0.44.0 | googlefonts/fontations `b373cdbc6043a791fb23b3e0f413649458f5e63a` (path `skrifa`) | crates.io `skrifa-0.44.0` | MIT OR Apache-2.0 |

These are the versions gpui-ce's parley stack pins (`~/projects/gpui-ce3`, branches `parley_stack_01_core` …
`parley_stack_06_editable_text`: `parley = "0.11"`, `fontique = "0.11.1"`, `skrifa = "0.44.0"`); parley 0.11 shapes
with harfrust (not swash). `read-fonts` 0.41, `parlance`, `parley_data` and the ICU crates stay ordinary crates.io
dependencies. Each directory is the published crate as extracted by cargo (`.cargo_vcs_info.json` names the commit),
with `Cargo.lock` and `Cargo.toml.orig` removed.

The workspace excludes `vendor/` (`exclude = ["vendor"]` in the root manifest): these are path dependencies, not
members, so upstream's tests and dev-dependencies are not built.

### The reference

`crates/bough-text` also depends on unmodified **upstream parley 0.11.1 from crates.io** (dev-dependency
`parley_upstream`, which pulls upstream fontique/harfrust/skrifa from crates.io too). Every graft is gated by
`crates/bough-text/tests/parley_twin.rs` and the gates built on `tests/common/twin.rs`: the grafted parley's layout
(lines, runs, glyph ids and positions as f32 bits, clusters and their boundary flags) must equal upstream's on the
same input.

### Changes against upstream

| crate | file | change | why |
|---|---|---|---|
| parley | `Cargo.toml` | `fontique`, `harfrust`, `skrifa` dependencies take `path = "../<crate>"` | use the vendored forks |
| parley, fontique, harfrust | `src/lib.rs` | `#![allow(deprecated)]` | newer semver-compatible deps (parlance 0.1.1, icu_properties 2.4) deprecate APIs these releases call; behaviour unchanged |
| harfrust | `src/hb/face.rs` | `#[allow(dead_code)]` on `pub fn shape` | nightly flags it unused in this build |
| parley | `src/bough/` (new) | bough-text's `script.rs` (+ `script/pairfast.rs`), `shape.rs`, `slots.rs` moved here verbatim (paths `crate::` → `super::`, `std` prelude imports, `tokenize`/`load_partial` public, `window::slots` as `slots::hybrid`); bough-text re-exports them | the grafts run inside parley's pipeline |
| parley | `Cargo.toml` | `ttf-parser` 0.25 dependency | the table builder (`bough/script.rs`) reads GSUB/GPOS/GDEF/HVAR through it (port to read-fonts: not done) |
| parley | `src/layout/data.rs` | `push_run` → `push_shaped_run` over any `ShapedGlyph` stream; `process_clusters` generic over it | **graft 1**: harfrust's buffer and bough's table feed the same cluster code |
| parley | `src/shape/mod.rs` | `ShapeContext.fast`; in `shape_item`, an LTR Latin segment with no features and no locale goes to `bough::fast` first (table accepts → `push_shaped_run`, else harfrust) | **graft 1**: the shaping fast path |
| parley | `src/bough/fast.rs` (new) | table cache per font blob / face / user-space variations (+ harfrust's normalized coords), AAT `trak` constant per size read from harfrust | **graft 1** |
| parley | `src/shape/mod.rs` | upstream's segment body extracted as `shape_segment` (unchanged); `FontSelector.first_complete`; `fast_item` before grapheme segmentation | **graft 2**: one font selection per style run (not per grapheme cluster) when the run's font is the query's first candidate and its table covers the run; adjacent equal fonts merge as upstream merges them |
| parley | `src/analysis/mod.rs` | upstream's per-character body factored as `char_info` (unchanged); `analyze_simple` + a 256-entry table built by `char_info` | **graft 3**: text of letters, digits and spaces (by ICU's LineBreak AL/NU and WordBreak ALetter/Numeric) needs no segmenter: UAX #14/#29 reduce to pair rules there |
| parley | `src/bough/mod.rs` | per-thread counters `segments()`, `analyses()` | the gates assert the fast paths are taken |
| parley | `src/layout/data.rs`, `src/shape/mod.rs` | `MetricsCache` in the shape context, passed to `push_run`/`push_shaped_run` | run metrics read once per (font, size, coordinates), not per run |
| parley | `src/builder.rs`, `src/shape/mod.rs` (`build_simple`), `src/layout/data.rs` (`push_slot_run`, `SlotRun`, `run_metrics` factored), `src/bough/fast.rs` (per-segment slots) | **graft 5**: the fused build. Simple text in one item, whose style runs' fonts are first complete candidates with covering tables, is laid out in one pass per font segment from bough's table output (same clusters, glyphs, f32 operations and run accounting as `process_clusters`), skipping analysis, style-index fill, the item/segment loops and harfrust; all or nothing |
| parley | `src/bough/breaks.rs` (new), `src/analysis/mod.rs`, `src/shape/mod.rs`, `src/layout/data.rs` | **graft 3b**: exact UAX #14 / #29 boundaries for all covered Latin-1 text by walking ICU4X's own baked rule tables (`Baked::SINGLETON_SEGMENTER_BREAK_{LINE,WORD}_V1`) over a 256-entry property map (the iterators' `next` transcribed); graft 3 and graft 5 accept every covered Latin/Common character (digits and other emoji-property characters included) | labels, numbers and log lines take the fused path |
| parley | `src/layout/layout.rs` (`Layout::retext`), `src/layout/data.rs` (`FastKey`), `src/shape/mod.rs` (`retext_simple`) | **graft 6**: retained re-text. A layout the fused path built from one style on one font keeps that font's key; a new covered text is laid out without the builder, style resolution or font query (boundaries, table shaping, clusters only) | labels, numbers, log lines, messages |

## Layout: cephal

`vendor/cephal` is zortax/cephal `9e77f91` (0.2.0), copied without history (`ad79959f5d`). Its own tests
(`cargo test --release` in `vendor/cephal`, run on ilo-prod) gate every change below: incremental and parallel
layout must equal a cold layout bit for bit over random trees and random edits.

| file | change | why |
|---|---|---|
| `src/compute/common/overflow.rs` | `contribution_extent` factored out of `scrollable_overflow_contribution` (same expressions) | the one place that says what a parent reads of a child's overflow |
| `src/tree/input.rs` | `LayoutOutput::same_contribution`: an in-flow child's final answer whose overflow changed only inside the box its parent placed (equal `contribution_extent` bits at the placed size) is the same answer for the parent | lane `layout` (round 6): a text edit inside a box of fixed size no longer re-gathers the overflow of every sibling of the box (`signal_cost`: the per-row growth of a width-changing label write is gone) |
| `src/schedule/mod.rs` | `answer_changed` takes `in_flow` and reads the child's placed size from its stored layout; out-of-flow boxes keep `same_for_parent` (their containing block's placement record holds their overflow) | as above |
| `Cargo.toml`, `src/tree/input.rs`, `src/schedule/mod.rs` | features `mutant-overflow-cut`, `mutant-boundary` (compiled out unless enabled) | negative controls: cephal's incremental tests and bough's `tests/lay_diff.rs` must fail with either |
| `tests/incremental.rs` | `CEPHAL_DEBUG_STEP=<n>` dumps every node before and after step `n` of `overflow_edits_match_cold_layout` | debugging the incremental tests |
| `examples/replay.rs`, `Cargo.toml` (dev-dependency `bough-mem`) | replays a bough scene's layout trees, dumped by `lay_probe` (bough-workloads, feature `dump`), through cephal's own `Tree`: cold layout and the recorded resize sweep, instructions per phase, query counters with `stats` | lane `layout` (round 7): the engine's cost on real application trees, on any host, under `perf` |
