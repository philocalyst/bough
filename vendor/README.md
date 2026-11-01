# Vendored crates

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

## The reference

`crates/bough-text` also depends on unmodified **upstream parley 0.11.1 from crates.io** (dev-dependency
`parley_upstream`, which pulls upstream fontique/harfrust/skrifa from crates.io too). Every graft is gated by
`crates/bough-text/tests/parley_twin.rs` and the gates built on `tests/common/twin.rs`: the grafted parley's layout
(lines, runs, glyph ids and positions as f32 bits, clusters and their boundary flags) must equal upstream's on the
same input.

## Changes against upstream

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
