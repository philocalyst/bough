//! General layout for the façade: flex row/column (gap, padding, grow/shrink, alignment), fixed and
//! percentage sizes, absolute positioning, and measured text leaves with wrapping, all computed by
//! cephal (the user's retained, incremental CSS engine, used unmodified).
//!
//! Layout runs in **device pixels** (every length is `points * scale` at mount), so cephal's sizes,
//! the text shaper's metrics and bough's instances share one unit.
//!
//! Structure (see LAYOUT.md):
//!
//! * builders (`column`, `row`, `block`, `para`, `label`) lower a typed description at mount time into
//!   (a) layout nodes in the engine (`Eng`: the mirror tree or the deep integration, chosen by the
//!   `deep` cargo feature), (b) the usual Bough records + instance runs + per-element `Node` state,
//!   with *provisional* (empty) ink;
//! * a **group** is one independent layout tree (the window's root, or one row of a list): it is laid
//!   out on its own (`compute`), then **written back** (`writeback`): positions go into the instances
//!   (group-local px; the group's space adds the origin) and the ink goes through the Bough's write
//!   path (`set_ink_handle`: old and new support to damage, exact);
//! * incremental edits (a text change) invalidate the leaf in the engine; the engine's `changed()`
//!   set bounds the write-back to the nodes that moved or resized, and their subtrees.
use crate::color::Color;
use crate::elem::{ApplyBg, ApplyText, Element, IntoProp, Mounter};
use crate::rt::*;
use bough_core::{NRect, RecordInit};
use bough_text::atlas::Instance;
use bough_text::wrap::wrap_greedy;
use bough_text::GlyphRun;
use cephal::style::{
    AlignItems, Dimension, Display, FlexDirection, JustifyContent, LengthPercentage, LengthPercentageAuto, Position,
};
use cephal::{AvailableSpace, MeasureInput, MeasureOutput, Size, Style};
use std::hash::{BuildHasherDefault, Hash, Hasher};

pub const NONE: u32 = u32::MAX;
/// A paint-less layout box's record (empty ink, no instances): present only in the deep variant, whose layout
/// structure *is* the Bough's depth column.
pub const REC_BOX: u8 = 3;
const DEEP: bool = cfg!(feature = "deep");

/// Measure-callback counters (relaxed; for the cost breakdown in LAYOUT.md).
pub static MEASURE_CALLS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub static WRAP_RUNS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

#[cfg(not(feature = "deep"))]
pub type Eng = crate::lay_mirror::Mirror;
#[cfg(feature = "deep")]
pub type Eng = crate::lay_deep::Deep;

// =============================================================================== spec (points)

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Dim {
    Auto,
    Pt(f32),
    /// Fraction of the parent (0..=1).
    Pct(f32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Start,
    End,
    Center,
    Stretch,
    Baseline,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Justify {
    Start,
    End,
    Center,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
}

/// A box's layout properties in points (converted to a cephal `Style` at mount, in device px).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spec {
    pub container: bool,
    pub column: bool,
    pub w: Dim,
    pub h: Dim,
    pub min_w: f32,
    pub min_h: f32,
    pub pad: [f32; 4], // left, top, right, bottom
    pub margin: [f32; 4],
    pub gap: f32,
    pub grow: f32,
    pub shrink: f32,
    pub items: Option<Align>,
    pub self_align: Option<Align>,
    pub justify: Option<Justify>,
    /// Absolute: (left, top) inset in points; the box sizes itself by `w`/`h`.
    pub abs: Option<(f32, f32)>,
    /// `position: relative`: the containing block of absolutely positioned children.
    pub relative: bool,
    /// `overflow: clip`: content overflow stays inside the box for layout (cephal stops propagating there).
    pub clip: bool,
}

impl Spec {
    pub const fn new(container: bool, column: bool) -> Spec {
        Spec {
            container,
            column,
            w: Dim::Auto,
            h: Dim::Auto,
            min_w: 0.0,
            min_h: 0.0,
            pad: [0.0; 4],
            margin: [0.0; 4],
            gap: 0.0,
            grow: 0.0,
            shrink: 1.0,
            items: None,
            self_align: None,
            justify: None,
            abs: None,
            relative: false,
            clip: false,
        }
    }

    fn dim(d: Dim, s: f32) -> Dimension {
        match d {
            Dim::Auto => Dimension::AUTO,
            Dim::Pt(v) => Dimension::length(v * s),
            Dim::Pct(f) => Dimension::percent(f),
        }
    }
    fn align(a: Align) -> AlignItems {
        match a {
            Align::Start => AlignItems::FLEX_START,
            Align::End => AlignItems::FLEX_END,
            Align::Center => AlignItems::CENTER,
            Align::Stretch => AlignItems::STRETCH,
            Align::Baseline => AlignItems::BASELINE,
        }
    }
    fn justify_c(j: Justify) -> JustifyContent {
        match j {
            Justify::Start => JustifyContent::FLEX_START,
            Justify::End => JustifyContent::FLEX_END,
            Justify::Center => JustifyContent::CENTER,
            Justify::SpaceBetween => JustifyContent::SPACE_BETWEEN,
            Justify::SpaceAround => JustifyContent::SPACE_AROUND,
            Justify::SpaceEvenly => JustifyContent::SPACE_EVENLY,
        }
    }

    /// The cephal style for this spec at `scale` device px per point.
    pub fn to_style(&self, s: f32) -> Style {
        let mut st = Style::DEFAULT;
        if self.container {
            st.display = Display::Flex;
            st.flex_direction = if self.column { FlexDirection::Column } else { FlexDirection::Row };
        }
        st.size.width = Self::dim(self.w, s);
        st.size.height = Self::dim(self.h, s);
        if self.min_w > 0.0 {
            st.min_size.width = LengthPercentageAuto::length(self.min_w * s);
        }
        if self.min_h > 0.0 {
            st.min_size.height = LengthPercentageAuto::length(self.min_h * s);
        }
        let l = |v: f32| LengthPercentage::length(v * s);
        st.padding.left = l(self.pad[0]);
        st.padding.top = l(self.pad[1]);
        st.padding.right = l(self.pad[2]);
        st.padding.bottom = l(self.pad[3]);
        let la = |v: f32| LengthPercentageAuto::length(v * s);
        st.margin.left = la(self.margin[0]);
        st.margin.top = la(self.margin[1]);
        st.margin.right = la(self.margin[2]);
        st.margin.bottom = la(self.margin[3]);
        st.gap.width = l(self.gap);
        st.gap.height = l(self.gap);
        st.flex_grow = self.grow;
        st.flex_shrink = self.shrink;
        st.align_items = self.items.map(Self::align);
        st.align_self = self.self_align.map(Self::align);
        st.justify_content = self.justify.map(Self::justify_c);
        if self.relative {
            st.position = Position::Relative;
        }
        if self.clip {
            st.overflow = cephal::Point { x: cephal::style::Overflow::Clip, y: cephal::style::Overflow::Clip };
        }
        if let Some((x, y)) = self.abs {
            st.position = Position::Absolute;
            st.inset.left = la(x);
            st.inset.top = la(y);
        }
        st
    }
}

/// Style methods shared by every layout builder.
pub trait Styled: Sized {
    fn spec_mut(&mut self) -> &mut Spec;
    /// Width in points.
    fn w(mut self, v: f32) -> Self {
        self.spec_mut().w = Dim::Pt(v);
        self
    }
    fn h(mut self, v: f32) -> Self {
        self.spec_mut().h = Dim::Pt(v);
        self
    }
    fn size(mut self, w: f32, h: f32) -> Self {
        self.spec_mut().w = Dim::Pt(w);
        self.spec_mut().h = Dim::Pt(h);
        self
    }
    /// Width / height as a fraction of the parent (1.0 = 100%).
    fn w_pct(mut self, f: f32) -> Self {
        self.spec_mut().w = Dim::Pct(f);
        self
    }
    fn h_pct(mut self, f: f32) -> Self {
        self.spec_mut().h = Dim::Pct(f);
        self
    }
    fn min_w(mut self, v: f32) -> Self {
        self.spec_mut().min_w = v;
        self
    }
    fn min_h(mut self, v: f32) -> Self {
        self.spec_mut().min_h = v;
        self
    }
    fn pad(mut self, l: f32, t: f32, r: f32, b: f32) -> Self {
        self.spec_mut().pad = [l, t, r, b];
        self
    }
    fn p(self, v: f32) -> Self {
        self.pad(v, v, v, v)
    }
    fn px(mut self, v: f32) -> Self {
        self.spec_mut().pad[0] = v;
        self.spec_mut().pad[2] = v;
        self
    }
    fn py(mut self, v: f32) -> Self {
        self.spec_mut().pad[1] = v;
        self.spec_mut().pad[3] = v;
        self
    }
    fn margin(mut self, l: f32, t: f32, r: f32, b: f32) -> Self {
        self.spec_mut().margin = [l, t, r, b];
        self
    }
    fn gap(mut self, v: f32) -> Self {
        self.spec_mut().gap = v;
        self
    }
    fn grow(mut self, v: f32) -> Self {
        self.spec_mut().grow = v;
        self
    }
    fn shrink(mut self, v: f32) -> Self {
        self.spec_mut().shrink = v;
        self
    }
    fn items(mut self, a: Align) -> Self {
        self.spec_mut().items = Some(a);
        self
    }
    fn align_self(mut self, a: Align) -> Self {
        self.spec_mut().self_align = Some(a);
        self
    }
    fn justify(mut self, j: Justify) -> Self {
        self.spec_mut().justify = Some(j);
        self
    }
    /// `overflow: clip`: the box's content overflow does not reach its parent's layout (an edit inside
    /// stops propagating here).
    fn clip_overflow(mut self) -> Self {
        self.spec_mut().clip = true;
        self
    }
    /// `position: relative` (the containing block of `abs` children).
    fn relative(mut self) -> Self {
        self.spec_mut().relative = true;
        self
    }
    /// Absolutely positioned at (x, y) points from the parent's padding box.
    fn abs(mut self, x: f32, y: f32) -> Self {
        self.spec_mut().abs = Some((x, y));
        self
    }
}

#[derive(Default)]
pub struct Fx(u64);
impl Hasher for Fx {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0.rotate_left(5) ^ b as u64).wrapping_mul(0x517c_c1b7_2722_0a95);
        }
    }
    #[inline]
    fn write_u32(&mut self, v: u32) {
        self.0 = (self.0.rotate_left(5) ^ v as u64).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
    #[inline]
    fn write_u8(&mut self, v: u8) {
        self.write_u32(v as u32)
    }
    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }
}
pub type FxBuild = BuildHasherDefault<Fx>;

/// A `Spec` as a hash key (bit-exact).
#[derive(Clone, Copy)]
pub struct SpecKey(pub Spec);
impl PartialEq for SpecKey {
    fn eq(&self, o: &Self) -> bool {
        self.0 == o.0
    }
}
impl Eq for SpecKey {}
impl Hash for SpecKey {
    fn hash<H: Hasher>(&self, h: &mut H) {
        let s = &self.0;
        h.write_u8(s.container as u8 | (s.column as u8) << 1 | (s.relative as u8) << 2);
        let dim = |h: &mut H, d: crate::layout::Dim| match d {
            crate::layout::Dim::Auto => h.write_u32(0),
            crate::layout::Dim::Pt(v) => {
                h.write_u32(1);
                h.write_u32(v.to_bits())
            }
            crate::layout::Dim::Pct(v) => {
                h.write_u32(2);
                h.write_u32(v.to_bits())
            }
        };
        dim(h, s.w);
        dim(h, s.h);
        for v in [s.min_w, s.min_h, s.gap, s.grow, s.shrink].into_iter().chain(s.pad).chain(s.margin) {
            h.write_u32(v.to_bits());
        }
        h.write_u8(s.items.map_or(255, |a| a as u8));
        h.write_u8(s.self_align.map_or(255, |a| a as u8));
        h.write_u8(s.justify.map_or(255, |a| a as u8));
        if let Some((x, y)) = s.abs {
            h.write_u32(x.to_bits());
            h.write_u32(y.to_bits());
        }
    }
}


// =============================================================================== measured text

/// One word (a run of non-spaces) of a measured text: its bytes `ws..we` and the pen positions, in font units,
/// where it starts (`x0`) and where it ends (`x1`: the pen at the following space, i.e. including the kerning
/// step into it, as `wrap_greedy` reads `x[we-1] + advance(we-1)`).
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Word {
    pub ws: u32,
    pub we: u32,
    pub x0: i32,
    pub x1: i32,
}

/// A measured text leaf's state.
///
/// The text is shaped once into a `GlyphRun` (the NEON table shaper, rustybuzz when a trigger or non-ASCII byte
/// needs it). **Fast path** (the table shaper covered it, so glyph i is byte i): measuring and wrapping work on a
/// *word table* built from the run in one byte scan (the ~65 words of a paragraph, each with its start and end pen
/// position), not on its ~400 glyphs: `wrap_greedy`'s decision (break before a word whose right edge is past the
/// line) becomes a loop over words, 9x cheaper (see LAYOUT.md). **Slow path**: `wrap_greedy` over the run, and only
/// when glyph i is byte i.
#[derive(Clone, Default)]
pub struct LeafCtx {
    pub fast: bool,
    pub words: Vec<Word>,
    /// Fast path: width of the text on one line, trailing spaces excluded (font units).
    pub line_w: i32,
    pub run: GlyphRun,
    pub text: Vec<u8>,
    /// First byte (fast) / glyph (slow) of each line of the last wrap.
    pub starts: Vec<u32>,
    /// `max_units` of the wrap in `starts` (`i32::MIN`: none) and its widest line, in font units.
    pub wrapped_at: i32,
    pub widest: i32,
    /// Longest word, font units (`-1`: not computed).
    pub min_word: i32,
    pub wrap: bool,
    pub ascii: bool,
    pub upem: f32,
    pub size_px: f32,
    pub lh_px: f32,
}

impl LeafCtx {
    pub fn new(size_px: f32, lh_px: f32, wrap: bool) -> LeafCtx {
        LeafCtx { wrapped_at: i32::MIN, min_word: -1, wrap, size_px, lh_px, ..Default::default() }
    }

    pub fn set_text(&mut self, ts: &mut crate::text::TextSys, text: &str) {
        self.text.clear();
        self.text.extend_from_slice(text.as_bytes());
        self.ascii = text.is_ascii();
        self.upem = ts.shaper.script.upem as f32;
        self.wrapped_at = i32::MIN;
        self.min_word = -1;
        // the table shaper (no GSUB/GPOS trigger, ASCII: glyph i is byte i) or rustybuzz
        self.fast = ts.shaper.shape_into(text, self.size_px, &mut self.run);
        if !self.fast {
            ts.shape_run(text, self.size_px, &mut self.run);
            return;
        }
        // word table from the run: spaces found 8 bytes at a time (SWAR zero-byte mask), words between them
        let (b, run) = (&self.text, &self.run);
        let n = b.len();
        let words = &mut self.words;
        words.clear();
        let mut start = 0usize;
        let mut push = |start: usize, end: usize, words: &mut Vec<Word>| {
            let x1 = if end < n { run.x[end] } else { run.width };
            words.push(Word { ws: start as u32, we: end as u32, x0: run.x[start], x1 });
        };
        let mut base = 0usize;
        const LO7: u64 = 0x7f7f_7f7f_7f7f_7f7f;
        while base + 8 <= n {
            let chunk = u64::from_le_bytes(b[base..base + 8].try_into().unwrap());
            let t = chunk ^ 0x2020_2020_2020_2020;
            // 0x80 in exactly the bytes of `t` that are zero (the spaces)
            let mut m = !(((t & LO7) + LO7) | t | LO7);
            while m != 0 {
                let p = base + (m.trailing_zeros() >> 3) as usize;
                m &= m - 1;
                if p > start {
                    push(start, p, words);
                }
                start = p + 1;
            }
            base += 8;
        }
        for p in base..n {
            if b[p] == b' ' {
                if p > start {
                    push(start, p, words);
                }
                start = p + 1;
            }
        }
        if n > start {
            push(start, n, words);
        }
        self.line_w = self.words.last().map_or(0, |w| w.x1);
    }

    #[inline]
    fn k(&self) -> f32 {
        self.size_px / self.upem.max(1.0)
    }

    /// Does this text wrap at all (a paragraph, whose slow path additionally needs glyph i = byte i)?
    #[inline]
    fn wraps(&self) -> bool {
        self.wrap && self.ascii && (self.fast || self.run.len() == self.text.len())
    }

    /// Wrap at `max_units`; returns the line count (`starts` holds the first byte / glyph of each line).
    pub fn wrap_at(&mut self, max_units: i32) -> usize {
        if self.wrapped_at == max_units && !self.starts.is_empty() {
            return self.starts.len();
        }
        self.wrapped_at = max_units;
        WRAP_RUNS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let empty = self.run.is_empty();
        if empty {
            self.starts.clear();
            self.starts.push(0);
            self.widest = 0;
            return 1;
        }
        if !self.wraps() {
            self.starts.clear();
            self.starts.push(0);
            self.widest = if self.fast { self.line_w } else { self.line_width() };
            return 1;
        }
        if self.fast {
            // wrap_greedy over the word table: break before a word whose right edge is past the line
            self.starts.clear();
            self.starts.push(0);
            let (mut line_x0, mut has_word, mut widest, mut last_x1) = (0i32, false, 0i32, 0i32);
            for w in &self.words {
                if has_word && w.x1 - line_x0 > max_units {
                    widest = widest.max(last_x1 - line_x0);
                    self.starts.push(w.ws);
                    line_x0 = w.x0;
                }
                has_word = true;
                last_x1 = w.x1;
            }
            self.widest = widest.max(last_x1 - line_x0);
            return self.starts.len();
        }
        let n = self.run.len();
        let lines = wrap_greedy(&self.run, &self.text, max_units, &mut self.starts);
        let mut widest = 0;
        for i in 0..lines {
            let a = self.starts[i] as usize;
            let mut b = if i + 1 < lines { self.starts[i + 1] as usize } else { n };
            while b > a && self.text[b - 1] == b' ' {
                b -= 1;
            }
            if b == a {
                continue;
            }
            let right = self.run.x[b - 1] + self.run.advance(b - 1);
            widest = widest.max(right - self.run.x[a]);
        }
        self.widest = widest;
        lines
    }

    /// Width of the whole text on one line, trailing spaces excluded (font units).
    fn line_width(&self) -> i32 {
        if self.fast {
            return self.line_w;
        }
        let n = self.run.len();
        if n == 0 {
            return 0;
        }
        let mut b = n;
        while b > 0 && self.ascii && self.text.get(b - 1) == Some(&b' ') {
            b -= 1;
        }
        if b == 0 {
            return 0;
        }
        self.run.x[b - 1] + self.run.advance(b - 1)
    }

    #[doc(hidden)]
    pub fn longest_word_for_test(&mut self) -> i32 {
        self.longest_word()
    }

    fn longest_word(&mut self) -> i32 {
        if self.min_word >= 0 {
            return self.min_word;
        }
        let best = if self.fast {
            self.words.iter().map(|w| w.x1 - w.x0).max().unwrap_or(0)
        } else {
            let (n, mut best, mut i) = (self.run.len(), 0, 0);
            while i < n && self.wraps() {
                let s = i;
                while i < n && self.text[i] != b' ' {
                    i += 1;
                }
                if i > s {
                    best = best.max(self.run.x[i - 1] + self.run.advance(i - 1) - self.run.x[s]);
                }
                while i < n && self.text[i] == b' ' {
                    i += 1;
                }
            }
            best
        };
        self.min_word = best;
        best
    }

    /// cephal's measure callback: content-box size of the text under `input`.
    pub fn measure(&mut self, input: MeasureInput) -> MeasureOutput {
        MEASURE_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let k = self.k().max(1e-6);
        let lh = self.lh_px;
        if !self.wraps() {
            let w = self.line_width() as f32 * k;
            return MeasureOutput::from_size(Size { width: input.known_dimensions.width.unwrap_or(w), height: input.known_dimensions.height.unwrap_or(lh) });
        }
        let width = match (input.known_dimensions.width, input.available_space.width) {
            (Some(w), _) => Some(w),
            (None, AvailableSpace::Definite(w)) => Some(w),
            (None, AvailableSpace::MinContent) => Some(self.longest_word() as f32 * k),
            (None, AvailableSpace::MaxContent) => None,
        };
        let (lines, widest) = match width {
            Some(w) => {
                let units = ((w + 1e-3) / k).floor() as i32;
                let l = self.wrap_at(units);
                (l, self.widest)
            }
            // max-content: one line, as wide as the text (no wrap pass)
            None => (1, self.line_width()),
        };
        MeasureOutput::from_size(Size {
            width: input.known_dimensions.width.unwrap_or(widest as f32 * k),
            height: input.known_dimensions.height.unwrap_or(lines as f32 * lh),
        })
    }
}

// =============================================================================== the bridge

#[derive(Clone, Copy)]
pub struct LInfo {
    /// `Rt::nodes` index of the drawable (quad / text), or NONE for a paint-less box.
    pub rtn: u32,
    pub parent: u32,
    pub space: u16,
    pub clip: u16,
    /// 0 box without paint, 1 quad, 2 text.
    pub kind: u8,
    pub live: bool,
    /// Written at least once.
    pub written: bool,
    /// Its layout changed in the last compute.
    pub needs: bool,
    /// A descendant's did.
    pub desc: bool,
    /// Its text changed (re-place even if the box did not move).
    pub tdirty: bool,
    /// Group roots: queued for compute.
    pub queued: bool,
    /// Last written absolute position and size (group-local px).
    pub ax: f32,
    pub ay: f32,
    pub aw: f32,
    pub ah: f32,
    /// Group roots: available space (px); `NaN` height = max-content.
    pub avail_w: f32,
    pub avail_h: f32,
}

impl LInfo {
    const fn blank() -> LInfo {
        LInfo {
            rtn: NONE,
            parent: NONE,
            space: 0,
            clip: 0,
            kind: 0,
            live: false,
            written: false,
            needs: false,
            desc: false,
            tdirty: false,
            queued: false,
            ax: 0.0,
            ay: 0.0,
            aw: 0.0,
            ah: 0.0,
            avail_w: 0.0,
            avail_h: f32::NAN,
        }
    }
}

#[derive(Default, Clone, Copy, Debug)]
pub struct LayStats {
    pub computes: u64,
    pub nodes_added: u64,
    pub nodes_written: u64,
    pub text_placed: u64,
}

/// Instruction-count profiler for the layout path (proc_pid_rusage; off unless enabled): mount (builders
/// -> engine nodes + records), insert (records into the Bough), compute (the engine), write-back
/// (instances + ink), unmount.
pub struct LProf {
    pub on: bool,
    pub acc: [u64; 8],
    last: u64,
    ovh: u64,
}
pub const LP_MOUNT: usize = 0;
pub const LP_INSERT: usize = 1;
pub const LP_COMPUTE: usize = 2;
pub const LP_WRITE: usize = 3;
pub const LP_UNMOUNT: usize = 4;
pub const LP_TEXT_SET: usize = 5;
pub const LP_TEXT_EMIT: usize = 6;
pub const LP_TEXT_COPY: usize = 7;
pub const LP_NAMES: [&str; 8] = ["mount", "insert", "compute", "write-back", "unmount", "(text set_text)", "(text emit)", "(text copy+ink)"];
impl LProf {
    pub fn enable(&mut self) {
        self.on = true;
        self.ovh = bough_metal::perf::snap_overhead();
    }
    #[inline]
    pub fn reset(&mut self) {
        if self.on {
            self.last = bough_metal::perf::snap().instrs;
        }
    }
    #[inline]
    pub fn mark(&mut self, i: usize) {
        if self.on {
            let s = bough_metal::perf::snap().instrs;
            self.acc[i] += (s - self.last).saturating_sub(self.ovh);
            self.last = bough_metal::perf::snap().instrs;
        }
    }
}

pub struct Lay {
    pub eng: Eng,
    pub prof: LProf,
    pub info: Vec<LInfo>,
    /// Layout parent while a subtree is being mounted (NONE: the next box is a group root).
    pub cur: u32,
    /// Available space (px) a group root created now gets; height `NaN` = max-content.
    pub mount_avail: (f32, f32),
    pub pending: Vec<u32>,
    pub ready: Vec<u32>,
    pub scale: f32,
    pub stats: LayStats,
    stack: Vec<(u32, f32, f32, bool)>,
    kids: Vec<u32>,
    line: GlyphRun,
    /// Roots whose text changed reactively since the last flush (for lists to re-measure rows).
    pub touched_roots: Vec<u32>,
}

impl Lay {
    pub fn new(scale: f32) -> Lay {
        Lay {
            eng: Eng::new(),
            prof: LProf { on: false, acc: [0; 8], last: 0, ovh: 0 },
            info: Vec::new(),
            cur: NONE,
            mount_avail: (0.0, f32::NAN),
            pending: Vec::new(),
            ready: Vec::new(),
            scale,
            stats: LayStats::default(),
            stack: Vec::new(),
            kids: Vec::new(),
            line: GlyphRun::default(),
            touched_roots: Vec::new(),
        }
    }

    /// Register a freshly created engine node.
    pub fn register(&mut self, id: u32, parent: u32) {
        if self.info.len() <= id as usize {
            self.info.resize(id as usize + 1, LInfo::blank());
        }
        self.info[id as usize] = LInfo { live: true, parent, ..LInfo::blank() };
        self.stats.nodes_added += 1;
    }

    pub fn root_of(&self, mut n: u32) -> u32 {
        while self.info[n as usize].parent != NONE {
            n = self.info[n as usize].parent;
        }
        n
    }

    /// Group height (px) after `compute`.
    pub fn root_height(&self, root: u32) -> f32 {
        self.eng.rect(root).3
    }

    /// Test / inspection helpers: the first live group root, a node's i-th child and its rect.
    pub fn first_root(&self) -> u32 {
        self.info.iter().position(|i| i.live && i.parent == NONE).expect("a layout group") as u32
    }

}

impl Rt {
    /// Test / inspection helpers: a layout node's i-th child and its rect.
    pub fn lay_child(&self, n: u32, i: usize) -> u32 {
        let mut v = Vec::new();
        self.lay.eng.children_into(&self.bough, n, &mut v);
        v[i]
    }
    pub fn lay_child_rect(&self, n: u32, i: usize) -> (f32, f32, f32, f32) {
        self.lay.eng.rect(self.lay_child(n, i))
    }

    /// Release a group: its layout nodes (the drawables are released with the scope's nodes).
    pub fn lay_remove_root(&mut self, root: u32) {
        let mut stack = std::mem::take(&mut self.lay.kids);
        stack.clear();
        stack.push(root);
        let mut all: Vec<u32> = Vec::new();
        while let Some(n) = stack.pop() {
            all.push(n);
            self.lay.eng.children_into(&self.bough, n, &mut stack);
        }
        self.lay.eng.remove_tree(&self.bough, root, &all);
        for n in all {
            self.lay.info[n as usize].live = false;
        }
        if let Some(i) = self.lay.pending.iter().position(|&r| r == root) {
            self.lay.pending.swap_remove(i);
        }
        if let Some(i) = self.lay.ready.iter().position(|&r| r == root) {
            self.lay.ready.swap_remove(i);
        }
        self.lay.kids = stack;
    }

    /// The text of layout text node `n` changed (reactive operand): re-shape into the leaf, invalidate it.
    pub fn lay_text_changed(&mut self, n: u32) {
        let lid = self.nodes[n as usize].lnode;
        let text = std::mem::take(&mut self.nodes[n as usize].text);
        if let Some(ctx) = self.lay.eng.ctx_mut(lid) {
            ctx.set_text(&mut self.text, &text);
        }
        self.nodes[n as usize].text = text;
        self.lay.eng.invalidate(&self.bough, lid);
        self.lay.info[lid as usize].tdirty = true;
        let root = self.lay.root_of(lid);
        if !self.lay.info[root as usize].queued {
            self.lay.info[root as usize].queued = true;
            self.lay.pending.push(root);
        }
        self.lay.touched_roots.push(root);
    }

    /// Lay out every pending group (no write-back yet).
    pub fn lay_compute_pending(&mut self) {
        self.lay.prof.reset();
        let mut pend = std::mem::take(&mut self.lay.pending);
        for &r in &pend {
            let li = self.lay.info[r as usize];
            let avail = Size {
                width: AvailableSpace::Definite(li.avail_w),
                height: if li.avail_h.is_nan() { AvailableSpace::MaxContent } else { AvailableSpace::Definite(li.avail_h) },
            };
            self.lay.eng.layout(&self.bough, r, avail);
            self.lay.stats.computes += 1;
            // flag what moved / resized, and the path to it
            let mut i = 0;
            while let Some(c) = self.lay.eng.changed_at(i) {
                i += 1;
                self.lay.info[c as usize].needs = true;
                let mut p = self.lay.info[c as usize].parent;
                while p != NONE && !self.lay.info[p as usize].desc {
                    self.lay.info[p as usize].desc = true;
                    p = self.lay.info[p as usize].parent;
                }
            }
            self.lay.info[r as usize].queued = false;
            self.lay.ready.push(r);
        }
        pend.clear();
        self.lay.pending = pend;
        self.lay.prof.mark(LP_COMPUTE);
    }

    /// Write the laid-out groups back into instances and ink.
    pub fn lay_writeback_ready(&mut self) {
        self.lay.prof.reset();
        let mut ready = std::mem::take(&mut self.lay.ready);
        for &r in &ready {
            self.lay_writeback(r);
        }
        ready.clear();
        self.lay.ready = ready;
        self.lay.prof.mark(LP_WRITE);
    }

    pub fn lay_flush(&mut self) {
        if self.lay.pending.is_empty() && self.lay.ready.is_empty() {
            return;
        }
        self.lay_compute_pending();
        self.lay_writeback_ready();
    }

    fn lay_writeback(&mut self, root: u32) {
        let mut stack = std::mem::take(&mut self.lay.stack);
        let mut kids = std::mem::take(&mut self.lay.kids);
        stack.clear();
        stack.push((root, 0.0, 0.0, false));
        while let Some((n, px, py, force)) = stack.pop() {
            let li = self.lay.info[n as usize];
            if !(force || li.needs || li.desc || !li.written || li.tdirty) {
                continue;
            }
            let (rx, ry, w, h) = self.lay.eng.rect(n);
            let (ax, ay) = (px + rx, py + ry);
            let moved = !li.written || ax != li.ax || ay != li.ay;
            let sized = !li.written || w != li.aw || h != li.ah;
            if li.kind != 0 && (moved || sized || li.tdirty) {
                match li.kind {
                    1 => self.place_quad(n, ax, ay, w, h),
                    _ => self.place_text(n, ax, ay, w, h, sized || li.tdirty),
                }
                self.lay.stats.nodes_written += 1;
            }
            {
                let l = &mut self.lay.info[n as usize];
                l.ax = ax;
                l.ay = ay;
                l.aw = w;
                l.ah = h;
                l.written = true;
                l.needs = false;
                l.desc = false;
                l.tdirty = false;
            }
            kids.clear();
            self.lay.eng.children_into(&self.bough, n, &mut kids);
            let fc = force || moved;
            for &c in &kids {
                stack.push((c, ax, ay, fc));
            }
        }
        self.lay.stack = stack;
        self.lay.kids = kids;
    }

    fn place_quad(&mut self, lid: u32, x: f32, y: f32, w: f32, h: f32) {
        let li = self.lay.info[lid as usize];
        let n = &mut self.nodes[li.rtn as usize];
        n.x = x;
        n.y = y;
        n.w = w;
        n.h = h;
        let inst = self.runs[n.run as usize].first;
        let p = unsafe { &mut *self.inst_ptr(inst) };
        p.xy = [x, y];
        p.wh = [w, h];
        let ink = self.ink_rect(li.space, x, y, w, h, self.nodes[li.rtn as usize].margin);
        let hd = self.nodes[li.rtn as usize].handle;
        self.bough.set_ink_handle(hd, ink);
    }

    /// (Re)generate a laid-out text node's instances for its box at (x, y) of width `w`.
    fn place_text(&mut self, lid: u32, x: f32, y: f32, w: f32, _h: f32, reflow: bool) {
        self.lay.stats.text_placed += 1;
        let li = self.lay.info[lid as usize];
        let (space, clip, run_id, color, strike, size_px, lh) = {
            let n = &self.nodes[li.rtn as usize];
            (n.space, n.clip, n.run, n.color, n.strike as usize, n.size_px, n.line_h)
        };
        let base_off = self.text.baseline(size_px, lh);
        // the run must hold one instance per glyph (the kernel writes straight into it)
        let n = self.lay.eng.ctx_mut(lid).expect("text leaf has a context").run.len();
        if n > (1usize << self.runs[run_id as usize].class) {
            self.regrow_run(run_id, n);
        }
        let first = self.runs[run_id as usize].first;
        let dst = self.inst_ptr(first);
        let cap = 1usize << self.runs[run_id as usize].class;
        let Rt { lay, text, .. } = self;
        let ctx = lay.eng.ctx_mut(lid).expect("text leaf has a context");
        let k = ctx.k().max(1e-6);
        let _ = reflow;
        let lines = if ctx.wraps() { ctx.wrap_at(((w + 1e-3) / k).floor() as i32) } else { ctx.wrap_at(i32::MAX) };
        lay.prof.reset();
        // The glyph kernel writes the instances straight into the node's pool run (a `Vec` view of the run: its
        // capacity is the run's, and it never grows: at most one instance per glyph, and the run was sized for it).
        // SAFETY: `dst` points at `cap` instances of pool memory owned by this run; the view is never dropped or
        // reallocated (`cap >= n >= the number of instances every `instances` call reserves for).
        let mut out = std::mem::ManuallyDrop::new(unsafe { Vec::<Instance>::from_raw_parts(dst, 0, cap) });
        for i in 0..lines {
            let a = ctx.starts[i] as usize;
            let b = if i + 1 < lines { ctx.starts[i + 1] as usize } else { n };
            if b <= a {
                continue;
            }
            // the line as its own run (positions rebased to the line start)
            let line = &mut lay.line;
            line.gid.clear();
            line.x.clear();
            line.cluster.clear();
            line.identity_clusters = true;
            line.upem = ctx.run.upem;
            line.size = ctx.run.size;
            let x0 = ctx.run.x[a];
            line.gid.extend_from_slice(&ctx.run.gid[a..b]);
            line.x.extend(ctx.run.x[a..b].iter().map(|v| v - x0));
            line.width = (if b < n { ctx.run.x[b] } else { ctx.run.width }) - x0;
            let baseline = (y + i as f32 * lh + base_off).round();
            text.cache.instances(strike, line, x, baseline, color, &mut out);
        }
        let cnt = out.len();
        debug_assert_eq!(out.as_ptr(), dst as *const Instance, "the run was not reallocated");
        lay.prof.mark(LP_TEXT_EMIT);
        self.runs[run_id as usize].len = cnt as u32;
        // stamp space + clip and take the glyph quads' union, one NEON pass
        let b = unsafe { stamp_and_bounds(dst, cnt, space, clip as u32) };
        let ink = if cnt == 0 { NRect::EMPTY } else { self.ink_rect(space, b[0], b[1], b[2] - b[0], b[3] - b[1], 1.0) };
        let hd = self.nodes[li.rtn as usize].handle;
        self.bough.set_ink_handle(hd, ink);
        self.lay.prof.mark(LP_TEXT_COPY);
    }
}

/// Stamp `space` and the clip index into `cnt` instances in place, and return the union of their quads
/// `[x0, y0, x1, y1]`: per instance one vector load, an extract + add (x + w, y + h), a min and a max.
///
/// # Safety
/// `dst` must be valid for `cnt` instances.
unsafe fn stamp_and_bounds(dst: *mut Instance, cnt: usize, space: u16, clip: u32) -> [f32; 4] {
    const SPACE: usize = std::mem::offset_of!(Instance, space);
    const CLIP: usize = std::mem::offset_of!(Instance, aux) + 12;
    #[cfg(target_arch = "aarch64")]
    unsafe {
        use core::arch::aarch64::*;
        let (mut mn, mut mx) = (vdupq_n_f32(f32::MAX), vdupq_n_f32(f32::MIN));
        for i in 0..cnt {
            let p = dst.add(i) as *mut u8;
            let v = vld1q_f32(p as *const f32); // x y w h
            let sum = vaddq_f32(v, vextq_f32::<2>(v, v)); // x+w y+h ..
            mn = vminq_f32(mn, v);
            mx = vmaxq_f32(mx, sum);
            (p.add(SPACE) as *mut u16).write_unaligned(space);
            (p.add(CLIP) as *mut u32).write_unaligned(clip);
        }
        [vgetq_lane_f32::<0>(mn), vgetq_lane_f32::<1>(mn), vgetq_lane_f32::<0>(mx), vgetq_lane_f32::<1>(mx)]
    }
    #[cfg(not(target_arch = "aarch64"))]
    unsafe {
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for i in 0..cnt {
            let g = &mut *dst.add(i);
            x0 = x0.min(g.xy[0]);
            y0 = y0.min(g.xy[1]);
            x1 = x1.max(g.xy[0] + g.wh[0]);
            y1 = y1.max(g.xy[1] + g.wh[1]);
            g.space = space;
            g.aux[3] = clip;
        }
        [x0, y0, x1, y1]
    }
}

// =============================================================================== builders

/// A layout box: a flex container (`column` / `row`) or a leaf rectangle (`block`), optionally painted.
pub struct LBox<Bg = Color, Ch = ()> {
    spec: Spec,
    draw: bool,
    bg: Bg,
    radius: f32,
    border: Option<(f32, Color)>,
    ch: Ch,
}

pub fn column<Ch>(ch: Ch) -> LBox<Color, Ch> {
    LBox { spec: Spec::new(true, true), draw: false, bg: Color(0), radius: 0.0, border: None, ch }
}
pub fn row<Ch>(ch: Ch) -> LBox<Color, Ch> {
    LBox { spec: Spec::new(true, false), draw: false, bg: Color(0), radius: 0.0, border: None, ch }
}
/// A leaf rectangle of `w` x `h` points (paint it with `.bg`).
pub fn block(w: f32, h: f32) -> LBox<Color, ()> {
    let mut b = LBox { spec: Spec::new(false, false), draw: false, bg: Color(0), radius: 0.0, border: None, ch: () };
    b.spec.w = Dim::Pt(w);
    b.spec.h = Dim::Pt(h);
    b
}

impl<Bg, Ch> Styled for LBox<Bg, Ch> {
    fn spec_mut(&mut self) -> &mut Spec {
        &mut self.spec
    }
}

impl<Bg, Ch> LBox<Bg, Ch> {
    pub fn bg<B2>(self, bg: B2) -> LBox<B2, Ch> {
        LBox { spec: self.spec, draw: true, bg, radius: self.radius, border: self.border, ch: self.ch }
    }
    pub fn rounded(mut self, r: f32) -> Self {
        self.radius = r;
        self.draw = true;
        self
    }
    pub fn border(mut self, w: f32, c: Color) -> Self {
        self.border = Some((w, c));
        self.draw = true;
        self
    }
}

/// Attach the layout node `lid` under the current parent (or make it a group root).
fn attach<V>(m: &mut Mounter<'_, V>, lid: u32) -> u32 {
    let parent = m.rt.lay.cur;
    m.rt.lay.register(lid, parent);
    {
        let l = &mut m.rt.lay.info[lid as usize];
        l.space = m.space;
        l.clip = m.clip;
    }
    if parent != NONE {
        m.rt.lay.eng.append(parent, lid);
    } else {
        let (aw, ah) = m.rt.lay.mount_avail;
        let l = &mut m.rt.lay.info[lid as usize];
        l.avail_w = aw;
        l.avail_h = ah;
        l.queued = true;
        m.rt.lay.eng.mark_root(lid);
        m.scope.lroots.push(lid);
        m.rt.lay.pending.push(lid);
    }
    parent
}

impl<V: 'static, Bg, Ch> Element<V> for LBox<Bg, Ch>
where
    Bg: IntoProp<Color>,
    Ch: Element<V>,
{
    fn mount(self, m: &mut Mounter<'_, V>) {
        let s = m.rt.scale;
        let hd = m.rt.bough.new_handle();
        let lid = m.rt.lay.eng.add(hd, &self.spec, s, None);
        let parent = attach(m, lid);
        let mut deeper = false;
        if self.draw {
            let node = m.rt.alloc_node();
            let bg = self.bg.lower::<ApplyBg, V>(m, node);
            let kind = match (self.border.is_some(), self.radius > 0.0) {
                (true, _) => KIND_BORDER,
                (false, true) => KIND_RRECT,
                _ => KIND_SOLID,
            };
            let margin = if kind == KIND_SOLID { 0.0 } else { 1.0 };
            let run = m.rt.alloc_run(1);
            m.rt.runs[run as usize].len = 1;
            let inst = m.rt.runs[run as usize].first;
            let mut ins = Instance { xy: [0.0; 2], wh: [0.0; 2], uv: [0; 4], color: bg.0, kind, space: m.space, aux: [0, 0, 0, m.clip as u32] };
            if let Some((bw, bc)) = self.border {
                ins.color = bc.0;
                ins.aux[0] = bg.0;
                ins.aux[2] = (bw * s).to_bits();
            }
            ins.aux[1] = (self.radius * s).to_bits();
            unsafe { m.rt.inst_ptr(inst).write(ins) };
            {
                let n = &mut m.rt.nodes[node as usize];
                n.handle = hd;
                n.run = run;
                n.space = m.space;
                n.clip = m.clip;
                n.kind = if self.border.is_some() { 3 } else { 1 };
                n.color = bg.0;
                n.margin = margin;
                n.lnode = lid;
            }
            m.scope.nodes.push(node);
            m.scope.runs.push(run);
            m.rt.lay.info[lid as usize].rtn = node;
            m.rt.lay.info[lid as usize].kind = 1;
            m.recs.push(RecordInit { depth: m.depth, ink: NRect::EMPTY, kind: REC_QUAD, payload: run, handle: hd.idx });
            m.depth += 1;
            deeper = true;
        } else if DEEP {
            m.recs.push(RecordInit { depth: m.depth, ink: NRect::EMPTY, kind: REC_BOX, payload: 0, handle: hd.idx });
            m.depth += 1;
            deeper = true;
        }
        m.rt.lay.cur = lid;
        self.ch.mount(m);
        m.rt.lay.cur = parent;
        if deeper {
            m.depth -= 1;
        }
    }
}

/// Wrapping text (`para`) or a single-line label (`label`), measured by the shaper.
pub struct Para<S> {
    s: S,
    spec: Spec,
    size: f32,
    lh: f32,
    color: Color,
    wrap: bool,
}

/// A paragraph that wraps at its width (set `.w(..)`, or it fills the available width).
pub fn para<S>(s: S) -> Para<S> {
    Para { s, spec: Spec::new(false, false), size: 12.0, lh: 16.0, color: Color::rgba(255, 255, 255, 255), wrap: true }
}
/// One line of text; the box is as wide as the text.
pub fn label<S>(s: S) -> Para<S> {
    Para { wrap: false, ..para(s) }
}

impl<S> Styled for Para<S> {
    fn spec_mut(&mut self) -> &mut Spec {
        &mut self.spec
    }
}
impl<S> Para<S> {
    pub fn font(mut self, size_pt: f32, line_height_pt: f32) -> Self {
        self.size = size_pt;
        self.lh = line_height_pt;
        self
    }
    pub fn color(mut self, c: Color) -> Self {
        self.color = c;
        self
    }
}

impl<V: 'static, S: IntoProp<String>> Element<V> for Para<S> {
    fn mount(self, m: &mut Mounter<'_, V>) {
        let s = m.rt.scale;
        let node = m.rt.alloc_node();
        let init = self.s.lower::<ApplyText, V>(m, node);
        let size_px = self.size * s;
        let lh_px = self.lh * s;
        let strike = m.rt.text.strike(size_px);
        let mut ctx = LeafCtx::new(size_px, lh_px, self.wrap);
        m.rt.lay.prof.reset();
        ctx.set_text(&mut m.rt.text, &init);
        m.rt.lay.prof.mark(LP_TEXT_SET);
        let hd = m.rt.bough.new_handle();
        let lid = m.rt.lay.eng.add(hd, &self.spec, s, Some(ctx));
        let _parent = attach(m, lid);
        let run = m.rt.alloc_run(init.len().max(4));
        {
            let n = &mut m.rt.nodes[node as usize];
            n.text = init;
            n.run = run;
            n.space = m.space;
            n.clip = m.clip;
            n.kind = NK_LTEXT;
            n.color = self.color.0;
            n.strike = strike as u32;
            n.size_px = size_px;
            n.line_h = lh_px;
            n.handle = hd;
            n.lnode = lid;
        }
        m.rt.lay.info[lid as usize].rtn = node;
        m.rt.lay.info[lid as usize].kind = 2;
        m.scope.nodes.push(node);
        m.scope.runs.push(run);
        m.recs.push(RecordInit { depth: m.depth, ink: NRect::EMPTY, kind: REC_TEXT, payload: run, handle: hd.idx });
    }
}

pub mod prelude {
    pub use super::{block, column, label, para, row, Align, Justify, Styled};
}
