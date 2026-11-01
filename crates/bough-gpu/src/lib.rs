//! bough's GPU path on the vendored wgpu-hal Metal backend (ROUND4 §8). Shaders are wgsl-rs (`shaders`), compiled
//! to one metallib at build time (build.rs); the hal loads it with `ShaderInput::MetalLib`.
pub mod shaders;

/// The kernels, compiled at build time (wgsl-rs -> WGSL -> naga -> MSL -> metallib).
pub static METALLIB: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/bough_gpu.metallib"));

#[cfg(target_os = "macos")]
pub mod hal;
#[cfg(target_os = "macos")]
pub mod fused;
