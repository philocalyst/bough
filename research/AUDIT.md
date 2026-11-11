# bough audit, 2026-11-11: the work list

Gaps between bough and its design (`design/final`), latest gpui-ce, and the evidence in the research branches, with the
work items, experiments and shortcomings that follow from them. Every claim about bough names the file it was read in
(`canonical` at 66c0eab5f0 unless a branch is named). External facts marked *(agent-reported)* came from the Haiku
research passes and were not re-verified here.

What was read:

* **bough**: all of the façade (`crates/bough/src`: lib, app, elem, signal, input, ui, layout, template, ltemplate,
  mlist, text, transition, inspect, backend/host), the headers and public surfaces of bough-core, bough-text,
  bough-metal (shader.metal kernel list, app.rs host), bough-gpu, the vendored parley grafts
  (`vendor/parley/src/bough`), `vendor/README.md`, BRIEF.md, STATUS.md, INSPECTOR.md, research/ROUND3–5. Branch
  `api` in full outline (`git diff canonical...api`: 304 files, +22,612) and its reflect.rs, signal.rs, elem.rs,
  API-RESULTS.md; branch `floor` by diff stat and BRIEF §9; every AWAGY workload source.
* **Design**: 00-model (whole), 01-state §1–8, 02-elements §2–9, 03-ports §2–5, 04-reactions §3–4, 06-text §3, 07 §4–7
  and 10 §1–3 (from the previous pass), the headings of 05, 08, 09, 11.
* **Research branches** (archive, 833 refs): the 2026-10-28 index of 141 `local/*` branches, `architecture-against-zgx.md`
  and the architecture-campaign register (every disposition line, the native-diagnosis and scroll-repair sections),
  plus the family names of the `codex/*`, `production/*`, `ime-*`, `accessibility-*`, `inspector-*`, `text-*`,
  `hummingbird-*` and `spike/*` refs.
* **gpui-ce** `main` a19d4a9345 (2026-11-11): crate list and sizes, `gpui/src` module list, `elements/`,
  `gpui_elements`, test contexts, gestures, a11y methods, the `view_example` app.

## 0. Verdict

bough is a fast, small, macOS-only rendering and damage kernel with a partial application API. As built on
`canonical`, it is not yet the framework the design describes, and it is not at feature parity with gpui-ce.

The fifteen items that matter most, in order:

1. **No re-description on canonical.** `App::new` calls `view()` once (`app.rs:269`). After that, only `sig!` wires
   update. A plain field mutated by a handler never shows, so the design's floor (00 §5, 01 §2: "a handler mutating
   `&mut self` reruns the owner's `view`") fails. Conditional UI (`if`/`match`/`Option`) is impossible. The `api`
   branch built re-description, but behind `const FIXED: bool` (`api:app.rs:32`), the constant the user rejected.
   This is the first work item.
2. **The design's binding spelling is on bough's rejection list.** The design wires a signal with a plain borrow,
   `text_input(&self.query)` (01 §3). It lists `Wired`/`Wires<T>` among the deleted vocabulary (00 §6). bough
   requires `sig!(self.query)` producing a `Wired` (`signal.rs:189`), and a plain `&Signal<T>` is a constant
   (`elem.rs:129`).
3. **The `api` branch (List, Entity, Reader, `changed`, reflection, virtualization as exposure) is not merged.** That
   is 22.6 K lines of the design, gated by 182 tests at ae360b7, sitting off the main line.
4. **Text runs on one font with no fallback, bidi or rich spans.** parley is vendored, but the runtime uses only the
   grafted kernels; nothing calls parley's `Layout`/fontique (`text.rs:1-16`, fontique appears only in tests).
5. **No IME, accessibility, clipboard, menus, dialogs, drag and drop, cursors or multi-window.** Every one of these
   is a release blocker, and gpui-ce has all of them.
6. **The host is a local NSEvent monitor on an Accessory-policy app** (`host.rs:236-262`, `bough-metal/app.rs:101`).
   It handles no KeyUp or modifier changes, no horizontal or momentum wheel, no middle button, no gestures, and
   scale is read once. It has no Dock icon and no menu bar, so Cmd-Q and the Edit menu don't exist.
7. **The frame path is a 1,596-line MSL file compiled at load** (`bough-metal/src/shader.metal`). wgsl-rs carries
   only the two bough-gpu kernels (`compact.rs`, `copy.rs`, 133 lines), and the wgpu path lacks transforms and
   decorations (floor). This misses the ROUND4 directive ("shaders stay in wgsl-rs", pure wgpu ≥ direct Metal) and
   costs startup.
8. **Elements are typed by their owner** (`Element<V>`, `elem.rs:53`). A reusable component must be generic over
   `V`, and every helper in the AWAGY apps is pinned to its app type (`fn row(i) -> impl Element<ScrollApp>`). The
   design's `Element<'f>` has no owner parameter; routes resolve their receiver at mount (L4).
9. **Presentation is driven by input events, not the display.** `host.rs:330` draws a frame per NSEvent. AWAGY
   measured ~240 presents/s on a 120 Hz panel and a 75.7 ms worst dashboard gap (vs Vizia-opt 31.0, zgx 42.5); no
   clicks were dropped (corrected, §15.4). p95 frame gap at 7 K rows is 12.4 ms against iced's 8.4–8.6 ms.
10. **Lists take a fixed row count, a `fn` row builder and a viewport in coordinates** (`mlist.rs:395`,
    `elem.rs:677`). There is no `List<T>` delta on canonical, and the prelude's `.scroll()` mounts every child, so
    virtualization isn't in core.
11. **The style surface is ~23 layout methods plus bg/rounded/border/font/color** (`layout.rs:199`). gpui-ce has 134
    styled functions. Missing: hover/active/focus/disabled variants, tokens and themes, shadows, opacity, transforms,
    gradients, text decoration/weight/alignment/ellipsis, grid, z-order, cursors.
12. **No test API at the design's level** (10 §2: `Win::demo`, `type_text`, `click(target)`, semantic queries,
    virtual clock), and no CI.
13. **Hygiene:** `App` and `Rt` expose their internals (`app.rs:128`, `rt.rs:352`: Rt has ~90 pub fields). There
    are 503 `unsafe` blocks, absolute `/Users/mileswirht/...` paths in three manifests, and the toolchain is
    nightly-only (`type_info` reflection on api).
14. **Five older benchmark crates (~45 files) are still written against `bough::raw`**: quads at coordinates. The
    AWAGY apps use only the prelude.
15. **Measurement discipline lags the archive's.** The research branches preregister thresholds, keep negative
    controls and reject any candidate that loses one fixed cell by more than 5 %. bough's rounds report wins scene by
    scene, measured per branch, mostly on a loaded machine (load average 18–110 in STATUS and API-RESULTS).

What is strong and must not regress:

* zero work when idle;
* per-frame instructions 6–17× below zgx on its desktop scenes (STATUS §1);
* constant-time updates for signal-bound properties;
* the hit test (12.7 ns among 1,000 cards);
* AWAGY: the smallest executable (2.66 MiB) and the lowest dashboard RSS of every framework measured;
* typed fn-item routes;
* compile-time reflection without opt-in (api).

---

## 1. Size and shape, bough vs gpui-ce

| | bough canonical | gpui-ce main |
|---|---|---|
| Rust lines (crate sources) | ~30.7 K (bough 10.7 K, core 5.8 K, metal 5.2 K, text 3.5 K, damage-lab 4.4 K, gpu 0.6 K, mem 0.4 K) + parley grafts 2.9 K | ~230 K (gpui 107 K, linux 16 K, windows 13 K, macos 12 K, wgpu 9.8 K, render 7.8 K, parley 7.8 K, elements 7.1 K, web 5.0 K, …) |
| platforms | macOS (AppKit, Metal) | macOS, Windows, Linux (X11 + Wayland), web; wgpu renderer on all, Metal renderer on macOS |
| tests | 142 `#[test]` (+ ~1.9 K lines of tests on api) | TestAppContext / VisualTestContext, `simulate_*` input, screenshots |
| app model | owner struct, `View::view`, `sig!` wires, mount once | `Render` + `Context<T>`, `Entity<T>`, `cx.notify`, and now `window.use_state(cx, ..)` hooks (`view_example_main.rs`) |
| layout | cephal (vendored, flex/block/grid features) | taffy |
| text | single-font token kernels + rustybuzz | parley 0.12 (`gpui_ce_parley`), font fallback, bidi, rich text |
| a11y | none | AccessKit tree, 27 `aria_*` element methods, `accesskit::Role` |
| inspector | headless prototype in a comparison crate | built in (issue #93: doesn't render *(agent-reported)*) |

gpui-ce is moving toward hooks (`use_state`), the opposite of the design's no-hooks law (00 §6). bough is the closer
of the two to the design on ambient context (no `cx`) and routes (fn items). It is further than gpui-ce from the
design on re-description and on binding spelling.

---

## 2. Design-fidelity gaps (the public API vs `design/final`)

| # | design | bough canonical | work |
|---|---|---|---|
| D1 | Immediate authoring: plain `T` fields; a `&mut self` handler reruns `view`, and the retained diff turns that into the delta (00 §2.2, 01 §2) | mounts once (`app.rs:269`); plain fields are dead after mount | Merge api's lockstep re-description with **no `FIXED` flag**: re-describe the owner after any handler that took `&mut self`, and make that cheap (D1a). Measure the cost of re-describing every AWAGY/zgx scene per click; it must stay within the fast-lane budget (08 §6). |
| D1a | a const-evaluable subtree lowers to a static template with a patch table (08 §7, 00 §2.2) | `template`/`ltemplate` exist as raw API | Lower `view` automatically: a subtree with no plain-field reads is template + wires, so its re-description is a no-op walk. This is how "no FIXED" keeps the mount-once wins. |
| D2 | `&self.x` in a sink **is** the wire; `Wired` is a deleted name (01 §3, 00 §6) | `sig!(self.x)` → `Wired`; `&Signal` = constant | Experiment X1: prove the wire from the compile-time schema instead of a macro. At mount, `off = addr - owner` is accepted only if `reflect::schema::<V>()` has a `Signal<T>` at exactly `off` through struct-only field paths (no enum or union on the path). That proves the same thing `offset_of!` does, with the design's spelling. Keep `sig!` as an internal fallback for non-nightly builds. |
| D3 | `El<'f>`: one concrete, sealed, frame-borrowed currency; `if`/`match` arms unify; `impl Element` for leaves (02 §2) | `impl Element<V> + '_` with structural types (`On<LBox<Color,(Para<S>,)>, HoverBg>`); arms don't unify | An `El<'f>` built on the per-turn tape (api's describe arena). Keep the monomorphic builders as the lowering. Gate: an `if`/`match` returning different builders compiles without `.into()`. |
| D4 | `Element<'f>` carries no owner type; routes resolve their receiver at mount by the owner chain (L4) | `Element<V>`; a helper must name its owner or be generic | Element without `V`: a route lowers to (TypeId of `R`, trampoline) and is checked at mount against the owner chain (a located mount error), as L4 says. Measure dispatch: today it's a direct trampoline (`input.rs:100`). |
| D5 | `ChildSeq` family: `El`, tuples, `Option`, iterators (ordinal realign), `&Vec<T: View>`, `&List<T>`, `list.each(f)` (02 §4) | tuples ≤ 8 (`elem.rs:73-80`), `Vec<E>`, `each(iter)`; no `Option`, no delta forms | `Option` (splice), iterator realign, `&List<T>`/`each` slot deltas (api `list.rs`, `listview.rs`), tuple arity via nesting or a macro. |
| D6 | storage ladder: `Signal`, `List<T>`/`Slot<T>`, contained views, `Entity<T>`, `Handoff<T>`, `Target<K>`; `Reader<T>` across latency (01, 05) | `Signal` only | merge api (`List`, `Entities`/`Entity`, `Reader`, cohorts); build `Handoff<T>` and `Target<K>` (missing on every branch) |
| D7 | `View::changed(&mut self, Changed<'_, Self>)`; `#[derive(Delta)]` (04 §1, 01 §8) | absent (api has `changed` with per-class shims; no derive) | `#[derive(Delta)]` that generates the boundary-only delta (04 §1.2); merge api's `changed`. |
| D8 | contained child views (rung 2): a field `editor: Editor` that is itself a `View` | no child `View` composition; one owner per `App` | Child views as owners with their own wires and `changed` (api has "owner classes"; verify it covers contained fields, not just list rows and entities). |
| D9 | handlers: `fn(&mut R, [ctx], [event], [bound]) -> R`; return-type table (consume/propagate, `Option`, `Result`); `.with(..)` bindings; closures must capture nothing (04 §3) | `Fn(&mut V) + Copy`, ≤ 16 bytes of captures (`input.rs:452`, `elem.rs:154`); `on_key(f)` gets every key | `.with(..)`; `on_key(Key::Escape, f)`; return-value table; reject capturing closures with the located error the design specifies. |
| D10 | `#[command("id")]` via `inventory`/`linkme`; keymaps as data (04 §3.2) | none | build it, plus a keymap loader and menu binding (with P-items). |
| D11 | focus as a queued claim on a `Target<K>`; `.focus_default()` (04 §4) | `focusable()`, `set_focus(handle)` on `App` (`input.rs:1404`) | `Target<K>` + `claim_focus()`; focus scopes; restore on unmount. |
| D12 | `text_input(&sig).placeholder().target().on_edit(interceptor → Accept/Amend/Reject)`; IME sub-transaction; selection, word/grapheme motion, platform shortcuts in the control (06 §3) | `text_input(sig!(..)).size(w,h)`; caret only; Left/Right/Home/End/Backspace/Delete (`input.rs:1360-1395`) | see T-items; the control is far from the contract. |
| D13 | `IntoProp` family: `T`, `&T`, `&Signal`, `Reader`, `Source` compositions, `Motion`, token, `Option` (03 §2) | `f32`, `(f32,f32)`, `Color`, `Wired`, `WMap` (`elem.rs:118-200`) | `Source` algebra (`map` over tuples of sources), tokens, motion values, `Reader`. |
| D14 | `Impact` per port, union per publication, attributed in the inspector (03 §3) | damage classes are implicit in each applier | name the impact per applier; expose "why did layout run" (10 §4). |
| D15 | motion: `animate(&sig).spring(..)`, keyframes, sequence, repeat, reduced motion, completion as a typed output, enter/exit snapshots (03 §4) | `.transition(Curve)` on a wired operand: tween + Hermite C1 (`transition.rs`) | springs with analytic velocity, repeat (spinner), reduced-motion, enter/exit; transitions addressable by property (INSPECTOR #7). |
| D16 | style: `.hover(\|s\| ..)`, `.active`, variants resolved into a table; layered cascade; tokens with contrast pairs; `.fg`/`.tint`; `tw!`; `Color::Red`/`oklch` (03 §5) | `hover_bg(color)`, `paint_with(sig, f)` | variant tables, tokens + cascade + theme swap, `.fg/.tint`, `tw!`; color spaces (07 §2). |
| D17 | placement algebra: `.child(x.align(TopRight))`, `.at(handle)`, `.above/.below`, `.z(n)`; geometry as a source (02 §7) | `abs()`/`relative()` in Styled; `anchor`/`offset` in raw | the four placements; text/caret geometry as an inherited source. |
| D18 | `Flex` and `Grid` recipes (02 §3) | `Flex` (row/column/gap/align/justify); cephal has grid enabled, but there is no `Grid` recipe | `Grid` recipe; `Flex::wrap`. |
| D19 | `boundary().loading(..).child(reader.project(..))`; `map_async`/`compute`; `.action(..)` policies; `Remote<T>` (05) | none on canonical (api: Reader, boundary by clip rects) | merge; `.action` and `Remote<T>` unbuilt. |
| D20 | serde journaling: restore = construct from sources (L11); `gpui_history` undo over the delta log (01 §9–10) | none | after D6/D7. |
| D21 | `draw(\|paint: Cx<Paint>\| ..)` floor; custom surfaces with phase capabilities (02 §1.2, §7) | raw `quad`/`bars`/`deco_box`/`svg_image` | a public paint floor that writes records, not a second vocabulary. |
| D22 | `Application::run`, windows as capabilities, `app.attach(host_loop)` / `app.poll(deadline)` (09 §7) | `Application::run(Window::document(v))` (`ui.rs`) | multi-window, attach/poll (with PL-items). |
| D23 | plugins: `AnyEntity` at the boundary, `typetag`/`abi_stable` (09 §2) | none | after D6. |

---

## 3. API gaps vs gpui-ce (parity list)

Grouped by area. "gpui-ce" means what its `main` exposes; bough's column names the closest thing it has.

### 3.1 Elements

| gpui-ce | bough | gap |
|---|---|---|
| `div` with 134 styled fns | `div` + ~28 | see D16 and §3.2 |
| `uniform_list`, `list` (variable height, `ListState`, scroll-to, alignment) | `virtual_list`, `measured_list`, `port_list` (raw; fixed count, `fn` rows, coordinate viewport) | a prelude list over `List<T>`/`Vec<T>`, scroll-to-item, sticky headers, dynamic count |
| `img` from `SharedUri`/path/bytes/`ImageSource`, async with loading/fallback, `object_fit`, `image_cache` | `image(&'static [u8])`, PNG only, decoded and resampled synchronously at mount (`ui.rs:140-225`) | formats (JPEG/WebP/GIF), files/URLs, async decode, `object_fit`, an LRU cache; the FNV key hashes every byte of the PNG on each mount |
| `svg` (path + color) | raw `svg_image` | prelude `svg(..).fg(color)` |
| `canvas` (prepaint/paint closures) | raw `quad`/`bars` | paint floor (D21) |
| `anchored` (popovers that flip to stay on screen), `deferred` (paint later) | raw `anchor`, `tooltip(trigger, text, rect)` (rect in coordinates) | popover/menu placement with collision, prelude tooltip without a rect |
| `surface` (CVPixelBuffer video) | none | media port (07 §3) |
| `StyledText`/`InteractiveText` (runs, highlights, clickable ranges) | `text`/`para` single style | rich text, through parley (T-items) |
| `gpui_elements`: `editable_text` (with history), `switch` | `text_field` | see T-items; checkbox, switch, slider, radio, select, progress, scrollbar visuals |
| `container_query` | none | layouts that answer the room they have |
| gpui-kit (longbridge/gpui-kit 0.9.1, 75+ components, AccessKit) *(agent-reported)* | none | a component crate written only on the prelude (the dogfood test of D4) |

### 3.2 Styling

Missing from bough's builders:

* shadows (`deco` has `Shadow` in raw);
* opacity (api boxops);
* transforms (raw `Xf`);
* gradients (raw `Grad`);
* `border_*` per side, dashed borders, outline;
* `max_w`/`max_h`, `aspect_ratio`, `flex_wrap`, insets (`top`/`left`/…);
* `overflow_hidden` in the prelude (`clip_overflow` exists), `z_index`, `visible`/`invisible`;
* `cursor_*`;
* font weight/style/family, `text_align`, `line_clamp`, `truncate`/`text_ellipsis`, `whitespace_nowrap`,
  `letter_spacing`, underline/strikethrough, text decoration colour;
* `blur`/`backdrop_blur`;
* `corner_smoothing` (gpui-ce learn example);
* `rem`/`px` units, relative sizes beyond `w_pct`/`h_pct`;
* style refinement (`Refineable`) or variants.

### 3.3 Interaction

gpui-ce has `on_mouse_down/up/move` per button, `on_click` with click count, `on_scroll_wheel`, `on_drag(value, ..)`
with typed `on_drop::<T>`, drag-over styling, `on_hover`, `tooltip`, capture-phase handlers, key context + actions +
keymaps, `track_focus`, tab stops, `on_pinch`/pressure, touch drag, long press and scroll physics (`gestures.rs`).

bough has `on_click`, `on_press`, `on_hover`, `hover_bg`, `paint_with`, `focusable`, `on_key`, `on_drag` (pointer
travel only), `scrolls`, and `no_hit` (`input.rs:574-613`).

Gaps:

* click count (double/triple click);
* typed drag payloads and drop targets;
* the capture phase;
* right-click and context menus;
* key bindings and actions;
* an IME-aware text input event;
* gestures, pressure, touch;
* scroll phases and momentum;
* horizontal wheel;
* consume/propagate.

### 3.4 Windows and app services

gpui-ce's window API has 275 public functions, including:

* bounds, fullscreen, minimize, titlebar options, transparency and blur, appearance, display, scale changes,
  activation, tabs, layer shell, RTL, touch prediction.

Its app services include:

* clipboard, including the primary selection;
* `prompt_for_paths` / `prompt_for_new_path`;
* menus and the dock menu;
* credentials (keyring);
* URL schemes, open URL;
* keymap, displays, thermal state, restart, quit hooks;
* notifications (gpui_macos), haptics, screen capture;
* an executor (foreground/background tasks), a timer, an http client.

bough exposes `Window::document(v).title(..).size(w,h)` and `Application::run` (`ui.rs`). Everything else is the
work list in §6.

### 3.5 Async

gpui-ce: `cx.spawn`, `background_executor`, `Task`, `gpui_tokio`, `async_tasks` example. bough canonical: none. api
has `Reader` with producers and cohorts, but no executor integration. The design's answer is `Reader`/`map_async`/
`.action`/`Remote` (05), so bough must not grow a public `Task` (00 §6).

### 3.6 Testing and tooling

gpui-ce has:

* `TestAppContext` / `VisualTestContext` with `simulate_click`, `simulate_keystrokes`, `simulate_input`,
  `simulate_resize`, `simulate_prompt_answer`, `simulate_new_path_selection`, `run_until_parked`,
  `capture_screenshot`;
* a profiler, a debug overlay, an inspector, and Subsecond hot patching *(agent-reported, #292)*.

bough has headless `App` with `turn`, `pointer_*`/`key_down`, `presented_pixels` vs `reference_pixels` (the counter
test uses these), `inspect.rs` record views, and `Prof` phase marks. Gaps are in §8.

---

## 4. Usability gaps (what writing the AWAGY and counter apps showed)

1. **Owner-typed helpers.** `fn record(i) -> impl Element<Dashboard>`: a row helper can't be shared between apps
   without `<V>` generics everywhere (D4).
2. **`sig!` everywhere.** `text(sig!(self.refreshes).map(|n| format!(..)))` vs the design's
   `text(self.refreshes.map(..))` / `&self.refreshes` (D2).
3. **Map closures must be `Copy`, capture ≤ 16 bytes, and run on every update** (`elem.rs:154`). Capturing a
   `String` or a theme fails with a const-assert, not a located message.
4. **No conditional children.** A toggle that shows or hides a panel can't be written; the dashboard's toggle could
   only relabel itself (D1, D5).
5. **Toggle boilerplate.** `self.enabled.set(!on)` after `let on = *self.enabled;`: no `Signal::update(|v| ..)`, no
   `toggle()`. Also, `DerefMut` marks QUEUED unconditionally (`signal.rs:73`), which matches L2, but `set` is the
   only equality-gated write.
6. **Fonts.** The default font comes from the `BOUGH_FONT`/`ZGX_FONT_FILE` env vars or Arial (`ui.rs:115-120`): no
   system font discovery, no family names, no fallback.
7. **Defaults read like debug UI.** The button is #2f6bff, the text field's built-in colours are dark-theme while
   `text_input` overrides them to white, and the button and input have no focus ring, disabled or pressed states.
8. **No placeholder in `text_input`, and Cmd-A, Cmd-C/V/X and Cmd-Z do nothing.**
9. **Sizes are points as `f32` literals.** No `px()`/`rems()`/`relative()` units, no spacing scale (`p_4`), no
   `size_full()`.
10. **`div()` is `column(())`** (`ui.rs:27`). A `div` defaults to a flex column, where CSS and gpui-ce default to
    block/flex-row with `.flex()`. Document it, or match gpui-ce.
11. **`Application::run` is `-> !`** and takes no app-level setup: no menus, no theme, no assets, no font
    registration.
12. **Errors.** Several invariants are `debug_assert!`s (`elem.rs:42`) or panics deep in the runtime: the glyph-column
    overflow seen in AWAGY was a crash until `APP_CAPS` was raised. Capacity errors should be located and, past
    construction, should grow the column or degrade instead of panicking.
13. **The `raw` vs `prelude` split** leaks: TipStyle, `Wired` in signatures, `Mounter` in list row functions
    (`fn(&V, usize, &mut Mounter<V>)`).
14. **Docs.** There is no book, no gallery, no cookbook; API.md describes rounds, not usage.

---

## 5. Performance gaps and the experiments that close them

Numbers are from this Mac unless named. Each experiment lists its gate. Preregister thresholds before measuring
(see §10, lessons).

| # | gap (evidence) | experiment / work | gate |
|---|---|---|---|
| F1 | frame per NSEvent: ~240 presents/s on 120 Hz; dashboard worst gap 75.7 ms (worst of three runs: 75.7/15.3/15.1; zgx 42.5, BUI 279.8); scroll-7K p95 12.4 ms vs iced 8.4–8.6 (AWAGY) | coalesce input into display-link turns: input mutates and marks, and the CADisplayLink (or `presentAfterMinimumDuration`) draws once per refresh; clicks are never dropped because they are queued, not raced; measure input→photon too | AWAGY: presents ≤ refresh, worst dashboard gap ≤ 2 frames, zero dropped clicks over 10 runs, p95 ≤ 8.4 ms at every row count |
| F2 | the dashboard's first-frame and anchor gaps (the 75.7 ms outlier) are unexplained | trace that run with `Prof` + `os_signpost`: shaping of 1,000 records, image decode, layout | find the cause before changing anything |
| F3 | startup: the shader compiles from MSL source at load (no Xcode, ROUND5); `MTLCreateSystemDefaultDevice` ~22 ms; zgx startup 28.5 ms vs bough 42–59 ms (API-RESULTS 1.2) | ship a build-time metallib from wgsl-rs (bough-gpu already does it for its kernels via naga → MSL → metallib), a binary archive (`MTLBinaryArchive`), device creation overlapped with mount (`prepare` exists for direct Metal) | startup ≤ zgx on the zgx startup scene, cold and warm |
| F4 | font tables: 120 M instructions per process (`SimpleScript::build`); teardown 0.46× zgx with repeats (API-RESULTS 1.1) | cache the built table on disk keyed by font hash (text3 has `build_cached`; verify it's on the default path) and a process-wide shaped-text cache keyed by (string, face, size) (BRIEF 7.3.7) | startup and teardown cells ≥ zgx |
| F5 | GPU time loses on lab (~2.2×) and vlist (~1.6×) (BRIEF 2.6); windowed animate 26.3 vs 14.5 Mcycles/s | the three designs in BRIEF 7.2.4: move zero pixels on scroll (compositor-scrolled layer or wraparound strip), a GPU-driven raster path for text-dense rows, pre-binned glyph runs (bin-scan is 47–64 % of GPU time) | ABAB GPU pairs (`bench/floor/gpu_pairs.py`) median ratio ≤ 1.0 on every scene, never run in full so far |
| F6 | mounting 7,000 rows mounts 7,000 rows (the prelude's `.scroll()`); memory and mount are O(N) | virtualization in core: offscreen subtrees of any container are stub records sized by estimate or measure (BRIEF 7.1.1); materialize on exposure; one mechanism for column, grid, tree, document | 1 M-row column and grid: mount O(visible), memory O(visible), scroll by copy kept; AWAGY non-virtualized spec still honoured by measuring both modes and labelling them |
| F7 | footprint 175 MiB vs RSS 67.8 MiB on scroll-7K | account the region columns (Caps: 1<<16 instances, 1<<19 glyphs, …), surfaces ×4, atlas; size `APP_CAPS` from the mount, grow by doubling, purge spares (trim exists) | footprint ≤ Vizia-opt and egui at every row count |
| F8 | re-description cost (D1) is unknown on canonical | measure api's lockstep walk per click on dashboard, zgx suite mount/update scenes, the inspector | ≤ 5 % on every STATUS §1 cell; a click on the dashboard ≤ 1 ms CPU |
| F9 | images: synchronous decode + resample at mount, a whole-PNG FNV hash per mount (`ui.rs:204`) | hash once per `&'static` pointer, decode off the main thread into the atlas, mipmaps or GPU resample | dashboard mount time; no first-frame stall |
| F10 | wgpu path lacks transform/deco kinds (floor) and isn't the default | finish floor's raw-encoder path, port every kind, flip the default, delete bough-metal's frame path | `bench/floor/slopes_ab.py` parity on every scene, pixel gates identical |
| F11 | layout: `parent`/`ancestors` lose to pointer chasing; `freeze_into` O(pages) (BRIEF 7.1.3) | change counter on `freeze_into`; research shows parent-column representations lost their fallback (see §10) | the STATUS cells |
| F12 | text beyond Latin-1 goes through rustybuzz per chunk on 48-byte instances (`text.rs:14-16`) | the parley `Layout` port (T1) with the grafts as fast paths; measure CJK and Arabic paragraphs and mixed scripts | zgx text_bench rich paragraphs, all faces |
| F13 | ProMotion: does bough request 120 Hz or adaptive rates? (`CADisplayLink` frame-rate range unverified) | set `preferredFrameRateRange` per animation; measure idle wakeups at 0 | no wakeups idle, 120 Hz while scrolling |
| F14 | the AWAGY numbers are one Mac, one day | rerun on the M3 Pro and a second machine; bring zgx and Blender into the same run (in progress) | the report's error bars |

---

## 6. Platform gaps

### 6.1 The host (macOS)

Verified in `crates/bough/src/backend/host.rs` and `crates/bough-metal/src/app.rs`:

* events come through `addLocalMonitorForEventsMatchingMask` (`host.rs:351`), not an `NSView` subclass, so there is
  no responder chain, no `NSTextInputClient` (no IME, no dictation, no press-and-hold accents) and no
  `NSServicesMenuRequestor`;
* the mask has no `KeyUp`, `FlagsChanged`, `OtherMouseDown/Up`, `Magnify`/`Rotate`/`Swipe`, `Pressure`,
  `TabletPoint`, `MouseEntered`, or `CursorUpdate`;
* the wheel reads `scrollingDeltaY` only (`host.rs:247`): no X, no phase/momentum (`phase`, `momentumPhase`), no
  rubber-band;
* keys map from `keyCode`/`characters` (`host.rs:206`): no F-keys, no Space distinct from `Char(' ')`, no keypad,
  no dead keys, no key repeat flag; `Key` has 14 named variants (`input.rs:45`);
* `NSApplicationActivationPolicy::Accessory` (`app.rs:101`): no Dock icon, no menu bar, no Cmd-Q/Cmd-W, no app
  switching entry; correct for benchmarks, wrong for apps;
* the scale is read once at creation (`host.scale()`); no `viewDidChangeBackingProperties`, so a window dragged to a
  1× display renders at 2× or blurry;
* no appearance (dark mode), accent colour, reduced motion, increased contrast, or text size observation;
* no occlusion observation (`NSWindowDidChangeOcclusionStateNotification`), no throttling when hidden;
* one window per run loop (INSPECTOR #8);
* resize is handled (`observe_resize`), with full-frame redraw per step.

Work: the vendored winit 0.30.13 host was never started (`vendor/README.md`). Either port to winit (gets Windows and
Linux and IME events, loses some AppKit control), or write a proper NSView host and later a winit host for other
platforms. Recommendation: winit for event/window plumbing on every platform, with an AppKit extension for what
winit lacks (menus via `muda`, IME details, CAMetalLayer present control). That is what the ROUND4 directive asked
for.

### 6.2 Capabilities (design 07 §4: Clipboard, DragDrop, Menus, Tray, FileDialogs)

None exist. Work items:

* clipboard: text, rich text, images, files; primary selection on Linux;
* drag and drop: OS file drops in, typed in-app drags, drag-out;
* menus: menu bar, context menus, dock menu; key equivalents bound to D10 commands;
* tray icon;
* file dialogs: open, save, choose folder; `rfd` or native;
* notifications, URL open, URL schemes, credentials;
* cursors: arrow, I-beam, pointer, resize, grab, not-allowed, hidden; a cursor property in the style schema;
* window: min/max sizes, fullscreen, transparency/vibrancy, titlebar styles, position save/restore, multi-display.

### 6.3 Accessibility (design 07 §5; gate T1)

None. Work: an AccessKit projection from the semantic column (BRIEF 5.3.1). That means roles, names, values,
actions, focus, live regions, text ranges for `text_input`, a scroll-into-view action, and hit-testing in
accessibility coordinates.

The archive researched this in depth: `production/accesskit`, `production/accessibility-sparse` (100 K-leaf
projection update from ~77.7 ms to 73.6 µs for one field), `accessibility-selection-sol`, `codex/a11y-*`,
`zbus6-atspi-*`. Port those findings rather than starting cold.

gpui-ce's open #282 (a11y click hits the wrong element when scrolled) *(agent-reported)* is a test case to copy.

### 6.4 IME (design 06 §3, 07 §4)

None. The protocol: marked text and preedit styling, the candidate rect, replacement ranges, UTF-8/UTF-16 agreement,
the surrounding-text revision, commit-on-blur.

The archive has 15+ `ime-*`/`codex/ime-*`/`luna/x11-ime-floor`/`deferred-xim-sol` branches. Read their dispositions
before designing.

### 6.5 Other platforms

| platform | gpui-ce | bough | path |
|---|---|---|---|
| Windows | gpui_windows (13 K) | none | winit host + wgpu (DX12/Vulkan) once F10 makes wgpu the frame path |
| Linux | gpui_linux (X11 + Wayland, 16 K) | none | winit + wgpu Vulkan; AT-SPI via AccessKit; IME via text-input-v3/XIM (archive research) |
| web | gpui_web | none | wgpu WebGPU; the region-in-a-buffer design maps to a storage buffer |
| iOS/Android | none | none | not now |

The Metal-specific pieces that need a portable answer:

* the IOSurface surfaces;
* `storageModeShared` region wrapping;
* the shared-event pre-armed stream;
* `presentAfterMinimumDuration`;
* the public/private Metal APIs the ROUND4 directive asked to use;
* subgroup ops (wgsl-rs grafted).

---

## 7. Text gaps (design 06; BRIEF 7.3)

| # | gap | work |
|---|---|---|
| T1 | one font file per face, no family lookup, no fallback; fontique is vendored and unused at runtime | **the façade's text on parley `Layout` + fontique**, with the grafts as fast paths for covered runs (the twin tests already prove the grafts are layout-identical) |
| T2 | no bidi, no RTL layout, no vertical text | parley gives bidi; RTL environment (07 §6) mirrors layout via cephal logical edges |
| T3 | no rich text in one paragraph (weights, sizes, colours, links) | parley `RangedBuilder`; `StyledText` equivalent |
| T4 | no emoji / colour glyphs (COLR/sbix) | swash colour outlines or bitmap strikes into the atlas (premultiplied) |
| T5 | editor: caret only; no selection, word/grapheme/paragraph motion, Cmd/Option/Shift motions, double/triple click, select-all, clipboard, undo/redo, placeholder, password, multiline, scroll-into-view | `PlainEditor`-backed `text_input` and `text_area` (BRIEF 7.3.2), wired to D12's interceptor and history (D20) |
| T6 | no selectable static text (copy from a label) | selection across elements (06 §4) |
| T7 | no ellipsis, line clamp, text-align, letter-spacing, decoration | style fields (D16) |
| T8 | variable fonts: axes beyond the instance at build | parley + skrifa |
| T9 | hinting/gamma: glyph rasterization has never been compared with CoreText | pixel comparison vs CoreText and zgx (the archive notes ZGX glyph raster differs) |
| T10 | Menlo/AAT through harfrust (BRIEF 7.3.3); kerning across segments beyond Latin-1 | as BRIEF |

---

## 8. Testing, tooling, inspector, diagnostics

| # | gap | work |
|---|---|---|
| Q1 | no CI (no `.github`), nightly pin, absolute paths in `crates/bough/Cargo.toml:40`, `crates/bough-text/Cargo.toml:20-21`, and five comparison manifests | make dev-deps optional behind a `bench` feature or env-relative paths; macOS CI with `cargo test --workspace`, clippy and pixel gates; a stable-toolchain build with reflection off |
| Q2 | the design's test surface (10 §2) is absent | `Win::demo(v)`, turn-barrier inputs (`type_text`, `click(target)`, `press(key)`), semantic queries (`contains_text`, `role`, `focused`, `bounds(target)`), a virtual clock, `app.until(..)`; built on `Target<K>` (D11) and the a11y projection (6.3) |
| Q3 | gate registry (10 §1: surface, reactions, async, platform bar T1–T6, colour C, media, memory M, fast path FP, text, history) not implemented as tests | one test module per family; the platform bar on every backend |
| Q4 | inspector: a headless prototype in `comparisons/bough-zgui/src/inspector.rs` on raw parts; no window of its own (one window per loop); state panel waits for reflection; transitions not addressable | merge api reflection; multi-window; the UiSnapshot contract (10 §3); write the inspector on the prelude only and log every friction (its purpose as dogfood) |
| Q5 | reflection (api) can't open enums (no active-variant index from `type_info`), schemas truncate at 256 nodes, `TypeId::info` ICEs on tuples in this toolchain (api commits an ICE dump `rustc-ice-2026-11-01T19_06_11-43241.txt`) | enum variants through a per-enum discriminant reader generated at compile time (when rustc exposes variant→discriminant) or a fallback `Debug` parse; lift the 256 cap; report the ICE upstream; delete the dump from the branch |
| Q6 | diagnostics (10 §4): no "why did layout run", no attributed impact log, no mount-cycle churn report | after D14 |
| Q7 | profiling: `Prof` marks + instruction counters; no trace export | Perfetto/Tracy or `os_signpost` output per turn phase |
| Q8 | hot reload | out of scope until the API settles; gpui-ce uses Subsecond *(agent-reported)* |
| Q9 | docs: no rustdoc pass, no book | after the API settles |

---

## 9. Soundness and hygiene

| # | issue | work |
|---|---|---|
| S1 | `App` has pub fields (`owner`, `rt`, `gpu`, `prof`, …; `app.rs:128`); `Rt` ~90 pub fields (`rt.rs:352`); `layout.rs` exposes cephal internals | crate-private; expose a typed inspector API instead (inspect.rs is the start) |
| S2 | 503 `unsafe` blocks; closures written unaligned into 16-byte recipes and read back by transmuted trampolines (`elem.rs:139-160`, `input.rs:90-110`); row `fn` pointers transmuted through `usize` (`elem.rs:698`, `mlist.rs:413`) | Miri on the headless gates (the archive ran Miri on its kernels: `codex/miri-*`); replace the `usize` transmutes with typed slots; audit reflect.rs vtables (BRIEF 7.7) |
| S3 | `Box::leak` of every font (`text.rs:42`, `text.rs:142`) | an owned font store with `'static` only where the atlas needs it |
| S4 | capacity overflow panics (glyph column 65,616 > 65,536 in AWAGY) | grow or degrade; never panic after construction |
| S5 | `sig!` soundness relies on `offset_of!` + owner boxing; the owner must never move (`Box<V>`) | keep under D2's schema proof; a compile-fail test corpus (exists in signal.rs docs) extended to the new spelling |
| S6 | thread-safety of the publisher thread and shared-event stream | a loom or stress test of the publisher handoff |
| S7 | `rustc-ice` dump committed on api | delete when merging |

---

## 10. What the research branches already settled (and how to measure like them)

The archive's clean-room GPUI campaign (141 `local/*` branches indexed 2026-10-28, plus `production/*`, `codex/*`,
`ime-*`, `accessibility-*`, `inspector-*`, `text-*`, `spike/*`, `hummingbird-*`) ran on Linux software Vulkan
(llvmpipe). Its results don't transfer as numbers, but its method and several negative results do.

**Method to adopt.**

* Preregister the workload matrix and thresholds before measuring.
* Keep a deliberate negative control per experiment (a mutant that must fail the gates).
* Report every cell.
* Reject a candidate when any fixed cell regresses past its limit (typically 1.05×), even with a large aggregate win.
  Its `local/text-witness-replay` was rejected for a 15 % gain against a 20 % target; `local/gpu-pixel-uniform` was
  rejected despite 8.33× on stripes because flat scenes got 16.6 % slower.
* Pixel-exact oracles against the full renderer, kept callable.
* Separate CPU, GPU, startup and memory claims.

bough's rounds measured per branch on a loaded machine and reported wins. Adopt the archive's discipline for F1–F14.

**Results to carry over.**

* Retained GPU output (copying a retained image after repairing damage) wins sparse expensive scenes and **loses
  flat and full-damage scenes** (`architecture-against-zgx.md`). bough's scroll copy saw the same in vlist ("a
  full-window copy costs 0.215 ms against a dense redraw's 0.249 ms", BRIEF 2.6). Any copy-based scheme needs a cost
  model that picks direct vs retained per frame.
* Parent-column / admitted-walk tree representations won deep walks 2.5–4.1× and **lost shallow or fallback paths**
  (`local/tree-parent-columns` scalar fallback 6.88× slower; `tree-admitted-walk` shallow +9.86 %). bough-core's
  `parent`/`ancestors` gap (F11) should be attacked with a representation that has no slow fallback, or not at all.
* Sparse text batching and question-source sets: 8–18× primary wins, rejected for +5 % on small dependent cells. Small
  cells are where bough's per-wire fixed costs show; measure them.
* Region batching and precise scroll repair were rejected (110/112 cells passed, 2 at 1.05).
* Native diagnosis: the clean-room GPUI was raster-bound (llvmpipe 3.8 cores), and ZGX completed 479 updates per
  8 s run where GPUI managed 144–453. Rendering work, not traversal, dominated. bough's GPU column (F5) is the same
  lesson on real hardware.
* Text caches trade memory against CPU: session cache 3.3 MB, bounded cache 45 KB with two CPU cells over 5 %. Size
  F4's caches with both measured.
* A11y sparse projection (77.7 ms → 73.6 µs per field), retained damage repair (21.64 ms → 206.7 µs for a moving
  caret) and packed children (40 → 24 bytes/node): these were adopted in the archive's production branches and are
  the baseline bough must beat, not ignore.

**Branch families bough hasn't mined yet** (read their final dossiers before designing the matching item):

* `production/{accesskit, accessibility-sparse, focus, semantic-metadata}` → 6.3;
* `ime-*`, `codex/ime-*`, `codex/component-ime-*`, `luna/x11-ime-floor`, `deferred-xim-sol` → 6.4;
* `text-selection-*`, `monotonic-caret-*`, `caret-*`, `acknowledged-text-*` → T5;
* `inspector-{ui,pointer,contained-owner}-sol` → Q4;
* `perceptual-gradient-*`, `designer-gamut-*` → colour (07 §2);
* `production/{images, image-axis}` → F9;
* `production/{canvas, canvas-runs, canvas-parallel}` → D21;
* `codex/{reactive-styles, retained-style-*, style-batch, style-dimensions, counted-style-palette}` → D16;
* `production/{remote, remote-guard, question-mailbox, reader-question}`, `reader-ticket-*` → D19;
* `codex/wgpu30`, `static-shader-stage-ownership`, `wgsl-buildtime-sol` → F3/F10;
* `webview-composition`, `window-background`, `codex/platform-window-background-appearance` → 6.1/6.2.

---

## 11. Integration and merge debt

| branch | state | action |
|---|---|---|
| `api` (ae360b7) | List, Entities, Reader, `changed`, re-description (`FIXED`), boxops, select, lanes, reflection, listview (virtualization as exposure), 16 zgx suite ports; 182 tests | merge into canonical with `FIXED` removed (D1/D1a), resolve against input/text3 (already merged into api at 3b2cf629), re-run STATUS cells A/B; delete the ICE dump |
| `floor` (db078e0) | bough-gpu raw-encoder path on vendored wgpu-hal; kernels lack transform and deco kinds | finish (F10) |
| `bough-integrate5`, `memory`, `startup`, `surface` worktrees (3abf8404d6) | round-5; integrate5 holds 305 uncommitted files from another session | leave untouched until its owner commits |
| `comparisons/bough-{zgui, heavy, scenes, text-bench, vlist}` | ~45 files on `bough::raw` | port to the prelude once D1–D5 land; until then their numbers measure the kernel, not the application API, and must say so |
| `bench/baselines/2026-11-04/` | untracked in the main checkout | owner unknown; leave |

---

## 12. Experiments to run (in this order)

1. **X1: schema-proven `&self.x` wires** (D2). Prototype `IntoProp for &Signal<T>` that looks the address up in
   `reflect::schema::<V>()`. Gates: the `sig!` compile-fail corpus re-expressed as runtime-rejection tests (enum, union
   and Box paths must become constants, never wires); mount cost per operand ≤ 20 ns; Miri clean.
2. **X2: re-describe always** (D1). On api, set every view re-describable, auto-lower plain-field-free subtrees to
   templates (D1a), measure F8. Kill criterion: any STATUS cell > 1.05×.
3. **X3: owner-free `Element`** (D4). A branch where `Element` loses `V`; routes check TypeId at mount. Measure
   dispatch and mount; port the counter, AWAGY apps and one zgx suite scene.
4. **X4: display-paced input** (F1). Coalesce in the host; rerun AWAGY dashboard and scroll ×10.
5. **X5: metallib at build time from wgsl-rs** (F3). Port `fs_bin`/`fs_tile`/`fs_copy`/`cs_*` to wgsl-rs, compare
   GPU pairs and cold start.
6. **X6: parley `Layout` under the façade** (T1). Behind a feature: labels and paragraphs through parley with the
   grafts as the covered fast path; gates are the existing text cells and twin tests, plus CJK, Arabic and emoji
   pixel tests.
7. **X7: virtualization as stub records in the Bough** (F6). 1 M-row column and grid, nested scrolls; compare api's
   per-region exposure.
8. **X8: zero-pixel scroll** (F5). A tall compositor layer vs a wraparound strip; vlist GPU pairs and footprint.
9. **X9: AccessKit projection** (6.3). Start from the archive's sparse projection; VoiceOver smoke test plus the T1
   gate.
10. **X10: winit host** (6.1). Event parity with the AppKit monitor, IME events, multi-window, startup delta.
11. **X11: a component crate on the prelude only** (dogfood). Checkbox, switch, slider, select, tabs, popover,
    context menu. Log friction as INSPECTOR.md did.

---

## 13. Roadmap, mapped to the design's phases (11-roadmap)

* **Phase 4–5 (reactive kernel, public surface):** X1, X2, X3, the api merge, `Target`/`Handoff`/`Delta`, `.with`,
  commands, the test surface (Q2), hygiene S1. *This is the next round.* Nothing else should change the public API
  until it lands, because every other item builds on its spelling.
* **Phase 2 debt (layout, dispatch, a11y):** X9, the winit host X10, the capabilities in 6.2, cursors.
* **Phase 3 (retained fragments, damage-aware presentation):** X4, X5, X8, F7, F10.
* **Phase 6 (collections, text, motion, history):** X6, T5 editor, X7, D15 motion, D20 history.
* **Phase 7 (style schema, media, canvas/shaders, colour):** D16 tokens and variants, D21, colour spaces, images F9.
* **Phase 8+ (editor kernel, workspace boundaries, plugins):** D23, multi-window workspaces.

The milestone the design calls "surface complete" requires gates T1–T6 on every supported backend; bough today has
one backend and none of the platform bar.

---

## 14. The rest of the design, read in full (2026-11-11, second pass)

The first pass read 00–04, 07 §4–7 and 10 §1–3. This pass read the remainder of `design/final` (05, 06, 07 §1–3,
08, 09, 10 §3–5, 11 Phases 0–4) and the supporting documents that carry the user's own notes:
`gap-register.md` and `performance.md` in `~/Downloads/designs/reactivity`. These are the gaps that pass adds; IDs
continue the families above.

### 14.1 The user's notes in `gap-register.md` (each is a requirement)

| note (verbatim, on gap) | item |
|---|---|
| S2 Vello/Vulkano RFC: "THIS IS INTERESTING JUST BECAUSE OF THE POINT MADE BY tronical ABOUT A BETTER WAY TO SCREENSHOT TEST, WE SHOULD SWITCH OUR SCREENSHOT TESTING FOR THIS" | **Q10**: tronical's point (slint#11451) is to test *serialised low-level scene descriptions* instead of screenshots: "higher level and more robust than screenshots (see the troubles we've had with text and skia and cross-platform output) but low-level enough to catch mistakes". It is a proposal on top of anyrender-serialize, not shipped. Slint's suite today compares against its software renderer with a near-exact threshold (0.1 max RGBA distance); Vello compares GPU with CPU by NV-FLIP (mean < 0.01). bough already has the scene description: the Bough's preorder records (kind, ink, clip, handle) plus instance and glyph-run words in the region. **Work:** a serialised scene snapshot per frame as the primary gate, renderer- and OS-independent, covering logical→physical, gradients and transforms; plus a CPU reference rasteriser with an NV-FLIP tolerance for the raster itself. Today every pixel gate compares `presented_pixels` with `reference_pixels` from the same GPU path, which proves damage exactness, not raster fidelity |
| S3 hinting + bi-level text (zed#55214, blocked on skrifa): "DOES PARLEY FIX THIS?" | **T11**: partly. skrifa (bough vendors 0.44.0) has the hinting engine: `HintingInstance`, `HintingOptions { engine, target }`, `Target::Mono` (bi-level), `Target::Smooth { mode: SmoothMode::{Normal, Light, Lcd, VerticalLcd} }` (`vendor/skrifa/src/outline/hint.rs:92-280`), so zed#55214's "blocked on skrifa" looks out of date. parley 0.12 exposes no hinting option or hinted advances (agent-reported, uncertain). bough's atlas rasterises with swash, unhinted, A8 only. **Work:** rasterise atlas glyphs through skrifa's `HintingInstance` (mono and light targets) and feed hinted advances into the grafted shaping kernels as a font-instance option, so layout and raster agree |
| S3 pixel snapping (slint#10962, zed#40641): "WOULD BE GREAT TO HAVE" | **T12**: a snapping policy. Slint#10962 wants absolute x/y/w/h at exact physical-pixel boundaries (tronical's workaround rounds to `1phx`). Chromium snaps at paint: round the origin, derive the far edge from origin + rounded size, and leave scroll offsets and tile phase unsnapped. WPF's `UseLayoutRounding` rounds in layout. bough snaps quad geometry to a 1/256-px grid for exact damage (`rt.rs` `snap`) and rounds spaces to device px, but has no snapping policy for borders, rules or text baselines at fractional scale. **Work:** paint-time physical-pixel snapping, on by default for borders, 1-px rules and baselines, off for transformed subtrees; `.snap(false)` per element |
| S4 system menus, tray, file dialogs: "THIS NEEDS TO BE CLEAN AND ELEGANT AND CROSS PLATFORM" | 6.2's capabilities, in the design's form: handler parameters (`fn open(&mut self, dialogs: FileDialogs)`), named-command menu entries, never ambient |
| S4 IME as a public API: "READ THROUGH THIS" | 6.4 now cites 07 §4's protocol in full (below, 14.4) |
| S5 event-loop ownership / interop: "READ THROUGH THIS" | D22: `app.attach(host)` / `app.poll(deadline)` driving the same turn queue (09 §7, 07 §7) |
| Slint/Iced: global/context state: "read through this, think we resolve" | resolved by the design (no globals, no `cx`; `Entity<T>` and inherited sources); bough also has none. No item, but the design's inherited-environment sources (07 §6) are still unbuilt (PL-items) |

Unannotated gaps from the same register, all absent in bough:

* window transparency/vibrancy/Mica, shadow and corner policy, control flags, window focus lifecycle (S1);
* a software/reference renderer, device selection and device-loss recovery (S2);
* glyph-atlas sampler filter, a public text-metrics query (S3);
* per-output DPI on monitor move, display enumeration at startup (S6);
* layout-level RTL mirroring, downward-inherited properties (a subtree-wide `disabled`), a virtual keyboard (Slint/Iced).

### 14.2 Async (05): what `Reader` still lacks on the api branch

api built `Reader<T>` with `produce(src, dst, fn(&S) -> Request<T>)`, tickets, cohorts committed in one epoch,
`latest`, and `boundary` (loading/failed/child by clip rects). Against 05:

* **A1** `signal.map_async(f)` / `try_map_async` / `compute` (CPU-pool twin) as the creation spelling, with the
  executor chosen once at creation: an injected runtime handle (Tokio, smol or bough's own), `map_async_on`, the
  `StampedCompletion` wake law (a `Waker` only schedules a poll). api's `produce` takes a synchronous `Request`
  constructor and has no executor integration.
* **A2** `reader.project(|&T| El)`, `map_ref` into a property port, `Settled<T>` via `reader.or(fallback)` (the one
  sound `Deref`).
* **A3** `boundary().reload_on(&input)`, first-readiness semantics, and the inspector's "declaration waterfall"
  report.
* **A4** `is_pending(x)` as a source.
* **A5** the failure table (domain `E` vs superseded vs worker fault vs plugin panic) with heal-on-retry.
* **A6** `RequestStamp` with frozen input versions (the `A → B → A` alias test).
* **A7** the action shape: `.action(Owner::persist.with(..)).queue(Queue::Reliable).pending(..).then(..).catch(..)`,
  with optimistic tagged deltas.
* **A8** `Remote<T>` streams with explicit lanes (`Reliable`/`Latest`/`Coalesce`/bounded), `try_send` → `Full`,
  prefix-loss-only delivery.
* **A9** the entanglement rule (§9.4) as a property test: a region reads a cohort-held input's retained operand and a
  live input's current value.

### 14.3 Text (06): the model bough must grow into

* **T13 One inline formatting context.**
  * Glyphs, spacing, decorations and atoms (U+FFFC placeholders with baseline/align/break/skip-decoration) share the
    line boxes, caret and hit test.
  * Three style sheets (font/shape, paragraph/wrap, paint/decoration) carry `Impact`.
  * The order of operations is fixed: transform → white-space collapse → shape → spacing → break → hang/indent →
    align → paint. Kerning stays inside shaping; tracking is a separate step.
  * bough has single-style labels and paragraphs.
* **T14 Decoration layers** (document < semantic highlight < IME < selection < find) with skip-ink, as opposed to a
  single optional underline.
* **T15 `rich(&pieces).atom(view_fn)`** over a `List<Inline>`, with marks as interval annotations, not a span tree.
* **T16 FlowIndex selection.**
  * `.selectable(Select::Contain/Exclude)` gives cross-element selection over labels; copy takes a `StaticRange`
    snapshot.
  * Document indices are always bytes; transform, hyphenation and bidi keep mapping tables.
* **T17 The editor kernel.**
  * Branded `Pos<Space>` coordinates (Buffer/Inlay/Fold/Tab/Wrap/Block/Crease/Display/Multi), so `Px` is its own
    type.
  * The display map is a mounted, async-maintained projection behind `code(&document)`, with an async `WrapDemand`.
  * Blocks and inlays are ordinary elements at typed positions.
  * `MultiBuffer` composes excerpts, and N viewports run over one document.
  * Nothing of this exists. bough's `code` port (api branch, zgx's code app) is a scene port, not this kernel.
* **T18 Text raster policy as data** (hinting, AA, atlas sampling filter, pixel snapping), the seam T11 and T12 need.

### 14.4 Platform (07 §1–4): beyond 6.1–6.4

* **PL1 Window vocabulary.**
  * `Window::utility(..).frame(..).background(..).material(Prefer::new(..).fallback(..)).shadow(..).corners(..).controls(..).level(..).hit_test(..)`.
  * Each capability is `Prefer<T>`/`Require<T>`, never silently omitted.
  * `Win<V>` exposes live readers (`active`, `bounds`, `scale`, `display`, `appearance`) and queued verbs
    (`activate`, `minimize`, `start_move`, `request_attention`).
  * bough has `.title().size()`.
* **PL2 Renderer selection and recovery.**
  * Selection: `Application::renderer(Auto | <backend> | ReferenceCpu | fallback list)`.
  * Device loss keeps the CPU scene, re-uploads leased resources and publishes `Recovered` or a typed fault.
  * Frontends are negotiated from a capability set; one backend selection per packet, pass or range.
  * bough is Metal-or-nothing.
* **PL3 Colour.**
  * CSS Color 4 constructors (`Color::Red`, `oklch`, `oklab`, `color(Space::DisplayP3, ..)`), alpha via `.alpha(t)`
    (packed `rgba(0xRRGGBBAA)` is deleted), and `mix` in oklab by default.
  * The working space is premultiplied linear extended sRGB; surfaces are `SdrSrgb8` (default) or `ExtendedSrgb16`
    (HDR, opt-in).
  * Gate C10 is zero-tax: no float surface unless requested.
  * bough's `Color` is a packed `u32` (`color.rs`, 14 lines), with sRGB-encoded blending in the kernels.
* **PL4 Temporal graph** (`post_process(|g: TemporalGraph| ..)`: history textures, pass DAG, known vs inferred
  motion). Absent; low priority.
* **PL5 Media** (`IntoMedia`).
  * Inputs: `RgbaImage`, `DynamicImage`, `Png(bytes)`, `asset!("logo.svg")`, `Uri::get(url)`, a `Reader<Frame>`.
  * One staged pipeline: acquire → sniff → decode → orient/scale/colour plan → upload/residency.
  * Presentation: `.fit/.sampling/.fg/.tint/.slice/.tile/.crop/.flip`.
  * Split, budgeted caches; animated images and video as timed streams.
  * bough has `image(&'static [u8])` (PNG).
* **PL6 The IME wire protocol.**
  * Target → platform: `Enable/Update/Disable(InputMethodState)` carrying a surrounding-text snapshot and revision,
    UTF-8 and UTF-16 selection, preedit styling, the replacement range, the cursor rect in window coordinates,
    purpose, capitalisation and direction.
  * Platform → target: `Preedit/Commit/DeleteSurrounding/SetSelection/Candidates` tagged with the revision.
  * Gate T2 runs a scripted IME on every backend.
* **PL7 Capabilities are handler parameters** (`fn copy(&mut self, clipboard: Clipboard)`), not ambient context.

### 14.5 Runtime (08): mechanisms bough hasn't built

| design | bough | item |
|---|---|---|
| turn pipeline (08 §1): handler → drain sends → drain completions → speculative describe → seal producers → settle cohorts → publish atomically → layout/hit/a11y/paint | `turn`: handler → settle wires → (api) `changed` waves → re-describe → layout | **R1** speculative description and atomic publication (`LogicalDraft`/`PresentationDraft`, layout inverse-op journal) |
| priority lanes Input/Animation/Normal/Idle; idle work aged and preemptible; input never behind cache warming | one lane | **R2** |
| teardown/eviction protocol: owner epochs, revoke before reclaim, virtualised rows holding IME/capture unevictable | api: two-phase release (cut now, reclaim in idle frames) | **R3** the eviction veto for rows with active IME composition or pointer capture |
| memory classes: steady-state **zero** allocation asserted per phase by a debug allocator; novelty page-granular and counted; every cache budgeted | region columns fixed at construction (`Caps`), capacity panics; caches unbudgeted | **R4** a `MemoryLedger` per window, a debug allocator hook, a pressure cascade, high-water trim; gates M0–M6 |
| fast loop: a `PresentationResolver` (hit index + precomputed hover/active/focus variant deltas + scroll table) handles pointer and wheel with zero graph work | hover is a paint write (`hover_bg`), wheel goes through the scroller; both run inside `dispatch_event` + `frame` per event | **R5** the resolver as its own published structure, with gate FP1 (`turn_settles == 0`) |
| `Display` into the retained buffer (derived text writes into the port's buffer, byte-compare) | `NumFmt` numeric operand (16-byte vector) for numbers; strings allocate | **R6** generalise to `impl Display` |
| `SharedString` 24 B SSO; closure storage 3 words with counted spill | routes and map closures: 16-byte recipes, a compile-time panic past that | **R7** counted spill instead of a hard limit (also fixes usability 4.3) |
| const subtree templates (08 §7) | `template`/`ltemplate` (raw, runtime) | D1a |
| frame pipelining with a latency lane | the pre-armed stream + publisher thread (bough-metal), no latency lane for input during animation | **R8** |

### 14.6 Boundaries (09)

* **B1 A non-generic `EntityCore`.** Per-type method/drop/describe tables, so a new `Entity<Foo>` doesn't
  monomorphise the scheduler. `Entity::new_cyclic(|me| ..)`, generation-checked `send` → `Delivery::Closed`. No
  `Deref` (api's `Entities<T>` is a slab with generational handles; check that it has no `Deref` and add
  `new_cyclic`).
* **B2** open item sets through `typetag`; durable identity in three layers (`kind` + framework-minted
  `ItemInstanceId` + item-owned `locator`/`state` bytes); `Target`/`Slot` never serialise.
* **B3** presence as a supervised `Remote<PresenceEnvelope>` session (collaboration); far off.
* **B4 Linking gates.** Generic instantiation count, incremental rebuild time after an app-only edit, clean link time,
  artifact size, exported symbols (gate 10). bough has never measured compile times. Its heavily monomorphised
  builder types (`On<LBox<Color,(Para<S>,)>, HoverBg>`) make this a real risk.

### 14.7 Quality (10 §3–5) and the reference workloads (`performance.md` §4)

* **Q11 Diagnostics are part of the sealed surface.**
  * Every sealed trait gets `#[diagnostic::on_unimplemented]` text in the design's vocabulary and
    `#[diagnostic::do_not_recommend]` on its blanket impls.
  * A `trybuild` corpus asserts the *message text* of the top mistakes: wrong layout-family method, an escaped
    element, a by-value signal, a captured `&self` in a projection, a mismatched route receiver, a read-only source
    in a two-way port.
  * bough has two `compile_fail` doctests on `sig!` and no message assertions.
* **Q12** the `UiSnapshot` contents (10 §3) as one serialisable schema per epoch, which today is
  `inspect.rs`'s record views plus api's reflection.
* **Q13 "Surpasses" (10 §5)** means all of:
  * no feature reduction;
  * no worse slope on any locality workload;
  * lower p50 **and** p99 input-to-present;
  * no unaccepted regression in cold start, memory or incremental compile;
  * no hidden work migration to upload bandwidth or threads;
  * reproduced on two hardware classes.

  bough's claims so far are instruction counts and AWAGY presentation cadence on one Mac. Input-to-present p99 has
  never been measured.
* **Q14 Reference workloads not yet built** (`performance.md` §4 lists 31):
  * #3 hover storm at 120/240 Hz;
  * #5 100 k keyed table (value/insert/remove/move/sort/filter/replace, work vs N and K);
  * #7 editor (large rope, multi-cursor, IME echo, a huge bidi paragraph, p99);
  * #8 kitchen sink;
  * #9 animation tiers by phase;
  * #12 adversarial dependency graph;
  * #13 developer loop (clean/incremental compile, rust-analyzer latency, binary size);
  * #14 capability-ergonomics compile-fail suite;
  * #15–17 property lowering matrix and a 10 k-consumer palette swap;
  * #23 retained drag protocol;
  * #24 placement graph;
  * #25 motion semantics;
  * #26 canvas/shader graph;
  * #27 inspector-disabled zero cost;
  * #28–30 image/video/frontend equivalence;
  * #31 history.

  Partly covered: #1 idle (zero frames, measured), #2 counter locality, #4 scroll split (zgx scenes, scroll by copy),
  #6 variable-height list (zgx vlist), #10 teardown (zgx suite), #11 cold start.

### 14.8 Roadmap Phase 0 is not done

11 §Phase 0 ("freeze the contracts") requires:

* conformance fixtures;
* a deterministic headless clock and event traces;
* a first `UiSnapshot`;
* a counter registry and frame-trace format;
* reproducible benchmark runners;
* pinned competitor ports;
* a `trybuild` harness for the diagnostics corpus;
* a panic-free lint baseline (`unwrap`/`expect`/indexing denied in kernel code, existing violations allow-listed with
  a burn-down file).

bough has the competitor ports (zgx, zgui, AWAGY) and instruction counters; the rest is missing. Phase 1's
"legacy-view island" doesn't apply: bough is not migrating an existing GPUI app.

### 14.9 From `comparison.md` and `approaches/` (A–D)

* **S8** The design says "stable Rust is enough for every required semantic; nightly features are optional
  experiments". After the api merge, bough's façade crate requires nightly for every build: it enables
  `#![feature(type_info, ptr_metadata, rustc_attrs, const_trait_impl, const_cmp, arbitrary_self_types_pointers)]`
  unconditionally. **Work:** gate `reflect.rs` (and the inspector's typed state panel) behind a `reflect` cargo
  feature that is on for the inspector and tests, off by default, so the default build is stable.
* Approach **C** (retained attribute graph) is the substrate the design selected, and bough's records + wires are
  that substrate. Its rejected parts are not in bough: universal MVCC, and durability as a type parameter.
* Approach **A**'s surviving ideas are also absent from bough: copyable `State<T>` keys resolved through
  `Scope`/`Update` capabilities, no ambient runtime, one sealed `Property<T>` installer. bough's `Wired` operands
  are the nearest equivalent (D2).
* Approach **B** (a public branded `'ui` lifetime) is rejected for the public surface; bough agrees (no brands in
  user code).
* Approach **D**: `tw!` is the one sanctioned macro (D16); a `#[component]` macro is optional and last.

---

## 15. Failed and rejected work: what merits another try

Every rejection below was read from its own dossier or results section. "Retry" means the reason it failed no
longer holds in bough, or the representation that would change the verdict has not been tried. Ordered by expected
value.

### 15.1 Worth retrying

| # | experiment (where) | why it failed | why retry, and how |
|---|---|---|---|
| RT1 | **Hidden-flip presentation**: show one of N fixed IOSurface layers, hide the rest (`crates/bough-metal/RESULTS.md` §12.3) | Rejected as unsafe: with fixed contents the compositor keeps all four surfaces `IOSurfaceIsInUse`, so nothing says when a hidden one may be redrawn. It was 25 K instructions/publish cheaper than a contents swap | **zgx ships exactly this** (`zgx-gpu/src/layer_present.rs`: "each surface is a tile of the plain layer, its contents set once; a frame shows its surface's tile and hides the one before", handed over at a fixed point before each refresh), and the AWAGY run verified one tile flip per shown frame (319/319 against zgx's own log). zgx makes it safe by timing (the refresh grid) instead of an in-use signal. Retry with zgx's handover schedule plus a tearing detector: stress the flip with full-surface writes and compare screen captures against the intended frames. Worth +25 K/publish, and it removes the swap's transaction |
| RT2 | **2D binning with a parallel scatter** (bough-metal RESULTS §13: "the next structural step") | The v7 per-tile list failed (sequential row-bin scan); never built | The GPU column is bough's main loss (lab 1.7×, vlist 1.5×; BRIEF 2.6). Glyph-dense rows make each tile scan ~230 row-bin entries for ~5 glyphs. Count + prefix + scatter per (row, column chunk) is the representation change the analysis named. Highest-value GPU experiment |
| RT3 | **Copy vs direct, chosen per frame** (archive `local/architecture-performance` retained output; bough scroll by copy; BRIEF 2.6 "a full-window copy 0.215 ms vs dense redraw 0.249 ms") | Retained copy wins sparse scenes and loses flat/full-damage ones; bough's scroll copy can't win vlist | Both campaigns lost on a *fixed* policy. `App::copy_plan` already knows the exposed area and the surface age. Build the cost model the archive listed as priority 1 (copied pixels vs repaired tiles vs glyph density), preregister the crossover workloads, and choose per frame. Cheap to try |
| RT4 | **Zero-pixel scroll** (BRIEF 7.2.4; "multiple small layers" rejected in bough-metal §12.3 at ~37 K per changed layer) | Many layers cost a commit each | One tall layer scrolled by `contentsRect`/position is a single layer change per frame, not N; it was never measured. Pairs with RT1 |
| RT5 | **A mount-time shaped-text cache** (bough-text RESULTS Attempt 5: caches rejected) | Shaping (~8 instr/glyph) is cheaper than hashing and lookup *per update* | The remaining loss is whole-process **remount** (zgx suite teardown 0.46× with repeats; zgx reuses a process-global text cache). Retry the cache on the mount path only, keyed by `&'static str` pointer/length or an interned id (no content hash), never on updates |
| RT6 | **Archive mask/sample-partition GPU work** (`local/mask-grid-native` .866/.870, `mask-tiles` .923, `mask-partitions`) | Rejected for missing a **20 %** target (stream ratios 0.87–0.92 against ≤ 0.8) while every work/cadence check passed | On llvmpipe these were real 8–13 % GPU wins; bough's GPU column is losing. Classifying a tile's samples as uniform before blending matters most on glyph-dense tiles. Port only the classification idea to bough's kernels after RT2, measured with `gpu_pairs.py` |
| RT7 | **Pixel-uniform tiles** (`local/gpu-pixel-uniform`: stripes 8.33×, text-stripes 1.30×, flat +16.6 %) | The flat-scene regression | The regression was classification overhead on scenes with nothing to gain. bough's binned kernel already has an opaque floor per tile; certify uniformity per tile at bin time and fall back for free. Medium |
| RT8 | **Depth-gated parent routing** (`local/tree-parent-columns` deep 4.11× but scalar fallback 6.88× slower; `tree-admitted-walk` deep 3.71×, shallow +9.9 %) | Fallback paths regressed | bough's `parent`/`ancestors` lose to pointer chasing (bough-core RESULTS: 0.25× / 0.04×), and the inspector and a11y projection (6.3) will walk ancestors constantly. `local/tree-columns-selected` ("safe compact parent routing with topology proof and depth-gated batches") was the last candidate and was never compared. Retry as a lazily built parent column with a change counter, used only past a depth gate |
| RT9 | **Text session cache** (`local/text-session-cache`: CPU-qualified, unadopted for 3.3 MB of retained scratch; `text-cache-bounded`: 45 KB, two cells > 5 %) | The memory cost, or two CPU cells | Relevant once bough's text goes through parley `Layout` (X6): parley's `LayoutContext` scratch is exactly this. Adopt the bounded variant with the cold-trim policy (`text-cache-cold-trim`: median gates pass) |

### 15.2 Already adopted in effect, or superseded

* Archive `local/damage-suffix` (exact unchanged-suffix trimming: **all 28 fixed workloads pass**, native 479
  updates/8 s) and `local/patch-workspace` (**112/112 gates**, reusable text patch boxes) are wins for the clean-room
  GPUI's draw-list/packet design. bough has no per-frame draw list or packet to trim, because records are retained
  and the GPU reads the region. Nothing to port, but cite them as the baseline the clean-room GPUI reached.
* `local/source-batch-publication` (typed text batches: −27 % CPU for 2–4 fields among 1,000 labels, qualified)
  removes owner re-description for multi-field updates. bough's wires never re-describe for signal writes. After the
  api merge, though, a handler that writes several *plain* fields re-describes the owner (D1). The lesson (keep up
  to 4 source identities and bypass description for disjoint text sinks) applies to the lockstep walk: X2 should
  skip subtrees whose recorded reads didn't change.
* `local/list-overscan`, `local/list-source`, `local/native-list-parity` (qualified keyed overscan, controlled list
  offsets): api's listview has overscan and anchored offsets; compare its gates with these dossiers' negative
  witnesses.

### 15.3 Correctly dead (don't retry without a new representation)

* Per-row hover signals (O(rows) per crossing); a separate hit index (page summaries already are one);
  swallowing content events (INPUT.md §4).
* Per-column bar strips: 32–65 K/tick for −30 % tiles (bough-metal RESULTS).
* GPU shaping: exact but not cheaper (bough-text Attempt 3).
* Ligatures inside the lanes (bough-text III.4).
* CAMetalLayer drawables, CAMetalDisplayLink and AVSampleBufferDisplayLayer as present paths: +292–700 K/frame
  (bough-metal §12.3). zgx agrees: it keeps the AVSampleBuffer path only behind a flag for comparison.
* Damage-lab interval lists, summed-area tables and bounded rect lists: lost to tile planes on CPU
  (bough-damage-lab RESULTS).
* Archive text-witness replay (15 % against a 20 % target, 10 K-label +5 %), delivery sweep (+2.6 % / +14.2 %),
  region batching and precise scroll repair (2 of 112 cells at 1.05): these are specific to the clean-room GPUI's
  rebuild-per-frame pipeline, which bough doesn't have.
* Universal MVCC, a public `'ui` brand, a general `#[component]` macro (approaches B–D).

### 15.4 bough results this session corrected

* **AWAGY "13 of 480 clicks dropped" was wrong.** In that run the input driver itself sent only 467 Refresh clicks
  (`events_sent` in `bench/awagy/results/results.json`), and bough counted all 467. The other two runs sent and
  counted 480. The report (`bench/awagy/results/bough-on-awagy.html`, verdict) and item 9 of §0 are corrected; the
  75.7 ms stall in that run is still unexplained (F2).
