# Damage tracking, invalidation, and repaint: mechanisms across production renderers

Scope: how mature renderers represent "what changed", propagate it, and turn it into a set of pixels to redraw, with a closing list of design levers for a retained-mode Rust GUI framework.

Evidence standard: every claim is tagged.
- **[verified]**: read in a fetched primary source during this research, with the URL given.
- **[memory]**: from prior knowledge, not re-read in this session. Treat as a lead to check against the source named.
- **[gap]**: a source was requested but could not be read (blocked, too large, or a navigation shell only).

Read-throughs that failed are listed in section 10 so nobody assumes they were covered.

---

## 1. Chromium (Blink, cc, Viz)

### 1.1 Pipeline shape [verified]
Source: https://developer.chrome.com/docs/chromium/renderingng-architecture

- Pre-paint "computes property trees and invalidates any existing display lists and GPU texture tiles as appropriate." Paint then produces a display list. Commit copies property trees and the display list to the compositor thread.
- Layerize breaks the display list into a composited layer list. Raster turns display lists into GPU texture tiles. Activate creates a compositor frame describing how to draw and position tiles, plus visual effects.
- The Aggregate stage in Viz combines compositor frames from all visible sources into one global compositor frame.
- The doc reports 12 pipeline stages, 3 process types, and walkthroughs of 15, 14, and 5 steps for render, scroll, and animation.
- Scroll and compositor-thread animations mutate property trees directly and re-run the compositor lifecycle without touching the main thread.

### 1.2 Representation of "what changed" [memory, not re-read]
Check these in source before relying on them.
- Blink: `PaintInvalidator` (`third_party/blink/renderer/core/paint/paint_invalidator.cc`) walks the layout tree and records per-object invalidation flags (paint rect invalidation, full paint invalidation, raster invalidation). `PaintChunk` and `PaintChunker` form the `PaintArtifact`, the list of chunks with property tree state IDs; the `PaintArtifactCompositor` turns chunks into cc layers. Raster invalidation is computed by comparing chunks between frames in `RasterInvalidator` (`third_party/blink/renderer/platform/graphics/paint/raster_invalidator.cc`), producing a list of `RasterInvalidationInfo` rects per layer.
- cc: `DamageTracker` (`cc/trees/damage_tracker.cc`) accumulates per-render-surface damage rects bottom-up through the render surface tree, and `LayerImpl::update_damage` / `damage_rect` feed it. Region type is `cc::Region` (a `gfx::Rect` list with an internal union that is simplified via `Region::Simplify`/`Region::Union`).
- Viz: `SurfaceAggregator` (`components/viz/service/surfaces/surface_aggregator.cc`) computes per-surface damage from `CompositorFrame::render_pass_list` (`RenderPass::damage_rect`) and unions it into the output pass's damage rect, which is what the GPU renderer uses for partial swap and scissor.

### 1.3 Propagation [memory]
- Blink invalidation is top-down-marking, bottom-up-rect-accumulation: a paint-invalidation flag set on a `LayoutObject` propagates to ancestors only as a "subtree needs check" bit; the actual rects are then computed in a second walk using the object's visual rect in the property-tree coordinate space. This is why the property trees exist: a transform change invalidates by moving the rect, not by repainting.
- cc: damage propagates from layers up the render-surface tree, transformed into each surface's space; a surface's damage is the union of its own layers' damage after applying the surface's transform and clip.

### 1.4 From damage to pixels [memory + verified partial]
- The compositor's output render pass carries a damage rect. The renderer uses it for a scissor on the framebuffer (and on partial swap if the platform supports it). Buffer age (section 7) decides how much to redraw when the back buffer is not preserved.
- Raster: invalidated tiles are re-rastered; tiles are typically 256x256 or device-dependent (check `cc/tiles/tile_priority.h` and `TilingData`); this is [memory].

### 1.5 Memory cost [memory]
- Per-layer: a `cc::Region` is a small vector of rects, typically one or a few. Chunks are a vector of `PaintChunk` (bounds, id, property state, display-item range); cost is O(chunks), not O(pixels).

### 1.6 Worst cases [memory]
- Large fixed-position or `will-change: transform` layers with overlapping damage unions can grow into a bounding box that covers most of the screen (region simplification fallback to bounds).
- Many small chunks (e.g., a DOM with thousands of absolutely positioned elements, each its own chunk) make the chunk diff O(chunks) per frame.

### 1.7 Published numbers
- RenderingNG docs give pipeline counts (above) but no damage-area or timing numbers. [verified: no numbers on damage]

---

## 2. Firefox WebRender

### 2.1 Tile invalidation, from source [verified]
Source: https://github.com/servo/webrender/blob/main/webrender/src/picture.rs (fetched; module docs and types quoted)

- `TILE_SIZE_DEFAULT` is **1024 x 512 device pixels** (the doc comment says "The size in device pixels of a normal cached tile."). Scrollbar tiles are 1024x32 (horizontal) and 32x1024 (vertical). Note the module-level docs say "2048x512 pixels (or 128x128 for the UI slice)", which disagrees with the constant. Trust the constant.
- Per-tile state, from the `Tile` struct: `local_dirty_rect`, `device_dirty_rect`, `current_descriptor`, `prev_descriptor`, `is_valid`, `invalidation_reason`, and a root `TileNode` (a quadtree).
- The per-frame dependency lists are "built each frame and compared to the previous frame to see if the tile changed." The dirty rect is "the union of the invalidated leaves of each quadtree" [verified].
- Invalidation reasons are an enum that includes `PrimCount`, `Content`, `ValidRectChanged`, `ScaleChanged` [verified].
- Per-primitive comparison results include `Descriptor`, `Clip`, `Transform`, `Image` [verified].
- Transforms are compared by value through `SpatialNodeComparer`, so an index change in the spatial tree alone does not invalidate [verified, comment truncated in fetch].
- Tiles that are off-screen skip dependency setup; their dependencies are compared "the next time it comes into view." `update_content_validity` "must be called even if the tile is not currently visible" [verified].
- Primitive identity uses `ItemUid` from `crate::intern`. The `PrimitiveDescriptor` / `TileDescriptor` / `TileCacheInstance` definitions were not in the visible part of the fetched file [gap].

### 2.2 Interning and diffing [memory + partial verified]
- Display list primitives are interned: the `intern` module stores a `PrimitiveKey`-style struct per unique primitive (rect, clip, color, etc.) in a `DataStore`, and display items reference them by `ItemUid` handle. The interning makes identity comparison cheap (handle equality) and lets the tile descriptor be a small list of handles plus a few hashes, rather than a full primitive copy.
- The source to read is `webrender/src/intern.rs` and the `TileDescriptor` definition in `webrender/src/picture.rs` (not fetched).

### 2.3 Propagation [verified partial]
- The dependency model is "built each frame and compared to the previous frame." Propagation is from primitives (leaves) to tile quadtree leaves to the tile's dirty rect. Parent/child invalidation is the quadtree union.

### 2.4 From damage to pixels [memory]
- Dirty rects from the composited tiles are unioned into the frame's dirty region, which is passed to the compositor as partial present damage (on platforms that support it, e.g., via DirectComposition or the native compositor). Non-dirty tiles are not re-rastered.
- Picture caching ("picture" = a surface whose content is cached) lives in `webrender/src/picture.rs`; the wiki's picture-caching pages were not visible in the fetch (16 pages hidden) [gap].

### 2.5 Memory cost [memory]
- Each tile keeps two descriptors (current and previous) and a quadtree. At 1024x512 RGBA8, one tile's texture is 2 MiB (1024*512*4 = 2,097,152 bytes). Tile count scales with viewport area / tile area, so a 2560x1440 viewport at default tile size needs about 3 x 3 = 9 tiles per picture.

### 2.6 Worst cases [memory]
- Animated transforms on a tile cause invalidation of every tile the primitive touches each frame.
- Many primitives with changing descriptors (e.g., text with per-frame position updates in a scrolling list) produce a large union dirty rect.

### 2.7 Published numbers [gap]
- The wiki page "GPU fill rate baseline" and "Q2 2020 WR Perf Notes" were listed but not read. Check them for measured numbers before quoting any.

---

## 3. Vello, Xilem, and Raph Levien's writing

### 3.1 Vello architecture [gap]
- The repo README did not name pipeline stages. The fetched page says "Vello CPU and Vello GPU share the Sparse Strips architecture and common infrastructure" and that `vello` (compute) does most work in compute shaders. Source: https://github.com/linebender/vello (fetched).
- Binning, coarse rasterization, and fine rasterization stage names: these are from prior knowledge of the original `vello` compute pipeline ([memory]: path flattening, binning into 16x16 tiles, coarse rasterization producing per-tile command lists, fine rasterization per tile with a fixed workgroup). Confirm in `ARCHITECTURE.md` in the repo; not fetched.
- Sparse strips is the newer approach shared by `vello_cpu` and `vello_gpu`: paths are split into strips (4-pixel-tall rows, sparse) and only strips with coverage are emitted [memory]. Confirm in `ARCHITECTURE.md`.

### 3.2 Raph's posts (fetched index) [verified list]
Source: https://raphlinus.github.io/ (fetched index). Titles and paths, all relevant:
- "The compositor is evil" (2020-09-13): https://raphlinus.github.io/ui/graphics/2020/09/13/compositor-is-evil.html
- "Xilem: an architecture for UI in Rust" (2022-05-07): https://raphlinus.github.io/rust/gui/2022/05/07/ui-architecture.html
- "Swapchains and frame pacing" (2021-10-22): https://raphlinus.github.io/ui/graphics/gpu/2021/10/22/swapchain-frame-pacing.html
- "A sort-middle architecture for 2D graphics" (2020-06-12): https://raphlinus.github.io/rust/graphics/gpu/2020/06/12/sort-middle.html
- "Fast 2D rendering on GPU" (2020-06-13): https://raphlinus.github.io/rust/graphics/gpu/2020/06/13/fast-2d-rendering.html
- "piet-gpu progress: clipping" (2022-02-24): https://raphlinus.github.io/rust/graphics/gpu/2022/02/24/piet-gpu-clipping.html

No post in the index is titled for "incremental rendering" or "sparse strips" as such. The fetched index had no incremental-rendering post.

### 3.3 "The compositor is evil" [verified]
Source: https://raphlinus.github.io/ui/graphics/2020/09/13/compositor-is-evil.html
- Compositors add a frame or more of latency. Best case 33 ms, worst 66 ms, mean 50 ms, from input to scanout in the post's model.
- The 16 ms budget applies to render plus composite together at 60 Hz. "The compositor typically composites all windows on every frame."
- Damage regions: early windowing systems wrote into the framebuffer and the damage was visible until repaint.
- Proposal: a beam-racing renderer that schedules a "minimal damage region" update to the scanline. Hardware tiling proposal uses **16x16 tiles** classified as "simple" (hardware composable) or "complex".
- The post gives **no** measured numbers comparing partial updates against full re-render. Its numbers are latency and the 16x16 tile size.

### 3.4 Xilem/Masonry damage [gap]
- The Masonry/Xilem repos were not fetched. Prior knowledge: Masonry (formerly Druid) uses a widget tree with per-widget `needs_paint`/`needs_layout` flags and bubbles flags up through the widget pass; the scene is rebuilt into a Vello `Scene` each frame, which means full scene encoding per frame and damage at the GPU level only. This is [memory]. Verify in `masonry/src/passes/` before relying on it.

---

## 4. Flutter

### 4.1 Status [gap]
The fetched Flutter page was the Performance landing page, not the rendering docs. The content below is from prior knowledge and must be checked against https://docs.flutter.dev/perf/rendering-performance and the framework source (`packages/flutter/lib/src/rendering/object.dart`, `layer.dart`, `proxy_box.dart`).

- `markNeedsPaint()` marks the render object and walks up to the nearest `RenderObject` whose `isRepaintBoundary` is true; only that boundary's layer is repainted in the next paint phase. `markNeedsLayout()` walks up to the nearest relayout boundary (a node whose size does not depend on its parent's constraints). [memory]
- Layer tree: `OffsetLayer`, `ClipRectLayer`, `OpacityLayer`, `PictureLayer`, composed into a `SceneBuilder` scene. [memory]
- Raster cache: caches rasterized picture layers in the Skia-era engine; Impeller changed the caching rules. [memory; check current docs]
- Frame budget: 16.7 ms at 60 Hz, 8.3 ms at 120 Hz (verified as general rule from the fetched page's section, though the page itself is light).

Propagation: marking is up-to-boundary, painting is down-the-tree from dirty boundaries. Flutter does not compute a region of damage per frame in the Chromium sense; it re-records only dirty layers and re-composes the layer tree every frame.

---

## 5. Apple, Android, Skia

### 5.1 Skia SkRegion [verified]
Source: https://api.skia.org/classSkRegion.html (fetched)
- "compact, efficiently storing a single integer rectangle, or a run length encoded array of rectangles." Iteration returns rectangles "sorted along y-axis, then x-axis."
- Copy-on-write: copies share the pointer; the rect array is copied only on modification.
- Ops: `kDifference_Op`, `kIntersect_Op`, `kUnion_Op`, `kXOR_Op`, `kReverseDifference_Op`, `kReplace_Op`. Construction from rects (`setRects`, union of the array) and from paths (`setPath`).
- Internal: `RunHead` and `Runs` describe spans per scanline band. Specifics were not in the fetched page [gap; check `src/core/SkRegion.cpp`].

### 5.2 Android [partial]
Source: https://developer.android.com/guide/topics/graphics/hardware-accel (fetched, but this was the Compose/overview page, not the View RenderNode page).
- Overdraw guideline: don't draw more than about **2.5x** the screen pixel count per frame; transparent bitmap pixels count.
- `Modifier.alpha` renders off-screen and doubles fill rate; `CompositingStrategy.ModulateAlpha` avoids it for non-overlapping content.
- Object reuse: avoid allocating `Paint`/`Path` per frame.
- The View-system RenderNode/damage accumulator details are on "Hardware acceleration (Views)" (linked from the fetched page), not fetched [gap]. From prior knowledge: `RenderNode` holds a display list recorded by `DisplayListCanvas`; `invalidate(rect)` propagates a dirty rect up the View hierarchy to the root; `ViewRootImpl` unions into a `dirty` rect and passes it to `ThreadedRenderer` with the damage accumulator's buffer-age logic [memory].

### 5.3 Apple Core Animation / UIKit [gap]
Not fetched. Prior knowledge: `CALayer` `setNeedsDisplay` / `setNeedsDisplayInRect:` marks a dirty rect; the render server composites layers and uses the dirty region for `CAMetalLayer` partial updates (no public partial present API on CAMetalLayer; the common trick is to render only the damaged region with a load action of `.load` on a persistent drawable, which is not reliable because drawables are not guaranteed to preserve contents). [memory; check the Apple docs before citing.]

### 5.4 Skia Graphite [gap]
Not fetched.

---

## 6. Region algebra and damage containers

### 6.1 Representations
- **Single rect** (bounding box accumulator): O(1) union, O(1) query, can over-redraw badly. Used as a fallback everywhere.
- **Banded rect list (pixman, X11 regions) [memory]**: `pixman_region16_data_t { long size; long numRects; }` followed by an array of `pixman_box16_t { int16 x1, y1, x2, y2; }`, sorted by y then x, with an `extents` box. Bands share y1/y2. Union/intersect are O(n + m) sweep-line merges.
- **Run-length spans per scanline (SkRegion) [verified]**: a sorted run list per band; ops are sweeps over runs; iteration is y-then-x.
- **Grid or bitmap tiles [memory]**: one bit per tile (e.g., 16x16 or 64x64 device pixels) in a bitset. Union/intersect are word-wise OR/AND (SIMD-friendly, 256 bits per AVX2 op); query is a bit test. Over-redraws to tile granularity; memory is (W/t)*(H/t)/8 bytes, so a 2560x1440 surface at 16x16 is 160*90 = 14,400 bits = 1,800 bytes; at 64x64 it is 40*23 = 920 bits = 115 bytes.
- **Quadtree (WebRender TileNode) [verified]**: per-tile dirty rect built as the union of invalidated leaves. Query is O(log n); union of leaves is O(leaves).
- **R-tree / BVH of dirty rects [memory]**: good for many sparse small rects; insert/query O(log n) with high constant; not SIMD-friendly.

### 6.2 Cost comparison (order of magnitude, from first principles)
- Union of two banded lists with k and m rects: O(k + m) branchy merge. For tiny k, m (typical UI damage: 1-10 rects) this is faster than anything else.
- Bitset union for a 2560x1440 surface at 16x16 tiles: 225 bytes of OR, about 8 AVX2 operations. Constant-time regardless of rect count, and cache resident.
- Bounding-box union: 4 min/max ops. Always cheapest; over-redraw is the price.

Recommendation: accumulate damage as a small fixed-capacity rect list (say up to 8 rects) with a bounding-box fallback, and promote to a tile bitset only when the list overflows. This is a design judgment, not a measured result.

---

## 7. GPU-side damage

### 7.1 Buffer age (EGL_EXT_buffer_age) [verified]
Source: https://registry.khronos.org/EGL/extensions/EXT/EGL_EXT_buffer_age.txt (fetched)
- Age 0: buffer contents undefined (just initialized or reallocated); redraw everything. Single-buffered surfaces always report 0.
- Age 1: previous swap was a copy; back buffer holds the previous frame. Always 1 with `EGL_BUFFER_PRESERVED`.
- Age n > 1: back buffer holds contents from n frames ago. Double-buffered flip usually reports 2, triple usually 3.
- Age updates on each `eglSwapBuffers`. The app keeps the last n frames' damage and redraws the union for age n.
- Caveats: query after binding the surface; the app must track pixel ownership (X11 expose/configure events); memory pressure can free back buffers and reset age to 0; `EGL_BUFFER_PRESERVED` is discouraged in favor of tracking buffer age.

Implication for a framework: keep a ring of the last `N` frames' damage rects (N = swap chain depth, typically 2-3), indexed by age. For age n, redraw the union of the last n-1 frames' damage plus this frame's damage.

### 7.2 Vulkan incremental present [gap]
VK_KHR_incremental_present spec fetch exceeded the 10 MB fetch limit. From prior knowledge: `VkPresentRegionsKHR` attaches `VkPresentRegionKHR` with an array of `VkRectLayerKHR { VkOffset2D offset; VkExtent2D extent; uint32_t layer; }`. The rectangles are a hint; drivers may ignore them and present the full image. Coordinates are in pixels; for the origin convention check the spec's `VkRectLayerKHR` section. Verify before citing.

### 7.3 DXGI, Metal [gap]
- DXGI `Present1` takes `DXGI_PRESENT_PARAMETERS` with `DirtyRectsCount` and `pDirtyRects`; scroll rects via `pScrollRect`/`pScrollOffset`. Memory-level, not fetched.
- Metal/CAMetalLayer: no public dirty-rect present; tricks are documented in Apple docs and are not fetched.

### 7.4 Scissor versus stencil damage masks
- Scissor: a single rect; clipping is free in hardware, but only one rect per pass. Multi-rect damage needs one pass per rect or a stencil.
- Stencil: write the damage mask once (e.g., from a bitmap or a set of quads), then draw with the stencil test enabled; cost is one extra pass over the mask and a stencil buffer, but a multi-rect damage set costs one draw per primitive instead of one draw per rect per primitive.
- The claim "damage through a stencil of itself" (attributed to zgx in the brief) was not verified; no source was fetched for it.

---

## 8. Terminal emulators

### 8.1 Alacritty [verified]
Source: https://github.com/alacritty/alacritty/blob/master/alacritty_terminal/src/term/mod.rs (fetched)
- `LineDamageBounds { line, left, right }`. A line is damaged when `left <= right`. `undamaged()` sets `left` to the column count and `right` to 0.
- `TermDamage<'a>`: `Full` (redraw everything) or `Partial(TermDamageIterator)` (changed lines only).
- `TermDamageIterator` skips lines outside the display offset (scrollback).
- `TermDamageState` (private): `full: bool`, `lines: Vec<LineDamageBounds>` (one per row), `last_cursor: Point`. Starts `full = true`.
- `Term::damage()` returns `Full` if the flag is set, else damages old and new cursor and returns `Partial`. `reset_damage()` clears per frame. `mark_fully_damaged()` is called by scroll, alt-screen swap, resize, option changes.
- Memory: one `LineDamageBounds` (three integers, 12 bytes with u32s; the field types are not confirmed) per row. For 50 rows that is 600 bytes.
- The fetched file did not show `write_at_cursor`, so how each cell write maps to `damage_line` is unverified [gap].

### 8.2 Ghostty, kitty, WezTerm [gap]
Not fetched. Prior knowledge: Ghostty uses per-row dirty flags with a full-screen fallback; kitty uses a per-line dirty bit plus scroll-region damage; WezTerm tracks dirty lines in its surface. [memory; verify each before citing.]

### 8.3 Lesson
Terminals are the cleanest real example of "row-granularity damage plus a full-redraw flag". The full flag is set by any operation that moves many rows at once. That is a deliberate coarsening: scroll becomes "full" rather than "moved region plus new rows". A framework can do better with a scroll-blit (section 9) but should keep the full flag as the escape hatch.

---

## 9. Immediate-mode and arena-based layouts

### 9.1 Clay [verified]
Source: https://github.com/nicbarker/clay (fetched; first 100k chars of a longer page)
- Arena: caller supplies memory. `Clay_CreateArenaWithCapacityAndMemory(capacity, memory)`. "Static arena based memory use with no malloc / free." Quoted: about 3.5 MB for 8192 layout elements. `Clay_MinMemorySize()` gives the required size.
- Output: `Clay_EndLayout()` returns `Clay_RenderCommandArray { length, internalArray }`. Each `Clay_RenderCommand` has `commandType`, `boundingBox`, `renderData` (union), and `uint32_t id`. Output is a "sorted list of rendering primitives." Off-screen elements are culled by default.
- The docs point out that `Clay_RenderCommand` is "a small enough struct that you can simply compare the memory of two render commands" to detect dirty elements in a retained renderer. This is the lightest damage-detection method in the set: a byte compare per command.
- Strings: `Clay_String` slices point into the original text; wrapping does not allocate.
- Text measurement is on the hot path; the library caches measurements. The docs claim "microsecond layout performance" with no per-frame numbers.

This is a precise match for the "flat array + memcmp" idea. The gap: the fetched page does not explain how the render command array is diffed across frames; the memcmp claim is the library's own suggestion, not a shipped diff.

### 9.2 Dear ImGui [gap]
Not read; the GitHub fetch returned navigation only. From prior knowledge: `ImDrawList` holds `ImVector<ImDrawVert>` (pos float2, uv float2, col u32 = 20 bytes per vertex), `ImVector<ImDrawIdx>` (16-bit indices by default, 32-bit when `ImDrawIdx` is redefined), and `ImVector<ImDrawCmd>` where each command is `{ ClipRect, TextureId, VtxOffset, IdxOffset, ElemCount }`. A frame is rebuilt from scratch each time. Verify in `imgui.h` / `imgui_draw.cpp`.

### 9.3 egui, dvui, microui [gap]
Not fetched. Prior knowledge: egui tessellates the shape list into one mesh per frame; dvui (Zig) and microui (Odin/C, rxi) use a command buffer with a fixed-size byte arena. All three redraw fully. [memory]

### 9.4 Why full redraw stays fast
Mechanisms that are verified or strongly supported:
- Flat arrays with no per-node allocation (Clay: fixed arena; ImGui: vectors that retain capacity across frames [memory]).
- The redraw is a linear pass over commands with no tree traversal; cache behavior is sequential.
- The GPU work is one or a few draw calls per clip change, so the CPU cost is dominated by building the command list, not by submitting it.

---

## 10. Source coverage and failures

- Fetched and used: RenderingNG architecture doc; Raph's blog index and "compositor is evil"; WebRender `picture.rs`; WebRender wiki index (no mechanism pages visible); EGL_EXT_buffer_age; Vello repo README; Alacritty `term/mod.rs`; Clay repo; SkRegion API docs; Flutter performance landing page (not the rendering page); Android hardware-acceleration overview (not the View page).
- Failed or not read: Vulkan spec (too large); pixman `pixman-region.c` (blocked by bot protection); ImGui `imgui_draw.cpp` (nav only); Flutter rendering-performance page; Apple docs; Skia Graphite; Ghostty, kitty, WezTerm; egui, dvui, microui; WebRender wiki picture-caching pages (16 hidden); Vello `ARCHITECTURE.md`; Masonry sources.
- No fetched source contained a published damage-area or frame-time number for partial vs full repaint. The only numeric results are the ones listed above (tile sizes, latency, overdraw factor, memory of Clay arena, Chromium pipeline counts).

Next steps that would most improve this report: read `ARCHITECTURE.md` in linebender/vello, `webrender/src/intern.rs` and the picture caching wiki pages, `cc/trees/damage_tracker.cc`, `pixman-region.c`, and the Flutter rendering-performance page. Each is one fetch or one clone.

---

## 11. Design levers

Ranked by expected impact on O(changed) work with a cache-friendly traversal. Each cites the source that motivates it. Items marked [memory] rest on sources not read this session.

1. **Dirty-bit propagation with a separate rect pass.** Mark dirty flags up the tree (cheap, bounded by changed nodes), then compute rects in a second pass over only dirty nodes. Motivated by Blink pre-paint/paint split and property-tree mutation [verified: RenderingNG doc, pipeline description]. Why: avoids re-walking unchanged subtrees; transforms move rects without repainting.

2. **Property-style split for transform/opacity/clip changes.** Changes to transform, opacity, or clip mutate a small side tree and move cached content instead of re-recording it. Motivated by Chromium property trees and scroll/animation running on the compositor thread [verified: RenderingNG]. Why: animation of a moving panel costs O(1) nodes, not O(subtree).

3. **Spatial-compare by value for invalidation, not index.** Compare spatial nodes by value so that tree reshaping alone does not invalidate [verified: WebRender `SpatialNodeComparer`]. Why: avoids spurious full invalidation after structural edits.

4. **Interned primitive handles plus per-tile descriptor diff.** Each cached tile keeps a small list of handles and hashes for the primitives it contains; invalidation is a diff of two lists [verified: WebRender `PrimitiveDependencyInfo`, `ItemUid`, descriptor compare]. Why: change detection is O(primitives in tile) with no pixel work.

5. **Two-tier damage container: small rect list with bounding-box fallback, bitset on overflow.** Motivated by SkRegion's run list and pixman's banded regions for the small case [verified: SkRegion; memory: pixman]. Why: union of 1 to 8 rects is faster than a bitset for typical UI damage; the bitset stays constant-cost when many rects appear. Section 6.2 is a first-principles estimate, not a measurement.

6. **Tile grid with 1 bit per tile for the coarse damage map.** At 16x16 device pixels, a 2560x1440 surface is 1,800 bytes; union is a word-wise OR [memory for layout; the 16x16 tile size is quoted in Raph's hardware-tiling discussion, verified]. Why: SIMD-friendly, constant-time, cache-resident.

7. **Retain damage history indexed by buffer age.** Keep the last N frames' damage (N = swap depth), and redraw the union for the current back buffer's age [verified: EGL_EXT_buffer_age]. Why: correct partial redraw on any swap model without `EGL_BUFFER_PRESERVED`.

8. **Full-redraw escape hatch with a flag.** Terminals set a `full` flag on scroll, resize, and mode switch [verified: Alacritty `mark_fully_damaged`]. Why: many operations are rare and expensive to track exactly; the flag keeps the common path cheap.

9. **Per-row or per-line bounds for text-like content.** Alacritty's `LineDamageBounds { line, left, right }` gives a per-row horizontal span [verified]. Why: a single keystroke marks one span, not a rect over the whole view.

10. **Flat render command array with byte-compare diffing.** Clay emits a sorted array of fixed-size commands and suggests a memcmp to find changed elements [verified: Clay docs]. Why: one linear compare, no tree walk, trivially cache-friendly. Caveat: command order must be stable across frames.

11. **Arena-allocated layout and render output, reset per frame.** Clay's static arena with no malloc or free [verified: Clay docs, about 3.5 MB for 8192 elements]. Why: allocation cost goes to zero and cache locality improves.

12. **Stencil-based damage masks for many small rects.** One stencil write, one masked draw per primitive, rather than one scissored draw per rect [memory; the zgx-style trick was not verified]. Why: avoids N draw calls when damage is fragmented.

13. **Scroll as a blit plus a damage strip.** Move the existing pixels by the scroll delta and damage only the newly exposed strip. Alacritty instead sets `full` on scroll [verified]; Chromium moves tiles with scroll offset in property trees [verified: RenderingNG]. Why: scrolling is the most common full-screen change.

14. **Raster cache keyed by picture identity, invalidated by descriptor change.** WebRender caches picture tiles and invalidates on descriptor mismatch [verified]; Flutter caches rasterized layers [memory]. Why: unchanged content costs one texture sample per frame.

15. **Coalesce invalidation per frame and per node.** Accumulate dirty flags and rects during event handling and layout, flush once in the paint pass. Chromium's pre-paint and commit model and Flutter's `markNeedsPaint` both batch this way [verified for Chromium stages; memory for Flutter]. Why: a burst of 1,000 setters should cost one dirty pass.

---

*Report generated under the research brief; not reviewed by a second reader.*

---

## Second pass (source-verified)

Method: shallow sparse clones under `/Users/mileswirht/projects/bough/research/src/` (vello, webrender, chromium, pixman, ghostty, kitty, flutter, egui, imgui). Every claim below is tagged `[verified file:line]` (read in the clone, path relative to the repo root) or `[gap]`. Line numbers refer to the default branch at clone time (2026-11).

### Corrections to the first pass

1. **Chromium `DamageTracker` does not use `cc::Region`.** The first pass said it did. `cc/trees/damage_tracker.h:66-108` defines `DamageAccumulator`, which is a single bounding rectangle (`x_, y_, right_, bottom_`) plus a `DamageReasonSet`. `Union` is min/max on the four edges (`damage_tracker.h:69-96`). `cc::Region` is used for tile invalidation, not frame damage (see 3.3).
2. **Two different "tile" sizes exist in vello.** The Sparse Strips pipeline uses 4x4-pixel tiles (`vello_common/src/tile.rs:263-266`). The older compute pipeline uses 16x16-pixel tiles grouped into 16x16-tile bins (`research/vello_shaders/src/cpu/binning.rs:9-12`). The first pass conflated them.
3. **WebRender tile-size claim.** `TILE_SIZE_DEFAULT` is 1024x512 (`webrender/src/picture.rs:266-270`). The module doc saying 2048x512 is stale; trust the constant.
4. **Ghostty Dirty flags.** The first pass implied a single per-row flag. Ghostty has per-row bits (in `Row`) plus terminal- and screen-level flag structs (see 5.1).

### 1. Vello (Sparse Strips, compute pipeline)

Repo layout changed: the compute renderer lives under `research/`, and the production CPU/GPU renderers are `vello_common`, `vello_cpu`, and (per the doc) `vello_gpu`. `[verified ARCHITECTURE.md:21-40]` says the shared CPU pipeline is in `vello_common` and paths are "flattened, binned into tiles, and represented as sparse horizontal strips," with "fully covered regions ... represented compactly." `[verified ARCHITECTURE.md:44-60]` says the compute renderer is `research/`. `vello_hybrid` is not in the clone; `[gap]`.

**(a) Representation.**
- Tile: `vello_common/src/tile.rs:216-260`. `#[repr(C)] struct Tile { x: u16, y: u16, packed_winding_line_idx: u32 }` (8 bytes). The low 5 bits are an intersection/winding mask (T, B, L, R, W); bits 5-31 are a line index. Sorting by the packed word orders tiles by (x,y) then line index. `WIDTH = HEIGHT = 4` (`tile.rs:263-266`).
- Strip: `vello_common/src/strip.rs:17-26`. `struct Strip { x: u16, y: u16, packed_alpha_idx_fill_gap: u32 }`, 8 bytes. Bit 31 is `fill_gap`; bits 0-30 are the index into the alpha buffer (`strip.rs:20-27, 29-53`).
- Fill segments: `StripFillSegment { tile_x0: u16, tile_x1: u16, tile_y: u16 }` (`strip.rs:57-62`), a 6-byte run with no alpha. `StripAlphaFillSegment { fill, alpha_idx: u32 }` (`strip.rs:32-37`) adds an alpha offset.

**(b) Propagation / the core loop.**
- `visit_strip_fill_segments` (`strip.rs:116-200`) walks consecutive strip pairs. For each strip it emits an alpha segment covering the strip's tile width, clipped to the viewport (`strip.rs:161-177`). Then, if `next_strip.fill_gap()` is set, it emits a no-alpha `StripFillSegment` for the gap between strips (`strip.rs:178-198`). So interior runs of full coverage cost one 6-byte record and no alpha reads.
- `emit_culled_background` (`strip.rs:287-310`) handles geometry culled to the left of the viewport. It uses `row_windings` to fill rows that have nonzero winding, so culling does not drop backgrounds.

**(c) Pixels.** The CPU renderer consumes strips into a pixmap (`vello_cpu/src/render.rs`, `vello_cpu/src/fine/`; `[gap]` for the exact per-tile loop). Solid spans are emitted as `CMD_SOLID` by the compute coarse stage (below).

**Empty and solid tile skipping (compute pipeline, verified).**
- Binning: `research/vello_shaders/shader/binning.wgsl:55-120`. Each draw object's bbox is intersected with the clip bbox (`binning.wgsl:70-86`). It is converted to bin coordinates, and an empty intersection is forced to `x1 = x0`, `y1 = y0` so the object lands in no bin (`binning.wgsl:88-92`, comment at 83-85). Bins are 16x16 tiles (`research/vello_shaders/src/cpu/binning.rs:11-12`).
- Coarse: `research/vello_shaders/shader/coarse.wgsl:88-110`, `write_path`. If a tile has no segments (`n_segs == 0`), it writes a one-word `CMD_SOLID` and no fill. Otherwise it writes a 4-word `CMD_FILL` with segment offset and backdrop. The CPU mirror is `research/vello_shaders/src/cpu/coarse.rs` (around line 80-96), same branch.
- Fine: `research/vello_shaders/shader/fine.wgsl:1065-1110`. The per-tile command interpreter loop reads a PTCL (per-tile command list). `CMD_SOLID` sets `area[i] = 1.0` for all pixels and skips edge evaluation (`fine.wgsl:1105-1110`). `CMD_FILL` calls `fill_path` (`fine.wgsl:1096-1103`).
- Early-out: if an earlier stage failed, `ptcl[0] == ~0u` makes fine return immediately (`fine.wgsl:1072-1076`).

**Complexity.** Binning is O(objects x bins-touched). Coarse is O(bins x partitions) with a prefix-sum over element counts (`coarse.wgsl:~200-240`). Fine work per tile is proportional to segments in the tile, with solid tiles costing one command.

**Memory.** 8 bytes per strip and 8 bytes per tile (`strip.rs`, `tile.rs`). A solid run costs 6 bytes (`StripFillSegment`) no matter how many pixels it covers.

**Worst cases.** Many thin, nearly horizontal edges make every 4-pixel strip non-solid and maximize tile count. Failure of an intermediate buffer (bump allocator overflow) forces a full re-run at larger sizes (`binning.wgsl:57-66`, `coarse.wgsl:~30-45`).

**Published numbers.** `[gap]` in the repo docs fetched; the vello_cpu thesis is linked from ARCHITECTURE.md:58-60 and was not read.

### 2. WebRender (intern, picture tiles, dirty regions, composite)

**(a) Interning.** `webrender/src/intern.rs`.
- `ItemUid { uid: u64 }` (`intern.rs:101-106`), a globally unique id for an interned item.
- `Handle<I> { index: u32, epoch: Epoch, _marker }` (`intern.rs:115-120`). Index plus epoch, so a stale handle is detectable after slot reuse.
- `Interner<I> { map: FastHashMap<I::Key, ItemDetails<I>>, free_list: Vec<usize>, update_list, current_epoch, ... }` (`intern.rs:237-250`).
- `ItemDetails { interned_epoch, last_used_epoch, index, ... }` (`intern.rs:207-215`). `last_used_epoch` drives garbage collection.
- `intern(&mut self, data: &I::Key, fun)` (`intern.rs:271-300`): a hash lookup first; on hit it updates `last_used_epoch` and returns a handle (no clone of the key, comment at 276-278). On miss it pops `free_list` or appends, then creates the handle and stores `ItemUid`.

Interning means a primitive's identity is a `(u64)` handle, and per-frame change detection compares handles instead of full primitive payloads.

**(b) Dirty-rect propagation.** `webrender/src/picture.rs`.
- Tile dirty state: `Tile { local_dirty_rect, device_dirty_rect, current_descriptor, prev_descriptor, is_valid, invalidation_reason, root: TileNode }` (`picture.rs`, struct around 780-800; `update_dirty_rects` method at `picture.rs:863-890`). `[verified]` from the first-pass fetch plus this read.
- Each leaf of the per-tile quadtree (`TileNodeKind::Leaf`) stores `prev_indices` and `curr_indices`, which are primitive index lists. Leaf comparison is at `picture.rs:7553-7605` (`TileNode::update_dirty_rects`):
  - If the two index lists differ in length, the leaf is dirty with `InvalidationReason::PrimCount`.
  - Otherwise, for each (prev, curr) pair it looks up `compare_cache: FastHashMap<PrimitiveComparisonKey, PrimitiveCompareResult>`. This caches comparisons across leaves. On the first non-`Equal` result it sets `invalidation_reason = Content`, unions `self.rect` into `dirty_rect`, and sets `dirty_tracker |= 1`, then breaks.
- Leaf history: `dirty_tracker: u64` (`picture.rs:7216-7218`) is a bitset of the last 64 frames. Each frame it is shifted left by 1 (`picture.rs:7346-7352`). Split/merge heuristic uses `dirty_tracker.count_ones()` (`picture.rs:7397-7410`).
- Dirty regions per picture cache: `DirtyRegion { combined: WorldRect, spatial_node_index }` (`picture.rs:1477-1482`). `add_dirty_region` maps a picture-space rect to world space and unions into `combined` (`picture.rs:1507-1530`). Note: this is a bounding rect, not a region list.

**(c) Pixels: composite and partial present.** `webrender/src/composite.rs`.
- `CompositorConfig::Draw { max_partial_present_rects, draw_previous_partial_present_regions, partial_present, ... }` (`composite.rs:291-305`). Default is full present (`composite.rs:341-346`).
- `PartialPresentCompositor::set_buffer_damage_region(&mut self, rects: &[DeviceIntRect])` (`composite.rs:1366-1372`). The doc says the region is "the frame's dirty region and some previous frames' dirty regions ... calculated using the buffer age." So WebRender implements buffer-age damage on the framework side and hands the union to the platform.

**(d) Memory.** Tile texture at 1024x512 RGBA8 = 2,097,152 bytes. Per-leaf: two index vecs, one `u64` tracker, and the descriptors `[gap]` for exact struct sizes. Interner: one `ItemDetails` per unique item plus the map entry.

**(e) Worst cases.** Any primitive whose descriptor changes every frame (animated transform, live text) marks every leaf it belongs to. The `Transform` compare is by value (`SpatialNodeComparer`, first pass). Invisible tiles skip dependency setup and are compared lazily when they come into view (`picture.rs` `pre_update`, first pass).

**(f) Published numbers.** `[gap]`: the wiki's "GPU fill rate baseline" and "Q2 2020 WR Perf Notes" were not read.

### 3. Chromium (cc, tiling, Viz)

**3.1 DamageTracker (frame damage).** `cc/trees/damage_tracker.h`, `.cc`.
- `class DamageAccumulator` (`damage_tracker.h:66-108`) is one rectangle with four ints plus `reasons_` (`DamageReasonSet`). `IsEmpty()` tests `x_ == right_ || y_ == bottom_`. `Union` (`:69-96`) skips empty input, merges reasons, and expands the rect (min/max). There is an `is_valid_rect_` flag for "invalid" accumulation.
- `ComputeSurfaceDamage` (`damage_tracker.cc:186-270`). Steps:
  1. `TrackDamageFromLeftoverRects()` gets damage from layers and surfaces that were removed (`:194`).
  2. For each contributing surface, in back-to-front order, set `intersects_damage_under` if the surface's rect intersects the damage (`:214-230`). Surfaces with pixel-moving backdrop filters are expanded to the full surface rect (`:219-226`).
  3. Union, map through `Filters().MapRect` (`:243-258`), and reset to a single rect.
  4. If the surface property changed, union `content_rect()` and set `kUntracked` (`:263-270`).
- The comment at `:186-190` says the accounting "cannot be bypassed with early-exits," so the tracker state stays correct across frames.

Complexity: O(contributing surfaces) per target surface plus O(layers with damage). Memory: O(1) per surface. Worst case: two far-apart damaged corners produce a bounding box covering most of the surface. The design gives up precision for O(1) union; reasons bits keep diagnosability.

**3.2 Viz SurfaceAggregator.** `components/viz/service/display/surface_aggregator.cc`.
- Root damage: `root_damage_rect_ = prewalk_damage_rect; root_damage_rect_.Union(target_damage);` (`surface_aggregator.cc:2261-2264`). It is a single rect, used to cull quads that do not intersect it (comment at `:2262-2263`; the cull test is near `:1640`).
- Per-pass surface damage list: `surface_damage_rect_list_` (`:442-562`). Each entry is a damage rect in root target space; quads with `has_per_quad_damage` are excluded from overlay assignment (`:574-580`). Empty damage is recorded as `gfx::Rect()` so indices stay aligned (`:597-602`).
- Overlay damage: a quad's damage index is assigned only when the damage rect is pixel-aligned (`:614-626`).

**3.3 cc::Region and tile invalidation.** `cc/base/region.h`, `cc/tiles/picture_layer_tiling.cc`.
- `class Region` is backed by an `SkRegion`: `explicit Region(const SkRegion& region);` (`cc/base/region.h`, class declaration around line 22-30).
- `PictureLayerTiling::RemoveTilesInRegion(const Region& layer_invalidation, bool recreate_tiles)` (`picture_layer_tiling.cc:283-330`):
  - For each rect in the Region (`:294`), convert to content space and intersect with `expanded_eventually_rect` (`:299-303`).
  - Iterate tiles intersecting that rect, including borders, via `TilingData::Iterator` (`:305-310`). Accumulate `remove_tiles[index].Union(invalid_content_rect)` (`:311-313`).
  - For each affected tile: `TakeTileAt`, then if `recreate_tiles` create a new tile and call `SetInvalidated(invalid_content_rect, old_tile->id())` (`:316-327`). The raster job re-rasters only that tile.
- Complexity: O(#invalidation rects x tiles-per-rect). Memory: O(#tiles touched).
- TODO at `:316-320` (crbug 490847) notes that pending-tree commits lose the old tile id, so invalidation info is dropped there.

**3.4 Published numbers.** The RenderingNG architecture page (first pass) gives counts, not damage numbers. `[gap]` for default tile size in `picture_layer_tiling`.

### 4. Pixman regions (y-x banded)

Source: pixman (`gitlab.freedesktop.org/pixman/pixman`); the GitHub `freedesktop/pixman` path does not exist. `pixman/pixman.h`, `pixman/pixman-region.c`.

**(a) Representation.**
- `struct pixman_region16_data { long size; long numRects; };` followed in memory by `pixman_box16_t rects[size]` (`pixman.h:462-466`).
- `struct pixman_box16 { int16_t x1, y1, x2, y2; };` (`pixman.h:474-477`), 8 bytes.
- `struct pixman_region16 { pixman_box16_t extents; pixman_region16_data_t *data; }` (`pixman.h:479-482`). The `extents` box is the bounding box. A "nil" region has `numRects == 0`; a broken region uses a sentinel `pixman_broken_data` (`pixman-region.c:72-74`).
- Layout rules are documented at `pixman-region.c:127-160`: rects are sorted by y1 then x1; a band is a set of rects sharing y1 and y2; bands are implicit (no band pointer array); rects in one band do not touch; bands are merged when adjacent and identical (`pixman-region.c:156-160`, coalesce logic around `:604-660`).

**(b) Algorithms.**
- Generic sweep `pixman_op` at `pixman-region.c:778-...`. It walks two band-sorted arrays `r1` and `r2`, and for each y-interval calls an `overlap_func` (`pixman_region_union_o` at `:1290`, `pixman_region_intersect_o` at `:1138`, `pixman_region_subtract_o` at `:1898`) on overlapping bands, and appends non-overlapping bands when `append_non1` / `append_non2` is set.
- Complexity: each op is a single merge sweep, O(n + m) in rect count, plus coalescing O(output).
- Insertion of arbitrary rect lists goes through validation, which sorts the rectangles with `quick_sort_rects` (`pixman-region.c:1533`, used around `:1693`). Complexity O(k log k) for k rects.

**(c) Pixels.** Regions are used as clip/damage sets and are iterated as rect arrays (`pixman_region_rectangles`, `pixman.h`); each rect becomes a fill.

**(d) Memory.** 8 bytes per rect plus 16 bytes header. For a 2560x1440 surface with 1-10 damage rects, the region is under 100 bytes.

**(e) Worst case.** Vertically crossing rects are split into stacked pieces (`pixman-region.c:127-160`, the diagram). A diagonal line of k small rects produces O(k) bands and O(k) rects with no coalescing benefit.

### 5. Terminals: Ghostty and kitty

**5.1 Ghostty** (`ghostty-org/ghostty`, `src/terminal/`, `src/renderer/`).
- Per-row bit: `Row` is `packed struct(u64)` (`src/terminal/page.zig:2146`), and it carries a `dirty` bit used by `RowDirtyMask = page.Mask(page.Row, &.{"dirty"}, scan_group_len)` (`src/terminal/render.zig:2621-2630`, `scan_group_len = 8`).
- Terminal-level flags: `Terminal.Dirty` packed struct with `palette`, `reverse_colors`, `clear`, `preedit`, `glyph_glossary`, ... (`src/terminal/Terminal.zig:233-250`). `Screen.Dirty` with `selection` and `hyperlink_hover` (`src/terminal/Screen.zig:90-97`). Hyperlink hover sets the whole screen dirty because links can span rows (`Screen.zig:91-93`).
- Clearing: `PageList.clearDirty` walks pages and clears `page.dirty` and each `row.dirty` (`src/terminal/PageList.zig:7220-7230`).
- The render-state update (`src/terminal/render.zig`): for each page chunk, if not `redraw` and not `page_dirty`, scan rows in groups of 8 with `RowDirtyMask.match`. A matching group is skipped with `@branchHint(.likely)` (`render.zig:1238-1248`). Each dirty row in a group is cleared and passed to `builder.row(...)` (`render.zig:1249-1262`). The tail (fewer than 8 rows) is scanned one by one (`:1264-1270`).
- Complexity: O(rows/8) group tests when clean; O(dirty rows) rebuild. A group of 8 `u64` rows is 64 bytes, one cache line.
- Full-redraw path: `Terminal.Dirty.clear` and `page_dirty` force a rebuild of the chunk (`render.zig:1201`, `:1228-1236`).

**5.2 kitty** (`kovidgoyal/kitty`, `kitty/`).
- Per-line bit: `union LineAttrs { struct { uint8_t has_dirty_text:1; has_image_placeholders:1; prompt_kind:2; padding:4; }; uint8_t val; }` (`kitty/line.h:88-96`). One byte per line.
- Set and clear: `linebuf_mark_line_dirty` and `linebuf_mark_line_clean` (`kitty/line-buf.c:42-54`, `:65`, `:228`). `linebuf_clear_attrs_and_dirty` clears attrs.
- Screen-level flag: `Screen.is_dirty` (`kitty/screen.c`, set in ~20 places, e.g. `:157, :256, :279`; cleared at `:3993`).
- GPU path: `kitty/shaders.c:1370` calls `update_cell_data` when `reload_all_gpu_data || scroll_changed || is_dirty || screen_resized || ...`.
- `screen_update_cell_data` (`kitty/screen.c:4124-4175`) iterates all lines (`linebuf_init_line`), but calls `render_line` only if `has_dirty_text` or the cursor moved (`:4166-4172`). When the GPU buffer was invalidated while rendering was paused, every row is rewritten (`:4127-4131`).
- Complexity: O(lines) bit tests per frame, O(dirty lines) of text shaping and cell writes. `[gap]` on whether the GPU upload is partial or whole-buffer; I did not trace the buffer upload.

**Lesson for us.** Both terminals use a byte or bit per row, a screen-level "full" flag, and a cheap scan. kitty's flag is per line and is independent of the others. Ghostty's group scan is the only vectorised form seen.

### 6. Flutter (object.dart, layer.dart)

Source: `flutter/flutter`, `packages/flutter/lib/src/rendering/`.

**(a) Dirty marking.** `markNeedsPaint` (`object.dart:3345-3395`):
- Early return if `_needsPaint` is already set (`object.dart:3348-3350`). This deduplicates repeated marks in one frame, so a burst of setters costs one pass up the tree.
- If `isRepaintBoundary && _wasRepaintBoundary`: add `this` to `owner._nodesNeedingPaint` and call `requestVisualUpdate()` (`:3360-3367`). The boundary is the paint root.
- Else: `parent!.markNeedsPaint()` (recursive, `:3375-3380`). If there is no parent, call `owner?.requestVisualUpdate()`.
- Complexity: O(depth) for the first mark, then O(1) per additional mark in the same frame.

`markNeedsLayout` (`object.dart:2683-2711`): the same dedupe at `:2685`. If `_isRelayoutBoundary` is true, add to `owner._nodesNeedingLayout` and request a visual update (`:2692-2700`). Else `markParentNeedsLayout()` (`:2714-2716`).

Relayout boundary is computed in `layout()`: `_isRelayoutBoundary = !parentUsesSize || sizedByParent || constraints.isTight || parent == null;` (`object.dart:2870`). A child whose size does not depend on the parent's constraints cuts propagation.

**(b) Retained engine layers.** `layer.dart`.
- Invariant comment at `layer.dart:358-363`: `alwaysNeedsAddToScene` implies `_needsAddToScene`; if `_needsAddToScene` is true and there is a parent, the parent's flag is true too.
- Field: `bool _needsAddToScene = true;` (`layer.dart:372`).
- `alwaysNeedsAddToScene` (`layer.dart:~407-410`) disables retention for a subtree.
- `markNeedsAddToScene` propagates to parents unless the parent `alwaysNeedsAddToScene` (`layer.dart:~465-482`).
- `_addToSceneWithRetainedRendering` (`layer.dart:~698-716`): if `!_needsAddToScene && _engineLayer != null`, emit `builder.addRetained(_engineLayer!)` and return. Otherwise call `addToScene(builder)`, then clear the flag after the call (`:~712`).

Complexity: a retained subtree costs one `addRetained` call per frame. The flag walk is O(changed path length).

**(c) Pixels.** Dirty repaint boundaries re-record `PictureLayer`s. Clean subtrees are referenced as engine layers (`addRetained`). The engine composites them.

**(d) Memory.** One `EngineLayer` per retained boundary (native texture/picture ref). `[gap]` for exact sizes.

**(e) Worst case.** Wrapping a fast-animating widget in a boundary is cheap; putting a boundary in each leaf creates one layer per leaf (the memory cost of layers, plus `alwaysNeedsAddToScene` subtrees that never retain).

**Gap:** the raster cache and Impeller behaviour are in `flutter/engine` (C++), not in this clone.

### 7. egui and Dear ImGui

**7.1 Dear ImGui** (`ocornut/imgui`).
- `ImDrawVert { ImVec2 pos; ImVec2 uv; ImU32 col; }` (`imgui.h:3275-3280`), 20 bytes by default (`imgui.h:189`).
- `typedef unsigned short ImDrawIdx;` (`imgui.h:3235`): 16-bit indices by default.
- `ImDrawCmd` (`imgui.h:3253-3262`): `ClipRect (ImVec4)`, `TexRef`, `VtxOffset`, `IdxOffset`, `ElemCount`, `UserCallback`. One command is typically one draw call.
- `ImDrawList` holds `ImVector<ImDrawCmd> CmdBuffer`, `ImVector<ImDrawIdx> IdxBuffer`, `ImVector<ImDrawVert> VtxBuffer` (`imgui.h:3415-3417`).
- `_ResetForNewFrame` (`imgui_draw.cpp:449-~470`): `CmdBuffer.resize(0); IdxBuffer.resize(0); VtxBuffer.resize(0);`. `ImVector::resize` only reallocates when `new_size > Capacity` (`imgui.h:2298`), so capacity is kept across frames. After warm-up, the frame allocates nothing.
- `_TryMergeDrawCmds` (`imgui_draw.cpp:580`) merges adjacent commands with identical clip and texture, reducing draw calls.
- Complexity: O(vertices + indices) per frame, emitted linearly. No tree walk, no per-frame allocation after warm-up.
- Full redraw cost: the whole list is rebuilt, so the cost is the emit loop, which is cache-friendly and branch-light.

**7.2 egui** (`emilk/egui`, `crates/egui/src/`).
- Repaint requests: `Context::request_repaint` calls `request_repaint_of(viewport_id)` (`context.rs:2128-2145`). The doc at `:2132-2137` says: "If this is called at least once in a frame, then there will be another frame right after this. Call as many times as you wish, only one repaint will be issued."
- Delayed repaint: `request_repaint_after` (`context.rs:2179`) and `request_repaint_after_for(duration, id)` (`context.rs:2222-2225`), which creates a `RepaintCause` (`context.rs:285-294`: `file`, `line`, `reason`). The cause is kept for debugging (`repaint_causes`, `context.rs:2254`).
- So: repaint is a boolean per viewport per frame, plus scheduled wakeups. No damage rect.
- Tessellation (shapes to meshes) lives in `crates/epaint`, which is not in this clone. `[gap]`.
- Complexity: O(1) per request; the frame itself is full.

**Lesson.** ImGui and egui get cheap full redraws from (1) capacity-retaining vectors (no per-frame malloc), (2) one linear emit, (3) draw-call merging, and (4) a boolean "need a frame" plus scheduled wakeups instead of damage tracking.

---

## Revised design levers (after second pass)

Changes from the first-pass ranking are marked. The first-pass list had propagation and region ideas ranked high; the source reading shows Chromium's frame damage is a single bounding rect, and the cheapest proven wins are dirty-flag dedupe, retained-subtree invariants, and per-leaf history bits.

1. **Dedupe dirty marks with an early return and a per-frame node list (Flutter `object.dart:3348-3367`).** Marking is O(1) after the first mark, and repaint roots are a list. Unchanged from first pass in spirit; now source-backed.
2. **Retained subtree output with a single `needs` flag and a parent-propagation invariant (Flutter `layer.dart:358-363, 372, ~698-716`).** A clean subtree costs one reference per frame. NEW as a named lever.
3. **Bounding-rect frame damage with reason bits (Chromium `damage_tracker.h:66-108`).** O(1) union. Demotes region algebra to tile invalidation only. CHANGED: previously ranked as a two-tier region container; now the default is a single rect, with regions only where tile granularity matters.
4. **Per-leaf `u64` dirty history for buffer-age damage (WebRender `picture.rs:7216-7352`).** One shift and one OR per frame gives 64-frame history. Pairs with `set_buffer_damage_region` (`composite.rs:1366-1372`). NEW.
5. **Interned primitive handles with epoch-tagged generation and GC by last-used epoch (WebRender `intern.rs:101-120, 207-215, 271-300`).** Compare handles, not payloads. Unchanged; now with the handle layout.
6. **Per-leaf prim-index lists diffed with a per-frame compare cache keyed by (prev, curr) pair (WebRender `picture.rs:7553-7605`).** Dedupes comparisons across leaves that share primitives. NEW.
7. **Solid-run encoding that skips per-pixel work (Vello strips `strip.rs:57-62, 161-198`; compute `coarse.wgsl:88-110`, `fine.wgsl:1105-1110`).** A solid span costs 6 bytes and no alpha reads. NEW as a named lever; first pass only mentioned empty tiles.
8. **Tile-granular invalidation via region iteration with tile recreate (cc `picture_layer_tiling.cc:283-330`).** Only for large scrollable/raster surfaces; the region itself is SkRegion-backed. CHANGED: now explicitly scoped to tile-cache invalidation, not frame damage.
9. **Group-of-8 dirty scan on a 64-byte row record (Ghostty `render.zig:1238-1270, 2621-2630`).** O(n/8) clean scan with one cache line per group. NEW.
10. **One byte of dirty bits per line/row, separate from a screen-level "full" flag (kitty `line.h:88-96`, `screen.c:3993, 4124-4175`).** Cheap, bit-packed; the screen flag is the escape hatch. NEW.
11. **Capacity-retaining command and vertex vectors, cleared not freed (ImGui `imgui.h:2298`, `imgui_draw.cpp:449-470`).** Zero per-frame malloc after warm-up; full redraw is a linear emit. Unchanged, now sourced.
12. **Command-buffer memcmp for change detection (Clay, first pass).** Still plausible for flat arrays; not re-verified in this pass.
13. **Banded y-x region with O(n+m) merge ops and coalescing (pixman `pixman-region.c:127-160, 778, 1138, 1290`).** Good for small rect lists used as clip/damage sets; not the frame-damage path. CHANGED: demoted below the bounding rect.
14. **Repaint as a boolean per viewport plus scheduled wakeups (egui `context.rs:2128-2225`).** Removes idle CPU for animation-free UIs. NEW.
15. **Stencil or scissor masks for fragmented damage (first pass; `[memory]`, not re-verified in these clones).** Keep as a GPU-side fallback, below the CPU-side levers.

**Ranking change summary.** Top three are now the framework-independent wins (dirty dedupe, retained subtree flag, bounding-rect accumulator). Region containers moved from the top of the list to a tile-granularity role. The per-leaf history bit and solid-run encoding entered the top ten. The ordering is a judgement from the code read, not from measurements; no benchmark numbers were obtained in this pass.

## Gaps remaining after the second pass

- Vello `vello_hybrid`, `vello_gpu`, and the vello_cpu thesis: not in the clone, or not read.
- WebRender `composite.rs` compositing path beyond the trait (`[gap]` on DirectComposition / native compositor specifics).
- Chromium: default tile size in `picture_layer_tiling` and the `ComputeSurfaceDamage` callers (`[gap]`).
- Ghostty: how the GPU cell buffer is uploaded after `builder.row` (`[gap]`).
- kitty: whether the GPU cell upload is partial or full (`[gap]`).
- Flutter: raster cache and Impeller (in engine, not this clone) (`[gap]`).
- egui: tessellation and the caching, if any, in `crates/epaint` (`[gap]`).
- No measured numbers (frame times, damage areas) were obtained from any source in this pass.
