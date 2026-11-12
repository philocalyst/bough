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

## Glyph rasteriser (size lane, `docs/lanes/developer-loop/`)

bough-text rasterises unhinted glyph outlines into alpha coverage with swash, which draws outlines through skrifa
and fills them with zeno. Both are vendored so the rasteriser can be reached through the outline-and-fill path alone:
upstream chooses hinting, colour/bitmap glyphs, the stroker and subpixel formats with run-time values, so all of
them are linked into every executable that rasterises a glyph (about 200 KB of code under `production`).

| crate | version | upstream commit | source | licence |
|---|---|---|---|---|
| `swash` | 0.2.10 | dfrg/swash `7773843df0d63cd468db61a29c152b5e7a99d4ab` | crates.io `swash-0.2.10` | Apache-2.0 OR MIT |
| `zeno` | 0.3.3 | dfrg/zeno `698399439f8035736d01de00cbcdc413b3976a18` | crates.io `zeno-0.3.3` | Apache-2.0 OR MIT |

Copied from the registry sources; `.cargo_vcs_info.json` kept, `Cargo.toml.orig`, `Cargo.lock`, `.github`,
dotfiles and zeno's `benches/` dropped. zeno's sources have CRLF line endings upstream; the repository stores them
with LF. swash keeps resolving skrifa 0.44 from crates.io (the vendored `skrifa` is parley's; it is the same
version but is not shared, to keep this change to the two crates).

### Changes against upstream (additive: no upstream function changed)

| crate | file | change | why |
|---|---|---|---|
| swash | `Cargo.toml` | `zeno` takes `path = "../zeno"` | use the vendored copy |
| swash | `src/scale/mod.rs` | `ScalerBuilder::build_unhinted` (`build` without the hinting cache: `hinting_instance: None`) | no hinting instance, so the TrueType interpreter and the autohinter are unreachable |
| swash | `src/scale/mod.rs` | `Scaler::scale_outline_unhinted_impl` (`scale_outline_impl(gid, None, None)` with the unhinted draw settings only) | the hinted arm of skrifa's `OutlineGlyph::draw` folds away |
| swash | `src/scale/mod.rs` | `render_outline_alpha(scaler, gid) -> Option<Image>`: `Render::new(&[Source::Outline]).format(Format::Alpha).render` for such a scaler, through `zeno::fill_alpha` | colour outlines, bitmap strikes (and yazi's inflate), the stroker and subpixel formats are unreachable |
| zeno | `src/fill.rs` (new), `src/lib.rs` (`mod fill; pub use fill::fill_alpha;`) | `fill_alpha(data, fill, origin, scratch, buf) -> Placement`: `Mask::with_scratch(..).format(Alpha).origin(..).style(fill).inspect(resize).render_into(.., None)` with no transform and zero offsets, step for step | `Mask` keeps a run-time `Style`, which links the stroker |

Gate: the frames of every pixel case (`bench/size/pixels.sh`: heavy, lab, animate, vlist, counter) are
byte-identical to the unmodified rasteriser's, and `cargo test --release -p bough-text` (the atlas, cache and window
tests) passes.

## Layout: cephal (size lane, `docs/lanes/developer-loop/`)

`vendor/cephal` is zortax/cephal `9e77f91` (0.2.0, vendored without history at the machine migration, commit
`ad79959f5d`). Changes against that commit:

| file | change | why |
|---|---|---|
| `src/tree/traits.rs` | `LayoutTree::compute_grid(&mut self, node, inputs)` (feature `grid`, `where Self: CacheAccess`), default body `compute::grid::compute_grid_layout(self, node, inputs)` | the grid algorithm becomes overridable like `compute_leaf`; every existing tree (`owned::View`, `Worker`, `Marker`) keeps the default, so their behaviour is unchanged |
| `src/compute/mod.rs` | `compute_uncached` calls `tree.compute_grid(..)` in its two grid arms instead of `compute_grid_layout` | a host can route grid layout through a function it installs only when it builds a grid style |

bough's `DView` overrides `compute_grid` with a function installed by the first `Display::Grid` style that enters
`lay_deep::Deep::intern` / `intern_style` (every style that can hold one: templates, restyles, reflection; the
prelude's `Spec::to_style` builds only flex and block). A program that never builds a grid style does not link the
grid algorithm (159,812 B of code under `production`). Gate: `crates/bough/tests/grid_hook.rs` (a grid template's
hand-computed geometry; with the install removed from `Deep::intern` it panics, which the test reports as a failure).
