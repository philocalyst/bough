# R6: What game engines and GPU vector renderers do for binning, reuse, occlusion, LOD, scheduling, pacing, memory

Scope: mechanisms only, mapped to a retained Rust GUI renderer on Apple silicon (Metal, TBDR). Target problem: the bin pass (per damaged-tile-row primitive lists) and tile pass (shade each 32x32 tile by scanning its row list) lose GPU time on glyph-dense scenes; CPU budget 100-300 K instructions/frame.

## Source status and limits (read first)

- Confirmed by fetching the page or document: the SIGGRAPH 2021 Advances index (Nanite talk listed, slides PDF named), the SIGGRAPH 2015 Advances index (Haar/Aaltonen talk, "hundreds of thousands of independent objects at 60 FPS", "a handful of indirect draw calls"), the Vello repo landing page and ARCHITECTURE.md summary, the GDC Vault page for Gyrling's fiber talk, the Vulkan `VK_EXT_present_timing` reference, and the piet-gpu stack-monoid post.
- Not confirmed: the Nanite slide body (only the title and abstract were read; PDF was not parsed), the Naughty Dog slide PDF (S3 returned 403), the Olsson clustered-shading paper (the PDF fetch returned binary that was not parsed), the Apple `CAMetalDisplayLink` reference (fetched page had only a title). Claims resting on these are marked **unconfirmed** and reflect general knowledge of the material, not a read of the source.
- Secondary sources used for a few numbers (cluster size, one-pixel threshold, VSM behaviour): the UIUC CS418 Nanite notes, a GCores article on the talk, Epic forum threads and an Epic tech blog snippet. These are labelled as secondary.
- Vello's per-tile data structure (pathtag stream, coarse segment lists, ptcl) and its overflow behaviour were **not** confirmed from the fetched ARCHITECTURE.md summary; the summary points at `research/doc/pathseg.md` and the "Potato" design doc, which were not read.
- Metal `presentAtTime` / `MTLDrawable.present(at:)` signatures: unconfirmed.

Primary URLs visited:
- https://advances.realtimerendering.com/s2021/index.html
- https://advances.realtimerendering.com/s2021/Karis_Nanite_SIGGRAPH_Advances_2021_final.pdf
- https://www.advances.realtimerendering.com/s2015/index.html
- https://github.com/linebender/vello
- https://github.com/linebender/vello/blob/main/ARCHITECTURE.md
- https://www.gdcvault.com/play/1022186/Parallelizing-the-Naughty-Dog-Engine
- https://docs.vulkan.org/refpages/latest/refpages/source/VK_EXT_present_timing.html
- https://developer.apple.com/documentation/quartzcore/cametaldisplaylink
- https://raphlinus.github.io/gpu/2020/09/05/stack-monoid.html

---

## 1. GPU-driven rendering and binning

Mechanisms that build per-tile or per-cluster work lists on the GPU. Most relevant to the bin pass.

| mechanism | source URL | what it does | mapping to 2D tile-damage UI renderer |
|---|---|---|---|
| Haar/Aaltonen GPU-driven pipeline (Ubisoft, SIGGRAPH 2015) | https://www.advances.realtimerendering.com/s2015/index.html (slides: `aaltonenhaar_siggraph2015_combined_final_footer_220dpi.pdf` in the same directory) | Object data lives in GPU buffers; culling and draw-argument generation run in compute; CPU issues "a handful of indirect draw calls" for hundreds of thousands of objects at 60 FPS (stated on the page). Per-material instance batching and mesh clustering (AC Unity). | Replace per-primitive CPU emission with compute that writes indirect dispatch args for the tile pass. The bin pass becomes a compute kernel that reads a persistent primitive buffer and writes a count-sized indirect command. |
| Nanite visibility buffer + cluster culling (Karis, Stubbe, Wihlidal, SIGGRAPH 2021) | https://advances.realtimerendering.com/s2021/index.html ; slides https://advances.realtimerendering.com/s2021/Karis_Nanite_SIGGRAPH_Advances_2021_final.pdf | Rasterises cluster/triangle IDs into a visibility buffer, then shades in a separate full-screen pass reconstructing material per pixel. Culling is per cluster, not per mesh (secondary: CS418 notes, https://cs418.cs.illinois.edu/website/text/nanite.html). Software rasteriser for small triangles (**unconfirmed** details from the slide body). | Visibility-buffer idea transfers as "record which primitive id covers this pixel, shade later". For glyph-dense text this suggests one coverage/ID pass over damaged tiles followed by a single shade pass that looks up glyph/material by id, instead of re-walking a list per tile. |
| Nanite cluster hierarchy and cluster-level culling | https://cs418.cs.illinois.edu/website/text/nanite.html (secondary) | Triangles grouped into clusters of about 100 triangles (secondary; "128" was not found in any source). Cluster bounds are tested before rasterisation. | Group glyphs into fixed-size runs (e.g. 64-128 glyphs) with bounds; cull runs before entering a tile list. Run granularity, not glyph granularity, is what the bin pass should index. |
| Two-pass HZB occlusion (Nanite and Frostbite/Ubisoft pipelines) | https://advances.realtimerendering.com/s2021/index.html (Nanite); https://www.advances.realtimerendering.com/s2015/index.html (Haar/Aaltonen) | Pass 1 draws last-frame visible set against last-frame HZB; pass 2 retests the rejected set against the HZB rebuilt from pass 1 and draws survivors. | Two-pass occlusion maps to: draw last frame's visible tiles first, build a per-tile "opaque cover" depth/mask, then retest the rest. Cheap in 2D if cover is an opaque rect bitmask rather than depth. |
| Clustered / tiled light binning (Olsson, Billeter, Assarsson, HPG 2012) | https://www.cse.chalmers.se/~uffe/clustered_shading_preprint.pdf (PDF fetched, text not extracted) | View frustum split into a 3D cluster grid; each light is tested against cluster AABBs/spheres; output is per-cluster light index lists built with count, prefix sum, scatter. | **Unconfirmed in detail.** The count / prefix-sum / scatter list build is the standard pattern: pass 1 counts entries per bin, a scan gives offsets, pass 2 writes indices. Directly applicable to building per-tile primitive lists from a damage rect set. |
| Tiled light culling in compute (Stratos/Johan Andersson, Dice tiled deferred) | https://www.advances.realtimerendering.com/s2015/index.html (Frostbite talks were in the same session) | Per-tile light list in LDS/global memory, built by one workgroup per tile. | One workgroup per tile reading all primitives that intersect it, with a fixed on-chip list cap. Avoids a separate scatter pass but needs a bound. **Unconfirmed**: exact Frostbite paper (not opened). |
| Vello coarse rasteriser: paths -> sparse strips -> tile binning -> per-tile command lists (ptcl) | https://github.com/linebender/vello ; https://github.com/linebender/vello/blob/main/ARCHITECTURE.md | Paths are flattened, binned into tiles, and represented as sparse horizontal strips (quoted from ARCHITECTURE.md summary). Compute renderer encodes the scene into path, draw, transform and resource buffers, and uses prefix-scan algorithms to parallelise allocation. Per-tile ptcl command lists are the output consumed by the fine stage. **Unconfirmed**: per-tile list layout, scan/scatter specifics, overflow handling (see pathseg doc, not read). | Closest existing analogue to the bin pass. The useful part is that work is expressed as a scanned, scattered command list per tile, consumed by a fine pass that does one tile at a time. |
| piet-gpu stack monoid and decoupled look-back (Levien) | https://raphlinus.github.io/gpu/2020/09/05/stack-monoid.html | Scan over a "pops then pushes" monoid computes parent/match relations in parallel. Decoupled look-back: each partition publishes a local aggregate and reads predecessors' aggregates, so one pass runs without a global barrier. Overflow: fixed window plus linked list, overflow allocated by atomic bump into global memory (quoted). Author's estimate of speedup is analytical only. | Use decoupled look-back for the per-tile offset scan when the bin pass is one dispatch: avoids a second dispatch for the scan. Use bump allocation with a fixed window for overflow of a tile's list. |
| Pathfinder (Pathfinder 3, Patrick Walton) | **Unconfirmed**: not fetched in this run. | Tile-based GPU vector renderer; the fill pass consumes tile-local winding and coverage records. | Background only; cite before use. |
| Forma / Rive / Skia Graphite / Impeller | **Unconfirmed**: not fetched in this run. Impeller docs: https://docs.flutter.dev/perf/impeller (not opened). Graphite design docs: https://skia.org/docs/dev/design/graphite/ (not opened). | Impeller and Graphite both record commands into GPU-friendly structures before submission and avoid per-draw state churn; Rive uses a path-level tessellation with GPU path rendering. | Check each before relying on it. Useful only as "what does the command recording look like" references. |

### Per-tile data structure (common pattern, sources above)

- Count pass: for each primitive, for each 32x32 tile its bounds touch, atomically increment `count[tile]` (or compute per-row counts, since the repair unit is a row).
- Scan pass: exclusive prefix sum over tile counts gives `offset[tile]`. Decoupled look-back (piet-gpu) or a 3-kernel scan (reduce, scan partials, add) does this in one dispatch or three.
- Scatter pass: each primitive writes its index to `list[offset[tile] + atomic_increment(cursor[tile])]`. Order within a tile is then non-deterministic unless the write goes to a pre-sorted slot.
- Sorted-order variant: sort (primitive, tile) pairs by (tile, z/paint order) with a radix sort, then build offsets from the sorted run. This is what Vello-style systems need to preserve paint order (**unconfirmed** for Vello specifically).

### Paint order

- Stable scatter: write each primitive's index into a slot computed from a per-primitive position in a pre-scanned list; the slot order equals the paint order. Requires two passes over primitives (one to count, one to scatter) and no atomic cursor.
- Atomic cursor scatter: fast, but order is lost. A second pass sorts each tile's list by paint index (cheap if lists are short).
- Compositing order in Vello is handled by encoding draws in order in the scene buffer and having fine tiles walk ptcl commands in order (consistent with the ptcl description; **unconfirmed** in detail).

### Memory bounds and overflow

- Fixed per-tile capacity: allocate `cap` entries per tile, count overflows, and fall back to a slow path (full-row scan) when `count > cap`. Simple, bounded, and what the current renderer effectively needs.
- Chunked lists: a tile's list is a chain of fixed-size chunks taken from a pool by atomic bump (piet-gpu stack approach). Bounded total memory via pool size; overflow = pool exhaustion, handled by flushing.
- Two-pass sizing: run the count pass first, read back the total on the CPU or via indirect dispatch, and size the scatter buffer exactly. Costs one GPU-to-CPU sync or a GPU-side indirect dispatch (the latter avoids the sync).

### Measured costs

- Haar/Aaltonen: 60 FPS with hundreds of thousands of objects on current consoles (slides not read; page statement only).
- Vello, Nanite, Frostbite: no timing numbers gathered in this run. Treat any number here as **unconfirmed**.

---

## 2. Retained and incremental acceleration structures

What is cached across frames and how invalidation is tracked.

| mechanism | source URL | what it does | mapping to 2D tile-damage UI renderer |
|---|---|---|---|
| Persistent instance buffer with dirty ranges (general engine pattern) | **Unconfirmed**: pattern from general knowledge; the Haar/Aaltonen 2015 slides are the closest primary source (https://www.advances.realtimerendering.com/s2015/index.html). | Instance transforms and material ids live in a GPU buffer; CPU writes only changed ranges; a per-chunk dirty bit or dirty range triggers upload. | The retained tree already knows dirty nodes. Keep primitive records in a persistent GPU buffer with a per-row (or per-32-row band) dirty bitset; upload only bands whose primitives changed. |
| Incremental BVH / TLAS refit | **Unconfirmed**: general knowledge of refit vs rebuild. | Refit updates bounds bottom-up without changing topology; cost O(nodes touched). Quality degrades under motion; rebuild periodically. | For 2D, the equivalent is a static quadtree or row-band index rebuilt only for dirty bands. Refit is unnecessary if bands are fixed-size. |
| Unreal Virtual Shadow Maps: page caching and static/dynamic separation | https://www.unrealengine.com/tech-blog/virtual-shadow-maps-in-fortnite-battle-royale-chapter-4 (secondary; snippet from search); Epic forums for CVars: https://forums.unrealengine.com/t/virtual-shadow-map-page-appearing-stuck-on-initial-load/2843200 | Shadow map is virtualised into pages; a page is reused across frames unless its source geometry changed. A new page can be reconstructed only if all source pages it covers were mapped last frame, otherwise a partial page would be cached (quoted from the Fortnite blog). Static and dynamic geometry are cached separately; dynamic-only invalidation is the default for moving objects. Distant lights are invalidated round-robin, capped by `r.Shadow.Virtual.MaxDistantUpdatePerFrame` (forum). | Page = fixed-size screen region (32x32 tile). Cache each tile's last rendered result plus a validity flag. Invalidate on: any primitive in the tile changed, any ancestor-clip/transform changed, or the tile's input set changed. Separate "static" (text that never moves) from "dynamic" (animated) layers so static tiles are not re-shaded. |
| Nanite streaming does not invalidate VSM pages (Epic staff, forum) | https://forums.unrealengine.com/t/vsm-invalidation-during-nanite-streaming/2511072 (secondary) | Streaming changes of LOD do not dirty cached shadow pages; an experimental CVar `r.Nanite.VSMInvalidateOnLODDelta` opts in. | Decouple resolution changes (LOD) from content changes for cache invalidation. In the UI case: a glyph atlas re-rasterisation at a new size should not dirty tiles unless the rendered pixels change. |
| Virtual texturing feedback + page residency | **Unconfirmed**: not fetched in this run (general knowledge: feedback buffer records requested pages, CPU maps them the next frame). | GPU writes which virtual pages were sampled; CPU maps them; one-frame latency. | Feedback = which glyph atlas pages / image tiles a damaged tile actually touched. Lets the renderer skip uploading unused glyph pages. |
| Temporal reuse (general) | **Unconfirmed**: general knowledge; see Nanite VSM above as the cache-validity concept. | Previous frame's results reused with reprojection or with a validity test. | For a 2D UI with no camera motion, temporal reuse is a per-tile "unchanged since frame N" test plus a sequence number, not reprojection. |

### Invalidation tracking (recommended pattern, not a single source)

- Each tile keeps `last_built_epoch`; each primitive in a retained node has `dirty_epoch`. A tile is rebuilt if any primitive intersecting it has `dirty_epoch > last_built_epoch`. Tile -> primitive intersection is precomputed once per layout change, not per frame.
- Store the reverse map (primitive -> tiles it touches) as a CSR list rebuilt only on layout changes.
- Per-frame cost on the CPU is then O(dirty primitives x tiles touched), typically a few hundred ops for a text edit.

---

## 3. Occlusion

| mechanism | source URL | what it does | mapping to 2D tile-damage UI renderer |
|---|---|---|---|
| Hierarchical Z (HZB) two-pass occlusion | https://advances.realtimerendering.com/s2021/index.html (Nanite); https://www.advances.realtimerendering.com/s2015/index.html (Haar/Aaltonen) | Depth pyramid built from the previous frame; cull instances whose bounds are behind it; retest after the first draw. | 2D analogue: a per-tile "opaque cover" pyramid (max-opaque-level per tile across layers) built during the paint pass; cull primitives behind a fully covered rect. |
| Front-to-back with early-out | **Unconfirmed**: general technique, see Vello fine stage for a 2D version (https://github.com/linebender/vello). | Draw nearest opaque first; a fragment test or tile counter stops work once a pixel is fully covered. | Process a tile's list from topmost to bottom; stop when the tile is fully covered by an opaque primitive. Requires paint order to be known, which the sorted-list variant provides. |
| "Last opaque cover" in 2D (Flutter/Skia/WebRender) | **Unconfirmed**: not opened. Impeller docs https://docs.flutter.dev/perf/impeller; WebRender docs not fetched. | Renderers track the last fully opaque rectangle that covers a region and drop earlier draws beneath it. | Store per tile the index of the last opaque cover primitive in paint order; the tile's list is then truncated at that index. Cheap to compute during the scatter step if cover is a rect. |
| Visibility buffer for 2D | https://advances.realtimerendering.com/s2021/Karis_Nanite_SIGGRAPH_Advances_2021_final.pdf (concept, slide body not read) | Store the id of the winning primitive per pixel, shade once. | For text: write glyph-run id per pixel in a coverage pass; the shade pass looks up the run's atlas and colour. Avoids shading occluded glyph pixels. |
| Opaque coverage test per tile (general) | **Unconfirmed** as to source. | Bitmask of which 4x4 sub-blocks of a tile are fully opaque. | A 32x32 tile fits in 64 bits as 8x8 sub-blocks; the bin pass can cull primitives entirely behind an opaque sub-block mask. |

---

## 4. Level of detail

| mechanism | source URL | what it does | mapping to 2D tile-damage UI renderer |
|---|---|---|---|
| Nanite screen-space error, one-pixel threshold | https://cs418.cs.illinois.edu/website/text/nanite.html ; GCores summary https://www.gcores.com/articles/184653 (both secondary); talk https://advances.realtimerendering.com/s2021/index.html | Simplification error is stored per cluster in the DAG; a cut is chosen where parent error projected to screen exceeds ~1 pixel and child error does not. Parent error is forced to be at least child error during offline build (secondary). | For glyphs: store the maximum pixel error of each rendering path (atlas sample vs. outline) at a given scale. Pick the cheaper path when the error is below 0.5 px. |
| Cluster LOD DAG | https://advances.realtimerendering.com/s2021/Karis_Nanite_SIGGRAPH_Advances_2021_final.pdf (slide body not read) | Clusters are grouped and simplified into parents; the cut is a set of clusters whose error is below threshold. | Text runs as a DAG: a run of glyphs collapses to a single "text blob" with an approximated coverage texture below a size threshold. |
| Mip selection (texture) | **Unconfirmed**: general knowledge; see Metal sampler docs (not fetched). | Pick mip where texel size matches pixel size. | Glyph atlas: pick the atlas cell at the scale quantised to a small set (e.g. 1/2, 1, 2 px per em steps) to bound cache size. |
| SDF / MSDF text at small sizes | **Unconfirmed**: not fetched in this run. Chlumsky MSDF project: https://github.com/Chlumsky/msdfgen (not opened). | Store distance fields; reconstruct edges; works across scales but corners need multi-channel distance. | Use SDF for scale-varying text to avoid re-rasterising, and bitmap glyphs below a threshold where SDF blurs. |
| Slug (Eric Lengyel) | **Unconfirmed**: not fetched in this run. Slug page: https://sluglibrary.com (not opened). | GPU evaluates quadratic Bezier coverage per pixel in a fragment shader; no atlas, exact at any scale. | Per-pixel exact evaluation avoids atlas LOD choices but costs per-pixel curve evaluation. Strong candidate for glyph-dense scenes if the fragment cost is bounded (**unconfirmed** cost numbers). |
| Imposters / impostor caching | **Unconfirmed**: general technique, not fetched. | Replace a distant mesh with a pre-rendered sprite refreshed when the view error exceeds a threshold. | Cache a complex text block as one texture; refresh only when its content changes or when its scale changes beyond a tolerance. |
| "Stop refining when pixels cannot differ" | https://cs418.cs.illinois.edu/website/text/nanite.html (secondary: temporal AA hides sub-pixel differences) | Refinement is stopped at error below 1 px, and TAA hides residual differences. | For UI, the equivalent threshold is device-pixel error <= 0.5 px at the render scale; in a non-temporal renderer the threshold should be lower. |

---

## 5. Job systems and parallelism

| mechanism | source URL | what it does | mapping to 2D tile-damage UI renderer |
|---|---|---|---|
| Naughty Dog fiber job system (Gyrling, GDC 2015) | https://www.gdcvault.com/play/1022186/Parallelizing-the-Naughty-Dog-Engine ; slides (403 on fetch): https://ubm-twvideo01.s3.amazonaws.com/o1/vault/gdc2015/presentations/Gyrling_Christian_Parallelizing_After.pdf | Jobs run on fibers so a job that waits on a counter yields its stack; goal was 60 FPS on PS4 for The Last of Us Remastered (landing page). Topics: frame-centric design, allocation patterns, lock strategy. Fiber stack size and wait mechanics **unconfirmed**. | Use fibers only if the renderer needs waits inside the job. For the bin pass, prefer plain jobs with no waits: the CPU budget is small. |
| Unreal TaskGraph | **Unconfirmed**: not fetched in this run. | Dependency graph of tasks with completion events; work stealing across threads. | A per-frame static graph for "extract -> bin -> repair -> present" with no dynamic allocation. |
| Bevy ECS parallel scheduling and render-world extract | **Unconfirmed**: not fetched in this run. Bevy docs: https://bevyengine.org (not opened). | Systems declare component access; the scheduler runs non-conflicting systems in parallel. Render world is extracted from the main world each frame. | The "extract" step is the retained-to-render copy. Make it write only dirty rows (the extraction is the place the dirty bitset is read). |
| Destiny job graph | **Unconfirmed**: not fetched in this run. | Engine-specific job graph with cost-based scheduling. | Background only. |
| Data-oriented design (Acton) | **Unconfirmed**: talk not fetched. | Organise data by access pattern (SoA), not by object. | Primitive records as SoA: positions, bounds, paint index, glyph run id in separate arrays so the bin pass touches only bounds. |
| SoA and SIMD batch processing (ISPC, Unity Burst/DOTS chunks) | **Unconfirmed**: not fetched in this run. ISPC: https://ispc.github.io (not opened). | Chunks of 16 KB (DOTS) of components; SIMD over lanes. | Bounds tests on 8-wide SIMD over primitive bounds; this is the cheap part of the bin pass. |
| False-sharing avoidance | **Unconfirmed**: general knowledge; pad per-thread counters to cache line (128 bytes on Apple silicon). | Per-thread counters on separate lines. | Per-row counts in the bin pass are written by different threads; pad or use per-thread local histograms. |
| Grain size | **Unconfirmed** as a number. | A job must amortise dispatch overhead (typically several microseconds on desktop; lower on Apple silicon). | For 100-300 K instructions per frame, one job per damage band (e.g. 8 rows) is the likely grain. |
| Frame pipelining (simulate N+1 while rendering N) | https://developer.apple.com/documentation/quartzcore/cametaldisplaylink (concept of display link pacing; body not read) | CPU simulation runs one frame ahead of GPU submission. | Build the bin lists for frame N+1 on the CPU while the GPU repairs frame N. Requires double-buffered list buffers. |

---

## 6. Frame pacing and latency

| mechanism | source URL | what it does | mapping to 2D tile-damage UI renderer |
|---|---|---|---|
| VK_EXT_present_timing | https://docs.vulkan.org/refpages/latest/refpages/source/VK_EXT_present_timing.html | Query refresh duration; request a present no earlier than a target time (`targetPresentTime`); read back per-stage timestamps via `vkGetPastPresentationTimingEXT`; queue size set by `vkSetSwapchainPresentTimingQueueSizeEXT`. Feedback can arrive several refresh cycles late (example: about five cycles on a 60 Hz phone). | The equivalent on Apple is the display link target timestamp. The key structure is: target present time chosen at submit, and feedback used to adjust the next frame's simulation time. |
| Metal CAMetalDisplayLink | https://developer.apple.com/documentation/quartzcore/cametaldisplaylink (title only fetched) | Display link tied to a `CAMetalLayer` that calls back per refresh with a drawable and timing (`targetTimestamp`, `targetPresentationTimestamp`, `preferredFrameLatency`). | Use `preferredFrameLatency` = 1 for latency; schedule the bin pass to finish before `targetTimestamp`. Field names are **unconfirmed** beyond the title. |
| Metal `present(at:)` / `present(afterMinimumDuration:)` | **Unconfirmed**: from memory; not in the fetched page. | Schedule presentation of a drawable at a time. | Use for frame-pacing when repair finishes early. |
| Late latching and reprojection (VR timewarp) | **Unconfirmed**: not fetched. | Sample pose as late as possible; re-project last rendered frame before scanout. | For UI: sample pointer/scroll position at the last moment; if the renderer is late, translate the last frame's tiles by the scroll delta instead of re-binning. |
| NVIDIA Reflex, AMD Anti-Lag | **Unconfirmed**: not fetched. | Delay CPU simulation start so input-to-display latency is minimised. | Equivalent: start the bin pass as late as possible, after the input event is in, with a deadline. |
| Input sampling as late as possible | **Unconfirmed**: general. | Read input right before simulation. | Read pointer position at bin-pass start, not at event delivery. |
| Click-to-photon measurement | **Unconfirmed**: not fetched. | Measure from input event to photon emission with a high-speed camera or a timestamp chain. | Use Metal GPU start/end timestamps plus `CACurrentMediaTime` at event and at present. |

Typical stage numbers: **unconfirmed** in this run. Budget at 60 Hz is 16.7 ms per frame; for a 120 Hz ProMotion display it is 8.3 ms.

---

## 7. Memory: streaming, budgets, compressed formats, and video-codec tools

### 7a. Memory and compressed formats

| mechanism | source URL | what it does | mapping to 2D tile-damage UI renderer |
|---|---|---|---|
| Transient/frame allocator (linear) | **Unconfirmed**: general pattern. | Bump allocate per frame, reset at frame end; no fragmentation. | Use for the bin lists and indirect args. Double-buffer by frame parity. |
| Streaming and budgets | https://advances.realtimerendering.com/s2021/index.html (Nanite streaming, talk title only) | Stream cluster pages by residency; budget by bytes. | Glyph atlas pages with LRU eviction and a hard byte budget. |
| Octahedral normal encoding | **Unconfirmed**: general technique (Cigolle et al., JCGT 2014, not opened). | Two-channel unit-vector encoding with 8-bit or 16-bit channels. | Not relevant unless the UI uses normals. |
| Nanite cluster bit budget | **Unconfirmed**: per-triangle bit count not confirmed. | Triangles encoded as local index triples with per-cluster vertex tables. | Index triples per glyph quad (4 vertices shared); use 16-bit local indices per run. |
| Vertex quantisation | **Unconfirmed**: general technique. | Positions as 16-bit fixed-point relative to a bounds box. | Glyph positions quantised to 1/64 px within a tile; 16-bit fixed point per tile. |
| GPU-decodable texture compression | **Unconfirmed**: BC/ASTC/ETC2 general. Apple GPUs support ASTC (general knowledge). | Block-compressed textures sampled directly. | Glyph atlas in ASTC 4x4 for alpha coverage, ~1 bpp effective for coverage data (**unconfirmed** exact quality). |

### 7b. Video codecs

| mechanism | source URL | what it does | mapping to 2D tile-damage UI renderer |
|---|---|---|---|
| Skip blocks | **Unconfirmed**: H.264/HEVC spec not fetched in this run. | A block with zero residual and predicted motion vector is coded as skip; no residual decoded. | A tile whose content equals the reference tile is skipped in the repair pass. |
| Motion vectors | **Unconfirmed**: spec not opened. | Each block points to a region in a reference frame. | For scrolling: a whole-tile motion vector = scroll delta; copy the reference tile instead of re-rendering. Only the newly exposed edge tiles need rendering. |
| Intra/inter mode decision by rate-distortion cost | **Unconfirmed**: general (J = D + lambda R). | Choose the mode minimising distortion plus lambda times rate. | Per tile, choose between "reuse previous", "copy shifted", and "re-render" by estimated cost (e.g. primitive count x coverage). |
| Reference frame management | **Unconfirmed**: general. | Keep N reference frames; choose which to predict from. | Keep the last two presented tile sets; a scroll reuses the previous frame's tiles. |
| Palette mode and intra block copy (screen-content coding tools in HEVC SCC and AV1) | **Unconfirmed**: spec not fetched (HEVC SCC is ITU-T H.265 v4; AV1 IntraBC is in the AV1 spec). | Palette mode codes a block as an index into a small colour palette. Intra block copy predicts a block from another position in the same frame, which is exactly "copy pixels from elsewhere in the same frame". | Palette: text is two-colour per tile, so a palette index per pixel plus a 2-entry table suffices. Intra block copy: repeated glyphs or repeated UI elements (scroll bars, icons) can be copied from an earlier tile position instead of re-rasterised. This is the closest codec tool to the glyph-dense case. |

---

## Ranked list: ten ideas most worth stealing

1. **Count / scan / scatter (with decoupled look-back) for per-tile lists, with a fixed per-tile cap and a slow-path fallback.** Mechanism: pass 1 atomically counts primitives per tile, a single-dispatch look-back scan produces offsets, pass 2 scatters indices. Source: https://raphlinus.github.io/gpu/2020/09/05/stack-monoid.html ; Olsson clustered shading (**unconfirmed** body) https://www.cse.chalmers.se/~uffe/clustered_shading_preprint.pdf. Gives a bounded, one-dispatch bin pass.
2. **Vello-style sparse strips and per-tile ptcl command lists.** Mechanism: encode the scene once into path/draw/transform buffers and emit per-tile command lists consumed by the fine stage. Source: https://github.com/linebender/vello/blob/main/ARCHITECTURE.md (structure confirmed at the summary level; list layout unconfirmed).
3. **Visibility-buffer-style "record id, shade once".** Mechanism: a coverage pass writes the winning glyph-run id per pixel; a single shade pass resolves colour from the id. Source: https://advances.realtimerendering.com/s2021/Karis_Nanite_SIGGRAPH_Advances_2021_final.pdf (concept confirmed from the talk listing; slides not read).
4. **Per-tile last-opaque-cover truncation during scatter.** Mechanism: if a primitive is an opaque rect covering the whole tile, drop all earlier entries in paint order. Source: **unconfirmed** as a named technique (Flutter/WebRender); the HZB two-pass pattern is confirmed in https://advances.realtimerendering.com/s2021/index.html.
5. **Two-pass HZB-style occlusion using last frame's visible set.** Mechanism: draw last frame's visible tiles first, build a cover pyramid, then retest the rest. Source: https://advances.realtimerendering.com/s2021/index.html (Nanite); https://www.advances.realtimerendering.com/s2015/index.html (Haar/Aaltonen).
6. **VSM-style page caching with a per-tile validity epoch and a split of static vs dynamic geometry.** Mechanism: a tile is reused unless a primitive intersecting it changed, and a tile whose LOD changed but not its pixels is not invalidated. Source: https://www.unrealengine.com/tech-blog/virtual-shadow-maps-in-fortnite-battle-royale-chapter-4 (secondary); https://forums.unrealengine.com/t/vsm-invalidation-during-nanite-streaming/2511072 (secondary).
7. **Intra block copy and motion-vector copy for scrolling.** Mechanism: a scroll delta is a whole-tile motion vector; copy the previous tile rather than re-rasterising. Source: AV1 IntraBC / HEVC SCC palette (**unconfirmed**, spec not fetched); the scroll analogue is general.
8. **Screen-space error threshold for glyph paths (~0.5 px) with a cluster-style DAG of simplified text blobs.** Mechanism: pick the cheaper glyph representation whenever its projected error is below threshold. Source: https://cs418.cs.illinois.edu/website/text/nanite.html (secondary, for the 1 px rule); https://www.gcores.com/articles/184653 (secondary).
9. **Fibers only where a job must wait, otherwise plain job graph with counters and no allocation.** Mechanism: static per-frame job graph with counter-based dependencies. Source: https://www.gdcvault.com/play/1022186/Parallelizing-the-Naughty-Dog-Engine (landing page confirmed; fiber internals unconfirmed).
10. **Present-timing-driven pacing: schedule the bin pass against the target present timestamp and sample input as late as possible.** Mechanism: `targetPresentTime` chosen at submit; feedback adjusts simulation time. Source: https://docs.vulkan.org/refpages/latest/refpages/source/VK_EXT_present_timing.html (confirmed); the Metal analogue https://developer.apple.com/documentation/quartzcore/cametaldisplaylink (title only fetched, fields unconfirmed).

## Open items for a follow-up pass

- Read the Nanite slide body for the visibility-buffer and software-rasteriser details (PDF URL above).
- Get the Naughty Dog slides through a path that is not 403 (or the GDC Vault video transcript).
- Read the Olsson clustered-shading PDF text (the fetch returned binary; try a local `pdftotext`).
- Read `research/doc/pathseg.md` and the "Potato" design doc in the Vello repo for per-tile list layout and overflow.
- Confirm Metal `present(at:)` and `CAMetalDisplayLink` property names against the full reference.
- Measure: the bin pass cost on glyph-dense scenes, with the count/scan/scatter variant versus the current per-row scan, on the target hardware.
