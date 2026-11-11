# Every avenue, by lane

The exhaustive catalog: every open item from `research/AUDIT.md`, the design (`designs/reactivity/final`), the gap
register's notes, the reference workloads (`designs/reactivity/performance.md` §4) and the failed work worth
retrying. Each line names its lane and its AUDIT ID, where one exists. `docs/LANES.md` gives each lane's first bounded
task and success criterion; this file is the backlog behind them. An item is a direction to card (`docs/CARD.md`),
not a claim.

Status key:

* **open**: nothing built;
* **partial**: something exists with a stated gap;
* **retry**: tried and rejected, but a new representation is named;
* **merged-untested**: on `main`, but the suite has not run on it.

---

## Per-platform plan

| capability | macOS | Windows | Linux | Web | iOS / Android (later) |
|---|---|---|---|---|---|
| window, events, loop | today: AppKit local monitor, Accessory policy (partial). Avenue: winit host, or an `NSView` subclass host for IME and services | winit (Win32) | winit (Wayland, X11) | winit (web) on a canvas | winit / UIKit, Android activity |
| presenter | direct Metal (`bough-metal`), wgpu-hal Metal (`bough-gpu`, partial) | wgpu-hal DX12 (and Vulkan) | wgpu-hal Vulkan; GL fallback | wgpu WebGPU (WebGL2 fallback?) | Metal / Vulkan |
| shaders | MSL at load today; wgsl-rs → MSL metallib (bough-gpu) | wgsl-rs → naga → HLSL/DXIL | wgsl-rs → naga → SPIR-V | wgsl-rs → WGSL | as above |
| region memory | `storageModeShared` buffer wrapping the region (zero copy) | upload heap / `ReBAR` where available; else staged writes | host-visible coherent memory; else staged | `writeBuffer` per frame (measure the copy) | shared/unified memory |
| surfaces & presentation | IOSurface ring, CALayer contents swap; tile-flip (RT1) | DXGI flip-model swap chain, DirectComposition, present with dirty rects | Wayland `wl_surface.damage_buffer`, `wp_presentation`; X11 Present | `requestAnimationFrame`, canvas | CAMetalLayer / SurfaceView |
| pacing | CADisplayLink / `CVDisplayLink` (pacing lane) | DXGI waitable object, `DwmFlush` | `wp_presentation` feedback, frame callbacks | rAF | CADisplayLink / Choreographer |
| IME | `NSTextInputClient` (marked text, candidate rect) | TSF (`ITextStoreACP`) or IMM32 | `zwp_text_input_v3`; IBus/Fcitx; XIM on X11 | `EditContext`, else a hidden `<textarea>` | `UITextInput` / `InputConnection` |
| accessibility | AccessKit macOS adapter (NSAccessibility) | AccessKit Windows (UIA) | AccessKit unix (AT-SPI over zbus) | ARIA through a parallel DOM | AccessKit adapters where they exist |
| clipboard | NSPasteboard (text, rich, images, files) | Win32 clipboard formats | Wayland `wl_data_device` and primary selection; X11 selections | async Clipboard API | UIPasteboard / ClipboardManager |
| drag and drop | NSDraggingSession, promised files | OLE `IDropTarget`/`IDataObject`, virtual files | `wl_data_device` DnD; XDND | HTML5 DnD | UIDragInteraction |
| menus | NSMenu menu bar, context, dock (`muda`-backed capability) | Win32 menus, jump lists | per-DE (dbus menu, or in-window) | in-page menus | none / in-app |
| dialogs | NSOpenPanel/NSSavePanel (`rfd`-backed) | IFileDialog | xdg-desktop-portal FileChooser | `<input type=file>`, File System Access | document pickers |
| tray, notifications | NSStatusItem, UNUserNotificationCenter | Shell_NotifyIcon, toast | StatusNotifierItem, notification portal | Notifications API | platform notifications |
| DPI / scale | `viewDidChangeBackingProperties` | per-monitor v2 `WM_DPICHANGED` | `wp_fractional_scale_v1`, `wl_output` scale | `devicePixelRatio`, ResizeObserver | screen scale |
| appearance / env | `effectiveAppearance`, accent, reduced motion, contrast | `UISettings`, high contrast | `org.freedesktop.appearance` (portal) | `prefers-color-scheme`, `prefers-reduced-motion` | trait collection / Configuration |
| fonts | CoreText discovery via fontique | DirectWrite via fontique | fontconfig via fontique | bundled or `FontFace` | system fonts via fontique |
| CI | GitHub macOS runners (Metal) | Windows runners (WARP for DX12) | lavapipe (Vulkan, correctness only) | headless Chrome with WebGPU | simulators |

Rules that apply on every platform: one scene and gate suite; correctness through scene snapshots and a CPU reference
raster; performance measured on each platform's own hardware class and never pooled across platforms (AUDIT §10).

---

## `platform-host`: portable host and presenter

* **[F10]** Port the transform and decoration kinds (4/5/6) to bough-gpu's wgsl-rs kernels; make wgpu the default once
  STATUS cells hold; delete bough-metal's frame path, keeping its research presenters as harnesses. *partial*
* **[6.5]** bough-gpu's build:
  * emit SPIR-V and HLSL through naga as well as MSL;
  * validate the bind-table/bindless contract per backend;
  * emulate subgroup ops where they are absent (naga capability checks).

  *open*
* **[6.5]** The region contract per backend: zero-copy shared memory on Apple and ReBAR GPUs, staged uploads
  elsewhere, with the upload cost counted in the frame. *open*
* **[6.1, X10]** A winit host with event parity:
  * key up and `FlagsChanged`, the full `Key` set (F-keys, keypad, dead keys, repeat);
  * middle button, horizontal and momentum wheel with phases;
  * magnify, rotate and swipe gestures; pressure, tablet and touch;
  * scale-change and occlusion notifications;
  * the window close/quit lifecycle.

  *open*
* **[6.1]** Regular activation policy for applications, a menu bar, Cmd-Q/Cmd-W, Dock presence (Accessory only for
  benchmarks). *open*
* **[6.1]** Scale changes: re-raster and relayout on `backingScaleFactor`/DPI change; a per-output scale. *open*
* **[6.1]** Occlusion: throttle hidden windows (`NSWindowDidChangeOcclusionStateNotification`, DXGI occlusion,
  Wayland frame callbacks). *open*
* **[D22, PL1]** Multi-window on one event loop: shared device, queue, pipelines and atlas, per-window surfaces and
  regions. *open*
* **[D22]** `app.attach(host_loop)` / `app.poll(deadline)` for embedding (09 §7; gap-register S5 "READ THROUGH THIS").
  *open*
* **[PL2]** Renderer selection (`Auto | backend | ReferenceCpu | fallbacks`), device-loss recovery (keep the CPU scene,
  re-upload, publish `Recovered` or a typed fault), integrated/discrete GPU selection. *open*
* **[S2 gap register]** A software (CPU) renderer as a real backend, not only an oracle: framebuffer/X-forwarding use
  cases. *open*
* **[F3]** Startup on every backend: cached pipelines (`MTLBinaryArchive`, the Vulkan pipeline cache, DX12 PSO
  libraries), device creation overlapped with mount. *partial on Metal*
* **[F13]** Frame-rate control: `preferredFrameRateRange` and its equivalents, adaptive sync where available. *open*
* **[6.5]** Windows: DX12 flip-model swap chain, DirectComposition, dirty-rect present, per-monitor DPI v2, WARP in CI.
  *open*
* **[6.5]** Linux:
  * Wayland (`xdg-shell`, `wp_presentation`, `wp_fractional_scale_v1`, `wp_viewporter`, `xdg-decoration`,
    `zwp_text_input_v3`);
  * X11 fallback;
  * Vulkan with lavapipe in CI;
  * layer shell (gpui-ce has it).

  *open*
* **[6.5]** Web: WebGPU, canvas sizing and DPR, a rAF loop, the region as a storage buffer with `writeBuffer`, wasm
  size budget. *open*
* **[6.5]** iOS/Android: deliberately later; record blockers only. *open*

## `platform-capabilities`

* **[6.4, PL6] IME.**
  * The bidirectional protocol types: `Enable/Update/Disable(InputMethodState)` with surrounding text and revision,
    UTF-8 and UTF-16 selection, preedit styling, the replacement range, the cursor rect, purpose, capitalisation and
    direction; `Preedit/Commit/DeleteSurrounding/SetSelection/Candidates` tagged with that revision.
  * A scripted fake IME for tests; commit-on-blur; history coalescing for a composition.
  * Then macOS `NSTextInputClient`, Windows TSF, Wayland text-input-v3, IBus/Fcitx, XIM, web `EditContext`.

  *open*
* **[6.3] Accessibility.**
  * An AccessKit projection from the record tree's semantic column: roles, names, values, actions, focus, live
    regions, text ranges for `text_input`, scroll-into-view, hit testing in a11y coordinates.
  * Port the archive's sparse projection (`production/accessibility-sparse`).
  * Copy gpui-ce #282 (a click on the wrong element while scrolled) as a test.

  *open*
* **[6.2] Clipboard.** Text, rich text, images and files; primary selection; the find pasteboard; lazy providers; an
  ownership-loss notification. *open*
* **[6.2] Drag and drop.** One `DataOffer` protocol for internal typed values, OS MIME data, promised files and virtual
  files; drag-out; drag-over styling; typed `on_drop::<T>`. *open*
* **[6.2] Menus.** Menu bar, context menus and dock menu, bound to commands [D10]; key equivalents shown; one
  cross-platform API ("CLEAN AND ELEGANT AND CROSS PLATFORM"). *open*
* **[6.2] Dialogs.** Open, save and folder, with filters and bookmarks, as async actions [A7]. *open*
* **[6.2]** Tray icon, notifications, open-URL, URL schemes, credentials (keyring), haptics, screen capture (gpui-ce
  has these). *open*
* **[6.2] Cursors.** Arrow, I-beam, pointer, resize, grab, not-allowed, hidden, as a style property. *open*
* **[PL1] Window vocabulary.**
  * Frame, background, material (`Prefer`/`Require`), shadow, corners, controls, level, hit test, min/max size,
    fullscreen, transparency/vibrancy/Mica, titlebar styles, tabs, parent/modal/popup.
  * `Win<V>` live readers (`active`, `bounds`, `scale`, `display`, `appearance`) and queued verbs.

  *open*
* **[07 §6]** Inherited environment: appearance, accent, text size, reduced motion, contrast, transparency, language
  direction, as inherited sources; RTL layout mirroring through cephal's logical edges (gap register: layout-level
  RTL). *open*
* **[gap register]** Downward-inherited properties (a subtree-wide `disabled`); a virtual keyboard on the IME
  protocol. *open*
* **[S6 gap register]** Display enumeration available at startup; per-output DPI on monitor moves. *open*

## `surface-api`: the design's public surface

* **[D1, F8]** Measure re-description per handler (dashboard click, the zgx suite, the inspector). *merged-untested*
* **[D1a]** Lower wire-only subtrees to templates automatically, so re-description is a near-free walk; then the
  `Config::once()` harness option can go. *open*
* **[D2, X1]** `&self.x` in a sink as the wire, proven by `reflect::schema::<V>()` (struct-only path to a `Signal<T>`),
  with `sig!` as the stable-build fallback. *open*
* **[D3]** `El<'f>`: one concrete, frame-borrowed currency; `if`/`match` arms unify without `.into()`. *open*
* **[D4, X3]** `Element` without an owner type: routes carry (TypeId, trampoline), checked at mount by the owner chain
  (L4). *open*
* **[D5]** The `ChildSeq` forms:
  * `Option` (splice);
  * iterators (ordinal realign);
  * `&Vec<T: View>`, `&List<T>`, `list.each(f)`;
  * tuples beyond 8.

  *partial (tuples ≤ 8, `each`, `Vec`; api has `List`)*
* **[D6]** `Handoff<T>` (give/take; untaken diagnosed; `Handoff<T, Latest>`); `Target<K>` (focus, tests, the capability
  visitor). *open*
* **[D7]** `#[derive(Delta)]` generating the boundary-only `Changed<'_, Self>`. *partial (api `changed`, no derive)*
* **[D8]** Contained child views (a field that is itself a `View`) as owners with their own wires and `changed`.
  *partial*
* **[D9]** Handler return table (`consume`/`propagate`, `Option`, `Result`), `.with(..)` bindings, capture-free
  closures with the located error, `on_key(Key::Escape, f)`. *open*
* **[D10]** `#[command("id")]` through `inventory`/`linkme`; keymaps as data; menu binding. *open*
* **[D11]** Focus as a queued claim on a `Target`; focus scopes; restore on unmount; tab order. *partial
  (`focusable`)*
* **[D12]** `text_input(&sig).placeholder().target().on_edit(interceptor → Accept/Amend/Reject)`. *open*
* **[D13]** The `IntoProp` family:
  * `T`, `&T`, `&Signal`;
  * `Reader`, `Source` compositions (`map` over tuples), `Motion`, tokens;
  * `Option` only where absence has one meaning.

  *partial*
* **[D14]** Named `Impact` per port; the impact union per publication; "why did layout run" in the inspector. *open*
* **[D15]** Motion:
  * `animate(&sig).spring(..)` with analytic springs, keyframes, sequence, repeat and reverse;
  * reduced motion;
  * completion as a typed output;
  * enter/exit snapshots;
  * transitions addressable by property.

  *partial (`transition(Curve)`)*
* **[D16]** Style:
  * `.hover/.active/.focus/.disabled` variant tables;
  * tokens, a layered cascade, theme swap, `Look`, contrast pairs;
  * `.fg/.tint`, `tw!`, `Color::Red`.

  *partial (`hover_bg`)*
* **[D17]** Placement: `.child(x.align(TopRight))`, `.at(handle)`, `.above/.below`, `.z(n)`; geometry as an inherited
  source. *open*
* **[D18]** `Grid` recipe; `Flex::wrap`. *open*
* **[D19, A1–A9]** Async (lane `async-entities`). *partial*
* **[D20]** serde journaling (restore = construct from sources); undo/redo over the delta log. *open*
* **[D21]** The `draw(|paint: Cx<Paint>|)` floor; custom surfaces with phase capabilities. *partial (raw)*
* **[D23, B2]** Plugins: `AnyEntity` at the boundary; `typetag` items; `abi_stable` for dynamic plugins. *open*
* **[§3.2] Missing style fields:**
  * shadows, opacity, transforms, gradients;
  * per-side and dashed borders, outline;
  * `max_w/max_h`, aspect ratio, `flex_wrap`, insets;
  * overflow, z-index, visibility, cursor;
  * font weight/style/family, text-align, line clamp, ellipsis, nowrap, letter-spacing, decoration;
  * blur and backdrop blur, corner smoothing;
  * units (`px`/`rems`/`relative`), a spacing scale, `size_full`.

  *partial (raw has several)*
* **[§3.3] Missing interaction:**
  * click count; typed drag payloads and drop targets;
  * the capture phase; right-click and context menus;
  * actions and key contexts;
  * gestures, pressure and touch; scroll phases; horizontal wheel;
  * consume/propagate.

  *open*
* **[U1–U14] Usability.**
  * Owner-free helpers; `Signal::update` and `toggle`.
  * Located errors instead of capacity panics and const-asserts.
  * Better defaults (button and input states, focus ring); `text_input` placeholder.
  * Units; `div()`'s default flow documented or aligned with gpui-ce.
  * `Application::run` setup (menus, theme, assets, fonts); prelude/raw leakage removed.
  * A book and a gallery.

  *open*
* **[R7]** Closure and recipe storage with a counted spill instead of the hard 16-byte limit. *open*
* **[S1]** Make `App`/`Rt` fields crate-private behind a typed inspector API. *open*
* **[S8]** The default build on stable (reflection behind a `reflect` feature). *open*

## `async-entities`

* **[A1]** `signal.map_async(f)`, `try_map_async`, `compute` (CPU pool), `map_async_on`; an injected executor handle
  (Tokio, smol, bough's); the `StampedCompletion` wake law. *partial (api `produce`)*
* **[A2]** `reader.project(|&T| El)`, `map_ref` into ports, `Settled<T>` via `reader.or(fallback)`. *open*
* **[A3]** `boundary().loading(..).failed(..).reload_on(&input)`, first-readiness semantics, the declaration-waterfall
  report. *partial*
* **[A4–A6]** `is_pending(x)`; the failure table with heal-on-retry; `RequestStamp` with frozen input versions (the
  `A → B → A` test). *partial*
* **[A7]** `.action(..).queue(Queue::Reliable).pending(..).then(..).catch(..)`, with optimistic tagged deltas. *open*
* **[A8]** `Remote<T>` streams with lanes (`Reliable`/`Latest`/`Coalesce`/bounded), `try_send` → `Full`,
  prefix-loss-only delivery. *open*
* **[A9]** The entanglement rule (05 §9.4) as a property test. *open*
* **[B1]** A non-generic `EntityCore`; `Entity::new_cyclic`; `Delivery::Closed`; no `Deref`. *partial (api
  `Entities`)*
* **[B3]** Presence as a supervised `Remote` session (collaboration). *open, far*
* **[R1]** Speculative description and atomic publication (`LogicalDraft`/`PresentationDraft`, layout inverse-op
  journal). *open*
* **[R2]** Priority lanes (Input/Animation/Normal/Idle), idle work aged and preemptible. *open*
* **[R3]** The teardown/eviction protocol: owner epochs, and rows holding IME composition or pointer capture
  unevictable. *partial*

## `text`

* **[T1, X6]** Parley `Layout` + fontique under the façade, with the grafts as the covered fast path. *open*
* **[T2]** Bidi and RTL layout; vertical text later. *open*
* **[T3, T15]** Rich text in one paragraph; `rich(&pieces).atom(view_fn)` over `List<Inline>` with interval marks.
  *open*
* **[T4]** Emoji and colour glyphs (COLR/sbix) into the atlas. *open*
* **[T5]** The editor on `PlainEditor`:
  * selection, word/grapheme/paragraph motion, platform shortcuts;
  * double and triple click, select-all;
  * clipboard, undo/redo, placeholder, password, multiline, scroll-into-view.

  *open*
* **[T6, T16]** Selectable static text; FlowIndex cross-element selection; the byte-offset law. *open*
* **[T7]** Ellipsis, line clamp, text-align, letter-spacing, decorations. *open*
* **[T8]** Variable-font axes at run time. *open*
* **[T9]** Raster fidelity against CoreText and zgx (gamma, AA). *open*
* **[T10]** Menlo/AAT through harfrust; kerning across segments beyond Latin-1; port `script.rs` to read-fonts. *open*
* **[T11]** Hinting and bi-level glyphs through skrifa `HintingInstance` (`Target::Mono`, `Smooth`); hinted advances
  fed to the shaping kernels. (Gap-register note: "DOES PARLEY FIX THIS?". Partly: skrifa yes, parley 0.12 no.) *open*
* **[T12]** Paint-time physical-pixel snapping for borders, rules and baselines; `.snap(false)` (gap register: "WOULD
  BE GREAT TO HAVE"). *open*
* **[T13, T14]** One inline formatting context with atoms and the CSS order of operations; layered decorations with
  skip-ink. *open*
* **[T17]** The editor kernel:
  * branded `Pos<Space>`;
  * the display-map transform stack, async `WrapDemand`;
  * blocks and inlays as elements;
  * `MultiBuffer`, N viewports.

  *open*
* **[T18]** Text raster policy as data (hinting, AA, sampler filter, snapping). *open*
* **[F4, RT5]** A process-wide or mount-time text cache for remounts; on-disk font tables. *retry*
* **[RT9]** A bounded parley scratch cache (archive `text-cache-cold-trim` policy). *retry*
* **[F12]** Multi-core shaping of large documents (`ChunkedDoc`); the SIMD ligature tokenizer. *open*

## `gpu`

* **[RT2]** 2D binning with a parallel scatter: the named fix for glyph-dense tiles. *retry*
* **[RT3]** Copy or redraw per frame from a cost model (exposed area, surface age, glyph density). *retry*
* **[RT1]** Tile-flip presentation with zgx's timed handover and a tearing detector. *retry*
* **[RT4, X8]** A zero-pixel scroll: one tall layer moved by position/`contentsRect`, or a wraparound strip. *retry*
* **[RT6, RT7]** Sample classification (uniform tiles, partitions) after RT2. *retry*
* **[F5]** Pre-binned glyph runs; bins patched by delta; 16-px quadrant masks. *open*
* **[X5, F3]** Every kernel in wgsl-rs; a build-time metallib or binary archive. *partial (fs_bin, fs_tile, compact,
  copy)*
* **[BRIEF 7.2.5–6]** GPU-evaluated paint transitions; half-precision accumulation if the pairs show a gap. *open*
* **[PL3]** Colour:
  * CSS Color 4 constructors (`oklch`, `oklab`, Display P3), `.alpha()`, `mix` in oklab;
  * premultiplied linear extended-sRGB compositing;
  * `ExtendedSrgb16` HDR surfaces (zero-tax when unused);
  * optional ICC colour management.

  *open*
* **[PL4]** The temporal graph (`post_process`). *open, low*
* **[gap register]** Glyph-atlas sampler filter option. *open*

## `style-motion-media` (new)

* **[D15]** Motion lanes (paint, transform/opacity, placement, layout, uniforms), one frame clock sampling dense lanes,
  analytic springs. *partial*
* **[D16]** Tokens and cascade with reverse-use lists (a palette swap patches only consumers); variant tables
  precomputed at publish. *open*
* **[PL5]** Media:
  * `IntoMedia` (`RgbaImage`, `DynamicImage`, `Png(bytes)`, `asset!`, `Uri::get`, a `Reader<Frame>`);
  * one staged pipeline with split, budgeted caches;
  * JPEG/WebP/GIF/SVG;
  * `.fit/.sampling/.slice/.tile/.crop/.flip`;
  * animated images as streams; video.

  *partial (`image(&'static [u8])` PNG)*
* **[F9]** Image decode off the main thread, a hash once per static pointer, mipmaps or GPU resample. *open*
* **[D21]** Canvas, custom shaders (`.shader(MyFx)`), named filters (`.blur`, `.backdrop_blur`). *partial (raw deco)*

## `pacing`

* **[F2]** Explain the 75.7 ms dashboard stall (trace one run). *open*
* **[F1, X4]** Input coalesced into display-link turns; never drop a click; presents ≤ refresh. *open*
* **[Q13]** Input-to-photon p50 and p99 on real hardware (a photodiode or capture method), per platform. *open*
* **[R5]** A published `PresentationResolver`: hover/active variants and the scroll table resolved with zero graph work
  (FP1). *partial (hover is a paint write)*
* **[R8]** Frame pipelining with an input latency lane during animation. *open*
* **[F14]** AWAGY on a second machine and display; zgx and BUI reruns. *open*

## `memory`

* **[F6, X7]** Virtualization in core: stub records for offscreen subtrees of any container, materialised on exposure;
  anchoring when a measured size differs from its estimate; nested scrolls; 1 M rows. *partial (listview)*
* **[F7]** Footprint: size region columns from the mount, grow by doubling, purge spare surfaces. *open*
* **[R4]** The `MemoryLedger` per window:
  * a debug allocator hook asserting steady-state zero allocation;
  * a pressure cascade and high-water trim;
  * learned size classes per View type;
  * gates M0–M6.

  *open*
* **[R6]** `impl Display` written into a retained buffer (generalising `NumFmt`). *open*
* **[S4]** Capacity overflow grows or degrades; never panics after construction. *open*
* **[F11, RT8]** `parent`/`ancestors` without a slow fallback: a lazily built parent column with a change counter, past
  a depth gate. *retry*

## `verification`

* **[Q1]** Run the suite on `main`; macOS CI (check, clippy, tests, examples), then one job per backend. *open*
* **[Q10]** Serialised scene snapshots as the primary gate (tronical's point: gap register S2, "SWITCH OUR SCREENSHOT
  TESTING FOR THIS"); a CPU reference rasteriser with an NV-FLIP tolerance. *open*
* **[Q2]** `Win::demo(v)`, turn-barrier inputs (`type_text`, `click(target)`, `press(key)`), semantic queries, a virtual
  clock, `app.until(..)`. *open*
* **[Q3]** The gate registry (10 §1: surface, reactions, async, platform bar T1–T6, colour, media, memory, fast path,
  text, history) as named tests. *open*
* **[Q11]** `#[diagnostic::on_unimplemented]` text and a `trybuild` corpus asserting messages. *open*
* **[Q12, Q4]** A `UiSnapshot` schema per epoch; the inspector on the prelude, with its own window and the typed state
  panel. *partial*
* **[Q5]** Reflection:
  * open enums (a per-enum discriminant reader);
  * lift the 256-node cap;
  * report the tuple `TypeId::info` ICE upstream.

  *partial*
* **[Q6, Q7]** "Why did layout run" diagnostics; trace export (Perfetto/Tracy/`os_signpost`). *open*
* **[S2]** Miri on the headless gates; replace `usize` fn-pointer transmutes; audit the reflect vtables. *open*
* **[S6]** Loom or stress tests of the publisher thread handoff. *open*
* **[§11]** Port the five `bough::raw` comparison crates to the prelude where it can express them. *open*
* **[14.8]** Phase 0:
  * conformance fixtures;
  * a deterministic headless clock and event traces;
  * a counter registry;
  * a panic-free lint baseline with a burn-down file.

  *open*
* **[Q8, Q9]** Hot reload (later); rustdoc and a book. *open*

## `components` (new)

* **[X11, §3.1]** A component crate written on the prelude only, as dogfood:
  * checkbox, switch, slider, radio, select, tabs;
  * popover with collision placement, context menu, tooltip without a rect;
  * scrollbar visuals, progress, data grid, tree view.

  Log the friction the way INSPECTOR.md did. *open*
* **[§3.1]** Container queries; anchored/deferred overlays. *open*

## `developer-loop` (new)

* **[B4]** The linking and compile gates:
  * generic instantiation count;
  * incremental rebuild after an app-only edit;
  * clean link time, artifact size, exported symbols.

  The builder types (`On<LBox<Color,(Para<S>,)>, HoverBg>`) are a real risk. *open*
* **[performance.md #13]** Developer-loop workload: clean and incremental compile, rust-analyzer latency, binary size;
  explicit vs macro spelling. *open*

## `independent-review`

* Replay every claim that reaches `MEASURED`, from a clean checkout and a fresh target on the stated hardware. *open*
* Re-run the corrected AWAGY claims (click counts, the zgx tile hook, the BUI view hook) on a second machine. *open*

---

## Reference workloads (`designs/reactivity/performance.md` §4)

Missing ones belong to the lane named.

| # | workload | state | lane |
|---|---|---|---|
| 1 | idle/park | measured (zero frames) | memory |
| 2 | counter locality | partial | surface-api |
| 3 | hover storm 120/240 Hz | open | pacing |
| 4 | scroll split | measured (zgx scenes, scroll by copy) | gpu |
| 5 | 100 k keyed table | open | memory |
| 6 | variable-height virtual list | measured (zgx vlist) | memory |
| 7 | editor | open | text |
| 8 | kitchen sink | open | components |
| 9 | animation tiers by phase | open | style-motion-media |
| 10 | churn and teardown | measured (zgx suite) | memory |
| 11 | cold/warm start | measured (zgx, zgui) | platform-host |
| 12 | adversarial graph | open | async-entities |
| 13 | developer loop | open | developer-loop |
| 14 | capability ergonomics (compile-fail) | open | verification |
| 15–17 | property lowering matrix, direct-field precision, 10 k-consumer palette swap | open | surface-api, style-motion-media |
| 18 | spatial and renderer pressure | open | gpu |
| 19 | reactive competitor constants (zgui) | partial | surface-api |
| 20 | protocol, publication and fault matrix | open | async-entities |
| 21 | collection threshold matrix | open | memory |
| 22 | typed work-edge matrix | open | async-entities |
| 23 | retained drag protocol | open | platform-capabilities |
| 24 | placement graph | open | surface-api |
| 25 | motion semantics | open | style-motion-media |
| 26 | canvas/shader/temporal graph | open | gpu |
| 27 | inspector-disabled zero cost | open | verification |
| 28–30 | image pipeline, video, frontend equivalence | open | style-motion-media, platform-host |
| 31 | history / undo | open | surface-api |
