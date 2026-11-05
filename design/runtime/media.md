# Typed Media and Renderer Frontend Contract

This document defines the detailed image, animated-image, video and native-surface contract for the
retained plan. It is normative where it uses **must**, **never**, or a named invariant.

## 1. Authoring surface

There is one obvious constructor per semantic destination. The source is an ordinary Rust value:

```rust,ignore
image(asset!("icons/logo.svg"))
image(path)
image(http.get(avatar_url).header(ACCEPT, "image/avif,image/webp,*/*"))
image(bytes)

video(path)
video(http.get(movie_url).bearer_auth(token))
video(camera.frames())
```

All `image` arguments implement a sealed `MediaInput<Still>` installation protocol; all `video`
arguments implement `MediaInput<Timed>`. The trait is local to GPUI, so the framework can supply
zero-wrapper implementations for `PathBuf`, embedded assets, byte buffers, its HTTP request type,
URLs with an explicit default request policy, `Work` results and native provider descriptors. The
sealed trait is visible in diagnostics but has no methods an application must call. A custom source
uses the separate implementable `MediaProvider` contract described below.

This is generic at compile time, not a public mega-enum and not a heap-allocated trait object in the
common path. Mounting consumes the temporary source descriptor and lowers it to a compact source
recipe plus property ports. Constant embedded/path sources need no reactive node. A reactive source
property owns one port; changing it revokes the previous acquisition generation and installs the
new recipe. The widget stores neither the original generic type nor an `Arc<RenderImage>`.

**Presentation (Slint Image / Iced filter / Bevy ImageNode).** These are element policy, not a
second constructor:

```rust,ignore
image(src)
    .fit(Fit::Cover)                 // Contain | Cover | Fill | ScaleDown | None
                                     // None/Auto: inherent size is a Taffy measure fn
                                     // Fill/slice/tile: measure fn cleared (Bevy ImageMeasure)
    .sampling(ImageRender::Smooth)   // Smooth | Pixelated
    .colorize(theme.accent.on)       // alpha mask → brush; icons too: icon(Magnifier).colorize(…)
    .tint(0x88aaff)                  // multiply on a photo; white = identity. Not colorize.
    .slice(NineSlice::px(12))        // 9-slice chrome (Bevy TextureSlicer)
    .tile(Tile::XY)                  // repeat
    .atlas(16, 16).index(3)          // sprite sheet
    .crop(rect(0, 0, 32, 32))
    .flip(true, false)
    .decorative()                    // Role::None; default is Image
```

`colorize` is the public verb on `svg` / `icon` / mask `image`. `tint` is multiply on a
decoded photo. They do not compose into one method: a mask that is also tinted is a located
error naming both. Fit / slice / tile do not change acquisition; they are layout + paint.
Pixelated sampling is a sampler, not a shader graph. 9-slice and tiled clear intrinsic
measure — the element sizes from style, not the PNG.

Source spelling is explicit where ambiguity is dangerous. `asset!(...)`, a `PathBuf`, a parsed URL,
and an HTTP request are distinct Rust types; arbitrary strings are not guessed to be files, URLs or
embedded names. A caller can therefore use a static path and a customized web request in exactly the
same `image(...)`/`video(...)` position without losing cache or security semantics.

There is no `AssetSource` and no `.assets()` on the application. Current GPUI's
`Application.with_assets` is a process-global string → bytes lookup; it is deleted. Bytes come from
the constructor argument (`asset!`, `PathBuf`, a request, bytes), from a **field** that implements
`MediaProvider` (a photo library, an authenticated avatar store), or from an optional crate the app
depends on and *holds* (a `rust-embed` catalog, a blob store). The kernel does not name that crate
and does not offer a service-locator hook for it. A custom HTTP client is the same story: it is a
field that produces requests, not `cx.set_http_client`.

Presentation policy remains destination-owned and composable:

```rust,ignore
image(profile.avatar_request)
    .fit(ObjectFit::Cover)
    .placeholder(asset!("avatar-placeholder.svg"))
    .on_error(asset!("avatar-error.svg"))
    .decode_at(layout_size())

video(http.get(movie_url))
    .fit(ObjectFit::Contain)
    .autoplay_when(visible())
    .poster(asset!("poster.avif"))
    .captions(captions)
```

`placeholder`, `poster`, and `on_error` are media inputs lowered through the same pipeline; they are
not special caches. Loading/error/value are readable projections only when the application needs
them. The normal element owns the subscription and does not require paired `source`/`bind_source`
methods.

### Custom providers and thread bounds

Framework conversions cover common static and network sources. Applications that resolve
content-addressed stores, authenticated services, databases, generated frames or cameras implement
one acquisition boundary:

```rust,ignore
impl MediaProvider<Still> for AvatarStore {
    type Request = AvatarId;
    type Error = AvatarError;

    async fn open(&self, request: AvatarId, out: MediaSink<'_>) -> Result<(), AvatarError> {
        // Stream metadata and encoded chunks; the framework owns cancellation/backpressure.
    }
}

image(self.avatars.media(user.avatar_id))   // the store is a field, not scope / App
```

The exact async trait spelling may use an associated future to preserve the repository's MSRV, but
the semantic contract is fixed. The provider emits typed metadata/chunks/frames into a bounded sink;
it does not receive `Window`, `App`, a renderer, or destination state. It declares a stable cache
identity or `no_store`, cancellation behavior, seek/range support, and whether bytes are replayable.

No ordinary `image`, `video`, canvas, or local provider value is required to be `Send`. `Send +
'static` applies only when the chosen acquisition/decoder/backend actually crosses a worker or
device thread. `scope.media_local` supports executor-local producers. A cross-thread implementation
receives owned/frozen request data and returns owned chunks or opaque import leases; it cannot carry
local state keys or renderer borrows.

## 2. One pipeline, typed stages

The runtime does not collapse media into “some pixels.” It lowers a source through independently
budgeted stages:

```text
source recipe
  -> acquire bytes or native samples
  -> sniff / metadata / demux
  -> decode or parse
  -> orient / crop / scale / color plan
  -> CPU pixels, vector scene, compressed GPU blocks, or native planes
  -> renderer import/upload and residency
  -> retained media fragment
  -> frame packet and fence retirement
```

Each arrow has a request/generation stamp, cancellation point, byte/time budget and inspector event.
Only the necessary stages run. An already decoded native video sample skips byte acquisition and
software decode. An SVG may retain a parsed vector display list and produce scale-specific raster
variants. A small static bitmap may enter an atlas; a large, tiled, animated or frequently replaced
image normally uses dedicated textures. A backend-supported native frame can be imported without
an RGBA conversion; the fallback produces uploadable planes or RGBA pixels.

The internal stage types prevent invalid shortcuts: encoded bytes cannot be sampled, an undecorated
YUV plane set cannot be presented without its color/crop metadata, and a device-generation import
cannot enter another renderer. These types remain private so application code sees the small API
above rather than a type-state pipeline.

### Identity and cache correctness

Caches are split because their lifetimes and keys differ:

- **Acquisition cache:** representation identity, validators and encoded chunks. HTTP identity
  includes the resolved URL and representation-affecting request data; it honors redirects,
  `Vary`, `ETag`/last-modified, freshness, authorization partitioning and `Cache-Control`/no-store.
  Secret headers are never logged or serialized to inspector snapshots.
- **Parsed/decode cache:** content identity plus codec/parser version and decode policy. Files use a
  canonical file identity plus observed metadata and can upgrade to content hashing when needed;
  embedded assets use build/content identity. Arbitrary closures are never silently hashed.
- **Variant cache:** source content plus target size/scale, orientation, frame/time, gamut/transfer,
  alpha/premultiplication, sampling, animation and accessibility policy.
- **Residency cache:** variant plus renderer device generation, texture/plane format, usage and
  backend import capability. Its values are fence-retired leases, not application-owned handles.

Every cache has byte/item/time budgets, recency and visibility priority, admission/eviction counters,
and owner references. Sharing is by explicit stable identity, not coincidental pointer or `u64`
hash alone. Hash collisions are checked against full keys. `no_store` content may still retain the
minimum in-flight data required for correctness and then releases it.

Visibility drives priority but not correctness. Imminently visible media can prefetch metadata and a
thumbnail; distant virtual rows lose decoder and GPU residency before encoded shared cache entries.
Unmount/cancellation prevents publication to a revoked owner, while shared acquisition may continue
only if another live consumer or an explicit prefetch owner remains.

## 3. Still and animated images

Still decode observes EXIF orientation, density, ICC/gamut/transfer data, alpha and target scale.
Decode-at-size is a hint with a correctness fallback, not license to blur after a later resize. The
runtime retains enough upstream authority to request a sharper variant, subject to budget. Large
images may decode and upload tiles/mips rather than allocating one maximum-size RGBA buffer.

Animated images are timed streams, not a `Vec<Frame>` permanently attached to an image. The decoder
maintains a bounded presentation window plus the disposal/compositing state needed by GIF/WebP/AVIF
semantics. Seek/restart may use sparse checkpoints; the budget chooses recomputation over unlimited
decoded retention. One injected media clock selects frames. Animation sampling creates a media
presentation deadline and a direct fragment/resource patch; it does not write source state or run a
general reactive effect for every frame.

The queue is bounded by decoded bytes and presentation duration. Slow consumers exert backpressure;
late non-key frames can be skipped only when the decoder's dependency and disposal rules permit it.
Hidden/occluded policy may pause decoding or reduce cadence, but resumption computes the correct
logical frame rather than replaying every missed tick.

## 4. Video and live frames

`video(source)` separates immutable source/resource identity from mutable playback state. A retained
player owns timeline position, play/pause/rate, seek generation, buffering, selected tracks,
captions and error state. Applications can bind those ordinary typed destinations without managing
demuxers, decoder queues, generation counters or renderer surfaces.

Demux and decode queues are bounded by bytes and media time. A seek revokes older packets and decoder
output generations. The scheduler prioritizes the frame needed for the next presentation deadline;
late frames are dropped by timestamp/dependency policy, never accumulated to preserve FIFO. Audio,
when present, participates in an explicit A/V clock policy; wall clock is not independently sampled
by the video node and animation scheduler. Rate changes, looping, discontinuities and background
suspension are deterministic inputs to that clock.

Decoded output carries dimensions, visible crop, orientation, sample aspect, timestamp/duration,
plane formats/strides, matrix/range/primaries/transfer metadata and HDR mastering information when
available. The renderer may sample YUV planes and convert/tone-map in the final color pipeline; it
must not silently assume BT.709 limited-range SDR or force every frame through CPU BGRA. Captions and
accessibility remain semantic nodes, not burned pixels.

Hardware decode/import is a capability path:

- Metal may lease Core Video planes through `CVMetalTextureCache`;
- DirectX may share Media Foundation/DXGI decoder surfaces with the rendering device;
- wgpu/WebGPU may use an external texture when the adapter and source form support it;
- other backends can register a typed native importer, upload decoded planes, or fall back to RGBA.

“Zero-copy” is never a public guarantee. The measurable fast-path contract is no CPU pixel copy and
no extra full-frame GPU copy when decoder, device, format, crop/color plan and shader use are
compatible. The inspector records why a frame copied. Native sample and texture wrappers stay alive
through the exact GPU completion fence, then return to the decoder/pool. Backpressure prevents a
stalled renderer from exhausting a decoder pool.

Protected content declares restrictions. It may forbid CPU readback, screenshots, remote inspection,
custom shaders or intermediate targets. Unsupported composition fails explicitly or uses a
platform-approved protected path; it never downgrades protection invisibly.

## 5. Renderer frontend composition

The retained core stops calling `PlatformWindow::draw(&Scene)` as its architectural boundary. Each
window connects to a renderer frontend negotiated from a typed capability set:

```rust,ignore
trait RenderFrontend {
    fn capabilities(&self) -> RenderCaps;
    fn prepare(&mut self, packet: &FramePacket) -> PreparedFrame;
    fn submit(&mut self, frame: PreparedFrame) -> Result<FrameFence, PresentError>;
}
```

This sketch names the boundary, not a commitment to dynamic dispatch or these exact methods. A
production implementation may use an enum over built-in backends and monomorphized lowering. The
critical rule is **one backend selection per packet/pass/range, never one virtual call per
primitive**.

`FramePacket` contains immutable changed fragment/range references, resource create/update/retire
operations, media-frame descriptors/import leases, pass graph, damage, target-validity state,
surface/device generation, scale, extended-sRGB working contract, and realized output contract. It
also carries the full-scene fallback description when required by recovery. UI `Color` and media
pixels meet only after both are in linear extended sRGB ([color.md](color.md) §7). HDR metadata stays
source-specific, while reference white/tone mapping/presentation are the shared color contract; no
style color is silently promoted to HDR. The packet pins only versions actually needed by the
in-flight frame and owns those pins until its fence retires.

Capabilities are independent bits/limits, not one flattering tier number: retained target,
damage-region replay, partial present, transform/layer recomposition, stable GPU slots, texture
arrays/argument binding, timestamp queries, custom pass formats, external plane formats, protected
surfaces, HDR output and readback. Lowering selects the cheapest valid path per operation. One
unsupported video import does not demote unrelated retained quads; it chooses a plane-upload/RGBA
fallback for that resource and records the reason.

The frontend owns device objects, pipeline/layout caches, staging rings, atlases, residency,
external imports and fence-deferred destruction. The UI/scene core owns backend-neutral retained
descriptions, identities and damage obligations. Platform code owns the swapchain/native window and
frame pacing. No core scene type contains `Any`, `CVPixelBuffer`, wgpu texture or D3D resource.
Instead, native providers create an opaque, generational `ImportLease` through a renderer-advertised
typed importer. Only the matching frontend can open it.

Device loss increments the device generation, rejects old imports, clears only device residency,
and asks upstream media stages for the cheapest recoverable representation. Surface loss preserves
logical scene and media state. A full replay is always correct. Cross-window sharing is limited to
compatible device-level pipelines, immutable decoded content and budgeted residency; target
validity, pacing, damage and in-flight media leases remain per window.

This composition enables backend-specific optimization without contaminating authoring:

- Metal can retain buffers/heaps, use native Core Video plane imports and encode only changed ranges.
- DirectX can use native decoder surfaces and its own descriptor/upload strategy.
- wgpu can use portable range uploads and capability-gated external textures.
- a deterministic software frontend consumes the same packet for tests and correctness oracles.

## 6. Scheduling, testing and inspection

Media work enters the same owner/revision scheduler but has three delivery classes: reliable
metadata/error changes, latest-by-generation acquisition/decode state, and latest-by-presentation-
timestamp frames. UI updates never poll a task or frame queue. Completions enqueue one owner-targeted
wake; publication validates owner, source generation, seek generation and device generation before
changing the retained fragment.

`UiTest` injects a media provider, decoder and clock. Tests can release headers/chunks/frames,
advance media time, force cache hit/miss/eviction, simulate range/HTTP errors, stall a decoder or GPU
fence, trigger device loss and select an import capability. Still-image golden tests use pinned
decoders/color policy. Video tests assert exact frame timestamps and scene/import commands; pixels
are exact only under the reference decoder/renderer fixture.

The inspector reports, with private data redacted:

- source/provider identity, generation, acquisition status, response/cache policy and byte flow;
- sniffed format, dimensions, orientation, color/HDR metadata and selected decode variant;
- CPU encoded/decoded and GPU resident bytes, owners, leases, eviction reason and cache hit chain;
- animated/video queue duration, presentation timestamp, dropped/repeated frames, seek/buffer state;
- import path, planes, copy count/bytes, color/tone-map plan, device generation and fence lifetime;
- fallback reason, decode/upload/submit timing and contribution to damage.

No inspector action can expose authorization headers, private response bodies, protected pixels or
native handles. Devtools observe the same stage records used by budgets and performance gates.

## 7. Non-negotiable gates

- A cached static path and an equivalent HTTP response converge on the same parse/decode/variant/
  residency code after acquisition; neither requires a widget-local cache or task poll.
- Replacing one reactive source revokes the old generation; late bytes, decode output and imports
  cannot overwrite the new image or clear its pending state.
- Animated-image and video decoded memory remains within declared byte/time windows under a stalled
  UI, renderer or decoder.
- An external/native fast path holds every source plane through GPU completion and returns it once.
- Device loss recovers without stale-handle use and without unnecessarily refetching still-valid
  encoded content.
- A one-frame video advance patches only the media resource/fragment and intersecting damage; it does
  not rebuild the widget, run unrelated graph computations, relayout fixed geometry or upload an
  RGBA copy on a proven native-import path.
- Color/crop/orientation results match the reference pipeline within the declared backend tolerance.
- With no media present, the media system adds no per-frame traversal or allocation. With a static
  warm image, it schedules no work and retains only budget-accounted cache/residency entries.
