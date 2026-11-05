# Color: extended-linear working light, open display boundary

Status: **normative color contract** for the retained plan. Consumes the audit in
[00-research/12-color-spaces.md](../00-research/12-color-spaces.md). Media source/decode lifetime
remains [media.md](media.md); this document owns the common color-management, working-light, and
presentation boundary. Glyph *fill* uses this document; glyph *coverage raster* (ClearType, macOS
dilation) is platform text, tinted here.

The laws:

> **Authors write `Color`. The GPU composites premultiplied linear extended sRGB. The surface is a
> negotiated, tagged contract.**
> `Hsla` and `Rgba` are constructors, not working spaces. Mixing two hex colors in gamma, or
> uploading HSL to three shaders that disagree about whether that is linear, is a bug, not a
> style.
>
> **A document owns its source profiles; GPUI owns no closed list of displays.** A CMS transforms
> document/media colors into the extended-sRGB working contract. The OS compositor normally maps
> that tagged contract to the current display. New display gamuts do not require a new GPUI
> framebuffer enum.

This is the 03 split (`Color` vs `LinearColor`) with the public-surface correction 07 demands:
application code never names the linear type in a style chain. Typed ports still make
wrong-space uniforms unrepresentable — that seam is *internal*.

---

## 0 · What ships today (honest)

Public types in [`crates/gpui/src/color.rs`](../../../crates/gpui/src/color.rs):

- `Rgba` / `Hsla` — channels in 0…1. HSL is the canonical *paint* color.
- `ColorSpace::{Srgb, Oklab}` — **gradient interpolation only**, two stops.
- `Background` — solid, linear gradient, slash, checkerboard.
- `Colors` — eight light/dark `Rgba` hex roles. No contrast math.

There is no `Oklch`, Lab, HWB, Display P3, Rec.2020, HDR, ICC, `color-mix`, or WCAG helper.
Surfaces are `BGRA8Unorm` / `B8G8R8A8_UNORM` / wgpu Unorm — **not** `*_sRGB`. Blending is
straight-α in encoded channels for quads; premultiplied for paths.

Three shader copies of `hsla_to_rgba` disagree:

| Backend | Claim | Reality |
|---|---|---|
| Metal | HSL → display-referred RGB; Oklab via `pow(γ, 2.2)` | Consistent with itself; approx transfer |
| HLSL | Same, but `linear_to_srgb` / `srgb_to_linear` **names are swapped** | Windows Oklab gradients diverge from Mac |
| WGSL | Comment: `hsla_to_rgba` returns *linear*; solids skip the round-trip | Internally inconsistent; piecewise sRGB for Oklab only |

`Hsla::blend` / `Rgba::blend` / `Lerp for Rgba` mix in encoded sRGB. That is not CSS
`color-mix()`. Highlight styles use it for syntax overlays.

Text: Windows/wgpu apply MS Terminal gamma + dual-source ClearType. macOS rasters grayscale
with luminance-keyed dilation and **no** that gamma path. Same `Hsla` is not the same pixels.

---

## 1 · Types

```rust,ignore
/// Public, Copy, tagged. Constructors below. Never a gamma/linear smash.
struct Color { /* packed: [f32; 4] + Space + flags */ }

enum Space {
    Srgb,          // CSS srgb (gamma-encoded numbers, retained with this authoring tag)
    SrgbLinear,    // CSS srgb-linear
    Oklab,
    Oklch,
    DisplayP3,
    // Lab / Lch / Hwb / Rec2020 as constructors that convert into the store
}

/// Internal / shader port. Premultiplied linear extended sRGB.
/// RGB is deliberately not clamped; alpha remains 0...1.
/// Not accepted by `.bg()` / `.text_color()`.
struct WorkingColor { r: f32, g: f32, b: f32, a: f32 }

/// Public only at explicitly HDR-capable paint/material sinks.
/// `Color` supplies chromaticity; luminance gives the otherwise-missing meaning of > SDR white.
struct HdrPaint { color: Color, luminance: Luminance }
enum Luminance { Nits(f32), SdrWhiteMultiple(f32) }
```

Constructors — CSS Color 4 names, one `Color`. **Packed `rgba(0xRRGGBBAA)` is deleted.**
Today's `rgb` silently drops the high byte of a `u32`; `rgba` reads the same width as
RRGGBBAA. Eight hex digits in `rgb` paint the wrong color with no error.

```rust,ignore
Color::Red                       // CSS named colors: associated consts, PascalCase as authored
Color::White
rgb(0x334155)                    // 24-bit sRGB, opaque
rgb(0x334155).alpha(0.4)         // alpha is a method — never a packed byte
0x334155                         // `impl IntoColor for u32` (24-bit); builders accept it
0x334155.alpha(0.4)              // prelude extension on u32, same Color
hsl(210., 0.40, 0.26)            // CSS hsl, degrees
oklch(0.45, 0.08, 250.)
oklab(0.45, 0.02, -0.08)
color(Space::DisplayP3, 1., 0., 0., 1.)
Color::from_hex("#334155")       // 6 or 8 digit; 8-digit is RRGGBBAA in the *string*, parsed
theme.surface                    // token, still a Property<Color>
```

`.bg(0x1e293b)`, `.bg(Color::Red)`, `.bg(theme.surface)`, `.bg(&self.tint)` are one method
(`impl Property<Color>`). A `u32` greater than `0xFFFFFF` is a debug assertion / located
error: "alpha is `.alpha(t)`, not a packed hex byte."

`Hsla` and `Rgba` remain `From`/`Into` `Color` so the current fluent API does not break. New
code is taught `Color::Red` / `oklch` / `rgb` / tokens. Do not add a third public 4-float
struct that means "maybe linear."

**Storage:** `Color` keeps its tagged authoring coordinates, including missing-hue flags, so
serialization and author-space interpolation round-trip. Retained paint lowering converts a
constant once—or a live color only when its source version changes—into extended linear sRGB
without gamut clipping and stores that `WorkingColor` in the paint slab. There is no conversion on
an unchanged frame. Mixing converts both inputs into the requested interpolation space, mixes, then
lowers the result for compositing. A P3/Rec.2020/ICC conversion may produce negative or
greater-than-one working components; that is valid data, not an overflow to clamp.

**Alpha:** straight in `Color` (CSS). Premultiply only when producing `WorkingColor` for a GPU port.
`HdrPaint` lowers against the realized surface's reference white/headroom; ordinary theme `Color`
never does. Unsupported HDR sinks or SDR surfaces take the window's declared tone-map/fallback.

---

## 2 · Mixing, gradients, motion

CSS Color 4/5, not `Rgba::blend`.

```rust,ignore
c.mix(other, t)                          // default: oklab, missing hue
c.mix_in(Space::Oklch, other, t)
c.mix_in(Space::Srgb, other, t)          // encoded sRGB — opt-in, never default
color_mix(Space::Oklab, [(c, 0.4), (d, 0.6)])
```

Hue interpolation: `shorter` default; `longer` / `increasing` / `decreasing` as optional flags
when mixing in a polar space (oklch, hsl).

**Gradients** (Background):

- v1: still two stops is a renderer limit to lift; schema allows N stops.
- Each gradient names an interpolation space (`in oklab` default for new builders; `in srgb`
  preserved for the current look).
- Midpoints later.
- Dither stays (existing triangular noise) — banding in 8-bit sRGB is real.

**Motion:** `Lerp` for `Color` goes through `mix_in(Oklab, …)` unless the field metadata names
another space. Lerping `Rgba` channels is deleted as the color path (keep it for generic
`[f32;4]` if needed). A theme cross-fade is paint-only.

**`relative color`** (CSS Color 5): v2. v1 can express the common case as `c.with_alpha(0.5)` /
`c.fade(0.16)` / `c.grayscale()` on the tagged value, computed in oklab lightness where it
matters.

---

## 3 · Compositing, framebuffer, and presentation

Pick one working pipeline and generate it once:

1. Source/profile transforms produce linear extended-sRGB values. SDR reference white is `1.0`;
   values above it require an explicit HDR/EDR intent, while negative values may simply encode a
   wide-gamut color.
2. Fragment shading produces **premultiplied linear extended-sRGB** light.
3. Blend `One / OneMinusSrcAlpha` in that space.
4. Present through one of two negotiated surface contracts:
   - **`SdrSrgb8` (default):** preserve the ordinary 4-byte/pixel surface. Gamut-map/tone-map the
     changed scene ranges as required, encode once to sRGB, and use an sRGB attachment or an
     explicit final encode—not both.
   - **`ExtendedSrgb16` (opt-in):** prefer a float16/scRGB-equivalent surface tagged **extended
     linear sRGB**. Preserve negative and greater-than-one RGB through submission. The platform
     compositor maps the tagged surface to the current display and exposes available HDR headroom.

`ExtendedSrgb16` is the stable framebuffer contract, not “the monitor is P3.” On Apple it maps to
an extended-linear-sRGB `CAMetalLayer` configuration; on Windows to
`R16G16B16A16_FLOAT`/scRGB; on Vulkan/Wayland to an advertised extended-linear-sRGB surface or an
explicit final transform into the compositor's accepted target. Backends return the **realized**
format/tag/headroom. `Prefer(ExtendedSrgb16).fallback(SdrSrgb8)` is normal; `Require` makes lack of
support a checked window-open/surface-reconfigure failure.

Quads, shadows, underlines, **glyph tints**, paths, filters, temporal history, custom canvases, and
intermediate/save layers use the same working contract. An extended final swapchain with 8-bit
intermediate layers is not an extended pipeline. Every intermediate declares `Working`, `Mask`,
`Data`, or an authored/profiled encoding so color transforms are never applied to velocity, ids,
masks, or arbitrary shader data.

**HDR is policy, not merely an unclamped number.** A window separately requests dynamic range,
reference white, tone-map behavior, and fallback. Apple EDR/Windows Advanced Color enablement and
display headroom are live surface facts. An SDR `Color` remains SDR even on an HDR surface; a
highlight/media effect must opt into a luminance-bearing/HDR paint value. Screenshot/export chooses
a tagged SDR or HDR encoding explicitly.

### 3.1 Zero-tax rule

The ordinary path retains today's byte class: no float swapchain, working-history texture, color
LUT, profile lookup, HDR metadata, or extra full-screen pass merely because the feature exists.
Pipelines are cached by `(shader, target_format, surface_contract)` and the extended variants are
created lazily. A window that requests `ExtendedSrgb16` pays the unavoidable extra bandwidth and
storage; a professional application can make that its default, but GPUI may not describe it as
free. Static color transforms happen at construction/decode/upload or in a cached LUT; changing a
display/profile patches the presentation transform and affected resources, not the element tree.

---

## 4 · Shared math (kill the triple shader)

One conversion module, three backends generated or included:

- IEC 61966-2-1 piecewise sRGB ↔ linear (not `pow(2.2)` except as a documented approx for
  text gamma tables).
- Bottosson Oklab ↔ linear sRGB (matrices already cited in the shaders).
- Oklch polar ↔ Oklab.
- CSS `over()` in linear premultiplied.

**Fix immediately when touching shaders:** HLSL `linear_to_srgb` / `srgb_to_linear` name swap
(`shaders.hlsl` ~124–131 vs `shaders.metal` ~950–956). Until then, `ColorSpace::Oklab`
gradients are not a cross-platform feature; they are a Mac demo.

CPU and GPU must match to ULP tolerance on a fixture palette (gate C2).

---

## 5 · Tokens, themes, contrast

[style/README.md](../style/README.md) already wants `PaletteToken` plus opacity/blend class,
reverse use lists, paint-only invalidation.

```rust,ignore
enum Paint {
    Token(PaletteToken, /* optional alpha multiplier */),
    Color(Color),
    Mix { space: Space, a: Paint, b: Paint, t: f32 },
}

theme.text                         // token
theme.text.opacity(0.6)            // still a token edge, not a resolved Color, until paint
theme.accent.mix(theme.surface, 0.12)
```

Resolving a token against appearance (light/dark/high-contrast) is once per frame per unique
token, not once per node. Changing `theme.accent` patches indexed users; it does not restyle
the tree.

**Contrast:**

```rust,ignore
fn contrast_ratio(fg: Color, bg: Color) -> f32;   // WCAG 2 relative luminance (sRGB)
fn contrast_color(on: Color) -> Color;            // CSS contrast-color(): black or white in oklab L
```

No token ships without a documented contrast pair for text-on-surface. This is an a11y gate,
not a theme-author's optional spreadsheet. A role is a **pair** (`fill` + `on`) — Slint Palette
/ Iced `extended_palette` `Pair` — so `theme.accent` in `.bg` is the fill and
`theme.accent.on` is the only ink that is legal on it.

**Seed → table (Iced 0.14).** `Palette::generate(oklch_seed, Scheme::Dark | Light)` fills a
TokenIndex table of pairs (surface, accent, danger, warning, success, selection, control).
Optional; hand-written tokens remain valid. Runs at theme load, not per frame. Animating a
token stays a located error (06). **`Pair::new` always runs `readable`** (Iced
`Pair { color, text }`) — contrast is constructed, not hoped. Public scale is weak / base /
strong; Iced's eight-step `weakest…strongest` Background is generator-internal.

**Named tables.** Iced ships Dracula / Nord / Solarized / Gruvbox / Catppuccin / … as
`Palette` constants. Kernel ships pairs + seed + one default table (`Colors`). Named
theme packs are an **optional crate** the app depends on and holds — same law as
`MediaProvider`. Not eight themes in the kernel.

`Colors` (the eight-role struct) becomes a *default palette*, not the color system.

**Brush.** Fill ports (`background`, text fill, icon `colorize`, path fill/stroke) take a
`Brush`: solid `Color` or a gradient (linear / radial / conic). `Color` is a solid brush.
`.brighter` / `.darker` / `.alpha` map every stop (Slint). Interpolation uses §2 mix space.

---

## 6 · Text

Coverage is platform; **tint is `Color`**.

- Glyph atlas: coverage (A8 or subpixel LCD). The fragment shader tints in the same linear
  over as fills (§3).
- **macOS dilation** is keyed on luminance. A token change that crosses dilation bands misses
  the atlas — reshape/raster, counted. Small tints of the same role must not.
- **Windows/wgpu ClearType gamma** stays a text-raster policy (`get_gamma_correction_ratios`).
  It is *not* the UI working space. Do not "unify" by deleting it; document that LCD text is a
  special composite onto an opaque backdrop. Transparent subpixel text remains meaningless
  (existing DirectX comment).
- Skip-ink samples coverage; decoration color is `Color` (09-text.md).
- `HighlightStyle::blend` becomes `color-mix` in oklab, not sRGB `Hsla::blend`.

Gradient fill on text (09 `fill(Background)`) uses the same gradient shader as quads, clipped
to glyph coverage — one interpolation space, not a second opinion.

---

## 7 · Professional media, documents, and display profiles

Images/video already carry color metadata in the media design. A creative application additionally
keeps the document's embedded/assigned ICC profile, rendering intent, black-point compensation,
proof profile, and transform revision in its own model. GPUI supplies an optional `ColorManagement`
capability for profile parsing/CPU transforms/GPU LUT realization, but the renderer contract does
not depend on one specific CMS implementation.

```rust,ignore
Window::document(studio)
    .surface_color(Prefer::new(SurfaceColor::ExtendedSrgb16)
        .fallback(SurfaceColor::SdrSrgb8))
    .dynamic_range(DynamicRange::Extended)

image(document_pixels)
    .source_profile(document.profile())
    .rendering_intent(document.intent())
```

Source pixels are transformed once—or through a cached GPU 1D/3D LUT—into linear extended sRGB,
not into the current monitor gamut. UI `Color` and managed document pixels therefore meet at
composite in `WorkingColor`. Moving a window between displays normally changes only the compositor's
output mapping/headroom; soft proofing or application-owned output transforms are explicit passes
keyed by the live display/profile revision. GPUI exposes opaque ICC/profile bytes or a stable
platform profile handle when available; it does not add a `Space` variant for every future display.

The framebuffer is not the document format. A `PixelBuffer`/canvas resource may retain native
integer, float16, float32, spectral/spot-channel, or profile-encoded application data for editing;
`Color` is the UI/paint value, not a professional pixel container. The application converts only
the layer/result being composited into `WorkingColor`. Float16 is the preferred presentation
precision, not a promise that every destructive filter or archival export should compute at f16.

HDR media and HDR UI share the extended surface but retain different metadata and policy. PQ/HLG
content is decoded to the working reference-white convention before composite. SDR fallback invokes
the declared tone mapper. A greater-than-one `WorkingColor` survives an extended surface, but it is
not authored HDR until a luminance/reference-white contract says what that value means.

---

## 8 · Authoring on the ladder

```rust,ignore
div()
    .bg(theme.surface)
    .text_color(theme.text)
    .border_color(theme.accent.mix(theme.surface, 0.4))

title.fill(linear_gradient(90., theme.accent, theme.accent.lighten(0.1)).in_oklab())
```

No `.bg_linear(…)` vs `.bg(…)`. `Background` already exists; it holds `Color` stops instead of
`Hsla`. `impl From<Color> for Background`. Tokens implement `Into<Paint>` / the same sealed
property bound as today.

Shaders/canvas uniforms that need lighting math take `WorkingColor` via an explicit
`color.to_working()` at the port — 03's unrepresentable-wrong-space rule, kept off
the Tailwind chain.

---

## 9 · Gates

- **C0 — One math.** Metal / HLSL / WGSL / CPU share conversions. HLSL name swap gone.
- **C1 — Working space.** Golden: 50% red over 50% green in linear vs encoded; we match CSS
  `color-mix(in srgb-linear, …)` for the linear path and document the old gamma blend as
  *not* the default.
- **C2 — Oklab round-trip.** Bottosson samples; CPU vs each GPU backend.
- **C3 — Text tint.** Same token on Mac grayscale vs Win ClearType: contrast-ratio oracle, not
  pixel-identical (subpixel). Dilation-band crossings are counted.
- **C4 — Tokens.** Changing one palette entry patches only indexed paint users; no layout.
- **C5 — Contrast.** Default theme roles for text/surface meet WCAG 2 AA in both appearances.
- **C6 — Destination contract.** Scene records working space, surface contract, reference white,
  tone-map policy, and realized tag/format. No fixed-P3 working path or silent clamp.
- **C7 — Extended range.** Negative and greater-than-one fixtures survive every working
  intermediate and an extended offscreen target; `SdrSrgb8` applies the declared gamut/tone map.
- **C8 — Profile corpus.** sRGB, Display P3, Adobe RGB, gray, CMYK, matrix/TRC and LUT ICC fixtures
  match a pinned CMS oracle within tolerance; malformed/unsupported profiles fail explicitly.
- **C9 — Display move/HDR.** Moving a live window between SDR/HDR/profile revisions updates the
  presentation transform/headroom without rebuilding its logical or layout tree; no double transform.
- **C10 — Zero tax.** The SDR fixture has the same surface byte count, attachment count, pass count,
  shader-variant creation, and clean-frame work as the color-management feature disabled build.

CSS Color 4 sample files are oracles for mix / oklch parse. We are not a browser CSS parser;
we are claiming the *color math*.
