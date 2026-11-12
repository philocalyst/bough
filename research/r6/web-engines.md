# Browser engine damage, culling, LOD, parallelism, scheduling, memory: mechanisms for a tile-damage GUI framework

Scope: how Chromium (cc, Blink paint, LayoutNG), WebKit, Gecko/WebRender and the CSS/Web platform APIs keep retained state, decide what to repaint, skip invisible or occluded work, choose raster scale, split work across threads, schedule frames, and bound memory.

Method and confidence:
- Primary sources are engine source headers and .cc files (fetched), specs (W3C), and engine design docs (Chromium `docs/`, WebRender/Servo doc.servo.org source, Firefox source docs).
- Where a claim came only from a search snippet, a mirror, or a truncated page, it is marked `[unconfirmed]` and the URL is the one the snippet came from.
- Many Chromium pages were fetched at a revision where the header declares an interface but the mechanism lives in a `.cc` file that was not read. Those gaps are listed in section 7.
- Numbers are quoted from the cited source. Byte-per-tile figures are arithmetic, marked `[computed]`.
- Two sources disagree on WebRender tile size (see section 1, WebRender row). Both are reported.

Vocabulary used below:
- **damage**: region of the screen that must be redrawn/recomposited this frame.
- **invalidation**: region of a retained raster surface or tile that must be re-rastered.
- **expose damage**: screen region that changes because a layer appeared, disappeared or was reordered, with no re-raster.
- **coverage rect / visible rect**: WebKit/Chromium terms for the area that must be backed by tiles vs the area on screen.

---

## 1. Invalidation and damage

What each engine records about "what changed", how the minimal repaint region is derived, what is retained, and how scrolling avoids repaint.

### 1.1 Chromium: paint, raster and draw damage

Three layers of invalidation, per `docs/how_cc_works.md`: paint invalidation (Blink must repaint a part of the document), raster invalidation (a composited layer's texture must be re-rastered), and damage (draw invalidation: screen region to redraw). Source: https://chromium.googlesource.com/chromium/src/+/main/docs/how_cc_works.md

| mechanism | where | what it does | what a tile-damage framework could take from it |
|---|---|---|---|
| PaintArtifact = ordered display items partitioned into PaintChunks | https://chromium.googlesource.com/chromium/src/+/main/third_party/blink/renderer/platform/graphics/paint/README.md | Each chunk carries transform, clip, effect and scroll node references; chunks are the unit of compositing and of raster invalidation. | Partition the record tree into chunks that each carry the full property state (clip/transform/opacity/scroll), so invalidation is keyed by chunk and not by individual record. |
| RasterInvalidator matches chunks across frames by stable ID | same README (RasterInvalidator section) | New chunk: bounds fully invalidated. Removed chunk: old bounds fully invalidated. Moved chunk: old and new bounds both invalidated, because a move can expose content. | Give every record a stable id. A reorder invalidates both old and new rects, not just the moved item. |
| Property-specific invalidation | same README | If only the clip changed, only the symmetric difference of old and new clip bounds is invalidated. Any other property change invalidates old and new bounds fully. | Store the previous clip/transform per chunk. A clip-only change should cost the clip delta, not the whole chunk. |
| Display-item level pass | same README (DisplayItemRasterInvalidator) | Runs only for chunks that matched and kept properties. Appear/disappear/reorder invalidate the visual rect; clients marked paint-invalid get incremental or full invalidation by reason. | Two-level diff: chunk-level first (cheap), item-level only inside chunks whose properties are stable. |
| DamageTracker per RenderSurfaceImpl | https://chromium.googlesource.com/chromium/src/+/main/cc/trees/damage_tracker.h | Header comment: "Computes the region where pixels have actually changed on a RenderSurfaceImpl. This region is used to scissor what is actually drawn". Holds `current_damage_`, `damage_for_this_update_`, and sorted `rect_history_for_layers_` / `rect_history_for_surfaces_` keyed by layer id and mailbox id. | Keep a per-surface damage accumulator plus a per-layer rect history keyed by a resource identity (here mailbox id). History is what makes a reused buffer's damage correct. |
| DamageAccumulator is a single union rect plus reason bits | damage_tracker.h (DamageAccumulator) | `Union(rect, reasons)` ignores empty rects, grows one bounding rect with min/max, and ORs `DamageReason` bits. Geometry is one rect, not a list. | One union rect per surface is cheap and enough for scissoring. Keep reasons alongside, for debugging and for choosing full repaint. |
| Two damage kinds: invalidation damage vs expose damage | how_cc_works.md (Damage section) | Invalidation damage comes from re-raster. Expose damage comes from a layer being removed, added or reordered and needs no re-raster; both are forwarded in the CompositorFrame. | Separate "pixels changed" from "composition order changed". Overlay-only changes then skip recomposing the whole scene. |
| Damage forwarded to display compositor for partial swap | how_cc_works.md | cc calculates damage in DamageTracker and forwards it with the CompositorFrame; display compositor uses it for partial swap and to recomposite only changed regions. | The renderer should emit the damage union to the GPU present call as well as the scissor. |
| Rect history for partial-buffer reuse | damage_tracker.h (`rect_history_for_layers_`) | Stored per (layer, mailbox): when a buffer is reused, damage since that buffer was last drawn is the union of the history. `[unconfirmed]` exact buffer-age logic: the header has no buffer-age field. | Track per back-buffer what was drawn last time it was used, so that partial redraw works with N-buffering. |
| Commit copies the main-thread state to the impl side under block | how_cc_works.md (Commit Flow) | "Rather than sending an ipc, commit is done by blocking the main thread and copying data over." | Snapshot the record tree atomically at a frame boundary; do not diff across threads. |
| Pending tree is a staging copy; activation swaps it in | how_cc_works.md (Trees: commit/activation) | "There always exists 2-3" trees; pending is the staging area until async raster is done; activation pushes pending to active; old pending becomes a recycle cache. | Double-buffer the record tree. Keep the previous tree as a recycle source so unchanged subtrees are not reallocated. |
| Layer tree properties split from paint (property trees) | RenderingNG property trees, described in https://chromium.googlesource.com/chromium/src/+/main/third_party/blink/renderer/platform/graphics/paint/README.md (transform/clip/effect/scroll node refs) `[unconfirmed]` as a separate design-doc link | Transform, clip, effect and scroll are nodes in trees; chunks reference them by id. A scroll changes a scroll node, not every chunk. | Scrolling should change one offset node. Chunks that reference that node are re-positioned, not re-rastered. |
| LayoutNG immutable fragment tree, parent-constraint diffing | https://developer.chrome.com/blog/layoutng | Output is a new immutable fragment tree; each fragment stores the parent constraints that produced it; layout diffs old and new constraints to decide whether a child reruns. | Store the inputs that produced each output node; reuse the output when inputs compare equal. This is a memoization key, not a dirty flag. |
| LayoutNG measure/layout results cached for flex and grid | https://developer.chrome.com/blog/layoutng | Two-pass layouts are kept O(n) by caching measure results; without caching, nested two-pass layout reaches O(2^n). | Any multi-pass measure must cache its first pass, or depth causes exponential work. |

Chromium scrolling: the compositor moves tiles/ layers by changing offsets and re-running tile selection; raster is re-used because the tile grid is in layer space. Source for layer-space tiling: https://chromium.googlesource.com/chromium/src/+/main/cc/tiles/picture_layer_tiling.h (tiling indexed in layer content space, `TileAt(i, j)`).

### 1.2 Chromium: the tile grid and damage-to-tile mapping

| mechanism | where | what it does | what a tile-damage framework could take from it |
|---|---|---|---|
| PictureLayerTiling grid in content space | https://chromium.googlesource.com/chromium/src/+/main/cc/tiles/picture_layer_tiling.h | Tiles stored in `TileMap tiles_` (`absl::flat_hash_map<TileIndex, unique_ptr<Tile>>`), indexed by `TileAt(i,j)`; `kBorderTexels = 1`. | Hash map keyed by tile index is a valid sparse tile store; the 1-texel border is the cost of bilinear sampling and should be budgeted. |
| Tile size from the client | picture_layer_tiling.h (`CalculateTileSize(content_bounds)`) | Tile size is a virtual callback on the client, not a constant. | Make tile size a per-surface policy, not a global. |
| `Cover(rect, scale)` returns a minimal tile iterator | picture_layer_tiling.h (CoverageIterator) | Iterates the minimal set of tiles that covers a rect at a scale. | Damage rect to tile list should go through one coverage iterator; avoid per-tile tests. |
| Tile visibility test | picture_layer_tiling.h (`IsTileVisible`) | Tile bounds intersect `current_visible_rect_`. | Keep the visibility test in tile space. |
| Tile required-for-draw | picture_layer_tiling.h (`IsTileRequiredForDraw`) | Only active-tree, HIGH_RESOLUTION, visible, non-occluded tiles are required to draw. | A frame can be drawn without optional tiles. Define a "required" subset per frame. |

### 1.3 Chromium: cc frame-level damage consumers (selected from source)

| mechanism | where | what it does | what a tile-damage framework could take from it |
|---|---|---|---|
| Damage-based skip of a frame | scheduler: `FrameSkippedReason::kNoDamage` in https://chromium.googlesource.com/chromium/src/+/main/cc/scheduler/scheduler.h | A BeginFrame with no damage is skipped with a recorded reason (kNoDamage, kWaitingOnMain, kDrawThrottled, kRecoverLatency). | Make "nothing changed" a first-class, counted outcome; the frame loop should be able to skip with no work. |
| View-transition damage | damage_tracker.h (`previous_view_transition_content_surfaces_by_id_`) | A surface appearing or disappearing forces full damage on the view-transition layer. | Structural changes (add/remove a subtree) should trigger expose damage for the parent, not a per-record repaint. |

### 1.4 WebRender (Gecko): dependency-tracked tile invalidation

Source base: WebRender source as rendered on doc.servo.org (tile cache, invalidation, picture). Each tile records what primitives, clips, images and bindings it depends on; those lists are rebuilt each frame and compared to the previous frame.

| mechanism | where | what it does | what a tile-damage framework could take from it |
|---|---|---|---|
| Per-tile primitive dependency lists rebuilt each frame and compared | https://doc.servo.org/src/webrender/picture.rs.html (module doc, "## Invalidation") and https://doc.servo.org/src/webrender/invalidation/mod.rs.html | "Each tile keeps track of the elements that affect it." Dependencies are primitives, clips, image keys, opacity bindings, transforms. Lists are rebuilt per frame and compared to the previous frame. | Do not diff the record tree directly. Record, per tile, the set of inputs that produced its pixels; invalidate on input change. |
| PrimitiveCompareResult enum (`Equal`, `Descriptor`, `Clip`, `Image`, `OpacityBinding`, `ColorBinding`) | https://doc.servo.org/src/webrender/invalidation/mod.rs.html (lines 112-131) | `#[repr(u8)]` result of comparing two dependency entries; `Clip` means a clip corner changed. u8 chosen because it is a hot path. | Give each dependency kind its own outcome code so the invalidation reason is known, and so only the affected sub-rect is repainted. |
| InvalidationReason enum (`NoTexture`, `NoSurface`, `PrimCount`, `SurfaceOpacityChanged`, ...) | invalidation/mod.rs (lines 86-110); tile_cache/mod.rs (`post_update`) | Reasons are logged with the invalidation; `PrimCount` means the dependency list length changed. | Tag every invalidation with a reason. Lets you find which subsystem causes full repaints. |
| Per-tile dirty rect = union of invalidated quadtree leaves | invalidation/mod.rs (`DirtyRegion::add_dirty_region`, line ~81 `self.combined = self.combined.union(&raster_rect)`); tile_cache/mod.rs comment on `device_dirty_rect` | The tile's dependency quadtree is walked; the union of invalidated leaves is the dirty rect. It sets the scissor for replaying the tile's commands and can drive partial present. | Use a quadtree of dirty leaves per tile; derive one bounding dirty rect for the GPU scissor, keep the leaves for finer replay. |
| Multiple dirty rects known but collapsed to one | tile_cache/mod.rs (TODO(gw) comment on `device_dirty_rect`): "We have multiple dirty rects available due to the quadtree above... expose these as multiple dirty rects" | The quadtree yields several rects; the code currently takes their union. | Expose a list of dirty rects to the backend when the backend supports multi-rect submission; it is a known next step in WebRender itself. |
| Quadtree split/merge with max split level | tile_cache/mod.rs (`maybe_merge_or_split`, `max_split_level = 3`, operates on `cached_surface.root`) | Splits and merges dirty subdivision of the cached surface up to level 3. Internals in `quadtree.rs` were not read. | Bounded subdivision depth caps bookkeeping per tile. Use a hard depth limit. |
| Tile invalidation: `invalidate(Option<PictureRect>, reason)` | tile_cache/mod.rs | Invalidate a sub-rect or the whole tile; `None` means whole tile. | Same API for partial and full invalidation. |
| Non-visible tiles do not accumulate dependencies | tile_cache/mod.rs (`add_prim_dependency`) | "If this tile isn't currently visible, we don't want to update the dependencies." Offscreen tiles skip dependency bookkeeping. | Offscreen content should not pay dependency-tracking cost. Overlaps with section 2. |
| Tile removal when empty or invalid | tile_cache/mod.rs (`post_update`) | Tiles with no primitives or an empty valid rect are culled; opacity change calls invalidate with `SurfaceOpacityChanged`. | Garbage-collect tiles whose content is empty each frame. |
| Tile size: default vs scrollbar vs UI slice (conflicting values across sources) | tile_cache/mod.rs (`TILE_SIZE_DEFAULT` = 1024 x 512 device px; `TILE_SIZE_SCROLLBAR_HORIZONTAL` = 1024 x 32; `TILE_SIZE_SCROLLBAR_VERTICAL` = 32 x 1024) vs picture.rs module doc ("fixed-size large tiles of 2048x512 pixels (or 128x128 for the UI slice)") | Tile size is re-evaluated every 120 frames (`frames_until_size_eval`) or on override; a size change drops all tiles. The two sources disagree; the tile_cache file is the one with the constants. | Tile size should be a policy with periodic re-evaluation, and changing it should be a full drop, not a migration. |
| Tile cache screen-rect inflation | tile_cache/mod.rs (`TileCacheInstance::pre_update`) | Screen rect is mapped to picture space and inflated vertically by one tile height, so just-offscreen tiles stay alive. | A one-tile hysteresis band prevents thrash when scrolling back and forth. |
| Slices: content, UI, background | picture.rs ("## Picture caching" and "## Display List shape") | Display list is cut into a few slices (content, UI, background). "Each time a new scroll root is encountered, a new picture cache slice will be created." More than 8 slices are squashed into one. | Cut the record tree at scroll roots; each scroll root gets its own cache surface. Cap the number of surfaces. |
| Compositor surfaces with overlay tiles | picture.rs ("## Compositor Surfaces") | Opaque video/canvas primitives can be native compositor surfaces so they update without invalidating tile content; "overlay tiles" preserve ordering. | A large fast-changing element should be a separate surface, not a tile invalidation. |
| Partial updates gated by platform | tile_cache/mod.rs (`supports_dirty_rects`) | Draw/Layer compositors use `gpu_supports_render_target_partial_update`; native compositors use `max_update_rects > 0`; otherwise the full tile is dirty. | Choose dirty-rect policy at runtime from backend capability. |
| Retained display list in Gecko | https://arai.searchfox.org/firefox-main/source/layout/painting/RetainedDisplayListBuilder.cpp `[unconfirmed]` read through search snippet only | Gecko keeps a retained display list; partial builds rebuild a subset and merge it into the full list; per-frame flags (Modified, HasProps, HadWillChange) decide reuse. Code comments say WebRender still needs per-item retained lookups. | Partial rebuild plus merge of a retained list is the same idea as a B+tree page update. Keep per-item "modified" flags. |
| AnimatedGeometryRoot tree | Gecko commit "Build a tree of AnimatedGeometryRoots to speed up traversal of ancestors" https://source-hub.etos-pro.ru/mozilla/gecko-dev/commit/41a4ad2285afe6d009fe7429971e6b990f29a2b9 `[unconfirmed]` mirror | AGRs are the boundaries whose ancestors move without content change; items are re-positioned under an AGR rather than re-painted. | Scroll and animation roots are an explicit boundary type in the tree. Content under such a root is not re-rastered when the root moves. |

### 1.5 WebRender: interning and handles (change detection for primitives)

| mechanism | where | what it does | what a tile-damage framework could take from it |
|---|---|---|---|
| Interner in scene builder, DataStore in frame builder | https://doc.servo.org/webrender/intern/index.html | "similar in concept to a traditional string interning container, but it is specialized to the WR thread model." Interner hashes on the scene thread; DataStore is array-indexed on the frame thread. | Hash on the producer thread, index on the consumer thread. The consumer never hashes. |
| Delta UpdateList per transaction | intern/index.html | Changes are pushed to the frame builder as an UpdateList during a transaction. | Ship deltas, not snapshots, across the thread boundary. |
| Epoch in each handle for GC | intern/index.html | Each handle stores the last scene epoch used; epoch increments per scene build; GC removes items whose epoch is stale. | Epoch-based GC is simpler than reference counting across threads. |
| Handles give identity, so equal handle means reuse | intern/index.html `[unconfirmed]` whether a handle compare triggers re-upload | Handles are stable ids; the docs do not say whether a changed handle causes re-upload. | Use interned handle equality as the cheap first test before comparing contents. |

### 1.6 WebKit: layer-based compositing and tiled backing

| mechanism | where | what it does | what a tile-damage framework could take from it |
|---|---|---|---|
| TiledBacking visible rect vs coverage rect | https://github.com/WebKit/WebKit/blob/main/Source/WebCore/platform/graphics/TiledBacking.h | `setVisibleRect()` / `setCoverageRect()`; coverage is what tiles must back. `setLayoutViewportRect()` is "Only used to update the tile coverage map." | Two rects per surface: what is visible (paint priority) and what is backed (memory). They differ. |
| Coverage flags by motion | TiledBacking.h (`TileCoverage`: `CoverageForVisibleArea`, `CoverageForVerticalScrolling`, `CoverageForHorizontalScrolling`, `CoverageForScrolling`) | Coverage is expanded in the scroll direction depending on motion. | Expand the backed region along velocity, not uniformly. |
| Margin tiles | TiledBacking.h (`setHasMargins`, `setMarginSize`, `bounds()` "Includes margins.", `boundsWithoutMargin()`) | Extra tile rows/columns at edges to avoid exposing unbacked content. | A margin is a pre-allocated band that a scroll can reveal without raster. |
| Tile removal notification | TiledBacking.h (`willRemoveTile(TiledBacking&, TileGridIdentifier, TileIndex)`) | Client is told when a tile is removed; grids can switch over time; a secondary grid exists when zoomed-out contents scale is set. | Tile removal must be observable to any cache that holds tile handles. |
| Scrollability enum | TiledBacking.h (`TiledBackingScrollability`: `NotScrollable`, `HorizontallyScrollable`, `VerticallyScrollable`) | Selects which coverage expansion applies. | Use scroll axis as a property of the surface. |
| Threaded scrolling tree (commit of scroll state) | https://github.com/WebKit/WebKit/blob/main/Source/WebCore/page/scrolling/ScrollingTree.h | `commitTreeState(std::unique_ptr<ScrollingStateTree>&&)` applies main-thread state to the scrolling tree under `m_treeLock`; `scrollingTreeNodeRequestsScroll` returns whether the request was handled; `setNeedsApplyLayerPositions()` defers layer positioning. | Scroll is a request handled by the scroll tree; if unhandled, it goes back to the main thread. Keep that fallback explicit. |
| Coverage expansion during scroll and animation | WebKit commit "Expand compositing coverage rect when scrolling and animating" (r183354) https://trac.webkit.org/r183354 `[unconfirmed]` search snippet | Coverage is pushed into TiledBacking from outside, and grown during scroll/animation. | Expand coverage on motion and let the tile cache shrink it later. |

WebKit bug 198254 "Clean up TiledBacking coverage and scrollability" https://www2.webkit.org/show_bug.cgi?id=198254 is listed as open in search results `[unconfirmed]` status.

### 1.7 Scrolling avoids repaint: summary across engines

| engine | how scroll avoids repaint | source |
|---|---|---|
| Chromium cc | Scroll is an offset on a scroll node in the property tree. The compositor re-composites existing tiles at the new offset; raster only runs for newly exposed tiles inside the coverage window. | how_cc_works.md; picture_layer_tiling.h |
| WebRender | Tile cache is in picture space; screen rect maps to picture space; tiles outside the inflated screen rect are not visible; visible-tile dependencies are updated only for visible tiles. | tile_cache/mod.rs |
| WebKit | Coverage rect expanded in scroll direction; tiles inside coverage remain backed; scroll position lives in the threaded scrolling tree. | TiledBacking.h; ScrollingTree.h |
| Gecko APZ | Compositor applies scroll transforms without the main thread. Displayport (area with display list) is sized by velocity and direction; scroll is reconciled with the main thread later. | https://firefox-source-docs.mozilla.org/gfx/AsyncPanZoom.html |

---

## 2. Never doing work for what is not visible

Mechanisms that make offscreen or occluded subtrees cost nothing without an author writing a virtual list.

### 2.1 CSS `content-visibility` and `contain`

Normative text from https://www.w3.org/TR/css-contain-2/ (section numbers as given in the fetched copy):

- `content-visibility: auto` "Turns on layout containment, style containment, and paint containment for the element." It also "skips its contents" when the element is not relevant to the user. Skipped contents of `auto` remain available to user-agent features (find-in-page, tab order). `hidden` skips contents and makes them inaccessible to those features.
- An element is "relevant to the user" if it is on-screen (its paint containment box's overflow clip edge intersects the viewport), or if it or its contents are focused, or if it is selected or in the top layer.
- When an element skips its contents, size containment is added to its used `contain` value (§4). Size containment means the element's intrinsic size is computed "as if the element had no content" (§3.1).

`contain-intrinsic-size` and `auto` remembered size: https://www.w3.org/TR/css-sizing-4/#last-remembered (section 5.2.1 "Last Remembered Size", under 5.2 "Overriding Contained Intrinsic Sizes"). The fetched copy was truncated before the normative text, so the mechanism here comes from the css-contain-2 non-normative note (§4.2): `contain-intrinsic-size: auto` lets the engine "snapshot" the element's size from the last time it was rendered and use that size while skipped. Source: https://www.w3.org/TR/css-contain-2/

Shipping dates (from the WICG repo landing page): `contain-intrinsic-size` shipped in Chromium M83; `content-visibility` in Chromium M85. Source: https://github.com/WICG/display-locking

| mechanism | where | what it does | what a tile-damage framework could take from it |
|---|---|---|---|
| Relevance predicate | css-contain-2 §4 "relevant to the user" | On-screen, focused, selected, top-layer. A single boolean per subtree decides whether to lay out, style and paint it. | Keep one per-subtree relevance flag computed from the viewport, plus focus/selection as extra causes. Skip entire subtrees on false. |
| Size containment for skipped subtrees | css-contain-2 §3.1, §4 | A skipped subtree's box size is independent of contents, so scroll height stays stable. | Skipped subtrees must report a size without children; otherwise scrollbars and layout jump. |
| Remembered size (`contain-intrinsic-size: auto`) | css-contain-2 §4.2 (non-normative); css-sizing-4 §5.2.1 | Last rendered size is reused for a skipped element, so the placeholder matches the content after first render. | Cache the measured size of each subtree on first render and reuse it as a placeholder. This is the basis of scroll-stability for virtualised content. |
| Find-in-page and tab order | css-contain-2 §4 | `auto` keeps skipped content reachable by user-agent features; `hidden` does not. | Accessibility and search must still see skipped records. A skipped subtree needs a cheap text path. |
| Chromium Display Locking (`content-visibility` implementation) | https://github.com/WICG/display-locking `[unconfirmed]` mechanics: the explainer files (`explainers/content-visibility.md`) were not fetched | Umbrella for content-visibility, contain-intrinsic-size, `hidden=until-found`, `renderpriority`. Repo is archived; work moved to CSSWG/WHATWG. | Treat "lock" as a state of a subtree that gates style/layout/paint; unlock on relevance. |
| Chromium `IntersectionObserver` driving relevance | https://www.w3.org/TR/intersection-observer/ §3.4.1 (runs as a substep of "Update the rendering"); §3.2.8 (`update the intersection observations steps`) | Intersection state is computed at the rendering step, not as a separate timer. `isIntersecting` is true when target and root "intersect or are edge-adjacent". | Compute visibility at the frame boundary, in the same pass as layout. Do not run a separate viewport query. |
| IntersectionObserver clipping by ancestors | intersection-observer spec §2.3, §3.2.7 step 3.4 | Target rect is intersected with each ancestor's clip rect (content clip or clip-path) up to the root. | Visibility must go through ancestor clips; a child inside a clipped parent is not visible. |
| IntersectionObserver scroll-delta optimization | https://chromium.googlesource.com/chromium/src/+/339bcd998b16c2069c2b7aab39b10cb07ff50eca (commit "Accumulate scroll delta per IntersectionObservation") | Tracks accumulated scroll delta per observation, so one observer can skip recomputation when the delta is below its threshold. | Skip visibility recomputation until the accumulated movement crosses the threshold. |
| Hidden frames stop lifecycle, observers must be forced | Chromium commit "[IntersectionObserver] Handle hidden OOPIF's correctly" https://chromium.googlesource.com/chromium/src/+/f9b9495ccdbf5d13b7c19d922a184e4139fb4ea8 `[unconfirmed]` (search snippet) | Hidden frames stop running lifecycle updates, so observers would stop; the fix forces observers to run. | Visibility state must be well-defined when the whole surface is hidden. |

### 2.2 Chromium cc: occlusion culling

| mechanism | where | what it does | what a tile-damage framework could take from it |
|---|---|---|---|
| OcclusionTracker walks layers front to back | https://chromium.googlesource.com/chromium/src/+/main/cc/trees/occlusion_tracker.h | Driven by `EffectTreeLayerListIterator`; calls `EnterLayer`, `LeaveLayer`, `EnterRenderTarget`, `FinishedRenderTarget`, `LeaveToRenderTarget`, `MarkOccludedBehindLayer`. | A front-to-back iteration with enter/leave callbacks is the shape of the occlusion pass. Make the tree walk the place occlusion is maintained. |
| Stack of occlusion regions per render target | occlusion_tracker.h (`std::vector<StackObject> stack_`; `occlusion_from_outside_target`, `occlusion_from_inside_target`, `SimpleEnclosedRegion`) | Occlusion is a stack keyed by target id (an integer id, not a pointer, to avoid pointer-wrapping cost). When a subtree finishes, its mask applies only to that subtree. | Keep occlusion state per scope in a stack, popped on leave. Use ids, not pointers, in the stack. |
| `GetCurrentOcclusionForLayer` query | occlusion_tracker.h | Returns the occlusion for a layer's content space so the caller can skip quads or tiles. The discard code is outside this header. | Expose occlusion as a query; the draw path decides whether to skip. `[unconfirmed]` exact quad-skipping code path. |
| Occluded tile pruning in tile manager | https://chromium.googlesource.com/chromium/src/+/main/cc/tiles/tile_manager.h (`FreeResourcesForOccludedTiles()`) | Occluded tiles can release their resources; `ScheduleReduceTileMemoryWhenIdle()` and `ScheduleTrimPrepaintTiles()` reduce memory in the background; `kDelayBeforeTimeReclaim = base::Minutes(5)`. | An occluded tile is a memory-reclaim candidate, not just a draw-skip. |
| Occluded visible tiles demoted from NOW to SOON | https://chromium.googlesource.com/chromium/src/+/main/cc/tiles/picture_layer_tiling.cc (`ComputePriorityForTile`: `priority_bin = is_tile_occluded ? SOON : NOW`) | An occluded tile is still needed soon but does not block activation/draw. | Occlusion feeds priority, not just visibility. |
| Occlusion in WebRender: CPU rectangle occluder | https://doc.servo.org/src/webrender/rectangle_occlusion.rs.html (`FrontToBackBuilder`) | Front-to-back insertion into two lists: `opaque_items` (non-overlapping, any draw order) and `alpha_items` (drawn back to front). Each rect is cut by existing opaque rects into at most four pieces per occluder; `SmallVec` inline capacity of 16. | Front-to-back opaque culling with append-only lists is cheap and removes the need for a depth buffer. See section 2.3. |
| Occlusion in WebRender tiles | picture.rs (`prepare_tiled_picture_surface`): `composite_state.occluders.is_tile_occluded(tile.z_id, world_draw_rect)`; occluded tile with native surface frees it | Only checked for fixed-position visible tiles under Native/Draw compositors; occluded native surfaces are freed and re-allocated on reveal. | Occlusion for tiles keyed by a z id per tile; a tile fully covered by an opaque tile in front is not rastered or drawn. |
| WebRender occluder cost note | rectangle_occlusion.rs module doc | "The cost of the algorithm grows with the number of opaque rectangle as each new rectangle is tested against" every existing opaque rect. Suggests spatial acceleration for many occluders, and treating small front opaque items as non-opaque to reduce splits. | Cap the occluder count per frame or cull small occluders to alpha. |
| Opaque-picture detection via backdrop candidates | Bugzilla 1742742 https://bugzilla.mozilla.org/show_bug.cgi?id=1742742 `[unconfirmed]` search snippet | Opaque pictures are detected from backdrop candidates during batching instead of checking every primitive. | Compute opacity once per surface, not per primitive. |
| CPU-side occlusion for picture-cache tiles | Bugzilla 1696905 https://bugzilla.mozilla.org/show_bug.cgi?id=1696905 `[unconfirmed]` search snippet | Introduced CPU occlusion culling for picture cache tiles, removing the need for a depth buffer; tiles are split into visible parts. | A tile-based UI can do occlusion on the CPU at tile granularity. |

### 2.3 WebRender opaque and alpha passes

| mechanism | where | what it does | what a tile-damage framework could take from it |
|---|---|---|---|
| Opaque pass first, alpha pass back to front | rectangle_occlusion.rs (`opaque_items`, `alpha_items`) | Opaque rects do not overlap, so their order does not matter; alpha is composited after, back to front. | Two-pass drawing: opaque first with no blending and no sorting; alpha after. |
| Opaque tile determined per tile | tile_cache/mod.rs (`Tile::is_opaque`; `has_opaque_bg_color`, `has_opaque_backdrop`) | Tile opacity picks the opaque or alpha native surface. A single-primitive opaque tile with a color backdrop can be drawn as `TileSurface::Color`. | An opaque tile is cheaper: no blending, no clear. |
| Overlay tiles | picture.rs ("## Compositor Surfaces") | When a compositor surface changes draw order within a tile, "overlay tiles" are drawn in alpha mode only where content sits above the surface; otherwise the fast path. | Only pay the alpha path where ordering actually requires it. |

### 2.4 Layer squashing and `will-change` (Chromium and WebKit)

| mechanism | where | what it does | what a tile-damage framework could take from it |
|---|---|---|---|
| Squashing: merge chunks into one composited layer | Blink compositing README https://crrev.com/bc3c4e4d7e0115ad011315e1413541a1a2586cdc/third_party/blink/renderer/platform/graphics/compositing/README.md `[unconfirmed]` (crrev mirror); chunks merged into `cc::Layer`s by `PendingLayer` merging, limited by direct compositing reasons, overlap and sparsity | Each paint chunk starts as a PendingLayer; adjacent chunks are merged unless a direct compositing reason or overlap forbids it. | Merge adjacent records into one surface unless an invalidation/animation reason forbids it. Otherwise you get surface explosion. |
| Squash-preventing reasons separated from compositing reasons | Chromium 2016 code review, exact review not confirmed (search snippet listed https://codereview.chromium.org/1730653002) `[unconfirmed]` | Squashing-disallowed reasons are kept separately so the information is not lost in the merge. | Record why a merge was refused; it is the first debugging question. |
| Layer explosion background | GPU Accelerated Compositing in Chrome design doc https://www.chromium.org/developers/design-documents/gpu-accelerated-compositing-in-chrome/ | Overlapping layers multiply; intrinsic triggers (e.g. 3D transform) are "direct" compositing reasons. | A surface count budget is needed; overlap is the multiplier. |
| `will-change: transform` raster scale | https://chromium.googlesource.com/chromium/src/+/main/cc/layers/picture_layer_impl.h (`raster_source_size_changed_`, `MinimumRasterContentsScaleForWillChangeTransform()`, `AdjustRasterScaleForTransformAnimation`) | Raster scale is held for will-change/transform animation so it does not re-raster per frame. Bodies are in picture_layer_impl.cc, not read. | Declaring an intent to animate should fix the raster scale at declaration time, not per frame. `[unconfirmed]` exact policy. |

---

## 3. Level of detail and zoom

### 3.1 Chromium: ideal vs raster scale, low-res tiling, pinch zoom

Source: https://chromium.googlesource.com/chromium/src/+/main/cc/layers/picture_layer_impl.h

Quoted from header comments:
- "Ideal scales are calcuated from the transforms applied to the layer." Three parts: `ideal_page_scale_` ("Page scale is from user pinch/zoom."), `ideal_device_scale_` (from device scale factor), `ideal_source_scale_` ("Source scale comes from javascript css scale.").
- `ideal_contents_scale_` = device scale × page scale × source scale.
- "Raster scales are set from ideal scales. They are scales we choose to raster at. They may not match the ideal scales at times to avoid raster for performance reasons."
- `directly_composited_image_default_raster_scale_`: for an image-only layer, the raster scale can differ from the ideal "to avoid re-raster on scale changes"; `CalculateDirectlyCompositedImageRasterScale()` "takes into account the ideal contents scale to ensure we don't use too much memory" for small layers.

Tiling: https://chromium.googlesource.com/chromium/src/+/main/cc/tiles/picture_layer_tiling.h. `contents_scale_key()` is used to index and sort tilings; "tilings must have unique scales." `TileResolution` has `HIGH_RESOLUTION` and `NON_IDEAL_RESOLUTION`; only HIGH_RESOLUTION tilings are drawable. Low-resolution tiling creation logic lives in `picture_layer_impl.cc`/`picture_layer_tiling.cc` and was not read `[unconfirmed]` for the exact creation trigger.

| mechanism | where | what it does | what a tile-damage framework could take from it |
|---|---|---|---|
| Ideal contents scale vs raster contents scale | picture_layer_impl.h | Ideal is what the transforms ask for; raster is what is rastered; they may differ "to avoid raster for performance reasons". | Keep two scales per surface: the target (from transform/zoom) and the raster (what is actually painted). Do not re-raster on each transform sample. |
| Tilings per contents scale in a set | picture_layer_tiling.h (`PictureLayerTilingSet`, `AddTiling`, `CleanUpTilings`, `MinimumContentsScale`, `MaximumContentsScale`) | Multiple tilings at different scales coexist; the set picks the one to draw. | Store a pyramid of tile grids at scales; draw from the nearest one while the ideal one is rastered. |
| HIGH vs NON_IDEAL resolution | picture_layer_tiling.h (`set_resolution`, `TileResolution`) | Only high-resolution tilings are drawn. Low-res tiling serves during gesture. | This is the pinch-zoom fallback: show the low-res tiling, raster the ideal one in the background. |
| Tiling-only visible check uses `ideal_contents_scale` | picture_layer_impl.h (`ShouldUpdateApproximatedVisibleContentArea(TileResolution)`) | The visible-content approximation is updated per resolution. | Keep per-resolution visible rects. `[unconfirmed]` exact approximation rule. |
| Gesture re-raster after end | APZ and Chromium behaviour `[unconfirmed]` exact timing | Chromium re-rasters the high-res tiling after gesture; during the gesture the raster scale is held. | Hold scale during the gesture, re-raster after it ends. Not confirmed in source. |
| Skewport target time (GPU raster) | https://chromium.googlesource.com/chromium/src/+/main/cc/trees/layer_tree_settings.h (`gpu_rasterization_skewport_target_time_in_seconds = 0.2f`; `skewport_target_time_in_seconds` for software) | The skewport (area around viewport to pre-raster) is sized so that it can be rastered in about 0.2 s of GPU time. | Size the prepaint band by time budget, not by a fixed pixel count. |
| Prepaint threshold relative to skewport | picture_layer_tiling.cc (`MakePrioritizedTile`: `process_for_images_only = distance > min_preraster_distance_ && (distance > max_preraster_distance_ || distance > 0.5f * max_skewport_extent_in_screen_space_)`) | Tiles farther than half the skewport extent are not pre-rastered, only their images are decoded. | Pre-raster by distance threshold relative to the band size. |

### 3.2 Chromium: solid colour detection

| mechanism | where | what it does | what a tile-damage framework could take from it |
|---|---|---|---|
| `SolidColorAnalyzer::DetermineIfSolidColor` on the paint op buffer | https://chromium.googlesource.com/chromium/src/+/main/cc/paint/solid_color_analyzer.cc | Runs the op list against an `SkNoDrawCanvas` clipped to the tile rect and returns `SkColor4f`, `kTransparent`, or `nullopt`. | Analyse the recorded op list, not the pixels, to prove a tile uniform. |
| Immediate rejection of non-solid ops | solid_color_analyzer.cc | Rejects `kDrawImage`, `kDrawImageRect`, `kDrawTextBlob`, `kDrawTextSlugs`, `kDrawPath`, `kDrawVertices`, `kSaveLayer*`, `kClipPath`, and others. | Any image, text or path in the tile makes it non-uniform; the check is conservative and cheap. |
| Op budget `max_ops_to_analyze` | solid_color_analyzer.cc (`++num_draw_ops > max_ops_to_analyze` returns nullopt); raster_source.h (`PerformSolidColorAnalysis(..., int max_ops_to_analyze = 1)`, comment "solid color tiles generated for views have 3 operations") | Analysis is capped; default is 1 at the raster source; comment notes even simple lists can have more than one op. | Keep the uniformity check bounded; it runs per tile per frame. |
| Coverage proof | solid_color_analyzer.cc (`IsFullQuad`) | Requires the clip to be a rect, to cover the layer, axis-preserving matrix that inverts, and the drawn shape to contain the clip bounds. | Uniform only if the fill covers the whole tile; partial rectangles do not qualify. |
| Blend restriction | solid_color_analyzer.cc (`IsSolidColorBlendMode` accepts only `kSrc` and `kSrcOver`; `CalculateSolidColor`) | Only src and src-over are folded into a color; the source-over blend is computed when the source is opaque or the canvas is already solid. | Fold only the blend modes you can prove; anything else falls back to raster. |
| Platform restriction | solid_color_analyzer.cc (Mac: colour must be opaque, citing crbug 922899) | Platform-specific constraint on transparency. | Keep a per-platform rule for alpha handling. |
| Raster source flags | https://chromium.googlesource.com/chromium/src/+/main/cc/raster/raster_source.h (`IsSolidColor()`, `GetSolidColor()`, `is_solid_color_`, `solid_color_`) | A whole raster source can be flagged solid and stored as a color. The header says the members are const because the source "may be in use on another thread." | Solid flag is set once at record time and read on the raster thread; const immutable. |
| What a solid tile becomes | raster_source.h comments do not say `[unconfirmed]` whether the tile is drawn as a solid quad or skipped from raster | Presumed: raster is skipped and a color quad is drawn. Not confirmed in the read files. | Replace raster with a fill; still keep a tile-slot so later changes can invalidate it. |
| WebRender equivalent: `TileSurface::Color` | tile_cache/mod.rs | A tile with one primitive, opaque, with a color backdrop is drawn as a color, not a texture. | Same idea, decided on the dependency list rather than on ops. |

### 3.3 Images: decode scale and mip levels (Chromium)

| mechanism | where | what it does | what a tile-damage framework could take from it |
|---|---|---|---|
| GPU decode cache decodes at target size | GPU image decode cache, mirror https://gitcode.com/openharmony-tpc/chromium_src/blob/master/cc/tiles/gpu_image_decode_cache.cc `[unconfirmed]` mirror snippet | Decodes straight to the target size if supported; else decodes at the nearest larger supported size then scales; NEAREST quality decodes at original size. | Decode to the size you will display, not the source size. |
| Filter quality capped at medium; mips only at medium and above | gpu_image_decode_cache.cc (mirror, chromium_src 114) `[unconfirmed]` | "Filter quality is capped at medium. Mips are not generated when filter quality is below medium." | Generate mip chains only for quality levels that sample them. |
| Sampling options from filter quality | https://chromium.googlesource.com/chromium/src.git/+/HEAD/cc/paint/paint_flags.cc (`FilterQualityToSkSamplingOptions`) `[unconfirmed]` partial read | Low: linear, no mipmaps. None: nearest, no mipmaps. High: CatmullRom cubic. Medium mapping not read. | Map a quality tier to one sampler so every consumer agrees. |
| Downscale uses mipmaps unless quality is None | Chromium code review https://codereview.chromium.org/2870753002/patch/120001/130003 | Software decode cache: downscale uses mipmaps (medium) unless None, which keeps the pixelated scale. | Downscale via mip, not on demand; keep a pixelated mode for exact pixel art. |
| Image decode cache API: refs and budget | https://chromium.googlesource.com/chromium/src/+/main/cc/tiles/image_decode_cache.h | `GetTaskForImageAndRef` must be paired with `UnrefImage`; `GetDecodedImageForDraw` with `DrawWithImageFinished`; `ReduceCacheUsage()` runs periodically on the compositor thread; `SetShouldAggressivelyFreeResources()` for hidden content; `GetMaximumMemoryLimitBytes()` is the locked budget. | Pin/unpin API for decoded images with an explicit budget; hidden content releases aggressively. |
| Predecode decodes only, does not upload | image_decode_cache.h (`GetOutOfRasterDecodeTaskForImageAndRef`) | Predecode runs on worker threads and must not upload to GPU. | Split decode (any thread) from upload (GPU thread). |
| Checker-imaging | layer_tree_settings.h (`enable_checker_imaging = false`): "the compositor may selectively defer image decodes ... and raster tiles without images until the decode is ready" | Tiles that need an undecoded image are rastered with a checker pattern until the decode is ready. | A placeholder fill for a not-yet-decoded image is preferable to blocking the tile. |

### 3.4 WebRender and sub-pixel / tiny-size skipping

| mechanism | where | what it does | what a tile-damage framework could take from it |
|---|---|---|---|
| Visibility by screen rect intersect | tile_cache/mod.rs (`Tile::is_visible` intersects world rect with screen) | Only visible tiles contribute to `world_culling_rect`. | Visibility per tile, before any other work. |
| Dirty rect empty means nothing to do | invalidation/mod.rs (`DirtyRegion`, `combined` empty) | Empty combined dirty region produces no raster work. | Empty damage is a valid, cheap frame. |
| Sub-pixel and tiny-size skipping | `[unconfirmed]` no WebRender or cc source read for a sub-pixel size threshold | Not verified in any source read. | Do not claim this without a source. Test it yourself. |
| Text at tiny sizes | `[unconfirmed]` not researched in primary sources | Not verified. | Out of scope for this evidence set. |

---

## 4. Parallelism

### 4.1 Chromium: threads, commit, activation

| mechanism | where | what it does | what a tile-damage framework could take from it |
|---|---|---|---|
| Main thread vs impl (compositor) thread | how_cc_works.md ("Process / thread architecture") https://chromium.googlesource.com/chromium/src/+/main/docs/how_cc_works.md | Single-threaded cc is used by the browser; multi-threaded by renderers. Multi-threaded "incurs a latency cost" but input and animation stay responsive while the other thread is busy. | Put input and animation on a thread that never waits for the main thread. |
| Commit: block main thread and copy | how_cc_works.md ("Commit Flow") | Commit is atomic and blocks the main thread during the copy; no IPC. | Snapshot the record tree synchronously at a defined point; keep the copy cheap. |
| Commit aborted when nothing changed | how_cc_works.md | Commit can be aborted early if no work is needed; single-threaded cc never aborts. | Compare a change counter first; skip the copy when nothing changed. |
| Pending/active trees with activation | how_cc_works.md ("Trees: commit / activation"); picture_layer_tiling.h (`IsTileRequiredForActivation`, `can_require_tiles_for_activation_`) | Pending tree waits for async raster; activation copies it to active; activation waits until required tiles are rastered. | Gate activation on the set of "required" tiles, so a new frame is never drawn half-rastered. |
| Recycle tree | how_cc_works.md | The previous pending tree is kept as a cache of the last pending tree. | Reuse allocations across frames. |
| Single commit per BeginFrame | scheduler.h (`ScheduledActionCommit`): "at most one commit per BeginFrame" | Limits main-to-impl copies per frame. | Cap the number of snapshots per frame. |
| Raster on worker threads with a task graph | how_cc_works.md ("Raster and tile management") | "TaskGraphs are not updated dynamically, but instead rescheduled as a whole graph." Tasks cannot be cancelled once started. | Rebuild the raster task graph per frame from priorities rather than mutating it. |
| Tile manager coordinates raster and decode | tile_manager.h (`TaskGraphRunner`, `CheckerImageTracker`, `image_worker_task_runner`) | Raster and image decode are scheduled in one place with priority tiers: required-for-activation/draw, then pre-paint, then pre-decode. | One scheduler for all asynchronous producers, with explicit tiers. |
| `PrepareTiles` assigns memory and schedules | tile_manager.h (`PrepareTiles(const GlobalStateThatImpactsTilePriority&)`, `AssignGpuMemoryToTiles()`) | Memory is assigned in priority order, then tasks are built and scheduled; `NotifyReadyToActivate` / `NotifyReadyToDraw` fire when required tiles are done or failed. | Assign budget before work; the budget decides what gets work. |
| Tile sizing for raster | how_cc_works.md ("Raster and tile management") | About 256 x 256 px for software raster; viewport width x viewport height / 4 for GPU raster. | Tile size should depend on the raster backend. |
| `max_staging_buffer_usage_in_bytes = 32 MiB` | layer_tree_settings.h | Cap on staging upload buffers. | Put an explicit cap on upload staging. |

### 4.2 WebRender: scene builder, frame builder, render backend

| mechanism | where | what it does | what a tile-damage framework could take from it |
|---|---|---|---|
| Scene builder thread | https://doc.servo.org/src/webrender/scene_builder_thread.rs.html (not fetched; names from search results) `[unconfirmed]` mechanics | Transactions are routed through the scene builder by default. | Separate "build the scene" from "generate the frame". |
| Transactions can skip the scene builder | https://doc.servo.org/webrender/render_api/struct.Transaction.html (API doc, search snippet) `[unconfirmed]` full text | Skipping is used to avoid jank for animated property updates, panning and zooming. Skipping transactions can race ahead of non-skipping ones. | Scroll and zoom updates should not wait for the scene rebuild. |
| GenerateFrame bypass for APZ | Mozilla bug, number not confirmed `[unconfirmed]`; search snippet from the webrender_api `api.rs` / Transaction docs https://doc.servo.org/webrender/render_api/struct.Transaction.html | APZ scroll stayed slow because GenerateFrame transactions were queued behind scene builds; the fix made them bypass the scene builder. | Frame generation from an updated transform must not wait for the scene. |
| Hit-testing direct to scene builder | Mozilla bug, number not confirmed `[unconfirmed]`; search snippet only | Latency from requests sitting in a busy render-backend loop; direct path avoids it. | Hit-testing needs its own low-latency path. |
| Interned data: hash on one thread, index on another | intern/index.html | Interner lives in the scene builder thread; DataStore in the frame builder; UpdateList carries deltas per transaction. | See section 1.5. |
| Frame builder reads without hashing | intern/index.html | "the frame builder reads the interned content by array indexing." | Index-only reads on the render thread. |

### 4.3 Servo and Stylo: parallel style and layout

| mechanism | where | what it does | what a tile-damage framework could take from it |
|---|---|---|---|
| Parallel style with work-stealing queues | https://hacks.mozilla.org/2017/08/inside-a-super-fast-css-engine-quantum-css-aka-stylo/ | When a node is processed, its children become work units in a queue; idle cores steal from other queues. "avoid walking the tree ahead of time to balance the load". | Work-stealing over subtrees, not a pre-balanced partition. |
| Rust type system for data-race freedom | Stylo article (quoted: "With Rust, you can statically verify that you don't have data races") | Parallel traversal is checked by the compiler; the article does not explain the Send/Sync mechanism. | Use the type system to restrict which record fields cross threads. |
| Parallel traversal via rayon in Servo | https://doc.servo.org/src/style/parallel.rs.html `[unconfirmed]` search snippet for the module doc | `style::parallel` is a traversal "based on Rayon". | rayon is a usable, proven work-stealing layer. |
| Parallelism is opportunistic: floats and counters block it | Servo Layout Engines Report https://www.github.com/servo/servo/wiki/Servo-Layout-Engines-Report (search snippet `[unconfirmed]` full text) and Servo design wiki | "parallelism is used opportunistically, when a given subtree allows it"; floats prevent parallelism for their sections. | Mark which record kinds have sequential dependencies; parallelise only independent subtrees. |
| Fragment tree as separate output of layout | https://book.servo.org/design-documentation/architecture.html (search snippet) `[unconfirmed]` full text | Layout produces a fragment tree separate from the box tree. | Keep inputs (box/records) and outputs (fragments/tiles) as separate trees. |
| Stylo claim of speed-up | Stylo article: "can run up to 2 or 4 or even 18 times faster"; "close to linear speed ups" | Claims without stated benchmark method. | Treat as an upper bound, not a measurement. |

### 4.4 Gecko APZ and threads

| mechanism | where | what it does | what a tile-damage framework could take from it |
|---|---|---|---|
| APZ thread model | https://firefox-source-docs.mozilla.org/gfx/AsyncPanZoom.html | Input and scroll are handled on the compositor/controller thread; the main thread runs content. Off-main-thread compositing applies scroll as transforms at composite time. | Scroll/zoom should be a transform applied at composition, outside the record tree. |
| Displayport sizing | AsyncPanZoom.html | Displayport (area with a display list) must be larger than the viewport, but cannot be the whole page; sized by velocity and direction; recomputed while panning. | Keep a prepaint region sized by velocity. |
| Checkerboarding | AsyncPanZoom.html | If the displayport does not cover an area reached by a fast scroll, unpainted content is shown; the design goal is to avoid it. | Measure and report checkerboard area; it is the metric to minimise. |
| Reconciliation after gesture | AsyncPanZoom.html | Scroll positions on the two threads are reconciled when a repaint is requested; "scroll changes made by web content ... are never lost". | The compositor offset and the record tree offset must reconcile without losing app-side changes. |

### 4.5 What is shared between threads and how snapshots are taken (summary)

| engine | shared data | snapshot mechanism | source |
|---|---|---|---|
| Chromium cc | Layer tree copied per commit; tiles, raster tasks, images | Commit blocks the main thread and copies; pending tree is activated after raster | how_cc_works.md |
| Chromium Blink paint | PaintArtifact (chunks, display items) read by compositor | Compositor input built from artifact at paint; RasterInvalidator diffs artifact against prior one | PaintChunks README |
| WebRender | Interner/DataStore; UpdateList deltas per transaction; scene built on scene thread | Epoch per scene; transaction carries deltas | intern/index.html; render_api Transaction |
| WebKit | ScrollingStateTree committed to ScrollingTree under lock | `commitTreeState` under `m_treeLock` | ScrollingTree.h |
| Gecko APZ | Scroll offsets, displayport; main thread holds display list | Repaint request with reconciled scroll state | AsyncPanZoom.html |
| Stylo | DOM nodes, style structs (shared via pointers), rule tree | rayon work units with Rust type rules | Stylo article |

---

## 5. Scheduling and latency

### 5.1 Chromium cc scheduler: BeginFrame and deadline

Source: https://chromium.googlesource.com/chromium/src/+/main/cc/scheduler/scheduler.h (current header). Implementation in scheduler.cc and scheduler_state_machine.h was not read.

| mechanism | where | what it does | what a tile-damage framework could take from it |
|---|---|---|---|
| Deadline task per BeginFrame | scheduler.h (`ScheduleBeginImplFrameDeadline()`, `begin_impl_frame_deadline_timer_`, "the deadline or drawing phase of the scheduler") | Deadline is posted only while processing a BeginFrame (`INSIDE_BEGIN_FRAME`); scheduled actions run at the deadline. | One frame deadline for all scheduled work; work that misses it is deferred to the next frame. |
| Deadline modes (`BeginImplFrameDeadlineMode`) | scheduler.h (`deadline_mode_` default `NONE`; `SetIsScrolling()`: "we will prioritize BeginImplFrameDeadlineMode::SCROLL") | Modes include SCROLL, IMMEDIATE, REGULAR, LATE; scroll takes priority over other modes. Definitions in scheduler_state_machine.h `[unconfirmed]`. | Scrolling changes the deadline policy, not just the work. |
| Scheduled actions | scheduler.h (`ScheduledActionCommit`, `ScheduledActionActivateSyncTree`, `ScheduledActionDrawIfPossible`, `ScheduledActionPrepareTiles`, `ScheduledActionSendBeginMainFrame`) | Actions are sequenced by the state machine: commit, activate, draw, prepare tiles. | A sequenced action list per frame, not ad-hoc callbacks. |
| Frame skip reasons | scheduler.h (`FrameSkippedReason`: `kRecoverLatency`, `kNoDamage`, `kWaitingOnMain`, `kDrawThrottled`) | Every skipped frame has a reason. | Count skips by reason. |
| Input backs off throttling | scheduler.h (`NotifyInputEvent()`): "may want to back off of throttling" | Input reduces throttling of main-thread frames. | Input events relax the throttle for the next frame. |
| Fudge factor and deadline adjustment | Older source in search results, e.g. https://chromium.googlesource.com/chromium/src/+/refs/tags/51.0.2681.1/cc/scheduler/scheduler.cc `[unconfirmed]` (2016) | A fudge factor is subtracted from the deadline for message latency. Not confirmed in the current source. | Leave headroom for dispatch; measure it. |
| Viz display scheduler deadline | https://chromium.googlesource.com/chromium.src.git/+/49cea54a/components/viz/service/display/display_scheduler.cc `[unconfirmed]` search snippet | Display side has its own deadline; a new BeginFrame forces the previous deadline first. | Each stage has its own deadline; a late stage does not block an early one. |

### 5.2 Input: coalescing, rAF alignment, prediction

| mechanism | where | what it does | what a tile-damage framework could take from it |
|---|---|---|---|
| Coalesced events (`getCoalescedEvents`) | https://www.w3.org/TR/pointerevents3/ §4.1, §4.2.4 | The pointermove event carries the list of coalesced events accumulated since the last dispatch. | Coalesce on the input thread; keep the raw samples for the app. |
| Predicted events (`getPredictedEvents`) | pointerevents3 §4.1 (definition in section 10.2, not read) `[unconfirmed]` that predicted events are estimates | Returns predicted points; definitions in §10 were truncated. | Predicted points are for drawing a lead, not for commit. `[unconfirmed]` wording from the spec. |
| rAF-aligned touch events | https://groups.google.com/a/chromium.org/g/blink-dev/c/axrIk4FPw7A/m/8X9WVwO5BAAJ | Touch input is delivered between the compositor starting BeginMainFrame and Blink, so rAF callbacks run with input closer to frame time; the author names only touchmove. On Windows (no vsync-aligned touch) event frequency drops. Android Finch: about 2.9% statistical improvement at the 75th percentile. | Deliver continuous input just before the frame's update, not on arrival. The measurement is in the thread but may be dated. |
| Continuous vs discrete dispatch | https://docs.google.com/document/d/1aM9AqyYuceRHOmsJZXFKcgi_D4jvFAymYvI36OEwIpI/export?format=pdf `[unconfirmed]` (search snippet for a 2016 design doc) | Continuous events (mousemove, wheel, touchmove) are queued and delivered before rAF; discrete events dispatch immediately. | Two input classes with different latency policy. |
| `desynchronized` canvas hint | https://developer.chrome.com/blog/desynchronized `[unconfirmed]` full text (search snippet) | A hint only; browsers may ignore it; attributes are locked after the first `getContext` call; no DOM element may sit above a translucent desynchronized canvas. | A direct-to-scanout path is an opt-in with constraints; the app can check whether it got it. |
| Intent to ship renamed lowLatency to desynchronized | Blink-dev intent https://groups.google.com/a/chromium.org/g/blink-dev/c/nxjWgMIeC1Q/m/LyC1sle2BQAJ `[unconfirmed]` search snippet | Rename during spec review. | Not relevant beyond naming. |
| Input-to-photon measurement in Chromium | cc/metrics `EventMetrics` https://sources.debian.org/src/chromium/139.0.7258.127-2/cc/metrics/event_metrics.h `[unconfirmed]` mirror (Debian) | `EventMetrics` carries per-event trace ids; `CompositorFrameReporter` derives the EventLatency metrics. | Stamp each input with an id and carry it through the frame; the latency is then a per-frame join. |
| EventLatency breakdowns go to UKM, top-level to UMA | https://chromium.googlesource.com/chromium/src/+/09022a43b4c95ba0cf0bf6fbecabe3837c1925b7 | Commit "Remove EventLatency breakdowns from UMA": only top-level metrics go to UMA; breakdowns go to UKM. | Keep breakdowns in a higher-cardinality store, top-level in aggregate. |
| INP thresholds and phases | https://web.dev/articles/optimize-inp.md.txt `[unconfirmed]` (search snippet) | INP good is 200 ms or less, needs improvement 200-500 ms, poor above 500 ms; phases are input delay, processing time, presentation delay. | Use the same three phase breakdown for your own input latency. |
| Input and presentation delay share | third-party guide claim `[unconfirmed]` | One guide says input and presentation delay often exceed 60% of INP. Not verified against a primary source. | Do not rely on this figure. |

### 5.3 Rendering-update coupling for observers and rAF

| mechanism | where | what it does | what a tile-damage framework could take from it |
|---|---|---|---|
| IntersectionObserver runs in the rendering step | intersection-observer spec §3.4.1 | Observation computation is a substep of "Update the rendering". | Compute visibility in the frame pass. |
| rAF and observers in sync with rendering | WebKit IntersectionObserver notes https://trac.webkit.org/wiki/IntersectionObserverNotes `[unconfirmed]` search snippet | Blink and Gecko schedule rAF in sync with "updating the rendering". | Callbacks fire at the frame boundary; keep one clock. |
| Event-latency coupling to frames | cc/metrics `CompositorFrameReporter` `[unconfirmed]` (search snippet) | Event metrics are attached to the frame that presents them. | Attach input ids to the frame. |

### 5.4 Typical numbers

| number | source | status |
|---|---|---|
| 60 Hz frame budget: 16.7 ms | arithmetic | `[computed]` |
| INP good ≤ 200 ms, poor > 500 ms | web.dev snippet (see 5.2) | `[unconfirmed]` (search snippet) |
| rAF-aligned touch: ~2.9% p75 improvement (Android Finch) | blink-dev thread (see 5.2) | reported by author, `[unconfirmed]` |
| GPU skewport target 0.2 s | layer_tree_settings.h | confirmed |
| Chromium EventLatency numeric budgets | not found in primary sources | `[unconfirmed]`: not found |

---

## 6. Memory

### 6.1 Chromium tile memory: budgets, eviction, bins

| mechanism | where | what it does | what a tile-damage framework could take from it |
|---|---|---|---|
| Priority bins NOW, SOON, EVENTUALLY | https://chromium.googlesource.com/chromium/src/+/main/cc/tiles/tile_priority.h | `enum PriorityBin { NOW, SOON, EVENTUALLY }`; ordering is bin first, then `distance_to_visible`; lower bin value is higher priority. | Three bins plus a float distance is enough; sort key = (bin, distance). |
| Default priority | tile_priority.h | Default is `EVENTUALLY` with infinite distance. | Unknown tiles are lowest priority. |
| Resolution in priority | tile_priority.h (`HIGH_RESOLUTION = 1`, `NON_IDEAL_RESOLUTION = 2`) | Priority carries whether the tile is ideal resolution. | Keep resolution in the priority key. |
| Bin assignment | https://chromium.googlesource.com/chromium/src/+/main/cc/tiles/picture_layer_tiling.cc (`ComputePriorityForTile`) | Visible and pending-visible → NOW with distance 0; skewport and soon-border raised to at least SOON; eventually-rect → EVENTUALLY; occluded visible → SOON; no valid priorities → EVENTUALLY. | Five regions map to three bins; occlusion demotes. |
| `distance_to_visible` | picture_layer_tiling.cc | `current_content_to_screen_scale_ * current_visible_rect_.ManhattanInternalDistance(tile_bounds)`. | Manhattan distance in screen pixels, scaled by content-to-screen scale. |
| Priority rect classification order | picture_layer_tiling.cc (`ComputePriorityRectTypeForTile`) | Visible, pending visible, skewport, soon border, eventually (with DCHECK). | Classify tiles by the first rect that contains them. |
| Eventually rect evicts | picture_layer_tiling.cc (`SetPriorityRect(eventually_rect, EVENTUALLY_RECT, /*evicts_tiles=*/true)`) | Only the eventually rect can evict tiles. | Only the outermost band is eligible for eviction. |
| Memory policy levels | https://chromium.googlesource.com/chromium/src/+/main/cc/tiles/tile_priority.h (`TileMemoryLimitPolicy`) | `ALLOW_NOTHING` (not visible), `ALLOW_ABSOLUTE_MINIMUM` (visible but not interacted), `ALLOW_PREPAINT_ONLY` (interacted, low memory), `ALLOW_ANYTHING`. Ordered; `IsTileMemoryLimitPolicyMoreRestictive`. | A small set of ordered global memory policies, selected by state. |
| Global state struct | tile_priority.h (`GlobalStateThatImpactsTilePriority`: `memory_limit_policy`, `soft_memory_limit_in_bytes`, `hard_memory_limit_in_bytes`, `num_resources_limit`, `tree_priority`, `viewport_size`) | All inputs to priority in one value, passed to `PrepareTiles`. | Pass one state struct to prioritisation; makes priority recomputation a pure function. |
| Tree priority | tile_priority.h (`TreePriority`: `SAME_PRIORITY_FOR_BOTH_TREES`, `SMOOTHNESS_TAKES_PRIORITY`, `NEW_CONTENT_TAKES_PRIORITY`) | Chooses whether the active tree's smoothness or the pending tree's new content wins for budget. | Explicit policy: smoothness vs freshness. |
| Eviction queues | https://chromium.googlesource.com/chromium/src/+/main/cc/tiles/tile_manager.h (`EvictionTilePriorityQueue`, `FreeTileResourcesUntilUsageIsWithinLimit`, `FreeTileResourcesWithLowerPriorityUntilUsageIsWithinLimit`) | Eviction drains a priority queue until usage fits the limit; the second variant only frees tiles with lower priority than a new one. | Evict by priority; a new tile may only evict strictly lower priority tiles. |
| Required tile freed → redraw | tile_manager.h (`freed_required_for_draw_tile` out-parameter) | If a draw-required tile was freed, the caller is told to redraw. | Eviction must notify the renderer about required content. |
| OOM marking | tile_manager.h (`MarkTilesOutOfMemory`) | Tiles that could not be prepared are marked. | OOM is a state, not an exception. |
| Idle reclaim delay | tile_manager.h (`kDelayBeforeTimeReclaim = base::Minutes(5)`) | Memory is trimmed after 5 minutes idle. | Two tiers: immediate eviction under pressure; time-based trim when idle. |
| Memory accounting | tile_manager.h (`MemoryUsage` with `memory_bytes_`, `resource_count_`, `FromTile`, `Exceeds`) | Usage is counted in bytes and resource count together. | Count bytes and tile count; limit on both. |
| Settings defaults | https://chromium.googlesource.com/chromium/src/+/main/cc/trees/layer_tree_settings.h | `max_memory_for_prepaint_percentage = 100`; `max_staging_buffer_usage_in_bytes = 32 * 1024 * 1024`; `max_render_buffer_bounds_for_sw = 16 * 1024`; `decoded_image_working_set_budget_bytes` from `ImageDecodeCacheUtils`. | Concrete defaults to start from. |

### 6.2 Chromium: bytes per tile

| tile | format | bytes | status |
|---|---|---|---|
| 256 x 256 software raster | RGBA8 | 262,144 (256 KiB) | `[computed]` from tile size in how_cc_works.md |
| viewport width × viewport/4 height (GPU raster), 1080 x 1920 viewport example | RGBA8 | 1080 × 480 × 4 = 2,073,600 (about 2 MB) | `[computed]` from how_cc_works.md tile-size rule |
| WebRender default 1024 x 512 | RGBA8 | 2,097,152 (2 MiB) | `[computed]` from tile_cache/mod.rs constant |
| WebRender scrollbar 1024 x 32 | RGBA8 | 131,072 (128 KiB) | `[computed]` from tile_cache/mod.rs constant |
| 32 x 32 tile (user framework) | RGBA8 | 4,096 (4 KiB) | `[computed]` |

Bytes per tile in these engines depend on the backend format; the values above assume RGBA8. Real GPU memory may pad or use different formats. The bin and eviction behaviour is what matters.

### 6.3 WebRender memory and decode

| mechanism | where | what it does | what a tile-damage framework could take from it |
|---|---|---|---|
| Tile cache frees on size change | tile_cache/mod.rs (`pre_update`: all tiles dropped on tile size change; re-evaluated every 120 frames) | Changing tile size drops the whole cache. | Tile size change is a full reset; do not migrate. |
| Native surfaces per opacity | tile_cache/mod.rs (opaque and alpha native surfaces per sub-slice) | Separate surfaces by opacity. | Opaque and alpha surfaces have different memory and blending; separate budgets. |
| Freed when occluded | picture.rs (`prepare_tiled_picture_surface`): occluded native surface freed, "will need to be re-allocated if it becomes visible" | Occluded tiles release their surfaces. | Occlusion is a memory signal. |
| Interner GC by epoch | intern/index.html | Items not used in the current epoch are removed. | Epoch-based GC on the record side. |

### 6.4 WebKit

| mechanism | where | what it does | what a tile-damage framework could take from it |
|---|---|---|---|
| Retained tile backing store metric | TiledBacking.h (`retainedTileBackingStoreMemory()`) | Reports retained memory of a tiled layer; exact policy not in the header. | Report retained memory per surface for budgeting. |
| Tile coverage shrink on idle | `[unconfirmed]` WebKit coverage release policy not read in source | Not verified. | Do not assume. |

### 6.5 Gecko

| mechanism | where | what it does | what a tile-damage framework could take from it |
|---|---|---|---|
| Picture cache tiles with occlusion-based splitting | Bugzilla 1696905 `[unconfirmed]` (search snippet) | Compositor tiles are split into visible parts. | Split tiles along occluder edges, not by fixed grid only. |

---

## 7. Gaps and what was not confirmed from a primary source

- Chromium `damage_tracker.cc`, `occlusion_tracker.cc`, `picture_layer_impl.cc`, `scheduler_state_machine.h`, `tile.h`, `tile_manager.cc`: the headers were read but not the implementations. Mechanisms that depend on them (exact damage union rules, quad-skipping, activation gating, deadline mode definitions, low-res tiling trigger) are marked `[unconfirmed]`.
- Chromium solid-colour: the consequence for a tile classified as solid (quad drawn vs raster skipped) is not confirmed.
- Chromium EventLatency: no numeric budgets found in primary sources. The `event_metrics.h` file was reached through a Debian mirror.
- Chromium display locking: the explainer files (`content-visibility.md`, `update-rendering.md`) were not fetched. The explainer link on the repo landing page is the only source.
- Chromium `content-visibility` implementation (Blink LocalFrameView and display_lock_context.cc) was not read; the snippet for display_lock_context.cc was a crrev mirror, not fetched.
- WebRender `scene_builder_thread.rs`, `frame_builder.rs`, `quadtree.rs`, `picture.rs` (second half) were not read; claims about them are from search snippets or the first part of the file.
- WebRender sub-pixel and tiny-size skipping: no primary source found.
- WebKit tiling release policy and `will-change` in WebKit: not read.
- Text rendering at tiny sizes: not researched in primary sources.
- Servo parallel layout: the Servo book, layout report and style `parallel.rs` were reached through search snippets, not fetched in full.
- `pointerevents3` §10.2 (predicted events definition, "estimates" wording): not reached in the fetched copy.
- rAF-aligned input Chromium design doc: only the blink-dev thread was read.
- The Gecko APZ page is marked as work in progress by its authors.
- Scheduler sources in search results are 2013 to 2016 revisions; they are labelled as older.

---

## 8. Ten ideas most worth stealing, ranked

Each idea gives the precise mechanism and its source. The ranking is by how much the idea would generalise to a tile-damage framework where virtualisation, occlusion and LOD should fall out of damage tracking.

1. **Per-tile dependency lists compared frame-to-frame, with typed outcomes.** Each tile records the set of inputs that produced its pixels (primitive, clip, image key, opacity binding, transform). The lists are rebuilt each frame and compared; the comparison returns a u8 enum (`Equal`, `Descriptor`, `Clip`, `Image`, `OpacityBinding`, `ColorBinding`), and only the tile whose input changed is invalidated. Source: https://doc.servo.org/src/webrender/invalidation/mod.rs.html (lines 112-131) and https://doc.servo.org/src/webrender/picture.rs.html (module doc, "## Invalidation").

2. **Stable-ID chunk matching with property-specific invalidation.** Records are grouped into chunks that carry transform, clip, effect and scroll node references. A chunk is matched to the previous frame by ID; a reorder invalidates old and new bounds; a clip-only change invalidates only the clip difference. Source: https://chromium.googlesource.com/chromium/src/+/main/third_party/blink/renderer/platform/graphics/paint/README.md (RasterInvalidator section).

3. **Damage split into raster damage and expose damage, with a per-surface rect history.** Invalidation damage (re-raster) and expose damage (layer added, removed or reordered) are distinct and forwarded together. The DamageTracker keeps per-layer rect history keyed by a resource identity so that a reused buffer's damage is correct. Source: https://chromium.googlesource.com/chromium/src/+/main/docs/how_cc_works.md (Damage section) and https://chromium.googlesource.com/chromium/src/+/main/cc/trees/damage_tracker.h.

4. **Front-to-back occlusion with append-only opaque and alpha lists.** Insert front to back; each rect is cut by existing opaque rects into at most four pieces per occluder; the opaque list is drawn unsorted and first; occluded visible tiles are demoted from NOW to SOON and their memory is reclaimed. Source: https://doc.servo.org/src/webrender/rectangle_occlusion.rs.html and https://chromium.googlesource.com/chromium/src/+/main/cc/tiles/picture_layer_tiling.cc (`ComputePriorityForTile`).

5. **Priority rings collapsed to three bins with a Manhattan distance tiebreak.** Visible and pending-visible tiles are NOW; skewport and soon-border tiles are at least SOON; the eventually band is EVENTUALLY and is the only band that evicts; `distance_to_visible = content_to_screen_scale * ManhattanInternalDistance(visible, tile)`; a tile is pre-rastered only if it is within half the skewport extent. Source: https://chromium.googlesource.com/chromium/src/+/main/cc/tiles/picture_layer_tiling.cc and https://chromium.googlesource.com/chromium/src/+/main/cc/tiles/tile_priority.h.

6. **Coverage window separate from visible rect, grown along motion, with a margin band and tile-removal callbacks.** Visible rect sets paint priority; coverage rect sets what is backed; coverage expands by scroll axis; margins are pre-allocated; the cache is told when a tile is removed. WebRender adds a one-tile vertical inflation for hysteresis. Source: https://github.com/WebKit/WebKit/blob/main/Source/WebCore/platform/graphics/TiledBacking.h and https://doc.servo.org/src/webrender/tile_cache/mod.rs.html (`TileCacheInstance::pre_update`).

7. **Ideal scale vs raster scale, with a low-res pyramid during gesture.** The target scale comes from transforms; the raster scale is held for performance; tilings at several scales coexist and only HIGH_RESOLUTION ones draw; a directly composited image keeps a default raster scale to avoid re-raster on every scale change. Source: https://chromium.googlesource.com/chromium/src/+/main/cc/layers/picture_layer_impl.h and https://chromium.googlesource.com/chromium/src/+/main/cc/tiles/picture_layer_tiling.h.

8. **Bounded solid-colour proof from the op list.** Run the tile's recorded ops against a no-draw canvas clipped to the tile; reject any image, text, path or save-layer op immediately; accept only a rect or rrect fill with src or src-over that covers the whole clip; cap the op count (default 1 at the raster source). Source: https://chromium.googlesource.com/chromium/src/+/main/cc/paint/solid_color_analyzer.cc and https://chromium.googlesource.com/chromium/src/+/main/cc/raster/raster_source.h (`PerformSolidColorAnalysis`).

9. **Subtree relevance flag with remembered size as placeholder.** A subtree is relevant if it is on-screen (intersecting the viewport with clips applied), focused, selected or top-layer; a skipped subtree gets size containment; `contain-intrinsic-size: auto` reuses the last rendered size so scroll height is stable; find-in-page and focus still reach it. Source: https://www.w3.org/TR/css-contain-2/ (§3.1, §4, §4.2) and https://www.w3.org/TR/css-sizing-4/#last-remembered.

10. **Commit as an atomic snapshot with a pending tree gated on required tiles.** The main thread blocks briefly to copy state to the impl side; the copy goes to a pending tree; activation happens only when the set of required tiles is rastered; the previous pending tree is kept as a recycle cache. Source: https://chromium.googlesource.com/chromium/src/+/main/docs/how_cc_works.md ("Commit Flow", "Trees: commit / activation") and https://chromium.googlesource.com/chromium/src/+/main/cc/tiles/tile_manager.h (`NotifyReadyToActivate`).

Honourable mention (not in the top ten): WebRender's interning with epoch-based GC across a scene thread and a frame thread (https://doc.servo.org/webrender/intern/index.html).

---

## 9. Mapping to the user's framework (32 x 32 tiles, B+tree record tree)

Non-normative notes that follow from the sources above. Each item cites the section that supports it.

- Keep the B+tree page id as the stable id for a record (idea 2, RasterInvalidator). A page split or merge is a reorder and must invalidate both old and new bounds.
- For each 32 x 32 tile, store the dependency list of record ids that produced its pixels (idea 1). The damage rect is the union of changed dependencies, not the union of changed records. This is the WebRender model at 32 px granularity.
- Keep one union damage rect per frame for the GPU scissor and a separate expose-damage list for reorders and structural changes (idea 3).
- Walk the page tree front to back for occlusion. Use an append-only opaque list. Demote occluded tiles from the visible bin to SOON and free their memory when a reclaim is due (idea 4).
- Derive virtualisation from priority rings: visible and pending-visible NOW; a skewport band SOON; the outer band EVENTUALLY and the only band eligible for eviction (idea 5). Virtualisation then means "evict EVENTUALLY tiles and keep the record", not "unmount".
- Use the remembered-size rule from CSS for pages of records that are skipped: store each page's measured height on first render and reuse it as the placeholder (idea 9).
- Level of detail: keep an ideal scale and a raster scale per surface; during zoom keep a low-resolution tile grid as fallback (idea 7). Solid tiles from a bounded op-list check (idea 8) are cheap to add if the record ops are recorded as a list.
- Scroll must not invalidate tiles: scroll changes a single offset node that tile positions read; tiles are re-selected, not re-rastered (section 1.7).
- Threading: snapshot at a defined boundary, double-buffer the tree, and gate activation on required tiles (idea 10). Interning across threads (section 1.5) is a candidate if record payloads are large.

---

## 10. Source index (primary unless marked)

- Chromium `docs/how_cc_works.md`: https://chromium.googlesource.com/chromium/src/+/main/docs/how_cc_works.md
- Chromium `cc/trees/damage_tracker.h`: https://chromium.googlesource.com/chromium/src/+/main/cc/trees/damage_tracker.h
- Chromium `cc/trees/occlusion_tracker.h`: https://chromium.googlesource.com/chromium/src/+/main/cc/trees/occlusion_tracker.h
- Chromium `cc/tiles/tile_priority.h`: https://chromium.googlesource.com/chromium/src/+/main/cc/tiles/tile_priority.h
- Chromium `cc/tiles/tile_manager.h`: https://chromium.googlesource.com/chromium/src/+/main/cc/tiles/tile_manager.h
- Chromium `cc/tiles/picture_layer_tiling.h`: https://chromium.googlesource.com/chromium/src/+/main/cc/tiles/picture_layer_tiling.h
- Chromium `cc/tiles/picture_layer_tiling.cc`: https://chromium.googlesource.com/chromium/src/+/main/cc/tiles/picture_layer_tiling.cc
- Chromium `cc/layers/picture_layer_impl.h`: https://chromium.googlesource.com/chromium/src/+/main/cc/layers/picture_layer_impl.h
- Chromium `cc/paint/solid_color_analyzer.cc`: https://chromium.googlesource.com/chromium/src/+/main/cc/paint/solid_color_analyzer.cc
- Chromium `cc/raster/raster_source.h`: https://chromium.googlesource.com/chromium/src/+/main/cc/raster/raster_source.h
- Chromium `cc/tiles/image_decode_cache.h`: https://chromium.googlesource.com/chromium/src/+/main/cc/tiles/image_decode_cache.h
- Chromium `cc/scheduler/scheduler.h`: https://chromium.googlesource.com/chromium/src/+/main/cc/scheduler/scheduler.h
- Chromium `cc/trees/layer_tree_settings.h`: https://chromium.googlesource.com/chromium/src/+/main/cc/trees/layer_tree_settings.h
- Blink PaintChunks README: https://chromium.googlesource.com/chromium/src/+/main/third_party/blink/renderer/platform/graphics/paint/README.md
- LayoutNG: https://developer.chrome.com/blog/layoutng
- WebKit `TiledBacking.h`: https://github.com/WebKit/WebKit/blob/main/Source/WebCore/platform/graphics/TiledBacking.h
- WebKit `ScrollingTree.h`: https://github.com/WebKit/WebKit/blob/main/Source/WebCore/page/scrolling/ScrollingTree.h
- WebRender `tile_cache/mod.rs`: https://doc.servo.org/src/webrender/tile_cache/mod.rs.html
- WebRender `invalidation/mod.rs`: https://doc.servo.org/src/webrender/invalidation/mod.rs.html
- WebRender `picture.rs`: https://doc.servo.org/src/webrender/picture.rs.html
- WebRender `rectangle_occlusion.rs`: https://doc.servo.org/src/webrender/rectangle_occlusion.rs.html
- WebRender interning: https://doc.servo.org/webrender/intern/index.html
- Firefox APZ: https://firefox-source-docs.mozilla.org/gfx/AsyncPanZoom.html
- Stylo article: https://hacks.mozilla.org/2017/08/inside-a-super-fast-css-engine-quantum-css-aka-stylo/
- CSS Containment 2: https://www.w3.org/TR/css-contain-2/
- CSS Sizing 4 (last remembered size): https://www.w3.org/TR/css-sizing-4/#last-remembered
- IntersectionObserver: https://www.w3.org/TR/intersection-observer/
- Pointer Events 3: https://www.w3.org/TR/pointerevents3/
- WICG display locking repo (landing page only): https://github.com/WICG/display-locking
- rAF-aligned touch (Blink-dev): https://groups.google.com/a/chromium.org/g/blink-dev/c/axrIk4FPw7A/m/8X9WVwO5BAAJ
- Desynchronized canvas: https://developer.chrome.com/blog/desynchronized
- EventLatency breakdown UMA change: https://chromium.googlesource.com/chromium/src/+/09022a43b4c95ba0cf0bf6fbecabe3837c1925b7
