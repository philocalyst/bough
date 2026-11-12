# R6 second pass: GPU binning, occlusion, and CPU SIMD/threads for a retained 2D renderer

Scope: four narrow topics, each grounded in source code or design documents read this pass. The first pass (`game-engines.md`) was broad; this one is deep and deliberately shorter where sources were not read. Every claim is tagged with where it came from. Claims tagged **UNCONFIRMED** were not verified in a primary source and should not be used as design inputs without a check.

Target renderer (from the brief): Rust retained GUI renderer on Apple silicon, Metal compute, 32x32 damage tiles, a bin kernel that builds per-damaged-row primitive lists, and a tile kernel that scans the whole row list (about 230 entries scanned to find 5 hits on glyph-dense rows).

## Sources actually read (primary text, this pass)

Read as source text (raw files or fetched page text):

1. https://raw.githubusercontent.com/linebender/vello/main/research/vello_shaders/shader/binning.wgsl
2. https://raw.githubusercontent.com/linebender/vello/main/research/vello_shaders/shader/coarse.wgsl
3. https://raw.githubusercontent.com/linebender/vello/main/research/vello_shaders/shader/fine.wgsl (grep and excerpts, not full read)
4. https://raw.githubusercontent.com/linebender/vello/main/research/vello_shaders/shader/tile_alloc.wgsl
5. https://raw.githubusercontent.com/linebender/vello/main/research/vello_shaders/shader/path_count.wgsl (grep and excerpts)
6. https://raw.githubusercontent.com/linebender/vello/main/research/vello_shaders/shader/shared/{bump,tile,ptcl,config}.wgsl
7. https://raw.githubusercontent.com/linebender/vello/main/ARCHITECTURE.md
8. https://raw.githubusercontent.com/linebender/vello/main/research/vello_encoding/src/config.rs (grep)
9. https://raw.githubusercontent.com/linebender/vello/main/research/vello_research/src/render.rs (grep)
10. https://raw.githubusercontent.com/servo/pathfinder/master/shaders/d3d11/{bin,tile,propagate,fill}.cs.glsl
11. https://raw.githubusercontent.com/servo/pathfinder/master/resources/shaders/metal/d3d11/{bin,tile,propagate}.cs.metal (grep)
12. https://raw.githubusercontent.com/servo/pathfinder/master/README.md
13. https://raw.githubusercontent.com/google/forma/master/forma/src/cpu/painter/layer_workbench/passes/tile_unchanged.rs
14. https://raw.githubusercontent.com/google/forma/master/forma/src/cpu/painter/layer_workbench/passes/skip_fully_covered_layers.rs
15. https://raw.githubusercontent.com/google/forma/master/README.md
16. https://raw.githubusercontent.com/servo/webrender/main/webrender/src/rectangle_occlusion.rs
17. https://raw.githubusercontent.com/servo/webrender/main/webrender/src/tile_cache.rs (header and config)
18. https://raw.githubusercontent.com/servo/webrender/main/webrender/src/picture.rs (lines about 5190-5310 and 3890-3950)
19. https://raw.githubusercontent.com/servo/webrender/main/webrender/src/composite.rs (around line 1413-1490, Occluders)
20. https://www.advances.realtimerendering.com/s2017/2017_Sig_Improved_Culling_final.pptx (Drobot, slides 1-82, text extracted from the slide XML)
21. https://www.advances.realtimerendering.com/s2017/index.html (talk list)
22. https://raw.githubusercontent.com/RoaringBitmap/RoaringFormatSpec/master/README.md
23. https://raphlinus.github.io/rust/graphics/gpu/2020/06/12/sort-middle.html
24. https://raphlinus.github.io/rust/graphics/gpu/2020/06/13/fast-2d-rendering.html
25. https://raphlinus.github.io/gpu/2020/09/05/stack-monoid.html (re-checked; no sort-middle content)
26. https://raphlinus.github.io/ (index, for the post list)
27. https://raw.githubusercontent.com/servo/stylo/main/style/{bloom.rs, parallel.rs, sharing/mod.rs, traversal.rs} (grep and excerpts)
28. https://raw.githubusercontent.com/bevyengine/bevy/main/crates/bevy_camera/src/visibility/mod.rs (grep and excerpt of `check_visibility_cpu_culling`)
29. https://raw.githubusercontent.com/bevyengine/bevy/main/crates/bevy_render/src/view/visibility/mod.rs (grep)
30. https://arxiv.org/abs/1902.08318 (simdjson abstract only)
31. https://arxiv.org/abs/1603.06549 (Roaring abstract only)

Attempted, not readable (binary PDF, no text tool installed in this environment):

- http://www.cse.chalmers.se/~uffe/clustered_shading_preprint.pdf (Olsson, Billeter, Assarsson 2012). **UNCONFIRMED** body; the fetch returned compressed PDF streams.

Search snippets only (not read as documents): Unity Entities docs (16 KiB chunk), DICE "Culling the Battlefield" (slideshare and ea.com pages found, not read), Apple Metal Shading Language spec (only the table of contents was indexed), Dawn/Tint `subgroupExclusiveAdd` to `simd_prefix_exclusive_sum` mapping commit.

Not opened at all: Practical Clustered Shading (Olsson and Persson), Pathfinder's `resources/shaders/metal/d3d11/bin.cs.metal` body beyond the grep, Forma's `painter/mod.rs` body, WebRender `composite.rs` sweep body, Stylo's work-unit constant, the Servo traversal body, simdjson full paper, Roaring full paper, Skia Graphite and Impeller.

Correction to the brief: Vello's compute shaders are at `research/vello_shaders/shader/` in the current repository (the path `vello_shaders/shader/` does not exist; the directory listing from the GitHub tree API confirmed the `research/` prefix). Vello's tile size is **16x16 pixels**, and a bin is **16x16 tiles (256x256 px)**, not 32x32 tiles (`shared/config.wgsl`, constants `TILE_WIDTH`, `N_TILE_X`).

---

## 1. Vello (compute path): exact bin, allocation, ptcl, overflow

### Mechanism, in order

1. **path_count / tile_alloc (per draw object).** `tile_alloc.wgsl` computes each draw object's tile rectangle in tiles, clipped to the target. A workgroup of 256 threads does an inclusive scan over tile counts in shared memory; the last thread does one `atomicAdd(&bump.tile, count)` per workgroup and broadcasts the base offset through workgroup memory. Each object writes `Path { bbox, tiles: tile_offset + subix }`. The tile array is zeroed by the same workgroup. A comment records that broadcasting the offset through `paths` with storage barriers hung DX12/FXC (linebender/vello#1991), so the code uses workgroup memory instead.
2. **binning.wgsl (per draw object x per bin).** A workgroup handles 256 consecutive draw objects (`WG_SIZE = 256`, one element per thread). Each thread clips its object's bbox to bins (`SX = 1/(N_TILE_X*TILE_WIDTH)`). Bin coverage is written as a **bitmask**: `sh_bitmaps[N_SLICE=8][N_TILE=256]` of `atomic<u32>`, bit `i` in slice `i/32` means "element `slice*32+i` touches this bin". Coverage is processed in blocks of 256 bins to bound shared memory.
3. **Counting and chunk allocation (inside binning.wgsl).** Per bin, the count is `countOneBits` over the eight slices, with partial counts packed two 16-bit values per `u32` (`sh_count`). The workgroup allocates one contiguous chunk per bin block with a single `atomicAdd(&bump.binning, element_count)`. The header for each (workgroup, bin) is `BinHeader { element_count, chunk_offset }`.
4. **Scatter without atomics on the output.** Each element writes `bin_data[chunk_offset + rank]`, where `rank = popcount(mask & (lane - 1))` plus the packed counts of earlier slices. The rank is a population count over the bitmask, not an atomic cursor. Because the rank follows the element index, **entries within one workgroup's contribution to a bin are in ascending element (paint) order**.
5. **coarse.wgsl (per bin, 256 threads).** Bin headers are read per partition of 256 draw objects. A prefix sum over partition counts gives a merged, ordered stream (`sh_drawobj_ix`) via binary search. Partitions are consumed in ascending order, so paint order is preserved across partitions too.
6. **Per-tile bitmask in coarse.** For each (draw object, tile) pair in the bin, `include_tile` is true if the tile has segments, or if the backdrop/clip state says the object contributes a solid tile. The thread sets `atomicOr(&sh_bitmaps[el_ix/32][y*16+x], 1 << (el_ix & 31))`. So the per-tile membership is a 256-bit bitmask over the bin's element window, stored in workgroup memory as 8 words per tile (32 bytes per tile per 256-element window).
7. **Ptcl write in paint order (coarse).** Each tile thread walks its bitmask with `firstTrailingBit(bitmap)` then `bitmap &= bitmap - 1`. This is an ascending bit walk, so **commands are emitted in paint order without a sort**. It writes `CMD_FILL` (with the segment range and backdrop) or `CMD_SOLID` (no segments: the tile is entirely inside the path), followed by paint commands (`CMD_COLOR`, gradients, images, blur, clip begin/end). Clip-zero state suppresses drawing when a `BEGIN_CLIP` tile is empty and has zero backdrop, until the matching `END_CLIP`.

### Allocation, overflow, and retry

- **Ptcl layout.** Each tile owns `PTCL_INITIAL_ALLOC = 64` u32s at `tile_ix * 64`. The first u32 holds the blend-spill offset. The list ends with `CMD_END`. When a tile needs more, `alloc_cmd` writes `CMD_JUMP` to a new 256-u32 increment from `atomicAdd(&bump.ptcl, PTCL_INCREMENT)` (`coarse.wgsl`, `alloc_cmd`).
- **Overflow flags.** Every allocating stage sets a bit in `bump.failed` with `atomicOr` (`STAGE_BINNING=1`, `STAGE_TILE_ALLOC=2`, `STAGE_FLATTEN=4`, `STAGE_PATH_COUNT=8`, `STAGE_COARSE=0x10`; `shared/bump.wgsl`). On binning overflow, `chunk_offset` is set to 0 and the stage flags failure, so writes are not guarded. Later stages check the flags and return early (`binning.wgsl`, `tile_alloc.wgsl`, `coarse.wgsl`). The ptcl overflow path sets `new_cmd = 0`, which the code comment calls "technical UB" accepted to avoid predicating writes.
- **Host side.** Each stage writes its final need into `BumpAllocators` (`config.rs` comment: "Final needed dynamic size of the buffers. If any of these are larger than the corresponding `_size` element reallocation needs to occur."). `render` returns the `BumpAllocators` value (`research/vello_research/src/render.rs`, `lib.rs` grep). **The caller's retry loop was not traced**: I did not confirm whether a failed frame is re-run after resizing.

### Solid tiles, backdrop, and early termination

- A tile with no segments and non-zero backdrop (after the even-odd rule) is a solid tile. `write_path` then emits only `CMD_SOLID` (no fill), and `fine` sets `area[i] = 1.0` for every pixel of the tile.
- The **fine stage does not terminate early**. The interpreter loop in `fine.wgsl` runs until `CMD_END`. A grep for `opaque`, `early`, and opacity tests in `coarse.wgsl` and `fine.wgsl` returned no opaque-cover logic. Compositing an opaque colour over everything below is still executed: `rgba[i] = rgba[i] * (1 - fg.a) + fg` for each command.
- Solid tiles are therefore **not** constant-colour detected: they are composited like any other paint.

### Clips

- `BEGIN_CLIP`: if the tile has no segments and clear backdrop, the code sets `clip_zero_depth` and suppresses the whole clipped subtree (zero-coverage shortcut). Otherwise it emits `CMD_BEGIN_CLIP` and increments the blend depth.
- `END_CLIP`: emits a fill of the clip path (`write_path`) plus `CMD_END_CLIP { blend, alpha }`.
- Blend stack: four levels in registers (`BLEND_STACK_SPLIT = 4`); deeper levels spill to a per-tile scratch buffer allocated by `atomicAdd(&bump.blend, ...)`.

### Quoted core (short excerpts)

`research/vello_shaders/shader/binning.wgsl`:

```wgsl
var<workgroup> sh_bitmaps: array<array<atomic<u32>, N_TILE>, N_SLICE>;
var<workgroup> sh_count: array<array<u32, N_TILE>, N_SUBSLICE>;
...
var idx = countOneBits(out_mask & (my_mask - 1u));
...
let offset = config.bin_data_start + sh_chunk_offset[sh_bin_ix];
bin_data[offset + idx] = element_ix;
```

`research/vello_shaders/shader/coarse.wgsl` (per-tile write, paint order):

```wgsl
var bitmap = atomicLoad(&sh_bitmaps[0u][local_id.x]);
...
let el_ix = slice_ix * 32u + firstTrailingBit(bitmap);
bitmap &= bitmap - 1;   // clear LSB
```

`research/vello_shaders/shader/shared/tile.wgsl` and `path_count.wgsl`: `Tile { backdrop: i32, segment_count_or_ix: u32 }`; the path count pass does `atomicAdd(&tile[base + x].segment_count_or_ix, 1u)` per crossing segment.

`research/vello_shaders/shader/shared/config.wgsl`: `TILE_WIDTH = 16u`, `TILE_HEIGHT = 16u`, `N_TILE_X = 16u`, `N_TILE_Y = 16u`, `N_TILE = 256u`.

| source (URL + file/line) | fact |
|---|---|
| research/vello_shaders/shader/shared/config.wgsl (`TILE_WIDTH`, `TILE_HEIGHT`, `N_TILE_X`, `N_TILE_Y`, `N_TILE`) | Tile 16x16 px; bin 16x16 tiles = 256 tiles, 256x256 px. |
| binning.wgsl (`WG_SIZE = 256`, `N_SLICE = 8`, `sh_bitmaps`) | One element per thread; per-bin 256-bit mask over the element window. |
| binning.wgsl (`countOneBits`, `idx = countOneBits(out_mask & (my_mask - 1u))`) | Output rank by popcount; no atomic cursor on bin_data. |
| binning.wgsl (`atomicAdd(&bump.binning, element_count)`, `binning_size` check) | One atomic per workgroup per bin block; overflow sets `STAGE_BINNING` and writes to offset 0. |
| coarse.wgsl (`sh_drawobj_ix`, binary search over partition counts) | Partitions consumed in order; paint order preserved across partitions. |
| coarse.wgsl (`include_tile = n_segs != 0u \|\| (backdrop_clear == is_clip) \|\| is_blend`) | Solid tile rule: no segments and non-zero backdrop contributes CMD_SOLID. |
| coarse.wgsl (`firstTrailingBit(bitmap)`, `bitmap &= bitmap - 1`) | Per-tile ascending bit walk emits ptcl commands in paint order. |
| coarse.wgsl (`alloc_cmd`, `PTCL_INITIAL_ALLOC`, `PTCL_INCREMENT`, `CMD_JUMP`) | Ptcl starts at 64 u32 per tile; overflow chains 256-u32 blocks from `bump.ptcl`. |
| coarse.wgsl (`clip_zero_depth`) | Empty clip tile suppresses its whole subtree until END_CLIP. |
| coarse.wgsl (`BLEND_STACK_SPLIT`, `bump.blend`) | Blend stack depth 4 in registers; deeper levels in a spill buffer. |
| fine.wgsl (`case CMD_SOLID`, `CMD_END` loop) | Solid sets area = 1; interpreter runs to CMD_END; no early exit (grep found no opaque logic). |
| tile_alloc.wgsl (`atomicAdd(&bump.tile, count)`, `sh_tile_offset`, comment on DX12/FXC hang, linebender/vello#1991) | Tile arena per workgroup; offset broadcast via workgroup memory. |
| path_count.wgsl (`atomicAdd(&tile[base + x].segment_count_or_ix, 1u)`, `atomicAdd(&bump.seg_counts, ...)`) | Segment counts by atomics per tile crossing. |
| shared/bump.wgsl (`STAGE_*` flags, `BumpAllocators`) | Failure bitmask and per-stage high-water marks in one struct. |
| research/vello_encoding/src/config.rs (lines 28-30 comment) | Host resizes buffers when a high-water mark exceeds its `_size`. |
| ARCHITECTURE.md ("Sparse Strips", "Compute-centric research renderer") | The compute renderer is research-tier; Sparse Strips (vello_cpu, vello_gpu) is the shipping path. |

### What Vello tells us about the target

- Vello's bin is a 256-element bitmask over paint order, and the per-tile list is a bit-walk over that mask. That is the exact shape the brief wants. Paint order is preserved by element index and by the rank function, not by a sort.
- Vello's per-tile cost is the same as the target's problem in a different form: it still scans the whole bin window (256 elements) per tile, one bit per element, and it has no per-tile last-opaque truncation.
- Overflow is handled as a monotonic bump allocator plus a failure bitmask, with a host-side resize and (not confirmed here) a retry. The fixed-size bitmask avoids overflow entirely, because its size is set by the element count, not the coverage.

---

## 2. Other 2D GPU renderers: data structure and the trick worth stealing

### Pathfinder (servo/pathfinder), d3d11 compute path

Per-tile data:
- `bin.cs.glsl` (local size 64) walks each microline through 16-pixel tiles with a DDA. Each step calls `addFill`, which does `fillIndex = atomicAdd(iIndirectDrawParams[1], 1)` and inserts into a per-tile linked list with `fillLink = atomicExchange(iTiles[tileIndex*4 + TILE_FIELD_FIRST_FILL_ID], fillIndex)`. Fill records are three u32s (`scaledLocalLine` at 256 sub-steps per pixel, packed x/y in 16-bit halves, plus the link).
- Backdrop deltas go to the tile's `BACKDROP_ALPHA_TILE_ID` word (upper 8 bits) by `atomicAdd`, or to a per-row backdrop array for tiles outside the path rect.
- `propagate.cs.glsl` walks each draw path's tile columns in order, carries the running backdrop (`currentBackdrop += drawBackdropDelta`), culls tiles outside a clip bounding rect or against a blank clip tile, allocates alpha-tile slots with `atomicAdd(iZBuffer[ALPHA_TILE_COUNT], 1)`, and writes the z-buffer with `atomicMax(iZBuffer[tileMapIndex + ...], drawTileIndex)` for **opaque solid tiles** (`zWrite && drawTileBackdrop != 0 && drawAlphaTileIndex < 0`). It stitches tiles into a per-framebuffer-tile linked list (`iFirstTileMap`) with `atomicExchange`.
- `tile.cs.glsl` walks that list per framebuffer tile (`while (tileIndex >= 0)`). It declares `uniform sampler2D uZBuffer` (line 39) but **does not read it in this file** (grep). The consumer of the z-buffer is **UNCONFIRMED**.

Trick worth stealing: **z-buffer-as-max-index occlusion** (`atomicMax` of the paint index of the top opaque solid tile per tile), written during the paint-order pass and read to drop everything below. It costs one u32 per tile and is order-independent (max). The Metal port (`resources/shaders/metal/d3d11/propagate.cs.metal`) confirms the same atomics exist as device-memory `atomic_fetch_max_explicit`, `atomic_fetch_add_explicit`, `atomic_exchange_explicit` on `device atomic_int/atomic_uint` (grep of the Metal file, lines 94-168). Pathfinder's README calls its tile-based occlusion culling a source of "dramatic performance wins over typical software renderers that use the painter's algorithm".

| source (URL + file/line) | fact |
|---|---|
| shaders/d3d11/bin.cs.glsl (`addFill`, `atomicExchange(iTiles[... FIRST_FILL_ID])`) | Fills are head-inserted into a per-tile linked list; index from `atomicAdd` on indirect params. |
| shaders/d3d11/bin.cs.glsl (`tileSize = ivec2(16)`, `vec4(256.0)`) | 16-px tiles; sub-tile coordinates at 1/256 px, packed into 16-bit halves. |
| shaders/d3d11/bin.cs.glsl (`adjustBackdrop`, `atomicAdd(iBackdrops[...])`) | Backdrop deltas by atomics, outside-rect deltas to a per-row array. |
| shaders/d3d11/propagate.cs.glsl (lines ~116-213, `zWrite`, `atomicMax(iZBuffer[...], drawTileIndex)`) | Opaque solid tiles write max draw index per framebuffer tile. |
| shaders/d3d11/propagate.cs.glsl (`atomicExchange(iFirstTileMap[...])`) | Per-framebuffer-tile list of draw tiles built by atomics. |
| shaders/d3d11/propagate.cs.glsl (blank clip tile cull, outside-clip cull) | Clip culling at tile granularity before any fill work. |
| shaders/d3d11/tile.cs.glsl (line 39 `uZBuffer`, line 77 `iFirstTileMap`, line 91 `while (tileIndex >= 0)`) | Tile kernel walks the list; z-buffer declared but not read in this file. |
| resources/shaders/metal/d3d11/bin.cs.metal (lines 94, 96, 127, 136) | Device atomics: `atomic_fetch_add_explicit`, `atomic_exchange_explicit` on `device atomic_uint`. |
| resources/shaders/metal/d3d11/propagate.cs.metal (lines 152, 164, 168) | `atomic_fetch_add_explicit`, `atomic_fetch_max_explicit`, `atomic_exchange_explicit` on `device atomic_int`. |
| README.md (occlusion culling paragraph) | Tile-based occlusion culling; cited as beating painter's-algorithm software renderers. |

### Forma (google/forma), CPU painter with damage-aware passes

The best primary source in this pass for the brief's "constant tile" and "last opaque cover" requirements.

- `layer_workbench/passes/tile_unchanged.rs`: if the clear colour is unchanged, the layer count is unchanged, and every layer id `is_unchanged(id)`, the pass returns `OptimizerTileWriteOp::None` (skip the tile entirely). A per-tile `cached_tile` holds the previous layer count.
- `layer_workbench/passes/skip_fully_covered_layers.rs`: iterates layers **topmost first** (`workbench.ids.iter_masked().rev()`). The first layer that `layer_is_full` (fully covers the tile) and is `Fill::Solid` with `BlendMode::Over` and `color.a == 1.0` becomes the **opaque cover**: `workbench.ids.skip_until(i)` drops every layer below it, then the loop breaks. A clipped or partial layer above it sets `Incomplete` and stops optimisation (`return ControlFlow::Continue(())`).
- If all visible layers are unchanged, the tile is skipped (`return ControlFlow::Break(OptimizerTileWriteOp::None)`). Otherwise, if every remaining layer is a solid fill, the pass folds them on the CPU with `try_fold(bottom_color, blend_mode.blend(dst, color))`, producing a **single colour** for the tile. Any non-solid layer makes the fold return `None`, falling back to painting.
- The clear colour is treated as a virtual opaque bottom layer (comment: "The clear color is used as a virtual first opqaue layer").
- Forma README: "We implemented a fail-fast per-tile optimizer that tries to skip the painting step entirely. A similar approach could also be tested on the GPU." It is marked "currently CPU-only".

Trick worth stealing: **top-down scan with stop at the first opaque full cover, then a constant-colour fold when everything left is solid**. Both steps are per-tile, need no GPU work, and are the exact shape of the early-out the brief asks for.

| source (URL + file/line) | fact |
|---|---|
| layer_workbench/passes/tile_unchanged.rs (`tile_unchanged_pass`, `cached_clear_color`, `is_unchanged`) | Whole tile skipped when clear colour, layer count and all layer props are unchanged. |
| layer_workbench/passes/skip_fully_covered_layers.rs (`iter_masked().rev()`, `layer_is_full`) | Top-down scan; the first full, solid, opaque (`a == 1.0`, `BlendMode::Over`) layer is the cover. |
| same file (`workbench.ids.skip_until(i)`, `break`) | Layers below the opaque cover are dropped from the work list. |
| same file (`try_fold(bottom_color, blend_mode.blend(dst, color))`) | If the remainder is all solid, the tile colour is folded on the CPU into one value. |
| same file (`InterestingCover::Incomplete`, `return ControlFlow::Continue(())`) | Any partial or clipped layer above disables the optimisation for the tile. |
| same file (comment "The clear color is used as a virtual first opqaue layer") | The clear colour acts as an opaque bottom layer. |
| README.md (section "Update only the tiles that change (currently CPU-only)") | Author states a GPU version "could also be tested". |

### WebRender (servo/webrender), occluder rectangles and tile occlusion

- `rectangle_occlusion.rs`: `FrontToBackBuilder` keeps `opaque_items` in front-to-back order. `add()` starts with the rect as one fragment and, for each opaque item that intersects, calls `apply_occluder`, which splits the fragment into up to four pieces (top, bottom, left, right) and removes the original. `test()` does the same without inserting. The fragment count is bounded by a `SmallVec<[_; 16]>`. Cost is O(opaque items x fragments), with no bitmasks.
- `composite.rs` (`Occluders::is_tile_occluded(z_id, world_rect)`): computes the non-overlapping area covered by occluders in front of the tile, using a sweep over `OcclusionEvent`s and an `active` list of y-ranges (scratch buffers `events`, `active`). A tile is occluded if the covered area equals the tile area. The comment: "It's often the case that a tile is only occluded by considering multiple picture caches in front of it".
- `picture.rs` (~lines 5233-5240): only fixed-position tiles (root spatial node) test occlusion. An occluded tile frees its native compositor surface and is marked not visible. Opaque regions of tile caches and opaque external surfaces are registered as occluders (`picture.rs` ~3894-3946).
- `tile_cache.rs`: `MAX_CACHE_SLICES = 12`; per-tile `local_dirty_rect` drives dirty-rect rasterisation; `TileDebugInfo::Occluded` exists for debug output.

Trick worth stealing: **occluder rectangles as a per-tile area test** (`covered_area == tile_area` means skip) rather than a per-pixel test. It works for the 32x32 tile case because a single opaque rect either covers the tile or does not. The sweep is the expensive piece; for a damage row with a few opaque rects it is cheap.

| source (URL + file/line) | fact |
|---|---|
| rectangle_occlusion.rs (`FrontToBackBuilder::add`, `test`, `opaque_items`) | Opaque items kept front-to-back; rects are split by occluders. |
| rectangle_occlusion.rs (`apply_occluder`: `top`, `bottom`, `left`, `right` pushes) | Rect minus occluder yields up to four pieces; `SmallVec<[_; 16]>`. |
| composite.rs (`Occluders::is_tile_occluded`, around line 1457) | Tile occluded when the covered area of occluders in front equals the tile area. |
| composite.rs (`OccludersScratchBuffers { events, active }`, around line 1413) | Area computed by a sweep over events with active y-ranges. |
| picture.rs (~5233-5240, `is_tile_occluded(tile.z_id, world_draw_rect)`) | Only fixed-position tiles test occlusion; occluded tiles free their native surface. |
| picture.rs (~3894-3946, `register_occluder`) | Opaque external surfaces and opaque tile-cache regions are registered as occluders. |
| tile_cache.rs (`MAX_CACHE_SLICES = 12`, `TileDebugInfo::Occluded`) | Slice cap of 12; occluded state is a debug category. |

### piet-gpu / Raph Levien's sort-middle posts

- `sort-middle` (2020-06-12): bins are "256x256 pixels in this implementation"; "the design calls for one bin per thread, so that would exceed the size of a workgroup". Each bin writes a contiguous segment per partition, "the elements within an output segment are kept sorted", and the coarse stage "merges the bin outputs, restoring the elements to sorted order". Binning is "not contributing significantly to the total render time". Binning all the way to tiles is raised as an idea and not recommended for workgroup-size reasons.
- `fast-2d-rendering` (2020-06-13): "A simple tile allocation kernel allocates and initializes a rectangular region of tiles for each path"; segments are inserted with "atomicExchange to insert segments into a linked list structure"; backdrop uses "a simple atomicAdd of +1 or -1". Measured: "even on Intel 630 the paper-1 example (dense vector text) runs in 7.6ms"; the Intel HD 630 is "about 5x slower than the GTX 1060". Canvas 2048x1536.
- Tile size is not stated in that post.

Trick worth stealing: **bins of 256x256 pixels (16x16 tiles) so that bin count stays small, and per-bin contiguous output segments that the next stage merges in order**. Maps directly onto the brief's "per-tile-row list" becoming a "per-bin tile bitmask": the bin is coarse, the per-tile list is fine.

| source (URL) | fact |
|---|---|
| raphlinus.github.io/rust/graphics/gpu/2020/06/12/sort-middle.html | 256x256 px bins; one bin per thread would exceed a workgroup; segments kept sorted; coarse merges; binning not significant in total time. |
| raphlinus.github.io/rust/graphics/gpu/2020/06/13/fast-2d-rendering.html | Tile allocation per path; `atomicExchange` linked lists for segments; backdrop by `atomicAdd`. |
| same post (performance quotes) | Dense vector text 7.6 ms on Intel HD 630; about 5x slower than GTX 1060; canvas 2048x1536. |
| raphlinus.github.io (index, post list) | Five related posts found, including "Requiem for piet-gpu-hal" (2023) and "piet-gpu clipping" (2022); clipping post not read. |

### Skia Graphite, Impeller

Not opened in this pass. No claim made.

---

## 3. Parallel binning in general: count/scan/scatter, bitmasks, z-binning

### Mechanism

Two families:

1. **Lists** (count, exclusive scan, scatter). One atomic or scan per (object, tile). Output is compact but order depends on the scatter method. Paint order needs either a stable scatter (rank by index, Vello's popcount rank) or a sort per tile.
2. **Bitmasks** (one bit per object per tile). Output size is fixed: `tiles x objects / 8` bytes. Membership is a bit set, order is the bit index (so paint order is free if objects are indexed in paint order). Cost of the tile pass is proportional to the number of words scanned, not to the number of hits. Early-out is a count-leading-zeros or a ctz.

### Drobot, "Improved Culling for Tiled and Clustered Rendering" (Call of Duty: Infinite Warfare, SIGGRAPH 2017)

Read from the slide XML (`2017_Sig_Improved_Culling_final.pptx`, slides 1-82; the deck text is in the source, slide numbers below).

- **Structure.** Slide 33: "Structure (256 bit array)". Per tile, a 256-bit array of lights (8 words). Slide 15: "Flat Bit Array. Collection of bits: Nth bit representing visibility of Nth entity from global list. Simple traversal: iterate through bits." Slide 16-17 code: `mask = entityMasksTile[address + wordIndex]; while (mask != 0) { bitIndex = firstbitlow(mask); entityIndex = 32*wordIndex + bitIndex; mask ^= 1 << bitIndex; ... }`.
- **Z-binning** (slides 20-33). The CPU sorts lights by Z and builds, per Z bin, a 2 x 16-bit LUT of the minimum and maximum light index overlapping that bin (slide 29-30: `ZBIN[int(LinearZ / BIN_WIDTH)] = MIN_LIGHT_ID_IN_BIN | MAX_LIGHT_ID_IN_BIN`). The GPU masks the tile's words to `[min, max]` (slide 32, `clamp(...)` and `BitFieldMask`) before the scan. Slide 33 gives cull operations per entity: Tiled 32,400 (240x135 tiles, 1 pass); Tiled+ZBin 8,096 (simple variant, 240x135 with 8,096 cull ops, 32 KB extra); Clustered 60x32x18 with 34,560 cull ops, 1,106 KB.
- **Memory** (slide 33): Tiled buffer 1,036 KB at 240x135 with a 256-bit array; Tiled+ZBin adds 32 KB; Clustered 1,106 KB.
- **Measured** (PS4, 1080p):
  - Slide 39, Hangar Fire: Tiled 9.00 ms opaque, Tiled+ZBin 7.65 ms (15%).
  - Slide 40, Zombies opening: Base tile 5.7 ms (100%), Tile+ZBin 5.2 ms (91%), Scalarised tile 5.1 ms (88%), Scalarised tile+ZBin 4.6 ms (80%).
  - Slide 18 (synthetic): base F+ 56 VGPR, occupancy 5; flat F+ scalarised 32 VGPR, occupancy 8.
  - Slide 53 (tile rasteriser of 3 full-screen lights, 240x135): atomic raster 1.44 ms; with 4xMSAA 0.27 ms; with compaction 0.36 ms; both 0.10 ms.
- Slide 21 trade-off: "Z-binning ... Tiles (+) vs Clusters (-)" for depth discontinuity and memory scaling. Slide 50: "Only need 4 operations to tag all tiles" after pruning atomics.
- **Scalarisation** (slides 9-19): iterate the bit words per wave, using `WaveReadFirstLane(WaveAllBitOr(mask))` to merge words, so that the loop is uniform. Hierarchical containers (pointer per tile) "can be scalarized on container level"; flat bitmasks "can be scalarized on stored entity level" (slide 19).

Cost model for the brief: a 256-bit tile array is 32 bytes per tile. A tile scan over the whole row list is `P/32` words; with 230 entries that is 8 words, versus 230 entries for the current design. Drobot's Z-bin is the same idea applied to the bit range: store the first and last set index per tile and scan only those words.

| source (Drobot deck slide) | fact |
|---|---|
| slide 15 ("Flat Bit Array") | Nth bit = visibility of Nth entity in the global list; traversal iterates bits. |
| slides 16-17 (code) | `firstbitlow(mask)` loop; `entityIndex = 32*wordIndex + bitIndex`; `mask ^= 1 << bitIndex`. |
| slide 21 | Z-binning trade-off: tiles keep spatial resolution; clusters suffer memory and depth-range costs. |
| slides 29-30 | CPU sorts lights by Z; per Z-bin LUT of MIN and MAX light index as 2 x 16-bit values. |
| slide 32 | GPU clamps each word's bit range to [min, max] before using the mask. |
| slide 33 | "Structure (256 bit array)"; Tiled 1,036 KB at 240x135; Tiled+ZBin +32 KB; Clustered 1,106 KB at 60x32x18; cull ops per entity 32,400 / 8,096 (simple) / 34,560. |
| slide 39 (Hangar Fire, PS4 1080p) | Tiled 9.00 ms; Tiled+ZBin 7.65 ms (15%). |
| slide 40 (Zombies opening, PS4 1080p) | Base tile 5.7 ms; +ZBin 5.2 ms; scalarised 5.1 ms; scalarised+ZBin 4.6 ms. |
| slide 18 (synthetic) | Base F+ 56 VGPR, occupancy 5; flat scalarised 32 VGPR, occupancy 8. |
| slide 50 | Pruned atomics: "Only need 4 operations to tag all tiles". |
| slide 53 | Tile rasteriser, 3 full-screen lights: atomic 1.44 ms, +4xMSAA 0.27 ms, +compaction 0.36 ms, both 0.10 ms. |
| slides 10-19 | Scalarisation: `WaveReadFirstLane(WaveAllBitOr(mask))` merges words; flat bit arrays scalarise at entity level. |

### Olsson, Billeter, Assarsson (2012) "Clustered Deferred and Forward Shading"

**UNCONFIRMED body.** The preprint PDF is compressed; no text was extracted. Olsson and Persson's "Practical Clustered Shading" (not read) is the main reference for the production bit-list design. The count/prefix-sum/scatter build is the widely cited pattern (general knowledge, not checked against this paper). Do not cite its numbers.

| source | fact |
|---|---|
| chalmers.se clustered_shading_preprint.pdf | Body not extracted (compressed PDF). **UNCONFIRMED**. |
| Pathfinder Metal port (grep) | Device atomics compile: fetch_add, exchange, fetch_max. |
| Apple forum thread (search snippet) | Chained scan with `simd_prefix_exclusive_sum` and a 32-entry `threadgroup uint` partials array; Apple suggests a traditional scan. |
| Dawn commit e7de00bb (search snippet) | `subgroupExclusiveAdd` maps to `simd_prefix_exclusive_sum`. |
| Metal Shading Language Specification v4 (search, TOC only) | Spec exists; limits and atomics sections not read. |

### Metal specifics for the bin/tile kernels

- **Device-memory atomics in compute kernels: confirmed in use.** Pathfinder's Metal port uses `atomic_fetch_add_explicit`, `atomic_exchange_explicit`, `atomic_fetch_max_explicit` on `device atomic_uint` / `atomic_int` in `bin.cs.metal` (lines 94-136) and `propagate.cs.metal` (lines 152-168). This shows the three atomics that a bin pass needs (count, linked-list head, max-index) compile for Metal device memory.
- **Atomic OR on u32 (bitmask set): not directly confirmed in Metal source.** `atomic_fetch_or_explicit` is the natural MSL counterpart (Vello's WGSL `atomicOr` on workgroup memory uses it); **UNCONFIRMED** against the MSL spec in this pass.
- **Subgroup prefix sum:** `simd_prefix_exclusive_sum` is the MSL call that Dawn maps WebGPU's `subgroupExclusiveAdd` onto (Dawn commit `e7de00bb`, search snippet, not read). An Apple developer-forum thread shows a chained scan that stores per-group partials in a `threadgroup uint` array of 32 entries (search snippet). Apple's accepted answer on that thread suggests a traditional prefix sum instead. Subgroup width on Apple silicon: **UNCONFIRMED** here.
- **Threadgroup memory limit:** **UNCONFIRMED.** The "32 KB" figure in the brief was not found in any source read. Check the Metal feature-set tables before sizing workgroup memory for the bin kernel.
- **Vello's 256-thread workgroup with 8 KB+ of workgroup bitmask** (`sh_bitmaps` = 8 x 256 x 4 = 8 KB, plus `sh_count` 4 x 256 x 4 = 4 KB, plus smaller arrays) is evidence that a Metal threadgroup of this size is plausible in WGSL; its Metal translation was not checked.

---

## 4. SIMD and threads for a retained UI tree on the CPU

### Servo / Stylo (style, Rust)

- `style/parallel.rs`: work is distributed to a rayon `ScopeFifo` in chunks of `work_unit_max` taken from a `VecDeque` (`distribute_work`, lines ~116-140). Discovered children are pushed back into a local queue when `discovered_children >= work_unit_max && discovered.len() >= local_queue_size + work_unit_max` (lines ~183-191). Thread stack size is 256 KB with a 168 KB safety margin (`STYLE_THREAD_STACK_SIZE_KB`, `STACK_SAFETY_MARGIN_KB`). **The value of `work_unit_max` at the call site was not located.**
- `style/bloom.rs`: a **counting bloom filter per worker thread**, stored in a leaked thread-local static (`BLOOM_KEY`). It holds the hashes of ancestors (local name, namespace, id, classes; `each_relevant_element_hash`). `push` and `pop` track a `pushed_hashes` SmallVec (inline capacity 64). `MEMSET_CLEAR_THRESHOLD = 25`: clearing removes hashes one by one when fewer than 25 were pushed, and memsets otherwise. `insert_parents_recovering` handles work stealing (a worker may need to push several ancestors).
- `style/sharing/mod.rs`: `SHARING_CACHE_SIZE = 32` LRU entries per cache; `SHARING_MAX_LEVELS = 8` caches, one per depth; `REASONABLE_CLASS_LIST_SIZE = 5`. The caches live in a thread-local array.
- The traversal is parallel and breadth-first per the bloom filter comment ("Servo does a parallel breadth-first traversal instead of a sequential depth-first traversal").

What to steal: **thread-local ancestor filter with push/pop and an identity-based sharing cache for siblings with equal style inputs**. For a retained UI, the sharing cache is the analogue of "same paint state as the previous sibling": reuse the computed paint record instead of recomputing it.

| source (URL + file/line) | fact |
|---|---|
| style/parallel.rs (`distribute_work`, `work_unit_max` chunks from `VecDeque`) | Chunked rayon `ScopeFifo` distribution. |
| style/parallel.rs (lines ~183-191) | Children spill to a local queue only when both thresholds are met (work-unit based). |
| style/parallel.rs (`STYLE_THREAD_STACK_SIZE_KB = 256`, `STACK_SAFETY_MARGIN_KB = 168`) | Fixed worker stack budget. |
| style/bloom.rs (`BLOOM_KEY`, thread-local leaked static) | Counting bloom filter per worker thread. |
| style/bloom.rs (`each_relevant_element_hash`) | Hashes: local name, namespace, id, classes. |
| style/bloom.rs (`MEMSET_CLEAR_THRESHOLD = 25`) | Remove hashes one by one below 25 pushes; memset above. |
| style/bloom.rs (`insert_parents_recovering`) | Recovers ancestor state when work stealing skips levels. |
| style/sharing/mod.rs (`SHARING_CACHE_SIZE = 32`, `SHARING_MAX_LEVELS = 8`, `REASONABLE_CLASS_LIST_SIZE = 5`) | LRU sharing cache per depth level, thread-local. |

### Bevy (visibility and extraction)

- `crates/bevy_camera/src/visibility/mod.rs`, `check_visibility_cpu_culling`: `visible_aabb_query.par_iter_mut().for_each_init(|| thread_queues.borrow_local_mut(), |queue, item| {...})`. Per-thread queues are `Local<Parallel<TypeIdHashMap<Vec<Entity>>>>`, merged later. The per-entity early outs, in order: `inherited_visibility` false, view mask mismatch, visibility range out, then a sphere-based frustum test before the AABB test (`Sphere { center, radius }` from `transform.affine()`).
- Change detection filters `Changed<InheritedVisibility>`, `Changed<GlobalTransform>`, `Changed<Projection>` and `Added<NoCpuCulling>` (lines ~630-912). The extraction query for CPU-culled entities uses `Or<(Changed<NoCpuCulling>, Changed<InheritedVisibility>)>` (`bevy_render/src/view/visibility/mod.rs`).
- Batch size of `par_iter_mut`: **UNCONFIRMED** (not located in the read files).

What to steal: **thread-local output queues via `for_each_init`, merged after the parallel pass**, and **changed-only queries**. Both avoid a shared atomic cursor for the output.

| source (URL + line) | fact |
|---|---|
| bevy_camera/src/visibility/mod.rs (`check_visibility_cpu_culling`, `par_iter_mut().for_each_init`) | Thread-local queue per worker via `borrow_local_mut`. |
| same file (`thread_queues: Local<Parallel<TypeIdHashMap<Vec<Entity>>>>`) | Output lists per thread, merged after the parallel pass. |
| same file (`inherited_visibility` / view mask / visibility range early outs, then sphere test) | Cheapest rejects first, AABB last. |
| same file (`Changed<GlobalTransform>`, `Changed<Projection>`, `Added<NoCpuCulling>`, lines ~630-912) | Change-detected queries limit work to changed entities. |
| bevy_render/src/view/visibility/mod.rs (`Or<(Changed<NoCpuCulling>, Changed<InheritedVisibility>)>`) | Extraction query is change-only. |

### Unity DOTS / Burst (chunk size)

- Search snippet of the Unity Entities 1.x manual: "Each chunk consists of 16 KiB and the number of entities that they can store depends on the number and size of the components in the chunk's archetype." Also: "cache misses happen every time a system has to jump from one chunk to another to process the next entity." **The reason for 16 KiB (page size, cache size, or something else) is not stated in what was read. UNCONFIRMED.** A forum thread reports a community attempt to change it; it has no official status.
- A 6.5-era snippet adds a capacity of at most 128 entities per chunk. Not confirmed in full.
- Burst compilation and SoA layout details were not found in this pass.

| source (search snippet of Unity Entities docs, not read in full) | fact |
|---|---|
| docs.unity3d.com Entities manual, "concepts archetypes" | Each chunk is 16 KiB; entities per chunk depend on archetype component sizes. |
| same family of docs, "performance chunk allocations" (search snippet) | Capacity capped at 128 entities per chunk (6.5-era page). |
| Unity "Minimizing cache misses" tutorial (search snippet) | Cache miss each time processing jumps to a new chunk. |
| forum thread (search snippet) | Community attempt to change the 16 KiB size; no official status. |

### simdjson (two-stage classification)

- Abstract only (arXiv 1902.08318, Langdale and Lemire, VLDB Journal 2019): "process gigabytes of data per second on a single core", "a quarter or fewer instructions than a state-of-the-art reference parser like RapidJSON", "extensive use of SIMD instructions".
- Stage 1 (structural indexing with SIMD classification into bitmasks, then prefix-XOR for quotes) and stage 2 (parse using the index) are **UNCONFIRMED** from a read of the paper body in this pass. The paper body was not fetched.
- What transfers: a 64-byte block is classified into bitmasks (one bit per byte per class), and the consumer walks set bits with ctz. That is the same bitmask-walk as Drobot's flat bit array and Vello's `firstTrailingBit` loop. The grain is a 64-byte block (the source of the 64-bit mask width; **UNCONFIRMED** in the paper body).

| source (URL) | fact |
|---|---|
| arxiv.org/abs/1902.08318 (abstract) | "process gigabytes of data per second on a single core"; "a quarter or fewer instructions than ... RapidJSON"; SIMD throughout. |
| same | Stage design, instruction set, and bytes per cycle: not in the abstract; **UNCONFIRMED**. |

### Roaring bitmaps (2D occupancy)

- The format spec (RoaringFormatSpec README, read): 32-bit keys split into 16-bit high key and 16-bit low container. Array container for cardinality up to and including 4096; bitset container for cardinality above 4096, stored in exactly 8192 bytes as 64-bit words; run containers for runs. Bit `j` of the container is in word `j/64`, bit `j%64`.
- Abstract (arXiv 1603.06549, Lemire, Ssi-Yan-Kai, Kaser): "can be several times faster (up to two orders of magnitude)" than RLE-based bitmaps (WAH, Concise, EWAH). This is compared against compressed bitmaps, not against an uncompressed bitmask scan.
- Transfer to the brief: a dense row bitmask is best stored as an uncompressed bitset (Drobot's 256-bit array; Vello's 256-element bins). The Roaring 4096 threshold is a density rule: below 4096 of 65536 set bits an array is smaller than a bitset. For per-tile masks over at most a few thousand primitives, the bitset is the right container; an array container only helps when tiles are very sparse.

| source (URL) | fact |
|---|---|
| github.com/RoaringBitmap/RoaringFormatSpec README (lines ~54, 102-103, 119) | Keys: 16-bit high; containers of 16-bit low values; array container up to cardinality 4096; bitset container exactly 8192 bytes of 64-bit words. |
| arxiv.org/abs/1603.06549 (abstract) | "several times faster (up to two orders of magnitude)" than RLE-compressed bitmaps (WAH, Concise, EWAH). |

### Frostbite "Culling the Battlefield" (Daniel Collin, GDC 2011)

Not read as a document. The slide deck was located (slideshare and ea.com pages). A gamedev.net article (secondary) describes SSE sphere and AABB frustum tests computed on four objects at once, and a forum thread notes the slides cover only the culling pass producing an index array. The "4 objects per SSE register" layout and the "no hierarchy beats a tree" claim from the brief are **UNCONFIRMED** against the slides.

---

| source (URL) | fact |
|---|---|
| slideshare.net/DICEStudio/culling-the-battlefield-data-oriented-design-in-practice (found via search, not read) | Deck exists; search snippet of notes: worlds up to about 15,000 objects; first parallel brute-force attempt about 3x faster than old culling. |
| gamedev.net forum topic 4833665 (search snippet, secondary) | SSE sphere and AABB tests on four objects at once; thread says the slides cover only the index-producing pass. |

## Summary table (per topic: one-line verdict)

| topic | confirmed source | key number or fact | still unconfirmed |
|---|---|---|---|
| Vello bin/tile/ptcl | binning.wgsl, coarse.wgsl, shared/config.wgsl | 16x16 px tiles, 256-element bin windows, popcount rank, ascending bit walk | fine early-out (none), host retry loop |
| Pathfinder | bin/propagate .cs.glsl, Metal port grep | atomicMax z-buffer for opaque tiles, linked lists by atomicExchange | consumer of z-buffer in tile kernel |
| Forma | skip_fully_covered_layers.rs, tile_unchanged.rs | top-down stop at opaque full cover; CPU colour fold for solid-only tiles | GPU port (not in repo) |
| WebRender | rectangle_occlusion.rs, composite.rs, picture.rs | rect subtraction for opaque items; area-equals-tile occlusion test | sweep body detail |
| Drobot 2017 | 2017_Sig_Improved_Culling_final.pptx slides 15-53 | 256-bit tile arrays, Z-bin LUT 2x16 bit, 9.00 -> 7.65 ms, 5.7 -> 4.6 ms | Olsson 2012 body |
| sort-middle (Levien) | sort-middle, fast-2d-rendering posts | 256x256 px bins; sorted segments merged in order | tile size of piet-gpu |
| Stylo | parallel.rs, bloom.rs, sharing/mod.rs | per-thread counting bloom, sharing cache 32 entries, 8 levels | work_unit_max value, measured speedup |
| Bevy | camera visibility mod.rs | for_each_init with per-thread queues, change-detection filters | par_iter batch size |
| Unity | search snippet only | 16 KiB chunks, chunk-jump cache misses | reason for 16 KiB |
| simdjson | arXiv abstract | "gigabytes per second", "a quarter or fewer instructions" | stage 1/2 details |
| Roaring | RoaringFormatSpec README | array <= 4096, bitset = 8192 bytes | intersection speed on a bitmask scan |

---

## What to build: per-tile-row bitmask bin with last-opaque early-out

A concrete proposal grounded in Vello's bin/coarse split (paint index = bit index, popcount rank, ascending bit walk), Pathfinder's atomicMax occlusion and Drobot's Z-binning (min/max word range per tile), and Forma's top-down opaque stop with colour fold. Sizes assume a 1920x1080 target with 32x32 tiles (60x34 = 2040 tiles) and P primitives per frame in paint order.

1. **Index space.** Primitive `p` has paint index `p` (0..P-1, P capped at `P_MAX` = 4096 per frame). Tile `t` is one of the damaged tiles, mapped by a compact index `damaged[t]`, so only damaged tiles get storage.
2. **Per-tile data (device buffer, SoA):** `mask[t][w]` as `u32` words, `w = 0..ceil(P/32)-1`, 4 bytes each: for P=1024, 128 bytes per tile. `opaque_full[t][w]`, same size, bit set when prim `p` is an opaque solid fill that covers all of tile `t` (rect test in the bin). `last_opaque[t]` as `u32` (`atomic_fetch_max` of the paint index, 0 = none; 4 bytes per tile). `wmin[t]`, `wmax[t]` as `u16` each (first and last nonzero word, Drobot's Z-bin range idea; 4 bytes per tile).
3. **Bin kernel.** One thread per primitive per workgroup of 256 primitives (as Vello). Compute the primitive's tile rect in damaged-tile space (clipped). For each tile in the rect, `atomic_fetch_or(&mask[t][p/32], 1 << (p%32))`. If the primitive is an opaque solid rect and covers the tile, also set `opaque_full` and `atomic_fetch_max(&last_opaque[t], p+1)`. Update `wmin`/`wmax` with `atomic_fetch_min/max`. Output is bit-set, so no ordering is needed: paint order is the bit index.
4. **Large-rect rule.** An opaque rect covering many tiles would set O(area) bits. Instead, for opaque full covers, skip the bit set for tiles where it is not needed: the tile kernel only needs bits above `last_opaque`. Implement as a row-band `atomic_fetch_max` over a per-row-band array, with the bin pass for opaque rects only touching band boundaries (**design choice, not measured**).
5. **Tile kernel (one threadgroup per damaged tile, 256 threads = 16x16 pixel quads).** Load `last_opaque[t]` and `wmin/wmax`. Start the bit range at `max(wmin, word_of(last_opaque-1))`. The bottom of the composited stack is `last_opaque`; everything below it is dropped. Walk words `w` from `wmin` to `wmax`: mask out bits below `last_opaque` in the first word, then for each set bit use `ctz` to get `p` in ascending order and composite. A prim with `opaque_full` set in its tile needs no coverage test, only the colour.
6. **Constant-colour check.** While walking, keep a `solid_only` flag: it is cleared if any remaining prim is not a `solid_full` (opaque fill covering the tile) or is a partial prim. If `solid_only` survives the walk, fold colours in order (the Forma fold) and write one colour to the tile without per-pixel work. This is the cheap detector the brief asks for. Cost: one `and` per word, no extra memory.
7. **Early-out at last opaque cover.** Top-down form: if `last_opaque[t]` is set and no partial prim is above it, the tile needs only the colour of `last_opaque` plus the prims above it. Bits below `last_opaque` are never read, so their words are never loaded (saves `last_opaque / 32` word reads per tile).
8. **Reads per tile.** Current design: 230 entries scanned. Proposal: `(wmax - wmin + 1)` words (8 for 230 entries spread over 256 indices, fewer with the range cut), plus a `ctz` per set bit. Estimate of word reads only: about 8 words versus 230 entries. **Not measured.**
9. **Overflow rules.** The mask has a fixed size set by P, so the bin cannot overflow a per-tile list. The only capacity limit is `P_MAX`. If the frame has more than `P_MAX` primitives: split into blocks of `P_MAX` in paint order, run the bin and tile kernels per block in order, and carry the tile colour between blocks in a per-tile RGBA8 buffer (blocks are paint-ordered, so this is exact). Set a `bump.failed`-style flag per block only if memory for the block's masks cannot be allocated; then fall back to a full-row repaint of the damaged band (the current behaviour), never a partial result.
10. **Gates and tests (AUDIT rule: pixel-exact).** Oracle: the tile kernel output equals a full scan of the row list for every damaged tile, for random scenes with overlapping opaque rects, partial clips, and glyph runs. Mutant that must fail: skip the `last_opaque` mask (paint below an opaque cover). Mutant that must fail: drop the `wmin/wmax` range (must still give the same pixels). Measure the bin and tile kernel separately on glyph-dense rows against the current design, with the thresholds fixed beforehand.
11. **Memory budget (1920x1080, 2040 tiles, P=1024, 32 rows damaged = ~400 tiles):** mask 128 B x 400 = 51 KB; opaque_full 51 KB; last_opaque + wmin/wmax 1.6 KB. Total about 104 KB, double-buffered per frame for pacing. Compare Drobot's 1,036 KB for 240x135 tiles with a 256-light array (slide 33); the proposal is smaller because it stores only damaged tiles.
12. **Why this shape.** The bitmask removes the need for paint-order sorting (Vello's rank, Drobot's bit index), the max-index occlusion is the Pathfinder z-buffer pattern, the colour fold is the Forma pass, and the word range is Drobot's Z-bin LUT applied to the bit array. None of the four ideas has a measured win for this renderer; the measurement is the next step.
