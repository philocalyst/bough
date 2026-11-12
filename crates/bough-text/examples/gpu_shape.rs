//! Attempt 3: shaping on the GPU. The CPU writes only the string bytes and a run table; a
//! compute kernel (one SIMD group per run) does the dense-table gather and the prefix sum,
//! and also the vertex shader's pixel snap. Exactness: every run's pens, snapped pixels and
//! trigger flag are read back and compared with the CPU kernels on the whole workload corpus
//! plus random strings. Then cost: CPU instructions per frame (bytes + encode + commit) and
//! GPU time per dispatch.
//!
//!   cargo run --release --example gpu_shape
//!
//! Metal only: on other platforms this example prints that and exits.

#[cfg(target_os = "macos")]
#[path = "common/corpus.rs"]
mod corpus;
#[cfg(target_os = "macos")]
#[path = "common/meter.rs"]
mod meter;

#[cfg(target_os = "macos")]
include!("common/gpu_shape_metal.rs");

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("gpu_shape: Metal only (macOS)");
}
