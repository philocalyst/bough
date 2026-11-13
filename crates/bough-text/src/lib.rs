//! bough-text: table-driven text shaping for the common case, verified bit-identical against
//! a full OpenType shaper (rustybuzz) by exhaustive and corpus tests, writing straight into a
//! GPU-visible glyph pool, with retained labels updated as patches and damage emitted as GPU
//! tile marks. See `RESULTS.md`.
//!
//! * [`script`]: the per-font table (classes, kerning, triggers, ligature replay, token table)
//! * [`shape`]: the ASCII dense kernels (scalar, NEON), the token path, the pool slot kernels
//! * [`pool`]: the glyph pool (6 bytes per glyph), retained labels, delta updates, tile marks
//! * [`window`]: runs as windows onto shaped slots: wrapped lines, streams
//! * [`slots`]: one slot per byte for all covered text (phantoms), the classifying NEON kernel
//! * [`numeric`]: numeric labels rendered from the value, without `core::fmt`
//! * [`atlas`]: rasterization, the atlas, token entry tables, the 48-byte reference writer
//! * [`wrap`]: greedy wrapping over a `GlyphRun`

pub mod atlas;
pub mod lines;
pub mod model;
pub mod numeric;
pub mod para;
pub mod pool;
/// Round 8: text as staged data (the shape memo, break tables with validity intervals, line records).
pub mod stage;
/// The table, the kernels and byte slots live in the vendored parley now (graft 1 shapes parley's covered font
/// segments with them); re-exported under their old paths.
pub use parley::bough::{script, shape, slots};
pub mod window;
pub mod wrap;

/// The vendored parley (vendor/README.md): bough's text path, with bough-text's mechanisms grafted in.
pub use parley;
pub use script::{CpSet, SimpleScript, TRIG};
pub use shape::{GlyphRun, Shaper};
