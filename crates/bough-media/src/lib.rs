//! bough-media: images and other large content, stored by what is exposed (lane memory, round 7; design in
//! `docs/lanes/memory/MEDIA.md`). Platform-independent: memory is an anonymous mapping, decoders are pure Rust,
//! the GPU executors only wrap the pool's memory.
//!
//! * [`Media`] names content (a file, embedded bytes, owned bytes) by identity, without reading or decoding it.
//! * A **variant** is that content decoded to one device size and fit, cut into **tiles**: 64 x 64 RGBA8 texel
//!   slots of one 16 KiB page each, holding 62 x 62 texels of content and a one-texel apron (the neighbouring
//!   texels, edge-replicated at the image's border), so a scaled variant samples across tile seams exactly.
//! * The **pool** is the one place decoded texels live: the CPU executor reads it in place and a GPU executor
//!   wraps the same memory. There is no CPU copy beside a texture and no upload.
//! * **Exposure asks**: a variant is requested when a record showing it is staged for a frame (its pixels are
//!   damaged and visible); decoding runs on worker threads, newest request first; a finished decode lands at
//!   the start of a turn ([`Store::land`]) and the caller turns the landing into an ordinary paint write.
//!   Variants not exposed at the last [`Store::retain_exposed`] become cache, which [`Store::trim`] drops.
#![deny(unsafe_op_in_unsafe_fn)]

pub mod decode;
pub mod pool;
pub mod store;

pub use pool::Pool;
pub use store::{Fit, Media, Store, VariantId, VariantKey};

/// Texels per slot side.
pub const TILE: u32 = 64;
/// Content texels per tile side (the slot minus its one-texel apron on each side).
pub const INNER: u32 = 62;
/// Bytes per slot (RGBA8): one VM page on Apple silicon, four on x86-64 Linux.
pub const SLOT_BYTES: usize = (TILE * TILE * 4) as usize;
/// Texels per slot.
pub const SLOT_TEXELS: u32 = TILE * TILE;
