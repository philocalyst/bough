# Shader & Renderer Unification — Migration to Real Parity

**Status:** proposal, verification-backed
**Date:** 2026-08-27
**Scope:** collapse GPUI-CE's three hand-maintained renderers + three hand-triplicated shader
files into one wgpu-core renderer with a single shader source of truth, cross-compiled by naga,
**without regressing any platform-specific capability.**

---

## 0. TL;DR

GPUI-CE ships three renderers and three copies of every shader:

| Crate | Renderer | Shader source | ~lines |
|---|---|---|---|
| `gpui_macos` | `metal_renderer.rs` (Metal, hand-written) | `shaders.metal` | 2260 + 1409 |
| `gpui_windows` | `directx_renderer.rs` (DX11 + DirectComposition) | `shaders.hlsl` (+ 2) | 2598 + 1377 |
| `gpui_wgpu` | `wgpu_renderer.rs` (Linux, wgpu) | `shaders.wgsl` (+ subpixel) | — + 1483 |

The plan:

1. **Shader source of truth → one WGSL file, cross-compiled by `naga` at build time** to MSL + HLSL.
   *This is verified working* — see §2. Kills ~2,800 lines of hand-triplicated shader logic.
2. **Renderer → one wgpu-core renderer + thin per-platform shims** (`SurfaceProvider` +
   `ExternalTextureImporter`), reaching raw platform handles through `wgpu-hal` escape hatches only
   where genuinely needed. Kills ~5,700 lines across the three renderers + three atlas allocators,
   leaving ~350–550 lines of real per-platform shim.
3. **Struct drift → `encase`** collapses the six hand-copied `BlurParams`/instance structs to one.
4. **(Deferred) pure-Rust authoring → `schell/wgsl-rs`** once it reaches beta — same WGSL→naga waist,
   different front door. Not on the critical path.

The waist the whole system converges on is **WGSL IR → naga**. Everything above it (hand-WGSL or
pure-Rust eDSL) and below it (wgpu-core, or the native renderers during transition) mates there.

---

## 1. What verification changed

The earlier design passes asserted five "hard blockers." A verification pass that read wgpu/wgpu-hal
source at the **actually-resolved version (29.0.4**, not the 29.0.3 in `Cargo.toml` — Cargo locked to
the newest 29.x patch) and **ran naga over the real shaders** found that **four of the five dissolved**:

| Prior "blocker" | Verdict | Evidence |
|---|---|---|
| `presentsWithTransaction` unreachable via wgpu | **REFUTED** | `wgpu-hal/src/metal/surface.rs:31` exposes `pub fn render_layer(&self) -> &Mutex<Retained<CAMetalLayer>>`; reachable via `Surface::as_hal` (`wgpu/src/api/surface.rs:205`). wgpu-hal's `present()` already branches on the flag (`metal/mod.rs:603-625`). The cited issue #6891 was about a *texture* accessor, not the surface. |
| IOSurface / CVMetalTextureCache zero-copy import | **CONFIRMED reachable** | `wgpu_hal::metal::Device::texture_from_raw` (`metal/device.rs:358`); DX12 equivalent at `dx12/device.rs:448`. GPUI's two-single-plane design (R8Unorm + Rg8Unorm) sidesteps the NV12 rejection (#6921, closed not-planned). Proven end-to-end by the third-party `grafting` crate at wgpu 29. |
| DX12 + DirectComposition transparency (#7108) | **REFUTED (fixed)** | Issue closed/completed by merged PR #7117; fix present in 29.0.4 (`dx12/mod.rs:1367`, alpha-mode mapping at `auxil/dxgi/conv.rs:286`). |
| Memoryless textures missing in 29.x | **REFUTED** | Present in 29.0.4 as `TextureUsages::TRANSIENT` (renamed `TRANSIENT_ATTACHMENT` in 30.x); maps to `MTLStorageMode::Memoryless` at `metal/device.rs:492`. |
| naga WGSL→MSL/HLSL fidelity for flat + dual-source blend | **CONFIRMED clean** | Ran naga 29.0.4 over the real `shaders.wgsl` + `shaders_subpixel.wgsl`. See §2. |

No 29→30 bump is required for any of these. (A 30.x bump may be worth doing for unrelated reasons —
HDR surface config, DX12 optional vertex slots — but it is out of scope here.)

---

## 2. The naga spike — primary evidence the shader migration works

A throwaway project (`scratchpad/naga_spike/`, pinned `naga = "=29.0.4"`, features `wgsl-in, msl-out,
hlsl-out`) parsed GPUI's actual shaders and reproduced the exact runtime concatenation GPUI does at
`wgpu_renderer.rs:825` (`enable dual_source_blending;\n{base}\n{subpixel}`):

```
=== base shaders.wgsl ===
  parse: OK (50 types, 37 functions, 21 entry points)
  validate: OK
  MSL  generated OK  (67965 bytes, lang_version 2.4)
  HLSL generated OK  (82624 bytes)
=== combined (dual_source_blending) ===
  parse: OK (54 types, 37 functions, 23 entry points)
  validate: OK
  MSL  generated OK  (72002 bytes)
  HLSL generated OK  (88269 bytes)
```

**`@interpolate(flat)` lowers correctly:**
```
MSL:   metal::float4 border_color [[user(loc0), flat]];
HLSL:  nointerpolation float4 border_color : LOC0;
```

**`@blend_src(0/1)` (ClearType subpixel text) lowers correctly:**
```
MSL:   float4 foreground [[color(0) index(0)]];   float4 alpha [[color(0) index(1)]];
HLSL:  float4 foreground : SV_Target0;            float4 alpha : SV_Target1;
```

**Dynamic blur loop** (trip count from a uniform) validates clean; naga injects its `force_loop_bounding`
guard so GPU-side termination is guaranteed without a uniformity violation. **`dpdx/dpdy`** lower to
`dfdx/dfdy` (MSL) / `ddx/ddy` (HLSL). **`clip_distance`** is a non-issue — GPUI never uses the builtin;
it already does manual `clip_distances: vec4<f32>` + `any(... < 0.0)` clipping.

One gotcha, one open question:

- **Gotcha (resolved):** naga's `msl::Options::default()` uses `lang_version (1,0)`, which rejects
  `instance_id`. Not a real problem — wgpu-hal always sets `lang_version` from the live `MTLDevice`
  (3.0+ on Apple Silicon). Set `lang_version = (2,4)` in the build-time generator.
- **Open question (§4, R1):** naga emits `[[color(0) index(0)]]` **without a comma**; common MSL
  examples use `[[color(0), index(0)]]`. Could not compile-verify — the sandbox Xcode lacks the Metal
  Toolchain. Must be confirmed with `xcrun metal` on real hardware before trusting generated MSL for
  the subpixel-text pipeline. Isolated to that one pipeline; trivial post-process fix if needed.

Artifacts: `scratchpad/naga_spike/{out_base,out_combined}.{msl,hlsl}`.

---

## 3. Target architecture

### 3.1 Shader source

```
shaders.wgsl (+ shaders_subpixel.wgsl)          ← single hand-written source of truth
        │
        ├─ gpui_wgpu     : consumed directly (already WGSL-native)
        ├─ gpui_macos/build.rs : naga wgsl-in → msl-out  → OUT_DIR/*.metal → xcrun metal
        └─ gpui_windows/build.rs: naga wgsl-in → hlsl-out → OUT_DIR/*.hlsl  → fxc
```

`cbindgen`-style struct stitching (already used for Metal) is replaced by shared structs (§3.3). The
platform build scripts' compile steps (`metallib`, `fxc`) are untouched — only their *input* changes
from a checked-in file to a generated one.

### 3.2 Renderer

One `Renderer` over wgpu-core owns everything platform-agnostic: pipeline creation, scene→drawcall
translation, blur/path/text passes, atlas management (the `etagere`-based `wgpu_atlas.rs`, which
already subsumes the two bespoke `metal_atlas`/`directx_atlas` allocators). Two traits are the only
remaining `#[cfg(target_os)]` code:

- **`SurfaceProvider`** — surface creation + presentation tuning.
  - macOS: own the `CAMetalLayer` (via `Surface::as_hal`), set `presentsWithTransaction`, control
    present/commit timing for flicker-free live-resize.
  - Windows: build the `IDCompositionDevice`/`Visual`, hand it to wgpu via
    `SurfaceTargetUnsafe::CompositionVisual`.
  - Linux: none — `create_surface_unsafe` with the raw handle, as today.
- **`ExternalTextureImporter`** — zero-copy video.
  - macOS: `CVMetalTextureCache` → two `MTLTexture`s → `texture_from_raw` ×2 → `create_texture_from_hal`.
  - Windows: `OpenSharedHandle` → `texture_from_raw` (DX12).
  - Linux: unchanged — already produces a `wgpu::Texture` upstream.

Reference implementation for both importers: the `grafting` crate (`merely-made/wgpu-graft`, crates.io
`grafting` v0.5.0, targets wgpu 29) implements exactly this Metal-IOSurface and DX12-shared-texture
interop, including a wgpu 29↔30 compat shim. (Notably its workspace references a `glass-hq/gpui` fork
already attempting this same unification — prior art worth tracking.)

### 3.3 Struct sharing

Adopt `encase` (`#[derive(ShaderType)]`) for the uniform/instance structs in `crates/gpui/src/scene.rs`
(`Quad`, `Shadow`, `Underline`, `MonochromeSprite`, `PolychromeSprite`, `TransformationMatrix`,
`Background`/`Hsla`, and the `BlurParams` triad). This collapses the **six** hand-copied `BlurParams`
definitions (`metal_renderer.rs:180`, `directx_renderer.rs:1478`, `shaders.wgsl:1368`, each with a
comment promising it mirrors the others) into one Rust struct reused by all three crates for byte-upload.

---

## 4. Real blockers that survive verification

Only these remain. None is fatal; each has a defined workaround.

| # | Blocker | Severity | Workaround | Cost |
|---|---|---|---|---|
| R1 | naga MSL dual-source `[[color(0) index(0)]]` comma syntax unverified on `xcrun metal` | Low, isolated to subpixel text | On-device `xcrun metal` compile of the generated MSL; if rejected, post-process a comma in build.rs or file a naga PR | One CI job / one afternoon |
| R2 | wgpu path has **no** zero-copy surface import today — macOS/Windows `draw_surfaces` is a no-op stub (`wgpu_renderer.rs:1906`); Linux only accepts a pre-built `wgpu::Texture` | Medium — **net-new code**, not a port | Implement both importers per `grafting`'s `metal_texture_ref.rs` / `dx12_shared_texture.rs` | Medium; proven reference exists |
| R3 | Windows moves **DX11 → DX12** (wgpu's Windows backend). Explicit barriers, PSO caching, command lists — different semantics | High (behavioral, largest risk surface) | Dedicated correctness+perf pass; keep native DX11 renderer behind the existing non-default `wgpu` feature until parity proven | High; own validation phase |
| R4 | Upstream Zed **declined** to extend wgpu past Linux, citing native perf/behavior | Unknown — a prior, not a defect | Treat performance parity as a hard gate (§5). Budget for discovering *why* they held the line | Benchmarking discipline |
| R5 | Pure-Rust authoring (`wgsl-rs`) is pre-beta, solo-maintainer, git-only | N/A on critical path | Keep WGSL-text as source of truth; adopt `wgsl-rs` only after its beta / 0.1 crates.io release | Deferred (§6) |

What is **not** a blocker (verified): `presentsWithTransaction`, IOSurface/DComp/memoryless capability,
naga flat/blend_src/derivative/dynamic-loop fidelity, drawable-count mapping.

---

## 5. Definition of "real parity" — the gates

A phase does not advance until its gate is green.

- **Visual parity:** per-pipeline golden-image differential (single quad, dashed border at several radii,
  wavy underline, gradient in each color space, shadow, blur, video surface, subpixel text) across
  Metal/DX/wgpu within a perceptual tolerance (not exact bytes — GPU vendors already differ).
- **Performance parity:** frame time (p50/p99), GPU memory, atlas upload throughput vs the native
  renderer on representative scenes. This is the gate R4 demands.
- **Behavioral parity:** flicker-free live-resize (macOS `presentsWithTransaction`), zero-copy video
  playback (no per-frame copy), transparent/layered windows (premultiplied alpha), ClearType subpixel
  text on Windows, HiDPI.

---

## 6. Phased plan

### Phase 0 — Foundations, no user-visible change
- Add `naga` as a build-dependency; build.rs skeleton generates MSL/HLSL into `OUT_DIR` (not yet wired in).
- **Resolve R1:** on-device `xcrun metal` compile test of generated MSL in CI.
- Adopt `encase`; collapse the six `BlurParams` structs to one.
- Stand up the differential golden-image harness against the **existing three** renderers (baseline
  before any change). Headless Metal already exists (`metal_renderer.rs` `new_headless`); add WARP-based
  headless DX and software-Vulkan headless wgpu for CI.

### Phase 1 — WGSL becomes the shader source of truth  *(independent of the renderer collapse)*
- Point `gpui_macos/build.rs` at naga-generated MSL; keep hand-written `shaders.metal` behind a feature
  flag for A/B. Golden-diff, soak, then delete.
- Same for `gpui_windows/build.rs` → HLSL.
- Net: ~2,800 lines of triplicated shader logic deleted, all three backends fed from one WGSL file.
- Low risk, high value, fully verified — this phase pays for itself even if the renderer collapse never
  ships.

### Phase 2 — Renderer collapse onto wgpu-core  *(behind the existing non-default `wgpu` feature)*
- Generalize `wgpu_context.rs` to select backends per target (add `METAL` on macOS; DX12 already wired
  on Windows via the existing feature).
- Extract `SurfaceProvider` + `ExternalTextureImporter`; implement the macOS/Windows shims.
- **Build R2** (zero-copy importers) from the `grafting` reference.
- **Work R3** (DX11→DX12) with its own correctness+perf pass.
- Ship opt-in; run all three parity gates (§5) against the native renderers.

### Phase 3 — Flip default, delete native renderers
- After parity is green through a soak period: make wgpu the default; delete `metal_renderer.rs`,
  `directx_renderer.rs`, `metal_atlas.rs`, `directx_atlas.rs`, `directx_devices.rs`, the DX11
  `D3DCompile` module, `InstanceBufferPool`, and the native headless paths.
- Net: ~5,700 lines gone; ~350–550 lines of shim remain.

### Phase 4 — (Deferred) pure-Rust shader authoring via `wgsl-rs`
- Gate: `wgsl-rs` reaches beta / a real `0.1` crates.io release.
- Migrate authoring from WGSL-text to `wgsl-rs` Rust modules pipeline-by-pipeline; downstream naga waist
  is unchanged. Unlocks CPU-unit-testable shader logic and compile-time host/GPU type checking.
- Meanwhile, contribute upstream: exercise `wgsl-rs`'s `#[blend_src]` against GPUI's subpixel pipeline
  and fix its loop-lowering bug (#154) — high-leverage, de-risks future adoption.

---

## 7. Authoring landscape (why WGSL-text now, `wgsl-rs` later)

Pure-Rust *authoring* was re-swept 2026-08-27 with live maintenance dates:

- **`shame`** — WGSL-native, stable, *does* support flat interpolation (`fill_flat()` — earlier claim it
  didn't was wrong) and derivatives/dynamic loops. But **no commit on `main` in ~10 months** (last
  2025-10-17), crates.io stuck at 0.0.4 (2024), **no dual-source blend**, WGSL-only backend. Stalled.
- **`schell/wgsl-rs`** — the sleeper. Pure-Rust proc-macro eDSL on stable Rust, emits WGSL, naga-validated
  at `cargo test`. Source-confirmed support for **flat interpolation, `#[blend_src(N)]` dual-source blend,
  runtime-bound loops, derivatives**, plus genuine host/GPU struct sharing and CPU-runnable shader logic.
  Actively developed (168 commits, pushed 2026-08-23). **But** created 2025-12-05, self-described pre-beta,
  restricted Rust subset (no traits, limited generics, no borrowing), solo/NLnet-funded, **not on crates.io**
  (placeholder only). Adopting today means co-developing it. → **the one to bet on for Phase 4.**

  *Why it's the right bet, not a guess (verified 2026-08-27, primary sources):* wgsl-rs's author, Schell
  Scivally, is a **rust-gpu co-maintainer** (since Nov 2024) whose flagship renderer `schell/renderling`
  ran on rust-gpu in production for ~4 years. He built wgsl-rs *because* of rust-gpu's structural pain and
  is now migrating renderling off rust-gpu onto it. In his own words ("Introducing wgsl-rs", 2026-03-04):
  *"spirv-std hasn't been updated for over two years… the rest of the Rust-GPU maintainers have incorporated
  as 'vectorware' and are working on their product… I made a decision to rely on my own tech stack… [wgsl-rs]
  works on stable Rust. No custom compiler backend. No nightly. No SPIR-V step."* His 2026 roadmap explicitly
  lists *"Rewrite Renderling's internals on the new stack,"* and renderling's `main` carries the comment
  `// this is temporary given the wgsl-rs re-stacking happening this year (2026)`. Corroborating signal: as
  of 2026-08-27 renderling has been dormant ~5 months (last commit 2026-03-22) while wgsl-rs commits land
  weekly — the author's own effort has visibly shifted. So `renderling`-on-rust-gpu is evidence of rust-gpu's
  *cost* (nightly pin drift, unreleased `spirv-std`, maintainer/commercialization risk), not proof it's the
  safer dependency. wgsl-rs README also notes the two are "not mutually exclusive" — you can start on wgsl-rs
  and drop to rust-gpu for advanced features later, so the escape hatch exists.
- **Hand-built `naga::Module` IR** — fully capable (naga IR expresses flat + blend_src + derivatives on
  all three targets) but no ergonomic wrapper; hundreds of lines of arena/`Handle` bookkeeping per shader.
  A real fallback, budgeted as bespoke in-house code.
- **`rendiation` shader/api** — active, builds naga IR, but **panics on dual-source blend** and is
  monorepo-trapped (never published). Out.
- **rust-gpu** — nightly-only (structural), pre-production; its WGSL target stalled at alpha. Out.
- **CubeCL** — compute-only, no rasterizer. Definitively out.
- **Slang (Rust FFI)** — real Khronos-backed compiler emitting MSL/HLSL/WGSL/SPIR-V, but you write Slang
  (an HLSL-family language), not Rust — fails the "pure Rust" goal, and adds a native toolchain dep.

**Key correction from this sweep:** naga's own MSL/HLSL/WGSL backends now support dual-source blend
(`back/msl/mod.rs:749`, `back/hlsl/writer.rs:633`, `back/wgsl/writer.rs:587`). That gap — which had
disqualified every prior eDSL — is closed at the waist. So any authoring layer that bottoms out in naga
IR (WGSL-text, `wgsl-rs`, hand-built IR) can reach GPUI's full feature set today.

---

## 8. Open questions
1. **R1:** does `xcrun metal` accept naga's comma-less `[[color(0) index(0)]]`? (Phase 0 CI test.)
2. Performance parity of wgpu-core vs native Metal on macOS at GPUI's draw-call volumes — the R4 unknown.
3. DX11→DX12 behavioral audit: any reliance on `FLIP_SEQUENTIAL` back-buffer semantics or immediate-context
   timing (R3)?
4. Does `wgsl-rs` reach beta on a timeline worth waiting for, or should Phase 4 target hand-built naga IR
   for the subpixel pipeline specifically?
