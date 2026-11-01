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

pub const NONE: u32 = u32::MAX;

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

// =============================================================================== measured text

/// A measured text leaf's state: the shaped run (font units), its bytes and the last wrap.
#[derive(Clone, Default)]
pub struct LeafCtx {
    pub run: GlyphRun,
    pub text: Vec<u8>,
    pub starts: Vec<u32>,
    /// `max_units` of the wrap in `starts` (`i32::MIN`: none) and its widest line, in font units.
    pub wrapped_at: i32,
    pub widest: i32,
    /// Longest word, font units (`-1`: not computed).
    pub min_word: i32,
    pub wrap: bool,
    pub ascii: bool,
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
        ts.shape_run(text, self.size_px, &mut self.run);
        self.wrapped_at = i32::MIN;
        self.min_word = -1;
    }

    #[inline]
    fn k(&self) -> f32 {
        self.size_px / self.run.upem.max(1) as f32
    }

    /// Wrap at `max_units`; returns the line count (`starts` holds the first glyph of each line).
    pub fn wrap_at(&mut self, max_units: i32) -> usize {
        if self.wrapped_at == max_units && !self.starts.is_empty() {
            return self.starts.len();
        }
        self.wrapped_at = max_units;
        let n = self.run.len();
        if n == 0 {
            self.starts.clear();
            self.starts.push(0);
            self.widest = 0;
            return 1;
        }
        if !self.wrap || !self.ascii {
            self.starts.clear();
            self.starts.push(0);
            self.widest = self.run.width;
            return 1;
        }
        let lines = wrap_greedy(&self.run, &self.text, max_units, &mut self.starts);
        let mut widest = 0;
        for i in 0..lines {
            let a = self.starts[i] as usize;
            let mut b = if i + 1 < lines { self.starts[i + 1] as usize } else { n };
            while b > a + 1 && self.text[b - 1] == b' ' {
                b -= 1;
            }
            let right = self.run.x[b - 1] + self.run.advance(b - 1);
            widest = widest.max(right - self.run.x[a]);
        }
        self.widest = widest;
        lines
    }

    fn longest_word(&mut self) -> i32 {
        if self.min_word >= 0 {
            return self.min_word;
        }
        let n = self.run.len();
        let mut best = 0;
        let mut i = 0;
        while i < n {
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
        self.min_word = best;
        best
    }

    /// cephal's measure callback: content-box size of the text under `input`.
    pub fn measure(&mut self, input: MeasureInput) -> MeasureOutput {
        let k = self.k().max(1e-6);
        let lh = self.lh_px;
        if !self.wrap || !self.ascii {
            let w = self.run.width as f32 * k;
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
            None => {
                let l = self.wrap_at(i32::MAX);
                (l, self.widest)
            }
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

pub struct Lay {
    pub eng: Eng,
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
    pub fn child(&self, n: u32, i: usize) -> u32 {
        let mut v = Vec::new();
        self.eng.children_into(n, &mut v);
        v[i]
    }
    pub fn child_rect(&self, n: u32, i: usize) -> (f32, f32, f32, f32) {
        self.eng.rect(self.child(n, i))
    }
}

impl Rt {
    /// Release a group: its layout nodes (the drawables are released with the scope's nodes).
    pub fn lay_remove_root(&mut self, root: u32) {
        let mut stack = std::mem::take(&mut self.lay.kids);
        stack.clear();
        stack.push(root);
        let mut all: Vec<u32> = Vec::new();
        while let Some(n) = stack.pop() {
            all.push(n);
            self.lay.eng.children_into(n, &mut stack);
        }
        self.lay.eng.remove_tree(root);
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
        self.lay.eng.invalidate(lid);
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
        let mut pend = std::mem::take(&mut self.lay.pending);
        for &r in &pend {
            let li = self.lay.info[r as usize];
            let avail = Size {
                width: AvailableSpace::Definite(li.avail_w),
                height: if li.avail_h.is_nan() { AvailableSpace::MaxContent } else { AvailableSpace::Definite(li.avail_h) },
            };
            self.lay.eng.layout(r, avail);
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
    }

    /// Write the laid-out groups back into instances and ink.
    pub fn lay_writeback_ready(&mut self) {
        let mut ready = std::mem::take(&mut self.lay.ready);
        for &r in &ready {
            self.lay_writeback(r);
        }
        ready.clear();
        self.lay.ready = ready;
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
            self.lay.eng.children_into(n, &mut kids);
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
        let Rt { lay, text, .. } = self;
        let ctx = lay.eng.ctx_mut(lid).expect("text leaf has a context");
        let k = ctx.k().max(1e-6);
        let _ = reflow;
        let lines = if ctx.wrap && ctx.ascii {
            ctx.wrap_at(((w + 1e-3) / k).floor() as i32)
        } else {
            ctx.wrap_at(i32::MAX)
        };
        text.scratch.clear();
        let n = ctx.run.len();
        for i in 0..lines {
            let a = ctx.starts[i] as usize;
            let b = if i + 1 < lines { ctx.starts[i + 1] as usize } else { n };
            if b <= a {
                continue;
            }
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
            text.cache.instances(strike, line, x, baseline, color, &mut text.scratch);
        }
        let cnt = text.scratch.len();
        if cnt > (1usize << self.runs[run_id as usize].class) {
            self.regrow_run(run_id, cnt);
        }
        let first = self.runs[run_id as usize].first;
        self.runs[run_id as usize].len = cnt as u32;
        let dst = self.inst_ptr(first);
        let src = self.text.scratch.as_ptr();
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        let clipw = clip as u32;
        for i in 0..cnt {
            let mut g: Instance = unsafe { *src.add(i) };
            x0 = x0.min(g.xy[0]);
            y0 = y0.min(g.xy[1]);
            x1 = x1.max(g.xy[0] + g.wh[0]);
            y1 = y1.max(g.xy[1] + g.wh[1]);
            g.space = space;
            g.aux[3] = clipw;
            unsafe { dst.add(i).write(g) };
        }
        let ink = if cnt == 0 { NRect::EMPTY } else { self.ink_rect(space, x0, y0, x1 - x0, y1 - y0, 1.0) };
        let hd = self.nodes[li.rtn as usize].handle;
        self.bough.set_ink_handle(hd, ink);
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
        let style = self.spec.to_style(s);
        let lid = m.rt.lay.eng.add(&style, None);
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
            let hd = m.rt.bough.new_handle();
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
        ctx.set_text(&mut m.rt.text, &init);
        let style = self.spec.to_style(s);
        let lid = m.rt.lay.eng.add(&style, Some(ctx));
        let _parent = attach(m, lid);
        let run = m.rt.alloc_run(init.len().max(4));
        let hd = m.rt.bough.new_handle();
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
