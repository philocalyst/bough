# Where the bytes of a production bough executable go (baseline, 2026-11-11)

Report 1 of the binary-size card (`cards/2026-11-11-binary-size.md`). Tree `lead/round6-base` (aa9943e7ae),
nightly-2026-10-17, `[profile.production]` (opt-level 3, fat LTO, 1 codegen unit, `strip = true`), default features
(direct-Metal presenter), macOS 15 arm64, Command Line Tools only (no Xcode: no `metal` compiler, so bough-metal embeds
its MSL source and compiles it at startup). Nothing below was changed; this is the measurement the levers start from.

Method (`bench/size/`): `build.sh` builds the executables; an unstripped twin of each is linked with the same flags plus
`-C strip=none -C link-arg=-Wl,-map,<file>` (`cargo rustc` on the final crate only, so every dependency is the same
object). `attribute.py` reads the ld64 map: every symbol's size and section, demangled with the CLT's `llvm-cxxfilt`.
With fat LTO all Rust code is one object, so a symbol's crate is the first path of its demangled name (code inlined
across crates counts where it landed). Anonymous constants have no name; the large ones were identified by content
(hex dumps, `strings`). Section sizes are `size -m`, dylibs `otool -L`.

## Totals

| executable | stripped bytes | MiB |
|---|---:|---:|
| `crates/bough/examples/counter.rs` | **2,887,488** | 2.75 |
| `bough-awagy` (scroll) | **2,837,888** | 2.71 |
| `bough-awagy --features dashboard` | **3,185,008** | 3.04 |

(The AWAGY page's 2,788,192 B was the scroll app on the round-5 tree; this is the same profile on round 6.)

## Sections (counter)

| section | bytes | what it is |
|---|---:|---|
| `__TEXT,__text` | 2,201,712 | machine code |
| `__TEXT,__const` | 283,756 | read-only data: the MSL source (80 KB), rustybuzz's Unicode tables, `dec2flt` tables, jump tables |
| `__TEXT,__eh_frame` | 131,736 | DWARF unwind info: 1,718 FDEs, each with epilogue CFI (`remember_state`/`restore`, LLVM's async unwind tables) that compact unwind cannot express |
| `__DATA_CONST,__const` | 113,136 | relocated constants: vtables, `&str` tables (rustybuzz's language-tag table is 34 KB of it) |
| `__TEXT,__cstring` | 32,740 | C strings: 27,427 B are 284 source paths for panic `Location`s (absolute paths of the build machine's toolchain and registry included) |
| `__TEXT,__gcc_except_tab` | 29,632 | landing-pad tables (unwinding) |
| `__TEXT,__unwind_info` | 19,880 | compact unwind |
| stubs, `__DATA`, TLS | ~9,000 | |
| `__LINKEDIT` (stripped) | 49,152 | symbol table left after `strip -x`, dyld info, code signature |

## Code by subsystem (counter `__text`, 2,201,712 B)

| subsystem | crates | bytes | share |
|---|---|---:|---:|
| **glyph rasterisation** | skrifa 266,816 + zeno 151,988 + read_fonts 132,264 + swash 48,436 + yazi 19,912 + font_types 604 | **620,020** | 28.2 % |
| **layout** | cephal (block + flex + grid, every one instantiated over `bough::lay_deep::DView`) | **379,988** | 17.3 % |
| **complex-script shaping fallback** | rustybuzz (non-Latin-1 chunks only, `bough/src/text.rs`) | **214,636** | 9.7 % |
| font tables | ttf_parser (bough's table builder; rustybuzz's parser too) | 126,860 | 5.8 % |
| bough's own crates | bough 224,960 + counter (the app crate) 38,496 + bough_core 53,956 + bough_metal 26,840 + bough_text 26,764 + bough_mem 2,116 | 373,132 | 16.9 % |
| text grafts | parley (only `parley::bough::{script, shape, slots}` is reachable; parley's own pipeline, fontique, harfrust, skrifa-in-parley and the ICU crates are dropped by LTO) | 59,796 | 2.7 % |
| Rust runtime | core 219,468 + std 67,976 + alloc 38,100 + hashbrown 8,420 + smallvec 2,612 + compiler_builtins, `__rustc` | ~339,000 | 15.4 % |
| backtrace printing | gimli 33,228 + rustc_demangle 19,756 + addr2line 10,188 + object 3,072 (+ std's printer, below) | 66,244 | 3.0 % |
| Objective-C boundary | objc2 3,200 + objc2_foundation 2,352 + objc2_encode 1,856 + block2 920 + dispatch2 632 | 8,960 | 0.4 % |

Cross-cutting groups (by demangled name, overlapping the table above):

| group | bytes | functions |
|---|---:|---:|
| backtrace machinery (gimli/addr2line/object/demangle + `std::backtrace`/`std::sys::backtrace` + their generic instances) | 137,512 | 200 |
| slice sorts (`core::slice::sort::*` instances: 14 stable quicksort, 14 driftsort, 10 unstable) | 74,768 | 74 |
| `core::fmt` and `Debug`/`Display` impls | 58,488 | 167 |
| drop glue | 50,116 | 245 |
| objc2 / block2 / dispatch2 including instances named after them | 22,224 | 43 |
| float parsing (`dec2flt`, code + its 10,416 B power-of-five table; `BOUGH_KEEPWARM` and friends) | 12,154 | 9 |
| panic entry points (`panic_bounds_check`, `unwrap_failed`, …) | 9,112 | 79 |

The AWAGY apps: scroll's `__text` is 2,162,992 B with the same subsystems; dashboard's is 2,322,864 B and adds the PNG
decoder (png 38,340 + fdeflate 14,676 + crc32fast 3,872 + simd_adler32 712 = 57,600 B) and its ten embedded PNGs
(123,165 B of application data in `__TEXT,__const`).

## The 40 largest symbols (counter)

| # | bytes | section | crate | symbol |
|---:|---:|---|---|---|
| 1 | 80,485 | __TEXT,__const | anon | (anon) bough-metal shader.metal source, include_str! |
| 2 | 74,788 | __TEXT,__text | cephal | `cephal::compute::grid::track_sizing::intrinsic::size_batch::<bough::lay_deep::DView>` |
| 3 | 64,932 | __TEXT,__text | cephal | `cephal::compute::flex::compute_flexbox_layout::<bough::lay_deep::DView>` |
| 4 | 42,176 | __TEXT,__text | skrifa | `<skrifa::outline::glyf::hint::engine::Engine>::run` |
| 5 | 41,592 | __TEXT,__const | anon | (anon) unicode-properties General_Category ranges (rustybuzz) |
| 6 | 37,584 | __TEXT,__text | cephal | `cephal::compute::grid::compute_grid_layout::<bough::lay_deep::DView>` |
| 7 | 34,296 | __DATA_CONST,__const | anon | (anon) rustybuzz language -> OpenType tag table (pointers, relocated) |
| 8 | 29,724 | __TEXT,__text | zeno | `<zeno::mask::Mask<(&[zeno::geometry::Vector], &[zeno::command::Verb])>>::placement` |
| 9 | 27,444 | __TEXT,__const | anon | (anon) unicode-script Script ranges (rustybuzz) |
| 10 | 25,612 | __TEXT,__text | cephal | `cephal::compute::flex::fused::compute::<cephal::compute::flex::axis::Row, bough::lay_deep::DView>` |
| 11 | 25,172 | __TEXT,__text | cephal | `cephal::compute::flex::fused::compute::<cephal::compute::flex::axis::Column, bough::lay_deep::DView>` |
| 12 | 24,976 | __TEXT,__const | anon | (anon) rustybuzz canonical decomposition table |
| 13 | 23,680 | __TEXT,__text | rustybuzz | `rustybuzz::hb::ot_shape::substitute_pre` |
| 14 | 23,168 | __TEXT,__text | zeno | `zeno::path_data::apply::<&(&[zeno::geometry::Vector], &[zeno::command::Verb]), zeno::style::Style, zeno::raster::Rasterizer<zeno::raster::AdaptiveStorage>>` |
| 15 | 23,120 | __TEXT,__text | rustybuzz | `rustybuzz::hb::ot_shape::position` |
| 16 | 22,528 | __TEXT,__text | cephal | `cephal::compute::block::compute_inner::<bough::lay_deep::DView>` |
| 17 | 22,160 | __TEXT,__text | parley | `<parley::bough::script::SimpleScript>::build_face` |
| 18 | 19,840 | __TEXT,__text | bough | `bough::backend::host::run_interactive::<counter::Counter, <bough::ui::Application>::run<bough::ui::Window<counter::Counter>>::{closure#0}, <bough::ui::Applic...` |
| 19 | 19,008 | __TEXT,__text | zeno | `<zeno::scratch::Inner>::apply::<&(&[zeno::geometry::Vector], &[zeno::command::Verb]), zeno::raster::Rasterizer<zeno::raster::HeapStorage>>` |
| 20 | 18,688 | __TEXT,__text | skrifa | `skrifa::outline::autohint::metrics::scale::compute_unscaled_style_metrics` |
| 21 | 17,728 | __TEXT,__text | skrifa | `skrifa::outline::autohint::hint::hint_outline_impl` |
| 22 | 17,600 | __TEXT,__text | bough | `<bough::rt::Rt>::text_set` |
| 23 | 17,092 | __TEXT,__text | rustybuzz | `<rustybuzz::hb::ot_shape_plan::hb_ot_shape_plan_t>::new` |
| 24 | 15,440 | __TEXT,__const | anon | (anon) rustybuzz canonical composition table |
| 25 | 15,264 | __TEXT,__text | bough | `<bough::rt::Rt>::lay_compute_pending` |
| 26 | 15,196 | __TEXT,__text | yazi | `yazi::decode::inflate::<yazi::decode::VecSink>` |
| 27 | 14,196 | __TEXT,__text | bough | `<bough::rt::Rt>::lists_place` |
| 28 | 13,760 | __TEXT,__text | bough | `<bough::rt::Rt>::lay_writeback_ready` |
| 29 | 13,236 | __TEXT,__text | bough | `<bough::rt::Rt>::new` |
| 30 | 12,812 | __TEXT,__text | bough | `bough::text::font_tables` |
| 31 | 11,612 | __TEXT,__text | swash | `<swash::scale::Scaler>::scale_bitmap_impl` |
| 32 | 11,468 | __TEXT,__text | cephal | `cephal::compute::compute_child_layout::<bough::lay_deep::DView>` |
| 33 | 10,796 | __TEXT,__text | zeno | `<zeno::mask::Mask<(&[zeno::geometry::Vector], &[zeno::command::Verb])>>::render_into` |
| 34 | 10,708 | __TEXT,__text | skrifa | `<skrifa::outline::OutlineGlyph>::draw_unhinted::<skrifa::instance::LocationRef, swash::scale::outline::OutlineWriter>::{closure#0}` |
| 35 | 10,600 | __TEXT,__text | skrifa | `<skrifa::outline::glyf::FreeTypeScaler as skrifa::outline::glyf::Scaler>::load` |
| 36 | 10,416 | __TEXT,__const | core | `core::num::imp::dec2flt::table::POWER_OF_FIVE_128` |
| 37 | 10,172 | __TEXT,__text | rustybuzz | `rustybuzz::hb::ot_shaper_use::setup_syllables` |
| 38 | 9,900 | __TEXT,__text | std | `std::sys::backtrace::_print_fmt::{closure#1}` |
| 39 | 9,836 | __TEXT,__text | rustybuzz | `rustybuzz::hb::aat_layout_morx_table::apply_subtable` |
| 40 | 9,776 | __TEXT,__text | rustybuzz | `rustybuzz::hb::tag_table::tags_from_complex_language` |

## Embedded data

| data | bytes | in | note |
|---|---:|---|---|
| bough-metal `shader.metal` (1,596 lines, `include_str!`) | 80,485 | all three | 117 comment lines; compiled at startup by `newLibraryWithSource` because the build machine has no `metal` compiler. With Xcode a `.metallib` is embedded **as well** (`load_library` references both) |
| rustybuzz's Unicode data | 41,592 + 27,444 + 24,976 + 15,440 + 34,296 = 143,748 | all three | General_Category ranges (unicode-properties), Script ranges (unicode-script), canonical decomposition and composition, the language-to-OpenType-tag table (relocated, in `__DATA_CONST`) |
| ICU / UAX #14 / #29 tables of vendored parley | 0 | none | graft 3b walks ICU's baked tables only in parley's own pipeline, which bough never calls: not linked |
| font tables | 0 baked | | fonts are read from disk at run time and bough's token table is built at startup (`SimpleScript::build_face`, 22,160 B of code) |
| SVG decoder (resvg / usvg / tiny-skia) | 0 | none | linked only when `svg_image` is reachable; none of these apps use it |
| PNG decoder | 57,600 code | dashboard | `ui::image`; swash also links its own bitmap-glyph path (`scale_bitmap_impl` 11,612 + yazi's inflate 19,912) in all three, unused with `Source::Outline` only |
| panic `Location` file paths | 27,427 | all three | 284 strings, including `/Users/<builder>/.local/share/...` paths |
| `core::num::dec2flt` power-of-five table | 10,416 | all three | |
| dashboard PNG assets | 123,165 | dashboard | application data |

## Linked dylibs (counter, `otool -L`)

AppKit, CoreData, CoreText, CoreImage, CloudKit, QuartzCore, CoreVideo, CoreGraphics, Metal, IOSurface, Foundation,
CoreFoundation, libSystem, libiconv, libobjc. CoreData, CoreImage, CloudKit and CoreText come from objc2-app-kit's
default framework features and libiconv from the `libc` crate; bough calls none of them. They cost load commands
(bytes: negligible) and dyld work at launch (startup: not measured here).

## Generic instantiations (AUDIT B4 baseline)

| executable | functions in `__text` | generic instances | generic items | bytes in instances |
|---|---:|---:|---:|---:|
| counter | 2,237 | 1,207 | 266 | 1,097,932 (50.1 %) |
| awagy scroll | 2,207 | 1,208 | 260 | 1,104,560 (51.3 %) |
| awagy dashboard | 2,374 | 1,276 | 278 | 1,164,608 (50.4 %) |

Counted from the LTO'd symbol table, so instances that were fully inlined are not counted (the count is of
out-of-line copies). The instance bytes are dominated by large single instances (cephal's layout algorithms over
`DView`: 86 instances, 362,028 B; zeno's path pipeline: 54, 147,332 B), not by many copies. Code instantiated over
the application's view type, i.e. what every new `View` adds: counter 44 functions / 75,708 B, scroll 47 / 91,352 B,
dashboard 56 / 125,304 B. The largest are `host::run_interactive::<V, ..>` (13–20 KB), `App<V>::update_hover`
(8,960 B, identical in all three), `App<V>::settle_input`, the per-shape `Element::mount` of the builder types
(`LBox<Color, ((), Para<String>)>` 16,436 B in scroll) and two block2 invoke shims (12,596 B).

## Build times (secondary; not optimised)

| build | wall | note |
|---|---:|---|
| clean `cargo build --profile production -p bough --example counter` | 132.3 s (user 186.6 s) | 116 crates, `CARGO_BUILD_JOBS=4`, load average 50–65 from other sessions: indicative only |
| incremental after touching `counter.rs` (app-only edit) | 22.9 s | fat LTO + 1 codegen unit: the whole program is re-optimised and re-linked |

## What this says about the levers (ordering for the card's amendment)

1. **Unwinding and panics: ~370 KB.** eh_frame 131.7 KB + LSDA 29.6 KB + backtrace printing 137.5 KB + panic
   paths 27.4 KB + a share of `core::fmt` and `__unwind_info`. Build configuration only (std rebuilt with
   `panic_immediate_abort`, sync unwind tables), no source change.
2. **Glyph rasterisation: 620 KB of code** for one job (unhinted outline coverage). Hinting (skrifa's TrueType
   interpreter and autohinter), colour/bitmap glyphs (+ yazi), and zeno's stroker are linked because they are chosen
   by runtime values (`hint(false)`, `Render::new(&[Source::Outline])`, `Style`). Calling the outline and fill paths
   directly should make them unreachable without touching the pixels.
3. **cephal: 380 KB** (grid 159,812, flex 148,552, block 26,180, out-of-flow 20,568, the rest shared). Grid is
   ~160 KB that no prelude app can reach; the layout dispatch reaches it at
   run time. cephal is on the vlist hot path (rows mount while scrolling).
4. **rustybuzz: ~360 KB** (215 KB code + 144 KB tables) for text outside Latin-1. A capability, not dead code.
5. **The MSL source: 80 KB**: without comments and indentation it is 50,860 B; deflated, 10,163 B.
6. **Per-crate opt-level** on the cold crates above (rasteriser, fallback shaper, font parsing), proven with
   instruction counts.
7. Smaller: float parsing (12 KB), objc2 (9–22 KB: not worth replacing by hand-written `objc_msgSend` externs on
   bytes alone), per-view monomorphisation (76–125 KB, the B1/B4 work), sort instances (75 KB).
