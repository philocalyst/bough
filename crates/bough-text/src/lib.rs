//! bough-text: table-driven text shaping for the common case, verified bit-identical against
//! a full OpenType shaper (rustybuzz) by exhaustive and corpus tests; plus a glyph atlas and
//! instance generation. See `RESULTS.md`.

pub mod atlas;
pub mod script;
pub mod shape;
pub mod wrap;

pub use script::{CpSet, SimpleScript, TRIG};
pub use shape::{GlyphRun, Shaper};
