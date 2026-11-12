//! bough-frame: the frame contract between the engine (the façade's runtime, bough-core's pages, bough-text's
//! glyph columns) and every executor that turns a frame into pixels: the Metal presenter (bough-metal), the
//! wgpu-hal presenter (bough-gpu) and the CPU executor. It holds data formats only, no platform code, so the
//! engine and the CPU executor build on every target and a GPU backend is one implementation among several.
//!
//! * [`instance`]: the 48-byte quad instance (`FInst` in the kernels) and its kinds;
//! * [`region`]: where each GPU-read column lives in the shared region ([`Layout`]), the capacity masks, the
//!   payload encodings of a record (text runs, instance runs), and the frame slot every executor reads its
//!   damaged tile rows from ([`write_slot`]).
//!
//! Moved here from bough-metal (round 7, platform lane); bough-metal re-exports every item under its old path.
pub mod instance;
pub mod region;

pub use instance::Instance;
pub use region::*;
