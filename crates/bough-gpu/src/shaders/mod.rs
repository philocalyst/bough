//! bough's kernels in wgsl-rs: one Rust source, compiled to WGSL by wgsl-rs, to MSL by naga and to a metallib by
//! `xcrun metal` in build.rs (nothing parses or compiles a shader at runtime).
//!
//! Bindings (one layout for every kernel; `build.rs` maps them to Metal indices exactly as wgpu-hal's pipeline
//! layout assigns them):
//! * group 0: `POOL` the region (storage, read), `T0..T3` the surfaces (bgra8unorm storage, write), `R0..R3` the
//!   same surfaces (sampled, read), `ATLAS` the glyph atlas;
//!   `BINS` the row bins and `COUNTS` their counts (storage, read-write);
//! * group 1: `SLOT` the frame slot's byte offset in the region (uniform).
pub mod compact;
pub mod copy;
pub mod raster;
