# 05. Startup and memory survey for bough on macOS

Scope: eight topics from the brief. Targets are cold start (time to first pixel, now ~40 ms, of which `MTLCreateSystemDefaultDevice` is ~22-29 ms) and steady memory (~110-130 MB `phys_footprint`, ~100 MB floor after first GPU draw).

Confidence key used throughout:
- **Verified**: read on a primary source page that was fetched in this session.
- **Snippet**: seen only in a search-result excerpt; not opened in full.
- **Unverified**: a claim from a forum post, blog, or inference that I could not confirm. Treat as a hypothesis to measure.

Source access note: several Apple documentation pages (`compressionType`, `IOSurfaceSetPurgeable`, "Reducing your app's launch time") returned only their titles to the fetch tool, so their content here is Snippet or Unverified. The Chromium `remote_layer_api.mm` mirror returned 404 and the Emacs manual returned 429, so those statements are Snippet-level too.

---

## Summary of findings

- Lossless compression on Apple GPUs is applied automatically by Metal under documented conditions (Verified). Whether an IOSurface-backed texture can be allocated compressed, and whether WindowServer composites it, is not established by any source I found.
- `IOSurfaceSetPurgeable` with volatile/non-volatile/empty states is a documented API (Snippet). How purgeable IOSurface bytes are charged to `phys_footprint` is not documented; the only account is a forum post (Unverified).
- The ~100 MB first-draw footprint has no published breakdown. The concrete leads are residency-set memory behavior (Verified forum report, with figures for a different allocation size) and `MTLCompilerService` memory (anecdotal). Neither is proven to explain bough's 100 MB.
- Binary archives (`MTLBinaryArchive`) are the best-documented startup lever here. Apple's WWDC20 session reports a vendor-measured pipeline build of 1 min 26 s falling to 3 s (Verified via session summary; one configuration).
- The strongest numbers for state snapshots come from V8: context creation 40 ms to under 2 ms (Verified). Emacs and CRIU numbers are vendor- or mailing-list-sourced and dated.
- No source gives a measured win for "show a cached frame before the GPU exists." That idea is reasoned, not measured, and must be prototyped.

---

## 1. Lossless framebuffer compression on Apple GPUs

**What it is.** Apple GPUs (apple5 family and later) apply lossless delta-style compression to textures to cut bandwidth. The compression metadata slightly increases footprint.

**Who uses it, where.**
- Apple: `optimizeContentsForGPUAccess(texture:)` and `optimizeContentsForCPUAccess(texture:)` on `MTLBlitCommandEncoder`, `allowGPUOptimizedContents` on `MTLTextureDescriptor`. (Verified: https://developer.apple.com/documentation/metal/optimizing-texture-data.md)
- Dawn (Chromium's WebGPU): commit "GetFormatEquivalentToIOSurfaceFormat: Add compressed formats" maps Apple's lossless CoreVideo pixel formats to Metal texture formats. (Snippet: https://dawn.googlesource.com/dawn/+/a442eddfefd353ece2397228ffd60cfce28ed37c%5E%21/)
- Impeller (Flutter) `texture_descriptor.h` carries a compression-type field and notes lossy works only on iOS 15+ on A15 (Snippet: https://flutter.googlesource.com/mirrors/engine/+/HEAD/impeller/core/texture_descriptor.h). The A15 limitation is Snippet-level only.

**Verified details (Apple, "Optimizing texture data").**
- Metal applies lossless compression on apple5+ if all hold: pixel format is not block-compressed (PVRTC, ASTC, BC); usage does not include `unknown`, `shaderWrite`, or `pixelFormatView`; and the texture is not backed by an `MTLBuffer` (`makeTexture(descriptor:offset:bytesPerRow:)`).
- Explicit opt-in for shared-storage textures: write with `replace(region:...)` and then run `optimizeContentsForGPUAccess`.
- Apple states the footprint "might increase slightly because it needs to store compression metadata." No numbers are given.
- Metal "may not be able to optimize some textures for specific hardware and ignores optimization API calls for those textures."

**Measured numbers.** None published in any source I found.

**Open questions (Unverified).**
- `MTLTextureCompressionType` (`lossless` / `lossy`) on `MTLTextureDescriptor.compressionType`: the page returned only its title. The `lossy` subpage exists on developer.apple.com (Snippet).
- Whether an IOSurface-created texture (`makeTexture(descriptor:iosurface:plane:)`) gets this compression at all. No source addressed it. The Metal texture `iosurface` accessor exists (Snippet).
- The IOSurface-level compression key I searched for, `kIOSurfaceCompressionMethod`, appears in no source I could find. Do not assume it exists.
- Whether WindowServer composites a compressed IOSurface without decompressing it. Unknown.

**Caveats.** The apple5 family and the IOSurface behavior are the two gating facts. The usage-flag rules (no `shaderWrite`) exclude compute-written targets, which bough's compute renderer probably uses.

**How it applies to bough.** Worth a bounded experiment: create a `renderTarget`-usage `MTLTexture` with `compressionType = .lossless` (if it exists in the SDK), do not use `shaderWrite`, and compare `phys_footprint` and frame time against an uncompressed baseline. Expected memory win: unknown, likely small (a few percent of surface bytes); expected bandwidth win: unknown. The bigger win here is only plausible if the compositor path can hand compressed surfaces to WindowServer, which is unverified.

---

## 2. IOSurface purgeability and memory accounting

**What it is.** `IOSurfaceSetPurgeable` moves an IOSurface between non-volatile, volatile, and empty states. Volatile surfaces may be discarded by the kernel under pressure. (Snippet: the WebCore SPI header https://opensource.apple.com/source/WebCore/WebCore-7603.2.4/platform/spi/cocoa/IOSurfaceSPI.h.auto.html declares the states and `IOSurfaceSetPurgeable(buffer, newState, oldState)`.)

**Documentation.** The Apple page for `IOSurfaceSetPurgeable(_:_:_:)` returned only its title (Unverified content). The CoreVideo key `kCVPixelBufferIOSurfacePurgeableKey` is documented for CVPixelBuffers backed by IOSurfaces (Snippet: https://developer.apple.com/tutorials/data/documentation/corevideo/kcvpixelbufferiosurfacepurgeablekey.md). Apple's guidance, as quoted in a header-doc excerpt (Snippet): keep surfaces non-volatile while in use and mark them volatile when their contents are optional or speculative.

**Accounting.**
- `phys_footprint` components listed on the Apple Developer Forums (Unverified, user-reported): internal, compressed internal, `iokit_mapped`, `purgeable_nonvolatile`, and page tables (https://developer.apple.com/forums/thread/111551).
- The same poster reported that memory mapped through IOKit's `pmap_enter_options` path did not appear in `phys_footprint`. In their test, about 1000 OpenGL textures showed ~24 MB of footprint against ~687 MB internal-plus-compressed. Unverified; it is one user's measurement on an older OS.
- Stack Overflow thread (Snippet): after purging volatile IOSurfaces under simulated pressure, Instruments still showed the memory allocated, and purged surfaces stayed usable with pixel data lost. The advice was to set them non-volatile again and check the old state to know whether to regenerate.
- XNU `osfmk/kperf/meminfo.c` samples `phys_footprint` and the `purgeable_volatile` ledger (Snippet, older XNU; https://git.saurik.com/apple/xnu.git/blob/5ba3f43ea354af8ad55bea84372a2bc834d8757c:/osfmk/kperf/meminfo.c).

**Does WindowServer's mapping count against the app?** Not established. The forum report suggests IOKit-mapped surfaces are not fully charged to the app's `phys_footprint`, which would mean a footprint measurement could understate true GPU memory. Verify with `footprint` and `vmmap` on the target OS, comparing `phys_footprint` against the IOSurface sizes bough allocates.

**Sizing (arithmetic, not measured).** A 1600x1000 pt window at 2x is 3200x2000 px, 4 bytes per pixel, = 25.6 MB per surface. Double-buffered, that is ~51 MB. This is the upper bound on what volatile marking can reclaim per window.

**Caveats.** Purgeable behavior is OS-version dependent, and the reclaim timing is controlled by the kernel. Apple documents the API, but the accounting details are not in public docs.

**How it applies to bough.** Mark back-buffer IOSurfaces volatile when a window is occluded, minimized, or idle, and non-volatile before drawing. Expected win: up to the full idle-window surface footprint (tens of MB per window by the arithmetic above), but only if the kernel actually reclaims them and `phys_footprint` reflects it. Prototype first; the accounting uncertainty is the main risk.

---

## 3. What the Metal driver allocates on first use

**What we know.**
- Apple's residency-set docs (Snippet, https://developer.apple.com/tutorials/data/documentation/metal/simplifying-gpu-resource-management-with-residency-sets.md): attaching a residency set to a command buffer makes its allocations resident after commit. Making allocations resident "can delay the graphics driver from submitting the command buffer to the GPU," so Apple suggests calling `requestResidency` ahead of time.
- Apple Developer Forums FB23959296 (Verified, fetched: https://developer.apple.com/forums/thread/839089): if a process creates a residency set, calls `requestResidency` and `endResidency`, then releases it without ever running GPU work, the backing memory is not freed. In the repro, a 4.30 GB residency set stayed at 4.30 GB `phys_footprint` five seconds after teardown. Adding one trivial blit (fill a 1-byte buffer and commit) freed it to 0.01 GB. Environment: Xcode 26.6, macOS 26.5.2. Not an Apple statement. The reported origin was llama.cpp. This is about a 4.3 GB allocation, not ~100 MB; the mechanism may be the same, but that is unverified.
- `MTLCompilerService` memory: users report it idling at ~0.5 GB and spiking to ~2 GB on macOS Tahoe 26.0.1, and >100 instances in some cases (Snippet/anecdotal: https://discussions.apple.com/thread/253153618 and https://discussions.apple.com/thread/256164061). Probably a system issue, not something bough's code controls.
- Zed commit "macOS: Enumerate GPUs first; prefer low-power non-removable; fall back to system default (#38164)" (Snippet: https://git.secluded.site/zed/commit/bb514c158e1604916b32fc8c9bbe602a78727742). Its premise: a launching app on a discrete-GPU Mac can pick the discrete device by default, which is slow. Reported by a MetalKit developer as causing a multi-second launch and an unresponsive Mac (Snippet: https://developer.apple.com/forums/thread/692769).
- Asahi Linux notes (Snippet, https://asahilinux.org/docs/hw/soc/agx/): AGX firmware allocates from a heap at its discretion, so per-context firmware memory is firmware-controlled rather than fixed by the driver. The GPU has 63 usable user contexts per one note; another note says 16 and flags it as "double check." Apple's macOS driver is not the Asahi driver; treat as analogy only.
- Apple's WWDC20 10615 (Verified via fetched summary): Metal Binary Archive files are memory-mapped; virtual memory is released only when the archive is released (see topic 7).

**Why first submission adds ~100 MB.** No source explains this. The candidates, each unverified:
1. Residency-set and heap allocation for the first command buffer (the Apple forum case shows this memory is charged and can linger).
2. `MTLCompilerService` or Metal shader-cache memory that is attributed to the process or to its footprint.
3. Shader library and pipeline state objects (PSOs) built at first use.
4. IOSurface or drawable backing for the first frame.

**How to find out.** Use `footprint` and `vmmap --summary` before and after `MTLCreateSystemDefaultDevice`, the first command buffer, and the first commit, and tag regions by their owner. Check `phys_footprint` with and without a residency set. The FB23959296 repro is a ready template.

**How others reduce it.** Zed prefers `MTLCreateSystemDefaultDevice` (Snippet). WebKit fixed a leak of the object that `MTLCreateSystemDefaultDevice` returns (Snippet, https://webkit.org/b/231899). Avoiding discrete GPU selection on dual-GPU Macs avoids a second driver load (Unverified magnitude).

**Caveats.** Everything in this topic is anecdotal or inferred. The 100 MB number is bough's own measurement; nothing published breaks it down.

**How it applies to bough.** Measure the breakdown before optimizing. If residency-set or heap memory is the cause, try creating resources lazily and only attaching residency sets at first draw, and check whether a trivial GPU submission at startup changes the reclaim behavior.

---

## 4. Creating a window without AppKit's launch

**CAContext / CALayerHost (Chromium and Electron).**
- Chromium's `ui/base/cocoa/remote_layer_api.mm` (Snippet, Debian mirror of 139.0.7258.127; the mirror returned 404 on fetch): a feature flag `RemoteCoreAnimationAPI`, enabled by default, checks that the `CAContext` and `CALayerHost` classes exist before enabling the remote-layer path. The GPU process creates a `CAContext` and commits layer-tree updates on swap; the browser process hosts it with a `CALayerHost` keyed by the context ID. (Snippet; source files `gpu/ipc/service/image_transport_surface_overlay_mac.mm`, `content/browser/compositor/browser_compositor_ca_layer_tree_mac.mm`, `content/browser/compositor/gpu_output_surface_mac.mm`.)
- Electron patch `mas_disable_remote_layer.patch` (Snippet, https://ayakael.net/mirrors/electron/blame/commit/90af7d7fe2fc70984404474feec4ac2c827794fe/patches/chromium/mas_disable_remote_layer.patch) compiles the CAContext/CALayerHost path out under `IS_MAS_BUILD()`. So the path is not usable in Mac App Store builds.
- Caveat: `CAContext` and `CALayerHost` are private QuartzCore API. Chromium uses them, and they are not in public headers. Feasibility is proven by Chromium's use, but the risk is Apple-owned and OS-version dependent. Sandboxed and MAS builds are excluded.

**SkyLight / CGS private APIs.**
- The SkyLight framework exposes WindowServer's window and space calls. Symbol names like `CGSNewWindow` and `CGSSetWindowBackingScaleFactor` were not confirmed in any source I found (Unverified).
- Practical technique (Snippet): load the framework and resolve symbols at runtime with `dlopen`/`dlsym`, and treat a missing symbol as "unsupported" rather than crashing. Loop PR #906 (https://github.com/mrkai77/Loop/pull/906) introduces a `SkyLightSymbolLoader` for exactly this reason. The `computer-use` crate's SKYLIGHT.md (https://docs.rs/crate/computer-use/0.2.0/source/docs/SKYLIGHT.md) notes that some symbols exist only on newer macOS.
- Caveat: private API. Sandbox and App Store review rule it out. Symbols change between macOS releases.

**Input.** How events reach a window created without `NSApplication` (CGEvent taps, WindowServer port, or the window's event port) was not researched further. Unverified.

**Savings vs NSApplication.** No measured numbers found. Avoiding AppKit means avoiding loading AppKit and its dependent frameworks. Its size is not measured here.

**How it applies to bough.** Largest structural startup change, but the most fragile. Recommend a spike: measure pre-main and time-to-window with `NSApplication` vs a raw window using `CAMetalLayer` on a view-less `CALayer`. Keep `CAContext` out of MAS builds, and use runtime symbol checks everywhere.

---

## 5. Showing a first frame instantly

**Technique.** Show a pre-rendered image as the layer's `contents` (a `CGImage` backed by an mmapped file) before any GPU device exists. The device is created afterward and takes over. iOS launch screens and snapshots do this at the OS level; the app-level variant is a cached first frame.

**Sources.** Apple's launch-screen guidance is the reference (not fetched; content unverified). Flutter's startup measurement (from a community tool, Snippet: https://pub.dev/packages/flutter_startup_metrics/changelog) counts cold start to first rasterized frame. No source here reports a number for a cached first frame vs a GPU-rendered one.

**Caveat.** The cached frame must not be stale. Keep it per-window-size and per-appearance (light/dark, scale factor), or it shows the wrong content. Gate its use on a checksum or version.

**Expected win.** The device creation (22-29 ms) and first draw are the things a cached frame hides, so the perceived time-to-first-pixel could drop by roughly that amount. This is arithmetic on bough's own numbers, not a measurement.

**How it applies to bough.** The cheapest large win: write a tiny PNG or raw BGRA snapshot of the last frame into a cache file and show it via `CALayer.contents` at launch. No private API, no sandbox issue. Measure against the 40 ms baseline.

---

## 6. Snapshotting initialized application state

**V8 startup snapshots (Verified).** https://v8.dev/blog/custom-startup-snapshots
- Creating a context from a snapshot: 40 ms down to under 2 ms on desktop.
- Average mobile phone: "a difference between 270 ms and 10 ms."
- TypeScript compiler preload: about 100 ms saved at startup on desktop.
- Shipped in V8 v4.3.
- Relocatability: a snapshot is a serialized heap. Deserialization skips building the global object and built-ins. Heap-only: API callbacks (`FunctionTemplate`) and typed arrays with external backing stores cannot be captured. Values like `Math.random` and `Date.now` are frozen at capture time.

**Emacs pdumper and unexec (Snippet only; the GNU manual fetch returned 429).**
- Early comparison (2016 mailing list, Snippet: https://lists.gnu.org/r/emacs-devel/2016-11/msg00714.html): the portable dumper matched unexec's startup "to within 100 ms" and was not yet optimized.
- Later recollection (2018, Snippet: https://lists.gnu.org/r/emacs-devel/2018-08/msg01017.html): a difference of about 1-2 ms on `emacs -Q` startup. Methodology unknown.
- Manual (Snippet): unexec is deprecated and not built by default; `pdumper-stats` reports restore time in seconds.
- Mechanism: pdumper avoids unexec's address-space tricks by writing a relocatable image and fixing pointers at load time. Unverified detail, from background knowledge.

**Android zygote (Snippet, blog sources).** Fork costs around 5-50 ms in one author's systrace captures (anecdotal). Preloading saves roughly 1-3 s per app launch, per a blog with no citation. Preload list size: AOSP around 4,000 classes; more than ~10,000 slows zygote startup and raises baseline memory. Historical preload took 10-12 s on old hardware (mailing list, dated). Not verified against current AOSP.

**CRIU and CRaC.** Open Liberty measured time-to-first-response with and without CRIU for three Java apps; the exact figures were not in the snippet. OpenJ9 states up to 10x faster startup to first response (vendor claim, Snippet: https://nljug.org/?p=8171). Checkpoint size scales with the process's memory allocation (arXiv 2307.12113, Snippet).

**Flutter AOT snapshots (Snippet).** The engine needs four artifacts per isolate: VM snapshot data and instructions, plus isolate snapshot data and instructions. Snapshots are architecture-specific. Startup figures are old (2018: about 328 ms first view on iPhone 6, about 82 ms on iPhone X).

**Smalltalk images.** No sources found in this session. Unverified.

**Relocatability.** A snapshot can be mapped at a fixed address (no relocation) or stored position-independent and fixed up at load. V8 fixes up pointers at deserialization. Emacs pdumper uses load-time relocation. Anything that holds OS handles (file descriptors, GPU objects, Mach ports) cannot be snapshotted.

**Caveats.** bough is Rust. A VM-style heap snapshot does not apply. The applicable analogue is precomputed, zero-copy initial data (for example, a `static` table built at compile time, or an mmapped file of pre-laid-out scene data). The V8 numbers are for JavaScript heaps, which are much larger than what bough initializes.

**How it applies to bough.** Low priority as a general technique. The one relevant piece: move any startup computation whose result is pure data (font tables, glyph atlases, layout tables) into a build-time or mmapped artifact.

---

## 7. Shader binaries without compilation at launch

**MTLBinaryArchive (Verified via WWDC20 10615 summary: https://developer.apple.com/videos/play/wwdc2020/10615/).**
- Workflow with timestamps: create an empty archive by setting `url = nil` (6:19); add render, compute, or tile-render pipelines (6:47); attach via `binaryArchives` on the pipeline descriptor (6:56); `serialize(to:)` (7:15); reopen with `url` (7:26).
- Vendor-reported numbers: Epic Games harvested an archive from 1,700 pipeline state objects (Fortnite has over 11,000 PSOs). On a six-core 3 GHz Mac mini with 32 GB, pipeline build went from 1 min 26 s to 3 s with the archive. Apple reported this as over 28x. One configuration; vendor-reported.
- Apple3 and Mac1 GPU families are listed as supported (from the session summary; check against docs).
- A harvested archive is reliably reused only on the same GPU and OS build. On a mismatch, Metal falls back to runtime compilation.
- Archive files are memory-mapped. Their virtual memory is released only when the archive is released. Opening an archive for lookup only, without adding new pipelines, keeps its pipelines out of active app memory.
- `failOnBinaryArchiveMiss` makes a miss return nil rather than compile, useful for CI.
- Newer API: `MTL4PipelineDataSetSerializer` (macOS 26, Snippet: https://developer.apple.com/documentation/metal/mtl4pipelinedatasetserializer.md). Its relation to `MTLBinaryArchive` is unknown.

**Metal's shader cache.** Apple's Metal Shader Cache (managed by `MTLCompilerService`, per Snippet from the search results) caches compiled code across runs. Before binary archives, it helped only after a previous run had populated it. Cold runs still compile.

**What precompiled metallibs still cost.** A metallib avoids source-to-AIR compilation, but pipeline creation still converts AIR to machine code. Binary archives are what remove that step. Unverified: the magnitude of the remaining per-PSO cost on current hardware.

**Caveats.** Archives must be regenerated per OS build and GPU model. They are an app asset, and Apple recommends populating them at development time, not at runtime.

**How it applies to bough.** Strong candidate. The Apple numbers suggest the win scales with pipeline count, and bough's compute renderer probably has few pipelines, so the absolute saving may be small. Prototype: harvest an archive for bough's pipelines at build time, ship it as a resource, attach at startup, and time `makeComputePipelineState` / `makeRenderPipelineState` with and without it.

---

## 8. macOS launch costs

**dyld closures (Snippet, WWDC17 413 summary: https://wwdcnotes.com/notes/wwdc17/413 and https://nonstrict.eu/wwdcindex/wwdc2017/413/).** Startup time is measured to `main()`. A launch closure holds the dylib list, symbol offsets, and fixups. dyld validates it, maps dylibs, applies fixups, runs initializers, then jumps to `main()`. System app closures are built into the shared cache; third-party closures are built at install time.

**Apple's guidance (Snippet, https://developer.apple.com/documentation/xcode/reducing-your-app-s-launch-time.md).** Each third-party framework adds launch time. MetricKit reports time-to-resume. The article's specific numbers were not returned to the fetch tool; treat any figure from it as unverified.

**Measured pre-main numbers (third-party blogs, old, Unverified).**
- One post on iOS 10 with `DYLD_PRINT_STATISTICS`: ~409 ms total pre-main, ~384 ms in dylib loading. Adding 10 Swift frameworks raised it to ~683 ms (https://cur.at/ATqDXtW?m=web).
- A developer reported roughly halving pre-main time by merging dynamic frameworks into static ones (https://ios-developersblog.blogspot.com/2017/06/how-we-cut-our-ios-apps-launch-time-in.html).
- A 2026 post claims a 30% launch-time cut in iOS 27; I could not verify it.

**Order files and page-ins.** No primary source fetched. Unverified.

**How it applies to bough.** The most likely large item is avoiding AppKit and its dependent frameworks, which ties to topic 4. Measure with `DYLD_PRINT_STATISTICS` (or `DYLD_PRINT_STATISTICS_DETAILS` on newer systems), `dyld` closure output, and `vmmap` page-in counts. Check whether the binary links any dynamic dependencies beyond system frameworks.

---

## Ranked top five for bough

1. **Cached first frame shown from an mmapped CGImage (topic 5).** Hides the 22-29 ms device creation and first draw from perception. No private API, no sandbox issue, small code. Expected win: up to roughly 25 ms of perceived time-to-first-pixel (arithmetic on bough's numbers, not measured). Lowest risk.

2. **Precompiled pipelines with `MTLBinaryArchive` (topic 7).** Apple's session shows large vendor-reported pipeline-build reductions. Expected win: scales with pipeline count, plausibly a few to tens of ms of cold start for bough's pipeline set (unmeasured estimate). Cost: regenerate per OS build and GPU.

3. **Purgeable back-buffer IOSurfaces when a window is idle or occluded (topic 2).** Memory win of up to the full idle-surface footprint: 25.6 MB per 1600x1000 pt window at 2x, double-buffered ~51 MB (arithmetic). Uses a documented API. The accounting uncertainty (whether `phys_footprint` reflects the reclaim) must be verified first.

4. **Avoid AppKit by creating the window through a raw path (topic 4, with topic 8).** Likely the largest structural startup win, because it removes AppKit's load and its dependent frameworks. Expected size unknown; no measured number found. Cost: private APIs (SkyLight, CAContext), blocked for MAS and sandboxed builds, and fragile across macOS releases. Needs runtime symbol checks.

5. **Discover what the ~100 MB first-draw footprint is, then attack it (topic 3).** The most important item by impact, and it ranks fifth only because nothing I found identifies the cause. Candidates: residency-set memory (the Apple forum report shows it can be charged and can linger), `MTLCompilerService`, first-PSO memory, and first-frame IOSurface backing. Expected win: unknown, potentially the bulk of the ~100 MB floor. First step is measurement with `footprint` and `vmmap`, not a code change.

Lossless compression (topic 1) and state snapshots (topic 6) rank below these. Compression has no proven IOSurface path and no published savings; snapshots are a VM technique with little direct application to a Rust binary.

---

## Gaps and next steps

- Apple pages for `compressionType`, `IOSurfaceSetPurgeable`, and "Reducing your app's launch time" returned only titles. Re-fetch them directly or use the developer documentation archive before relying on details.
- `kIOSurfaceCompressionMethod`: not found in any source.
- Chromium `remote_layer_api.mm` and the Gecko and WezTerm/Alacritty/Godot sources were not opened; claims about them rest on snippets or are absent.
- Measurement is needed for: the ~100 MB first-draw breakdown (topic 3), IOSurface purgeable reclaim vs `phys_footprint` (topic 2), and the binary-archive win on bough's real pipeline set (topic 7).
