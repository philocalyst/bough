// Copyright 2021 the Parley Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Text shaping implementation using `harfrust`for shaping
//! and `icu` for text analysis.

use alloc::vec::Vec;
use core::mem;
use core::ops::RangeInclusive;
use harfrust::ShapeOptions;

use super::layout::Layout;
use super::resolve::{ResolveContext, Resolved, ResolvedStyle};
use super::style::{Brush, FontFeature, FontVariation};
use crate::analysis::cluster::{Char, CharCluster, Status};
use crate::analysis::{AnalysisDataSources, CharInfo};
use crate::convert::script_to_harfrust;
use crate::inline_box::InlineBox;
use crate::lru_cache::LruCache;
use crate::util::nearly_eq;
use crate::{FontData, convert};
use fontique::Language;
use icu_properties::props::Script;

use fontique::{self, Query, QueryFamily, QueryFont};

mod cache;

pub(crate) struct ShapeContext {
    shape_data_cache: LruCache<cache::ShapeDataKey, harfrust::ShaperData>,
    shape_instance_cache: LruCache<cache::ShapeInstanceId, harfrust::ShaperInstance>,
    shape_plan_cache: LruCache<cache::ShapePlanId, harfrust::ShapePlan>,
    unicode_buffer: Option<harfrust::UnicodeBuffer>,
    features: Vec<harfrust::Feature>,
    char_cluster: CharCluster,
    /// bough graft: run metrics per (font, size, coordinates).
    metrics: crate::layout::data::MetricsCache,
    /// bough graft 1: the table shaper for covered font segments, and its variation scratch.
    #[cfg(feature = "std")]
    fast: crate::bough::fast::FastShaper,
    #[cfg(feature = "std")]
    fast_vars: Vec<harfrust::Variation>,
    /// bough graft 2: the item's font segments (byte range in the item, font).
    #[cfg(feature = "std")]
    fast_runs: Vec<(usize, usize, SelectedFont)>,
    /// bough graft 3b: per-character boundaries and properties (scratch).
    #[cfg(feature = "std")]
    pub(crate) bounds: Vec<u8>,
    #[cfg(feature = "std")]
    pub(crate) props: Vec<u8>,
}

impl Default for ShapeContext {
    fn default() -> Self {
        const MAX_ENTRIES: usize = 16;
        Self {
            shape_data_cache: LruCache::new(MAX_ENTRIES),
            shape_instance_cache: LruCache::new(MAX_ENTRIES),
            shape_plan_cache: LruCache::new(MAX_ENTRIES),
            unicode_buffer: Some(harfrust::UnicodeBuffer::new()),
            features: Vec::new(),
            char_cluster: CharCluster::default(),
            metrics: Default::default(),
            #[cfg(feature = "std")]
            fast: Default::default(),
            #[cfg(feature = "std")]
            fast_vars: Vec::new(),
            #[cfg(feature = "std")]
            fast_runs: Vec::new(),
            #[cfg(feature = "std")]
            bounds: Vec::new(),
            #[cfg(feature = "std")]
            props: Vec::new(),
        }
    }
}

struct Item {
    style_index: u16,
    size: f32,
    script: Script,
    level: u8,
    locale: Option<Language>,
    variations: Resolved<FontVariation>,
    features: Resolved<FontFeature>,
    word_spacing: f32,
    letter_spacing: f32,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn shape_text<'a, B: Brush>(
    rcx: &'a ResolveContext,
    mut fq: Query<'a>,
    styles: &'a [ResolvedStyle<B>],
    inline_boxes: &[InlineBox],
    infos: &[(CharInfo, u16)],
    levels: &[u8],
    scx: &mut ShapeContext,
    mut text: &str,
    layout: &mut Layout<B>,
    analysis_data_sources: &AnalysisDataSources,
) {
    // If we have both empty text and no inline boxes, shape with a fake space
    // to generate metrics that can be used to size a cursor.
    if text.is_empty() && inline_boxes.is_empty() {
        text = " ";
    }
    // Do nothing if there is no text or styles (there should always be a default style)
    if text.is_empty() || styles.is_empty() {
        // Process any remaining inline boxes whose index is greater than the length of the text
        for box_idx in 0..inline_boxes.len() {
            // Push the box to the list of items
            layout.data.push_inline_box(box_idx);
        }
        return;
    }

    // Setup mutable state for iteration
    let initial_style_index = infos.first().map_or(0, |(_, style_index)| *style_index);
    let mut style = &styles[initial_style_index as usize];
    let mut item = Item {
        style_index: initial_style_index,
        size: style.font_size,
        level: levels.first().copied().unwrap_or(0),
        script: infos
            .iter()
            .map(|x| x.0.script)
            .find(|&script| real_script(script))
            .unwrap_or(Script::Latin),
        locale: style.locale,
        variations: style.font_variations,
        features: style.font_features,
        word_spacing: style.word_spacing,
        letter_spacing: style.letter_spacing,
    };

    let mut char_range = 0..0;
    let mut text_range = 0..0;

    let mut inline_box_iter = inline_boxes.iter().enumerate();
    let mut current_box = inline_box_iter.next();

    // Iterate over characters in the text
    for ((char_index, (byte_index, ch)), (info, style_index)) in
        text.char_indices().enumerate().zip(infos)
    {
        let mut break_run = false;
        let mut script = info.script;
        if !real_script(script) {
            script = item.script;
        }
        let level = levels.get(char_index).copied().unwrap_or(0);
        if item.style_index != *style_index {
            item.style_index = *style_index;
            style = &styles[*style_index as usize];
            if !nearly_eq(style.font_size, item.size)
                || style.locale != item.locale
                || style.font_variations != item.variations
                || style.font_features != item.features
                || !nearly_eq(style.letter_spacing, item.letter_spacing)
                || !nearly_eq(style.word_spacing, item.word_spacing)
            {
                break_run = true;
            }
        }

        if level != item.level || script != item.script {
            break_run = true;
        }

        // Check if there is an inline box at this index
        // Note:
        //   - We loop because there may be multiple boxes at this index
        //   - We do this *before* processing the text run because we need to know whether we should
        //     break the run due to the presence of an inline box.
        let mut deferred_boxes: Option<RangeInclusive<usize>> = None;
        while let Some((box_idx, inline_box)) = current_box {
            if inline_box.index == byte_index {
                break_run = true;
                if let Some(boxes) = &mut deferred_boxes {
                    deferred_boxes = Some((*boxes.start())..=box_idx);
                } else {
                    deferred_boxes = Some(box_idx..=box_idx);
                };
                // Update the current box to the next box
                current_box = inline_box_iter.next();
            } else {
                break;
            }
        }

        if break_run && !text_range.is_empty() {
            shape_item(
                &mut fq,
                rcx,
                styles,
                &item,
                scx,
                text,
                &text_range,
                &char_range,
                infos,
                layout,
                analysis_data_sources,
            );
            item.size = style.font_size;
            item.level = level;
            item.script = script;
            item.locale = style.locale;
            item.variations = style.font_variations;
            item.features = style.font_features;
            item.word_spacing = style.word_spacing;
            item.letter_spacing = style.letter_spacing;
            text_range.start = text_range.end;
            char_range.start = char_range.end;
        }

        if let Some(deferred_boxes) = deferred_boxes {
            for box_idx in deferred_boxes {
                layout.data.push_inline_box(box_idx);
            }
        }

        text_range.end += ch.len_utf8();
        char_range.end += 1;
    }

    if !text_range.is_empty() {
        shape_item(
            &mut fq,
            rcx,
            styles,
            &item,
            scx,
            text,
            &text_range,
            &char_range,
            infos,
            layout,
            analysis_data_sources,
        );
    }

    // Process any remaining inline boxes whose index is greater than the length of the text
    if let Some((box_idx, _inline_box)) = current_box {
        layout.data.push_inline_box(box_idx);
    }
    for (box_idx, _inline_box) in inline_box_iter {
        layout.data.push_inline_box(box_idx);
    }
}

// Rebuilds the provided `char_cluster` in-place using the existing allocation
// for the given grapheme `segment_text`, consuming items from `item_infos_iter`.
fn fill_cluster_in_place(
    segment_text: &str,
    item_infos_iter: &mut core::slice::Iter<'_, (CharInfo, u16)>,
    code_unit_offset_in_string: &mut usize,
    char_cluster: &mut CharCluster,
) {
    // Reset cluster but keep allocation
    char_cluster.clear();

    let mut force_normalize = false;
    let mut is_emoji_or_pictograph = false;
    let mut map_len: u8 = 0;
    let start = *code_unit_offset_in_string as u32;

    for ((_, ch), (info, style_index)) in segment_text.char_indices().zip(item_infos_iter.by_ref())
    {
        force_normalize |= info.force_normalize();
        // TODO - make emoji detection more complete, as per (except using composite Trie tables as
        //  much as possible:
        //  https://github.com/conor-93/parley/blob/4637d826732a1a82bbb3c904c7f47a16a21cceec/parley/src/shape/mod.rs#L221-L269
        is_emoji_or_pictograph |= info.is_emoji_or_pictograph();
        *code_unit_offset_in_string += ch.len_utf8();

        // TODO: Explore ignoring other modifiers in determining `contributes_to_shaping`:
        //  regional indicators, subdivision flag tag sequences, skin tone modifiers
        //  See also: https://github.com/google/emoji-segmenter

        // If the color emoji has a non-printing variation selector, ignore the variation selector.
        // Its presentation depends on the platform and font.
        //
        // e.g.
        //  - `U+270C + U+FE0F`: `✌`, force basic presentation
        //  - `U+270C + U+FE0F`: `✌️`, force emoji presentation
        //
        // <https://www.unicode.org/reports/tr37/>
        let is_emoji_with_non_printing_variation_selector =
            is_emoji_or_pictograph && info.is_variation_selector();

        let contributes_to_shaping =
            info.contributes_to_shaping() && !is_emoji_with_non_printing_variation_selector;
        if contributes_to_shaping {
            map_len += 1;
        }

        char_cluster.chars.push(Char {
            ch,
            contributes_to_shaping,
            glyph_id: 0,
            style_index: *style_index,
            is_control_character: info.is_control(),
        });
    }

    // Finalize cluster metadata
    let end = *code_unit_offset_in_string as u32;
    char_cluster.is_emoji = is_emoji_or_pictograph;
    char_cluster.map_len = map_len;
    char_cluster.start = start;
    char_cluster.end = end;
    char_cluster.force_normalize = force_normalize;
}

fn shape_item<'a, B: Brush>(
    fq: &mut Query<'a>,
    rcx: &'a ResolveContext,
    styles: &'a [ResolvedStyle<B>],
    item: &Item,
    scx: &mut ShapeContext,
    text: &str,
    text_range: &core::ops::Range<usize>,
    char_range: &core::ops::Range<usize>,
    infos: &[(CharInfo, u16)],
    layout: &mut Layout<B>,
    analysis_data_sources: &AnalysisDataSources,
) {
    let item_text = &text[text_range.clone()];
    let item_infos = &infos[char_range.start..char_range.end]; // Only process current item
    let first_style_index = item_infos[0].1;
    let fb_script = convert::script_to_fontique(item.script, analysis_data_sources);
    let mut font_selector =
        FontSelector::new(fq, rcx, styles, first_style_index, fb_script, item.locale);

    // bough graft 2: one font selection per style run instead of one per grapheme cluster, when the font chosen
    // for the run's first character was the query's first candidate and bough's table covers the whole run.
    #[cfg(feature = "std")]
    if fast_eligible(rcx, item)
        && fast_item(
            &mut font_selector,
            rcx,
            item,
            scx,
            item_text,
            item_infos,
            text_range,
            char_range,
            fb_script,
            layout,
            analysis_data_sources,
        )
    {
        return;
    }

    let grapheme_cluster_boundaries = analysis_data_sources
        .grapheme_segmenter()
        .segment_str(item_text);
    let mut item_infos_iter = item_infos.iter();
    let mut code_unit_offset_in_string = text_range.start;
    // Owned for the loop so `scx` can be passed to `shape_segment` (bough refactor for graft 2); restored after.
    let mut char_cluster_owned = mem::take(&mut scx.char_cluster);
    let char_cluster = &mut char_cluster_owned;

    // Build an iterator of boundaries and consume the first segment to seed the loop
    let mut boundaries_iter = grapheme_cluster_boundaries.skip(1);
    let mut last_boundary = 0_usize;
    let Some(mut current_boundary) = boundaries_iter.next() else {
        return; // No clusters
    };

    fill_cluster_in_place(
        &item_text[last_boundary..current_boundary],
        &mut item_infos_iter,
        &mut code_unit_offset_in_string,
        char_cluster,
    );

    let mut current_font = font_selector.select_font(char_cluster, analysis_data_sources);

    // Main segmentation loop (based on swash shape_clusters) - only within current item
    while let Some(font) = current_font.take() {
        // Collect all clusters for this font segment
        let cluster_range = char_cluster.range();
        let segment_start_offset = cluster_range.start as usize - text_range.start;
        let mut segment_end_offset = cluster_range.end as usize - text_range.start;

        for next_boundary in boundaries_iter.by_ref() {
            // Build next cluster in-place
            last_boundary = current_boundary;
            current_boundary = next_boundary;
            fill_cluster_in_place(
                &item_text[last_boundary..current_boundary],
                &mut item_infos_iter,
                &mut code_unit_offset_in_string,
                char_cluster,
            );

            if let Some(next_font) = font_selector.select_font(char_cluster, analysis_data_sources)
            {
                if next_font != font {
                    current_font = Some(next_font);
                    break;
                } else {
                    // Same font - add to current segment
                    segment_end_offset = char_cluster.range().end as usize - text_range.start;
                }
            } else {
                // No font determined, continue to next cluster
                continue;
            }
        }

        shape_segment(
            rcx,
            item,
            scx,
            &font,
            item_text,
            item_infos,
            text_range,
            char_range,
            segment_start_offset,
            segment_end_offset,
            fb_script,
            layout,
        );
    }
    scx.char_cluster = char_cluster_owned;
}

/// Shapes one font segment of an item (`item_text[segment_start_offset..segment_end_offset]`, every cluster on
/// `font`) and pushes its run: bough's table first (graft 1), harfrust otherwise. Upstream's loop body, unchanged.
#[allow(clippy::too_many_arguments)]
fn shape_segment<B: Brush>(
    rcx: &ResolveContext,
    item: &Item,
    scx: &mut ShapeContext,
    font: &SelectedFont,
    item_text: &str,
    item_infos: &[(CharInfo, u16)],
    text_range: &core::ops::Range<usize>,
    char_range: &core::ops::Range<usize>,
    segment_start_offset: usize,
    segment_end_offset: usize,
    fb_script: fontique::Script,
    layout: &mut Layout<B>,
) {
    // Shape this font segment with harfrust
    let segment_text = &item_text[segment_start_offset..segment_end_offset];
    // Shape the entire segment text including newlines
    // The line breaking algorithm will handle newlines automatically

    // bough graft 1: a left-to-right Latin segment with the default features and no language is shaped by
    // bough's table when it accepts the text (the same glyph stream harfrust would produce).
    #[cfg(feature = "std")]
    if fast_eligible(rcx, item) {
        scx.fast_vars.clear();
        scx.fast_vars.extend(variations_iter(&font.font.synthesis, rcx.variations(item.variations)));
        if scx.fast.shape(
            font.font.blob.id(),
            font.font.index,
            font.font.blob.as_ref(),
            &scx.fast_vars,
            segment_text,
            item.size,
        ) {
            let char_start = char_range.start + item_text[..segment_start_offset].chars().count();
            let segment_char_start = char_start - char_range.start;
            let segment_char_count = segment_text.chars().count();
            let segment_infos =
                &item_infos[segment_char_start..(segment_char_start + segment_char_count)];
            layout.data.push_shaped_run(
                FontData::new(font.font.blob.clone(), font.font.index),
                item.size,
                font.attrs,
                font.font.synthesis,
                scx.fast.glyphs(segment_text),
                item.level,
                item.style_index,
                item.word_spacing,
                item.letter_spacing,
                segment_text,
                segment_infos,
                (text_range.start + segment_start_offset)..(text_range.start + segment_end_offset),
                scx.fast.coords(),
                &mut scx.metrics,
            );
            crate::bough::count_segment(true);
            return;
        }
    }

    #[cfg(feature = "std")]
    crate::bough::count_segment(false);
    // TODO: How do we want to handle errors like this?
    let font_ref =
        harfrust::FontRef::from_index(font.font.blob.as_ref(), font.font.index).unwrap();

    // Create harfrust shaper
    let shaper_data = scx.shape_data_cache.entry(
        cache::ShapeDataKey::new(font.font.blob.id(), font.font.index),
        || harfrust::ShaperData::new(&font_ref),
    );
    let instance = scx.shape_instance_cache.entry(
        cache::ShapeInstanceKey::new(
            font.font.blob.id(),
            font.font.index,
            &font.font.synthesis,
            rcx.variations(item.variations),
        ),
        || {
            harfrust::ShaperInstance::from_variations(
                &font_ref,
                variations_iter(&font.font.synthesis, rcx.variations(item.variations)),
            )
        },
    );

    let direction = if item.level & 1 != 0 {
        harfrust::Direction::RightToLeft
    } else {
        harfrust::Direction::LeftToRight
    };
    let hb_script = script_to_harfrust(fb_script);
    let language = item
        .locale
        .as_ref()
        .and_then(|lang| lang.language().parse::<harfrust::Language>().ok());
    scx.features.clear();
    for feature in rcx.features(item.features).unwrap_or(&[]) {
        scx.features.push(harfrust::Feature::new(
            harfrust::Tag::new(&feature.tag.to_bytes()),
            feature.value as u32,
            ..,
        ));
    }
    let harf_shaper = shaper_data
        .shaper(&font_ref)
        .instance(Some(instance))
        .build();
    let shaper_plan = scx.shape_plan_cache.entry(
        cache::ShapePlanKey::new(
            font.font.blob.id(),
            font.font.index,
            &font.font.synthesis,
            direction,
            hb_script,
            language.clone(),
            &scx.features,
            rcx.variations(item.variations),
        ),
        || {
            harfrust::ShapePlan::new(
                &harf_shaper,
                direction,
                Some(hb_script),
                language.as_ref(),
                &scx.features,
            )
        },
    );

    // Prepare harfrust buffer
    let mut buffer = mem::take(&mut scx.unicode_buffer).unwrap();
    buffer.clear();

    // Use the entire segment text including newlines
    buffer.reserve(segment_text.len());
    for (i, ch) in segment_text.chars().enumerate() {
        // Ensure that each cluster's index matches the index into `infos`. This is required
        // for efficient cluster lookup within `data.rs`.
        //
        // In other words, instead of using `buffer.push_str`, which iterates `segment_text`
        // with `char_indices`, push each char individually via `.chars` with a cluster index
        // that matches its `infos` counterpart. This allows us to lookup `infos` via cluster
        // index in `data.rs`.
        buffer.add(ch, i as u32);
    }

    buffer.set_direction(direction);

    buffer.set_script(hb_script);

    if let Some(lang) = language {
        buffer.set_language(lang);
    }

    let glyph_buffer = harf_shaper.shape(
        buffer,
        ShapeOptions::new()
            .plan(Some(shaper_plan))
            .features(&scx.features)
            .point_size(Some(item.size)),
    );

    // Extract relevant CharInfo slice for this segment
    let char_start = char_range.start + item_text[..segment_start_offset].chars().count();
    let segment_char_start = char_start - char_range.start;
    let segment_char_count = segment_text.chars().count();
    let segment_infos =
        &item_infos[segment_char_start..(segment_char_start + segment_char_count)];

    // Push harfrust-shaped run for the entire segment
    layout.data.push_run(
        FontData::new(font.font.blob.clone(), font.font.index),
        item.size,
        font.attrs,
        font.font.synthesis,
        &glyph_buffer,
        item.level,
        item.style_index,
        item.word_spacing,
        item.letter_spacing,
        segment_text,
        segment_infos,
        (text_range.start + segment_start_offset)..(text_range.start + segment_end_offset),
        harf_shaper.coords(),
        &mut scx.metrics,
    );

    // Replace buffer to reuse allocation in next iteration.
    scx.unicode_buffer = Some(glyph_buffer.clear());
}

/// bough grafts 1-2: an item bough's table can model (left to right, Latin, default features, no language).
#[cfg(feature = "std")]
#[inline]
fn fast_eligible(rcx: &ResolveContext, item: &Item) -> bool {
    item.level & 1 == 0
        && item.locale.is_none()
        && item.script == Script::Latin
        && rcx.features(item.features).is_none_or(|f| f.is_empty())
}

/// bough graft 2: the item's font segments without the per-cluster selection loop. For each run of one style
/// index, the font is selected for its first character only; when that font was the query's first candidate
/// (complete) and its table covers every character of the run, every cluster of the run would select it too
/// (a covered single-character cluster maps completely in any font that has the character, and nothing precedes
/// the first candidate). Adjacent runs on equal fonts merge into one segment, as upstream's loop merges them.
/// `false` (nothing pushed) sends the item to upstream's loop.
#[cfg(feature = "std")]
#[allow(clippy::too_many_arguments)]
fn fast_item<'a, B: Brush>(
    font_selector: &mut FontSelector<'a, '_, B>,
    rcx: &'a ResolveContext,
    item: &Item,
    scx: &mut ShapeContext,
    item_text: &str,
    item_infos: &[(CharInfo, u16)],
    text_range: &core::ops::Range<usize>,
    char_range: &core::ops::Range<usize>,
    fb_script: fontique::Script,
    layout: &mut Layout<B>,
    analysis_data_sources: &AnalysisDataSources,
) -> bool {
    if !crate::bough::fast::maybe_covered(item_text.as_bytes()) {
        return false;
    }
    let mut runs = mem::take(&mut scx.fast_runs);
    runs.clear();
    let mut char_cluster = mem::take(&mut scx.char_cluster);
    let mut ok = true;
    let mut iter = item_text.char_indices().zip(item_infos.iter()).peekable();
    while let Some(((start, ch), (_, style_index))) = iter.next() {
        let mut end = start + ch.len_utf8();
        while let Some(&((b, c), (_, si))) = iter.peek() {
            if si != style_index {
                break;
            }
            end = b + c.len_utf8();
            iter.next();
        }
        // The run's first character as its own cluster (a covered character is its own grapheme).
        let first = &item_text[start..start + ch.len_utf8()];
        let first_char = char_range.start + item_text[..start].chars().count();
        let mut infos = item_infos[first_char - char_range.start..].iter();
        let mut offset = text_range.start + start;
        fill_cluster_in_place(first, &mut infos, &mut offset, &mut char_cluster);
        let Some(font) = font_selector.select_font(&mut char_cluster, analysis_data_sources) else {
            ok = false;
            break;
        };
        if !font_selector.first_complete {
            ok = false;
            break;
        }
        scx.fast_vars.clear();
        scx.fast_vars.extend(variations_iter(&font.font.synthesis, rcx.variations(item.variations)));
        if !scx.fast.covers(font.font.blob.id(), font.font.index, font.font.blob.as_ref(), &scx.fast_vars, &item_text[start..end]) {
            ok = false;
            break;
        }
        match runs.last_mut() {
            Some(last) if last.2 == font => last.1 = end,
            _ => runs.push((start, end, font)),
        }
    }
    scx.char_cluster = char_cluster;
    if ok {
        for (start, end, font) in &runs {
            shape_segment(rcx, item, scx, font, item_text, item_infos, text_range, char_range, *start, *end, fb_script, layout);
        }
    }
    runs.clear();
    scx.fast_runs = runs;
    ok
}

/// bough graft 5: the fused build for *simple* text (letters, digits and spaces by graft 3's classes) in one item
/// (every style run agrees on size, spacing, variations; no features, no locale; word-break normal) whose style
/// runs' fonts are their query's first complete candidate with a table covering the run. Upstream would run text
/// analysis (pair rules here), style-index fill, item and font-segment loops, harfrust and `process_clusters`;
/// this writes the same layout data in one pass per font segment from bough's table output (`push_slot_run`).
/// All or nothing: `false` (nothing pushed) means the caller runs the full pipeline.
#[cfg(feature = "std")]
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_simple<'a, B: Brush>(
    rcx: &'a ResolveContext,
    mut fq: Query<'a>,
    styles: &'a [ResolvedStyle<B>],
    style_runs: &[crate::resolve::StyleRun],
    scx: &mut ShapeContext,
    text: &str,
    layout: &mut Layout<B>,
    analysis_data_sources: &AnalysisDataSources,
) -> bool {
    use crate::WordBreak;
    let classes = crate::analysis::simple_classes(analysis_data_sources);
    let b = text.as_bytes();
    if b.is_empty() {
        return false;
    }
    let mut k = 0;
    while k < b.len() {
        let (cp, n) = match b[k] {
            c if c < 0x80 => (c, 1),
            0xC2 if k + 1 < b.len() => (b[k + 1], 2),
            0xC3 if k + 1 < b.len() => (b[k + 1] + 0x40, 2),
            _ => return false,
        };
        if classes[cp as usize] == 0 {
            return false;
        }
        k += n;
    }
    // One item: every style run agrees with the first on what breaks an item.
    let first_index = style_runs[0].style_index;
    let first = &styles[first_index as usize];
    if first.locale.is_some() || !rcx.features(first.font_features).is_none_or(|f| f.is_empty()) {
        return false;
    }
    for r in style_runs {
        let st = &styles[r.style_index as usize];
        if !nearly_eq(st.font_size, first.font_size)
            || st.locale != first.locale
            || st.font_variations != first.font_variations
            || st.font_features != first.font_features
            || !nearly_eq(st.letter_spacing, first.letter_spacing)
            || !nearly_eq(st.word_spacing, first.word_spacing)
            || st.word_break != WordBreak::Normal
        {
            return false;
        }
    }
    let item = Item {
        style_index: first_index,
        size: first.font_size,
        script: Script::Latin,
        level: 0,
        locale: None,
        variations: first.font_variations,
        features: first.font_features,
        word_spacing: first.word_spacing,
        letter_spacing: first.letter_spacing,
    };
    let fb_script = convert::script_to_fontique(item.script, analysis_data_sources);
    let mut font_selector = FontSelector::new(&mut fq, rcx, styles, first_index, fb_script, None);
    let mut runs = mem::take(&mut scx.fast_runs);
    runs.clear();
    let mut char_cluster = mem::take(&mut scx.char_cluster);
    let mut ok = true;
    let mut infos_one = [(CharInfo::placeholder(), 0_u16)];
    for r in style_runs {
        if r.range.is_empty() {
            continue;
        }
        // The run's first character as its own cluster (a simple character is its own grapheme).
        let ch = text[r.range.start..].chars().next().unwrap();
        infos_one[0].1 = r.style_index;
        let mut it = infos_one.iter();
        let mut offset = r.range.start;
        fill_cluster_in_place(&text[r.range.start..r.range.start + ch.len_utf8()], &mut it, &mut offset, &mut char_cluster);
        let Some(font) = font_selector.select_font(&mut char_cluster, analysis_data_sources) else {
            ok = false;
            break;
        };
        if !font_selector.first_complete {
            ok = false;
            break;
        }
        scx.fast_vars.clear();
        scx.fast_vars.extend(variations_iter(&font.font.synthesis, rcx.variations(item.variations)));
        if !scx.fast.covers(font.font.blob.id(), font.font.index, font.font.blob.as_ref(), &scx.fast_vars, &text[r.range.clone()]) {
            ok = false;
            break;
        }
        match runs.last_mut() {
            Some(last) if last.2 == font => last.1 = r.range.end,
            _ => runs.push((r.range.start, r.range.end, font)),
        }
    }
    scx.char_cluster = char_cluster;
    // Shape every segment before pushing anything (a refusal leaves the layout untouched).
    if ok {
        for (k, (start, end, font)) in runs.iter().enumerate() {
            scx.fast_vars.clear();
            scx.fast_vars.extend(variations_iter(&font.font.synthesis, rcx.variations(item.variations)));
            if !scx.fast.shape_seg(k, font.font.blob.id(), font.font.index, font.font.blob.as_ref(), &scx.fast_vars, &text[*start..*end], item.size) {
                ok = false;
                break;
            }
        }
    }
    if ok {
        let mut bounds = mem::take(&mut scx.bounds);
        let mut props = mem::take(&mut scx.props);
        crate::bough::breaks::boundaries(text.as_bytes(), &mut bounds, &mut props);
        let mut ordinal = 0usize;
        let mut sr = 0usize;
        let mut style_of = |byte: usize| {
            while style_runs[sr].range.end <= byte {
                sr += 1;
            }
            style_runs[sr].style_index
        };
        for (k, (start, end, font)) in runs.iter().enumerate() {
            let (slots, coords) = scx.fast.seg(k);
            layout.data.push_slot_run(
                FontData::new(font.font.blob.clone(), font.font.index),
                item.size,
                font.attrs,
                font.font.synthesis,
                slots,
                item.style_index,
                item.word_spacing,
                item.letter_spacing,
                &text[*start..*end],
                *start..*end,
                coords,
                &mut scx.metrics,
                &bounds,
                &mut ordinal,
                &mut style_of,
            );
            crate::bough::count_segment(true);
        }
        scx.bounds = bounds;
        scx.props = props;
        if style_runs.len() == 1 && runs.len() == 1 {
            let font = &runs[0].2;
            scx.fast_vars.clear();
            scx.fast_vars.extend(variations_iter(&font.font.synthesis, rcx.variations(item.variations)));
            layout.data.bough_fast = Some(crate::layout::data::FastKey {
                font: FontData::new(font.font.blob.clone(), font.font.index),
                attrs: font.attrs,
                synthesis: font.font.synthesis,
                size: item.size,
                style_index: item.style_index,
                word_spacing: item.word_spacing,
                letter_spacing: item.letter_spacing,
                vars: scx.fast_vars.clone(),
            });
        }
    }
    runs.clear();
    scx.fast_runs = runs;
    ok
}

/// bough graft 6: lay out a new text in a layout the fused path built from one style on one font, without the
/// builder: no style resolution, no font query. That font was the query's first candidate, so for a new covered
/// text every cluster selects it again (graft 2's argument), and the style table is unchanged (the caller's
/// contract: only the text changed). `false` (layout untouched) means the caller rebuilds.
#[cfg(feature = "std")]
pub(crate) fn retext_simple<B: Brush>(
    layout: &mut Layout<B>,
    scx: &mut ShapeContext,
    analysis_data_sources: &AnalysisDataSources,
    text: &str,
) -> bool {
    let Some(key) = layout.data.bough_fast.take() else { return false };
    let ok = (|| {
        if text.is_empty() || !layout.data.inline_boxes.is_empty() {
            return false;
        }
        let classes = crate::analysis::simple_classes(analysis_data_sources);
        let b = text.as_bytes();
        let mut k = 0;
        while k < b.len() {
            let (cp, n) = match b[k] {
                c if c < 0x80 => (c, 1),
                0xC2 if k + 1 < b.len() => (b[k + 1], 2),
                0xC3 if k + 1 < b.len() => (b[k + 1] + 0x40, 2),
                _ => return false,
            };
            if classes[cp as usize] == 0 {
                return false;
            }
            k += n;
        }
        let (id, index, data) = (key.font.data.id(), key.font.index, key.font.data.clone());
        if !scx.fast.covers(id, index, data.as_ref(), &key.vars, text)
            || !scx.fast.shape_seg(0, id, index, data.as_ref(), &key.vars, text, key.size)
        {
            return false;
        }
        let d = &mut layout.data;
        d.text_len = text.len();
        d.width = 0.;
        d.full_width = 0.;
        d.height = 0.;
        d.coords.clear();
        d.runs.clear();
        d.items.clear();
        d.clusters.clear();
        d.glyphs.clear();
        d.lines.clear();
        d.line_items.clear();
        let mut bounds = mem::take(&mut scx.bounds);
        let mut props = mem::take(&mut scx.props);
        crate::bough::breaks::boundaries(b, &mut bounds, &mut props);
        let (slots, coords) = scx.fast.seg(0);
        let mut ordinal = 0usize;
        let style = key.style_index;
        d.push_slot_run(
            key.font.clone(),
            key.size,
            key.attrs,
            key.synthesis,
            slots,
            key.style_index,
            key.word_spacing,
            key.letter_spacing,
            text,
            0..text.len(),
            coords,
            &mut scx.metrics,
            &bounds,
            &mut ordinal,
            &mut |_| style,
        );
        scx.bounds = bounds;
        scx.props = props;
        d.finish();
        crate::bough::count_segment(true);
        true
    })();
    layout.data.bough_fast = Some(key);
    ok
}

fn real_script(script: Script) -> bool {
    script != Script::Common && script != Script::Unknown && script != Script::Inherited
}

fn variations_iter<'a>(
    synthesis: &'a fontique::Synthesis,
    item: Option<&'a [FontVariation]>,
) -> impl Iterator<Item = harfrust::Variation> + 'a {
    synthesis
        .variation_settings()
        .iter()
        .map(|(tag, value)| harfrust::Variation {
            tag: *tag,
            value: *value,
        })
        .chain(
            item.unwrap_or(&[])
                .iter()
                .map(|variation| harfrust::Variation {
                    tag: harfrust::Tag::new(&variation.tag.to_bytes()),
                    value: variation.value,
                }),
        )
}

struct FontSelector<'a, 'b, B: Brush> {
    query: &'b mut Query<'a>,
    fonts_id: Option<usize>,
    rcx: &'a ResolveContext,
    styles: &'a [ResolvedStyle<B>],
    style_index: u16,
    attrs: fontique::Attributes,
    variations: &'a [FontVariation],
    features: &'a [FontFeature],
    /// bough graft 2: whether the last `select_font` chose the first candidate the query offered, with the
    /// cluster completely mapped (then nothing earlier in the query order can be chosen for a covered cluster).
    first_complete: bool,
}

impl<'a, 'b, B: Brush> FontSelector<'a, 'b, B> {
    fn new(
        query: &'b mut Query<'a>,
        rcx: &'a ResolveContext,
        styles: &'a [ResolvedStyle<B>],
        style_index: u16,
        fb_script: fontique::Script,
        locale: Option<Language>,
    ) -> Self {
        let style = &styles[style_index as usize];
        let fonts_id = style.font_family.id();
        let fonts = rcx.stack(style.font_family).unwrap_or(&[]);
        let attrs = fontique::Attributes {
            width: style.font_width,
            weight: style.font_weight,
            style: style.font_style,
        };
        let variations = rcx.variations(style.font_variations).unwrap_or(&[]);
        let features = rcx.features(style.font_features).unwrap_or(&[]);
        query.set_families(fonts.iter().copied());

        query.set_fallbacks(fontique::FallbackKey::new(fb_script, locale.as_ref()));
        query.set_attributes(attrs);

        Self {
            query,
            fonts_id: Some(fonts_id),
            rcx,
            styles,
            style_index,
            attrs,
            variations,
            features,
            first_complete: false,
        }
    }

    fn select_font(
        &mut self,
        cluster: &mut CharCluster,
        analysis_data_sources: &AnalysisDataSources,
    ) -> Option<SelectedFont> {
        let style_index = cluster.style_index();
        let is_emoji = cluster.is_emoji;
        if style_index != self.style_index || is_emoji || self.fonts_id.is_none() {
            self.style_index = style_index;
            let style = &self.styles[style_index as usize];

            let fonts_id = style.font_family.id();
            let fonts = self.rcx.stack(style.font_family).unwrap_or(&[]);
            let fonts = fonts.iter().copied().map(QueryFamily::Id);
            if is_emoji {
                use core::iter::once;
                let emoji_family = QueryFamily::Generic(fontique::GenericFamily::Emoji);
                self.query.set_families(fonts.chain(once(emoji_family)));
                self.fonts_id = None;
            } else if self.fonts_id != Some(fonts_id) {
                self.query.set_families(fonts);
                self.fonts_id = Some(fonts_id);
            }

            let attrs = fontique::Attributes {
                width: style.font_width,
                weight: style.font_weight,
                style: style.font_style,
            };
            if self.attrs != attrs {
                self.query.set_attributes(attrs);
                self.attrs = attrs;
            }
            self.variations = self.rcx.variations(style.font_variations).unwrap_or(&[]);
            self.features = self.rcx.features(style.font_features).unwrap_or(&[]);
        }
        let mut selected_font = None;
        let (mut first, mut first_complete) = (true, false);
        self.query.matches_with(|font| {
            let is_first = mem::replace(&mut first, false);
            let Some(charmap) = font.charmap() else {
                return fontique::QueryStatus::Continue;
            };

            let map_status = cluster.map(
                |ch| {
                    charmap
                        .map(ch)
                        .map(|g| {
                            // HACK: in reality, we're only computing coverage, so
                            // we only care about whether the font  has a mapping
                            // for a particular glyph. Any non-zero value indicates
                            // the existence of a glyph so we can simplify this
                            // without a fallible conversion from u32 to u16.
                            (g != 0) as u16
                        })
                        .unwrap_or_default()
                },
                analysis_data_sources,
            );

            match map_status {
                Status::Complete => {
                    first_complete = is_first;
                    selected_font = Some(SelectedFont {
                        font: font.clone(),
                        attrs: self.attrs,
                    });
                    fontique::QueryStatus::Stop
                }
                Status::Keep => {
                    selected_font = Some(SelectedFont {
                        font: font.clone(),
                        attrs: self.attrs,
                    });
                    fontique::QueryStatus::Continue
                }
                Status::Discard => {
                    if selected_font.is_none() {
                        selected_font = Some(SelectedFont {
                            font: font.clone(),
                            attrs: self.attrs,
                        });
                    }
                    fontique::QueryStatus::Continue
                }
            }
        });
        self.first_complete = first_complete;
        selected_font
    }
}

struct SelectedFont {
    font: QueryFont,
    attrs: fontique::Attributes,
}

impl PartialEq for SelectedFont {
    fn eq(&self, other: &Self) -> bool {
        self.font.family == other.font.family && self.font.synthesis == other.font.synthesis
    }
}
