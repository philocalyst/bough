#![feature(portable_simd)]
//! bough's one raster kernel and its executors.
//!
//! `kernel.rs` defines, once and in integers, what a pixel is:
//! `store8(fold(blend, 0, [shade(cmd, x, y) for cmd in program]))`. It is compiled by rustc for the CPU
//! (`cpu.rs`) and through wgsl-rs -> WGSL -> naga -> MSL for the GPU (`metal.rs`, macOS only). Nothing here is
//! a general compute layer: the kernel knows rounded boxes, borders, glyphs, shadows, gradients, transitions
//! and user nodes, and the executors exploit what GUI frames are.
//!
//! # Identities (each is a property of `kernel.rs`, tested in `tests/exact.rs`, and each removes work)
//! 1. **Superset.** `shade` is 0 outside a command's window and `blend(acc, 0) == acc`: any program that
//!    contains the commands touching a tile gives the same pixels. Executors may bin differently; a
//!    transition is binned once by its swept window.
//! 2. **Cut.** `blend(acc, sh) == sh` when `sh.a == ONE`: an opaque cover of a tile removes every command
//!    beneath it from that tile's program.
//! 3. **Core.** Inside `cmd.core`, `shade == core_value(cmd)`: spans on the CPU, 2 x 2 blocks on the GPU.
//! 4. **Flat rows.** On rows inside a box's core rows, `shade` does not depend on y: a pixel row whose
//!    commands are all flat equals the row above it and is copied, not shaded.
//! 5. **Translation.** `shade(cmd + d, p + d) == shade(cmd, p)` for whole-pixel d (Q8 geometry has no
//!    position-dependent rounding): tiles with equal origin-relative programs are one raster and N copies,
//!    and scrolled pixels equal re-rastered ones.
mod kernel;
pub mod cpu;
pub mod simd;
#[cfg(target_os = "macos")]
pub mod metal;
#[cfg(target_os = "linux")]
pub mod vulkan;
/// The GPU executor of this platform: Metal on macOS, Vulkan on Linux (the same API).
#[cfg(target_os = "macos")]
pub use metal as gpu;
#[cfg(target_os = "linux")]
pub use vulkan as gpu;

/// This platform's GPU executor over the kernel (Metal from its MSL, Vulkan from its SPIR-V); `None` without a
/// device.
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub fn gpu_executor() -> Option<gpu::Gpu> {
    #[cfg(target_os = "macos")]
    return metal::Gpu::new(KERNEL_MSL);
    #[cfg(target_os = "linux")]
    return vulkan::Gpu::new(KERNEL_SPIRV);
}
pub mod scenes;
pub mod stage;

pub use kernel::fx;
pub use kernel::kernel as k;
pub use k::Cmd;

/// The kernel as naga translated it (compiled by Metal at run time).
pub const KERNEL_MSL: &str = include_str!(concat!(env!("OUT_DIR"), "/kernel.metal"));
/// The kernel as naga's SPIR-V 1.3 (the Vulkan executor's module), little-endian words.
pub const KERNEL_SPIRV: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/kernel.spv"));
/// The kernel as wgsl-rs emitted it.
pub const KERNEL_WGSL: &str = include_str!(concat!(env!("OUT_DIR"), "/kernel.wgsl"));

/// FNV-1a over a frame's pixels (cross-host comparison of executors).
pub fn hash(px: &[u32]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for &p in px {
        for b in p.to_le_bytes() {
            h = (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}
