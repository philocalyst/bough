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
use crate::elem::{quad_instance, ApplyBg, Element, IntoProp, Mounter, TextSrc};
use crate::rt::*;
use crate::text::{TShape, TextNode};
use bough_core::{NRect, RecordInit};
use bough_metal::fused::{pay_irun, pay_runs};
use bough_metal::instance::Instance;
use bough_text::pool::GlyphPool;
use bough_text::para::Para as TextPara;
use bough_text::window::Shaped;
use bough_text::wrap::wrap_greedy;
use bough_text::GlyphRun;
use cephal::style::{
    AlignItems, Dimension, Display, FlexDirection, JustifyContent, LengthPercentage, LengthPercentageAuto, Position,
};
use cephal::{AvailableSpace, MeasureInput, MeasureOutput, Size, Style};
use std::hash::{BuildHasherDefault, Hash, Hasher};

pub use crate::rt::NONE;

/// Measure-callback counters (relaxed; for the cost breakdown in LAYOUT.md).
pub static MEASURE_CALLS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub static WRAP_RUNS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The layout engine: cephal's `LayoutTree`/`CacheAccess`/`Incremental` implemented over the Bough itself
/// (`lay_deep.rs`; there is no second tree).
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
#[derive(Default)]
pub struct LeafCtx {
    pub fast: bool,
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
    /// Fast path: the text shaped into a glyph-pool block (the GPU draws its lines as windows onto it; the
    /// word table is read from its pens: shaped once for measuring and drawing). A view of `para`'s block.
    pub block: Option<Shaped>,
    /// Fast path: the paragraph (byte slots, word table, wraps per width), edited line-locally.
    pub para: TextPara,
}

/// A clone is the leaf's measuring state without its pool block (it never owns the block: the original does;
/// its paragraph is a measuring copy).
impl Clone for LeafCtx {
    fn clone(&self) -> Self {
        LeafCtx {
            fast: self.fast,
            line_w: self.line_w,
            run: self.run.clone(),
            text: self.text.clone(),
            starts: self.starts.clone(),
            wrapped_at: self.wrapped_at,
            widest: self.widest,
            min_word: self.min_word,
            wrap: self.wrap,
            ascii: self.ascii,
            upem: self.upem,
            size_px: self.size_px,
            lh_px: self.lh_px,
            block: None,
            para: self.para.clone(),
        }
    }
}

impl LeafCtx {
    pub fn new(size_px: f32, lh_px: f32, wrap: bool) -> LeafCtx {
        LeafCtx { wrapped_at: i32::MIN, min_word: -1, wrap, size_px, lh_px, ..Default::default() }
    }

    /// Release the pool block (the leaf is going away).
    pub fn release(&mut self, pool: &mut GlyphPool) {
        self.para.release(pool);
        self.block = None;
    }

    /// Set the leaf's text. Fast path (any covered text: byte slots in a glyph-pool block): the leaf's
    /// paragraph (`bough_text::para::Para`, text round 3) is **edited line-locally**: only the changed words are
    /// shaped, the rest of the slots and the word table shift by constants, and the cached wraps re-wrap from the
    /// line before the change until a break lands where it was. Slow path (beyond Latin-1): the general shaper
    /// into a glyph run.
    pub fn set_text(&mut self, ts: &mut crate::text::TextSys, pool: &mut GlyphPool, text: &str) {
        self.text.clear();
        self.text.extend_from_slice(text.as_bytes());
        self.ascii = text.is_ascii();
        self.upem = ts.shaper.script.upem as f32;
        self.wrapped_at = i32::MIN;
        self.min_word = -1;
        self.run.gid.clear();
        self.run.x.clear();
        let fonts = [&ts.shaper];
        let ok = if self.fast && self.para.base != u32::MAX {
            self.para.edit(pool, &fonts, text).is_ok()
        } else {
            self.para.release(pool);
            self.para.set(pool, &fonts, text, &[]).is_ok()
        };
        if !ok {
            self.para.release(pool);
            self.para = TextPara::default();
            self.fast = false;
            self.block = None;
            ts.shape_run(text, self.size_px, &mut self.run);
            return;
        }
        self.fast = true;
        let p = &self.para;
        self.block = Some(Shaped { base: p.base, cap: p.cap, len: text.len() as u32, width: p.width, exotic: p.exotic });
        self.line_w = p.words.last().map_or(0, |w| w.x1);
    }

    #[inline]
    fn k(&self) -> f32 {
        self.size_px / self.upem.max(1.0)
    }

    /// Does this text wrap at all (a paragraph, whose slow path additionally needs glyph i = byte i)?
    #[inline]
    fn wraps(&self) -> bool {
        // fast: byte slots (any covered text, ligatures and Latin-1 included), so a byte is a slot
        self.wrap && (self.fast || (self.ascii && self.run.len() == self.text.len()))
    }

    /// Wrap at `max_units`; returns the line count (`starts` holds the first byte / glyph of each line).
    pub fn wrap_at(&mut self, max_units: i32) -> usize {
        if self.wrapped_at == max_units && !self.starts.is_empty() {
            return self.starts.len();
        }
        self.wrapped_at = max_units;
        WRAP_RUNS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let empty = if self.fast { self.text.is_empty() } else { self.run.is_empty() };
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
            // the paragraph's wrap: cached per width, re-wrapped from the last edit until it converges
            self.para.wrap(max_units);
            let (p, starts) = (&self.para, &mut self.starts);
            let w = p.wrap_cached(max_units).expect("just wrapped");
            starts.clear();
            starts.extend(w.starts.iter().map(|&s| if s == 0 { 0 } else { p.words[s as usize].ws }));
            self.widest = w.widest;
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
        while b > 0 && (self.ascii || self.fast) && self.text.get(b - 1) == Some(&b' ') {
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
            self.para.words.iter().map(|w| w.x1 - w.x0).max().unwrap_or(0)
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
    pub nodes_visited: u64,
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
    /// Nodes whose layout changed (engine's `changed()`) or whose text changed since the last write-back.
    pub dirty: Vec<u32>,
    pub scale: f32,
    pub stats: LayStats,
    stack: Vec<(u32, f32, f32, bool)>,
    kids: Vec<u32>,
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
            dirty: Vec::new(),
            scale,
            stats: LayStats::default(),
            stack: Vec::new(),
            kids: Vec::new(),
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
        for &n in &all {
            if let Some(ctx) = self.lay.eng.ctx_mut(n) {
                ctx.release(&mut self.glyphs);
            }
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
    pub fn lay_text_changed(&mut self, n: u32, text: &str) {
        let lid = self.nodes[n as usize].lnode;
        let ti = self.nodes[n as usize].text;
        {
            let t = &mut self.texts[ti as usize];
            t.text.clear();
            t.text.push_str(text);
        }
        let Rt { lay, text: ts, glyphs, .. } = self;
        if let Some(ctx) = lay.eng.ctx_mut(lid) {
            ctx.set_text(ts, glyphs, text);
        }
        self.lay.eng.invalidate(&self.bough, lid);
        self.lay.info[lid as usize].tdirty = true;
        self.lay.dirty.push(lid);
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
            // what moved / resized: the write-back starts from exactly these nodes
            let mut i = 0;
            while let Some(c) = self.lay.eng.changed_at(i) {
                i += 1;
                self.lay.info[c as usize].needs = true;
                self.lay.dirty.push(c);
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
        self.lay_writeback();
        self.lay.ready.clear();
        self.lay.prof.mark(LP_WRITE);
    }

    pub fn lay_flush(&mut self) {
        if self.lay.pending.is_empty() && self.lay.ready.is_empty() {
            return;
        }
        self.lay_compute_pending();
        self.lay_writeback_ready();
    }

    /// Write back every group laid out since the last call: a cold group (never written) from its root down;
    /// otherwise only from the nodes whose layout or text changed (their absolute position is the sum of the
    /// rects up their parent chain), descending into a node's children only if it moved. A node's untouched
    /// siblings are never visited.
    fn lay_writeback(&mut self) {
        let mut stack = std::mem::take(&mut self.lay.stack);
        let mut kids = std::mem::take(&mut self.lay.kids);
        let mut dirty = std::mem::take(&mut self.lay.dirty);
        stack.clear();
        for &r in &self.lay.ready {
            if !self.lay.info[r as usize].written {
                stack.push((r, 0.0, 0.0, false));
            }
        }
        for &c in &dirty {
            let ci = self.lay.info[c as usize];
            if !ci.live || !(ci.needs || ci.tdirty) {
                continue;
            }
            let (mut px, mut py, mut p) = (0.0f32, 0.0f32, ci.parent);
            while p != NONE {
                let (x, y, _, _) = self.lay.eng.rect(p);
                px += x;
                py += y;
                p = self.lay.info[p as usize].parent;
            }
            stack.push((c, px, py, false));
        }
        dirty.clear();
        self.lay.dirty = dirty;
        while let Some((n, px, py, force)) = stack.pop() {
            let li = self.lay.info[n as usize];
            if !(force || li.needs || li.desc || !li.written || li.tdirty) {
                continue;
            }
            self.lay.stats.nodes_visited += 1;
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
                // only what moved (a moved parent moves everything below it) or is flagged is visited
                let ci = &self.lay.info[c as usize];
                if fc || ci.needs || ci.desc || !ci.written || ci.tdirty {
                    stack.push((c, ax, ay, fc));
                }
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
        let (inst, margin, hd) = (self.runs[n.run as usize].first, n.margin, n.handle);
        let p = self.inst_mut(inst);
        p.pos = [x, y];
        p.size = [w, h];
        // the old position's pixels (a sub-pixel move can keep the rounded ink), then old and new ink
        self.bough.touch_handle(hd);
        let ink = self.ink_rect(li.space, x, y, w, h, margin);
        self.bough.set_ink_handle(hd, ink);
    }

    /// Place a laid-out text node's lines in its box at (x, y) of width `w`: the fast path points one run
    /// header per wrapped line at the leaf's glyph-pool block (no glyph is written); the slow path writes
    /// glyph instances. Damage: the old and the new ink.
    fn place_text(&mut self, lid: u32, x: f32, y: f32, w: f32, _h: f32, _reflow: bool) {
        self.lay.stats.text_placed += 1;
        let li = self.lay.info[lid as usize];
        let node = li.rtn;
        let ti = self.nodes[node as usize].text;
        let hd = self.nodes[node as usize].handle;
        let (size_px, lh) = {
            let t = &self.texts[ti as usize];
            (t.size_px, t.line_h)
        };
        let base_off = self.text.baseline(size_px, lh);
        let ctx = self.lay.eng.ctx_mut(lid).expect("text leaf has a context");
        let k = ctx.k().max(1e-6);
        let lines = if ctx.wraps() { ctx.wrap_at(((w + 1e-3) / k).floor() as i32) } else { ctx.wrap_at(i32::MAX) };
        let n = ctx.text.len() as u32;
        let block = ctx.block;
        let spans: Vec<(u32, u32)> = (0..lines).map(|i| (ctx.starts[i], if i + 1 < lines { ctx.starts[i + 1] } else { n })).collect();
        let slow_text = if block.is_none() { Some(String::from_utf8_lossy(&ctx.text).into_owned()) } else { None };
        self.lay.prof.reset();
        let old_ink = self.texts[ti as usize].ink;
        {
            let t = &mut self.texts[ti as usize];
            t.ox = x;
            t.oy = (y + base_off).round();
        }
        match block {
            Some(b) => {
                if b.exotic {
                    let strike = self.texts[ti as usize].strike as usize;
                    let ents = self.glyphs.ent[b.base as usize..(b.base + b.len) as usize].to_vec();
                    self.text.ready(strike, &ents);
                }
                if !matches!(self.texts[ti as usize].shape, TShape::Lines { .. }) {
                    let old = std::mem::take(&mut self.texts[ti as usize].shape);
                    crate::text::release(self, TextNode { shape: old, ..Default::default() });
                    let (first, cap) = self.glyphs.alloc_run_range(lines.max(1));
                    self.texts[ti as usize].shape = TShape::Lines { block: None, owned: false, first, cap, n: 0, widths: Vec::new(), spans: Vec::new() };
                    self.bough.set_payload_handle(hd, pay_runs(first, cap));
                }
                self.lines_point(ti, b, &spans, false);
            }
            None => {
                // the general shaper's lines as glyph instances, one line per wrap line
                let old = std::mem::take(&mut self.texts[ti as usize].shape);
                crate::text::release(self, TextNode { shape: old, ..Default::default() });
                let text = slow_text.unwrap();
                let (strike, color, space, oy) = {
                    let t = &self.texts[ti as usize];
                    (t.strike as usize, t.color, t.space, t.oy)
                };
                self.text.scratch.clear();
                for (i, &(a, e)) in spans.iter().enumerate() {
                    let line = text.get(a as usize..e as usize).unwrap_or("");
                    self.text.line(strike, size_px, line, x, (oy + i as f32 * lh).round(), color);
                }
                let cnt = self.text.scratch.len();
                let run = self.alloc_run(cnt.max(1));
                self.runs[run as usize].len = cnt as u32;
                let first = self.runs[run as usize].first;
                let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
                for i in 0..cnt.max(1) {
                    let ins = match self.text.scratch.get(i) {
                        Some(g) => {
                            x0 = x0.min(g.xy[0]);
                            y0 = y0.min(g.xy[1]);
                            x1 = x1.max(g.xy[0] + g.wh[0]);
                            y1 = y1.max(g.xy[1] + g.wh[1]);
                            Instance { pos: g.xy, size: g.wh, color: g.color, uv: g.uv, space, kind: KIND_GLYPH, ..Default::default() }
                        }
                        None => Instance { space, kind: KIND_GLYPH, ..Default::default() },
                    };
                    self.inst[(first + i as u32) as usize] = ins;
                }
                let ink = if cnt == 0 { NRect::EMPTY } else { self.ink_rect(space, x0, y0, x1 - x0, y1 - y0, 1.0) };
                let t = &mut self.texts[ti as usize];
                t.ink = ink;
                t.shape = TShape::Glyphs { run };
                self.nodes[node as usize].run = run;
                self.bough.set_payload_handle(hd, pay_irun(first, cnt.max(1) as u32));
            }
        }
        self.lay.prof.mark(LP_TEXT_EMIT);
        let ink = self.texts[ti as usize].ink;
        self.bough.damage_rect(old_ink);
        self.bough.set_ink_handle(hd, ink);
        self.lay.prof.mark(LP_TEXT_COPY);
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
    Bg: IntoProp<Color, V>,
    Ch: Element<V>,
{
    fn mount(self, m: &mut Mounter<'_, V>) {
        let s = m.rt.scale;
        let hd = m.rt.bough.new_handle();
        let lid = m.rt.lay.eng.add(hd, &self.spec, s, None);
        let parent = attach(m, lid);
        if self.draw {
            let node = m.rt.alloc_node();
            let bg = self.bg.lower::<ApplyBg>(m, node);
            let (ins, _kind, margin) = quad_instance(bg, self.radius * s, self.border, s, m.space);
            let run = m.rt.alloc_run(1);
            m.rt.runs[run as usize].len = 1;
            let inst = m.rt.runs[run as usize].first;
            *m.rt.inst_mut(inst) = ins;
            {
                let n = &mut m.rt.nodes[node as usize];
                n.handle = hd;
                n.run = run;
                n.space = m.space;
                n.clip = m.clip;
                n.kind = if self.border.is_some() { NK_BORDER } else { NK_QUAD };
                n.color = bg.0;
                n.margin = margin;
                n.lnode = lid;
            }
            m.scope.nodes.push(node);
            m.scope.runs.push(run);
            m.rt.lay.info[lid as usize].rtn = node;
            m.rt.lay.info[lid as usize].kind = 1;
            m.recs.push(RecordInit { depth: m.depth, ink: NRect::EMPTY, kind: REC_QUAD, payload: inst, handle: hd.idx, clip: m.clip });
        } else {
            // a paint-less box still has a record: the layout structure IS the Bough's depth column
            m.recs.push(RecordInit { depth: m.depth, ink: NRect::EMPTY, kind: REC_BOX, payload: 0, handle: hd.idx, clip: m.clip });
        }
        m.depth += 1;
        m.rt.lay.cur = lid;
        self.ch.mount(m);
        m.rt.lay.cur = parent;
        m.depth -= 1;
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

impl<V: 'static, S: TextSrc<V>> Element<V> for Para<S> {
    fn mount(self, m: &mut Mounter<'_, V>) {
        let s = m.rt.scale;
        let node = m.rt.alloc_node();
        m.pending_num = None;
        let init = self.s.lower(m, node);
        let num = m.pending_num.take();
        let size_px = self.size * s;
        let lh_px = self.lh * s;
        let strike = m.rt.text.strike(size_px);
        let mut ctx = LeafCtx::new(size_px, lh_px, self.wrap);
        m.rt.lay.prof.reset();
        {
            let Rt { text, glyphs, .. } = &mut *m.rt;
            ctx.set_text(text, glyphs, &init);
        }
        m.rt.lay.prof.mark(LP_TEXT_SET);
        let hd = m.rt.bough.new_handle();
        let lid = m.rt.lay.eng.add(hd, &self.spec, s, Some(ctx));
        let _parent = attach(m, lid);
        let ti = m.rt.alloc_text(TextNode {
            text: init,
            strike: strike as u32,
            size_px,
            line_h: lh_px,
            color: self.color.0,
            ox: 0.0,
            oy: 0.0,
            space: m.space,
            handle: hd,
            ink: NRect::EMPTY,
            shape: TShape::None,
            per_glyph: false,
            num,
        });
        {
            let n = &mut m.rt.nodes[node as usize];
            n.text = ti;
            n.space = m.space;
            n.clip = m.clip;
            n.kind = NK_LTEXT;
            n.color = self.color.0;
            n.size_px = size_px;
            n.line_h = lh_px;
            n.handle = hd;
            n.lnode = lid;
        }
        m.rt.lay.info[lid as usize].rtn = node;
        m.rt.lay.info[lid as usize].kind = 2;
        m.scope.nodes.push(node);
        // provisional: empty ink (never drawn) until the first write-back points the lines at the block
        m.recs.push(RecordInit { depth: m.depth, ink: NRect::EMPTY, kind: REC_TEXT, payload: 0, handle: hd.idx, clip: m.clip });
    }
}

pub mod prelude {
    pub use super::{block, column, label, para, row, Align, Justify, Styled};
}
