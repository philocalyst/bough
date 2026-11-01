# Vendored GPU and windowing stack (ROUND4 §8)

Copied verbatim from the crates.io sources of the versions below (the registry packages, `Cargo.toml` as published;
`.cargo_vcs_info.json` and `Cargo.toml.orig` dropped). The workspace resolves every use of these crates to these
copies through `[patch.crates-io]` in the root `Cargo.toml`, and `exclude = ["vendor"]` keeps them out of
`cargo test --workspace`.

| crate | version | upstream commit | used by |
|---|---|---|---|
| wgpu | 29.0.4 | gfx-rs/wgpu e99f5305ded96ff7006f0714d043a7f735bd45c2 | (not on the frame path; for the wgpu-core comparison) |
| wgpu-core | 29.0.4 | e99f5305 | (same) |
| wgpu-hal | 29.0.4 | e99f5305 | `crates/bough-gpu` (Metal backend, the frame path) |
| wgpu-types | 29.0.4 | e99f5305 | `crates/bough-gpu` |
| naga | 29.0.4 | e99f5305 | `crates/bough-gpu/build.rs` (WGSL -> MSL at build time); wgpu-hal's `metal` feature links it but bough never calls it at runtime |
| wgpu-naga-bridge | 29.0.4 | e99f5305 | wgpu-core |
| wgpu-core-deps-apple | 29.0.4 | e99f5305 | wgpu-core |
| winit | 0.30.13 | rust-windowing/winit e9809ef54b18499bb4f2cac945719ecc2a61061b | (the host port, not started) |

Version choice: gpui-ce (`~/projects/gpui-ce3` origin/main) pins `wgpu = "29.0.3"` and `wgsl-rs` at
schell/wgsl-rs `b2de16a1b8`, whose naga is 29; 29.0.4 is that line's latest patch release. (The archive's
`codex/wgpu30` moved gpui to wgpu 30 with a newer wgsl-rs; not needed here.)

## Local changes

None yet. What the hal already exposes covers the first increment: `Device::device_from_raw`,
`Queue::queue_from_raw` / `as_raw`, `Device::buffer_from_raw` (the `storageModeShared` region the CPU writes in
place), `Device::texture_from_raw` (IOSurface-backed surfaces), `ShaderInput::MetalLib` (the build-time metallib),
`CommandEncoder::raw_command_buffer` (shared-event waits and signals between passes of a pre-armed batch), and
`Fence::raw_shared_event`.

Planned extensions (each a clean addition in `wgpu-hal/src/metal`, listed here when made):
* a bindless table: write surface / atlas resource ids into the region and bind only the region and the slot
  offset per encoder (today's hal bind group calls `setTexture` for every surface on every pre-armed encoder, which
  costs per frame and makes pending frames hold every IOSurface "in use");
* residency-set membership for wrapped resources (`addAllocation` on the hal queue's set);
* CAMetalLayer-free presentation of IOSurface contents (the publisher thread's transaction).
