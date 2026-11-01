// Copyright 2026 the bough authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Graft 1: the shaping fast path. A font segment of an item that harfrust would shape left to right with the
//! default features, no language and the Latin script is shaped by bough's table instead when the table accepts
//! the text (every codepoint covered, no trigger pair). The result is the same glyph stream harfrust produces
//! (gate: `tests/parley_twin.rs`), fed to the same cluster code (`LayoutData::push_shaped_run`). Anything else
//! goes to harfrust.
//!
//! The table is a pure function of the font's bytes and its variation instance, so it is cached per font blob,
//! face index and user-space variation settings (and on disk by `SimpleScript::build_cached`).

use std::prelude::rust_2024::*;

use super::script::SimpleScript;
use super::shape::{GlyphRun, Shaper};
use crate::layout::data::ShapedGlyph;

struct Entry {
    blob: u64,
    index: u32,
    vars: Vec<([u8; 4], f32)>,
    /// `None`: the font has no simple-script model (the segment goes to harfrust).
    shaper: Option<Box<Shaper>>,
    /// The instance's normalized coordinates, as harfrust computes them (the run records them).
    coords: Vec<harfrust::NormalizedCoord>,
}

#[derive(Default)]
pub(crate) struct FastShaper {
    tables: Vec<Entry>,
    /// AAT tracking per (entry, point size): harfrust adds it to every grapheme's advance when the font has `trak`
    /// and `STAT` (one integer per size; 0 for every other font).
    tracks: Vec<(usize, u32, i32)>,
    run: GlyphRun,
    /// Index of the entry used by the last successful `shape`, and its tracking at that size.
    last: usize,
    track: i32,
    /// Graft 5: one shaped run per font segment of a simple build (glyph run, entry, tracking).
    segs: Vec<(GlyphRun, usize, i32)>,
}

/// Whether every character of a text can be in a table's covered set: ASCII 0x20..=0x7E and two-byte Latin-1
/// 0xA0..=0xFF (C2 A0..BF without C2 AD, C3 80..BF). Controls, C1 controls and the soft hyphen are never covered.
#[inline]
pub(crate) fn maybe_covered(s: &[u8]) -> bool {
    let mut i = 0;
    while i < s.len() {
        let b = s[i];
        if b < 0x80 {
            if !(0x20..0x7F).contains(&b) {
                return false;
            }
            i += 1;
        } else {
            match (b, s.get(i + 1)) {
                (0xC2, Some(&c)) if c >= 0xA0 && c != 0xAD => {}
                (0xC3, Some(_)) => {}
                _ => return false,
            }
            i += 2;
        }
    }
    true
}

/// Whether a text can be in the table's covered set at all (ASCII and two-byte Latin-1, lead bytes C2/C3): a
/// cheap filter so fonts that only ever shape other scripts never build a table.
#[inline]
fn latin1_only(s: &[u8]) -> bool {
    s.iter().all(|&b| b < 0xC4)
}

impl FastShaper {
    fn entry(&mut self, blob: u64, index: u32, data: &[u8], vars: &[harfrust::Variation]) -> usize {
        let same = |e: &Entry| {
            e.blob == blob
                && e.index == index
                && e.vars.len() == vars.len()
                && e.vars.iter().zip(vars).all(|(a, b)| a.0 == b.tag.to_be_bytes() && a.1.to_bits() == b.value.to_bits())
        };
        if let Some(i) = self.tables.iter().position(same) {
            return i;
        }
        let uvars: Vec<([u8; 4], f32)> = vars.iter().map(|v| (v.tag.to_be_bytes(), v.value)).collect();
        let script = if uvars.is_empty() {
            SimpleScript::build_cached(data, index)
        } else {
            SimpleScript::build_instance_cached(data, index, &uvars)
        };
        let coords = harfrust::FontRef::from_index(data, index)
            .map(|f| harfrust::ShaperInstance::from_variations(&f, vars.iter().copied()).coords().to_vec())
            .unwrap_or_default();
        self.tables.push(Entry { blob, index, vars: uvars, shaper: script.map(|s| Box::new(Shaper::new(s))), coords });
        self.tables.len() - 1
    }

    /// Whether this font instance has a table covering every character of `text` (`maybe_covered` text).
    pub(crate) fn covers(&mut self, blob: u64, index: u32, data: &[u8], vars: &[harfrust::Variation], text: &str) -> bool {
        let i = self.entry(blob, index, data, vars);
        let Some(sh) = self.tables[i].shaper.as_deref() else { return false };
        let cov = &sh.script.covered;
        let s = text.as_bytes();
        let mut k = 0;
        while k < s.len() {
            let (cp, n) = match s[k] {
                b if b < 0x80 => (b, 1),
                0xC2 => (s[k + 1], 2),
                _ => (s[k + 1] + 0x40, 2),
            };
            if !cov.contains(cp) {
                return false;
            }
            k += n;
        }
        true
    }

    /// Shapes `text` with the table of this font instance. `false` sends the segment to harfrust.
    pub(crate) fn shape(&mut self, blob: u64, index: u32, data: &[u8], vars: &[harfrust::Variation], text: &str, size: f32) -> bool {
        if !latin1_only(text.as_bytes()) {
            return false;
        }
        let i = self.entry(blob, index, data, vars);
        let Some(sh) = self.tables[i].shaper.as_deref() else { return false };
        if !sh.shape_into(text, size, &mut self.run) {
            return false;
        }
        self.last = i;
        self.track = match self.tracks.iter().find(|t| t.0 == i && t.1 == size.to_bits()) {
            Some(t) => t.2,
            None => {
                let t = track(data, index, vars, size, self.tables[i].shaper.as_deref().unwrap());
                self.tracks.push((i, size.to_bits(), t));
                t
            }
        };
        true
    }

    /// Graft 5: shapes segment `k` of a simple build into its own slot (`false`: the table refuses it).
    pub(crate) fn shape_seg(&mut self, k: usize, blob: u64, index: u32, data: &[u8], vars: &[harfrust::Variation], text: &str, size: f32) -> bool {
        if !self.shape(blob, index, data, vars, text, size) {
            return false;
        }
        if self.segs.len() <= k {
            self.segs.resize_with(k + 1, Default::default);
        }
        let seg = &mut self.segs[k];
        core::mem::swap(&mut seg.0, &mut self.run);
        seg.1 = self.last;
        seg.2 = self.track;
        true
    }

    /// Segment `k` of a simple build: its slots and its instance's normalized coordinates.
    pub(crate) fn seg(&self, k: usize) -> (crate::layout::data::SlotRun<'_>, &[harfrust::NormalizedCoord]) {
        let (r, e, track) = &self.segs[k];
        (
            crate::layout::data::SlotRun {
                gid: &r.gid,
                x: &r.x,
                cluster: (!r.identity_clusters).then_some(&r.cluster[..]),
                width: r.width,
                track: *track,
            },
            &self.tables[*e].coords,
        )
    }

    /// The normalized coordinates of the last shaped segment's instance.
    pub(crate) fn coords(&self) -> &[harfrust::NormalizedCoord] {
        &self.tables[self.last].coords
    }

    /// The last shaped segment as harfrust's glyph stream: font units, zero offsets (an offset is a trigger),
    /// `cluster` = the char index of the glyph's first character in `text` (parley adds chars with their index).
    pub(crate) fn glyphs<'a>(&'a self, text: &'a str) -> impl Iterator<Item = ShapedGlyph> + 'a {
        let r = &self.run;
        let bytes = text.as_bytes();
        let (mut byte, mut ch) = (0usize, 0u32);
        (0..r.len()).map(move |k| {
            let c = r.cluster_of(k) as usize;
            // Count the chars between the previous glyph's cluster and this one (non-continuation bytes).
            while byte < c {
                ch += (bytes[byte] & 0xC0 != 0x80) as u32;
                byte += 1;
            }
            let next = if k + 1 < r.len() { r.x[k + 1] } else { r.width };
            ShapedGlyph { id: r.gid[k] as u32, cluster: ch, x_advance: next - r.x[k] + self.track, x_offset: 0, y_offset: 0 }
        })
    }
}

/// The AAT tracking harfrust adds to each grapheme's advance at `size`: read from harfrust itself (a space shaped
/// alone, minus the table's advance for it), so the value is harfrust's by construction. 0 without `trak`+`STAT`.
fn track(data: &[u8], index: u32, vars: &[harfrust::Variation], size: f32, sh: &Shaper) -> i32 {
    use harfrust::{Direction, ShapeOptions, ShapePlan, ShaperData, ShaperInstance, UnicodeBuffer};
    let Ok(f) = harfrust::FontRef::from_index(data, index) else { return 0 };
    use skrifa::raw::TableProvider as _;
    if f.trak().is_err() || f.stat().is_err() {
        return 0;
    }
    let data_ = ShaperData::new(&f);
    let inst = ShaperInstance::from_variations(&f, vars.iter().copied());
    let hs = data_.shaper(&f).instance(Some(&inst)).build();
    let latin = harfrust::Script::from_iso15924_tag(harfrust::Tag::new(b"Latn")).unwrap();
    let plan = ShapePlan::new(&hs, Direction::LeftToRight, Some(latin), None, &[]);
    let mut b = UnicodeBuffer::new();
    b.add(' ', 0);
    b.set_direction(Direction::LeftToRight);
    b.set_script(latin);
    let g = hs.shape(b, ShapeOptions::new().plan(Some(&plan)).point_size(Some(size)));
    let mut run = GlyphRun::default();
    if !sh.shape_into(" ", size, &mut run) {
        return 0;
    }
    g.glyph_positions()[0].x_advance - run.width
}
