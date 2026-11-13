//! A face at a size, prepared (round 8, text lane; `docs/r8/TEXT-SEAM.md` part B and revision 2).
//!
//! What a frame needs from a font is where each glyph goes and what it covers. For one face at one size
//! both are constants, so they are made when the application is built (`bough_text::stage::bake`, by the same
//! table shaper and rasteriser the façade uses, gated against them) and read in place from rodata:
//!
//! * [`Face`]: the image. Shaping a string it covers is arithmetic over its bytes ([`Face::shape`]): a token
//!   per glyph (an ASCII byte, a Latin-1 code point or a ligature), a pen step per pair of tokens. A string it
//!   does not cover (another script, a pair that needs contextual shaping) is refused and belongs to the full
//!   shaper, which is never on the path to the first frame.
//! * [`Glyphs`]: shaped lines as ranges of two shared lanes, 6 bytes per glyph (a glyph slot and a pen in font
//!   units), memoised by (face, text) so that describing the same text again gives the same range. A line
//!   record is two words ([`Run`]); nothing is placed in pixels until it is painted.
//! * [`Face::spans`]: the kernel. The pixel position of a glyph is one multiply and a rounding in integers
//!   ([`Face::px`]), the same on any executor; coverage rows are handed out cut to a clip.
//! * [`num`]: a number's glyphs and pens as arithmetic on its value: no shaping, no lanes.
#![no_std]
extern crate alloc;

mod face;
mod glyphs;
pub mod num;

pub use face::{Cell, Face, Slot, MAGIC, TOK_PAD};
pub use glyphs::{key, Glyphs, Run};
