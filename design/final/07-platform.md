# 07 — Platform

Scope: window chrome and compositor policy, the renderer/color/HDR contract, clipboard/drag/
menus/tray/dialogs, the IME wire protocol, accessibility as a committed-epoch artifact with a
typed visitor, theme/environment/RTL, displays and event-loop interop, the media pipeline, and
the cross-cutting protocol catalog (grids, trees, docking, gestures, container queries, live
regions, theme fanout) that sits above these platform primitives.

Depends on: [02-elements.md](02-elements.md) (phase closures for custom rendering);
[03-ports.md](03-ports.md) §5 (tokens); [06-text.md](06-text.md) (IME's document-side half).
Feeds: [10-quality.md](10-quality.md) (platform bar T1–T6, gates 16, 22, 23, PG1–PG6, and the
media gate family — 10 §1).

---

## 1 · Window chrome and compositor policy

`Window<V>` separates identity/kind, geometry, frame, compositor, controls, and activation. A
backend may report lack of support; the framework may not silently omit the concept — every
capability below is `Prefer<T>`/`Require<T>`:

```rust,ignore
Window::utility(inspector)
    .frame(Frame::Custom(titlebar))
    .background(WindowBackground::Transparent)
    .material(Prefer::new(Material::Mica).fallback(Material::Blurred))
    .shadow(WindowShadow::System)
    .corners(WindowCorners::Square)
    .controls(WindowControls::default().minimize(false).maximize(false))
    .level(WindowLevel::AlwaysOnTop)
    .hit_test(WindowHitTest::Content)
```

The vocabulary covers transparent/vibrant/Mica/blurred backgrounds, system/custom/no shadow,
corner policy, min/max/close buttons and placement, custom/native/server-side frames, movable and
resizable policy, always-on-top, mouse passthrough, native tab grouping, and parent/modal/popup
relationships. `Prefer<T>` records fallback; `Require<T>` makes unsupported realization a checked
open failure — silent platform divergence is forbidden.

`Win<V>::active`, `bounds`, `scale`, `display`, and `appearance` are live readers (`Source`
instances, 03 §1). `activate`, `minimize`, `maximize`, `restore`, `close`, `start_move`,
`start_resize`, and `request_attention` are checked/best-effort queued verbs. `Win<V>` is a
generational owned route, not a borrow of the application; a stale handle returns `Closed`, never
dangles. `AnyWin` exists only for genuinely heterogeneous storage and plugin protocols; titles are
presentation, never identity.

## 2 · Renderer, color, and HDR

Layout emits a renderer-neutral retained scene protocol. `Application::renderer` chooses `Auto`,
a named GPU backend, `ReferenceCpu`, or an ordered fallback policy; device preference and
recovery policy belong to the application. Device loss keeps the committed CPU scene, realizes
another backend, reuploads leased resources, and publishes either `Recovered` or a typed surface
fault.

### 2.1 The working contract

> **Authors write `Color`. The GPU composites premultiplied linear extended sRGB. The surface is
> a negotiated, tagged contract.**

`Color` is a tagged, `Copy` authoring type — CSS Color 4 constructors (`Color::Red`, `rgb(...)`,
`hsl(...)`, `oklch(...)`, `oklab(...)`, `color(Space::DisplayP3, ...)`) plus theme tokens. Packed
`rgba(0xRRGGBBAA)` is deleted (today's silent-truncation footgun); alpha is always `.alpha(t)`.
Mixing (`c.mix(other, t)`, default `oklab`) and gradients name their interpolation space
explicitly; `Rgba::blend`-style encoded-space mixing is not the default anywhere. The internal
`WorkingColor` type (premultiplied linear extended sRGB, unclamped) is never accepted by `.bg()`/
`.text_color()` — application code names only `Color`; the conversion is a sealed port, the same
`IntoProp` discipline as everywhere else (03 §2).

Two negotiated surface contracts:

- **`SdrSrgb8` (default)** — the ordinary 4-byte/pixel surface; gamut/tone-map the changed scene
  ranges as required and encode once to sRGB.
- **`ExtendedSrgb16` (opt-in)** — a float16/scRGB-equivalent surface tagged extended linear sRGB,
  preserving negative and greater-than-one RGB through submission; the platform compositor maps
  the tagged surface to the current display and exposes available HDR headroom.

```rust,ignore
Window::document(studio)
    .surface_color(Prefer::new(SurfaceColor::ExtendedSrgb16).fallback(SurfaceColor::SdrSrgb8))
    .dynamic_range(DynamicRange::Extended)
```

`ExtendedSrgb16` is the stable framebuffer contract, not "the monitor is P3" — a future display
gamut never adds a GPUI framebuffer enum. HDR is policy, not merely an unclamped number: a window
separately requests dynamic range, reference white, tone-map behavior, and fallback; an SDR
`Color` remains SDR even on an HDR surface. **Zero-tax rule**: the ordinary path retains today's
byte class — no float swapchain, LUT, or profile lookup exists merely because the feature is
compiled in; a window that never requests extended color has the same surface bytes, attachment
count, and pass count as a build with the feature disabled (gate C10).

A professional application additionally owns source/assigned/proof ICC profiles, rendering
intent, and black-point policy through an optional `ColorManagement` capability that realizes
CPU/GPU LUT transforms into `WorkingColor` — GPUI mandates no particular CMS. The framebuffer is
never the document's storage format; a creative canvas may keep native integer/float/profile-
encoded planes and convert only the composited result.

Screenshot tests rasterize a committed scene directly into an offscreen `ReferenceCpu` target
with fixed fonts, scale, working/surface contract, and sampling — semantic tree/scene assertions
remain the cheap default; reference pixels cover raster fidelity, and each GPU backend runs
conformance images against the reference with declared tolerances (10 §2).

### 2.2 The renderer frontend boundary

Each window connects to a renderer frontend negotiated from a typed capability set (retained
target, damage-region replay, partial present, stable GPU slots, external plane formats,
protected surfaces, HDR output, readback), never one flattering tier number. The critical rule:
**one backend selection per packet/pass/range, never one virtual call per primitive.** No core
scene type contains `Any`, a platform pixel buffer, or a backend-specific texture handle; native
providers create an opaque, generational `ImportLease` through a renderer-advertised typed
importer, and only the matching frontend can open it. Device loss increments the device
generation, rejects old imports, and asks upstream media stages for the cheapest recoverable
representation without rebuilding the logical scene.

### 2.3 The retained temporal graph

Moving a property is one kind of motion; frame differences, decay trails, motion vectors,
directional blur, onion skinning, and temporal filters need to *observe and shade motion across
frames*. That is a scoped, typed post-process capability on the same phase surface:
`post_process(|g: TemporalGraph<'_>| ...)` declares history textures (separate previous/current
resources — portable ping-pong, with read-write aliasing an explicit backend capability, never an
assumption — carrying declared first-frame/resize/profile/device-loss resets), a pass/resource
DAG validated once at mount and retained, typed compute/render passes with declared read/write
hazards and workgroup/scale policy, and a composite output. A frame only resolves typed ports,
dispatches active passes, swaps history generations after successful submission, and composites.

The graph distinguishes *inferred* motion — frame difference / optical flow over current and
previous color, honestly sensitive to illumination, occlusion, and cadence, exposing confidence
rather than promising a correct vector field — from *known* motion: velocity derived from
retained scene identity and previous/current presentation transforms, with ordinary scene nodes
emitting the attachment only when a pass requests it. GPU→CPU readback is a named async edge
with byte/rate/backpressure limits, never hidden behind a property read. Zero-tax when absent:
an application with no temporal graph has no history resources, velocity attachments, compute
dispatches, readback queues, or frame requests. Gate 22 (10 §1) binds to this subsection.

## 3 · Media

One `image(...)`/`video(...)` constructor per semantic destination, both sealed `IntoMedia` plug
families over ordinary Rust values — the closed family the design's survey audit specifically
demanded, because the alternative (base64 strings, hand-rolled canvases) is the industry's
standing failure mode:

```rust,ignore
image(&self.photo)                 // Signal<RgbaImage>: a live image port
image(rgba_image)                  // image::RgbaImage — owned pixels, zero ceremony
image(dynamic_image)               // image::DynamicImage — converted in the pipeline
image(Png(bytes))                  // encoded bytes, format-tagged; decode is staged
image(asset!("logo.svg"))          // build-time asset; vector-aware
image(Uri::get(url))               // request descriptor: fetch/decode staged, cached
image(self.thumbnails.reader())    // Reader<Frame> — video/live/provider frames
```

```text
IntoMedia ⊇ RgbaImage | DynamicImage | tagged encoded bytes | asset | path | request
          | frame provider | any Source of the above
```

Every form lowers into the *same* typed pipeline: `source recipe → acquire bytes/native samples →
sniff/demux → decode/parse → orient/crop/scale/color plan → CPU pixels, vector scene, or native
planes → renderer import/upload and residency → retained fragment → frame packet`. A provider
does not have a private fast path and bytes do not have a slow one; pipeline identity means the
same `RgbaImage` handle shown twice uploads once. **Data URLs are not an in-memory image API and
never become one** — a string form is at most a parser for foreign content. Presentation
(`.fit`, `.sampling`, `.fg`, `.tint`, `.slice`, `.tile`, `.atlas`, `.crop`, `.flip`) is
element policy, layered on top and never changing acquisition. Recoloring is not a media-only verb:
a single-channel/mask image inks with `.fg()`, the same port as a glyph or icon (03 §5), and
`.tint()` is the multiplicative modulation of a multi-channel image — there is no `colorize`.

Caches are split by lifetime and key: acquisition (HTTP identity, validators, `Vary`/auth
partitioning), parsed/decode (content + codec version), variant (size/scale/orientation/gamut),
and residency (variant + device generation) — each budgeted, evicted, and inspector-visible with
private headers redacted. Animated images are timed streams with a bounded presentation window,
never a permanently retained `Vec<Frame>`; video separates immutable source identity from
mutable playback state and shares an injected media clock with animation (03 §4). "Zero-copy" is
never claimed as a blanket guarantee — the measurable contract is *no CPU pixel copy and no extra
full-frame GPU copy* on a proven-compatible fast path, with every fallback counted and reasoned
in the inspector.

Impact classification follows 03 §3: a new frame on an installed media port is `PAINT`; an
intrinsic-size change is `LAYOUT | PAINT`. The pipeline carries its own gate family (MED1–MED8,
10 §1): pipeline convergence, source-replacement generation revocation, decode-window bounds,
native-plane exactly-once return, device-loss recovery without refetch, one-frame-advance cost,
color/orientation fidelity, and zero-tax-with-no-media.

## 4 · Clipboard, drag, menus, tray, dialogs, and IME

Desktop integration is a set of independently installable typed capabilities, requested by
handlers as parameters, never threaded ambiently:

```rust,ignore
fn copy(&mut self, clipboard: Clipboard) -> Result<(), ClipboardError> { /* .. */ }
fn open(&mut self, dialogs: FileDialogs) { /* an ordinary async action (05 §8) */ }
```

- **`Clipboard`** — MIME-tagged values, images, external paths, X11/Wayland primary selection,
  the find pasteboard, async/lazy providers, ownership-loss notification.
- **`DragDrop`** — one `DataOffer` protocol for internal values, OS MIME data, promised files, and
  virtual files produced on demand.
- **`Menus`** — named-command entries (a `&str` id + serialized payload, or a bare fn-item route
  where nothing remaps it, 04 §3) for menu bar, context, dock/taskbar, and
  application menus.
- **`Tray`** — icon/menu/status/attention and activation routes.
- **`FileDialogs`** — standard futures for open/save/folder choices with filters, bookmarks, and
  cancellation.
- **`OpenExternal`**, notifications, haptics, and accessibility platform bridges follow the same
  capability/result pattern.

**IME is a public bidirectional protocol**, not a backend-private callback. An input target
publishes `Enable/Update/Disable(InputMethodState)` — surrounding-text snapshot/revision, UTF-8
*and* UTF-16 selection, composition/preedit and styling, replacement range, cursor rectangle in
window coordinates, input purpose, capitalization, writing direction. The platform sends
`Preedit`, `Commit`, `DeleteSurrounding`, `SetSelection`, and `Candidates` events tagged with that
revision; stale events rebase or reject. This is the wire protocol `text_input` (06 §3) speaks on
the application's behalf — an in-process virtual keyboard implements the same protocol and needs
no privileged widget API. Platform bar gate T2 (10 §2) exercises the full protocol, including
focus-loss composition settlement, in CI with a scripted IME on every supported backend.

## 5 · Accessibility

> Because the retained tree, its properties, and its impact-tagged publication log are the
> committed epoch, the accessibility projection is a *pure function of that epoch* — which yields
> the platform bar's hardest gate almost for free.

> **T1 — activation completeness.** When assistive technology connects, the full initial
> accessibility tree is produced from the current committed epoch with no describe turn, no
> repaint, and no application code. A window idle for an hour answers immediately and completely.

- **Every standard element has default semantics.** `text` is a label; `button` is a button with
  its action; `text_input` implements the full text-editing accessibility contract (value,
  selection, ranges, edits round-tripped through the same transaction law as an ordinary edit —
  an accessibility-initiated edit is just another `Edit` source, 06 §3). List/tree/grid protocols
  project from the §6 projections.
- **`A11y`-impact publications patch the projection incrementally** — the same delta discipline
  as paint (03 §3), so screen-reader updates are exact, never tree rebuilds.
- **Custom surfaces implement the a11y phase or say why not.** A retained node opting out is an
  inspector-visible hole, not silence; canvas/a11y callbacks remain the escape hatch (T5, an
  audit over the standard element set, requires no retained interactive node to lack a role,
  name, and action surface).

`Target<K>` remains the only ordinary way application code reaches a mounted part. The
inspector/test tier gets a **typed capability visitor** — walk committed nodes, query semantic
role/text/bounds, drive the accessibility action surface — which is exactly what `Win<V>`'s
semantic test queries already are (04 §4, 10 §2), now specified as the same visitor the
accessibility bridge uses. No string query API exists at either tier: tests and assistive
technology address parts by `Target`, role, and semantic text, the same vocabulary.

## 6 · Theme, environment, and RTL

Theme colors are semantic, color-space-aware tokens, not precomposited leaf colors — the full
token/cascade mechanism is [03-ports.md §5](03-ports.md); this section owns only the *inherited*
half. The retained environment supports a sealed set of typed inherited properties: theme/
palette, `Enabled`, visibility, locale, writing direction, reduced motion, contrast, text
defaults, and accessibility state — each a cascade source key, so `panel.provide(Enabled(false))`
patches the affected subtree without recreating it.

`FlowDirection` drives logical `Start`/`End`, flex order, grid placement, padding/margin, icon
mirroring opt-in, and placement fallback; bidi and layout mirroring share locale/direction but
are separate algorithms — setting Arabic text does not blindly mirror a code editor's chrome.

Application-global domain data is not an ambient singleton lookup: `Application::provide`
installs typed resources; constructors receive `Reader<T>`/`Entity<T>` explicitly; handlers may
request a sealed service capability; visual cascade data uses `provide` on the tree. Tests replace
a resource on `Application::demo` (10 §2). This handles settings, theme, HTTP clients, clocks,
and feature flags without an arbitrary global lookup in `view` — exactly the `cx.global`/
`observe_global` pattern this model has no equivalent for, by design.

## 7 · Displays and event-loop interop

`Application::run` owns a normal platform loop; `attach(host)` integrates with an external owner
(a game engine, an audio tool, an embedded shell); `poll(deadline)` exposes prepare/wait-dispatch/
commit deadlines. All three terminate the same application plan and use the same turn queue;
platform callbacks may wake the host but never recursively settle GPUI.

`Application::displays()` returns a `Reader<Arc<[Display]>>` (05 §1) — there is no
platform-specific parallel availability type. A boundary distinguishes first readiness from a
real empty set or a typed platform failure; `displays.resolve().await` is the imperative
startup synchronization point. Each `Display` carries durable identity, live geometry/scale, an
opaque color-profile handle when available, SDR reference white, HDR headroom, and compositor
capabilities; each `Win<V>` publishes its realized surface contract and which display currently
presents it — so layer-shell output selection and a document CMS wait for facts instead of
guessing.

## 8 · The cross-cutting protocol catalog

Selection, grids, trees, docking, gestures, container queries, and live regions are coordination
that spans this design's deliberately local boundaries. The uniform fix is not a second retained
tree with its own lifecycle — it is three nouns:

> **A projection** is a read-only, incrementally maintained index over the manifest, updated by
> the same span splices the tape diff already emits, transactionally, in the same turn. It has no
> independent write path, so it cannot drift. **A session index** is a protocol-owned structure
> (selection geometry, a find match set, a drop gap) leased from first protocol event to teardown,
> rebuilt from protocol state plus projections, never from the manifest alone. **A protocol** is a
> window-level state machine that owns cross-cutting interaction state and consumes projections. A
> primitive is elements + a projection + a protocol.

Four projections, sum-tree-shaped, `O(log n)` maintenance per splice:

| Projection | Summary per node | Serves |
|---|---|---|
| **FlowIndex** | text-run count, byte length, reading order | cross-element selection/copy (06 §4), a11y text order |
| **Lattice** | two 1D extent sum-trees over the visible arrangement | grids, frozen panes, cell nav |
| **FlattenIndex** | expanded-subtree visible-row count | trees/outlines, scroll anchoring, type-select |
| **TokenIndex** | `(ScopeId, NameId)` → consuming slots | theme swap fanout (03 §5) |

**Zero-cost rule**: a window with no `.selectable()`, no `grid()`, no `tree()`, and no live
regions allocates no projection, claim table, or announcement queue; unused *protocols* cost zero
too — session indexes never outlive their protocol.

**Grids** are one `Lattice` over the visible arrangement (never the store), with frozen
rows/columns as the same cells drawn in a clip that ignores one scroll axis; the resident set is
`(frozen ∪ viewport_band) × (frozen ∪ viewport_band)`, never the gap between them. **Trees** pull
children (`Children::{Leaf, Ready(&[N]), Pending}` — one placeholder row, one splice on arrival)
and track cursor ≠ marked-set, with collapse rebasing cursor/marks/drop onto the parent or next
visible row. **Virtualization** preserves slots, edit sessions, focus/accessibility, anchor
extents, and measurement generations; every off-viewport row the virtualizer keeps resident names
its *retention reason*, inspector-visible, so residency is attributable rather than emergent.
**Docking** is two separate delta algebras — `ChromeDelta` for workbench slots and
`GridDelta` for editor splits — kept apart so persistence, "reset layout," and drag-between-them
policies stay sane; both are sealed, invertible, journal-eligible deltas (01 §10.2). **Gestures**
arbitrate through, in order: static compositor claims (`.pan(Pan::VERTICAL)`, resolved before any
event so resident scroll never consults recognizers), a failure DAG
(`.after_failure_of::<Tap>()`), a nested-scroll pre/child/post pipeline, pairwise simultaneity, and
only then innermost-wins as a tie-break among still-possible recognizers. **Container queries**
resolve against a *named ancestor* that declared containment — never self-size, which reopens the
shrink-to-fit cycle. **Live regions** must exist in the accessibility tree *before* the change
that fills them (same-turn create-and-fill is silently dropped by real assistive technology);
`Announce::polite("Saved")` is an ordinary effect value from `changed`, coalesced per region under
a rate limit.

Full projection/protocol/session-index detail, arbitration ordering, and the sealed recognizer
set live in the historical `06-cross-cutting.md` source pass; this section is the platform-level
summary every gate in [10-quality.md](10-quality.md) §1 (PG1–PG6) binds to.
