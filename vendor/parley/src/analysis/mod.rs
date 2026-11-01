// Copyright 2025 the Parley Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

pub(crate) mod cluster;

use alloc::vec::Vec;
use core::marker::PhantomData;

use crate::break_overrides::{LineBreakContext, LineBreakOverrideFn};
use crate::resolve::StyleRun;
use crate::{Brush, LayoutContext, WordBreak};

use icu_normalizer::properties::{
    CanonicalComposition, CanonicalCompositionBorrowed, CanonicalDecomposition,
    CanonicalDecompositionBorrowed,
};
use icu_properties::props::{BidiMirroringGlyph, GeneralCategory, GraphemeClusterBreak, Script};
use icu_properties::{
    CodePointMapData, CodePointMapDataBorrowed, PropertyNamesShort, PropertyNamesShortBorrowed,
};
use icu_segmenter::options::{LineBreakOptions, LineBreakWordOption, WordBreakInvariantOptions};
use icu_segmenter::{
    GraphemeClusterSegmenter, GraphemeClusterSegmenterBorrowed, LineSegmenter,
    LineSegmenterBorrowed, WordSegmenter, WordSegmenterBorrowed,
};
use parley_data::Properties;

pub(crate) struct AnalysisDataSources;

impl AnalysisDataSources {
    pub(crate) fn new() -> Self {
        Self
    }

    #[inline(always)]
    pub(crate) fn properties(&self, c: char) -> Properties {
        Properties::get(c)
    }

    #[inline(always)]
    pub(crate) fn grapheme_segmenter(&self) -> GraphemeClusterSegmenterBorrowed<'_> {
        const { GraphemeClusterSegmenter::new() }
    }

    #[inline(always)]
    fn word_segmenter(&self) -> WordSegmenterBorrowed<'static> {
        #[cfg(feature = "complex-scripts")]
        {
            WordSegmenter::new_dictionary(WordBreakInvariantOptions::default())
        }
        #[cfg(not(feature = "complex-scripts"))]
        {
            const { WordSegmenter::new_for_non_complex_scripts(WordBreakInvariantOptions::default()) }
        }
    }

    #[inline(always)]
    fn line_segmenter(&self, word_break_strength: WordBreak) -> LineSegmenterBorrowed<'static> {
        match word_break_strength {
            WordBreak::Normal => {
                let mut opt = LineBreakOptions::default();
                opt.word_option = Some(LineBreakWordOption::Normal);
                line_segmenter_impl(opt)
            }
            WordBreak::BreakAll => {
                let mut opt = LineBreakOptions::default();
                opt.word_option = Some(LineBreakWordOption::BreakAll);
                line_segmenter_impl(opt)
            }
            WordBreak::KeepAll => {
                let mut opt = LineBreakOptions::default();
                opt.word_option = Some(LineBreakWordOption::KeepAll);
                line_segmenter_impl(opt)
            }
        }
    }

    #[inline(always)]
    fn composing_normalizer(&self) -> CanonicalCompositionBorrowed<'_> {
        const { CanonicalComposition::new() }
    }

    #[inline(always)]
    fn decomposing_normalizer(&self) -> CanonicalDecompositionBorrowed<'_> {
        const { CanonicalDecomposition::new() }
    }

    #[inline(always)]
    pub(crate) fn script_short_name(&self) -> PropertyNamesShortBorrowed<'static, Script> {
        PropertyNamesShort::new()
    }

    #[inline(always)]
    fn brackets(&self) -> CodePointMapDataBorrowed<'_, BidiMirroringGlyph> {
        const { CodePointMapData::new() }
    }
}

#[cfg(feature = "complex-scripts")]
#[inline(always)]
fn line_segmenter_impl(opt: LineBreakOptions<'_>) -> LineSegmenterBorrowed<'static> {
    LineSegmenter::new_dictionary(opt)
}

#[cfg(not(feature = "complex-scripts"))]
#[inline(always)]
fn line_segmenter_impl(opt: LineBreakOptions<'_>) -> LineSegmenterBorrowed<'static> {
    LineSegmenter::new_for_non_complex_scripts(opt)
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct CharInfo {
    /// The line/word breaking boundary classification of this character.
    pub boundary: Boundary,
    /// The Unicode script this character belongs to.
    pub script: Script,
    /// The grapheme cluster boundary property of this character.
    pub grapheme_cluster_break: GraphemeClusterBreak,
    /// The impact this character has on directionality.
    pub bidi_class: icu_properties::props::BidiClass,
    /// Whether or not the character is a bracket, plus mirror data if so.
    pub bracket: BidiMirroringGlyph,

    flags: u8,
}

impl CharInfo {
    /// bough graft 5: an info for a cluster built without analysis (only its style index is read there).
    #[cfg(feature = "std")]
    pub(crate) fn placeholder() -> Self {
        simple_table(&AnalysisDataSources).info[b'a' as usize]
    }

    const VARIATION_SELECTOR_SHIFT: u8 = 0;
    const REGION_INDICATOR_SHIFT: u8 = 1;
    const CONTROL_SHIFT: u8 = 2;
    const EMOJI_OR_PICTOGRAPH_SHIFT: u8 = 3;
    const CONTRIBUTES_TO_SHAPING_SHIFT: u8 = 4;
    const FORCE_NORMALIZE_SHIFT: u8 = 5;

    #[allow(
        dead_code,
        reason = "To be used in more complete emoji checking, in select_font"
    )]
    const VARIATION_SELECTOR_MASK: u8 = 1 << Self::VARIATION_SELECTOR_SHIFT;
    #[allow(
        dead_code,
        reason = "To be used in more complete emoji checking, in select_font"
    )]
    const REGION_INDICATOR_MASK: u8 = 1 << Self::REGION_INDICATOR_SHIFT;
    const CONTROL_MASK: u8 = 1 << Self::CONTROL_SHIFT;
    const EMOJI_OR_PICTOGRAPH_MASK: u8 = 1 << Self::EMOJI_OR_PICTOGRAPH_SHIFT;
    const CONTRIBUTES_TO_SHAPING_MASK: u8 = 1 << Self::CONTRIBUTES_TO_SHAPING_SHIFT;
    const FORCE_NORMALIZE_MASK: u8 = 1 << Self::FORCE_NORMALIZE_SHIFT;

    fn new(
        boundary: Boundary,
        script: Script,
        grapheme_cluster_break: GraphemeClusterBreak,
        bidi_class: icu_properties::props::BidiClass,
        bracket: BidiMirroringGlyph,
        is_variation_selector: bool,
        is_region_indicator: bool,
        is_control: bool,
        is_emoji_or_pictograph: bool,
        contributes_to_shaping: bool,
        force_normalize: bool,
    ) -> Self {
        Self {
            boundary,
            script,
            grapheme_cluster_break,
            bidi_class,
            bracket,
            flags: (is_variation_selector as u8) << Self::VARIATION_SELECTOR_SHIFT
                | (is_region_indicator as u8) << Self::REGION_INDICATOR_SHIFT
                | (is_control as u8) << Self::CONTROL_SHIFT
                | (is_emoji_or_pictograph as u8) << Self::EMOJI_OR_PICTOGRAPH_SHIFT
                | (contributes_to_shaping as u8) << Self::CONTRIBUTES_TO_SHAPING_SHIFT
                | (force_normalize as u8) << Self::FORCE_NORMALIZE_SHIFT,
        }
    }

    #[allow(
        dead_code,
        reason = "To be used in more complete emoji checking, in select_font"
    )]
    #[inline(always)]
    pub(crate) fn is_variation_selector(self) -> bool {
        self.flags & Self::VARIATION_SELECTOR_MASK != 0
    }

    #[allow(
        dead_code,
        reason = "To be used in more complete emoji checking, in select_font"
    )]
    #[inline(always)]
    pub(crate) fn is_region_indicator(self) -> bool {
        self.flags & Self::REGION_INDICATOR_MASK != 0
    }

    #[inline(always)]
    pub(crate) fn is_control(self) -> bool {
        self.flags & Self::CONTROL_MASK != 0
    }

    #[inline(always)]
    pub(crate) fn is_emoji_or_pictograph(self) -> bool {
        self.flags & Self::EMOJI_OR_PICTOGRAPH_MASK != 0
    }

    #[inline(always)]
    pub(crate) fn contributes_to_shaping(self) -> bool {
        self.flags & Self::CONTRIBUTES_TO_SHAPING_MASK != 0
    }

    #[inline(always)]
    pub(crate) fn force_normalize(self) -> bool {
        self.flags & Self::FORCE_NORMALIZE_MASK != 0
    }
}

/// Boundary type of a character or cluster.
#[derive(Copy, Clone, PartialOrd, Ord, PartialEq, Eq, Debug)]
#[repr(u8)]
pub(crate) enum Boundary {
    /// Not a boundary.
    None = 0,
    /// Start of a word.
    Word = 1,
    /// Potential line break.
    Line = 2,
    /// Mandatory line break.
    Mandatory = 3,
}

pub(crate) fn analyze_text<B: Brush>(
    lcx: &mut LayoutContext<B>,
    mut text: &str,
    line_break_override: Option<&LineBreakOverrideFn>,
) {
    struct WordBreakSegmentIter<'a, I: Iterator, B: Brush> {
        text: &'a str,
        style_runs: I,
        lcx: &'a LayoutContext<B>,
        char_indices: core::str::CharIndices<'a>,
        current_char: (usize, char),
        building_range_start: usize,
        previous_word_break_style: WordBreak,
        done: bool,
        _phantom: PhantomData<B>,
    }

    impl<'a, I, B: Brush + 'a> WordBreakSegmentIter<'a, I, B>
    where
        I: Iterator<Item = &'a StyleRun>,
    {
        fn new(
            text: &'a str,
            style_runs: I,
            lcx: &'a LayoutContext<B>,
            first_style_run: &StyleRun,
        ) -> Self {
            let mut char_indices = text.char_indices();
            let current_char_len = char_indices.next().unwrap();
            let first_style = &lcx.style_table[first_style_run.style_index as usize];

            Self {
                text,
                style_runs,
                lcx,
                char_indices,
                current_char: current_char_len,
                building_range_start: first_style_run.range.start,
                previous_word_break_style: first_style.word_break,
                done: false,
                _phantom: PhantomData,
            }
        }
    }

    impl<'a, I, B: Brush + 'a> Iterator for WordBreakSegmentIter<'a, I, B>
    where
        I: Iterator<Item = &'a StyleRun>,
    {
        type Item = (&'a str, WordBreak, bool);

        fn next(&mut self) -> Option<Self::Item> {
            if self.done {
                return None;
            }

            for style_run in self.style_runs.by_ref() {
                // Empty style ranges are disallowed.
                assert!(style_run.range.start < style_run.range.end);

                let style_start_index = style_run.range.start;
                let mut prev_char_index = self.current_char;

                // Find the character at the style boundary
                while self.current_char.0 < style_start_index {
                    prev_char_index = self.current_char;
                    self.current_char = self.char_indices.next().unwrap();
                }

                let current_word_break_style =
                    self.lcx.style_table[style_run.style_index as usize].word_break;
                if self.previous_word_break_style == current_word_break_style {
                    continue;
                }

                // Produce one substring for each different word break style run
                let prev_size = prev_char_index.1.len_utf8();
                let size = self.current_char.1.len_utf8();

                let substring = &self.text[self.building_range_start..style_start_index + size];
                let result_style = self.previous_word_break_style;

                self.building_range_start = style_start_index - prev_size;
                self.previous_word_break_style = current_word_break_style;

                return Some((substring, result_style, false));
            }

            // Final segment
            self.done = true;
            let last_substring = &self.text[self.building_range_start..self.text.len()];
            Some((last_substring, self.previous_word_break_style, true))
        }
    }

    if text.is_empty() {
        text = " ";
    }

    // bough graft 3: text of letters, digits and spaces only (by ICU's own LineBreak / WordBreak properties)
    // needs no segmenter: its line and word boundaries are pair rules (see `analyze_simple`).
    #[cfg(feature = "std")]
    if line_break_override.is_none() && analyze_simple(lcx, text) {
        crate::bough::count_analysis(true);
        return;
    }
    #[cfg(feature = "std")]
    crate::bough::count_analysis(false);

    // Line boundaries (word break naming refers to the line boundary determination config).
    //
    // This breaks text into sequences with similar line boundary config (part of style
    // information). If this config is consistent for all text, we use a fast path through this.
    let (first_style_run, rest_runs) = lcx
        .style_runs
        .split_first()
        .expect("analyze_text requires at least one style run");

    let contiguous_word_break_substrings =
        WordBreakSegmentIter::new(text, rest_runs.iter(), lcx, first_style_run);
    let mut global_offset = 0;
    let mut line_boundary_positions: Vec<usize> = Vec::new();
    for (substring_index, (substring, word_break_strength, last)) in
        contiguous_word_break_substrings.enumerate()
    {
        // Fast path for text with a single word-break option.
        if substring_index == 0 && last {
            let mut lb_iter = lcx
                .analysis_data_sources
                .line_segmenter(word_break_strength)
                .segment_str(substring);

            let _first = lb_iter.next();
            let second = lb_iter.next();
            if second.is_none() {
                continue;
            }
            let third = lb_iter.next();
            if third.is_none() {
                continue;
            }

            let iter = [second.unwrap(), third.unwrap()].into_iter().chain(lb_iter);

            line_boundary_positions.extend(iter);
            // Remove the unnecessary boundary at the end added by ICU4X.
            line_boundary_positions.pop();
            break;
        }

        let line_boundaries_iter = lcx
            .analysis_data_sources
            .line_segmenter(word_break_strength)
            .segment_str(substring);

        let mut substring_chars = substring.chars();
        if substring_index != 0 {
            global_offset -= substring_chars.next().unwrap().len_utf8();
        }
        // There will always be at least two characters if we are not taking the fast path for
        // a single word break style substring.
        let last_len = substring_chars.next_back().unwrap().len_utf8();

        // Mark line boundaries (overriding word boundaries where present).
        for (index, pos) in line_boundaries_iter.enumerate() {
            // icu adds leading and trailing line boundaries, which we don't use.
            if index == 0 || pos == substring.len() {
                continue;
            }

            // For all but the last substring, we ignore line boundaries caused by the last
            // character, as this character is carried back from the next substring, and will be
            // accounted for there.
            if !last && pos == substring.len() - last_len {
                continue;
            }
            line_boundary_positions.push(pos + global_offset);
        }

        if !last {
            global_offset += substring.len() - last_len;
        }
    }

    // Collect boundary byte positions compactly
    let mut wb_iter = lcx
        .analysis_data_sources
        .word_segmenter()
        .segment_str(text)
        .peekable();

    // Merge boundaries - line takes precedence over word
    let mut lb_iter = line_boundary_positions.iter().peekable();
    let mut prev_char = None;
    let mut prev_prev_char = None;
    let boundary_iter = text.char_indices().map(|(byte_pos, ch)| {
        // advance any stale word boundary positions
        while let Some(&w) = wb_iter.peek() {
            if w < byte_pos {
                _ = wb_iter.next();
            } else {
                break;
            }
        }
        // advance any stale line boundary positions
        while let Some(&l) = lb_iter.peek() {
            if *l < byte_pos {
                _ = lb_iter.next();
            } else {
                break;
            }
        }

        let mut is_word = false;
        if let Some(&w) = wb_iter.peek() {
            if w == byte_pos {
                is_word = true;
                _ = wb_iter.next();
            }
        }
        let mut is_line = false;
        if let Some(&l) = lb_iter.peek() {
            if *l == byte_pos {
                is_line = true;
                _ = lb_iter.next();
            }
        }

        // This leaves word boundaries intact. Consumers can only impact line boundaries.
        if let (Some(prev), Some(lb_override)) = (prev_char, line_break_override) {
            let forced = lb_override(LineBreakContext {
                before_before: prev_prev_char,
                before: prev,
                after: ch,
            });
            if let Some(forced) = forced {
                is_line = forced;
            }
        }
        prev_prev_char = prev_char;
        prev_char = Some(ch);

        let boundary = if is_line {
            Boundary::Line
        } else if is_word {
            Boundary::Word
        } else {
            Boundary::None
        };

        (boundary, ch)
    });

    let mut needs_bidi_resolution = false;

    lcx.info.reserve(text.len());
    boundary_iter
        // Shift line break data forward one, as line boundaries corresponding with line-breaking
        // characters (like '\n') exist at an index position one higher than the respective
        // character's index, but we need our iterators to align, and the rest are simply
        // character-indexed.
        .fold(false, |is_mandatory_linebreak, (boundary, ch)| {
            let boundary = if is_mandatory_linebreak {
                Boundary::Mandatory
            } else {
                boundary
            };
            let (info, needs_bidi, next_mandatory_linebreak) =
                char_info(&lcx.analysis_data_sources, ch, boundary);
            needs_bidi_resolution |= needs_bidi;
            lcx.info.push((info, 0)); // Style index is populated later
            next_mandatory_linebreak
        });

    if needs_bidi_resolution {
        lcx.bidi.resolve(
            text.chars().zip(
                lcx.info
                    .iter()
                    .map(|info| (info.0.bidi_class, info.0.bracket)),
            ),
            None,
        );
    }
}

/// One character's analysis: upstream's per-character body (properties, force-normalize, bracket, flags), with its
/// boundary given. Returns the info, whether the character needs bidi resolution, and whether it is a mandatory
/// line break (which makes the next character's boundary `Mandatory`).
#[inline(always)]
fn char_info(sources: &AnalysisDataSources, ch: char, boundary: Boundary) -> (CharInfo, bool, bool) {
    let properties = sources.properties(ch);
    let script = properties.script();
    let grapheme_cluster_break = properties.grapheme_cluster_break();
    let bidi_class = properties.bidi_class();
    let general_category = properties.general_category();
    let is_emoji_or_pictograph = properties.is_emoji_or_pictograph();
    let is_variation_selector = properties.is_variation_selector();
    let is_region_indicator = properties.is_region_indicator();
    let next_mandatory_linebreak = properties.is_mandatory_linebreak();

    let force_normalize = {
        // "Extend" break chars should be normalized first, with two exceptions
        if matches!(grapheme_cluster_break, GraphemeClusterBreak::Extend) &&
            ch as u32 != 0x200C && // Is not a Zero Width Non-Joiner &&
            !is_variation_selector
        {
            true
        } else {
            // All spacing mark break chars should be normalized first.
            matches!(grapheme_cluster_break, GraphemeClusterBreak::SpacingMark)
        }
    };

    let needs_bidi = crate::bidi::needs_bidi_resolution(bidi_class);
    // TODO: maybe extend Properties to u64 to fit BidiMirroringGlyph
    let bracket = sources.brackets().get(ch);

    (
        CharInfo::new(
            boundary,
            script,
            grapheme_cluster_break,
            bidi_class,
            bracket,
            is_variation_selector,
            is_region_indicator,
            general_category == GeneralCategory::Control,
            is_emoji_or_pictograph,
            contributes_to_shaping(general_category, script),
            force_normalize,
        ),
        needs_bidi,
        next_mandatory_linebreak,
    )
}

/// bough graft 3: the simple analysis, for text of *simple* characters: covered Latin-1 (ASCII 0x20..=0x7E,
/// 0xA0..=0xFF without U+00AD) of the Latin or Common script, needing no bidi resolution, not ending a line, not a
/// control. Boundaries come from ICU4X's own rule tables walked over a 256-entry property map
/// (`bough::breaks`, graft 3b); the first version (kept as the argument for letters, digits and spaces) reduced
/// the segmenters to pair rules:
///
/// * line (UAX #14, word-break `normal`): LB7 `× SP`, LB18 `SP ÷`, LB23 `AL × NU`, `NU × AL`, LB25 `NU × NU`,
///   LB28 `AL × AL`; no rule with wider context (LB14-17 need OP/QU/CL/CP/B2, LB25's prefixes PR/PO/OP/HY/IS/SY,
///   LB30 OP/CP) can match. So a line boundary sits exactly where a non-space follows a space.
/// * word (UAX #29): WB3d `WSegSpace × WSegSpace`, WB5/8/9/10 between ALetter and Numeric, WB999 elsewhere; the
///   wider rules (WB6/7/11/12 need MidLetter/MidNum/MidNumLet) cannot match. So a word boundary sits at the
///   start and wherever space-ness changes.
///
/// The per-character info comes from a 256-entry table built by `char_info` itself (exact by construction). The
/// gate `tests/parley_twin.rs` checks every pair of covered characters and corpora against upstream's segmenters.
#[cfg(feature = "std")]
fn analyze_simple<B: Brush>(lcx: &mut LayoutContext<B>, text: &str) -> bool {
    let t = simple_table(&lcx.analysis_data_sources);
    let b = text.as_bytes();
    // Every byte simple (two-byte Latin-1 via its lead byte C2/C3) and one word-break option for the whole text.
    let mut k = 0;
    while k < b.len() {
        let (cp, n) = match b[k] {
            c if c < 0x80 => (c, 1),
            0xC2 if k + 1 < b.len() => (b[k + 1], 2),
            0xC3 if k + 1 < b.len() => (b[k + 1] + 0x40, 2),
            _ => return false,
        };
        if t.class[cp as usize] == 0 {
            return false;
        }
        k += n;
    }
    if lcx
        .style_runs
        .iter()
        .any(|r| lcx.style_table[r.style_index as usize].word_break != WordBreak::Normal)
    {
        return false;
    }
    let mut bounds = core::mem::take(&mut lcx.scx.bounds);
    let mut props = core::mem::take(&mut lcx.scx.props);
    crate::bough::breaks::boundaries(b, &mut bounds, &mut props);
    lcx.info.reserve(bounds.len());
    let mut k = 0;
    for &bd in &bounds {
        let (cp, n) = match b[k] {
            c if c < 0x80 => (c, 1),
            0xC2 => (b[k + 1], 2),
            _ => (b[k + 1] + 0x40, 2),
        };
        let mut info = t.info[cp as usize];
        info.boundary = match bd {
            2 => Boundary::Line,
            1 => Boundary::Word,
            _ => Boundary::None,
        };
        lcx.info.push((info, 0));
        k += n;
    }
    lcx.scx.bounds = bounds;
    lcx.scx.props = props;
    true
}

#[cfg(feature = "std")]
struct SimpleTable {
    /// 0: not simple; 1: letter or digit (AL/NU, ALetter/Numeric); 2: the space.
    class: [u8; 256],
    info: [CharInfo; 256],
}

/// Graft 5: the simple class of each Latin-1 codepoint (0: not simple; 1: letter or digit; 2: the space).
#[cfg(feature = "std")]
pub(crate) fn simple_classes(sources: &AnalysisDataSources) -> &'static [u8; 256] {
    &simple_table(sources).class
}

#[cfg(feature = "std")]
fn simple_table(sources: &AnalysisDataSources) -> &'static SimpleTable {
    use icu_properties::props::{LineBreak, WordBreak as Wb};
    static TABLE: std::sync::OnceLock<SimpleTable> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        let lb = CodePointMapData::<LineBreak>::new();
        let wb = CodePointMapData::<Wb>::new();
        let (placeholder, _, _) = char_info(sources, ' ', Boundary::None);
        let mut t = SimpleTable { class: [0; 256], info: [placeholder; 256] };
        for cp in 0u32..256 {
            let ch = char::from_u32(cp).unwrap();
            let (info, needs_bidi, mandatory) = char_info(sources, ch, Boundary::None);
            t.info[cp as usize] = info;
            let covered = (0x20..0x7F).contains(&cp) || (cp >= 0xA0 && cp != 0xAD);
            let script_ok = matches!(info.script, Script::Latin | Script::Common);
            // Emoji-property characters (digits, `#`, `*`, ©, ®) stay: their info comes from the table, and font
            // selection appends the Emoji family *after* the style's stack, so the first candidate is unchanged.
            if !covered || !script_ok || needs_bidi || mandatory || info.is_control() {
                continue;
            }
            // Every other covered character: its boundaries come from ICU's own tables (`bough::breaks`).
            let _ = (&lb, &wb, ch);
            t.class[cp as usize] = 1;
        }
        t
    })
}

/// All characters contribute to shaping except:
/// - Control characters
/// - Format characters, unless they use the "Inherited" script
#[inline(always)]
pub(crate) fn contributes_to_shaping(general_category: GeneralCategory, script: Script) -> bool {
    if matches!(
        general_category,
        GeneralCategory::Control
            | GeneralCategory::LineSeparator
            | GeneralCategory::ParagraphSeparator
    ) {
        return false;
    }

    !(general_category == GeneralCategory::Format && script != Script::Inherited)
}
