#![feature(const_type_name)]
//! Round 8, raster lane: paints as types.
//!
//! * `ops.rs`: the leaves, concrete integer functions in the wgsl-rs subset over the round-7 kernel's
//!   arithmetic. The only bodies.
//! * `expr.rs`: expressions as types (a mirror of the expr lane's seam); a combinator is a leaf's name.
//! * `paint.rs`: primitives and compositions as types; a paint drives a `Sink` by continuation.
//! * `tile.rs`: the CPU tile rasteriser as sinks, monomorphised per archetype.
//! * `scene.rs`: columns (the GPU's buffers), paint order, the window lane, bins, the CPU frame.
pub mod expr;
pub mod ops;
pub mod paint;
pub mod probe;
pub mod scene;
pub mod tile;
pub use ops::ops as x;
pub use wgsl_rs::std::{Vec4, Vec4i, Vec4u};
