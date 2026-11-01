// Copyright 2026 the bough authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! bough's grafts into parley (vendor/README.md; crates/bough-text/RESULTS.md Part IV).
//!
//! These modules are bough-text's mechanisms, moved here so parley's own pipeline can use them. Every graft is
//! gated by `crates/bough-text/tests/parley_twin.rs`: a layout through the grafted parley equals unmodified
//! upstream parley's on the same input (glyphs, positions, clusters, lines, as f32 bits).
//!
//! * [`script`]: the per-font table (classes, kerning, triggers, ligature replay, token table), with the
//!   variable-font instance and the table cache.
//! * [`shape`]: the dense ASCII kernels (scalar, NEON), the token path, the byte-slot kernels.
//! * [`slots`]: one slot per byte for all covered text, the classifying NEON kernel.
//! * `fast` (graft 1): parley's shaping step takes the table for a covered font segment instead of harfrust.
#![allow(
    unsafe_code,
    missing_docs,
    unreachable_pub,
    missing_debug_implementations,
    trivial_numeric_casts,
    unused_qualifications,
    unnameable_types,
    elided_lifetimes_in_paths,
    clippy::all,
    clippy::pedantic,
    reason = "bough-text's code, moved verbatim: its own lint policy (unsafe with SAFETY arguments, NEON kernels)"
)]

pub(crate) mod fast;
pub mod script;
pub mod shape;
pub mod slots;

std::thread_local! {
    static SEGMENTS: core::cell::Cell<[u64; 2]> = const { core::cell::Cell::new([0; 2]) };
    static SIMPLE: core::cell::Cell<[u64; 2]> = const { core::cell::Cell::new([0; 2]) };
}

/// Counts a shaped font segment (`fast`: by bough's table; else by harfrust). Per thread.
#[inline]
pub(crate) fn count_segment(fast: bool) {
    SEGMENTS.with(|c| {
        let mut v = c.get();
        v[fast as usize] += 1;
        c.set(v);
    });
}

/// `[harfrust, table]` font segments shaped on this thread so far (tests assert the fast path is taken).
pub fn segments() -> [u64; 2] {
    SEGMENTS.with(|c| c.get())
}

/// Counts a text analysis (`simple`: graft 3's pair rules; else ICU's segmenters). Per thread.
#[inline]
pub(crate) fn count_analysis(simple: bool) {
    SIMPLE.with(|c| {
        let mut v = c.get();
        v[simple as usize] += 1;
        c.set(v);
    });
}

/// `[ICU, simple]` text analyses on this thread so far.
pub fn analyses() -> [u64; 2] {
    SIMPLE.with(|c| c.get())
}
