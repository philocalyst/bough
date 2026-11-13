//! Text as staged data (round 8, text lane; `docs/r8/TEXT-SEAM.md`, `docs/lanes/text/JOURNAL.md`).
//!
//! The smallest real form of each mechanism, measured by `examples/stage_bench.rs`:
//!
//! * the memo is `bough_face::Glyphs`: shaped lines as ranges of two shared lanes, keyed by (face, text), with
//!   the shaper as a parameter (the prepared face, the table shaper, the full shaper).
//! * [`brk`]: the break table of a shaped text (two prefix sums per break opportunity) and `block(w)`: the
//!   line count at a width with the half-open interval of widths over which it holds, by one binary search
//!   per line. This is what the layout lane's `Measure::block` is answered from, with no glyphs.
//! * [`flow`]: a text that grows: an append costs the appended glyphs and the last line.
//! * [`doc`]: a document as blocks of lines; an edit touches one block.
//! * [`digits`]: a number as a glyph source: the k-th glyph is arithmetic on the value (`bough_face::num`).
//! * [`bake`]: the build step: a face at a size as a `bough_face` image the first frame reads in place.
//! * [`line`]: the line record's glyph words and a reference kernel over them.
pub mod bake;
pub mod brk;
pub mod digits;
pub mod doc;
pub mod flow;
pub mod line;
